//! Account balance helpers.

use rusqlite::Connection;
use time::Date;

use crate::domain::{AccountId, AccountType, EntityId};
use crate::error::{Error, Result};
use crate::util::format_date;

/// Signed normal balance for an account type given raw debit/credit totals.
#[must_use]
pub fn normal_balance(account_type: AccountType, debits: i64, credits: i64) -> i64 {
    if account_type.is_debit_normal() {
        debits.saturating_sub(credits)
    } else {
        credits.saturating_sub(debits)
    }
}

/// Posted entries that are not voided and are not void-reversals.
///
/// A void posts a reverse entry and sets `voided_by` on the original. Newer voids
/// also mark the reverse; older data may only mark the original — so we exclude
/// any entry that is the target of another entry's `voided_by_entry_id`.
pub(crate) const ACTIVE_ENTRY_PREDICATE: &str = "
    je.voided_by_entry_id IS NULL
    AND NOT EXISTS (
        SELECT 1 FROM journal_entries je_void
        WHERE je_void.voided_by_entry_id = je.id
    )
";

/// Balance of one account as of an ISO date string, resolving the account's
/// normal-balance side internally. Convenience wrapper for the IPC layer.
///
/// # Errors
///
/// Unknown account, invalid date, or DB errors.
pub fn account_balance(conn: &Connection, account_id: AccountId, as_of: &str) -> Result<i64> {
    let account = crate::ledger::accounts::get_account(conn, account_id)?;
    let as_of_d = crate::util::parse_date(as_of)?;
    account_balance_as_of(conn, account_id, account.account_type, as_of_d)
}

/// Balance of one account as of `as_of` (inclusive), posted entries only.
///
/// # Errors
///
/// Returns DB errors from the underlying query.
pub fn account_balance_as_of(
    conn: &Connection,
    account_id: AccountId,
    account_type: AccountType,
    as_of: Date,
) -> Result<i64> {
    let as_of_s = format_date(as_of);
    let sql = format!(
        "
        SELECT
            COALESCE(SUM(jl.debit_minor), 0),
            COALESCE(SUM(jl.credit_minor), 0)
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        WHERE jl.account_id = ?1
          AND je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
          AND je.entry_date <= ?2
        "
    );
    let (debits, credits): (i64, i64) = conn
        .query_row(
            &sql,
            rusqlite::params![account_id.0.to_string(), as_of_s],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    Ok(normal_balance(account_type, debits, credits))
}

/// Sum normal balances for all accounts of given types for an entity as of date.
pub(crate) fn sum_types_as_of(
    conn: &Connection,
    entity_id: EntityId,
    types: &[AccountType],
    as_of: Date,
) -> Result<i64> {
    let mut total = 0_i64;
    for account_type in types {
        let type_s = account_type_str(*account_type);
        let as_of_s = format_date(as_of);
        let sql = format!(
            "
            SELECT
                COALESCE(SUM(jl.debit_minor), 0),
                COALESCE(SUM(jl.credit_minor), 0)
            FROM journal_lines jl
            JOIN journal_entries je ON je.id = jl.entry_id
            JOIN accounts a ON a.id = jl.account_id
            WHERE a.entity_id = ?1
              AND a.account_type = ?2
              AND je.status = 'posted'
              AND {ACTIVE_ENTRY_PREDICATE}
              AND je.entry_date <= ?3
            "
        );
        let (debits, credits): (i64, i64) = conn
            .query_row(
                &sql,
                rusqlite::params![entity_id.0.to_string(), type_s, as_of_s],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|err| Error::Io(err.to_string()))?;

        total = total.saturating_add(normal_balance(*account_type, debits, credits));
    }
    Ok(total)
}

/// Sum activity in date range (inclusive) for account types (for P&L).
pub(crate) fn sum_types_in_range(
    conn: &Connection,
    entity_id: EntityId,
    types: &[AccountType],
    from: Date,
    to: Date,
) -> Result<i64> {
    let mut total = 0_i64;
    for account_type in types {
        let type_s = account_type_str(*account_type);
        let from_s = format_date(from);
        let to_s = format_date(to);
        let sql = format!(
            "
            SELECT
                COALESCE(SUM(jl.debit_minor), 0),
                COALESCE(SUM(jl.credit_minor), 0)
            FROM journal_lines jl
            JOIN journal_entries je ON je.id = jl.entry_id
            JOIN accounts a ON a.id = jl.account_id
            WHERE a.entity_id = ?1
              AND a.account_type = ?2
              AND je.status = 'posted'
              AND {ACTIVE_ENTRY_PREDICATE}
              AND je.entry_date >= ?3
              AND je.entry_date <= ?4
            "
        );
        let (debits, credits): (i64, i64) = conn
            .query_row(
                &sql,
                rusqlite::params![entity_id.0.to_string(), type_s, from_s, to_s],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|err| Error::Io(err.to_string()))?;

        total = total.saturating_add(normal_balance(*account_type, debits, credits));
    }
    Ok(total)
}

/// Persist `AccountType` as the stable on-disk string.
pub(crate) fn account_type_str(t: AccountType) -> &'static str {
    match t {
        AccountType::Asset => "asset",
        AccountType::Liability => "liability",
        AccountType::Equity => "equity",
        AccountType::Income => "income",
        AccountType::Expense => "expense",
    }
}

/// Parse the on-disk account-type string written by [`account_type_str`].
pub(crate) fn parse_account_type(s: &str) -> Result<AccountType> {
    match s {
        "asset" => Ok(AccountType::Asset),
        "liability" => Ok(AccountType::Liability),
        "equity" => Ok(AccountType::Equity),
        "income" => Ok(AccountType::Income),
        "expense" => Ok(AccountType::Expense),
        other => Err(Error::VaultCorrupt(format!(
            "unknown account type: {other}"
        ))),
    }
}
