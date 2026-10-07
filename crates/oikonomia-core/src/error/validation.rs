//! The reasons a request can be refused for breaking a rule.
//!
//! A [`ValidationError`] is what [`Error::Validation`](crate::Error) carries.
//! It reaches the user as three things, none of them a sentence written here:
//!
//! - [`ValidationError::code`], a stable `snake_case` code the UI maps to
//!   wording in the user's language;
//! - [`ValidationError::params`], the named values that wording fills in,
//!   such as an account code or a minimum length;
//! - for an account that is missing or wrong, the [`AccountRole`] it was
//!   meant to play, sent as a parameter so the UI can point at the field.
//!
//! The `Display` text is English diagnostic text; the UI never shows it.
//!
//! # Keeping Rust and the UI in step
//!
//! The UI's copy lives in the web sources, so three hand-written lists must
//! agree with the enums: [`ValidationError::ALL_CODES`], [`AccountRole::ALL`]
//! and the parameter names in `web/src/lib/errorCodeParams.json`. The tests
//! in this module check each list against its enum through an exhaustive
//! `match`, so a variant added without its code, its place in the list or its
//! parameters fails to compile or fails a test. The desktop crate checks the
//! codes and the roles against the UI's own lists.
//!
//! # What does not belong here
//!
//! A rule the user cannot act on (a malformed id, a caller bug) is
//! [`ValidationError::Internal`]: it has one code for all cases, and its
//! detail is diagnostic text and is never sent as a parameter.

use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;

/// The part an account plays in a simple entry.
///
/// A refused entry names the role so the UI can say which field to fix. The
/// UI shows the same label the entry form uses for it, chosen by
/// [`AccountRole::identifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccountRole {
    /// The expense account an expense is for.
    Category,
    /// The asset or liability account an expense or bill is paid from.
    Payment,
    /// The asset account an income is received into.
    Deposit,
    /// The income account an income is booked to.
    Income,
    /// The expense account a bill is for.
    BillCategory,
    /// The liability account that holds an unpaid bill.
    BillsPayable,
    /// The account a transfer takes money from.
    TransferSource,
    /// The account a transfer puts money into.
    TransferDestination,
}

impl AccountRole {
    /// Every role.
    ///
    /// The desktop crate checks this list, and its order, against
    /// `accountRoles.json`.
    pub const ALL: &'static [Self] = &[
        Self::Category,
        Self::Payment,
        Self::Deposit,
        Self::Income,
        Self::BillCategory,
        Self::BillsPayable,
        Self::TransferSource,
        Self::TransferDestination,
    ];

    /// Returns the stable `snake_case` identifier the UI maps to a translated
    /// label.
    #[must_use]
    pub fn identifier(self) -> &'static str {
        match self {
            Self::Category => "category",
            Self::Payment => "payment",
            Self::Deposit => "deposit",
            Self::Income => "income",
            Self::BillCategory => "bill_category",
            Self::BillsPayable => "bills_payable",
            Self::TransferSource => "transfer_source",
            Self::TransferDestination => "transfer_destination",
        }
    }
}

impl fmt::Display for AccountRole {
    /// Writes the identifier with spaces for underscores, which reads as
    /// English in logs.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.identifier().replace('_', " "))
    }
}

/// Why a request was refused. Mapped one to one onto a UI code.
///
/// Messages that deserve the same user-facing text share a variant. Anything
/// the user cannot act on is [`ValidationError::Internal`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
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
        /// The role that has no account.
        role: AccountRole,
    },

    /// The chosen account has the wrong type for its role in the entry.
    #[error("{role} account {code} has the wrong type for this entry")]
    AccountWrongType {
        /// The role the account was chosen for.
        role: AccountRole,
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
    /// the caller). The detail is diagnostic text and is never sent as a
    /// parameter.
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

    /// Returns the stable `snake_case` identifier the UI maps to localized
    /// text.
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

    /// Returns the values the UI substitutes into the localized text, by name.
    ///
    /// Roles go out as identifiers, never English labels, so the UI can
    /// translate them. Internal detail is left out on purpose.
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
            Self::AccountInactive { code } => {
                params.insert("code", code.clone());
            }
            Self::AccountRequired { role } => {
                params.insert("role", role.identifier().to_owned());
            }
            Self::AccountWrongType { role, code } => {
                params.insert("code", code.clone());
                params.insert("role", role.identifier().to_owned());
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
    use super::{AccountRole, ValidationError};
    use oikonomia_test_support::listed_variants;
    use std::collections::{BTreeMap, BTreeSet};

    /// The parameters of `error` as name and value pairs, in name order.
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
    fn account_wrong_type_reports_the_account_code_and_the_role_identifier() {
        let error = ValidationError::AccountWrongType {
            role: AccountRole::Payment,
            code: "5100".into(),
        };

        assert_eq!(error.code(), "account_wrong_type");
        assert_eq!(
            params_of(&error),
            vec![("code", "5100".to_owned()), ("role", "payment".to_owned())]
        );
        assert!(error.to_string().contains("payment"));
    }

    #[test]
    fn account_required_reports_which_role_is_missing() {
        for (role, identifier) in [
            (AccountRole::Category, "category"),
            (AccountRole::BillsPayable, "bills_payable"),
            (AccountRole::TransferDestination, "transfer_destination"),
        ] {
            let error = ValidationError::AccountRequired { role };

            assert_eq!(error.code(), "account_required");
            assert_eq!(params_of(&error), vec![("role", identifier.to_owned())]);
        }
    }

    #[test]
    fn account_wrong_type_reports_the_role_for_other_roles_too() {
        for (role, identifier) in [
            (AccountRole::Deposit, "deposit"),
            (AccountRole::BillCategory, "bill_category"),
            (AccountRole::TransferSource, "transfer_source"),
        ] {
            let error = ValidationError::AccountWrongType {
                role,
                code: "1010".into(),
            };

            assert_eq!(
                params_of(&error),
                vec![("code", "1010".to_owned()), ("role", identifier.to_owned())]
            );
        }
    }

    #[test]
    fn every_role_has_a_distinct_snake_case_identifier() {
        let identifiers: BTreeSet<&str> = AccountRole::ALL
            .iter()
            .map(|role| role.identifier())
            .collect();

        assert_eq!(identifiers.len(), AccountRole::ALL.len());
        for identifier in identifiers {
            assert!(
                identifier
                    .chars()
                    .all(|letter| letter.is_ascii_lowercase() || letter == '_'),
                "{identifier}"
            );
        }
    }

    listed_variants! {
        units listed_roles for AccountRole {
            AccountRole::Category,
            AccountRole::Payment,
            AccountRole::Deposit,
            AccountRole::Income,
            AccountRole::BillCategory,
            AccountRole::BillsPayable,
            AccountRole::TransferSource,
            AccountRole::TransferDestination,
        }
    }

    /// Fails unless `AccountRole::ALL` is exactly the set of roles in the
    /// `listed_roles` list above, each once. The compiler checks that list
    /// against the enum with an exhaustive `match`, so a role added to the enum
    /// but left out of the list does not compile. The order of `ALL` is pinned
    /// separately, against `accountRoles.json`.
    #[test]
    fn the_role_list_covers_every_variant() {
        let listed = listed_roles::variants();

        assert_eq!(
            AccountRole::ALL.len(),
            listed_roles::COUNT,
            "AccountRole::ALL and the listed variants differ in number"
        );
        for role in listed {
            assert!(
                AccountRole::ALL.contains(&role),
                "{role:?} is missing from AccountRole::ALL"
            );
        }
        listed_roles::assert_every_position_once(
            AccountRole::ALL
                .iter()
                .map(listed_roles::position)
                .collect(),
        );
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

    listed_variants! {
        patterns listed_errors for ValidationError {
            ValidationError::PasswordTooShort { .. },
            ValidationError::NameRequired { .. },
            ValidationError::NameTaken { .. },
            ValidationError::AccountCodeTaken,
            ValidationError::SystemAccountProtected,
            ValidationError::AccountInactive { .. },
            ValidationError::AccountRequired { .. },
            ValidationError::AccountWrongType { .. },
            ValidationError::SameAccount,
            ValidationError::AmountNotPositive,
            ValidationError::BillStatusRequired,
            ValidationError::InvalidDate { .. },
            ValidationError::DateRangeInverted,
            ValidationError::DateOutOfRange,
            ValidationError::DayOfMonthInvalid,
            ValidationError::LockTimeoutTooShort { .. },
            ValidationError::CurrencyInvalid,
            ValidationError::EntryAlreadyVoided,
            ValidationError::EntryNotPosted,
            ValidationError::WrongBook,
            ValidationError::OpeningBalanceAccountType,
            ValidationError::OpeningBalanceUnchanged,
            ValidationError::NoEquityAccount,
            ValidationError::FileEmpty,
            ValidationError::FileTooLarge { .. },
            ValidationError::FileTypeUnsupported,
            ValidationError::VaultAlreadyInitialized,
            ValidationError::Internal { .. },
        }
    }

    /// One value of every variant, in declaration order.
    fn every_variant() -> Vec<ValidationError> {
        vec![
            ValidationError::PasswordTooShort { min: 12 },
            ValidationError::NameRequired {
                field: "entity name",
            },
            ValidationError::NameTaken { name: "x".into() },
            ValidationError::AccountCodeTaken,
            ValidationError::SystemAccountProtected,
            ValidationError::AccountInactive { code: "x".into() },
            ValidationError::AccountRequired {
                role: AccountRole::Category,
            },
            ValidationError::AccountWrongType {
                role: AccountRole::Category,
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
        ]
    }

    /// Fails when `every_variant` has no sample for a variant named in the
    /// `listed_errors` list above, or when `ALL_CODES` is not the codes of the
    /// samples in order. The compiler checks `listed_errors` against the enum
    /// with an exhaustive `match`, so a variant added to the enum but not to
    /// that list does not compile. It does not check the wording of a code or
    /// that the UI has copy for it; the desktop crate does.
    #[test]
    fn all_lists_exactly_the_code_of_every_variant() {
        let samples = every_variant();
        let codes: Vec<&str> = samples.iter().map(ValidationError::code).collect();

        listed_errors::assert_every_position_once(
            samples.iter().map(listed_errors::position).collect(),
        );
        assert_eq!(codes, ValidationError::ALL_CODES);
    }

    /// The parameter names the shared fixture pins for each code that has any.
    fn pinned_params() -> BTreeMap<String, Vec<String>> {
        serde_json::from_str(include_str!("../../../../web/src/lib/errorCodeParams.json"))
            .expect("errorCodeParams.json parses")
    }

    /// The parameter names Rust sends for each code that sends any, taken from
    /// each variant's sample.
    ///
    /// A parameter that depends on the value (an optional field) would need
    /// the full set the UI copy may reference; none of the codes has one.
    fn produced_params() -> BTreeMap<String, Vec<String>> {
        let mut produced = BTreeMap::new();

        for sample in every_variant() {
            let names: Vec<String> = sample
                .params()
                .keys()
                .map(|name| (*name).to_owned())
                .collect();

            if !names.is_empty() {
                produced.insert(sample.code().to_owned(), names);
            }
        }

        produced
    }

    #[test]
    fn the_params_fixture_lists_exactly_the_params_rust_sends() {
        assert_eq!(
            pinned_params(),
            produced_params(),
            "errorCodeParams.json and ValidationError::params differ"
        );
    }

    #[test]
    fn every_code_is_snake_case() {
        for code in ValidationError::ALL_CODES {
            assert!(
                code.chars()
                    .all(|letter| letter.is_ascii_lowercase() || letter == '_'),
                "{code}"
            );
        }
    }

    #[test]
    fn every_message_starts_in_lowercase_and_has_no_trailing_period() {
        for error in every_variant() {
            let message = error.to_string();
            let first = message.chars().next();

            assert!(
                first.is_some_and(|first| !first.is_uppercase()),
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
}
