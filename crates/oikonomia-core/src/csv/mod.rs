//! Bank CSV import and journal CSV export.
//!
//! Import parses a statement into suggested [`PostSimpleEntry`] rows and
//! **does not post**. Posting is a separate call ([`post_import_rows`]).
//!
//! # Amount sign → kind
//!
//! Default bank convention: **money leaving the account is negative** and
//! maps to [`SimpleEntryKind::Expense`]; a positive amount maps to
//! [`SimpleEntryKind::Income`]. Two-line simple entries only (the existing
//! kind → debit/credit mapping in `post_simple_entry`). If a given bank
//! inverts that sign, the file must be adjusted before import; v1 does not
//! auto-flip.
//!
//! # Duplicate detection
//!
//! A row matches an existing **active** journal entry (posted, not voided,
//! not a void-reversal) when all three are equal:
//! - booking date (`YYYY-MM-DD`)
//! - amount in minor units (absolute value of the signed CSV amount; for
//!   ledger entries, `Σ debit_minor`)
//! - description after [`normalize_description`]
//!
//! Preview sets `duplicate: true` on matches, including later rows in the
//! same file that repeat an earlier parsed row. [`post_import_rows`] **skips**
//! those keys unless `include_duplicates` is true. Skipped duplicates are
//! not an error.
//!
//! # Export amounts
//!
//! Journal export writes **integer minor units** in `debit_minor` and
//! `credit_minor` (not decimal major units). Status is `posted` or `voided`.
//! Voided originals and their reversing entries are included and marked.

mod amount;
mod export;
mod parse;
mod post;

use serde::{Deserialize, Serialize};

use crate::domain::{AccountId, EntityId};
use crate::error::Error;
use crate::ledger::{PostSimpleEntry, PostedEntryView, SimpleEntryKind};

pub use amount::{currency_minor_exponent, parse_signed_minor};
pub use export::{
    JournalCsvLine, JournalCsvStatus, default_journal_export_file_name, ensure_csv_path,
    export_journal_csv, parse_journal_export, write_journal_csv_file,
};
pub use parse::{parse_bank_csv, parse_csv_date, read_csv_text};
pub use post::{post_import_rows, preview_bank_csv, preview_bank_csv_file};

/// Upper bound on a CSV file read into memory (same cap as documents).
pub const MAX_CSV_BYTES: u64 = 8 * 1024 * 1024;

/// Parse-shape failures for bank and journal CSV.
///
/// Defined here so call sites can match without a wildcard; new variants may
/// be added without bumping the crate to a breaking release.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CsvError {
    /// File is empty or whitespace only.
    #[error("CSV is empty")]
    Empty,
    /// Bytes were not valid UTF-8.
    #[error("CSV is not valid UTF-8")]
    NotUtf8,
    /// File exceeds [`MAX_CSV_BYTES`].
    #[error("CSV is larger than 8 MB")]
    TooLarge,
    /// First row could not be used as headers.
    #[error("CSV is missing a header row")]
    MissingHeader,
    /// No date-like column.
    #[error("CSV is missing a date column")]
    MissingDateColumn,
    /// No amount, debit, or credit column.
    #[error("CSV is missing an amount column")]
    MissingAmountColumn,
    /// A cell is not a supported date.
    #[error("invalid date: {0}")]
    InvalidDate(String),
    /// A cell is not a supported amount.
    #[error("invalid amount: {0}")]
    InvalidAmount(String),
    /// Magnitude does not fit in `i64`.
    #[error("amount overflow")]
    AmountOverflow,
    /// Parsed amount is zero (simple entries require a positive amount).
    #[error("amount is zero")]
    ZeroAmount,
}

impl From<CsvError> for Error {
    fn from(err: CsvError) -> Self {
        if matches!(err, CsvError::AmountOverflow) {
            return Error::MoneyOverflow;
        }
        Error::CsvParse(err.to_string())
    }
}

/// Role accounts filled into each suggested [`PostSimpleEntry`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CsvImportAccounts {
    /// Bank / cash / card for expense payments and income deposits.
    pub wallet_account_id: Option<AccountId>,
    /// Expense category (used when the signed amount is negative).
    pub expense_account_id: Option<AccountId>,
    /// Income category (used when the signed amount is positive).
    pub income_account_id: Option<AccountId>,
}

/// IPC payload for `csv_import_preview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportPreviewInput {
    /// Book to match duplicates against and to stamp on suggested rows.
    pub entity_id: EntityId,
    /// Filesystem path. When omitted, the Tauri command opens a native Open dialog.
    #[serde(default)]
    pub path: Option<String>,
    /// Bank / cash / card for suggested expense payments and income deposits.
    #[serde(default)]
    pub wallet_account_id: Option<AccountId>,
    /// Default expense category for negative amounts.
    #[serde(default)]
    pub expense_account_id: Option<AccountId>,
    /// Default income category for positive amounts.
    #[serde(default)]
    pub income_account_id: Option<AccountId>,
}

impl CsvImportPreviewInput {
    /// Role accounts carried into each suggested row.
    #[must_use]
    pub const fn accounts(&self) -> CsvImportAccounts {
        CsvImportAccounts {
            wallet_account_id: self.wallet_account_id,
            expense_account_id: self.expense_account_id,
            income_account_id: self.income_account_id,
        }
    }
}

/// IPC payload for `csv_import_post`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportPostInput {
    /// Selected suggested rows (typically from preview `suggested`).
    pub rows: Vec<PostSimpleEntry>,
    /// When false (default), skip rows matching the duplicate rule.
    #[serde(default)]
    pub include_duplicates: bool,
}

/// One parsed bank-CSV data row, before duplicate flagging and posting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBankRow {
    /// 1-based CSV record number (the header is record 1).
    pub source_row: u32,
    /// ISO booking date.
    pub entry_date: String,
    /// Raw description (not normalized).
    pub description: String,
    /// Optional reference / check number.
    pub reference: Option<String>,
    /// Signed minor units: negative = money leaving = Expense.
    pub signed_amount_minor: i64,
    /// Absolute amount for [`PostSimpleEntry::amount_minor`].
    pub amount_minor: i64,
    /// Expense or Income from the sign.
    pub kind: SimpleEntryKind,
}

/// Per-row outcome from [`parse_bank_csv`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvRowOutcome {
    /// Row parsed into a suggested simple entry.
    Parsed(ParsedBankRow),
    /// Row is present but unusable.
    Invalid {
        /// 1-based CSV record number (the header is record 1).
        source_row: u32,
        /// English reason.
        message: String,
    },
}

/// Preview of a bank CSV: suggested simple entries, duplicate flags, per-row errors.
///
/// Does not write to the ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportPreview {
    /// Filesystem path, or empty when parsed from in-memory text.
    pub source: String,
    /// Data rows in file order.
    pub rows: Vec<CsvImportPreviewRow>,
}

/// One preview row for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportPreviewRow {
    /// 1-based CSV record number (the header is record 1).
    pub source_row: u32,
    /// True when date + amount + normalized description matches an active
    /// entry or an earlier parsed row in this file.
    pub duplicate: bool,
    /// Set when this row cannot be posted as-is.
    pub error: Option<String>,
    /// Suggested simple-form input when `error` is `None`.
    pub suggested: Option<PostSimpleEntry>,
    /// Signed minor units as parsed (negative = Expense).
    pub signed_amount_minor: Option<i64>,
}

/// Result of [`post_import_rows`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportPostResult {
    /// Entries that were actually posted.
    pub posted: Vec<PostedEntryView>,
    /// Rows skipped because they matched the duplicate rule.
    pub skipped_duplicate_count: u32,
}

/// Trim, collapse Unicode whitespace to a single ASCII space, then lowercase.
///
/// This is the **only** description transform used for duplicate detection.
/// Case folding is Unicode lowercase (`str::to_lowercase`), not a locale-
/// specific mapping. The original description stored on the journal entry is
/// the trimmed CSV cell, not this normalized form.
#[must_use]
pub fn normalize_description(raw: &str) -> String {
    let mut words = raw.split_whitespace().map(str::to_lowercase);
    let Some(mut out) = words.next() else {
        return String::new();
    };
    for word in words {
        out.push(' ');
        out.push_str(&word);
    }
    out
}

pub(crate) fn suggested_entry(
    entity_id: EntityId,
    row: &ParsedBankRow,
    accounts: &CsvImportAccounts,
) -> PostSimpleEntry {
    let category_account_id = match row.kind {
        SimpleEntryKind::Expense => accounts.expense_account_id,
        SimpleEntryKind::Income => accounts.income_account_id,
        SimpleEntryKind::Bill | SimpleEntryKind::Transfer => None,
    };
    PostSimpleEntry {
        entity_id,
        kind: row.kind,
        bill_status: None,
        entry_date: row.entry_date.clone(),
        description: row.description.clone(),
        reference: row.reference.clone(),
        amount_minor: row.amount_minor,
        category_account_id,
        wallet_account_id: accounts.wallet_account_id,
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_collapses_and_lowercases() {
        assert_eq!(
            normalize_description("  Café   POS\tTicket  "),
            "café pos ticket"
        );
        assert_eq!(normalize_description("GROCERIES"), "groceries");
        assert_eq!(normalize_description("   "), "");
    }
}
