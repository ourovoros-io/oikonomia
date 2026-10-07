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
use time::Date;

use crate::csv::parse::{parse_bank_csv, read_csv_text};
use crate::csv::{
    CsvColumnMapping, CsvImportAccounts, CsvImportPostResult, CsvImportPreview,
    CsvImportPreviewRow, CsvRowOutcome, normalize_description, suggested_entry,
};
use crate::db::{collect_rows, read_column, stored_date};
use crate::domain::{AccountId, EntityId};
use crate::error::{DatabaseContext, Error, Result, ValidationError};
use crate::ledger::{
    ACTIVE_ENTRY_PREDICATE, PostSimpleEntryRequest, PostedEntryView, SimpleBillStatus,
    SimpleEntryKind, get_account, get_entity, post_simple_entry_unchecked,
};
use crate::util::parse_date;

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
/// - [`Error::Csv`] for a problem with the file as a whole, as
///   [`parse_bank_csv`] lists them. A bad row is not an error; it is a row
///   of the preview with `error` set.
/// - [`Error::VaultCorrupt`] for a stored entry the duplicate rule cannot
///   read: a date that does not parse, or a date, a description or an amount
///   of the wrong storage class.
/// - [`Error::Database`] on database errors.
pub fn preview_bank_csv(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    csv_text: &str,
    mapping: Option<&CsvColumnMapping>,
) -> Result<CsvImportPreview> {
    let statement = Statement {
        csv_text,
        mapping,
        source: String::new(),
    };
    preview_statement(conn, entity_id, accounts, statement)
}

/// Reads the CSV file at `path` and previews it as [`preview_bank_csv`]
/// does. **Does not post.**
///
/// The preview's `source` is `path` as displayed.
///
/// # Errors
///
/// - [`Error::Io`] when `path` cannot be read or is not a regular file.
/// - [`Error::Csv`] when the file is larger than
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
    let statement = Statement {
        csv_text: &text,
        mapping,
        source: path.display().to_string(),
    };
    preview_statement(conn, entity_id, accounts, statement)
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
/// - [`ValidationError::InvalidDate`] for a row whose `entry_date` is not a
///   `YYYY-MM-DD` date. The date is read before the duplicate rule is
///   applied, so this is reported for any row.
/// - [`ValidationError::AmountNotPositive`],
///   [`ValidationError::BillStatusRequired`] or
///   [`ValidationError::AccountRequired`] for a row that is to be posted and
///   does not convert into a
///   [`PostSimpleEntry`](crate::ledger::PostSimpleEntry). A row skipped as a
///   duplicate is not converted.
/// - Every error of [`post_simple_entry`](crate::ledger::post_simple_entry)
///   for a row the ledger refuses, such as an account of the wrong type.
/// - [`Error::VaultCorrupt`] for a stored entry date that does not parse.
/// - [`Error::Database`] on database errors.
pub fn post_import_rows(
    conn: &Connection,
    rows: &[PostSimpleEntryRequest],
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
            return Err(ValidationError::Internal {
                detail: "import rows must belong to a single entity".into(),
            }
            .into());
        }
    }
    let _entity = get_entity(conn, entity_id)?;

    let transaction = conn.unchecked_transaction().database("begin CSV import")?;
    let mut seen = load_active_movements(&transaction, entity_id)?;
    let mut posted: Vec<PostedEntryView> = Vec::new();
    let mut skipped_duplicate_count = 0u32;

    for row in rows {
        // The date is read first because the duplicate rule needs it. The
        // rest of the row is checked only once it is known to be posted, so
        // a skipped duplicate is never refused for what else it holds. The
        // conversion below takes the date parsed here.
        let entry_date = parse_date(&row.entry_date)?;
        let movement = Movement::new(
            entry_date,
            row.amount_minor,
            Flow::of_request(row),
            &row.description,
        );
        if !include_duplicates && seen.contains(&movement) {
            skipped_duplicate_count = skipped_duplicate_count.saturating_add(1);
            continue;
        }
        let entry = row.dated(entry_date)?;
        let view = post_simple_entry_unchecked(&transaction, &entry)?;
        seen.record(&movement);
        posted.push(view);
    }

    transaction.commit().database("commit CSV import")?;
    Ok(CsvImportPostResult {
        posted,
        skipped_duplicate_count,
    })
}

/// Which way an entry moves money, as a bank statement signs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Money received: a positive amount on a statement.
    In,
    /// Money spent or owed: a negative amount on a statement.
    Out,
    /// Money moved between the book's own accounts, which changes no income
    /// and no expense: a transfer, or the payment of a bill recorded before.
    /// The statement of the account it left shows it negative and the
    /// statement of the account it reached shows it positive, so it counts
    /// as both.
    Internal,
}

impl Flow {
    /// Returns the flow of a parsed statement row from its signed amount.
    const fn of_signed(signed_amount_minor: i64) -> Self {
        if signed_amount_minor < 0 {
            Self::Out
        } else {
            Self::In
        }
    }

    /// Returns the flow of a row to post, from its kind and bill status
    /// alone.
    ///
    /// A bill that does not say whether it is paid counts as money out. Such
    /// a row cannot be posted, so the answer only decides whether it is
    /// skipped as a duplicate before it is refused.
    const fn of_request(row: &PostSimpleEntryRequest) -> Self {
        match (row.kind, row.bill_status) {
            (SimpleEntryKind::Income, _) => Self::In,
            (SimpleEntryKind::Expense, _)
            | (
                SimpleEntryKind::Bill,
                Some(SimpleBillStatus::Paid | SimpleBillStatus::Unpaid) | None,
            ) => Self::Out,
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::PayExisting))
            | (SimpleEntryKind::Transfer, _) => Self::Internal,
        }
    }

    /// Returns the flow of a ledger entry from what it adds to the result:
    /// the credits minus the debits of its lines on income and expense
    /// accounts.
    const fn of_result(result_minor: i64) -> Self {
        match result_minor {
            0 => Self::Internal,
            minor if minor > 0 => Self::In,
            _ => Self::Out,
        }
    }
}

/// One entry as the duplicate rule sees it: the values the [`crate::csv`]
/// module doc names.
///
/// Built only through [`Movement::new`], so the description is always in its
/// normalized form.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Movement {
    /// Booking date.
    date: Date,
    /// Size of the entry in minor units, without a sign.
    amount_minor: i64,
    /// Which way the money moved.
    flow: Flow,
    /// Description after [`normalize_description`].
    description: String,
}

impl Movement {
    /// Returns the movement of an entry, normalizing `description`.
    fn new(date: Date, amount_minor: i64, flow: Flow, description: &str) -> Self {
        Self {
            date,
            amount_minor,
            flow,
            description: normalize_description(description),
        }
    }

    /// Returns the keys two movements have in common exactly when they are
    /// duplicates: one for money in or out, and both signs for an internal
    /// movement.
    ///
    /// A movement whose amount is not positive has no key. No entry in the
    /// ledger has such an amount, and a row with one is refused when it is
    /// posted; without a key it is never skipped as a duplicate first.
    fn keys(&self) -> impl Iterator<Item = DedupeKey> + '_ {
        let amount = Some(self.amount_minor).filter(|minor| *minor > 0);
        // A positive `i64` always has a negation that fits.
        let negated = amount.and_then(i64::checked_neg);
        let signed = match self.flow {
            Flow::In => [amount, None],
            Flow::Out => [negated, None],
            Flow::Internal => [amount, negated],
        };

        signed
            .into_iter()
            .flatten()
            .map(|signed_amount_minor| DedupeKey {
                date: self.date,
                signed_amount_minor,
                description: self.description.clone(),
            })
    }
}

/// What two duplicate entries share: the date, the amount signed as a bank
/// statement signs it (negative for money out), and the normalized
/// description.
///
/// The sign is part of the key so that an expense and an income of one size
/// on one day with one text, such as a purchase and its refund, are two
/// entries and not one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DedupeKey {
    /// Booking date.
    date: Date,
    /// Amount in minor units, negative for money out.
    signed_amount_minor: i64,
    /// Description after [`normalize_description`].
    description: String,
}

/// The movements a row is compared with: the active entries of the ledger,
/// and the rows that came before it.
#[derive(Debug, Default)]
struct SeenMovements {
    /// Every key of every movement recorded.
    keys: HashSet<DedupeKey>,
}

impl SeenMovements {
    /// Returns whether `movement` duplicates one recorded before.
    fn contains(&self, movement: &Movement) -> bool {
        movement.keys().any(|key| self.keys.contains(&key))
    }

    /// Adds `movement` to what later ones are compared with.
    fn record(&mut self, movement: &Movement) {
        self.keys.extend(movement.keys());
    }
}

/// A bank statement to preview.
struct Statement<'a> {
    /// The statement as CSV text.
    csv_text: &'a str,
    /// The columns to read, or `None` to detect them from the header row.
    mapping: Option<&'a CsvColumnMapping>,
    /// What the preview reports the text came from; empty for in-memory
    /// text.
    source: String,
}

/// The shared body of the two previews.
///
/// # Errors
///
/// Those of [`preview_bank_csv`].
fn preview_statement(
    conn: &Connection,
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    statement: Statement<'_>,
) -> Result<CsvImportPreview> {
    let entity = get_entity(conn, entity_id)?;
    check_role_account(conn, entity_id, accounts.wallet_account_id)?;
    check_role_account(conn, entity_id, accounts.expense_account_id)?;
    check_role_account(conn, entity_id, accounts.income_account_id)?;

    let parsed = parse_bank_csv(statement.csv_text, entity.base_currency, statement.mapping)?;
    let mut seen = PreviewSeen {
        ledger: load_active_movements(conn, entity_id)?,
        earlier_rows: SeenMovements::default(),
    };
    let mut rows = Vec::with_capacity(parsed.rows.len());

    for outcome in parsed.rows {
        rows.push(preview_row(entity_id, accounts, &mut seen, outcome));
    }

    Ok(CsvImportPreview {
        source: statement.source,
        headers: parsed.headers,
        detected_mapping: parsed.detected_mapping,
        rows,
    })
}

/// What the rows of a preview are compared with, kept apart because the
/// amount Oikonomia 0.1.0 read for a row is compared with the ledger only.
#[derive(Debug)]
struct PreviewSeen {
    /// The active entries of the ledger.
    ledger: SeenMovements,
    /// The parsed rows of this file before the current one.
    earlier_rows: SeenMovements,
}

/// Turns one parse outcome into a preview row, recording it in `seen`.
///
/// A parsed row is a duplicate when its movement is in the ledger or repeats
/// an earlier row of this file. It is also flagged when the movement
/// Oikonomia 0.1.0 read for it is in the ledger, because 0.1.0 imported a
/// negative debit or credit cell the other way round. That second check is
/// against the ledger only: within one file every row is read the same way.
fn preview_row(
    entity_id: EntityId,
    accounts: CsvImportAccounts,
    seen: &mut PreviewSeen,
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
            let movement = Movement::new(
                row.entry_date,
                row.amount_minor,
                Flow::of_signed(row.signed_amount_minor),
                &row.description,
            );
            let legacy_movement = row.legacy_signed_amount_minor.and_then(|legacy| {
                let amount_minor = legacy.checked_abs()?;
                Some(Movement::new(
                    row.entry_date,
                    amount_minor,
                    Flow::of_signed(legacy),
                    &row.description,
                ))
            });
            let duplicate = seen.ledger.contains(&movement)
                || seen.earlier_rows.contains(&movement)
                || legacy_movement.is_some_and(|legacy| seen.ledger.contains(&legacy));
            seen.earlier_rows.record(&movement);
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

/// Loads the movement of every active entry of `entity_id`: posted, not
/// voided, and not itself the reversal of a voided entry.
///
/// An entry's amount is the sum of its debit lines, which for a balanced
/// entry is its total, and its flow follows what its lines on income and
/// expense accounts add to the result ([`Flow::of_result`]). Every entry of
/// the book is read on each call; there is no index on the key.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] naming the column when a stored entry date does
///   not parse, or a date, a description or an amount has the wrong storage
///   class.
/// - [`Error::Database`] on database errors, which include a total that
///   overflows `i64` inside `SQLite`'s `SUM`.
fn load_active_movements(conn: &Connection, entity_id: EntityId) -> Result<SeenMovements> {
    let sql = format!(
        "
        SELECT je.entry_date, je.description,
               COALESCE(SUM(jl.debit_minor), 0) AS amount_minor,
               COALESCE(SUM(
                   CASE WHEN a.account_type IN ('income', 'expense')
                        THEN jl.credit_minor - jl.debit_minor
                        ELSE 0
                   END
               ), 0) AS result_minor
        FROM journal_entries je
        JOIN journal_lines jl ON jl.entry_id = je.id
        JOIN accounts a ON a.id = jl.account_id
        WHERE je.entity_id = ?1
          AND {ACTIVE_ENTRY_PREDICATE}
        GROUP BY je.id, je.entry_date, je.description
        "
    );
    let mut statement = conn
        .prepare(&sql)
        .database("read entries for duplicate check")?;

    let mapped = statement
        .query_map([entity_id.to_string()], |row| Ok(map_active_movement(row)))
        .database("read entries for duplicate check")?;

    let mut seen = SeenMovements::default();
    for movement in collect_rows("read entries for duplicate check", mapped)? {
        seen.record(&movement);
    }
    Ok(seen)
}

/// Maps a row selected as `entry_date, description, amount_minor,
/// result_minor` to its movement.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when the date does not parse or
/// a column has the wrong storage class.
fn map_active_movement(row: &rusqlite::Row<'_>) -> Result<Movement> {
    let date = stored_date(
        "journal_entries.entry_date",
        &read_column::<String>(row, 0)?,
    )?;
    let description: String = read_column(row, 1)?;
    let amount_minor = read_column(row, 2)?;
    let flow = Flow::of_result(read_column(row, 3)?);

    Ok(Movement::new(date, amount_minor, flow, &description))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A movement of `amount_minor` on 15 March 2026 described as "Shop".
    fn movement(amount_minor: i64, flow: Flow) -> Movement {
        Movement::new(
            time::macros::date!(2026 - 03 - 15),
            amount_minor,
            flow,
            "Shop",
        )
    }

    /// The signed amounts of the keys of a movement.
    fn signed_amounts(amount_minor: i64, flow: Flow) -> Vec<i64> {
        movement(amount_minor, flow)
            .keys()
            .map(|key| key.signed_amount_minor)
            .collect()
    }

    #[test]
    fn a_key_is_signed_by_the_flow_and_an_internal_movement_has_both_signs() {
        assert_eq!(signed_amounts(2_500, Flow::In), [2_500]);
        assert_eq!(signed_amounts(2_500, Flow::Out), [-2_500]);
        assert_eq!(signed_amounts(2_500, Flow::Internal), [2_500, -2_500]);
        assert_eq!(signed_amounts(i64::MAX, Flow::Out), [-i64::MAX]);
    }

    #[test]
    fn an_amount_that_is_not_positive_has_no_key_and_so_duplicates_nothing() {
        for amount_minor in [0, -2_500, i64::MIN] {
            for flow in [Flow::In, Flow::Out, Flow::Internal] {
                assert_eq!(
                    signed_amounts(amount_minor, flow),
                    [0_i64; 0],
                    "{amount_minor}"
                );
            }
        }

        let mut seen = SeenMovements::default();
        seen.record(&movement(2_500, Flow::Out));
        assert!(!seen.contains(&movement(-2_500, Flow::In)));
    }

    #[test]
    fn opposite_flows_are_not_duplicates_and_an_internal_one_matches_both() {
        let mut seen = SeenMovements::default();
        seen.record(&movement(2_500, Flow::Out));

        assert!(seen.contains(&movement(2_500, Flow::Out)));
        assert!(!seen.contains(&movement(2_500, Flow::In)));
        assert!(seen.contains(&movement(2_500, Flow::Internal)));
        assert!(!seen.contains(&movement(2_501, Flow::Out)));
    }
}
