//! Non-secret UI preferences stored as plaintext JSON in the data directory.
//!
//! # Why outside the vault
//!
//! The file `ui-prefs.json` is kept outside the encrypted vault on purpose:
//! the tray menu and window chrome must be built in the user's language
//! before any password has been entered. It holds the app language and the
//! ids of the book and accounts last used in quick add; no name, amount or
//! other ledger content.
//!
//! # What is stored
//!
//! [`UiPrefs`] is the whole file. Its [`Locale`] is the app language, which
//! is more than a display setting: it also decides the language of text core
//! writes into books (see [`Locale`]).
//!
//! # Reading and writing
//!
//! - [`load_ui_prefs`] never fails. A missing or undecodable file gives the
//!   defaults, because a damaged preferences file must not stop the app from
//!   starting.
//! - [`save_ui_prefs`] writes a temporary file and renames it over the
//!   target, so a reader sees the old file or the new one, complete.
//! - A change is a load, an edit and a save. Nothing here locks, so two
//!   writers can lose each other's change; callers that change the file
//!   serialise themselves with a lock of their own.
//! - [`remember_quick_add`] is that load, edit and save for the book and
//!   accounts last used in quick add.
//!
//! # The first run
//!
//! [`resolve_locale`] picks the language from the system's preferred
//! languages once, when the file has no `locale` key, and stores it. After
//! that the stored value wins, so a user who chose English on a Greek system
//! stays in English. [`stored_locale`] is how "never chosen" is told from
//! "chose English", which the default value of the field cannot express.
//!
//! # Compatibility
//!
//! Builds of different ages share the file, so [`UiPrefs`] ignores unknown
//! keys and defaults missing ones. There is no theme preference: the app is
//! dark-only, so a `"theme"` key written by an older build is one of the
//! ignored keys.

use crate::domain::EntityId;
use crate::error::{Error, IoContext, Result, SerializationContext};
use crate::ledger::SimpleEntryKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The language of the app.
///
/// One stored value governs three things:
///
/// - the native UI the desktop shell builds itself: the tray menu, native
///   window titles and file-dialog filters;
/// - the web UI, which asks the shell for this value at startup and uses it
///   as its language;
/// - text that core writes into a book and that stays there: seeded account
///   names and generated descriptions ([`crate::text`]). That text keeps the
///   language it was written in when this value changes later.
///
/// Serialized as the lowercase two-letter code (`"en"`, `"el"`, `"fr"`,
/// `"de"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    /// English, the default.
    #[default]
    En,
    /// Greek.
    El,
    /// French.
    Fr,
    /// German.
    De,
}

impl Locale {
    /// Every supported language, in declaration order.
    ///
    /// Code that must treat each language (detecting the system language,
    /// tests over the wording tables) iterates this list instead of naming
    /// the variants again. A test checks the list against the enum, so a new
    /// variant cannot be left out of it.
    pub const ALL: &'static [Self] = &[Self::En, Self::El, Self::Fr, Self::De];

    /// Returns the app language for the system's preferred languages, most
    /// preferred first.
    ///
    /// The first tag naming a supported language wins; tags for other
    /// languages are skipped. Only the primary subtag of a tag counts,
    /// compared without regard to ASCII case, and both `-` and `_` separate
    /// subtags. The result is English when no tag matches, so an empty list,
    /// empty strings and arbitrary text are all accepted. Only the first 16
    /// tags are read, and a tag longer than 35 characters is skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::prefs::Locale;
    ///
    /// assert_eq!(Locale::from_system_languages(&["el-GR", "en-US"]), Locale::El);
    /// assert_eq!(Locale::from_system_languages(&["ja-JP", "de_CH"]), Locale::De);
    /// assert_eq!(Locale::from_system_languages(&["ja-JP"]), Locale::En);
    /// assert_eq!(Locale::from_system_languages::<&str>(&[]), Locale::En);
    /// ```
    #[must_use]
    pub fn from_system_languages<S: AsRef<str>>(tags: &[S]) -> Self {
        tags.iter()
            .take(MAX_SYSTEM_LANGUAGES)
            .find_map(|tag| Self::from_language_tag(tag.as_ref()))
            .unwrap_or_default()
    }

    /// Returns the supported language a system language tag names, if any.
    ///
    /// Only the primary language subtag counts, compared without regard to
    /// ASCII case, and both `-` and `_` separate subtags (`el-GR`, `EL_gr`).
    /// Surrounding whitespace is trimmed before the tag is split; the length
    /// limit applies to the tag as given, before trimming.
    fn from_language_tag(tag: &str) -> Option<Self> {
        // `nth` stops at the limit, so an absurdly long input is never
        // walked to its end.
        if tag.chars().nth(MAX_SYSTEM_LANGUAGE_TAG_CHARS).is_some() {
            return None;
        }

        let primary = tag.trim().split(['-', '_']).next()?;

        Self::ALL
            .iter()
            .copied()
            .find(|locale| primary.eq_ignore_ascii_case(locale.code()))
    }

    /// Returns the lowercase language code, which is also the serialized form.
    const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::El => "el",
            Self::Fr => "fr",
            Self::De => "de",
        }
    }
}

/// The number of system language tags read when picking the first-run
/// language; later tags are ignored.
const MAX_SYSTEM_LANGUAGES: usize = 16;

/// The longest system language tag considered, in characters; a longer one
/// is skipped.
///
/// 35 is the smallest limit RFC 5646 (section 4.4.1) lets a protocol set on
/// a language tag. A longer tag can be valid; it is skipped here so that the
/// work per tag stays bounded whatever the system reports.
const MAX_SYSTEM_LANGUAGE_TAG_CHARS: usize = 35;

/// The accounts last used for one kind of quick-add entry in one entity.
///
/// Each field is an account id as text, or `None` when that kind of entry
/// does not use the field or nothing was remembered. The ids are not checked
/// here; an account may have been archived or removed since.
///
/// The field names are the keys of the preferences file on users' machines,
/// so they stay as they are. They are the wire names of the parts of a simple
/// entry; [`SimpleEntryRoleAccounts`](crate::ledger::SimpleEntryRoleAccounts)
/// has the table that maps them to the names the ledger and its errors use.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LastRoleAccounts {
    /// The expense or income category last posted to.
    pub category_account_id: Option<String>,
    /// The bank, cash or card account last paid from or into.
    pub wallet_account_id: Option<String>,
    /// The liability account the last unpaid bill was booked against.
    pub payable_account_id: Option<String>,
    /// The account the last transfer left.
    pub from_account_id: Option<String>,
    /// The account the last transfer arrived in.
    pub to_account_id: Option<String>,
}

/// The contents of the preferences file.
///
/// Older and newer builds share one file, so loading is forgiving in three
/// ways: a key this build does not know is ignored, a missing key takes its
/// default, and a `locale` value this build does not know takes the default
/// locale without affecting the other fields. Any other value of the wrong
/// shape still fails the whole file, which then loads as the defaults.
///
/// Saving writes only the fields below, so a key or a locale value this build
/// could not read is not carried over to the saved file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// The app language; see [`Locale`] for what it governs.
    #[serde(deserialize_with = "known_locale_or_default")]
    pub locale: Locale,
    /// The id of the entity last used in quick add, as text.
    pub last_entity_id: Option<String>,
    /// The accounts last used in quick add, keyed by entity and entry kind
    /// with the key [`last_accounts_key`] builds.
    pub last_accounts_by_entity_kind: BTreeMap<String, LastRoleAccounts>,
}

/// Reads a stored locale, taking any value this build does not know as the
/// default locale.
///
/// A build with more languages may have written the file. Failing here would
/// fail the whole [`UiPrefs`], and the next save would then overwrite the
/// stored entity and account choices with defaults.
fn known_locale_or_default<'de, D>(deserializer: D) -> std::result::Result<Locale, D::Error>
where
    D: serde::Deserializer<'de>,
{
    /// A stored locale value: a language of this build, or anything else.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StoredLocale {
        /// One of the values [`Locale`] serializes to.
        Known(Locale),
        /// Any other JSON value, read and discarded.
        Unknown(serde::de::IgnoredAny),
    }

    match StoredLocale::deserialize(deserializer)? {
        StoredLocale::Known(locale) => Ok(locale),
        StoredLocale::Unknown(serde::de::IgnoredAny) => {
            log::warn!("stored locale is not one this build knows; using the default");
            Ok(Locale::default())
        }
    }
}

/// Returns the key of [`UiPrefs::last_accounts_by_entity_kind`] for an entity
/// and an entry kind: the entity's id, a colon, and the kind as
/// [`SimpleEntryKind::identifier`] writes it.
///
/// # Examples
///
/// ```
/// use oikonomia_core::domain::EntityId;
/// use oikonomia_core::ledger::SimpleEntryKind;
/// use oikonomia_core::prefs::last_accounts_key;
///
/// let entity_id: EntityId = "11111111-1111-4111-8111-111111111111".parse()?;
/// assert_eq!(
///     last_accounts_key(entity_id, SimpleEntryKind::Expense),
///     "11111111-1111-4111-8111-111111111111:expense"
/// );
/// # Ok::<(), oikonomia_core::Error>(())
/// ```
#[must_use]
pub fn last_accounts_key(entity_id: EntityId, kind: SimpleEntryKind) -> String {
    format!("{entity_id}:{}", kind.identifier())
}

/// Returns the path of the preferences file, `ui-prefs.json` in `data_dir`.
#[must_use]
pub fn ui_prefs_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ui-prefs.json")
}

/// Returns the stored preferences, or the defaults when there are none to
/// read.
///
/// A file that is missing or cannot be read gives the defaults silently. A
/// file that is not a [`UiPrefs`] gives the defaults too, with a warning
/// through the `log` facade. Nothing is an error: a damaged preferences file must not stop
/// the app from starting, it only costs the saved choices.
#[must_use]
pub fn load_ui_prefs(data_dir: &Path) -> UiPrefs {
    let path = ui_prefs_path(data_dir);
    let Ok(text) = fs::read_to_string(&path) else {
        return UiPrefs::default();
    };

    decode_ui_prefs(&text, &path)
}

/// Decodes the text of the preferences file at `path`, giving the defaults
/// (and logging why) when the text is not a [`UiPrefs`].
fn decode_ui_prefs(text: &str, path: &Path) -> UiPrefs {
    serde_json::from_str(text).unwrap_or_else(|err| {
        log::warn!("ignoring corrupt ui prefs at {}: {err}", path.display());
        UiPrefs::default()
    })
}

/// Returns the locale stored on disk, or `None` when none was ever stored.
///
/// [`UiPrefs::locale`] defaults to English, which cannot tell "never chosen"
/// from "chose English", so this checks whether the preferences file is a
/// JSON object with a `locale` key. Any value under that key counts as
/// stored, including one this build cannot read; the locale returned is the
/// one [`load_ui_prefs`] gives for the same text. A missing file, text that
/// is not JSON, and JSON that is not an object all count as never stored.
///
/// The file is read once, so the key check and the returned locale describe
/// the same contents.
#[must_use]
pub fn stored_locale(data_dir: &Path) -> Option<Locale> {
    let path = ui_prefs_path(data_dir);
    let text = fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    value
        .as_object()?
        .contains_key("locale")
        .then(|| decode_ui_prefs(&text, &path).locale)
}

/// Stores `locale`, keeping every other preference this build can read.
///
/// This loads the file, changes the one field and saves the result with
/// [`save_ui_prefs`], which replaces the file by renaming a temporary one over
/// it. Nothing here locks: a caller that can race another writer must hold a
/// lock of its own across the call, or a save made between this load and
/// this save is lost.
///
/// # Errors
///
/// Returns [`Error::Io`] when the data directory cannot be created or the
/// file cannot be written, and [`Error::Serialization`] when the preferences
/// cannot be encoded, as [`save_ui_prefs`] does.
pub fn store_locale(data_dir: &Path, locale: Locale) -> Result<()> {
    let mut prefs = load_ui_prefs(data_dir);
    prefs.locale = locale;

    save_ui_prefs(data_dir, &prefs)
}

/// What [`resolve_locale`] found or decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleResolution {
    /// The language the app uses from now on.
    pub locale: Locale,
    /// `true` when this call chose the language and stored it, so native text
    /// built from the earlier default needs a refresh.
    pub newly_stored: bool,
}

/// Returns the app language, choosing it from the system on the very first
/// run.
///
/// When a locale is already stored it is returned unchanged and nothing is
/// written: the system language is consulted once per installation, so a user
/// who chose English on a Greek system stays in English. Otherwise the
/// language is mapped from `system_languages`, stored, and returned.
///
/// Nothing here locks: a caller holds its own lock across the call so that
/// the check and the write cannot interleave with a language change.
///
/// # Errors
///
/// Returns the error of [`store_locale`] when a language is chosen and cannot
/// be stored. A
/// call that finds a stored language writes nothing and cannot fail.
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

/// Writes `prefs` as the preferences file, replacing the one that is there.
///
/// The JSON is written to a temporary sibling file, flushed to disk, and
/// renamed over the target, so a reader (and a crash) sees either the old or
/// the new complete file, never a truncated one. A temporary file left by an
/// earlier crash is removed first, and after a failed write the temporary
/// file is removed again; a removal that fails is only logged.
///
/// The file gets the process's default permissions, not the owner-only mode
/// of vault files: it is not part of the vault.
///
/// # Errors
///
/// Returns [`Error::Io`] when the data directory cannot be created or the
/// temporary file cannot be written or renamed into place, and
/// [`Error::Serialization`] when the preferences cannot be encoded as JSON.
pub fn save_ui_prefs(data_dir: &Path, prefs: &UiPrefs) -> Result<()> {
    fs::create_dir_all(data_dir).io("create data directory")?;

    let json = serde_json::to_string_pretty(prefs).serialization("encode preferences")?;

    let temporary = ui_prefs_temporary_path(data_dir);
    remove_stale_temporary(&temporary);

    let staged = write_synced(&temporary, json.as_bytes())
        .and_then(|()| fs::rename(&temporary, ui_prefs_path(data_dir)));

    staged.map_err(|err| {
        remove_stale_temporary(&temporary);
        Error::io("write preferences file", err)
    })
}

/// Remembers `accounts` as the ones last used for a quick-add entry of
/// `kind` in the book `entity_id`, and that book as the one last used.
///
/// The accounts are stored under [`last_accounts_key`] for the book and the
/// kind, replacing what was remembered for that pair; every other
/// preference this build can read is kept. The ids in `accounts` are stored
/// as given and are not checked against the vault, which may be locked.
///
/// This loads the file, changes the two values and saves the result with
/// [`save_ui_prefs`], which replaces the file by renaming a temporary one
/// over it. Nothing here locks: a caller that can race another writer must
/// hold a lock of its own across the call, or a save made between this load
/// and this save is lost.
///
/// # Errors
///
/// Returns [`Error::Io`] when the data directory cannot be created or the
/// file cannot be written, and [`Error::Serialization`] when the preferences
/// cannot be encoded, as [`save_ui_prefs`] does.
pub fn remember_quick_add(
    data_dir: &Path,
    entity_id: EntityId,
    kind: SimpleEntryKind,
    accounts: LastRoleAccounts,
) -> Result<()> {
    let mut prefs = load_ui_prefs(data_dir);
    prefs
        .last_accounts_by_entity_kind
        .insert(last_accounts_key(entity_id, kind), accounts);
    prefs.last_entity_id = Some(entity_id.to_string());

    save_ui_prefs(data_dir, &prefs)
}

/// Returns the path of the temporary file a save is staged in before it
/// replaces the preferences file.
fn ui_prefs_temporary_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ui-prefs.json.tmp")
}

/// Writes `bytes` to a new file at `path`, truncating one that exists, and
/// flushes them to disk.
fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Removes a leftover temporary file, logging a failure other than the file
/// not being there, which is the normal case.
fn remove_stale_temporary(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!("could not remove {}: {err}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oikonomia_test_support::listed_variants;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    listed_variants! {
        units listed_locales for Locale {
            Locale::En,
            Locale::El,
            Locale::Fr,
            Locale::De,
        }
    }

    /// Fails unless `Locale::ALL` is the variants in the `listed_locales`
    /// list above, in the order the enum declares them (its discriminants).
    /// The compiler checks that list against the enum with an exhaustive
    /// `match`, so a language added to the enum but left out of the list does
    /// not compile.
    #[test]
    fn all_lists_every_locale_once_in_declaration_order() {
        assert_eq!(Locale::ALL, listed_locales::variants());
        assert!(
            Locale::ALL
                .iter()
                .map(|locale| *locale as usize)
                .eq(0..Locale::ALL.len()),
            "Locale::ALL is not in declaration order"
        );
        listed_locales::assert_every_position_once(
            Locale::ALL.iter().map(listed_locales::position).collect(),
        );
    }

    #[test]
    fn every_locale_code_is_its_serialized_form_and_is_detected() {
        for locale in Locale::ALL {
            let serialized = serde_json::to_value(locale).unwrap();

            assert_eq!(serialized, serde_json::Value::from(locale.code()));
            assert_eq!(Locale::from_system_languages(&[locale.code()]), *locale);
        }
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempdir().unwrap();

        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = tempdir().unwrap();

        assert!(fs::write(ui_prefs_path(dir.path()), "not json").is_ok());
        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let dir = tempdir().unwrap();

        let json = r#"{ "theme": "light", "future_field": 42 }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
    }

    #[test]
    fn last_used_round_trips() {
        let dir = tempdir().unwrap();

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
        let dir = tempdir().unwrap();

        let json = r#"{ "theme": "light" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.locale, Locale::En);
        assert_eq!(prefs.last_entity_id, None);
        assert!(prefs.last_accounts_by_entity_kind.is_empty());
    }

    #[test]
    fn prefs_file_from_the_paid_build_still_loads() {
        let dir = tempdir().unwrap();

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
        let dir = tempdir().unwrap();

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
        let dir = tempdir().unwrap();

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
        let dir = tempdir().unwrap();

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
        let dir = tempdir().unwrap();

        let json = r#"{ "theme": "light" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.locale, Locale::En);
    }

    #[test]
    fn unknown_keys_beside_a_locale_are_ignored() {
        let dir = tempdir().unwrap();

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
        let dir = tempdir().unwrap();

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
            // Three-letter and longer primary subtags never match a
            // two-letter language, and "english" is not the "en" tag.
            (&["ell"], Locale::En),
            (&["english"], Locale::En),
            (&["english", "de-DE"], Locale::De),
            // Only `-` and `_` separate subtags, so a POSIX modifier glues
            // onto the language and nothing matches.
            (&["fr@euro"], Locale::En),
            (&["fr@euro", "de"], Locale::De),
            (&["C"], Locale::En),
            (&["POSIX"], Locale::En),
            (&["zh-Hant-TW"], Locale::En),
            (&["de-CH-1996"], Locale::De),
            // The language wins over the region.
            (&["en-GR"], Locale::En),
            (&["en-GR", "el"], Locale::En),
            (&["el-"], Locale::El),
            (&["   "], Locale::En),
            (&["\t\n"], Locale::En),
            (&["fr\u{7}"], Locale::En),
            (&["e\u{7}l"], Locale::En),
            (&["\u{7}de", "fr"], Locale::Fr),
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
        let dir = tempdir().unwrap();

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

        let written = fs::read_to_string(ui_prefs_path(dir.path())).unwrap();
        let again = resolve_locale(dir.path(), &["fr-FR"]);
        assert_eq!(
            again,
            Ok(LocaleResolution {
                locale: Locale::El,
                newly_stored: false
            })
        );
        assert_eq!(
            fs::read_to_string(ui_prefs_path(dir.path())).unwrap(),
            written
        );
    }

    #[test]
    fn a_stored_english_choice_is_not_replaced_by_the_system_language() {
        let dir = tempdir().unwrap();

        assert!(fs::write(ui_prefs_path(dir.path()), r#"{ "locale": "en" }"#).is_ok());
        let before = fs::read_to_string(ui_prefs_path(dir.path())).unwrap();

        let resolved = resolve_locale(dir.path(), &["el-GR"]);
        assert_eq!(
            resolved,
            Ok(LocaleResolution {
                locale: Locale::En,
                newly_stored: false
            })
        );
        assert_eq!(
            fs::read_to_string(ui_prefs_path(dir.path())).unwrap(),
            before
        );
    }

    #[test]
    fn an_unreadable_stored_locale_value_still_counts_as_chosen() {
        let dir = tempdir().unwrap();

        // The value loads as the default locale, but the key is present, so
        // this is not a first run and the system language is not consulted.
        let json = r#"{ "locale": "klingon", "last_entity_id": "ent-1" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let before = fs::read_to_string(ui_prefs_path(dir.path())).unwrap();

        assert_eq!(
            load_ui_prefs(dir.path()).last_entity_id.as_deref(),
            Some("ent-1")
        );

        let resolved = resolve_locale(dir.path(), &["el-GR"]);
        assert_eq!(
            resolved,
            Ok(LocaleResolution {
                locale: Locale::En,
                newly_stored: false
            })
        );
        assert_eq!(
            fs::read_to_string(ui_prefs_path(dir.path())).unwrap(),
            before
        );
    }

    #[test]
    fn a_locale_this_build_does_not_know_costs_only_the_locale() {
        let dir = tempdir().unwrap();

        // Written by a build with a fifth language, Spanish.
        let json = r#"{
            "locale": "es",
            "last_entity_id": "ent-1",
            "last_accounts_by_entity_kind": {
                "ent-1:expense": { "category_account_id": "cat-1" }
            }
        }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());

        let mut last_accounts = BTreeMap::new();
        last_accounts.insert(
            "ent-1:expense".to_string(),
            LastRoleAccounts {
                category_account_id: Some("cat-1".into()),
                ..LastRoleAccounts::default()
            },
        );
        let expected = UiPrefs {
            locale: Locale::En,
            last_entity_id: Some("ent-1".into()),
            last_accounts_by_entity_kind: last_accounts,
        };
        assert_eq!(load_ui_prefs(dir.path()), expected);

        // The next save must not wipe what this build could read.
        assert!(store_locale(dir.path(), Locale::De).is_ok());
        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                locale: Locale::De,
                ..expected
            }
        );
    }

    #[test]
    fn a_wrongly_shaped_value_other_than_the_locale_still_fails_the_whole_file() {
        let dir = tempdir().unwrap();

        let json = r#"{ "locale": "el", "last_entity_id": 7 }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());

        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
        assert_eq!(stored_locale(dir.path()), Some(Locale::En));
    }

    #[test]
    fn a_file_without_a_locale_key_is_a_first_run_that_keeps_other_prefs() {
        let dir = tempdir().unwrap();

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
            let dir = tempdir().unwrap();

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
        let dir = tempdir().unwrap();

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

    #[test]
    fn a_whitespace_only_file_is_a_first_run() {
        let dir = tempdir().unwrap();

        assert!(fs::write(ui_prefs_path(dir.path()), " \n\t ").is_ok());
        assert_eq!(stored_locale(dir.path()), None);
    }

    #[test]
    fn a_null_locale_counts_as_stored_and_is_not_rewritten() {
        let dir = tempdir().unwrap();

        // `null` is not a language, so the field loads as the default locale
        // and the rest of the file loads normally; the key counts as stored.
        let json = r#"{"locale": null, "last_entity_id": "ent-1"}"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
        let before = fs::read_to_string(ui_prefs_path(dir.path())).unwrap();

        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                last_entity_id: Some("ent-1".into()),
                ..UiPrefs::default()
            }
        );

        let resolved = resolve_locale(dir.path(), &["el-GR"]);
        assert_eq!(
            resolved,
            Ok(LocaleResolution {
                locale: Locale::En,
                newly_stored: false
            })
        );
        assert_eq!(
            fs::read_to_string(ui_prefs_path(dir.path())).unwrap(),
            before
        );
    }

    #[test]
    fn a_wrong_case_locale_counts_as_stored_and_is_not_rewritten() {
        let dir = tempdir().unwrap();

        // The stored spelling is lowercase only, so "EL" is not a known value:
        // the locale loads as the default, but the key counts as chosen.
        assert!(fs::write(ui_prefs_path(dir.path()), r#"{"locale": "EL"}"#).is_ok());
        let before = fs::read_to_string(ui_prefs_path(dir.path())).unwrap();

        let resolved = resolve_locale(dir.path(), &["fr-FR"]);
        assert_eq!(
            resolved,
            Ok(LocaleResolution {
                locale: Locale::En,
                newly_stored: false
            })
        );
        assert_eq!(
            fs::read_to_string(ui_prefs_path(dir.path())).unwrap(),
            before
        );
    }

    #[test]
    fn a_save_leaves_no_temporary_file_and_the_target_parses() {
        let dir = tempdir().unwrap();

        let prefs = UiPrefs {
            locale: Locale::De,
            ..UiPrefs::default()
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());

        assert!(!ui_prefs_temporary_path(dir.path()).exists());
        let text = fs::read_to_string(ui_prefs_path(dir.path())).unwrap();
        assert_eq!(serde_json::from_str::<UiPrefs>(&text).ok(), Some(prefs));
    }

    #[test]
    fn a_stale_temporary_file_does_not_break_a_save() {
        let dir = tempdir().unwrap();

        assert!(fs::write(ui_prefs_temporary_path(dir.path()), "{\"locale\": ").is_ok());

        let prefs = UiPrefs {
            locale: Locale::Fr,
            ..UiPrefs::default()
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());

        assert!(!ui_prefs_temporary_path(dir.path()).exists());
        assert_eq!(load_ui_prefs(dir.path()), prefs);
    }

    #[test]
    fn loading_ignores_a_temporary_file() {
        let dir = tempdir().unwrap();

        let temporary = ui_prefs_temporary_path(dir.path());
        assert!(fs::write(&temporary, r#"{"locale": "de"}"#).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), UiPrefs::default());
        assert_eq!(stored_locale(dir.path()), None);

        let prefs = UiPrefs {
            locale: Locale::El,
            ..UiPrefs::default()
        };
        assert!(save_ui_prefs(dir.path(), &prefs).is_ok());
        assert!(fs::write(&temporary, r#"{"locale": "de"}"#).is_ok());
        assert_eq!(load_ui_prefs(dir.path()), prefs);
    }

    #[test]
    fn a_failed_write_leaves_no_temporary_file() {
        let dir = tempdir().unwrap();

        // A directory at the target makes the rename fail after the write.
        assert!(fs::create_dir(ui_prefs_path(dir.path())).is_ok());

        assert!(save_ui_prefs(dir.path(), &UiPrefs::default()).is_err());
        assert!(!ui_prefs_temporary_path(dir.path()).exists());
    }

    /// The book the quick-add tests remember accounts for.
    const BOOK: &str = "11111111-1111-4111-8111-111111111111";

    /// The accounts of an expense paid from `wallet`.
    fn expense_accounts(wallet: &str) -> LastRoleAccounts {
        LastRoleAccounts {
            category_account_id: Some("cat-1".into()),
            wallet_account_id: Some(wallet.into()),
            ..LastRoleAccounts::default()
        }
    }

    #[test]
    fn remembering_quick_add_stores_the_accounts_under_the_book_and_kind() {
        let dir = tempdir().unwrap();
        let book: EntityId = BOOK.parse().unwrap();

        remember_quick_add(
            dir.path(),
            book,
            SimpleEntryKind::Expense,
            expense_accounts("wal-1"),
        )
        .unwrap();

        // The keys and the shape of the file are what older builds wrote.
        let stored: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(ui_prefs_path(dir.path())).unwrap()).unwrap();
        assert_eq!(
            stored,
            serde_json::json!({
                "locale": "en",
                "last_entity_id": BOOK,
                "last_accounts_by_entity_kind": {
                    format!("{BOOK}:expense"): {
                        "category_account_id": "cat-1",
                        "wallet_account_id": "wal-1",
                        "payable_account_id": null,
                        "from_account_id": null,
                        "to_account_id": null,
                    }
                }
            })
        );
        assert!(!ui_prefs_temporary_path(dir.path()).exists());
    }

    #[test]
    fn remembering_quick_add_replaces_one_pair_and_keeps_every_other_preference() {
        let dir = tempdir().unwrap();
        let book: EntityId = BOOK.parse().unwrap();
        let other_book: EntityId = "22222222-2222-4222-8222-222222222222".parse().unwrap();
        let transfer = LastRoleAccounts {
            from_account_id: Some("from-1".into()),
            to_account_id: Some("to-1".into()),
            ..LastRoleAccounts::default()
        };
        let before = UiPrefs {
            locale: Locale::El,
            ..UiPrefs::default()
        };
        save_ui_prefs(dir.path(), &before).unwrap();

        let remember = |entity_id, kind, accounts| {
            remember_quick_add(dir.path(), entity_id, kind, accounts).unwrap();
        };
        remember(book, SimpleEntryKind::Expense, expense_accounts("wal-1"));
        remember(
            other_book,
            SimpleEntryKind::Expense,
            expense_accounts("wal-9"),
        );
        remember(book, SimpleEntryKind::Transfer, transfer.clone());
        remember(book, SimpleEntryKind::Expense, expense_accounts("wal-2"));

        let expected = UiPrefs {
            locale: Locale::El,
            last_entity_id: Some(BOOK.to_owned()),
            last_accounts_by_entity_kind: BTreeMap::from([
                (
                    last_accounts_key(book, SimpleEntryKind::Expense),
                    expense_accounts("wal-2"),
                ),
                (last_accounts_key(book, SimpleEntryKind::Transfer), transfer),
                (
                    last_accounts_key(other_book, SimpleEntryKind::Expense),
                    expense_accounts("wal-9"),
                ),
            ]),
        };
        assert_eq!(load_ui_prefs(dir.path()), expected);
    }

    #[test]
    fn remembering_quick_add_reports_a_file_it_cannot_write() {
        let dir = tempdir().unwrap();
        // A directory at the target makes the rename fail after the write.
        assert!(fs::create_dir(ui_prefs_path(dir.path())).is_ok());

        let failed = remember_quick_add(
            dir.path(),
            BOOK.parse().unwrap(),
            SimpleEntryKind::Income,
            LastRoleAccounts::default(),
        );

        assert!(
            matches!(&failed, Err(Error::Io { operation, .. }) if *operation == "write preferences file"),
            "{failed:?}"
        );
        assert!(!ui_prefs_temporary_path(dir.path()).exists());
    }
}
