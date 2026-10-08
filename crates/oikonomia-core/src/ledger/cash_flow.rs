//! Cash flow over a window: income and expenses per day or per calendar month,
//! with running totals, on exactly the basis
//! [`dashboard_summary`](crate::ledger::dashboard_summary) uses.
//!
//! Income is the activity on income accounts and expenses the activity on
//! expense accounts, counted on active entries by their entry date. A
//! transfer between two wallet accounts touches neither and does not appear.
//!
//! # Buckets
//!
//! A window of at most [`DAILY_BUCKET_MAX_DAYS`] days gets one bucket per day;
//! a longer one gets one per calendar month, the first and last clipped to
//! the window. The buckets are contiguous and cover the whole window, empty
//! ones included, so a chart can draw them without filling gaps.
//!
//! A caller whose date filter may be empty on either side asks through
//! [`cash_flow_series_for_window`], which first settles the window from the
//! book's entries ([`activity_window`]).
//!
//! The ledger is read once, grouped by day and account type. Each day is then
//! placed in its bucket by binary search, and the running totals are added in
//! a second pass over the buckets.

use crate::db::{collect_rows, read_column, stored_date};
use crate::domain::{AccountType, EntityId};
use crate::error::{DatabaseContext, Result, ValidationError};
// Named only by the documentation below.
#[cfg(doc)]
use crate::error::Error;
use crate::ledger::balance::{
    ACTIVE_ENTRY_PREDICATE, add_minor, count_hidden_pnl_entries, normal_balance,
    parse_account_type, subtract_minor,
};
use crate::ledger::entities::get_entity;
use crate::util::format_date;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Date;

/// Windows of this many days or fewer get one bucket per day; longer windows
/// get one per calendar month. 92 days covers any calendar quarter.
pub const DAILY_BUCKET_MAX_DAYS: i32 = 92;

/// How a [`CashFlowSeries`] is bucketed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CashFlowGranularity {
    /// One bucket per calendar day.
    Day,
    /// One bucket per calendar month, clipped to the window.
    Month,
}

/// One bucket of a [`CashFlowSeries`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashFlowBucket {
    /// First day in the bucket (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub start: Date,
    /// Last day in the bucket (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub end: Date,
    /// Income in the bucket: credits minus debits on Income accounts.
    pub income_minor: i64,
    /// Expenses in the bucket: debits minus credits on Expense accounts.
    pub expenses_minor: i64,
    /// Income from the window's first day through `end`.
    pub cumulative_income_minor: i64,
    /// Expenses from the window's first day through `end`.
    pub cumulative_expenses_minor: i64,
}

/// Income and expenses across `[from, to]`, bucketed per day or per month.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashFlowSeries {
    /// Entity the series was computed for.
    pub entity_id: EntityId,
    /// First day of the window (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub from: Date,
    /// Last day of the window (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub to: Date,
    /// Day or month buckets, chosen by the window's length.
    pub granularity: CashFlowGranularity,
    /// Income across the window; equals `dashboard_summary`'s `income`.
    pub total_income_minor: i64,
    /// Expenses across the window; equals `dashboard_summary`'s `expenses`.
    pub total_expenses_minor: i64,
    /// Income minus expenses across the window.
    pub net_minor: i64,
    /// Contiguous buckets covering `from..=to`, oldest first.
    pub buckets: Vec<CashFlowBucket>,
    /// Hidden entries in the window that the totals count, which an export
    /// leaves out.
    pub hidden_entry_count: usize,
}

/// Computes income and expenses per day or per month between `from` and `to`
/// inclusive.
///
/// Posted, active entries only (voided entries and their reversals are
/// excluded); Hidden entries count, as they do on the dashboard. The final
/// running totals always equal `dashboard_summary` for the same window.
///
/// # Errors
///
/// [`ValidationError::DateRangeInverted`] for `from > to`;
/// [`Error::NotFound`] for an unknown entity; [`Error::MoneyOverflow`] when a
/// bucket, a running total or the net does not fit in `i64`;
/// [`Error::VaultCorrupt`] for a stored date or account type that does not
/// parse; database errors as [`Error::Database`].
pub fn cash_flow_series(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
) -> Result<CashFlowSeries> {
    if from > to {
        return Err(ValidationError::DateRangeInverted.into());
    }
    // Only checks that the entity exists. An archived entity passes, so its
    // series can still be read.
    get_entity(conn, entity_id)?;

    let granularity = granularity_for(from, to);
    let mut buckets: Vec<CashFlowBucket> = bucket_ranges(from, to, granularity)
        .into_iter()
        .map(|(start, end)| CashFlowBucket {
            start,
            end,
            income_minor: 0,
            expenses_minor: 0,
            cumulative_income_minor: 0,
            cumulative_expenses_minor: 0,
        })
        .collect();

    for day in daily_activity(conn, entity_id, from, to)? {
        // Buckets are sorted and contiguous, so the first one ending on or
        // after the day holds it.
        let index = buckets.partition_point(|bucket| bucket.end < day.date);
        if let Some(bucket) = buckets.get_mut(index) {
            bucket.income_minor = add_minor(bucket.income_minor, day.income_minor)?;
            bucket.expenses_minor = add_minor(bucket.expenses_minor, day.expenses_minor)?;
        }
    }

    let mut income = 0_i64;
    let mut expenses = 0_i64;
    for bucket in &mut buckets {
        income = add_minor(income, bucket.income_minor)?;
        expenses = add_minor(expenses, bucket.expenses_minor)?;
        bucket.cumulative_income_minor = income;
        bucket.cumulative_expenses_minor = expenses;
    }

    Ok(CashFlowSeries {
        entity_id,
        from,
        to,
        granularity,
        total_income_minor: income,
        total_expenses_minor: expenses,
        net_minor: subtract_minor(income, expenses)?,
        buckets,
        hidden_entry_count: count_hidden_pnl_entries(conn, entity_id, Some(from), Some(to))?,
    })
}

/// Returns the window to draw when a date filter leaves one or both bounds
/// empty.
///
/// - Both given: the window is `from..=to` as given.
/// - Only `from` given: `to` is the book's last active entry date, or `today`
///   when the book has no active entries. If that lands before `from`, `to`
///   is `from`.
/// - Only `to` given: `from` is the book's first active entry date, or `to`
///   itself when the book has no active entries (`today` plays no part). If
///   the first entry is after `to`, `from` is `to`.
/// - Neither given: the first and last active entry dates, or `today` for
///   both when the book has no active entries.
///
/// # Errors
///
/// [`ValidationError::DateRangeInverted`] for an explicit `from > to`;
/// [`Error::NotFound`] for an unknown entity; [`Error::VaultCorrupt`] for a
/// stored entry date that does not parse; database errors as [`Error::Database`].
pub fn activity_window(
    conn: &Connection,
    entity_id: EntityId,
    from: Option<Date>,
    to: Option<Date>,
    today: Date,
) -> Result<(Date, Date)> {
    // Only checks that the entity exists; an archived one passes.
    get_entity(conn, entity_id)?;
    if let (Some(start), Some(end)) = (from, to) {
        if start > end {
            return Err(ValidationError::DateRangeInverted.into());
        }
        return Ok((start, end));
    }

    let (earliest, latest) = active_entry_bounds(conn, entity_id)?;
    match (from, to) {
        (Some(start), None) => Ok((start, latest.unwrap_or(today).max(start))),
        (None, Some(end)) => Ok((earliest.unwrap_or(end).min(end), end)),
        _ => Ok((earliest.unwrap_or(today), latest.unwrap_or(today))),
    }
}

/// Computes the cash flow series for a date filter that may leave one or
/// both bounds empty.
///
/// This is [`cash_flow_series`] over the window [`activity_window`] draws
/// for `from` and `to`; its documentation says how each empty bound is
/// filled, from the book's active entries or from `today`. The
/// buckets are per day or per month by the length of that window, as in
/// [`cash_flow_series`]. `today` is the caller's to pass so that the result
/// does not depend on the clock; the app passes the current date in UTC.
///
/// # Errors
///
/// [`ValidationError::DateRangeInverted`] for an explicit `from > to`;
/// [`Error::NotFound`] for an unknown entity; [`Error::MoneyOverflow`] when a
/// bucket, a running total or the net does not fit in `i64`;
/// [`Error::VaultCorrupt`] for a stored date or account type that does not
/// parse; database errors as [`Error::Database`].
pub fn cash_flow_series_for_window(
    conn: &Connection,
    entity_id: EntityId,
    from: Option<Date>,
    to: Option<Date>,
    today: Date,
) -> Result<CashFlowSeries> {
    let (start, end) = activity_window(conn, entity_id, from, to, today)?;

    cash_flow_series(conn, entity_id, start, end)
}

/// Chooses day buckets for a window of at most [`DAILY_BUCKET_MAX_DAYS`] days,
/// counting both ends, and month buckets for a longer one.
fn granularity_for(from: Date, to: Date) -> CashFlowGranularity {
    let days = to.to_julian_day() - from.to_julian_day() + 1;
    if days <= DAILY_BUCKET_MAX_DAYS {
        CashFlowGranularity::Day
    } else {
        CashFlowGranularity::Month
    }
}

/// Returns the first and last day of each bucket of `from..=to`, oldest first.
///
/// The ranges are contiguous and cover the window exactly. A month bucket
/// ends on the last day of its month or on `to`, whichever is earlier, so the
/// first and last ones may be partial months. The caller passes `from <= to`.
fn bucket_ranges(from: Date, to: Date, granularity: CashFlowGranularity) -> Vec<(Date, Date)> {
    let mut ranges = Vec::new();
    let mut start = from;
    loop {
        let end = match granularity {
            CashFlowGranularity::Day => start,
            CashFlowGranularity::Month => last_day_of_month(start).min(to),
        };
        ranges.push((start, end));
        match end.next_day() {
            Some(next) if next <= to => start = next,
            _ => break,
        }
    }
    ranges
}

/// Returns the last day of the month `date` is in.
///
/// The day asked for is the length of that very month, so the replacement
/// fails for no date and the fallback is never taken.
fn last_day_of_month(date: Date) -> Date {
    date.replace_day(date.month().length(date.year()))
        .unwrap_or(date)
}

/// Income and expenses on one calendar day.
struct DayActivity {
    /// The entry date the figures are for.
    date: Date,
    /// Credits minus debits on income accounts that day, in minor units.
    income_minor: i64,
    /// Debits minus credits on expense accounts that day, in minor units.
    expenses_minor: i64,
}

/// Reads income and expenses per entry date between `from` and `to`
/// inclusive, oldest first, counting active entries only.
///
/// A day with no income or expense activity has no element.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored entry date or account type that
///   does not parse.
/// - [`Error::MoneyOverflow`] when a day's income or expenses do not fit in
///   `i64`.
/// - [`Error::Database`] on database errors.
fn daily_activity(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
) -> Result<Vec<DayActivity>> {
    let sql = format!(
        "
        SELECT je.entry_date, a.account_type,
               COALESCE(SUM(jl.debit_minor), 0) AS debits,
               COALESCE(SUM(jl.credit_minor), 0) AS credits
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        JOIN accounts a ON a.id = jl.account_id
        WHERE a.entity_id = ?1
          AND a.account_type IN ('income', 'expense')
          AND {ACTIVE_ENTRY_PREDICATE}
          AND je.entry_date >= ?2
          AND je.entry_date <= ?3
        GROUP BY je.entry_date, a.account_type
        ORDER BY je.entry_date
        "
    );
    let mut stmt = conn.prepare(&sql).database("read daily activity")?;
    let rows = stmt
        .query_map(
            rusqlite::params![entity_id.to_string(), format_date(from), format_date(to)],
            |row| Ok(map_activity_row("read daily activity", row)),
        )
        .database("read daily activity")?;

    let mut days: Vec<DayActivity> = Vec::new();
    for (date, account_type, amount) in collect_rows("read daily activity", rows)? {
        // Rows arrive ordered by date, at most one per account type, so the
        // rows of a day are adjacent and extend the last element.
        if days.last().map(|day| day.date) != Some(date) {
            days.push(DayActivity {
                date,
                income_minor: 0,
                expenses_minor: 0,
            });
        }
        if let Some(day) = days.last_mut() {
            match account_type {
                AccountType::Income => day.income_minor = add_minor(day.income_minor, amount)?,
                AccountType::Expense => {
                    day.expenses_minor = add_minor(day.expenses_minor, amount)?;
                }
                AccountType::Asset | AccountType::Liability | AccountType::Equity => {}
            }
        }
    }
    Ok(days)
}

/// Maps a row of [`daily_activity`], selected as `entry_date, account_type,
/// debits, credits`, to the date, the account type and the activity signed
/// towards that type's normal side.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] naming the column when the date or the account
///   type does not parse, or a column has the wrong storage class. A stored
///   amount that is not an integer makes its total a real number, which is
///   reported under `debits` or `credits`.
/// - [`Error::MoneyOverflow`] when the activity does not fit in `i64`.
fn map_activity_row(
    operation: &'static str,
    row: &rusqlite::Row<'_>,
) -> Result<(Date, AccountType, i64)> {
    let date = stored_date(
        "journal_entries.entry_date",
        &read_column::<String>(operation, row, 0)?,
    )?;
    let account_type = parse_account_type(&read_column::<String>(operation, row, 1)?)?;
    let amount = normal_balance(
        account_type,
        read_column(operation, row, 2)?,
        read_column(operation, row, 3)?,
    )?;

    Ok((date, account_type, amount))
}

/// Returns the earliest and the latest entry date among the active entries
/// of an entity; both are `None` when it has none.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored entry date that does not parse.
/// - [`Error::Database`] on database errors.
fn active_entry_bounds(
    conn: &Connection,
    entity_id: EntityId,
) -> Result<(Option<Date>, Option<Date>)> {
    let sql = format!(
        "
        SELECT MIN(je.entry_date), MAX(je.entry_date)
        FROM journal_entries je
        WHERE je.entity_id = ?1
          AND {ACTIVE_ENTRY_PREDICATE}
        "
    );
    let (earliest, latest): (Option<String>, Option<String>) = conn
        .query_row(&sql, rusqlite::params![entity_id.to_string()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .database("read entry date bounds")?;

    let stored_bound = |text: Option<String>| {
        text.map(|text| stored_date("journal_entries.entry_date", &text))
            .transpose()
    };
    Ok((stored_bound(earliest)?, stored_bound(latest)?))
}
