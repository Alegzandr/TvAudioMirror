//! Settings persistence, portable or per-user.
//!
//! A marker file next to the executable switches the application to portable
//! mode, where settings live beside the binary and the machine is left
//! untouched. Without it, settings go to the usual per-user location.
//!
//! Writes are atomic and coalesced. Atomic because a half-written file at the
//! wrong moment would cost the user their whole setup; coalesced because the
//! engine reports every change, including the small ones, and none of them is
//! worth a disk write of its own.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::audio::model::MirrorConfig;

/// Presence of this file next to the executable selects portable mode.
const PORTABLE_MARKER: &str = "AudioMirror.portable";

const FILE_NAME: &str = "AudioMirror.config.json";

/// How long further changes are awaited before writing to disk.
const COALESCE: Duration = Duration::from_millis(400);

/// Current settings format. Bumped only when a migration becomes necessary.
const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    /// Start with the window hidden, leaving only the tray icon.
    pub start_minimized: bool,
    /// Closing the window hides it instead of quitting.
    pub close_to_tray: bool,
    /// Launch with the session.
    pub autostart: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            start_minimized: false,
            close_to_tray: true,
            autostart: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub mirror: MirrorConfig,
    pub preferences: Preferences,
}

/// Where settings are read from and written to.
pub struct Store {
    path: PathBuf,
    portable: bool,
}

impl Store {
    /// Chooses the location: next to the executable in portable mode, in the
    /// per-user directory otherwise.
    pub fn locate(user_directory: PathBuf) -> Self {
        if let Some(directory) = executable_directory() {
            if directory.join(PORTABLE_MARKER).is_file() {
                return Self {
                    path: directory.join(FILE_NAME),
                    portable: true,
                };
            }
        }

        Self {
            path: user_directory.join(FILE_NAME),
            portable: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_portable(&self) -> bool {
        self.portable
    }

    /// Reads the settings, falling back to defaults.
    ///
    /// An unreadable or damaged file yields defaults rather than an error: the
    /// application must still start, and the file gets rewritten on the next
    /// change anyway.
    pub fn load(&self) -> Settings {
        let Ok(raw) = fs::read_to_string(&self.path) else {
            return Settings::default();
        };

        let mut settings: Settings = serde_json::from_str(&raw).unwrap_or_default();
        settings.version = FORMAT_VERSION;
        settings
    }

    /// Writes the settings in one atomic step.
    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut settings = settings.clone();
        settings.version = FORMAT_VERSION;
        let serialised = serde_json::to_string_pretty(&settings)?;

        // Write beside the target, then swap it in: a crash mid-write leaves
        // the previous settings intact rather than a truncated file.
        let staging = self.path.with_extension("json.tmp");
        fs::write(&staging, serialised)?;
        fs::rename(&staging, &self.path)
    }
}

/// Background writer that folds a burst of changes into a single write.
pub struct Persister {
    sender: Sender<Settings>,
}

impl Persister {
    pub fn spawn(store: Store) -> Self {
        let (sender, receiver) = mpsc::channel::<Settings>();

        thread::Builder::new()
            .name("audiomirror-settings".into())
            .spawn(move || {
                while let Ok(first) = receiver.recv() {
                    let mut pending = first;

                    // Keep the most recent value while changes keep arriving.
                    loop {
                        match receiver.recv_timeout(COALESCE) {
                            Ok(newer) => pending = newer,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }

                    if let Err(error) = store.save(&pending) {
                        eprintln!("audiomirror: could not save settings: {error}");
                    }
                }
            })
            .expect("settings thread");

        Self { sender }
    }

    pub fn submit(&self, settings: Settings) {
        let _ = self.sender.send(settings);
    }
}

fn executable_directory() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let store = Store {
            path: PathBuf::from("does-not-exist-audiomirror.json"),
            portable: false,
        };
        let settings = store.load();
        assert!(!settings.mirror.enabled);
        assert!(settings.preferences.close_to_tray);
    }

    #[test]
    fn survives_a_round_trip() {
        let directory = std::env::temp_dir().join("audiomirror-settings-test");
        let _ = fs::create_dir_all(&directory);
        let store = Store {
            path: directory.join("settings.json"),
            portable: true,
        };

        let mut settings = Settings::default();
        settings.preferences.autostart = true;
        settings.mirror.enabled = true;
        store.save(&settings).expect("save");

        let restored = store.load();
        assert!(restored.preferences.autostart);
        assert!(restored.mirror.enabled);
        assert_eq!(restored.version, FORMAT_VERSION);

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn damaged_content_does_not_prevent_starting() {
        let directory = std::env::temp_dir().join("audiomirror-settings-damaged");
        let _ = fs::create_dir_all(&directory);
        let path = directory.join("settings.json");
        fs::write(&path, "{ this is not json").expect("write");

        let store = Store {
            path,
            portable: false,
        };
        let settings = store.load();
        assert!(!settings.mirror.enabled);

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        // A file written by a later version must not lock the user out.
        let raw = r#"{"version":9,"mirror":{"enabled":true},"somethingNew":42}"#;
        let settings: Settings = serde_json::from_str(raw).expect("lenient parse");
        assert!(settings.mirror.enabled);
    }
}
