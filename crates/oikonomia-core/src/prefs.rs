//! Non-secret UI preferences stored as plaintext JSON in the data directory.
//!
//! Kept outside the encrypted vault on purpose: the unlock screen must render
//! with the user's theme before any password has been entered. Nothing stored
//! here is sensitive.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// UI color theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Dark theme (the default).
    Dark,
    /// Light theme.
    Light,
}

/// Non-secret UI preferences.
///
/// Unknown or missing fields fall back to defaults so older and newer app
/// versions can share the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// Color theme.
    pub theme: Theme,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self { theme: Theme::Dark }
    }
}

/// Path of the preferences file inside the app data directory.
#[must_use]
pub fn ui_prefs_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ui-prefs.json")
}

/// Load preferences.
///
/// A missing or unreadable file yields the defaults: a corrupt preferences
/// file must never block startup, it just costs the saved choices.
#[must_use]
pub fn load_ui_prefs(data_dir: &Path) -> UiPrefs {
    let path = ui_prefs_path(data_dir);
    let Ok(text) = fs::read_to_string(&path) else {
        return UiPrefs::default();
    };

    serde_json::from_str(&text).unwrap_or_else(|err| {
        log::warn!("ignoring corrupt ui prefs at {}: {err}", path.display());
        UiPrefs::default()
    })
}

/// Persist preferences.
///
/// # Errors
///
/// Returns [`Error::Io`] when the data directory cannot be created or the
/// file cannot be written.
pub fn save_ui_prefs(data_dir: &Path, prefs: &UiPrefs) -> Result<()> {
    fs::create_dir_all(data_dir)
        .map_err(|e| Error::Io(format!("could not create data directory: {e}")))?;

    let json = serde_json::to_string_pretty(prefs)
        .map_err(|e| Error::Io(format!("could not encode ui prefs: {e}")))?;

    fs::write(ui_prefs_path(data_dir), json)
        .map_err(|e| Error::Io(format!("could not write ui prefs: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_yields_defaults() {
        let Ok(dir) = tempdir() else {
            return;
        };

        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
    }

    #[test]
    fn theme_round_trips() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let prefs = UiPrefs {
            theme: Theme::Light,
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), prefs);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let Ok(dir) = tempdir() else {
            return;
        };

        assert!(fs::write(ui_prefs_path(dir.path()), "not json").is_ok());
        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let json = r#"{ "theme": "light", "future_field": 42 }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                theme: Theme::Light
            }
        );
    }
}
