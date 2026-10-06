//! The error type of the crate's public API.
//!
//! Every fallible public function returns [`enum@Error`], except the serde
//! hooks, which return the serializer's or deserializer's own error.
//!
//! # Shape
//!
//! [`enum@Error`] is a public enum because the desktop shell and the tests
//! branch on it. Its variants are of four kinds:
//!
//! - **A state or a rule of the ledger**, with no payload or with the values
//!   that broke the rule: [`Error::VaultLocked`], [`Error::UnbalancedEntry`],
//!   [`Error::NotFound`] with the [`Resource`] that is missing.
//! - **A refused request.** [`Error::Validation`] wraps a
//!   [`ValidationError`] and [`Error::Csv`] a [`CsvError`]. Both convert with
//!   `From`, so a function writes the reason itself and `?` or `.into()`
//!   does the rest.
//! - **Stored data that cannot be used.** [`Error::VaultCorrupt`] and
//!   [`Error::BackupInvalid`] carry an enum of the reasons
//!   ([`VaultCorruption`], [`BackupDefect`]), so the reason can be matched
//!   and not only read. [`Error::VaultTooNew`] is kept apart from them: that
//!   vault is sound, and only this build is too old for it.
//! - **A failure below the crate.** [`Error::Database`], [`Error::Io`],
//!   [`Error::Serialization`], [`Error::Crypto`] and [`Error::Analysis`] say
//!   which layer failed and carry an `operation` and a `detail`, described
//!   below.
//!
//! # Codes and parameters, not sentences
//!
//! The UI never shows text written here. An error reaches the user as two
//! things:
//!
//! - [`Error::code`], a stable `snake_case` code the UI maps to wording in
//!   the user's language;
//! - [`Error::params`], the named values that wording fills in: an account
//!   code, a minimum length, the kind of record that was not found.
//!
//! The `Display` text is English diagnostic text. The desktop shell passes
//! it along beside the code, where it reaches a log and never the screen.
//!
//! Both methods match every variant without a wildcard arm, here and in
//! [`ValidationError`] and [`CsvError`]. A new variant therefore does not
//! compile until it has a code and its parameters are decided, in the crate
//! that defines it. The tests below fail when [`Error::ALL_CODES`] falls out
//! of step with the variants, and when the parameter names differ from
//! `web/src/lib/errorCodeParams.json`, which the UI's own tests read.
//!
//! A value that is not a number or a name goes out as an identifier, never
//! as English: [`Resource::identifier`], [`AccountRole::identifier`],
//! [`NameField::identifier`]. The UI translates it.
//!
//! # Operation and detail
//!
//! A variant for a failure below the crate has two fields.
//!
//! - `operation` is what core was doing, as a fixed lowercase phrase:
//!   `insert journal entry`, `write backup archive`. It is written at the
//!   call site, it never holds data, and it is sent as a parameter.
//! - `detail` is the lower-level error's own text. It may hold a path or an
//!   operating-system message, so it is for logs only and is never a
//!   parameter.
//!
//! `Display` is `operation: detail`. The call site picks the variant through
//! a helper that exists for one foreign error type only, so a `rusqlite`
//! failure cannot be reported as a file failure by mistake:
//!
//! ```ignore
//! conn.execute(sql, params).database("insert journal entry")?;
//! fs::rename(from, to).io("rename vault file")?;
//! ```
//!
//! # Why the error is `Eq` and has no source
//!
//! [`enum@Error`] is `Clone + PartialEq + Eq`, so a test compares a whole
//! result with `assert_eq!` and a caller can keep an error after reporting
//! it. `rusqlite::Error` and `std::io::Error` are neither `Clone` nor
//! `PartialEq`, so the error holds no foreign error type and no boxed trait
//! object, and [`source`](std::error::Error::source) is `None` for every
//! variant. What a source would have told a reader is in `detail`, and what
//! core was doing is in `operation`, as typed fields. A test compares the
//! variant and the operation, never the text of a dependency.
//!
//! # Exhaustive on purpose
//!
//! None of the enums here is `#[non_exhaustive]`. The workspace is not
//! published, so its only other user is the desktop crate, and that crate
//! is meant to break when a variant is added: it matches these enums without
//! a wildcard arm, so the compiler points at every place that has to decide
//! what the new variant means. With `#[non_exhaustive]` those matches would
//! need a wildcard, and a new variant would fall into it unnoticed.

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
/// and how they reach the user. The UI words the error from
/// [`Error::code`] and [`Error::params`]; a `detail` field is English text
/// for logs and is not shown.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
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
    /// [`Error::Validation`] and [`Error::Csv`], whose codes are
    /// [`ValidationError::ALL_CODES`] and [`CsvError::ALL_CODES`].
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
    /// A validation error and a CSV error give their own code
    /// ([`ValidationError::code`], [`CsvError::code`]). The match has no
    /// wildcard arm, so a new variant does not compile until it has a code
    /// here.
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
    /// samples other than the validation and CSV ones, in order. The compiler checks
    /// `listed_errors` against the enum with an exhaustive `match`, so a
    /// variant added to the enum but not to that list does not compile. It
    /// does not check that the UI has copy for a code; the desktop crate does.
    #[test]
    fn all_codes_lists_the_code_of_every_variant_that_has_its_own() {
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

    /// The validation and CSV samples stand for their enums, whose own tests
    /// check every variant's message.
    #[test]
    fn every_message_starts_in_lowercase_and_has_no_trailing_period() {
        for error in every_variant() {
            let message = error.to_string();

            assert!(
                message
                    .chars()
                    .next()
                    .is_some_and(|first| !first.is_uppercase()),
                "{}: {message:?} must not be empty or start with a capital",
                error.code()
            );
            assert!(
                !message.ends_with('.'),
                "{}: {message:?} must not end with a period",
                error.code()
            );
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
