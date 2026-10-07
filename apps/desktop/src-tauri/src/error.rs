//! Errors as the web frontend receives them: a code plus parameters.
//!
//! The UI never shows text written in Rust. Every error that crosses IPC is a
//! [`CommandError`]: a stable `code` the UI maps to localized copy, `params`
//! that fill the placeholders of that copy, and an English `message` that
//! only ever reaches the webview console. `web/src/lib/errorCodes.json` lists
//! every code, and a test here fails when that list and the Rust enumerations
//! differ.
//!
//! # Where the codes come from
//!
//! - **Core.** `oikonomia_core::Error` names its own code. A broken rule is
//!   its `Validation` variant and an unreadable CSV its `Csv` variant; the
//!   `ValidationError` or `CsvError` inside names a code of its own and the
//!   parameters that go with it. Converted with `From`, so a command
//!   propagates a core error with `?`.
//! - **The update crate.** `oikonomia_update::UpdateError` names its own
//!   code and has no parameters. Converted with `From` as well.
//! - **The shell.** [`DesktopError`] is the failures only the shell can
//!   produce: dialogs, files the user picked, the blocking pool. Built with
//!   [`CommandError::desktop`].
//!
//! # Parameters
//!
//! Core decides them. `oikonomia_core::Error::params` sits beside `code` and
//! matches every variant without a wildcard arm, so the conversion here
//! copies what it returns and chooses nothing. A new core variant that
//! carries data does not compile in core until its parameters are decided
//! there.
//!
//! The lower-level text of a failure (the `detail` of `database`, `io`,
//! `serialization`, `crypto`, `analysis`, `csv_parse` and
//! `validation_internal`, and the reason of `vault_corrupt`,
//! `vault_unlock_before_backup` and `backup_invalid`) is diagnostic, may
//! hold operating-system error text, and is never a parameter. It reaches
//! the webview only in `message`.
//!
//! # Codes used more broadly than their name
//!
//! Three codes are sent for conditions their name does not cover. In each
//! case no existing code fits better, and a new code is a change to the
//! contract with the frontend, so they are recorded here instead.
//!
//! - `save_location_invalid` is also sent by the open dialogs (a backup to
//!   restore, a CSV to import), not only by save dialogs. The copy, "Could
//!   not use that location. Choose a different one.", reads correctly for
//!   both.
//! - `task_failed` is documented as a background task that panicked or was
//!   cancelled. It is also sent when the system names no cache directory for
//!   a downloaded update (`crate::update`), and when a preferences command
//!   runs after a failed start, with no application state
//!   (`crate::commands`). The copy asks the user to try again, which helps in
//!   neither case.
//! - `update_artifact_integrity`, worded in the update crate as "failed
//!   verification", is returned by `crate::update_exec` for every failure of
//!   the install step, which runs after verification has passed: the
//!   download is no longer there, a copy, permission change, rename or
//!   unpack fails, the installer cannot be started, or the running copy
//!   cannot find its own executable or bundle. The UI gives every update
//!   code the same sentence, so the user sees no difference; what is lost is
//!   the distinction in the diagnostics.

use oikonomia_core::Error as CoreError;
use oikonomia_update::UpdateError;
use serde::Serialize;
use std::collections::BTreeMap;

/// The error payload a Tauri command returns to the webview.
///
/// The UI shows localized text chosen by `code` and filled in from `params`.
///
/// `message` is English diagnostic text that the UI never shows. Rust does
/// not log it, and a release build has no logger in any case. It crosses IPC
/// with the rest of the error, where the frontend may write it to the webview
/// console (`logCommandError` in `web/src/lib/commandError.ts`).
///
/// The serialized shape is `{ code, message, params }`, which the frontend's
/// type of the same name mirrors (`web/src/lib/tauri.ts`).
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub(crate) struct CommandError {
    /// The stable machine code the UI branches on and words the error from.
    pub code: String,
    /// English diagnostic text. May contain operating-system error text.
    pub message: String,
    /// Named values for the localized text. Empty when there are none; never
    /// holds operating-system error text.
    pub params: BTreeMap<String, String>,
}

/// The result every fallible command returns.
pub(crate) type CommandResult<T> = Result<T, CommandError>;

impl CommandError {
    /// Builds the error for a failure only the shell can produce.
    ///
    /// `message` is English and may include the operating-system error. The UI
    /// must not show it, so it is never copied into `params`.
    #[must_use]
    pub(crate) fn desktop(kind: DesktopError, message: impl Into<String>) -> Self {
        Self {
            code: kind.code().to_owned(),
            message: message.into(),
            params: BTreeMap::new(),
        }
    }
}

impl From<CoreError> for CommandError {
    fn from(value: CoreError) -> Self {
        Self {
            code: value.code().to_owned(),
            message: value.to_string(),
            params: value
                .params()
                .into_iter()
                .map(|(name, text)| (name.to_owned(), text))
                .collect(),
        }
    }
}

impl From<UpdateError> for CommandError {
    fn from(value: UpdateError) -> Self {
        Self {
            code: value.code().to_owned(),
            message: value.to_string(),
            params: BTreeMap::new(),
        }
    }
}

/// A failure that only the desktop shell can produce (dialogs, files, tasks).
///
/// Each variant is one code. Its `Display` text leads with that code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum DesktopError {
    /// File contents sent from the webview were not valid base64.
    #[error("file_data_invalid: file data from the webview is not valid base64")]
    FileDataInvalid,
    /// A file the user picked or dropped could not be read.
    #[error("file_unreadable: a picked or dropped file cannot be read")]
    FileUnreadable,
    /// The native dialog returned a location that is not a usable path.
    #[error("save_location_invalid: the dialog returned a location that is not a path")]
    SaveLocationInvalid,
    /// Writing a file to the chosen location failed.
    #[error("save_failed: cannot write the file to the chosen location")]
    SaveFailed,
    /// The webview named a path the user never chose in a native dialog.
    #[error("path_not_granted: the path was not chosen through the app")]
    PathNotGranted,
    /// The system could not open the default mail client.
    #[error("mail_client_failed: cannot open the default mail client")]
    MailClientFailed,
    /// A background task panicked or was cancelled.
    #[error("task_failed: a background task panicked or was cancelled")]
    TaskFailed,
}

impl DesktopError {
    /// Every desktop error, so that a test can check each code has UI copy.
    #[cfg(test)]
    pub(crate) const ALL: &'static [Self] = &[
        Self::FileDataInvalid,
        Self::FileUnreadable,
        Self::SaveLocationInvalid,
        Self::SaveFailed,
        Self::PathNotGranted,
        Self::MailClientFailed,
        Self::TaskFailed,
    ];

    /// Returns the stable `snake_case` identifier the UI maps to localized text.
    #[must_use]
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::FileDataInvalid => "file_data_invalid",
            Self::FileUnreadable => "file_unreadable",
            Self::SaveLocationInvalid => "save_location_invalid",
            Self::SaveFailed => "save_failed",
            Self::PathNotGranted => "path_not_granted",
            Self::MailClientFailed => "mail_client_failed",
            Self::TaskFailed => "task_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use oikonomia_core::Error as CoreError;
    use oikonomia_core::csv::CsvError;
    use oikonomia_core::error::{
        AccountRole, BackupDefect, Resource, ValidationError, VaultCorruption,
    };
    use oikonomia_update::UpdateError;

    use super::{CommandError, DesktopError};
    use oikonomia_test_support::listed_variants;

    listed_variants! {
        units listed_desktop_errors for DesktopError {
            DesktopError::FileDataInvalid,
            DesktopError::FileUnreadable,
            DesktopError::SaveLocationInvalid,
            DesktopError::SaveFailed,
            DesktopError::PathNotGranted,
            DesktopError::MailClientFailed,
            DesktopError::TaskFailed,
        }
    }

    /// Fails unless `DesktopError::ALL` is exactly the listed variants, each
    /// once.
    ///
    /// The compiler checks the `listed_desktop_errors` list above against the
    /// enum with an exhaustive `match`, so a variant added to the enum but left
    /// out of the list does not compile. This does not check the order of `ALL`,
    /// nor that the UI has copy for the code;
    /// `the_shared_fixture_lists_exactly_the_codes_rust_can_emit` does that.
    #[test]
    fn all_lists_every_desktop_variant() {
        let listed = listed_desktop_errors::variants();

        assert_eq!(
            DesktopError::ALL.len(),
            listed_desktop_errors::COUNT,
            "DesktopError::ALL and the listed variants differ in number"
        );
        for variant in listed {
            assert!(
                DesktopError::ALL.contains(&variant),
                "{variant:?} is missing from DesktopError::ALL"
            );
        }
        listed_desktop_errors::assert_every_position_once(
            DesktopError::ALL
                .iter()
                .map(listed_desktop_errors::position)
                .collect(),
        );
    }

    #[test]
    fn the_role_fixture_lists_exactly_the_roles_rust_can_name() {
        let fixture: Vec<String> =
            serde_json::from_str(include_str!("../../../../web/src/lib/accountRoles.json"))
                .expect("accountRoles.json");
        let listed: Vec<&str> = AccountRole::ALL
            .iter()
            .map(|role| role.identifier())
            .collect();

        assert_eq!(
            fixture, listed,
            "accountRoles.json and AccountRole::ALL differ"
        );
    }

    /// Returns every code Rust can send to the UI, built from the enumerations.
    fn every_code() -> BTreeSet<String> {
        let mut codes: BTreeSet<String> = CoreError::ALL_CODES
            .iter()
            .map(|code| (*code).to_owned())
            .collect();

        codes.extend(
            ValidationError::ALL_CODES
                .iter()
                .map(|code| (*code).to_owned()),
        );
        codes.extend(CsvError::ALL_CODES.iter().map(|code| (*code).to_owned()));
        codes.extend(DesktopError::ALL.iter().map(|kind| kind.code().to_owned()));
        codes.extend(UpdateError::ALL_CODES.iter().map(|code| (*code).to_owned()));

        // The catch-all the web layer shows when it meets a code it has no
        // copy for. No Rust path sends it: core's codes come from an
        // exhaustive match.
        codes.insert("unknown".to_owned());

        codes
    }

    #[test]
    fn the_shared_fixture_lists_exactly_the_codes_rust_can_emit() {
        let fixture: Vec<String> =
            serde_json::from_str(include_str!("../../../../web/src/lib/errorCodes.json"))
                .expect("errorCodes.json");
        let listed: BTreeSet<String> = fixture.iter().cloned().collect();

        assert_eq!(
            listed.len(),
            fixture.len(),
            "duplicate code in errorCodes.json"
        );
        assert_eq!(
            every_code(),
            listed,
            "errorCodes.json and the Rust enumerations differ"
        );
    }

    #[test]
    fn a_validation_error_carries_its_own_code_and_params() {
        let error = CommandError::from(CoreError::Validation(ValidationError::PasswordTooShort {
            min: 12,
        }));

        assert_eq!(error.code, "password_too_short");
        assert_eq!(error.message, "password must be at least 12 characters");
        assert_eq!(
            serde_json::to_value(&error).expect("serialize"),
            serde_json::json!({
                "code": "password_too_short",
                "message": "password must be at least 12 characters",
                "params": { "min": "12" },
            })
        );
    }

    #[test]
    fn an_error_without_params_serializes_an_empty_object() {
        let error = CommandError::from(CoreError::VaultLocked);

        assert_eq!(
            serde_json::to_value(&error).expect("serialize"),
            serde_json::json!({
                "code": "vault_locked",
                "message": "vault is locked",
                "params": {},
            })
        );
    }

    #[test]
    fn a_desktop_error_keeps_its_message_out_of_the_params() {
        let error = CommandError::desktop(DesktopError::SaveFailed, "could not save file: denied");

        assert_eq!(error.code, "save_failed");
        assert_eq!(error.message, "could not save file: denied");
        assert_eq!(error.params, BTreeMap::new());
    }

    #[test]
    fn a_desktop_error_displays_its_code_and_a_lowercase_message() {
        for kind in DesktopError::ALL {
            let shown = kind.to_string();
            let (code, message) = shown.split_once(": ").expect("code: message");

            assert_eq!(code, kind.code(), "{kind:?}");
            assert!(!message.is_empty(), "{kind:?}");
            assert_eq!(message, message.to_lowercase(), "{kind:?}");
            assert!(!message.ends_with('.'), "{kind:?}");
        }
    }

    #[test]
    fn a_command_error_displays_its_code_and_message_and_is_an_error() {
        let error = CommandError::desktop(DesktopError::SaveFailed, "could not save file: denied");

        assert_eq!(
            error.to_string(),
            "save_failed: could not save file: denied"
        );
        // Neither type has a lower-level cause to report.
        assert!(std::error::Error::source(&error).is_none());
        assert!(std::error::Error::source(&DesktopError::SaveFailed).is_none());
    }

    #[test]
    fn every_desktop_code_is_distinct_snake_case() {
        let codes: BTreeSet<&str> = DesktopError::ALL.iter().map(|kind| kind.code()).collect();

        assert_eq!(codes.len(), DesktopError::ALL.len());
        for code in codes {
            assert!(
                code.chars()
                    .all(|letter| letter.is_ascii_lowercase() || letter == '_'),
                "{code}"
            );
        }
    }

    /// The text every diagnostic field in `core_errors` carries.
    const DIAGNOSTIC_TEXT: &str = "disk on fire";

    /// One value of every core variant other than `Validation` and `Csv`,
    /// and a second `VaultCorrupt` for the code of the reasons one unlock
    /// settles, with the names of the parameters it sends.
    ///
    /// `every_core_variant_is_listed_with_its_parameters` checks the list
    /// against `CoreError::ALL_CODES`, which core's own tests tie to the
    /// variants.
    fn core_errors() -> Vec<(CoreError, &'static [&'static str])> {
        let text = || DIAGNOSTIC_TEXT.to_owned();
        let failure = |build: fn(&'static str, String) -> CoreError| build("write", text());

        vec![
            (CoreError::VaultUninitialized, &[]),
            (CoreError::VaultLocked, &[]),
            (CoreError::InvalidPassword, &[]),
            (
                CoreError::UnbalancedEntry {
                    debits: 100,
                    credits: 50,
                },
                &["credits", "debits"],
            ),
            (CoreError::TooFewLines, &[]),
            (CoreError::InvalidLineAmounts, &[]),
            (CoreError::AccountWrongEntity, &[]),
            (CoreError::MoneyOverflow, &[]),
            (CoreError::NegativeMoney, &[]),
            (
                failure(|operation, detail| CoreError::Database { operation, detail }),
                &["operation"],
            ),
            (
                failure(|operation, detail| CoreError::Io { operation, detail }),
                &["operation"],
            ),
            (
                failure(|operation, detail| CoreError::Serialization { operation, detail }),
                &["operation"],
            ),
            (
                failure(|operation, detail| CoreError::Crypto { operation, detail }),
                &["operation"],
            ),
            (
                CoreError::VaultCorrupt(VaultCorruption::HeaderUnreadable { detail: text() }),
                &[],
            ),
            (
                CoreError::VaultCorrupt(VaultCorruption::UnfinishedPasswordChange),
                &[],
            ),
            (
                CoreError::VaultTooNew {
                    found: 8,
                    supported: 7,
                },
                &["found", "supported"],
            ),
            (
                CoreError::BackupInvalid(BackupDefect::EmptyMember { name: text() }),
                &[],
            ),
            (CoreError::RestoreWouldOverwrite, &[]),
            (CoreError::NotFound(Resource::Account), &["resource"]),
            (
                failure(|operation, detail| CoreError::Analysis { operation, detail }),
                &["operation"],
            ),
        ]
    }

    /// Fails when core gains a variant this list does not have, and when a
    /// variant stops sending a parameter the list names or starts sending one
    /// it does not. There are no known gaps: every variant that carries a
    /// value the copy could use sends it.
    #[test]
    fn every_core_variant_is_listed_with_its_parameters() {
        let listed: Vec<&str> = core_errors()
            .iter()
            .map(|(error, _)| error.code())
            .collect();

        assert_eq!(
            listed,
            CoreError::ALL_CODES,
            "core_errors() and CoreError::ALL_CODES differ: list the new variant"
        );

        for (error, expected) in core_errors() {
            let sent = CommandError::from(error.clone()).params;
            let names: Vec<&str> = sent.keys().map(String::as_str).collect();

            assert_eq!(names, expected, "{error:?}");
        }
    }

    /// Checked on the serialized error, as the webview receives it: the
    /// lower-level text is in `message` and nowhere in `params`.
    #[test]
    fn a_core_error_keeps_its_diagnostic_text_out_of_the_parameters() {
        for (error, _) in core_errors() {
            let sent = serde_json::to_value(CommandError::from(error.clone())).expect("serialize");

            assert!(
                !sent["params"].to_string().contains(DIAGNOSTIC_TEXT),
                "{error:?} leaks its text into the parameters"
            );
            if format!("{error:?}").contains(DIAGNOSTIC_TEXT) {
                let message = sent["message"].as_str().expect("message");
                assert!(message.contains(DIAGNOSTIC_TEXT), "{error:?}: {message}");
            }
        }
    }

    #[test]
    fn an_unbalanced_entry_sends_both_totals() {
        let error = CommandError::from(CoreError::UnbalancedEntry {
            debits: 100,
            credits: 50,
        });

        assert_eq!(
            serde_json::to_value(&error).expect("serialize")["params"],
            serde_json::json!({ "credits": "50", "debits": "100" })
        );
    }

    #[test]
    fn a_missing_record_sends_what_was_not_found() {
        let error = CommandError::from(CoreError::NotFound(Resource::JournalEntry));

        assert_eq!(error.code, "not_found");
        assert_eq!(
            error.params,
            BTreeMap::from([("resource".to_owned(), "journal_entry".to_owned())])
        );
    }

    #[test]
    fn a_csv_error_carries_its_own_code_and_params() {
        let error = CommandError::from(CoreError::from(CsvError::InvalidDate("31/31".into())));

        assert_eq!(error.code, "csv_invalid_date");
        assert_eq!(
            error.params,
            BTreeMap::from([("value".to_owned(), "31/31".to_owned())])
        );
    }

    #[test]
    fn an_update_error_keeps_its_specific_code() {
        let error = CommandError::from(UpdateError::Network);

        assert_eq!(error.code, "update_network");
        assert_eq!(error.params, BTreeMap::new());
    }
}
