//! Balances: which entries count, how a balance is signed, and how amounts
//! are added up.
//!
//! # Active entries
//!
//! Every balance and report counts *active* entries only: posted, not voided,
//! and not the reversing entry of a void. [`ACTIVE_ENTRY_PREDICATE`] is that
//! rule as SQL, and every query in the ledger that totals amounts embeds it
//! instead of spelling the rule again.
//!
//! # Sign
//!
//! The journal stores a debit and a credit amount per line, both
//! non-negative. A balance is signed towards the normal side of the account's
//! type: debits minus credits for assets and expenses, credits minus debits
//! for liabilities, equity and income ([`normal_balance`]). A negative balance
//! is therefore an account on the other side of where its type usually sits,
//! such as an overdrawn bank account.
//!
//! # Arithmetic
//!
//! Amounts are added and subtracted through [`add_minor`], [`subtract_minor`]
//! and [`sum_minor`], which return [`Error::MoneyOverflow`] where plain `i64`
//! arithmetic would wrap or panic.
//!
//! The module also reads back the text an account type is stored as
//! ([`parse_account_type`]; [`AccountType::identifier`] writes it), because
//! the queries here filter on it.

use crate::db::corrupt_column;
use crate::domain::{AccountId, AccountType, EntityId};
use crate::error::{DatabaseContext, Error, Result};
use crate::ledger::accounts::get_account;
use crate::util::format_date;
use rusqlite::Connection;
use time::Date;

/// SQL predicate on `journal_entries je` that selects the active entries:
/// posted, not voided, and not the reversing entry of a void.
///
/// The status test is part of the predicate so that a query cannot use the
/// void test and forget it.
///
/// A void posts a reversing entry and links the pair both ways through
/// `voided_by_entry_id`, so the first void test alone excludes both. The
/// `NOT EXISTS` test is for a vault whose voids linked only the original to
/// its reversal: there the reversal has no link of its own and is recognised
/// as the target of the original's.
pub(crate) const ACTIVE_ENTRY_PREDICATE: &str = "
    je.status = 'posted'
    AND je.voided_by_entry_id IS NULL
    AND NOT EXISTS (
        SELECT 1 FROM journal_entries je_void
        WHERE je_void.voided_by_entry_id = je.id
    )
";

/// Returns a balance signed towards the normal side of `account_type`.
///
/// `debits` and `credits` are totals in minor units. The result is
/// `debits - credits` for an asset or expense account and `credits - debits`
/// for a liability, equity or income account.
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

/// Returns the balance of one account through `as_of` inclusive, signed
/// towards the normal side of the account's type.
///
/// This is [`account_balance_as_of`] for a caller that has only the account's
/// id; it looks the account's type up first.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown account.
/// - [`Error::VaultCorrupt`] when the stored account does not parse.
/// - The errors of [`account_balance_as_of`].
pub fn account_balance(conn: &Connection, account_id: AccountId, as_of: Date) -> Result<i64> {
    let account = get_account(conn, account_id)?;

    account_balance_as_of(conn, account_id, account.account_type, as_of)
}

/// Returns the balance of one account through `as_of` (inclusive), counting
/// active entries only.
///
/// `account_type` decides the sign, as in [`normal_balance`]; it is the
/// caller's to pass so that a caller that already holds the account does not
/// pay for a second lookup. An unknown `account_id` has no lines and gives 0.
///
/// # Errors
///
/// [`Error::Database`] when the query fails, which includes a debit or credit total
/// that overflows `i64` inside `SQLite`'s `SUM`.
pub fn account_balance_as_of(
    conn: &Connection,
    account_id: AccountId,
    account_type: AccountType,
    as_of: Date,
) -> Result<i64> {
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
            rusqlite::params![account_id.to_string(), format_date(as_of)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .database("sum account balance")?;

    normal_balance(account_type, debits, credits)
}

/// Sums the balances of every account of one type in an entity, through
/// `as_of` (inclusive), counting active entries only.
///
/// The total is signed towards the normal side of `account_type`.
///
/// # Errors
///
/// - [`Error::MoneyOverflow`] when the balance does not fit in `i64`.
/// - [`Error::Database`] when the query fails, which includes a total that
///   overflows `i64` inside `SQLite`'s `SUM`.
pub(crate) fn sum_type_as_of(
    conn: &Connection,
    entity_id: EntityId,
    account_type: AccountType,
    as_of: Date,
) -> Result<i64> {
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
            rusqlite::params![
                entity_id.to_string(),
                account_type.identifier(),
                format_date(as_of),
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .database("sum balances by account type")?;

    normal_balance(account_type, debits, credits)
}

/// Sums the activity of every account of one type in an entity, between
/// `from` and `to` (both inclusive), counting active entries only.
///
/// The sign is as in [`sum_type_as_of`]. Profit and loss figures come from
/// here: income and expense accounts are read over a window, never as of a
/// date.
///
/// # Errors
///
/// - [`Error::MoneyOverflow`] when the total does not fit in `i64`.
/// - [`Error::Database`] when the query fails, which includes a total that
///   overflows `i64` inside `SQLite`'s `SUM`.
pub(crate) fn sum_type_in_range(
    conn: &Connection,
    entity_id: EntityId,
    account_type: AccountType,
    from: Date,
    to: Date,
) -> Result<i64> {
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
            rusqlite::params![
                entity_id.to_string(),
                account_type.identifier(),
                format_date(from),
                format_date(to),
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .database("sum activity by account type")?;

    normal_balance(account_type, debits, credits)
}

/// Counts the hidden active entries dated in `from..=to` that touch an income
/// or expense account.
///
/// These are the entries that move a profit-and-loss figure on screen and
/// that an export leaves out, so the count is the "includes N hidden entries"
/// a screen can say. `None` leaves that end of the window open. An entry with
/// several such lines counts once.
///
/// # Errors
///
/// [`Error::Database`] when the query fails.
pub(crate) fn count_hidden_pnl_entries(
    conn: &Connection,
    entity_id: EntityId,
    from: Option<Date>,
    to: Option<Date>,
) -> Result<usize> {
    let sql = format!(
        "
        SELECT COUNT(DISTINCT je.id)
        FROM journal_entries je
        JOIN journal_lines jl ON jl.entry_id = je.id
        JOIN accounts a ON a.id = jl.account_id
        WHERE je.entity_id = ?1
          AND je.hidden = 1
          AND a.account_type IN ('income', 'expense')
          AND {ACTIVE_ENTRY_PREDICATE}
          AND (?2 IS NULL OR je.entry_date >= ?2)
          AND (?3 IS NULL OR je.entry_date <= ?3)
        "
    );

    let count: i64 = conn
        .query_row(
            &sql,
            rusqlite::params![
                entity_id.to_string(),
                from.map(format_date),
                to.map(format_date),
            ],
            |row| row.get(0),
        )
        .database("count hidden entries")?;

    Ok(usize::try_from(count).unwrap_or(0))
}

/// Parses the text [`AccountType::identifier`] writes into
/// `accounts.account_type`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `accounts.account_type` when `stored` is
/// none of the five strings.
pub(crate) fn parse_account_type(stored: &str) -> Result<AccountType> {
    match stored {
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
    fn an_account_type_is_read_back_from_the_text_it_is_stored_as() {
        for account_type in [
            AccountType::Asset,
            AccountType::Liability,
            AccountType::Equity,
            AccountType::Income,
            AccountType::Expense,
        ] {
            assert_eq!(
                parse_account_type(account_type.identifier()),
                Ok(account_type)
            );
        }
        assert_eq!(
            parse_account_type("Asset").map_err(|error| error.code()),
            Err("vault_corrupt")
        );
    }

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
