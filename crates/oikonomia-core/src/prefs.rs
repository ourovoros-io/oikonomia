//! Non-secret UI preferences stored as plaintext JSON in the data directory.
//!
//! Kept outside the encrypted vault on purpose: the tray menu and window
//! chrome must be built in the user's locale before any password has been
//! entered. Nothing stored here is sensitive.
//!
//! There is no theme preference: the app is dark-only, so a `"theme"` key in
//! a file written by an older build is ignored on load.

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

/// Most system language tags considered when picking the first-run language.
const MAX_SYSTEM_LANGUAGES: usize = 16;

/// Longest system language tag considered, in characters. BCP 47 tags in
/// practice stay well below this; anything longer is not a language tag.
const MAX_SYSTEM_LANGUAGE_TAG_CHARS: usize = 35;

impl Locale {
    /// The supported language a system language tag names, if any.
    ///
    /// Only the primary language subtag counts, compared without regard to
    /// case, and both `-` and `_` separate subtags (`el-GR`, `EL_gr`).
    fn from_language_tag(tag: &str) -> Option<Self> {
        // Bounded check: never scan past the limit of an absurd input.
        if tag.chars().nth(MAX_SYSTEM_LANGUAGE_TAG_CHARS).is_some() {
            return None;
        }

        let primary = tag.trim().split(['-', '_']).next()?;

        [Self::En, Self::El, Self::Fr, Self::De]
            .into_iter()
            .find(|locale| primary.eq_ignore_ascii_case(locale.code()))
    }

    /// The lowercase language code, matching the serialized form.
    const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::El => "el",
            Self::Fr => "fr",
            Self::De => "de",
        }
    }

    /// Pick the app language from the system's preferred languages, most
    /// preferred first.
    ///
    /// The first tag naming a supported language wins; tags for other
    /// languages are skipped. English when nothing matches, so an empty list,
    /// empty strings and garbage are all safe. At most the first 16 tags are
    /// read and tags longer than 35 characters are ignored.
    #[must_use]
    pub fn from_system_languages<S: AsRef<str>>(tags: &[S]) -> Self {
        tags.iter()
            .take(MAX_SYSTEM_LANGUAGES)
            .find_map(|tag| Self::from_language_tag(tag.as_ref()))
            .unwrap_or_default()
    }
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

/// The locale stored on disk, or `None` when none was ever stored.
///
/// `UiPrefs::locale` defaults to English, which cannot tell "never chosen"
/// from "chose English", so this reads whether the preferences file is a JSON
/// object with a `locale` key. Any value counts as stored; the returned
/// locale is then what [`load_ui_prefs`] reports, exactly as before. A
/// missing, empty or corrupt file counts as never stored.
#[must_use]
pub fn stored_locale(data_dir: &Path) -> Option<Locale> {
    let text = fs::read_to_string(ui_prefs_path(data_dir)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    value
        .as_object()?
        .contains_key("locale")
        .then(|| load_ui_prefs(data_dir).locale)
}

/// Persist `locale`, keeping every other stored preference.
///
/// Callers hold the prefs lock: the file is rewritten in place.
///
/// # Errors
///
/// Returns [`Error::Io`] when the file cannot be written.
pub fn store_locale(data_dir: &Path, locale: Locale) -> Result<()> {
    let mut prefs = load_ui_prefs(data_dir);
    prefs.locale = locale;

    save_ui_prefs(data_dir, &prefs)
}

/// Outcome of [`resolve_locale`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleResolution {
    /// The language the app uses from now on.
    pub locale: Locale,
    /// True when this call chose and stored it, so native text needs a refresh.
    pub newly_stored: bool,
}

/// The app language, choosing it from the system on the very first run.
///
/// When a locale is already stored it is returned unchanged and nothing is
/// written: the system language is consulted once per installation, so a user
/// who chose English on a Greek system stays in English. Otherwise the
/// language is mapped from `system_languages`, stored, and returned.
///
/// Callers hold the prefs lock so the check and the write cannot interleave
/// with a language change.
///
/// # Errors
///
/// Returns [`Error::Io`] when the chosen locale cannot be written.
pub fn resolve_locale<S: AsRef<str>>(
    data_dir: &Path,
    system_languages: &[S],
) -> Result<LocaleResolution> {
    if let Some(locale) = stored_locale(data_dir) {
        return Ok(LocaleResolution {
            locale,
            newly_stored: false,
        });
    }

    let locale = Locale::from_system_languages(system_languages);
    store_locale(data_dir, locale)?;

    Ok(LocaleResolution {
        locale,
        newly_stored: true,
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
        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
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
    }

    #[test]
    fn prefs_file_from_the_paid_build_still_loads() {
        let Ok(dir) = tempdir() else {
            return;
        };

        // Written by builds that had a trial; the key is now unknown.
        let json = r#"{
            "locale": "de",
            "last_entity_id": "ent-1",
            "trial_started_at": "2026-09-01T10:00:00Z"
        }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());

        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.locale, Locale::De);
        assert_eq!(prefs.last_entity_id.as_deref(), Some("ent-1"));
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

    #[test]
    fn retired_theme_key_still_loads() {
        let Ok(dir) = tempdir() else {
            return;
        };

        // Builds before the dark-only redesign wrote a "theme" key. It must
        // load as an ignored field, never make the file read as corrupt.
        let json = r#"{ "theme": "light", "locale": "el" }"#;
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
    fn system_languages_map_to_the_first_supported_one() {
        let cases: &[(&[&str], Locale)] = &[
            (&["el-GR"], Locale::El),
            (&["el"], Locale::El),
            (&["EL_gr"], Locale::El),
            (&["fr-CA"], Locale::Fr),
            (&["FR"], Locale::Fr),
            (&["de-AT"], Locale::De),
            (&["de_CH"], Locale::De),
            (&["en-GB"], Locale::En),
            (&["EN"], Locale::En),
            (&["ja-JP", "de-DE", "fr-FR"], Locale::De),
            (&["fr-FR", "de-DE"], Locale::Fr),
            (&["ja-JP", "zh-Hans-CN"], Locale::En),
            (&["es", "el-GR"], Locale::El),
            (&[], Locale::En),
            (&[""], Locale::En),
            (&["", "   ", "-", "_", "--el"], Locale::En),
            (&["", "fr"], Locale::Fr),
            (&["eleven"], Locale::En),
            (&["\u{1F600}", "\0", "el-"], Locale::El),
        ];

        for (tags, expected) in cases {
            assert_eq!(Locale::from_system_languages(tags), *expected, "{tags:?}");
        }
    }

    #[test]
    fn an_over_long_system_language_tag_is_ignored() {
        let long = format!("el-{}", "x".repeat(40));
        assert_eq!(
            Locale::from_system_languages(&[long, "fr".into()]),
            Locale::Fr
        );

        let boundary = format!("el-{}", "x".repeat(32));
        assert_eq!(boundary.chars().count(), 35);
        assert_eq!(Locale::from_system_languages(&[boundary]), Locale::El);
    }

    #[test]
    fn only_the_first_sixteen_system_languages_count() {
        let mut tags: Vec<String> = vec!["ja".to_string(); 16];
        tags.push("de-DE".to_string());
        assert_eq!(Locale::from_system_languages(&tags), Locale::En);

        let mut tags: Vec<String> = vec!["ja".to_string(); 15];
        tags.push("de-DE".to_string());
        assert_eq!(Locale::from_system_languages(&tags), Locale::De);
    }

    #[test]
    fn first_run_resolves_stores_and_then_never_changes() {
        let Ok(dir) = tempdir() else {
            return;
        };

        assert_eq!(stored_locale(dir.path()), None);

        let first = resolve_locale(dir.path(), &["el-GR", "en-US"]);
        assert_eq!(
            first,
            Ok(LocaleResolution {
                locale: Locale::El,
                newly_stored: true
            })
        );
        assert_eq!(load_ui_prefs(dir.path()).locale, Locale::El);
        assert_eq!(stored_locale(dir.path()), Some(Locale::El));

        let written = fs::read_to_string(ui_prefs_path(dir.path())).ok();
        let again = resolve_locale(dir.path(), &["fr-FR"]);
        assert_eq!(
            again,
            Ok(LocaleResolution {
                locale: Locale::El,
                newly_stored: false
            })
        );
        assert_eq!(fs::read_to_string(ui_prefs_path(dir.path())).ok(), written);
    }

    #[test]
    fn a_stored_english_choice_is_not_replaced_by_the_system_language() {
        let Ok(dir) = tempdir() else {
            return;
        };

        assert!(fs::write(ui_prefs_path(dir.path()), r#"{ "locale": "en" }"#).is_ok());
        let before = fs::read_to_string(ui_prefs_path(dir.path())).ok();

        let resolved = resolve_locale(dir.path(), &["el-GR"]);
        assert_eq!(
            resolved,
            Ok(LocaleResolution {
                locale: Locale::En,
                newly_stored: false
            })
        );
        assert_eq!(fs::read_to_string(ui_prefs_path(dir.path())).ok(), before);
    }

    #[test]
    fn an_unreadable_stored_locale_value_still_counts_as_chosen() {
        let Ok(dir) = tempdir() else {
            return;
        };

        // Today this loads as English; it must not be treated as a first run.
        let json = r#"{ "locale": "klingon", "last_entity_id": "ent-1" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let before = fs::read_to_string(ui_prefs_path(dir.path())).ok();

        let resolved = resolve_locale(dir.path(), &["el-GR"]);
        assert_eq!(
            resolved,
            Ok(LocaleResolution {
                locale: Locale::En,
                newly_stored: false
            })
        );
        assert_eq!(fs::read_to_string(ui_prefs_path(dir.path())).ok(), before);
    }

    #[test]
    fn a_file_without_a_locale_key_is_a_first_run_that_keeps_other_prefs() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let mut last_accounts = BTreeMap::new();
        last_accounts.insert(
            "ent-1:expense".to_string(),
            LastRoleAccounts {
                category_account_id: Some("cat-1".into()),
                ..LastRoleAccounts::default()
            },
        );
        let json = r#"{
            "theme": "light",
            "last_entity_id": "ent-1",
            "last_accounts_by_entity_kind": {
                "ent-1:expense": { "category_account_id": "cat-1" }
            }
        }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        assert_eq!(stored_locale(dir.path()), None);

        let resolved = resolve_locale(dir.path(), &["de-DE"]);
        assert_eq!(
            resolved,
            Ok(LocaleResolution {
                locale: Locale::De,
                newly_stored: true
            })
        );
        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                locale: Locale::De,
                last_entity_id: Some("ent-1".into()),
                last_accounts_by_entity_kind: last_accounts,
            }
        );
    }

    #[test]
    fn empty_and_corrupt_files_are_a_first_run() {
        for content in ["", "not json", "[1, 2]", "null", "{\"locale\": "] {
            let Ok(dir) = tempdir() else {
                return;
            };

            assert!(fs::write(ui_prefs_path(dir.path()), content).is_ok());
            assert_eq!(stored_locale(dir.path()), None, "{content:?}");

            let resolved = resolve_locale(dir.path(), &["fr-FR"]);
            assert_eq!(
                resolved,
                Ok(LocaleResolution {
                    locale: Locale::Fr,
                    newly_stored: true
                }),
                "{content:?}"
            );
            assert_eq!(load_ui_prefs(dir.path()).locale, Locale::Fr);
            assert_eq!(stored_locale(dir.path()), Some(Locale::Fr));
        }
    }

    #[test]
    fn store_locale_replaces_only_the_locale() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let prefs = UiPrefs {
            locale: Locale::En,
            last_entity_id: Some("ent-1".into()),
            ..UiPrefs::default()
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());

        assert!(store_locale(dir.path(), Locale::Fr).is_ok());
        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                locale: Locale::Fr,
                ..prefs
            }
        );
    }
}
