//! Commands exposed to the interface.
//!
//! The frontend has no direct access to the system: every capability it needs
//! passes through this surface, which keeps the permission list down to the
//! Tauri core and makes the whole exchange auditable in one file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Manager, State, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::audio::device::DeviceCatalog;
use crate::audio::engine::{Command, EngineHandle};
use crate::audio::model::{EngineStatus, LatencyProfile, MirrorConfig};
use crate::audio::source::MAX_TARGETS;
use crate::settings::{Persister, Preferences, Settings};

pub struct AppState {
    pub engine: EngineHandle,
    pub persister: Arc<Persister>,
    pub preferences: Arc<Mutex<Preferences>>,
    pub settings_path: PathBuf,
    pub portable: bool,
    /// Set once the user really means to leave, so closing the window can be
    /// told apart from quitting.
    pub quitting: AtomicBool,
    /// Tray entry mirroring the mirror's on/off state.
    pub tray_toggle: Mutex<Option<CheckMenuItem<Wry>>>,
}

impl AppState {
    /// Writes the current configuration and preferences out.
    pub fn persist(&self) {
        self.persister.submit(Settings {
            version: 0,
            mirror: self.engine.config(),
            preferences: self.preferences.lock().clone(),
        });
    }

    /// Keeps the tray entry in step with the mirror's state.
    pub fn sync_tray(&self, enabled: bool) {
        if let Some(item) = self.tray_toggle.lock().as_ref() {
            let _ = item.set_checked(enabled);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    pub os: &'static str,
    /// True when an output can be captured without an extra driver.
    pub loopback_available: bool,
    pub portable: bool,
    pub settings_path: String,
    pub max_targets: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub catalog: DeviceCatalog,
    pub status: EngineStatus,
    pub config: MirrorConfig,
    pub preferences: Preferences,
    pub platform: Platform,
}

/// Everything the interface needs to draw itself, in one round trip.
#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Snapshot {
    Snapshot {
        catalog: state.engine.catalog(),
        status: state.engine.status(),
        config: state.engine.config(),
        preferences: state.preferences.lock().clone(),
        platform: Platform {
            os: std::env::consts::OS,
            loopback_available: crate::audio::device::supports_loopback(),
            portable: state.portable,
            settings_path: state.settings_path.display().to_string(),
            max_targets: MAX_TARGETS,
        },
    }
}

#[tauri::command]
pub fn set_enabled(enabled: bool, state: State<'_, AppState>) {
    state.engine.send(Command::SetEnabled(enabled));
    state.sync_tray(enabled);
}

#[tauri::command]
pub fn select_source(id: Option<String>, state: State<'_, AppState>) {
    state.engine.send(Command::SetSource(id));
}

#[tauri::command]
pub fn add_target(id: String, state: State<'_, AppState>) {
    state.engine.send(Command::AddTarget(id));
}

#[tauri::command]
pub fn remove_target(id: String, state: State<'_, AppState>) {
    state.engine.send(Command::RemoveTarget(id));
}

#[tauri::command]
pub fn set_target_enabled(id: String, enabled: bool, state: State<'_, AppState>) {
    state
        .engine
        .send(Command::SetTargetEnabled { id, enabled });
}

#[tauri::command]
pub fn set_target_gain(id: String, gain_db: f32, muted: bool, state: State<'_, AppState>) {
    state
        .engine
        .send(Command::SetTargetGain { id, gain_db, muted });
}

#[tauri::command]
pub fn set_latency(profile: LatencyProfile, custom_ms: u32, state: State<'_, AppState>) {
    state
        .engine
        .send(Command::SetLatency { profile, custom_ms });
}

#[tauri::command]
pub fn rescan(state: State<'_, AppState>) {
    state.engine.send(Command::Rescan);
}

/// Tells the engine whether anyone is looking at the meters. Publishing them to
/// a hidden window is the bulk of what the application would otherwise cost
/// while idle.
#[tauri::command]
pub fn set_watching(watching: bool, state: State<'_, AppState>) {
    state.engine.send(Command::SetTelemetry(watching));
}

#[tauri::command]
pub fn set_preferences(
    preferences: Preferences,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Preferences, String> {
    let previous = state.preferences.lock().autostart;

    if preferences.autostart != previous {
        let launcher = app.autolaunch();
        let outcome = if preferences.autostart {
            launcher.enable()
        } else {
            launcher.disable()
        };
        outcome.map_err(|error| error.to_string())?;
    }

    // Read the registration back rather than trust the request: the system may
    // have refused, and the interface must show what is actually in place.
    let effective = Preferences {
        autostart: app.autolaunch().is_enabled().unwrap_or(preferences.autostart),
        ..preferences
    };

    *state.preferences.lock() = effective.clone();
    state.persist();

    Ok(effective)
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn quit(app: AppHandle, state: State<'_, AppState>) {
    state.quitting.store(true, Ordering::SeqCst);
    state.persist();
    app.exit(0);
}
