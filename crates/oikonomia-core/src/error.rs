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

use crate::csv::CsvError;
use std::collections::BTreeMap;
use std::fmt::Display;
use thiserror::Error;

mod context;
mod damage;
mod resource;
mod validation;

pub(crate) use context::{
    AnalysisContext, CryptoContext, DatabaseContext, IoContext, SerializationContext,
};
pub use damage::{BackupDefect, VaultCorruption};
pub use resource::Resource;
pub use validation::{AccountRole, NameField, ValidationError};

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
    #[error(transparent)]
    Validation(#[from] ValidationError),

    /// A statement or a transaction on the vault database failed.
    #[error("{operation}: {detail}")]
    Database {
        /// What core was doing, as a lowercase phrase such as
        /// `insert journal entry`.
        operation: &'static str,
        /// The driver's own text. For logs; never sent as a parameter.
        detail: String,
    },

    /// Reading or writing a file or a directory failed.
    #[error("{operation}: {detail}")]
    Io {
        /// What core was doing, as a lowercase phrase such as
        /// `write backup archive`.
        operation: &'static str,
        /// The operating system's own text. For logs; never sent as a
        /// parameter.
        detail: String,
    },

    /// Encoding or decoding one of the application's own JSON files failed.
    #[error("{operation}: {detail}")]
    Serialization {
        /// What core was doing, as a lowercase phrase such as
        /// `encode preferences`.
        operation: &'static str,
        /// The encoder's own text. For logs; never sent as a parameter.
        detail: String,
    },

    /// Deriving the vault key or applying a cipher setting to the database
    /// failed.
    #[error("{operation}: {detail}")]
    Crypto {
        /// What core was doing, as a lowercase phrase such as
        /// `derive vault key`.
        operation: &'static str,
        /// The library's own text. For logs; never sent as a parameter.
        detail: String,
    },

    /// Stored vault data cannot be interpreted: the header, the database or
    /// a row in it. The reason says which.
    #[error("vault is corrupt: {0}")]
    VaultCorrupt(VaultCorruption),

    /// The vault was written by a later build: its schema version is above
    /// the one this build migrates to. The vault is sound and is left
    /// untouched; a newer build opens it.
    #[error("vault schema version {found} is newer than this build supports ({supported})")]
    VaultTooNew {
        /// The schema version the vault records.
        found: i64,
        /// The highest schema version this build knows.
        supported: i64,
    },

    /// A backup file is not a usable Oikonomia backup; the reason says why.
    #[error("backup is invalid: {0}")]
    BackupInvalid(BackupDefect),

    /// A restore was refused because vault files exist and replacing them
    /// was not asked for.
    #[error("a vault already exists; restore requires replace")]
    RestoreWouldOverwrite,

    /// The requested record does not exist.
    #[error("{0} not found")]
    NotFound(Resource),

    /// Reading an image with the bundled OCR failed: its models, decoding
    /// the image, or the engine.
    #[error("{operation}: {detail}")]
    Analysis {
        /// What core was doing, as a lowercase phrase such as `decode image`.
        operation: &'static str,
        /// The library's own text. For logs; never sent as a parameter.
        detail: String,
    },

    /// A bank CSV or a journal CSV could not be read; the reason carries its
    /// own code and the values the UI fills into its wording.
    #[error(transparent)]
    Csv(#[from] CsvError),
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
        "database",
        "io",
        "serialization",
        "crypto",
        "vault_corrupt",
        "vault_too_new",
        "backup_invalid",
        "restore_would_overwrite",
        "not_found",
        "analysis",
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
            Self::Database { .. } => "database",
            Self::Io { .. } => "io",
            Self::Serialization { .. } => "serialization",
            Self::Crypto { .. } => "crypto",
            Self::VaultCorrupt(_) => "vault_corrupt",
            Self::VaultTooNew { .. } => "vault_too_new",
            Self::BackupInvalid(_) => "backup_invalid",
            Self::RestoreWouldOverwrite => "restore_would_overwrite",
            Self::NotFound(_) => "not_found",
            Self::Analysis { .. } => "analysis",
            Self::Csv(reason) => reason.code(),
        }
    }

    /// Returns the values the UI substitutes into the localized text, by name.
    ///
    /// Every variant decides here what it sends, in a match with no wildcard
    /// arm, so a new variant that carries data does not compile until its
    /// parameters are chosen:
    ///
    /// - an unbalanced entry sends `debits` and `credits`, in minor units;
    /// - a missing record sends `resource`, a [`Resource::identifier`];
    /// - a vault from a newer build sends `found` and `supported`;
    /// - a failure below the crate sends `operation` and never its `detail`,
    ///   which may hold operating-system text and is for logs;
    /// - a validation or CSV error sends its own
    ///   ([`ValidationError::params`], [`CsvError::params`]);
    /// - a corrupt vault and an invalid backup send nothing: the reason is
    ///   for the log, and the copy is one sentence each.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::Error;
    /// use oikonomia_core::error::Resource;
    ///
    /// let params = Error::NotFound(Resource::JournalEntry).params();
    ///
    /// assert_eq!(params["resource"], "journal_entry");
    /// ```
    #[must_use]
    pub fn params(&self) -> BTreeMap<&'static str, String> {
        match self {
            Self::UnbalancedEntry { debits, credits } => BTreeMap::from([
                ("credits", credits.to_string()),
                ("debits", debits.to_string()),
            ]),
            Self::Validation(reason) => reason.params(),
            Self::Csv(reason) => reason.params(),
            Self::Database { operation, .. }
            | Self::Io { operation, .. }
            | Self::Serialization { operation, .. }
            | Self::Crypto { operation, .. }
            | Self::Analysis { operation, .. } => {
                BTreeMap::from([("operation", (*operation).to_owned())])
            }
            Self::VaultTooNew { found, supported } => BTreeMap::from([
                ("found", found.to_string()),
                ("supported", supported.to_string()),
            ]),
            Self::NotFound(resource) => {
                BTreeMap::from([("resource", resource.identifier().to_owned())])
            }
            Self::VaultUninitialized
            | Self::VaultLocked
            | Self::InvalidPassword
            | Self::TooFewLines
            | Self::InvalidLineAmounts
            | Self::AccountWrongEntity
            | Self::MoneyOverflow
            | Self::NegativeMoney
            | Self::VaultCorrupt(_)
            | Self::BackupInvalid(_)
            | Self::RestoreWouldOverwrite => BTreeMap::new(),
        }
    }
}

/// Constructors for the variants that wrap a lower-level failure.
impl Error {
    /// Builds the error for a database failure during `operation`.
    pub(crate) fn database(operation: &'static str, detail: impl Display) -> Self {
        Self::Database {
            operation,
            detail: detail.to_string(),
        }
    }

    /// Builds the error for a file or directory failure during `operation`.
    pub(crate) fn io(operation: &'static str, detail: impl Display) -> Self {
        Self::Io {
            operation,
            detail: detail.to_string(),
        }
    }

    /// Builds the error for a failure to encode or decode during `operation`.
    pub(crate) fn serialization(operation: &'static str, detail: impl Display) -> Self {
        Self::Serialization {
            operation,
            detail: detail.to_string(),
        }
    }

    /// Builds the error for a cryptographic failure during `operation`.
    pub(crate) fn crypto(operation: &'static str, detail: impl Display) -> Self {
        Self::Crypto {
            operation,
            detail: detail.to_string(),
        }
    }

    /// Builds the error for a failure to read an image during `operation`.
    pub(crate) fn analysis(operation: &'static str, detail: impl Display) -> Self {
        Self::Analysis {
            operation,
            detail: detail.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BackupDefect, Error, Resource, ValidationError, VaultCorruption};
    use crate::csv::CsvError;
    use oikonomia_test_support::listed_variants;
    use std::collections::BTreeMap;

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
            Error::Database {
                operation: "x",
                detail: "x".into(),
            },
            Error::Io {
                operation: "x",
                detail: "x".into(),
            },
            Error::Serialization {
                operation: "x",
                detail: "x".into(),
            },
            Error::Crypto {
                operation: "x",
                detail: "x".into(),
            },
            Error::VaultCorrupt(VaultCorruption::MissingMetaTable),
            Error::VaultTooNew {
                found: 8,
                supported: 7,
            },
            Error::BackupInvalid(BackupDefect::Truncated),
            Error::RestoreWouldOverwrite,
            Error::NotFound(Resource::Account),
            Error::Analysis {
                operation: "x",
                detail: "x".into(),
            },
            Error::Csv(CsvError::Empty),
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
            Error::Database { .. },
            Error::Io { .. },
            Error::Serialization { .. },
            Error::Crypto { .. },
            Error::VaultCorrupt(_),
            Error::VaultTooNew { .. },
            Error::BackupInvalid(_),
            Error::RestoreWouldOverwrite,
            Error::NotFound(_),
            Error::Analysis { .. },
            Error::Csv(_),
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
            .filter(|sample| !matches!(sample, Error::Validation(_) | Error::Csv(_)))
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
    fn a_csv_error_gives_its_own_code() {
        assert_eq!(Error::from(CsvError::Empty).code(), "csv_empty");
    }

    #[test]
    fn an_unbalanced_entry_sends_both_totals() {
        let error = Error::UnbalancedEntry {
            debits: 100,
            credits: 50,
        };

        assert_eq!(
            error.params(),
            BTreeMap::from([("credits", "50".to_owned()), ("debits", "100".to_owned())])
        );
    }

    #[test]
    fn a_missing_record_sends_what_was_not_found() {
        assert_eq!(
            Error::NotFound(Resource::RecurringTemplate).params(),
            BTreeMap::from([("resource", "recurring_template".to_owned())])
        );
    }

    #[test]
    fn a_vault_from_a_newer_build_sends_both_versions() {
        let error = Error::VaultTooNew {
            found: 9,
            supported: 7,
        };

        assert_eq!(
            error.params(),
            BTreeMap::from([("found", "9".to_owned()), ("supported", "7".to_owned())])
        );
    }

    #[test]
    fn a_lower_level_failure_sends_its_operation_and_never_its_detail() {
        let failures = [
            Error::database("insert journal entry", "disk on fire"),
            Error::io("insert journal entry", "disk on fire"),
            Error::serialization("insert journal entry", "disk on fire"),
            Error::crypto("insert journal entry", "disk on fire"),
            Error::analysis("insert journal entry", "disk on fire"),
        ];

        for error in failures {
            assert_eq!(
                error.params(),
                BTreeMap::from([("operation", "insert journal entry".to_owned())]),
                "{error:?}"
            );
            assert_eq!(error.to_string(), "insert journal entry: disk on fire");
        }
    }

    #[test]
    fn a_wrapped_error_sends_the_parameters_of_what_it_wraps() {
        let validation = ValidationError::PasswordTooShort { min: 12 };
        let csv = CsvError::InvalidDate("31/31".into());

        assert_eq!(
            Error::from(validation.clone()).params(),
            validation.params()
        );
        assert_eq!(Error::from(csv.clone()).params(), csv.params());
    }

    #[test]
    fn a_reason_for_the_log_is_not_a_parameter() {
        let corrupt = Error::VaultCorrupt(VaultCorruption::Column {
            column: "accounts.id".into(),
            detail: "not an id".into(),
        });
        let invalid = Error::BackupInvalid(BackupDefect::EmptyMember {
            name: "vault.db".into(),
        });

        assert_eq!(corrupt.params(), BTreeMap::new());
        assert_eq!(invalid.params(), BTreeMap::new());
    }

    /// The parameter names the shared fixture pins for each code that has any.
    fn pinned_params() -> BTreeMap<String, Vec<String>> {
        serde_json::from_str(include_str!("../../../web/src/lib/errorCodeParams.json"))
            .expect("errorCodeParams.json parses")
    }

    /// The validation and CSV samples are checked by the tests of their own
    /// modules, which know every variant of those enums.
    #[test]
    fn the_params_fixture_lists_exactly_the_params_each_code_sends() {
        let pinned = pinned_params();

        for sample in every_variant() {
            if matches!(sample, Error::Validation(_) | Error::Csv(_)) {
                continue;
            }
            let sent: Vec<String> = sample
                .params()
                .keys()
                .map(|name| (*name).to_owned())
                .collect();
            let listed = pinned.get(sample.code()).cloned().unwrap_or_default();

            assert_eq!(sent, listed, "errorCodeParams.json for {}", sample.code());
        }
    }

    /// With the per-code checks of the three enums, this makes the fixture
    /// exactly what Rust sends: no entry for a code that does not exist.
    #[test]
    fn the_params_fixture_names_no_code_rust_does_not_have() {
        for code in pinned_params().keys() {
            let known = Error::ALL_CODES.contains(&code.as_str())
                || ValidationError::ALL_CODES.contains(&code.as_str())
                || CsvError::ALL_CODES.contains(&code.as_str());

            assert!(known, "errorCodeParams.json lists unknown code {code}");
        }
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
