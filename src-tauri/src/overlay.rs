//! The small status pill. It is a non-activating panel, so it can sit above every app, including
//! full-screen ones, without ever taking focus from the text field being dictated into.

use gapir_core::engine::{Cue, Notice, View};
use objc2_app_kit::NSSound;
use objc2_foundation::NSString;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl};
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};

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
/// Only the latest notice may hide the pill.
static NOTICE_GENERATION: AtomicU64 = AtomicU64::new(0);

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
    SESSION_VISIBLE.store(visible, Ordering::SeqCst);
    if visible {
        set_visible(app, true);
    } else {
        // A notice raised at the end of the session keeps the pill until it expires.
        let notice_active = NOTICE_ACTIVE.load(Ordering::SeqCst);
        if !notice_active {
            set_visible(app, false);
        }
    }
}

static NOTICE_ACTIVE: AtomicBool = AtomicBool::new(false);

fn notice_text(notice: Notice) -> &'static str {
    match notice {
        Notice::StillTranscribing => "Matnga oʻgirilmoqda, biroz kuting",
        Notice::SilenceStopped => "Jimlik sababli yozuv tugadi",
        Notice::LimitReached => "60 daqiqa limiti tugadi",
        Notice::MicLost => "Mikrofon ishlamadi",
        Notice::NoSpeech => "Ovoz eshitilmadi",
        Notice::TextInMenu => "Matn menu'da: Oxirgi matnni nusxalash",
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
