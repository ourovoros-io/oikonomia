//! Account balance helpers.

use rusqlite::Connection;
use time::Date;

use crate::db::corrupt_column;
use crate::domain::{AccountId, AccountType, EntityId};
use crate::error::{Error, Result};
use crate::util::format_date;

/// Returns the signed normal balance for an account type given raw debit and
/// credit totals.
///
/// # Errors
///
/// [`Error::MoneyOverflow`] when the difference does not fit in `i64`. Totals
/// read from the journal are sums of `debit_minor` and `credit_minor`, which
/// the schema's `CHECK (... >= 0)` keeps non-negative; two values in
/// `0..=i64::MAX` always have a difference that fits. The check covers a
/// caller that passes a negative total.
pub fn normal_balance(account_type: AccountType, debits: i64, credits: i64) -> Result<i64> {
    if account_type.is_debit_normal() {
        subtract_minor(debits, credits)
    } else {
        subtract_minor(credits, debits)
    }
}

/// Adds two signed amounts in minor units.
///
/// # Errors
///
/// [`Error::MoneyOverflow`] when the sum does not fit in `i64`.
pub(crate) fn add_minor(left: i64, right: i64) -> Result<i64> {
    left.checked_add(right).ok_or(Error::MoneyOverflow)
}

/// Subtracts `right` from `left`, both signed amounts in minor units.
///
/// # Errors
///
/// [`Error::MoneyOverflow`] when the difference does not fit in `i64`.
pub(crate) fn subtract_minor(left: i64, right: i64) -> Result<i64> {
    left.checked_sub(right).ok_or(Error::MoneyOverflow)
}

/// Adds up signed amounts in minor units.
///
/// # Errors
///
/// [`Error::MoneyOverflow`] when a running sum does not fit in `i64`.
pub(crate) fn sum_minor(amounts: impl IntoIterator<Item = i64>) -> Result<i64> {
    amounts.into_iter().try_fold(0_i64, add_minor)
}

/// SQL predicate on `journal_entries je`: posted entries that are not voided
/// and are not void-reversals.
///
/// The status test is part of the predicate so that a query cannot use the
/// void test and forget it.
///
/// A void posts a reverse entry and sets `voided_by` on the original. Newer voids
/// also mark the reverse; older data may only mark the original — so we exclude
/// any entry that is the target of another entry's `voided_by_entry_id`.
pub(crate) const ACTIVE_ENTRY_PREDICATE: &str = "
    je.status = 'posted'
    AND je.voided_by_entry_id IS NULL
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
/// [`Error::Io`] from the underlying query.
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

    normal_balance(account_type, debits, credits)
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

        total = add_minor(total, normal_balance(*account_type, debits, credits)?)?;
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

        total = add_minor(total, normal_balance(*account_type, debits, credits)?)?;
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
        other => Err(corrupt_column(
            "accounts.account_type",
            format_args!("unknown account type: {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_balance_follows_the_normal_side_of_the_account_type() {
        assert_eq!(normal_balance(AccountType::Asset, 700, 200), Ok(500));
        assert_eq!(normal_balance(AccountType::Expense, 200, 700), Ok(-500));
        assert_eq!(normal_balance(AccountType::Liability, 200, 700), Ok(500));
        assert_eq!(normal_balance(AccountType::Income, 700, 200), Ok(-500));
    }

    #[test]
    fn normal_balance_refuses_a_difference_that_does_not_fit() {
        assert_eq!(
            normal_balance(AccountType::Asset, i64::MIN, 1),
            Err(Error::MoneyOverflow)
        );
        assert_eq!(
            normal_balance(AccountType::Income, i64::MIN, 1),
            Err(Error::MoneyOverflow)
        );
        assert_eq!(
            normal_balance(AccountType::Asset, i64::MAX, 0),
            Ok(i64::MAX)
        );
    }

    #[test]
    fn the_active_entry_predicate_alone_selects_posted_unvoided_entries() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        conn.execute_batch(
            "
            CREATE TABLE journal_entries (
                id TEXT PRIMARY KEY NOT NULL,
                status TEXT NOT NULL,
                voided_by_entry_id TEXT
            );
            INSERT INTO journal_entries VALUES
                ('posted', 'posted', NULL),
                ('draft', 'draft', NULL),
                ('voided', 'posted', 'reversal'),
                ('reversal', 'posted', 'voided'),
                ('voided-before-reversals-were-marked', 'posted', 'old-reversal'),
                ('old-reversal', 'posted', NULL);
            ",
        )
        .expect("create and fill the table");

        let sql = format!("SELECT je.id FROM journal_entries je WHERE {ACTIVE_ENTRY_PREDICATE}");
        let active: Vec<String> = conn
            .prepare(&sql)
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows");

        assert_eq!(active, ["posted"]);
    }

    #[test]
    fn sums_refuse_a_total_that_does_not_fit() {
        assert_eq!(sum_minor([i64::MAX, -1, 1]), Ok(i64::MAX));
        assert_eq!(sum_minor([i64::MAX, 1]), Err(Error::MoneyOverflow));
        assert_eq!(sum_minor([i64::MIN, -1]), Err(Error::MoneyOverflow));
        assert_eq!(sum_minor([]), Ok(0));
        assert_eq!(subtract_minor(i64::MIN, 1), Err(Error::MoneyOverflow));
    }
}
