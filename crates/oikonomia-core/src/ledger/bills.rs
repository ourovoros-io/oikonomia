//! Open bills and the payments that settle them.
//!
//! A payment posted as "pay an existing bill" may name the bill it settles
//! (`journal_entries.settles_entry_id`, schema v12). One payment settles one
//! bill, in full: it pays the bill's whole amount from the payable account
//! the bill is owed on. There are no partial payments.
//!
//! # Open bills
//!
//! An open bill is an active entry posted as a bill with the status unpaid
//! (stored since schema v11) that no active payment settles. Voiding the
//! payment reopens the bill. Bills posted before v11 store no status and are
//! never open: the payments of that time named no bill, so whether one of
//! them was paid is not known.
//!
//! # A settled bill stays as it is
//!
//! A bill that an active payment settles cannot be voided or corrected
//! ([`ValidationError::BillSettled`]): the payment would be left paying
//! nothing. The user voids the payment first, which reopens the bill.

use crate::db::{collect_rows, read_column, stored_date, stored_id};
use crate::domain::{AccountId, EntityId, JournalEntryId};
use crate::error::{DatabaseContext, Error, Resource, Result, ValidationError};
use crate::ledger::balance::ACTIVE_ENTRY_PREDICATE;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Date;

/// An unpaid bill no payment settles yet, as the bill picker of the payment
/// form shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenBill {
    /// The bill's entry, which a payment names to settle it.
    pub entry_id: JournalEntryId,
    /// Accounting date of the bill.
    #[serde(with = "crate::util::serde_date")]
    pub entry_date: Date,
    /// Description of the bill, as stored.
    pub description: String,
    /// The bill's amount in minor units: what a payment of it must pay.
    pub amount_minor: i64,
    /// The liability account the bill is owed on: what a payment of it must
    /// debit.
    pub payable_account_id: AccountId,
}

/// Matches a payment `pay` that is in the books.
///
/// A voided payment has its `voided_by_entry_id` set, and a reversal never
/// names a bill, so a payment that is out of the books never counts.
const ACTIVE_PAYMENT_PREDICATE: &str = "
    pay.status = 'posted' AND pay.voided_by_entry_id IS NULL
";

/// Lists the open bills of an entity, oldest first.
///
/// With `payment`, a bill that payment settles is listed as open too: the
/// form that corrects the payment offers its bill again, and correcting it
/// voids the payment before the replacement settles the bill.
///
/// An entity that does not exist has no bills and gives an empty list.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] for a stored id, date or amount that does not
/// parse; [`Error::Database`] on database errors.
pub fn list_open_bills(
    conn: &Connection,
    entity_id: EntityId,
    payment: Option<JournalEntryId>,
) -> Result<Vec<OpenBill>> {
    query_open_bills(conn, entity_id, None, payment)
}

/// Returns the open bill `bill_id` of the book of `entity_id`, for a
/// payment to settle.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entry.
/// - [`ValidationError::WrongBook`] for an entry of another book.
/// - [`ValidationError::BillNotOpen`] for an entry that is not an open bill.
/// - [`Error::VaultCorrupt`] for a stored id, date or amount that does not
///   parse; [`Error::Database`] on database errors.
pub(crate) fn open_bill(
    conn: &Connection,
    entity_id: EntityId,
    bill_id: JournalEntryId,
) -> Result<OpenBill> {
    let bill_entity: String = conn
        .query_row(
            "SELECT entity_id FROM journal_entries WHERE id = ?1",
            [bill_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::JournalEntry),
            other => Error::database("read the bill a payment settles", other),
        })?;
    if bill_entity != entity_id.to_string() {
        return Err(ValidationError::WrongBook.into());
    }

    query_open_bills(conn, entity_id, Some(bill_id), None)?
        .pop()
        .ok_or_else(|| ValidationError::BillNotOpen.into())
}

/// Checks that no active payment settles the entry `id`, before it is
/// voided or corrected.
///
/// # Errors
///
/// [`ValidationError::BillSettled`] when one does; [`Error::Database`] on
/// database errors.
pub(crate) fn ensure_not_settled(conn: &Connection, id: JournalEntryId) -> Result<()> {
    let payments: i64 = conn
        .query_row(
            &format!(
                "
                SELECT COUNT(1) FROM journal_entries pay
                WHERE pay.settles_entry_id = ?1 AND {ACTIVE_PAYMENT_PREDICATE}
                "
            ),
            [id.to_string()],
            |row| row.get(0),
        )
        .database("look for the payment of a bill")?;
    if payments > 0 {
        return Err(ValidationError::BillSettled.into());
    }
    Ok(())
}

/// Reads the open bills of `entity_id`, or only `bill_id` when given,
/// counting a bill that `payment` settles as open.
///
/// A bill has one debit line, on its category, and one credit line, on its
/// payable account, so joining the credit line gives one row per bill.
///
/// # Errors
///
/// Those of [`list_open_bills`].
fn query_open_bills(
    conn: &Connection,
    entity_id: EntityId,
    bill_id: Option<JournalEntryId>,
    payment: Option<JournalEntryId>,
) -> Result<Vec<OpenBill>> {
    const OPERATION: &str = "list open bills";

    let sql = format!(
        "
        SELECT je.id, je.entry_date, je.description, jl.credit_minor, jl.account_id
        FROM journal_entries je
        JOIN journal_lines jl ON jl.entry_id = je.id AND jl.credit_minor > 0
        WHERE je.entity_id = ?1
          AND (?2 IS NULL OR je.id = ?2)
          AND je.kind = 'bill'
          AND je.bill_status = 'unpaid'
          AND {ACTIVE_ENTRY_PREDICATE}
          AND NOT EXISTS (
              SELECT 1 FROM journal_entries pay
              WHERE pay.settles_entry_id = je.id
                AND {ACTIVE_PAYMENT_PREDICATE}
                AND (?3 IS NULL OR pay.id <> ?3)
          )
        ORDER BY je.entry_date ASC, je.created_at ASC, je.rowid ASC
        "
    );
    let mut stmt = conn.prepare(&sql).database(OPERATION)?;
    let rows = stmt
        .query_map(
            rusqlite::params![
                entity_id.to_string(),
                bill_id.map(|id| id.to_string()),
                payment.map(|id| id.to_string()),
            ],
            |row| Ok(map_open_bill_row(OPERATION, row)),
        )
        .database(OPERATION)?;

    collect_rows(OPERATION, rows)
}

/// Maps a row selected as `id, entry_date, description, credit_minor,
/// account_id`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when a value does not parse or
/// has the wrong storage class.
fn map_open_bill_row(operation: &'static str, row: &rusqlite::Row<'_>) -> Result<OpenBill> {
    Ok(OpenBill {
        entry_id: stored_id(
            "journal_entries.id",
            &read_column::<String>(operation, row, 0)?,
        )?,
        entry_date: stored_date(
            "journal_entries.entry_date",
            &read_column::<String>(operation, row, 1)?,
        )?,
        description: read_column(operation, row, 2)?,
        amount_minor: read_column(operation, row, 3)?,
        payable_account_id: stored_id(
            "journal_lines.account_id",
            &read_column::<String>(operation, row, 4)?,
        )?,
    })
}
