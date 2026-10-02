//! Typed errors for the core library.

use thiserror::Error;

mod validation;

pub use validation::{AccountRole, ValidationError};

/// Fallible operations in `oikonomia-core`.
pub type Result<T> = std::result::Result<T, Error>;

/// Domain and vault errors returned to the application layer.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Vault has not been created yet.
    #[error("vault is not initialized")]
    VaultUninitialized,

    /// Vault exists but is locked.
    #[error("vault is locked")]
    VaultLocked,

    /// Master password rejected or key derivation failed to open the vault.
    #[error("could not unlock vault")]
    InvalidPassword,

    /// Journal lines do not balance.
    #[error("journal entry is unbalanced: debits {debits} != credits {credits}")]
    UnbalancedEntry {
        /// Sum of debit minor units.
        debits: i64,
        /// Sum of credit minor units.
        credits: i64,
    },

    /// Posted entry must have at least two lines.
    #[error("journal entry needs at least two lines")]
    TooFewLines,

    /// A journal line must be debit XOR credit (non-zero on exactly one side).
    #[error("journal line must have debit or credit, not both or neither")]
    InvalidLineAmounts,

    /// Account does not belong to the entry's entity.
    #[error("account does not belong to this entity")]
    AccountWrongEntity,

    /// Money arithmetic overflowed.
    #[error("money amount overflow")]
    MoneyOverflow,

    /// Money amount was negative where non-negative is required.
    #[error("money amount must be non-negative")]
    NegativeMoney,

    /// A rule the caller broke; the typed reason carries a stable code and
    /// parameters so the UI can show localized text.
    #[error("{0}")]
    Validation(ValidationError),

    /// Filesystem or database I/O failure.
    #[error("I/O error: {0}")]
    Io(String),

    /// Cryptographic operation failed (KDF, parameters).
    #[error("crypto error: {0}")]
    Crypto(String),

    /// On-disk vault header or database structure is invalid.
    #[error("vault is corrupt: {0}")]
    VaultCorrupt(String),

    /// Portable vault backup is not a valid Oikonomia archive.
    #[error("backup is invalid: {0}")]
    BackupInvalid(String),

    /// Restore refused because vault files already exist and `replace` was false.
    #[error("a vault already exists; restore requires replace")]
    RestoreWouldOverwrite,

    /// Requested resource does not exist.
    #[error("{0} not found")]
    NotFound(String),

    /// Document analysis backend unavailable or failed.
    #[error("analysis failed: {0}")]
    Analysis(String),

    /// Bank CSV or journal CSV could not be parsed.
    #[error("{0}")]
    CsvParse(String),
}

impl Error {
    /// Every code [`Error::code`] returns for a variant other than
    /// [`Error::Validation`], whose codes are [`ValidationError::ALL_CODES`].
    ///
    /// The desktop crate checks this list against `errorCodes.json`.
    pub const ALL_CODES: &'static [&'static str] = &[
        "vault_uninitialized",
        "vault_locked",
        "invalid_password",
        "unbalanced_entry",
        "too_few_lines",
        "invalid_line_amounts",
        "account_wrong_entity",
        "money_overflow",
        "negative_money",
        "io",
        "crypto",
        "vault_corrupt",
        "backup_invalid",
        "restore_would_overwrite",
        "not_found",
        "analysis",
        "csv_parse",
    ];

    /// Stable `snake_case` identifier the UI maps to localized text.
    ///
    /// A validation error names its own code. The match has no wildcard arm,
    /// so a new variant does not compile until it has a code here.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::VaultUninitialized => "vault_uninitialized",
            Self::VaultLocked => "vault_locked",
            Self::InvalidPassword => "invalid_password",
            Self::UnbalancedEntry { .. } => "unbalanced_entry",
            Self::TooFewLines => "too_few_lines",
            Self::InvalidLineAmounts => "invalid_line_amounts",
            Self::AccountWrongEntity => "account_wrong_entity",
            Self::MoneyOverflow => "money_overflow",
            Self::NegativeMoney => "negative_money",
            Self::Validation(reason) => reason.code(),
            Self::Io(_) => "io",
            Self::Crypto(_) => "crypto",
            Self::VaultCorrupt(_) => "vault_corrupt",
            Self::BackupInvalid(_) => "backup_invalid",
            Self::RestoreWouldOverwrite => "restore_would_overwrite",
            Self::NotFound(_) => "not_found",
            Self::Analysis(_) => "analysis",
            Self::CsvParse(_) => "csv_parse",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, ValidationError};
    use crate::test_macros::listed_variants;

    /// One value of every variant, in the order of the enum.
    fn every_variant() -> Vec<Error> {
        vec![
            Error::VaultUninitialized,
            Error::VaultLocked,
            Error::InvalidPassword,
            Error::UnbalancedEntry {
                debits: 100,
                credits: 50,
            },
            Error::TooFewLines,
            Error::InvalidLineAmounts,
            Error::AccountWrongEntity,
            Error::MoneyOverflow,
            Error::NegativeMoney,
            Error::Validation(ValidationError::SameAccount),
            Error::Io("x".into()),
            Error::Crypto("x".into()),
            Error::VaultCorrupt("x".into()),
            Error::BackupInvalid("x".into()),
            Error::RestoreWouldOverwrite,
            Error::NotFound("x".into()),
            Error::Analysis("x".into()),
            Error::CsvParse("x".into()),
        ]
    }

    listed_variants! {
        patterns listed_errors for Error {
            Error::VaultUninitialized,
            Error::VaultLocked,
            Error::InvalidPassword,
            Error::UnbalancedEntry { .. },
            Error::TooFewLines,
            Error::InvalidLineAmounts,
            Error::AccountWrongEntity,
            Error::MoneyOverflow,
            Error::NegativeMoney,
            Error::Validation(_),
            Error::Io(_),
            Error::Crypto(_),
            Error::VaultCorrupt(_),
            Error::BackupInvalid(_),
            Error::RestoreWouldOverwrite,
            Error::NotFound(_),
            Error::Analysis(_),
            Error::CsvParse(_),
        }
    }

    /// Fails when `every_variant` has no sample for a variant named in the
    /// `listed_errors` list above, or when `ALL_CODES` is not the codes of the
    /// samples other than the validation one, in order. The compiler checks
    /// `listed_errors` against the enum with an exhaustive `match`, so a
    /// variant added to the enum but not to that list does not compile. It
    /// does not check that the UI has copy for a code; the desktop crate does.
    #[test]
    fn all_codes_lists_the_code_of_every_variant_but_validation() {
        let samples = every_variant();
        let codes: Vec<&str> = samples
            .iter()
            .filter(|sample| !matches!(sample, Error::Validation(_)))
            .map(Error::code)
            .collect();

        listed_errors::assert_every_position_once(
            samples.iter().map(listed_errors::position).collect(),
        );
        assert_eq!(codes, Error::ALL_CODES);
    }

    #[test]
    fn a_validation_error_gives_its_own_code() {
        assert_eq!(
            Error::Validation(ValidationError::SameAccount).code(),
            "same_account"
        );
    }

    #[test]
    fn every_code_is_snake_case() {
        for code in Error::ALL_CODES {
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{code}"
            );
        }
    }
}
