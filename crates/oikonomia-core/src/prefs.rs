//! Non-secret UI preferences stored as plaintext JSON in the data directory.
//!
//! Kept outside the encrypted vault on purpose: the tray menu and window
//! chrome must be built in the user's locale before any password has been
//! entered. Nothing stored here is sensitive.
//!
//! There is no theme preference: the app is dark-only (the Livery chassis),
//! so a `"theme"` key in an older file is simply ignored on load.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Native UI locale (tray, dialogs). Webview i18n is separate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    /// English (the default).
    #[default]
    En,
    /// Greek.
    El,
    /// French.
    Fr,
    /// German.
    De,
}

/// Last role-account picks for a single entity+kind tray post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LastRoleAccounts {
    pub category_account_id: Option<String>,
    pub wallet_account_id: Option<String>,
    pub payable_account_id: Option<String>,
    pub from_account_id: Option<String>,
    pub to_account_id: Option<String>,
}

/// Non-secret UI preferences.
///
/// Unknown or missing fields fall back to defaults so older and newer app
/// versions can share the same file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UiPrefs {
    /// Native locale for tray menu, window titles, and file-dialog filters.
    pub locale: Locale,
    /// Last entity used in the tray quick-add panel.
    pub last_entity_id: Option<String>,
    /// Map key: `"{entity_id}:{kind}"` (kind = expense|income|bill|transfer).
    pub last_accounts_by_entity_kind: BTreeMap<String, LastRoleAccounts>,
    /// RFC3339 timestamp of the first successful vault unlock. Set once;
    /// never reset. Missing on older files until that first unlock.
    pub trial_started_at: Option<String>,
}

/// Build the map key for last-used accounts.
#[must_use]
pub fn last_accounts_key(entity_id: &str, kind: &str) -> String {
    format!("{entity_id}:{kind}")
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
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn missing_file_yields_defaults() {
        let Ok(dir) = tempdir() else {
            return;
        };

        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
    }

    #[test]
    fn locale_round_trips() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let prefs = UiPrefs {
            locale: Locale::El,
            ..UiPrefs::default()
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

        // "theme" is a retired key an older build would have written, so it
        // doubles as the unknown-field fixture alongside an invented one.
        let json = r#"{ "locale": "el", "theme": "light", "future_field": 42 }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                locale: Locale::El,
                ..UiPrefs::default()
            }
        );
    }

    #[test]
    fn last_used_round_trips() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let mut last_accounts = BTreeMap::new();
        last_accounts.insert(
            "ent-1:expense".to_string(),
            LastRoleAccounts {
                category_account_id: Some("cat-1".into()),
                wallet_account_id: Some("wal-1".into()),
                payable_account_id: None,
                from_account_id: None,
                to_account_id: None,
            },
        );

        let prefs = UiPrefs {
            locale: Locale::En,
            last_entity_id: Some("ent-1".into()),
            last_accounts_by_entity_kind: last_accounts,
            trial_started_at: None,
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), prefs);
    }

    #[test]
    fn missing_last_used_fields_default() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let json = r#"{ "theme": "light" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.locale, Locale::En);
        assert_eq!(prefs.last_entity_id, None);
        assert!(prefs.last_accounts_by_entity_kind.is_empty());
        assert_eq!(prefs.trial_started_at, None);
    }

    #[test]
    fn missing_trial_started_at_defaults_to_none() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let json = r#"{ "theme": "light" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.trial_started_at, None);
    }

    #[test]
    fn locale_defaults_to_en() {
        assert_eq!(UiPrefs::default().locale, Locale::En);
    }

    #[test]
    fn locale_round_trips_el() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let prefs = UiPrefs {
            locale: Locale::El,
            ..UiPrefs::default()
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), prefs);
        assert_eq!(load_ui_prefs(dir.path()).locale, Locale::El);
    }

    #[test]
    fn locale_round_trips_fr() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let prefs = UiPrefs {
            locale: Locale::Fr,
            ..UiPrefs::default()
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), prefs);
        assert_eq!(load_ui_prefs(dir.path()).locale, Locale::Fr);
    }

    #[test]
    fn locale_round_trips_de() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let prefs = UiPrefs {
            locale: Locale::De,
            ..UiPrefs::default()
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), prefs);
        assert_eq!(load_ui_prefs(dir.path()).locale, Locale::De);
    }

    #[test]
    fn missing_locale_defaults_to_en() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let json = r#"{ "theme": "light" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.locale, Locale::En);
    }

    #[test]
    fn unknown_locale_field_is_ignored() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let json = r#"{ "theme": "light", "locale": "el", "future_field": 42 }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                locale: Locale::El,
                ..UiPrefs::default()
            }
        );
    }
}
