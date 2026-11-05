//! Commands called from the setup and settings window.

use crate::stt::MODEL;
use crate::{permissions, Shared};
use gapir_core::model_store;
use gapir_core::settings::Settings;
use serde::Serialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

#[derive(Serialize)]
pub struct MicInfo {
    id: String,
    name: String,
}

#[derive(Serialize)]
pub struct Status {
    mic: &'static str,
    accessibility: bool,
    model_present: bool,
    model_size: u64,
    downloading: bool,
    version: &'static str,
    settings: Settings,
}

#[tauri::command]
pub fn get_status(shared: State<'_, Arc<Shared>>) -> Status {
    Status {
        mic: permissions::mic_status(),
        accessibility: permissions::accessibility(),
        model_present: model_store::is_present(&shared.models_dir, &MODEL),
        model_size: MODEL.size,
        downloading: shared.downloading.load(Ordering::SeqCst),
        version: env!("CARGO_PKG_VERSION"),
        settings: shared.settings(),
    }
}

#[tauri::command]
pub fn list_mics() -> Vec<MicInfo> {
    handy_recorder::list_input_devices()
        .unwrap_or_default()
        .into_iter()
        .map(|device| MicInfo {
            id: device.id,
            name: device.name,
        })
        .collect()
}

#[tauri::command]
pub fn request_permission(kind: String) {
    match kind.as_str() {
        "mic" if permissions::mic_status() == "unknown" => permissions::request_mic(),
        "mic" => permissions::open_pane("mic"),
        "accessibility" => {
            permissions::request_accessibility();
            permissions::open_pane("accessibility");
        }
        _ => {}
    }
}

#[tauri::command]
pub fn pause_hotkeys(shared: State<'_, Arc<Shared>>, paused: bool) {
    shared.keys.pause(paused);
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
    settings: Settings,
) -> Result<(), String> {
    settings
        .toggle_hotkey
        .parse::<handy_keys::Hotkey>()
        .map_err(|_| "Bu tugmalar birikmasini ishlatib boʻlmaydi".to_string())?;
    settings
        .save(&shared.settings_path)
        .map_err(|error| error.to_string())?;
    apply_autostart(&app, settings.autostart);
    shared.keys.apply_settings(&settings);
    *shared.settings.lock().expect("settings lock") = settings;
    Ok(())
}

pub fn apply_autostart(app: &AppHandle, enabled: bool) {
    let autostart = app.autolaunch();
    let current = autostart.is_enabled().unwrap_or(false);
    let result = match (enabled, current) {
        (true, false) => autostart.enable(),
        (false, true) => autostart.disable(),
        _ => Ok(()),
    };
    if let Err(error) = result {
        log::warn!("autostart change failed: {error}");
    }
}

#[tauri::command]
pub fn download_model(app: AppHandle, shared: State<'_, Arc<Shared>>) {
    if shared.downloading.swap(true, Ordering::SeqCst) {
        return;
    }
    let shared = shared.inner().clone();
    std::thread::spawn(move || {
        let cancel = AtomicBool::new(false);
        let mut last_event = Instant::now();
        let result = model_store::download(
            &shared.models_dir,
            &MODEL,
            |done, total| {
                if last_event.elapsed().as_millis() >= 200 {
                    last_event = Instant::now();
                    let _ = app.emit_to(
                        "main",
                        "model-progress",
                        json!({ "done": done, "total": total }),
                    );
                }
            },
            &cancel,
        );
        shared.downloading.store(false, Ordering::SeqCst);
        let error = result.err().map(|error| error.to_string());
        if let Some(reason) = &error {
            log::error!("model download failed: {reason}");
        }
        let _ = app.emit_to("main", "model-done", json!({ "error": error }));
    });
}
