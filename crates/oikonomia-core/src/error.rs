//! The error type of the crate's public API.
//!
//! Every fallible public function returns [`enum@Error`], except the serde
//! hooks, which return the serializer's or deserializer's own error.
//!
//! # Shape
//!
//! [`enum@Error`] is a public enum because the desktop shell branches on it, and
//! it has two layers:
//!
//! - Variants of [`enum@Error`] itself say that an operation failed: the vault is
//!   locked, an entry does not balance, a file could not be written.
//! - [`Error::Validation`] wraps a [`ValidationError`], the reasons a request
//!   was refused because the caller broke a rule. They are kept in their own
//!   enum because they alone carry parameters for the UI.
//!
//! # Codes, not sentences
//!
//! The UI never shows text written here. Each error has a stable
//! `snake_case` code ([`Error::code`]) that the UI maps to wording in the
//! user's language, and a validation error adds named values for that wording
//! ([`ValidationError::params`]). The `Display` text is English diagnostic
//! text. The desktop shell passes it along beside the code, and the UI never
//! shows it.
//!
//! Adding a variant therefore means adding a code. [`Error::code`] and
//! [`ValidationError::code`] match without a wildcard arm, so a variant
//! without a code does not compile, and the tests below fail when
//! [`Error::ALL_CODES`] or [`ValidationError::ALL_CODES`] falls out of step
//! with the variants.
//!
//! # Sources are flattened to text
//!
//! [`enum@Error`] is `Clone + PartialEq + Eq` so that tests can compare whole
//! results, and it holds no `rusqlite`, `std::io` or other foreign error
//! type (`std::io::Error`, for one, is neither `Clone` nor `PartialEq`). The
//! price is that a lower-level error is kept as its message, in the `String`
//! of variants such as [`Error::Io`], and not as a
//! [`source`](std::error::Error::source): no variant has one.

use thiserror::Error;

mod validation;

pub use validation::{AccountRole, ValidationError};

/// The result of a fallible operation in `oikonomia-core`.
pub type Result<T> = std::result::Result<T, Error>;

/// A failure of an operation in `oikonomia-core`.
///
/// See the [module documentation](self) for how the variants are organised
/// and how they reach the user. A `String` payload is English detail that
/// appears in the `Display` text; the UI words the error from
/// [`Error::code`] and does not show it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    /// No vault exists in the data directory yet.
    #[error("vault is not initialized")]
    VaultUninitialized,

    /// The vault exists but has not been unlocked, so it has no open database.
    #[error("vault is locked")]
    VaultLocked,

    /// The password did not open the vault.
    #[error("could not unlock vault")]
    InvalidPassword,

    /// The lines of a journal entry do not balance.
    #[error("journal entry is unbalanced: debits {debits} != credits {credits}")]
    UnbalancedEntry {
        /// Sum of the debit amounts, in minor units.
        debits: i64,
        /// Sum of the credit amounts, in minor units.
        credits: i64,
    },

    /// A journal entry has fewer than the two lines double entry needs.
    #[error("journal entry needs at least two lines")]
    TooFewLines,

    /// A journal line has an amount on both sides or on neither.
    #[error("journal line must have debit or credit, not both or neither")]
    InvalidLineAmounts,

    /// An account belongs to a different entity than the entry or request.
    #[error("account does not belong to this entity")]
    AccountWrongEntity,

    /// An amount or a total does not fit in `i64` minor units.
    #[error("money amount overflow")]
    MoneyOverflow,

    /// An amount is negative where [`Money`](crate::Money) requires zero or
    /// more.
    #[error("money amount must be non-negative")]
    NegativeMoney,

    /// The caller broke a rule; the reason carries its own code and the
    /// values the UI fills into its wording.
    #[error("{0}")]
    Validation(ValidationError),

    /// The filesystem or the database failed; the text is the cause.
    #[error("I/O error: {0}")]
    Io(String),

    /// Deriving the vault key or applying a cipher setting to the database
    /// failed; the text is the cause.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Stored vault data cannot be interpreted: the header, the database or
    /// a row in it. The text says which.
    #[error("vault is corrupt: {0}")]
    VaultCorrupt(String),

    /// A backup file is not a usable Oikonomia backup; the text says why.
    #[error("backup is invalid: {0}")]
    BackupInvalid(String),

    /// A restore was refused because vault files exist and replacing them
    /// was not asked for.
    #[error("a vault already exists; restore requires replace")]
    RestoreWouldOverwrite,

    /// The requested record does not exist; the text names its kind, such as
    /// `account`.
    #[error("{0} not found")]
    NotFound(String),

    /// Reading an image with the bundled OCR failed (its models, decoding
    /// the image, or the engine); the text is the cause.
    #[error("analysis failed: {0}")]
    Analysis(String),

    /// A bank CSV or journal CSV could not be read, or the journal export
    /// could not be written; the text says why.
    #[error("{0}")]
    CsvParse(String),
}

impl Error {
    /// Every code [`Error::code`] returns for a variant other than
    /// [`Error::Validation`], whose codes are [`ValidationError::ALL_CODES`].
    ///
    /// The codes are in the order of the variants. The desktop crate checks
    /// this list against `errorCodes.json`.
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

    /// Returns the stable `snake_case` identifier the UI maps to localized
    /// text.
    ///
    /// A validation error gives its own code, [`ValidationError::code`]. The
    /// match has no wildcard arm, so a new variant does not compile until it
    /// has a code here.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::Error;
    /// use oikonomia_core::error::ValidationError;
    ///
    /// assert_eq!(Error::VaultLocked.code(), "vault_locked");
    /// assert_eq!(
    ///     Error::Validation(ValidationError::SameAccount).code(),
    ///     "same_account"
    /// );
    /// ```
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
    use oikonomia_test_support::listed_variants;

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
                code.chars()
                    .all(|letter| letter.is_ascii_lowercase() || letter == '_'),
                "{code}"
            );
        }
    }
}
