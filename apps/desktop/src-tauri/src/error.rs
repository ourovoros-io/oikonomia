//! Serializable errors for the web frontend.

use oikonomia_core::Error as CoreError;
use oikonomia_update::UpdateError;
use serde::Serialize;

/// Error payload returned from Tauri commands.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    /// Stable machine code for UI branching.
    pub code: String,
    /// Human-readable message (English).
    pub message: String,
}

impl From<CoreError> for CommandError {
    fn from(value: CoreError) -> Self {
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
            CoreError::Validation(_) => "validation",
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
        }
    }
}

impl From<UpdateError> for CommandError {
    fn from(value: UpdateError) -> Self {
        Self {
            code: value.code().to_owned(),
            message: value.to_string(),
        }
    }
}

/// Command result alias.
pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use oikonomia_core::Error as CoreError;

    #[test]
    fn every_emitted_code_is_in_the_shared_fixture() {
        let fixture: Vec<String> =
            serde_json::from_str(include_str!("../../../../web/src/lib/errorCodes.json"))
                .expect("errorCodes.json");
        let samples: Vec<CoreError> = vec![
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
            CoreError::Validation("x".into()),
            CoreError::Io("x".into()),
            CoreError::Crypto("x".into()),
            CoreError::VaultCorrupt("x".into()),
            CoreError::BackupInvalid("x".into()),
            CoreError::RestoreWouldOverwrite,
            CoreError::NotFound("x".into()),
            CoreError::Analysis("x".into()),
            CoreError::CsvParse("x".into()),
        ];
        for sample in samples {
            let code = super::CommandError::from(sample).code;
            assert!(
                fixture.contains(&code),
                "code {code} missing from errorCodes.json"
            );
        }
    }
}
