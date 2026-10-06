//! Serializable errors for the web frontend.

use std::collections::BTreeMap;

use oikonomia_core::Error as CoreError;
use oikonomia_update::UpdateError;
use serde::Serialize;

/// Error payload returned from Tauri commands.
///
/// The UI shows localized text chosen by `code` and filled in from `params`.
///
/// `message` is English diagnostic text that the UI never shows. Rust does
/// not log it, and a release build has no logger in any case. It crosses IPC
/// with the rest of the error, where the frontend may write it to the webview
/// console (`logCommandError` in `web/src/lib/commandError.ts`).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandError {
    /// Stable machine code for UI branching and localized text.
    pub code: String,
    /// Human-readable message (English). May contain OS error text.
    pub message: String,
    /// Named values for the localized text. Empty when there are none; never
    /// holds OS error text.
    pub params: BTreeMap<String, String>,
}

/// Failures that only the desktop shell can produce (dialogs, files, tasks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopError {
    /// File contents sent from the webview were not valid base64.
    FileDataInvalid,
    /// A file the user picked or dropped could not be read.
    FileUnreadable,
    /// The native dialog returned a location that is not a usable path.
    SaveLocationInvalid,
    /// Writing a file to the chosen location failed.
    SaveFailed,
    /// The webview named a path the user never chose in a native dialog.
    PathNotGranted,
    /// The system could not open the default mail client.
    MailClientFailed,
    /// A background task panicked or was cancelled.
    TaskFailed,
}

impl DesktopError {
    /// Every desktop error, so a test can check each code has UI copy.
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

    /// Stable `snake_case` identifier the UI maps to localized text.
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

impl CommandError {
    /// Build the error for a desktop-only failure.
    ///
    /// `message` is English and may include the OS error. The UI must not
    /// show it, so it is never copied into `params`.
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
        // A validation error also names the values its copy fills in.
        let params = match &value {
            CoreError::Validation(reason) => reason
                .params()
                .into_iter()
                .map(|(name, text)| (name.to_owned(), text))
                .collect(),
            _ => BTreeMap::new(),
        };

        Self {
            code: value.code().to_owned(),
            message: value.to_string(),
            params,
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

/// Command result alias.
pub(crate) type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use oikonomia_core::Error as CoreError;
    use oikonomia_core::error::{AccountRole, ValidationError};
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

    /// Fails unless `DesktopError::ALL` is exactly the set of variants in the
    /// `listed_desktop_errors` list above, each once. The compiler checks that
    /// list against the enum with an exhaustive `match`, so a variant added to
    /// the enum but left out of the list does not compile. It does not check
    /// the order of `ALL`, nor that the UI has copy for the code;
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

    /// Every code Rust can send to the UI, built from the enumerations.
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
    fn every_desktop_code_is_distinct_snake_case() {
        let codes: BTreeSet<&str> = DesktopError::ALL.iter().map(|kind| kind.code()).collect();

        assert_eq!(codes.len(), DesktopError::ALL.len());
        for code in codes {
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{code}"
            );
        }
    }

    #[test]
    fn an_update_error_keeps_its_specific_code() {
        let error = CommandError::from(UpdateError::Network);

        assert_eq!(error.code, "update_network");
        assert_eq!(error.params, BTreeMap::new());
    }
}
