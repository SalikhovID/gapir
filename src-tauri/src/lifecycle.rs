//! Keeps the app alive in the background: menu bar item, main window, and the Cmd+Q override.

use crate::{updater, Shared};
use gapir_core::engine::View;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{
    Menu, MenuBuilder, MenuEvent, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{
    include_image, ActivationPolicy, AppHandle, Manager, RunEvent, WebviewUrl,
    WebviewWindowBuilder, Window, WindowEvent,
};

const MAIN: &str = "main";

const ICON_IDLE: Image<'static> = include_image!("icons/tray-idle.png");
const ICON_RECORDING: Image<'static> = include_image!("icons/tray-recording.png");
const ICON_BUSY: Image<'static> = include_image!("icons/tray-busy.png");

/// Handles of the tray menu entries that change at runtime.
pub struct Tray {
    icon: TrayIcon,
    status: MenuItem<tauri::Wry>,
    copy_last: MenuItem<tauri::Wry>,
    update: MenuItem<tauri::Wry>,
    /// Text the update entry was built with, used to restore it.
    no_access: Mutex<bool>,
}

/// Cmd+Q is a custom item, not the predefined Quit: the predefined one calls `terminate:`
/// directly and would end the process, which is exactly what must not happen.
pub fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let hide = MenuItemBuilder::with_id("hide", "Gapirni yashirish")
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;
    let close = MenuItemBuilder::with_id("hide-window", "Oynani yopish")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;
    let gapir = SubmenuBuilder::new(app, "Gapir")
        .item(&hide)
        .item(&close)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .build()?;
    MenuBuilder::new(app).item(&gapir).item(&edit).build()
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItemBuilder::with_id("status", "Tayyor")
        .enabled(false)
        .build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Sozlamalar").build(app)?;
    let copy_last = MenuItemBuilder::with_id("copy-last", "Oxirgi matnni nusxalash")
        .enabled(false)
        .build(app)?;
    let update = MenuItemBuilder::with_id("update", "Yangilash")
        .enabled(false)
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Butunlay chiqish").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .item(&settings)
        .item(&copy_last)
        .item(&update)
        .separator()
        .item(&quit)
        .build()?;
    let icon = TrayIconBuilder::with_id("main")
        .icon(ICON_IDLE)
        .icon_as_template(true)
        .tooltip("Gapir")
        .menu(&menu)
        .build(app)?;
    let _ = update.set_text("Yangilanish yoʻq");
    app.manage(Tray {
        icon,
        status,
        copy_last,
        update,
        no_access: Mutex::new(false),
    });
    Ok(())
}

pub fn set_tray_view(app: &AppHandle, view: View) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let tray = handle.state::<Tray>();
        let (icon, template, text) = match view {
            View::Hidden => (ICON_IDLE, true, "Tayyor"),
            View::Starting | View::Recording { .. } => (ICON_RECORDING, false, "Yozilmoqda"),
            View::Transcribing { .. } => (ICON_BUSY, true, "Matnga oʻgirilmoqda"),
        };
        let _ = tray.icon.set_icon(Some(icon));
        let _ = tray.icon.set_icon_as_template(template);
        if !*tray.no_access.lock().expect("tray lock") {
            let _ = tray.status.set_text(text);
        }
    });
}

/// Shows a warning in the menu while Accessibility is missing, because hotkeys cannot work then.
pub fn set_access(app: &AppHandle, granted: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let tray = handle.state::<Tray>();
        *tray.no_access.lock().expect("tray lock") = !granted;
        let text = if granted {
            "Tayyor"
        } else {
            "Accessibility ruxsati kerak"
        };
        let _ = tray.status.set_text(text);
    });
}

pub fn set_last_text_available(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = handle.state::<Tray>().copy_last.set_enabled(true);
    });
}

pub fn set_update_available(app: &AppHandle, tag: &str) {
    let handle = app.clone();
    let text = format!("Yangilash: {tag}");
    let _ = app.run_on_main_thread(move || {
        let tray = handle.state::<Tray>();
        let _ = tray.update.set_text(text);
        let _ = tray.update.set_enabled(true);
    });
}

pub fn show_main(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = handle.set_activation_policy(ActivationPolicy::Regular);
        if let Some(window) = handle.get_webview_window(MAIN) {
            let _ = window.show();
            let _ = window.set_focus();
            return;
        }
        let window = WebviewWindowBuilder::new(&handle, MAIN, WebviewUrl::App("index.html".into()))
            .title("Gapir")
            .inner_size(480.0, 640.0)
            .resizable(false)
            .maximizable(false)
            .center()
            .build();
        match window {
            Ok(window) => {
                let _ = window.set_focus();
            }
            Err(error) => log::error!("main window could not be created: {error}"),
        }
    });
}

/// The window is destroyed, not hidden: its web process is the biggest idle memory user.
fn close_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.close();
    }
    let _ = app.set_activation_policy(ActivationPolicy::Accessory);
}

pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "hide" | "hide-window" => close_main(app),
        "settings" => show_main(app),
        "copy-last" => copy_last_text(app),
        "update" => updater::run_update(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn copy_last_text(app: &AppHandle) {
    use std::io::Write;
    let shared = app.state::<Arc<Shared>>();
    let text = shared.last_text.lock().expect("last text lock").clone();
    let child = std::process::Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn();
    if let Ok(mut child) = child {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN {
        return;
    }
    if let WindowEvent::Destroyed = event {
        let _ = window
            .app_handle()
            .set_activation_policy(ActivationPolicy::Accessory);
    }
}

pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        // Fires when the last window closes. The hotkeys must outlive every window.
        RunEvent::ExitRequested {
            api, code: None, ..
        } => api.prevent_exit(),
        RunEvent::Reopen { .. } => show_main(app),
        _ => {}
    }
}
