//! Typed errors for the core library.

use thiserror::Error;

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

    /// Generic validation failure with a stable message for logs/UI mapping.
    #[error("{0}")]
    Validation(String),

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

    /// License file is missing, unreadable, or fails verification.
    #[error("license is invalid")]
    LicenseInvalid,

    /// License or trial has expired; mutating writes are blocked.
    #[error("license has expired")]
    LicenseExpired,
}
