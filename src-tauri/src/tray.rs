//! Tray icon and its context menu.
//!
//! The tray is the application's resting state: the window is a panel that gets
//! opened when needed, not something that has to stay around for the mirror to
//! keep running.

use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::audio::engine::Command;
use crate::ipc::AppState;

pub fn install(app: &AppHandle, enabled: bool) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Open AudioMirror").build(app)?;
    let toggle = CheckMenuItemBuilder::with_id("toggle", "Mirror active")
        .checked(enabled)
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open)
        .separator()
        .item(&toggle)
        .separator()
        .item(&quit)
        .build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("the bundled application icon");

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("AudioMirror")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => reveal(app),
            "toggle" => toggle_mirror(app),
            "quit" => leave(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left click opens the panel, right click opens the menu, which is
            // what every other tray application on the system does.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal(tray.app_handle());
            }
        })
        .build(app)?;

    if let Some(state) = app.try_state::<AppState>() {
        state.tray_toggle.lock().replace(toggle);
    }

    Ok(())
}

/// Brings the panel back to the front and resumes metering.
pub fn reveal(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();

    if let Some(state) = app.try_state::<AppState>() {
        state.engine.send(Command::SetTelemetry(true));
    }
}

fn toggle_mirror(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let enabled = !state.engine.config().enabled;
    state.engine.send(Command::SetEnabled(enabled));
    state.sync_tray(enabled);
}

fn leave(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state
            .quitting
            .store(true, std::sync::atomic::Ordering::SeqCst);
        state.persist();
    }
    app.exit(0);
}
