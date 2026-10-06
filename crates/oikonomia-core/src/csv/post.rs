//! Preview (read-only) and explicit post of selected import rows.
//!
//! The two halves share one rule, the duplicate key ([`DedupeKey`]), and
//! apply it at different times on purpose. The preview flags a row that
//! matches the ledger or an earlier row of the file, so the user can see
//! it. The post checks again against the ledger as it is then, inside its
//! transaction, because the rows it is given are whatever the caller kept,
//! possibly edited, and other entries may have been posted since the
//! preview.
//!
//! A post is all or nothing: one row the ledger refuses rolls back every
//! row of the batch. A skipped duplicate is not a refusal.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;

use crate::csv::parse::{parse_bank_csv, read_csv_text};
use crate::csv::{
    CsvColumnMapping, CsvImportAccounts, CsvImportPostResult, CsvImportPreview,
    CsvImportPreviewRow, CsvRowOutcome, currency_minor_exponent, normalize_description,
    suggested_entry,
};
use crate::domain::{AccountId, EntityId};
use crate::error::{Error, Result, ValidationError};
use crate::ledger::{
    PostSimpleEntry, PostedEntryView, get_account, get_entity, post_simple_entry_unchecked,
};

/// Parses a bank CSV into suggested entries and flags duplicates. **Does
/// not post.**
///
/// `mapping` replaces header auto-detection when `Some`; see
/// [`CsvColumnMapping`]. The amounts are read with the exponent of the
/// entity's base currency.
///
/// # Errors
///
/// - [`Error::NotFound`] when the entity or one of the role accounts in
///   `accounts` does not exist.
/// - [`Error::AccountWrongEntity`] when a role account belongs to another
///   entity.
/// - [`Error::CsvParse`] for a problem with the file as a whole, as
///   [`parse_bank_csv`] lists them. A bad row is not an error; it is a row
///   of the preview with `error` set.
/// - [`Error::Io`] on database errors.
pub fn preview_bank_csv(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    csv_text: &str,
    mapping: Option<&CsvColumnMapping>,
) -> Result<CsvImportPreview> {
    preview_bank_csv_named(conn, entity_id, accounts, csv_text, mapping, String::new())
}

/// Reads the CSV file at `path` and previews it as [`preview_bank_csv`]
/// does. **Does not post.**
///
/// The preview's `source` is `path` as displayed.
///
/// # Errors
///
/// - [`Error::Io`] when `path` cannot be read or is not a regular file.
/// - [`Error::CsvParse`] when the file is larger than
///   [`MAX_CSV_BYTES`](crate::csv::MAX_CSV_BYTES) or is not UTF-8.
/// - Every error of [`preview_bank_csv`].
pub fn preview_bank_csv_file(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    path: &Path,
    mapping: Option<&CsvColumnMapping>,
) -> Result<CsvImportPreview> {
    let text = read_csv_text(path)?;
    preview_bank_csv_named(
        conn,
        entity_id,
        accounts,
        &text,
        mapping,
        path.display().to_string(),
    )
}

/// Posts the selected suggested rows as simple entries, in one transaction.
///
/// Rows that match the duplicate rule in the [`crate::csv`] module doc are
/// skipped and counted unless `include_duplicates` is true. A row that
/// repeats an earlier row of the same batch is skipped the same way. An
/// empty `rows` succeeds and posts nothing.
///
/// # Errors
///
/// Any error rolls back the whole batch.
///
/// - [`Error::Validation`] with [`ValidationError::Internal`] when the rows
///   do not all carry the same `entity_id`.
/// - [`Error::NotFound`] when that entity does not exist.
/// - Every error of [`post_simple_entry`](crate::ledger::post_simple_entry)
///   for a row the ledger refuses: a non-positive amount, a missing or
///   mistyped role account.
/// - [`Error::Io`] on database errors.
pub fn post_import_rows(
    conn: &Connection,
    rows: &[PostSimpleEntry],
    include_duplicates: bool,
) -> Result<CsvImportPostResult> {
    let Some(first) = rows.first() else {
        return Ok(CsvImportPostResult {
            posted: Vec::new(),
            skipped_duplicate_count: 0,
        });
    };
    let entity_id = first.entity_id;
    for row in rows {
        if row.entity_id != entity_id {
            return Err(Error::Validation(ValidationError::Internal {
                detail: "import rows must belong to a single entity".into(),
            }));
        }
    }
    let _entity = get_entity(conn, entity_id)?;

    let transaction = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let mut seen = load_active_keys(&transaction, entity_id)?;
    let mut posted: Vec<PostedEntryView> = Vec::new();
    let mut skipped_duplicate_count = 0u32;

    for row in rows {
        let key = DedupeKey::new(&row.entry_date, row.amount_minor, &row.description);
        if !include_duplicates && seen.contains(&key) {
            skipped_duplicate_count = skipped_duplicate_count.saturating_add(1);
            continue;
        }
        let view = post_simple_entry_unchecked(&transaction, row)?;
        seen.insert(key);
        posted.push(view);
    }

    transaction
        .commit()
        .map_err(|err| Error::Io(err.to_string()))?;
    Ok(CsvImportPostResult {
        posted,
        skipped_duplicate_count,
    })
}

/// What makes two entries the same for duplicate detection: the three
/// values the [`crate::csv`] module doc names.
///
/// Built only through [`DedupeKey::new`], so the description is always in
/// its normalized form and two keys compare the way the rule says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DedupeKey {
    /// Booking date, `YYYY-MM-DD`.
    date: String,
    /// Unsigned amount in minor units: an expense and an income of the same
    /// size on the same day with the same text are one key.
    amount_minor: i64,
    /// Description after [`normalize_description`].
    description: String,
}

impl DedupeKey {
    /// Returns the key of an entry, normalizing `description`.
    fn new(date: &str, amount_minor: i64, description: &str) -> Self {
        Self {
            date: date.to_owned(),
            amount_minor,
            description: normalize_description(description),
        }
    }
}

/// The shared body of the two previews; `source` is what the preview
/// reports the text came from, empty for in-memory text.
#[expect(
    clippy::too_many_arguments,
    reason = "the import source is passed beside the five inputs of the public preview"
)]
fn preview_bank_csv_named(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    csv_text: &str,
    mapping: Option<&CsvColumnMapping>,
    source: String,
) -> Result<CsvImportPreview> {
    let entity = get_entity(conn, entity_id)?;
    check_role_account(conn, entity_id, accounts.wallet_account_id)?;
    check_role_account(conn, entity_id, accounts.expense_account_id)?;
    check_role_account(conn, entity_id, accounts.income_account_id)?;

    let exponent = currency_minor_exponent(&entity.base_currency);
    let parsed = parse_bank_csv(csv_text, exponent, mapping)?;
    let mut seen = load_active_keys(conn, entity_id)?;
    let mut rows = Vec::with_capacity(parsed.rows.len());

    for outcome in parsed.rows {
        rows.push(preview_row(entity_id, accounts, &mut seen, outcome));
    }

    Ok(CsvImportPreview {
        source,
        headers: parsed.headers,
        detected_mapping: parsed.detected_mapping,
        rows,
    })
}

/// Turns one parse outcome into a preview row, recording its key in `seen`.
///
/// A parsed row is a duplicate when its key was already in `seen`: from the
/// ledger, or from an earlier row of this file.
fn preview_row(
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    seen: &mut HashSet<DedupeKey>,
    outcome: CsvRowOutcome,
) -> CsvImportPreviewRow {
    match outcome {
        CsvRowOutcome::Invalid { source_row, reason } => CsvImportPreviewRow {
            source_row,
            duplicate: false,
            error: Some(reason),
            suggested: None,
            signed_amount_minor: None,
        },
        CsvRowOutcome::Parsed(row) => {
            let key = DedupeKey::new(&row.entry_date, row.amount_minor, &row.description);
            let duplicate = !seen.insert(key);
            CsvImportPreviewRow {
                source_row: row.source_row,
                duplicate,
                error: None,
                suggested: Some(suggested_entry(entity_id, &row, &accounts)),
                signed_amount_minor: Some(row.signed_amount_minor),
            }
        }
    }
}

/// Checks that a role account, when one is given, exists and belongs to
/// `entity_id`.
///
/// Its type is not checked here; the post does that for the kind of each
/// row.
fn check_role_account(conn: &Connection, entity_id: EntityId, id: Option<AccountId>) -> Result<()> {
    let Some(id) = id else {
        return Ok(());
    };
    let account = get_account(conn, id)?;
    if account.entity_id != entity_id {
        return Err(Error::AccountWrongEntity);
    }
    Ok(())
}

/// Loads the key of every active entry of `entity_id`: posted, not voided,
/// and not itself the reversal of a voided entry.
///
/// An entry's amount is the sum of its debit lines, which for a balanced
/// entry is its total. Every entry of the book is read on each call; there
/// is no index on the key.
fn load_active_keys(conn: &Connection, entity_id: EntityId) -> Result<HashSet<DedupeKey>> {
    let mut statement = conn
        .prepare(
            "
            SELECT je.entry_date, je.description, COALESCE(SUM(jl.debit_minor), 0)
            FROM journal_entries je
            JOIN journal_lines jl ON jl.entry_id = je.id
            WHERE je.entity_id = ?1
              AND je.status = 'posted'
              AND je.voided_by_entry_id IS NULL
              AND NOT EXISTS (
                  SELECT 1 FROM journal_entries je_void
                  WHERE je_void.voided_by_entry_id = je.id
              )
            GROUP BY je.id, je.entry_date, je.description
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mapped = statement
        .query_map([entity_id.0.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut keys = HashSet::new();
    for row in mapped {
        let (date, description, amount) = row.map_err(|err| Error::Io(err.to_string()))?;
        keys.insert(DedupeKey::new(&date, amount, &description));
    }
    Ok(keys)
}
