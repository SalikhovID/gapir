//! Gapir desktop app: system-wide Uzbek dictation that runs entirely on this computer.

mod commands;
mod desktop;
mod hotkeys;
mod lifecycle;
mod output;
mod overlay;
mod permissions;
mod recorder;
mod stt;
mod updater;

use desktop::Desktop;
use gapir_core::chunker::EarshotVad;
use gapir_core::engine::{Engine, Input};
use gapir_core::model_store;
use gapir_core::settings::Settings;
use hotkeys::Keys;
use recorder::Mic;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use stt::{Stt, MODEL};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

const TICK: Duration = Duration::from_millis(250);
/// Audio arrives in 30 ms chunks; the pill redraws about 15 times per second.
const LEVEL_EVERY: u32 = 2;

/// State shared between the engine thread and window commands.
pub struct Shared {
    pub settings_path: PathBuf,
    pub models_dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub last_text: Mutex<String>,
    pub keys: Arc<Keys>,
    pub downloading: AtomicBool,
}

impl Shared {
    pub fn settings(&self) -> Settings {
        self.settings.lock().expect("settings lock").clone()
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn spawn_engine(
    app: AppHandle,
    shared: Arc<Shared>,
    input: Sender<Input>,
    receiver: Receiver<Input>,
) {
    std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            let mic = Mic::new(input.clone());
            let broken_app = app.clone();
            let broken_shared = shared.clone();
            let stt = Stt::new(shared.models_dir.clone(), input, move || {
                on_model_broken(&broken_app, &broken_shared);
            });
            let platform = Desktop::new(app.clone(), shared, mic, stt);
            let mut engine = Engine::new(platform, EarshotVad::new);
            let mut last_tick = Instant::now();
            let mut audio_chunks = 0u32;
            loop {
                match receiver.recv_timeout(TICK) {
                    Ok(message) => {
                        if let Input::Audio(samples) = &message {
                            audio_chunks += 1;
                            if audio_chunks.is_multiple_of(LEVEL_EVERY) {
                                overlay::level(&app, rms(samples));
                            }
                        }
                        engine.handle(message, Instant::now());
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                if last_tick.elapsed() >= TICK {
                    last_tick = Instant::now();
                    engine.handle(Input::Tick, last_tick);
                }
            }
        })
        .expect("failed to start the engine thread");
}

/// A model that will not load is almost always a damaged file: drop it and offer a new download.
fn on_model_broken(app: &AppHandle, shared: &Shared) {
    if let Ok(false) = model_store::verify(&shared.models_dir, &MODEL) {
        log::error!("model file is damaged, removing it");
        let _ = std::fs::remove_file(model_store::model_path(&shared.models_dir, &MODEL));
    }
    lifecycle::show_main(app);
}

fn log_folder() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Library/Logs/Gapir")
}

pub fn run() {
    let hidden = std::env::args().any(|argument| argument == "--hidden");
    let log_plugin = tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::Folder {
            path: log_folder(),
            file_name: Some("gapir".into()),
        }))
        .level(log::LevelFilter::Info)
        .max_file_size(1_000_000)
        .rotation_strategy(RotationStrategy::KeepOne)
        .build();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            lifecycle::show_main(app);
        }))
        .plugin(log_plugin)
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::list_mics,
            commands::request_permission,
            commands::open_keyboard_settings,
            commands::pause_hotkeys,
            commands::save_settings,
            commands::download_model,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            let settings_path = data_dir.join("settings.json");
            let first_run = !settings_path.exists();
            let settings = Settings::load(&settings_path);
            if first_run {
                settings.save(&settings_path)?;
            }
            let keys = Keys::new(&settings);
            let shared = Arc::new(Shared {
                settings_path,
                models_dir: data_dir.join("models"),
                settings: Mutex::new(settings.clone()),
                last_text: Mutex::new(String::new()),
                keys: keys.clone(),
                downloading: AtomicBool::new(false),
            });
            app.manage(shared.clone());

            app.set_menu(lifecycle::app_menu(&handle)?)?;
            lifecycle::create_tray(&handle)?;
            overlay::create(&handle);
            commands::apply_autostart(&handle, settings.autostart);

            let (input, receiver) = mpsc::channel();
            let access_app = handle.clone();
            hotkeys::spawn(keys, input.clone(), move |granted| {
                lifecycle::set_access(&access_app, granted);
            });
            spawn_engine(handle.clone(), shared.clone(), input, receiver);

            let update_shared = shared.clone();
            updater::spawn(handle.clone(), move || {
                update_shared.settings().update_check
            });

            let setup_done = permissions::accessibility()
                && permissions::mic_status() == "granted"
                && model_store::is_present(&shared.models_dir, &MODEL);
            if !hidden || !setup_done {
                lifecycle::show_main(&handle);
            }
            Ok(())
        })
        .on_menu_event(lifecycle::on_menu_event)
        .on_window_event(lifecycle::on_window_event)
        .build(tauri::generate_context!())
        .expect("failed to build Gapir");
    app.run(lifecycle::on_run_event);
}
