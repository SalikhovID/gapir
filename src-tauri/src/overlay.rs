//! The small status pill. It is a non-activating panel, so it can sit above every app, including
//! full-screen ones, without ever taking focus from the text field being dictated into.

use gapir_core::engine::{Cue, Notice, View};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSSound;
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl};
#[cfg(target_os = "macos")]
use tauri_nspanel::{
    CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask, tauri_panel,
};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(OverlayPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

const LABEL: &str = "overlay";
const WIDTH: f64 = 340.0;
const HEIGHT: f64 = 64.0;
const BOTTOM_OFFSET: f64 = 28.0;
const NOTICE_TIME: Duration = Duration::from_millis(2_800);

/// A session is on screen; a notice must not hide the pill when it expires.
static SESSION_VISIBLE: AtomicBool = AtomicBool::new(false);
static NOTICE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Only the latest notice may hide the pill.
static NOTICE_GENERATION: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
pub fn create(app: &AppHandle) {
    let panel = PanelBuilder::<_, OverlayPanel>::new(app, LABEL)
        .url(WebviewUrl::App("overlay.html".into()))
        .title("Gapir")
        .position(tauri::Position::Logical(LogicalPosition { x: 0.0, y: 0.0 }))
        .level(PanelLevel::Status)
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: WIDTH,
            height: HEIGHT,
        }))
        .has_shadow(false)
        .transparent(true)
        .no_activate(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|window| window.decorations(false).transparent(true).focusable(false))
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary(),
        )
        .build();
    match panel {
        Ok(panel) => {
            panel.hide();
            if let Some(window) = app.get_webview_window(LABEL) {
                let _ = window.set_ignore_cursor_events(true);
            }
        }
        Err(error) => log::error!("overlay could not be created: {error}"),
    }
}

/// A borderless, never-focused, always-on-top window plays the panel's part on Windows and Linux.
#[cfg(not(target_os = "macos"))]
pub fn create(app: &AppHandle) {
    let window =
        tauri::WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
            .title("Gapir")
            .inner_size(WIDTH, HEIGHT)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focusable(false)
            .visible(false)
            .build();
    if let Err(error) = window {
        log::error!("overlay could not be created: {error}");
    }
}

/// Moves the pill to the bottom centre of the screen the mouse is on.
fn place(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let cursor = app.cursor_position().ok();
    let monitors = app.available_monitors().unwrap_or_default();
    let under_cursor = cursor.and_then(|point| {
        monitors.iter().find(|monitor| {
            let (origin, size) = (monitor.position(), monitor.size());
            point.x >= origin.x as f64
                && point.x < origin.x as f64 + size.width as f64
                && point.y >= origin.y as f64
                && point.y < origin.y as f64 + size.height as f64
        })
    });
    let primary = app.primary_monitor().ok().flatten();
    let Some(monitor) = under_cursor.or(primary.as_ref()) else {
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let x = area.position.x as f64 / scale + (area.size.width as f64 / scale - WIDTH) / 2.0;
    let bottom = (area.position.y as f64 + area.size.height as f64) / scale;
    let _ = window.set_position(LogicalPosition {
        x,
        y: bottom - HEIGHT - BOTTOM_OFFSET,
    });
}

#[cfg(target_os = "macos")]
fn set_visible(app: &AppHandle, visible: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Ok(panel) = handle.get_webview_panel(LABEL) else {
            return;
        };
        if visible {
            place(&handle);
            panel.order_front_regardless();
        } else {
            panel.hide();
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn set_visible(app: &AppHandle, visible: bool) {
    // Wayland compositors ignore `focusable(false)`: the shown pill takes focus and the paste lands
    // in it. shortcut: no pill on Wayland, upgrade with a layer-shell surface.
    if visible && cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            return;
        };
        if visible {
            place(&handle);
            let _ = window.show();
            // Only a shown window has a GDK window to clip; before that GTK has none and tao panics.
            let _ = window.set_ignore_cursor_events(true);
        } else {
            let _ = window.hide();
        }
    });
}

fn emit(app: &AppHandle, payload: serde_json::Value) {
    if let Err(error) = app.emit_to(LABEL, "overlay", payload) {
        log::debug!("overlay event: {error}");
    }
}

pub fn show_view(app: &AppHandle, view: View) {
    let payload = match view {
        View::Hidden => json!({ "view": "hidden" }),
        View::Starting => json!({ "view": "starting" }),
        View::Recording { elapsed_s, waiting } => {
            json!({ "view": "recording", "elapsed": elapsed_s, "waiting": waiting })
        }
        View::Transcribing { waiting } => json!({ "view": "transcribing", "waiting": waiting }),
    };
    emit(app, payload);

    let visible = view != View::Hidden;
    let was_visible = SESSION_VISIBLE.swap(visible, Ordering::SeqCst);
    if visible && !was_visible {
        set_visible(app, true);
    }
    // A notice raised at the end of the session keeps the pill until it expires.
    if !visible && !NOTICE_ACTIVE.load(Ordering::SeqCst) {
        set_visible(app, false);
    }
}

fn notice_text(notice: Notice) -> &'static str {
    match notice {
        Notice::StillTranscribing => "Matnga oʻgirilmoqda, biroz kuting",
        Notice::SilenceStopped => "Jimlik sababli yozuv tugadi",
        Notice::LimitReached => "60 daqiqa limiti tugadi",
        Notice::MicLost => "Mikrofon ishlamadi",
        Notice::NoSpeech => "Ovoz eshitilmadi",
        Notice::TextInMenu => "Matn menyuda: Oxirgi matnni nusxalash",
        Notice::RemovalRefused => "Matn oʻchirilmadi, kursor oʻzgargan",
        Notice::ChunkFailed => "Bir boʻlak matnga oʻgirilmadi",
    }
}

pub fn notify(app: &AppHandle, notice: Notice) {
    emit(app, json!({ "notice": notice_text(notice) }));
    NOTICE_ACTIVE.store(true, Ordering::SeqCst);
    set_visible(app, true);

    let generation = NOTICE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(NOTICE_TIME);
        if NOTICE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        NOTICE_ACTIVE.store(false, Ordering::SeqCst);
        emit(&handle, json!({ "notice": null }));
        if !SESSION_VISIBLE.load(Ordering::SeqCst) {
            set_visible(&handle, false);
        }
    });
}

pub fn level(app: &AppHandle, value: f32) {
    emit(app, json!({ "level": value }));
}

#[cfg(target_os = "macos")]
pub fn play_cue(app: &AppHandle, cue: Cue) {
    let name = match cue {
        Cue::Start => "Tink",
        Cue::Stop => "Pop",
    };
    let _ = app.run_on_main_thread(move || {
        if let Some(sound) = NSSound::soundNamed(&NSString::from_str(name)) {
            sound.play();
        }
    });
}

#[cfg(windows)]
pub fn play_cue(_app: &AppHandle, cue: Cue) {
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONASTERISK, MB_OK};
    let sound = match cue {
        Cue::Start => MB_OK,
        Cue::Stop => MB_ICONASTERISK,
    };
    // SAFETY: plays a system sound asynchronously; no pointers involved.
    unsafe { MessageBeep(sound) };
}

/// Freedesktop theme sounds through PulseAudio, which recording needs anyway.
#[cfg(target_os = "linux")]
pub fn play_cue(_app: &AppHandle, cue: Cue) {
    let name = match cue {
        Cue::Start => "message",
        Cue::Stop => "complete",
    };
    // A thread waits for the player, so no zombie process is left behind.
    std::thread::spawn(move || {
        let _ = std::process::Command::new("paplay")
            .arg(format!("/usr/share/sounds/freedesktop/stereo/{name}.oga"))
            .status();
    });
}
