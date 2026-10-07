//! Bank CSV import and journal CSV export.
//!
//! Import parses a statement into suggested [`PostSimpleEntryRequest`] rows and
//! **does not post**. Posting is a separate call ([`post_import_rows`]).
//!
//! The import is two steps so that nothing a bank file says reaches the
//! ledger without the user seeing it first. A file-level problem (no header,
//! no date column) is an error; a problem in one row makes that row invalid
//! and leaves the others usable.
//!
//! # Amounts
//!
//! An amount cell becomes signed integer minor units; no floating point is
//! involved. A statement is read for the book's base currency, which gives
//! the number of decimals ([`currency_minor_exponent`]) and the one currency
//! code a cell may carry. What is accepted, with the result for a book in
//! euros:
//!
//! | Part             | Accepted                      | Example                  |
//! |------------------|-------------------------------|--------------------------|
//! | Negative         | Leading `-` or U+2212         | `-25` → −2500            |
//! |                  | Trailing `-` or U+2212        | `25-` → −2500            |
//! |                  | Parentheses around the cell   | `(25,00)` → −2500        |
//! | Positive         | No sign, or a leading `+`     | `+25.00` → 2500          |
//! | Decimal mark     | `.` or `,`                    | `1234,5` → 123450        |
//! | Grouping         | The other separator           | `1.234,56` → 123456      |
//! |                  |                               | `1,234.56` → 123456      |
//! |                  | An apostrophe, `'` or U+2019  | `1'234.56` → 123456      |
//! |                  | Whitespace between digits     | `1 234,56` → 123456      |
//! | Three-digit tail | A thousands group             | `1.234` → 123400         |
//! | Currency sign    | `€ $ £ ¥ ₹ ₺ ₩`, anywhere     | `€1.234,56` → 123456     |
//! | Currency code    | The book's code, at an end    | `12.00 EUR` → 1200       |
//! | Other whitespace | Ignored                       | `- 25 EUR` → −2500       |
//! | Exponent         | None                          | `1e3` is rejected        |
//!
//! The decimal mark is the last `.` or `,` in the cell and is followed by at
//! most `exponent` digits. Groups are of three digits after a first group of
//! one to three with no leading zero, whichever separator groups them. A
//! three-digit tail after a `.` or `,` is a group and not a fraction unless
//! the currency has three decimals. An apostrophe and whitespace between
//! digits (a space, U+00A0 or the narrow U+202F) are never the decimal mark.
//!
//! Anything else is rejected, not guessed at: a second sign (`-25-`), a
//! fraction longer than the currency has (`0.125` in EUR), irregular
//! grouping (`1,2,3.45`, `12'34`, `1 2 3,45`), a currency sign outside the
//! list, the code of another currency than the book's (`25 USD` in a book in
//! euros), and a separator with no digit. The exact rules and their order
//! are in the `csv/amount.rs` module doc. [`parse_signed_minor`] is the same
//! grammar for a caller that has a number of decimals and no book: it drops
//! any three-letter code.
//!
//! # Amount sign → kind
//!
//! Default bank convention: **money leaving the account is negative** and
//! maps to [`SimpleEntryKind::Expense`]; a positive amount maps to
//! [`SimpleEntryKind::Income`]. Two-line simple entries only (the existing
//! kind → debit/credit mapping in `post_simple_entry`). A zero amount makes
//! the row invalid.
//!
//! A file that does not follow the convention is handled by its columns,
//! never by a setting that flips every sign. Separate debit and credit
//! columns are read as money out and money in, and a minus written in one
//! of them reverses it: a negative debit is money in (a debit taken back)
//! and a negative credit is money out. A direction column
//! ([`CsvColumnMapping::direction`]) decides the sign of the amount beside
//! it, whatever sign that amount was written with. The `csv/parse.rs` module
//! doc has the table of the debit and credit cases.
//!
//! # Duplicate detection
//!
//! A row matches an existing **active** journal entry (posted, not voided,
//! not a void-reversal) when all three are equal:
//! - booking date (`YYYY-MM-DD`)
//! - signed amount in minor units, negative for money out
//! - description after [`normalize_description`]
//!
//! The sign is part of the rule, so a purchase and its refund (the same
//! size, day and text, in opposite directions) are two entries. A row's
//! signed amount is the one the CSV gave it. A ledger entry's size is
//! `Σ debit_minor`, and its sign follows what it adds to the result, the
//! credits minus the debits of its lines on income and expense accounts:
//! positive is money in, negative is money out. An entry that adds nothing
//! to the result (a transfer between the book's own accounts, the payment of
//! a bill recorded earlier) matches a row of either sign, because the
//! statement of the account the money left shows it negative and the
//! statement of the account it reached shows it positive.
//!
//! Preview sets `duplicate: true` on matches, including later rows in the
//! same file that repeat an earlier parsed row. [`post_import_rows`] **skips**
//! those rows unless `include_duplicates` is true. Skipped duplicates are
//! not an error.
//!
//! # Export amounts
//!
//! Journal export writes **integer minor units** in `debit_minor` and
//! `credit_minor` (not decimal major units). Status is `posted` or `voided`.
//! Voided originals and their reversing entries are included and marked.
//! Hidden entries are omitted from the export (the owner still sees them
//! in-app).

mod amount;
mod error;
mod export;
mod parse;
mod post;

use serde::{Deserialize, Serialize};
use time::Date;

use crate::domain::{AccountId, EntityId};
use crate::ledger::{PostSimpleEntryRequest, PostedEntryView, SimpleEntryKind};
use crate::ui_text::UiText;
use crate::util::format_date;

pub use amount::{currency_minor_exponent, parse_signed_minor};
pub use error::{CsvError, CsvMappingProblem};
pub use export::{
    JournalCsvLine, JournalCsvStatus, default_journal_export_file_name, ensure_csv_path,
    export_journal_csv, parse_journal_export, write_journal_csv_file,
};
pub use parse::{ParsedBankCsv, parse_bank_csv, parse_csv_date, read_csv_text};
pub use post::{post_import_rows, preview_bank_csv, preview_bank_csv_file};

/// Upper bound on a CSV file read into memory: 8 MiB, the same as
/// [`crate::documents::MAX_DOCUMENT_BYTES`].
pub const MAX_CSV_BYTES: u64 = 8 * 1024 * 1024;

/// Role accounts filled into each suggested [`PostSimpleEntryRequest`].
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
    /// Column mapping. When omitted, headers are auto-detected.
    #[serde(default)]
    pub mapping: Option<CsvColumnMapping>,
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
    /// Selected suggested rows (typically from preview `suggested`), each in
    /// the wire form of a simple entry.
    pub rows: Vec<PostSimpleEntryRequest>,
    /// When false (default), skip rows matching the duplicate rule.
    #[serde(default)]
    pub include_duplicates: bool,
}

/// One parsed bank-CSV data row, before duplicate flagging and posting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBankRow {
    /// 1-based CSV record number (the header is record 1).
    pub source_row: u32,
    /// Booking date.
    pub entry_date: Date,
    /// Raw description (not normalized).
    pub description: String,
    /// Optional reference / check number.
    pub reference: Option<String>,
    /// Signed minor units: negative = money leaving = Expense.
    pub signed_amount_minor: i64,
    /// Absolute amount for [`PostSimpleEntryRequest::amount_minor`].
    pub amount_minor: i64,
    /// Expense or Income from the sign.
    pub kind: SimpleEntryKind,
}

/// Per-row outcome from [`parse_bank_csv`].
///
/// A row either parsed or it did not, and the preview in this crate matches
/// both variants without a wildcard. The desktop crate never sees this
/// type: it receives [`CsvImportPreviewRow`] values. A third variant would
/// therefore be caught by the compiler here and nowhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvRowOutcome {
    /// Row parsed into a suggested simple entry.
    Parsed(ParsedBankRow),
    /// Row is present but unusable.
    Invalid {
        /// 1-based CSV record number (the header is record 1).
        source_row: u32,
        /// Why the row is unusable, as a code the UI words.
        reason: UiText,
    },
}

/// Header-name mapping for one bank CSV (Map columns step).
///
/// Values are **header names** as they appear in the first row, matched
/// case-insensitively (ASCII). 0-based indexes are not accepted.
///
/// When this struct is provided (`Some`), `date` and `description` are
/// required, and the amount side must be **either** `amount` **or** both
/// `debit` and `credit` — not both forms, and not neither. `reference` and
/// `direction` are optional. Auto-detect is not used for any field, so a
/// column left out here is not read.
///
/// When omitted (`None` on [`CsvImportPreviewInput::mapping`]), the parser
/// auto-detects columns from header aliases.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsvColumnMapping {
    /// Date column header.
    #[serde(default)]
    pub date: Option<String>,
    /// Description / payee / memo column header.
    #[serde(default)]
    pub description: Option<String>,
    /// Signed amount column. Mutually exclusive with [`Self::debit`] / [`Self::credit`].
    #[serde(default)]
    pub amount: Option<String>,
    /// Debit (money-out) column; requires [`Self::credit`] when mapping is
    /// explicit. A negative cell is a debit taken back: money in.
    #[serde(default)]
    pub debit: Option<String>,
    /// Credit (money-in) column; requires [`Self::debit`] when mapping is
    /// explicit. A negative cell is a credit taken back: money out.
    #[serde(default)]
    pub credit: Option<String>,
    /// Optional reference / check-number column.
    #[serde(default)]
    pub reference: Option<String>,
    /// Optional column that says which way the money moved (`Debit` /
    /// `Credit`, `D` / `C`, `In` / `Out`), for files whose [`Self::amount`]
    /// is unsigned. When set, a recognized cell decides the sign of its row,
    /// an empty cell leaves the amount's own sign, and any other cell makes
    /// the row invalid. Ignored when [`Self::debit`] and [`Self::credit`]
    /// are mapped.
    #[serde(default)]
    pub direction: Option<String>,
}

/// Preview of a bank CSV: suggested simple entries, duplicate flags, per-row errors.
///
/// Does not write to the ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportPreview {
    /// Filesystem path, or empty when parsed from in-memory text.
    pub source: String,
    /// Header row, in file order (trimmed). For the Map columns UI.
    pub headers: Vec<String>,
    /// Auto-detected mapping from header aliases, for Map UI pre-fill.
    /// Present even when the caller supplied [`CsvImportPreviewInput::mapping`].
    pub detected_mapping: CsvColumnMapping,
    /// Data rows in file order.
    pub rows: Vec<CsvImportPreviewRow>,
}

/// One preview row for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportPreviewRow {
    /// 1-based CSV record number (the header is record 1).
    pub source_row: u32,
    /// True when date + signed amount + normalized description matches an
    /// active entry or an earlier parsed row in this file.
    pub duplicate: bool,
    /// Set when this row cannot be posted as-is: a code the UI words, never
    /// a sentence.
    pub error: Option<UiText>,
    /// Suggested simple-form input when `error` is `None`, in its wire form:
    /// an account the import was not given is left empty for the user.
    pub suggested: Option<PostSimpleEntryRequest>,
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

/// Returns `raw` trimmed, with each run of Unicode whitespace as one ASCII
/// space, in lowercase.
///
/// This is the **only** description transform used for duplicate detection.
/// Case folding is Unicode lowercase (`str::to_lowercase`), not a locale-
/// specific mapping. The original description stored on the journal entry is
/// the trimmed CSV cell, not this normalized form.
#[must_use]
pub fn normalize_description(raw: &str) -> String {
    let mut words = raw.split_whitespace().map(str::to_lowercase);
    let Some(mut normalized) = words.next() else {
        return String::new();
    };
    for word in words {
        normalized.push(' ');
        normalized.push_str(&word);
    }
    normalized
}

/// Returns the simple entry suggested for a parsed bank row, with the role
/// accounts of the import filled in.
///
/// The category is the expense or the income account, by the kind the sign
/// gave the row. An import never produces a bill or a transfer.
pub(crate) fn suggested_entry(
    entity_id: EntityId,
    row: &ParsedBankRow,
    accounts: &CsvImportAccounts,
) -> PostSimpleEntryRequest {
    let category_account_id = match row.kind {
        SimpleEntryKind::Expense => accounts.expense_account_id,
        SimpleEntryKind::Income => accounts.income_account_id,
        SimpleEntryKind::Bill | SimpleEntryKind::Transfer => None,
    };
    PostSimpleEntryRequest {
        entity_id,
        kind: row.kind,
        bill_status: None,
        entry_date: format_date(row.entry_date),
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
