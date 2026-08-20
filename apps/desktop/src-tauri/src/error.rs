//! Serializable errors for the web frontend.

use oikonomia_core::Error as CoreError;
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
            CoreError::LicenseInvalid => "license_invalid",
            CoreError::LicenseExpired => "license_expired",
            _ => "unknown",
        };

        Self {
            code: code.to_owned(),
            message: value.to_string(),
        }
    }
}

/// Command result alias.
pub type CommandResult<T> = Result<T, CommandError>;
