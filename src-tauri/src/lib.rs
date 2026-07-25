//! AudioMirror: duplicates one audio output to as many devices as wanted.

pub mod audio;
pub mod ipc;
pub mod settings;
pub mod tray;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use audio::engine::{Command, EngineHandle, Event};
use ipc::AppState;
use settings::{Persister, Settings, Store};

/// Passed by the autostart registration, and honoured on a manual launch too.
const MINIMIZED_FLAG: &str = "--minimized";

pub fn run() {
    tauri::Builder::default()
        // Registered first so a second launch is caught before it can touch a
        // device already held by the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::reveal(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_FLAG]),
        ))
        .setup(setup)
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            ipc::bootstrap,
            ipc::set_enabled,
            ipc::select_source,
            ipc::add_target,
            ipc::remove_target,
            ipc::set_target_enabled,
            ipc::set_target_gain,
            ipc::set_latency,
            ipc::rescan,
            ipc::set_watching,
            ipc::set_preferences,
            ipc::hide_window,
            ipc::quit,
        ])
        .build(tauri::generate_context!())
        .expect("AudioMirror failed to start")
        .run(|app, event| {
            // Closing the window leaves the mirror running: the process only
            // ends when the user says so.
            if let RunEvent::ExitRequested { api, .. } = event {
                let leaving = app
                    .try_state::<AppState>()
                    .is_some_and(|state| state.quitting.load(Ordering::SeqCst));
                if !leaving {
                    api.prevent_exit();
                }
            }
        });
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let user_directory = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));

    let store = Store::locate(user_directory);
    let settings = store.load();
    let portable = store.is_portable();
    let settings_path = store.path().to_path_buf();

    let persister = Arc::new(Persister::spawn(store));
    let preferences = Arc::new(Mutex::new(settings.preferences.clone()));

    let emitter = app.handle().clone();
    let observed_persister = Arc::clone(&persister);
    let observed_preferences = Arc::clone(&preferences);

    let (engine, _engine_thread) = EngineHandle::spawn(settings.mirror.clone(), move |event| {
        match event {
            Event::Status(status) => {
                let _ = emitter.emit("state", status);
            }
            Event::Config(mirror) => {
                let _ = emitter.emit("config", &mirror);
                observed_persister.submit(Settings {
                    version: 0,
                    mirror,
                    preferences: observed_preferences.lock().clone(),
                });
            }
            Event::Catalog(catalog) => {
                let _ = emitter.emit("catalog", catalog);
            }
        }
    });

    app.manage(AppState {
        engine,
        persister,
        preferences,
        settings_path,
        portable,
        quitting: AtomicBool::new(false),
        tray_toggle: Mutex::new(None),
    });

    tray::install(app.handle(), settings.mirror.enabled)?;

    let start_hidden = settings.preferences.start_minimized
        || std::env::args().any(|argument| argument == MINIMIZED_FLAG);

    if start_hidden {
        // The window is declared hidden, so there is nothing to hide here; only
        // the metering needs to be told nobody is watching.
        app.state::<AppState>()
            .engine
            .send(Command::SetTelemetry(false));
    } else if let Some(window) = app.get_webview_window("main") {
        window.show()?;
    }

    Ok(())
}

fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let Some(state) = window.app_handle().try_state::<AppState>() else {
        return;
    };

    match event {
        WindowEvent::CloseRequested { api, .. } => {
            let to_tray =
                state.preferences.lock().close_to_tray && !state.quitting.load(Ordering::SeqCst);

            if to_tray {
                api.prevent_close();
                let _ = window.hide();
                state.engine.send(Command::SetTelemetry(false));
            } else {
                state.quitting.store(true, Ordering::SeqCst);
                state.persist();
            }
        }

        WindowEvent::Destroyed => {
            state.persist();
        }

        _ => {}
    }
}
