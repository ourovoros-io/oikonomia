//! Preview (read-only) and explicit post of selected import rows.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;

use super::parse::{parse_bank_csv, read_csv_text};
use super::{
    CsvImportAccounts, CsvImportPostResult, CsvImportPreview, CsvImportPreviewRow, CsvRowOutcome,
    currency_minor_exponent, normalize_description, suggested_entry,
};
use crate::domain::{AccountId, EntityId};
use crate::error::{Error, Result};
use crate::ledger::{
    PostSimpleEntry, PostedEntryView, get_account, get_entity, post_simple_entry_unchecked,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DedupeKey {
    date: String,
    amount_minor: i64,
    description: String,
}

impl DedupeKey {
    fn new(date: &str, amount_minor: i64, description: &str) -> Self {
        Self {
            date: date.to_owned(),
            amount_minor,
            description: normalize_description(description),
        }
    }
}

/// Parse a bank CSV and flag duplicates. **Does not post.**
///
/// # Errors
///
/// Unknown entity, file-level CSV shape errors, or database errors.
pub fn preview_bank_csv(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    csv_text: &str,
) -> Result<CsvImportPreview> {
    preview_bank_csv_named(conn, entity_id, accounts, csv_text, String::new())
}

/// [`preview_bank_csv`] reading `path` from disk. **Does not post.**
///
/// # Errors
///
/// Filesystem errors plus [`preview_bank_csv`].
pub fn preview_bank_csv_file(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    path: &Path,
) -> Result<CsvImportPreview> {
    let text = read_csv_text(path)?;
    preview_bank_csv_named(conn, entity_id, accounts, &text, path.display().to_string())
}

fn preview_bank_csv_named(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    csv_text: &str,
    source: String,
) -> Result<CsvImportPreview> {
    let entity = get_entity(conn, entity_id)?;
    check_role_account(conn, entity_id, accounts.wallet_account_id)?;
    check_role_account(conn, entity_id, accounts.expense_account_id)?;
    check_role_account(conn, entity_id, accounts.income_account_id)?;

    let exponent = currency_minor_exponent(&entity.base_currency);
    let parsed = parse_bank_csv(csv_text, exponent)?;
    let mut seen = load_active_keys(conn, entity_id)?;
    let mut rows = Vec::with_capacity(parsed.len());

    for outcome in parsed {
        rows.push(preview_row(entity_id, accounts, &mut seen, outcome));
    }

    Ok(CsvImportPreview { source, rows })
}

fn preview_row(
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    seen: &mut HashSet<DedupeKey>,
    outcome: CsvRowOutcome,
) -> CsvImportPreviewRow {
    match outcome {
        CsvRowOutcome::Invalid {
            source_row,
            message,
        } => CsvImportPreviewRow {
            source_row,
            duplicate: false,
            error: Some(message),
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

/// Post selected simple-entry rows via [`post_simple_entry_unchecked`].
///
/// Rows that match the duplicate rule are skipped unless `include_duplicates`
/// is true. Intra-batch duplicates are skipped the same way. Junk (non-positive
/// amount, missing/wrong role accounts, unbalanced lines) fails the whole
/// batch and rolls back.
///
/// # Errors
///
/// Empty `rows` succeeds with no posts. Mixed `entity_id` values, posting
/// failures, or database errors.
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
            return Err(Error::Validation(
                "import rows must belong to a single entity".into(),
            ));
        }
    }
    let _entity = get_entity(conn, entity_id)?;

    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let mut seen = load_active_keys(&tx, entity_id)?;
    let mut posted: Vec<PostedEntryView> = Vec::new();
    let mut skipped_duplicate_count = 0u32;

    for row in rows {
        let key = DedupeKey::new(&row.entry_date, row.amount_minor, &row.description);
        if !include_duplicates && seen.contains(&key) {
            skipped_duplicate_count = skipped_duplicate_count.saturating_add(1);
            continue;
        }
        let view = post_simple_entry_unchecked(&tx, row)?;
        seen.insert(key);
        posted.push(view);
    }

    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(CsvImportPostResult {
        posted,
        skipped_duplicate_count,
    })
}

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

fn load_active_keys(conn: &Connection, entity_id: EntityId) -> Result<HashSet<DedupeKey>> {
    let mut stmt = conn
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

    let mapped = stmt
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
