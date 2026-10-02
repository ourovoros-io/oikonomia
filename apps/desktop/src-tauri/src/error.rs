//! Serializable errors for the web frontend.

use std::collections::BTreeMap;

use oikonomia_core::Error as CoreError;
use oikonomia_update::UpdateError;
use serde::Serialize;

/// Error payload returned from Tauri commands.
///
/// The UI shows localized text chosen by `code` and filled in from `params`.
/// `message` is English, for logs and as a last resort when a code has no copy.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
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
pub enum DesktopError {
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
    pub fn code(self) -> &'static str {
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
    /// `message` is English and may include the OS error; it is for logs and
    /// as a fallback, so it is never copied into `params`.
    #[must_use]
    pub fn desktop(kind: DesktopError, message: impl Into<String>) -> Self {
        Self {
            code: kind.code().to_owned(),
            message: message.into(),
            params: BTreeMap::new(),
        }
    }
}

impl From<CoreError> for CommandError {
    fn from(value: CoreError) -> Self {
        // A validation error names its own specific code and parameters.
        if let CoreError::Validation(reason) = &value {
            return Self {
                code: reason.code().to_owned(),
                message: value.to_string(),
                params: reason
                    .params()
                    .into_iter()
                    .map(|(name, text)| (name.to_owned(), text))
                    .collect(),
            };
        }

        let code = match &value {
            CoreError::VaultUninitialized => "vault_uninitialized",
            CoreError::VaultLocked => "vault_locked",
            CoreError::InvalidPassword => "invalid_password",
            CoreError::UnbalancedEntry { .. } => "unbalanced_entry",
            CoreError::TooFewLines => "too_few_lines",
            CoreError::InvalidLineAmounts => "invalid_line_amounts",
            CoreError::AccountWrongEntity => "account_wrong_entity",
            CoreError::MoneyOverflow => "money_overflow",
            CoreError::NegativeMoney => "negative_money",
            CoreError::Io(_) => "io",
            CoreError::Crypto(_) => "crypto",
            CoreError::VaultCorrupt(_) => "vault_corrupt",
            CoreError::BackupInvalid(_) => "backup_invalid",
            CoreError::RestoreWouldOverwrite => "restore_would_overwrite",
            CoreError::NotFound(_) => "not_found",
            CoreError::Analysis(_) => "analysis",
            CoreError::CsvParse(_) => "csv_parse",
            _ => "unknown",
        };

        Self {
            code: code.to_owned(),
            message: value.to_string(),
            params: BTreeMap::new(),
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
pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use oikonomia_core::Error as CoreError;
    use oikonomia_core::error::{AccountRole, ValidationError};
    use oikonomia_update::UpdateError;

    use super::{CommandError, DesktopError};

    /// One value of every desktop variant. The `match` makes a new variant a
    /// compile error here until it is given a sample, and
    /// `all_lists_every_desktop_variant` then fails until it is in
    /// [`DesktopError::ALL`]. It does not check that the UI has copy for the
    /// code; `the_shared_fixture_lists_exactly_the_codes_rust_can_emit` does.
    fn every_desktop_variant() -> Vec<DesktopError> {
        let samples = vec![
            DesktopError::FileDataInvalid,
            DesktopError::FileUnreadable,
            DesktopError::SaveLocationInvalid,
            DesktopError::SaveFailed,
            DesktopError::PathNotGranted,
            DesktopError::MailClientFailed,
            DesktopError::TaskFailed,
        ];

        for sample in &samples {
            match sample {
                DesktopError::FileDataInvalid
                | DesktopError::FileUnreadable
                | DesktopError::SaveLocationInvalid
                | DesktopError::SaveFailed
                | DesktopError::PathNotGranted
                | DesktopError::MailClientFailed
                | DesktopError::TaskFailed => {}
            }
        }

        samples
    }

    #[test]
    fn all_lists_every_desktop_variant() {
        let from_variants: BTreeSet<&str> = every_desktop_variant()
            .iter()
            .map(|kind| kind.code())
            .collect();
        let listed: BTreeSet<&str> = DesktopError::ALL.iter().map(|kind| kind.code()).collect();

        assert_eq!(from_variants, listed);
        assert_eq!(listed.len(), DesktopError::ALL.len(), "duplicate variant");
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

    /// One value of each core variant except `Validation`, whose codes come
    /// from [`ValidationError::ALL_CODES`].
    fn non_validation_core_samples() -> Vec<CoreError> {
        vec![
            CoreError::VaultUninitialized,
            CoreError::VaultLocked,
            CoreError::InvalidPassword,
            CoreError::UnbalancedEntry {
                debits: 100,
                credits: 50,
            },
            CoreError::TooFewLines,
            CoreError::InvalidLineAmounts,
            CoreError::AccountWrongEntity,
            CoreError::MoneyOverflow,
            CoreError::NegativeMoney,
            CoreError::Io("x".into()),
            CoreError::Crypto("x".into()),
            CoreError::VaultCorrupt("x".into()),
            CoreError::BackupInvalid("x".into()),
            CoreError::RestoreWouldOverwrite,
            CoreError::NotFound("x".into()),
            CoreError::Analysis("x".into()),
            CoreError::CsvParse("x".into()),
        ]
    }

    /// Every code Rust can send to the UI, built from the enumerations.
    fn every_code() -> BTreeSet<String> {
        let mut codes: BTreeSet<String> = non_validation_core_samples()
            .into_iter()
            .map(|sample| CommandError::from(sample).code)
            .collect();

        codes.extend(
            ValidationError::ALL_CODES
                .iter()
                .map(|code| (*code).to_owned()),
        );
        codes.extend(DesktopError::ALL.iter().map(|kind| kind.code().to_owned()));
        codes.extend(UpdateError::ALL_CODES.iter().map(|code| (*code).to_owned()));

        // Reached only through the wildcard arm for a variant added to core
        // after this crate was written.
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
