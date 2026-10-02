//! The reasons a request can be refused for breaking a rule.
//!
//! Each reason has a stable `snake_case` code and named parameters, so the UI
//! can show text in the user's language. The `Display` text is English and is
//! meant for logs only.

use std::collections::BTreeMap;

use thiserror::Error;

/// Why a request was refused. Mapped one to one onto a UI code.
///
/// Messages that deserve the same user-facing text share a variant. Anything
/// the user cannot act on is [`ValidationError::Internal`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationError {
    /// The new vault password is shorter than the minimum.
    #[error("password must be at least {min} characters")]
    PasswordTooShort {
        /// Minimum length, in characters.
        min: usize,
    },

    /// A required name or code was blank.
    #[error("{field} is required")]
    NameRequired {
        /// English description of the blank field, for logs.
        field: &'static str,
    },

    /// An entity or document with this name already exists.
    #[error("the name \"{name}\" is already in use")]
    NameTaken {
        /// The name that clashed.
        name: String,
    },

    /// Another account in the book already uses this code.
    #[error("account code already exists for this entity")]
    AccountCodeTaken,

    /// System accounts cannot be archived.
    #[error("system accounts cannot be archived")]
    SystemAccountProtected,

    /// The account is archived and cannot take new postings.
    #[error("account {code} is inactive")]
    AccountInactive {
        /// Code of the inactive account.
        code: String,
    },

    /// An entry needs an account for a role and none was chosen.
    #[error("{role} account is required")]
    AccountRequired {
        /// English name of the role, for logs.
        role: &'static str,
    },

    /// The chosen account has the wrong type for its role in the entry.
    #[error("{role} account {code} has the wrong type for this entry")]
    AccountWrongType {
        /// English name of the role, for logs.
        role: &'static str,
        /// Code of the account that was chosen.
        code: String,
    },

    /// Both sides of an entry resolve to the same account.
    #[error("entry needs two different accounts")]
    SameAccount,

    /// An amount was zero or negative where a positive one is needed.
    #[error("amount must be positive")]
    AmountNotPositive,

    /// A bill entry did not say whether it is paid.
    #[error("bill entries need a bill status")]
    BillStatusRequired,

    /// A date was not a real `YYYY-MM-DD` calendar date.
    #[error("invalid date: {value}")]
    InvalidDate {
        /// The text that failed to parse.
        value: String,
    },

    /// A range starts after it ends.
    #[error("from date must be on or before to")]
    DateRangeInverted,

    /// Date arithmetic left the supported range.
    #[error("date is outside the supported range")]
    DateOutOfRange,

    /// The day of month is missing, outside 1 to 31, or set on a non-monthly template.
    #[error("day_of_month must be 1-31 and is only valid for monthly cadence")]
    DayOfMonthInvalid,

    /// The idle lock timeout is below the minimum.
    #[error("lock timeout must be at least {min_secs} seconds")]
    LockTimeoutTooShort {
        /// Minimum timeout, in seconds.
        min_secs: u64,
    },

    /// The currency was not a three-letter code.
    #[error("base_currency must be a 3-letter ISO code")]
    CurrencyInvalid,

    /// The entry was already voided.
    #[error("entry is already voided")]
    EntryAlreadyVoided,

    /// Only posted entries can be voided.
    #[error("only posted entries can be voided")]
    EntryNotPosted,

    /// The entry or document belongs to a different book.
    #[error("entry belongs to a different book")]
    WrongBook,

    /// Opening balances apply only to asset and liability accounts.
    #[error("opening balances apply to asset or liability accounts")]
    OpeningBalanceAccountType,

    /// The account already has the requested balance.
    #[error("the account already has this balance")]
    OpeningBalanceUnchanged,

    /// The book has no equity account to offset an opening balance.
    #[error("this book has no equity account to post the opening balance against")]
    NoEquityAccount,

    /// The file has no content.
    #[error("empty file")]
    FileEmpty,

    /// The file is over the size limit.
    #[error("file too large (max {max_mb} MB)")]
    FileTooLarge {
        /// Limit, in megabytes.
        max_mb: u64,
    },

    /// The file type is not one the app accepts.
    #[error("unsupported file type; use PDF, PNG, JPEG, WebP, or plain text")]
    FileTypeUnsupported,

    /// A vault already exists here.
    #[error("vault is already initialized")]
    VaultAlreadyInitialized,

    /// A rule broke that the user cannot act on (a malformed id, a bug in
    /// the caller). The detail is for logs and is never sent as a parameter.
    #[error("{detail}")]
    Internal {
        /// What went wrong, in English.
        detail: String,
    },
}

impl ValidationError {
    /// Every code [`ValidationError::code`] can return.
    ///
    /// The desktop crate checks this list against `errorCodes.json`.
    pub const ALL_CODES: &'static [&'static str] = &[
        "password_too_short",
        "name_required",
        "name_taken",
        "account_code_taken",
        "system_account_protected",
        "account_inactive",
        "account_required",
        "account_wrong_type",
        "same_account",
        "amount_not_positive",
        "bill_status_required",
        "invalid_date",
        "date_range_inverted",
        "date_out_of_range",
        "day_of_month_invalid",
        "lock_timeout_too_short",
        "currency_invalid",
        "entry_already_voided",
        "entry_not_posted",
        "wrong_book",
        "opening_balance_account_type",
        "opening_balance_unchanged",
        "no_equity_account",
        "file_empty",
        "file_too_large",
        "file_type_unsupported",
        "vault_already_initialized",
        "validation_internal",
    ];

    /// Stable `snake_case` identifier the UI maps to localized text.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::PasswordTooShort { .. } => "password_too_short",
            Self::NameRequired { .. } => "name_required",
            Self::NameTaken { .. } => "name_taken",
            Self::AccountCodeTaken => "account_code_taken",
            Self::SystemAccountProtected => "system_account_protected",
            Self::AccountInactive { .. } => "account_inactive",
            Self::AccountRequired { .. } => "account_required",
            Self::AccountWrongType { .. } => "account_wrong_type",
            Self::SameAccount => "same_account",
            Self::AmountNotPositive => "amount_not_positive",
            Self::BillStatusRequired => "bill_status_required",
            Self::InvalidDate { .. } => "invalid_date",
            Self::DateRangeInverted => "date_range_inverted",
            Self::DateOutOfRange => "date_out_of_range",
            Self::DayOfMonthInvalid => "day_of_month_invalid",
            Self::LockTimeoutTooShort { .. } => "lock_timeout_too_short",
            Self::CurrencyInvalid => "currency_invalid",
            Self::EntryAlreadyVoided => "entry_already_voided",
            Self::EntryNotPosted => "entry_not_posted",
            Self::WrongBook => "wrong_book",
            Self::OpeningBalanceAccountType => "opening_balance_account_type",
            Self::OpeningBalanceUnchanged => "opening_balance_unchanged",
            Self::NoEquityAccount => "no_equity_account",
            Self::FileEmpty => "file_empty",
            Self::FileTooLarge { .. } => "file_too_large",
            Self::FileTypeUnsupported => "file_type_unsupported",
            Self::VaultAlreadyInitialized => "vault_already_initialized",
            Self::Internal { .. } => "validation_internal",
        }
    }

    /// The values the UI substitutes into the localized text, by name.
    ///
    /// English role names and internal detail are left out on purpose: the
    /// UI cannot translate them.
    #[must_use]
    pub fn params(&self) -> BTreeMap<&'static str, String> {
        let mut params = BTreeMap::new();

        match self {
            Self::PasswordTooShort { min } => {
                params.insert("min", min.to_string());
            }
            Self::NameTaken { name } => {
                params.insert("name", name.clone());
            }
            Self::AccountInactive { code } | Self::AccountWrongType { code, .. } => {
                params.insert("code", code.clone());
            }
            Self::InvalidDate { value } => {
                params.insert("value", value.clone());
            }
            Self::LockTimeoutTooShort { min_secs } => {
                params.insert("min_secs", min_secs.to_string());
            }
            Self::FileTooLarge { max_mb } => {
                params.insert("max_mb", max_mb.to_string());
            }
            Self::NameRequired { .. }
            | Self::AccountCodeTaken
            | Self::SystemAccountProtected
            | Self::AccountRequired { .. }
            | Self::SameAccount
            | Self::AmountNotPositive
            | Self::BillStatusRequired
            | Self::DateRangeInverted
            | Self::DateOutOfRange
            | Self::DayOfMonthInvalid
            | Self::CurrencyInvalid
            | Self::EntryAlreadyVoided
            | Self::EntryNotPosted
            | Self::WrongBook
            | Self::OpeningBalanceAccountType
            | Self::OpeningBalanceUnchanged
            | Self::NoEquityAccount
            | Self::FileEmpty
            | Self::FileTypeUnsupported
            | Self::VaultAlreadyInitialized
            | Self::Internal { .. } => {}
        }

        params
    }
}
#[cfg(test)]
mod tests {
    use super::ValidationError;
    use std::collections::{BTreeMap, BTreeSet};

    fn params_of(error: &ValidationError) -> Vec<(&'static str, String)> {
        error.params().into_iter().collect()
    }

    #[test]
    fn password_too_short_reports_the_minimum() {
        let error = ValidationError::PasswordTooShort { min: 12 };

        assert_eq!(error.code(), "password_too_short");
        assert_eq!(params_of(&error), vec![("min", "12".to_owned())]);
    }

    #[test]
    fn name_taken_reports_the_name() {
        let error = ValidationError::NameTaken {
            name: "Home".into(),
        };

        assert_eq!(error.code(), "name_taken");
        assert_eq!(params_of(&error), vec![("name", "Home".to_owned())]);
    }

    #[test]
    fn account_inactive_reports_the_account_code() {
        let error = ValidationError::AccountInactive {
            code: "1010".into(),
        };

        assert_eq!(error.code(), "account_inactive");
        assert_eq!(params_of(&error), vec![("code", "1010".to_owned())]);
    }

    #[test]
    fn account_wrong_type_reports_the_account_code_but_not_the_english_role() {
        let error = ValidationError::AccountWrongType {
            role: "payment",
            code: "5100".into(),
        };

        assert_eq!(error.code(), "account_wrong_type");
        assert_eq!(params_of(&error), vec![("code", "5100".to_owned())]);
        assert!(error.to_string().contains("payment"));
    }

    #[test]
    fn invalid_date_reports_the_offending_text() {
        let error = ValidationError::InvalidDate {
            value: "2026-13-01".into(),
        };

        assert_eq!(error.code(), "invalid_date");
        assert_eq!(params_of(&error), vec![("value", "2026-13-01".to_owned())]);
    }

    #[test]
    fn lock_timeout_too_short_reports_the_minimum_seconds() {
        let error = ValidationError::LockTimeoutTooShort { min_secs: 60 };

        assert_eq!(error.code(), "lock_timeout_too_short");
        assert_eq!(params_of(&error), vec![("min_secs", "60".to_owned())]);
    }

    #[test]
    fn file_too_large_reports_the_limit_in_megabytes() {
        let error = ValidationError::FileTooLarge { max_mb: 8 };

        assert_eq!(error.code(), "file_too_large");
        assert_eq!(params_of(&error), vec![("max_mb", "8".to_owned())]);
    }

    #[test]
    fn internal_errors_keep_their_detail_out_of_the_params() {
        let error = ValidationError::Internal {
            detail: "invalid id: nope".into(),
        };

        assert_eq!(error.code(), "validation_internal");
        assert_eq!(error.params(), BTreeMap::new());
        assert_eq!(error.to_string(), "invalid id: nope");
    }

    #[test]
    fn parameterless_errors_have_no_params() {
        assert_eq!(ValidationError::SameAccount.params(), BTreeMap::new());
        assert_eq!(ValidationError::DateRangeInverted.params(), BTreeMap::new());
    }

    /// One value of every variant. The `match` makes a new variant a compile
    /// error here until it is given a sample.
    fn every_variant() -> Vec<ValidationError> {
        let samples = vec![
            ValidationError::PasswordTooShort { min: 12 },
            ValidationError::NameRequired {
                field: "entity name",
            },
            ValidationError::NameTaken { name: "x".into() },
            ValidationError::AccountCodeTaken,
            ValidationError::SystemAccountProtected,
            ValidationError::AccountInactive { code: "x".into() },
            ValidationError::AccountRequired { role: "category" },
            ValidationError::AccountWrongType {
                role: "category",
                code: "x".into(),
            },
            ValidationError::SameAccount,
            ValidationError::AmountNotPositive,
            ValidationError::BillStatusRequired,
            ValidationError::InvalidDate { value: "x".into() },
            ValidationError::DateRangeInverted,
            ValidationError::DateOutOfRange,
            ValidationError::DayOfMonthInvalid,
            ValidationError::LockTimeoutTooShort { min_secs: 60 },
            ValidationError::CurrencyInvalid,
            ValidationError::EntryAlreadyVoided,
            ValidationError::EntryNotPosted,
            ValidationError::WrongBook,
            ValidationError::OpeningBalanceAccountType,
            ValidationError::OpeningBalanceUnchanged,
            ValidationError::NoEquityAccount,
            ValidationError::FileEmpty,
            ValidationError::FileTooLarge { max_mb: 8 },
            ValidationError::FileTypeUnsupported,
            ValidationError::VaultAlreadyInitialized,
            ValidationError::Internal { detail: "x".into() },
        ];

        for sample in &samples {
            match sample {
                ValidationError::PasswordTooShort { .. }
                | ValidationError::NameRequired { .. }
                | ValidationError::NameTaken { .. }
                | ValidationError::AccountCodeTaken
                | ValidationError::SystemAccountProtected
                | ValidationError::AccountInactive { .. }
                | ValidationError::AccountRequired { .. }
                | ValidationError::AccountWrongType { .. }
                | ValidationError::SameAccount
                | ValidationError::AmountNotPositive
                | ValidationError::BillStatusRequired
                | ValidationError::InvalidDate { .. }
                | ValidationError::DateRangeInverted
                | ValidationError::DateOutOfRange
                | ValidationError::DayOfMonthInvalid
                | ValidationError::LockTimeoutTooShort { .. }
                | ValidationError::CurrencyInvalid
                | ValidationError::EntryAlreadyVoided
                | ValidationError::EntryNotPosted
                | ValidationError::WrongBook
                | ValidationError::OpeningBalanceAccountType
                | ValidationError::OpeningBalanceUnchanged
                | ValidationError::NoEquityAccount
                | ValidationError::FileEmpty
                | ValidationError::FileTooLarge { .. }
                | ValidationError::FileTypeUnsupported
                | ValidationError::VaultAlreadyInitialized
                | ValidationError::Internal { .. } => {}
            }
        }

        samples
    }

    #[test]
    fn all_lists_exactly_the_code_of_every_variant() {
        let from_variants: BTreeSet<&str> =
            every_variant().iter().map(ValidationError::code).collect();
        let listed: BTreeSet<&str> = ValidationError::ALL_CODES.iter().copied().collect();

        assert_eq!(from_variants, listed);
        assert_eq!(
            listed.len(),
            ValidationError::ALL_CODES.len(),
            "duplicate code"
        );
    }

    #[test]
    fn every_code_is_snake_case() {
        for code in ValidationError::ALL_CODES {
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{code}"
            );
        }
    }

    #[test]
    fn every_message_is_readable_english() {
        for error in every_variant() {
            assert!(!error.to_string().is_empty(), "{}", error.code());
        }
    }
}
