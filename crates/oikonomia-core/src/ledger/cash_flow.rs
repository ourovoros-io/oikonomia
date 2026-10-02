//! Cash flow over a window: income and expenses per day or per calendar month,
//! with running totals, on exactly the basis `dashboard_summary` uses.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::domain::{AccountType, EntityId};
use crate::error::{Error, Result, ValidationError};
use crate::ledger::balance::{ACTIVE_ENTRY_PREDICATE, normal_balance, parse_account_type};
use crate::ledger::entities::get_entity;
use crate::util::{format_date, parse_date};

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
    /// Entity id.
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
}

/// Income and expenses per day or per month between `from` and `to` inclusive.
///
/// Posted, active entries only (voided entries and their reversals are
/// excluded); Hidden entries count, as they do on the dashboard. The final
/// running totals always equal `dashboard_summary` for the same window.
///
/// # Errors
///
/// [`Error::Validation`] for a malformed date or `from > to`;
/// [`Error::NotFound`] for an unknown entity; database errors as [`Error::Io`].
pub fn cash_flow_series(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
) -> Result<CashFlowSeries> {
    let from_d = parse_date(from)?;
    let to_d = parse_date(to)?;
    if from_d > to_d {
        return Err(Error::Validation(ValidationError::DateRangeInverted));
    }
    let _ = get_entity(conn, entity_id)?;

    let granularity = granularity_for(from_d, to_d);
    let mut buckets: Vec<CashFlowBucket> = bucket_ranges(from_d, to_d, granularity)
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

    for day in daily_activity(conn, entity_id, from_d, to_d)? {
        // Buckets are sorted and contiguous, so the first one ending on or
        // after the day holds it.
        let index = buckets.partition_point(|bucket| bucket.end < day.date);
        if let Some(bucket) = buckets.get_mut(index) {
            bucket.income_minor = bucket.income_minor.saturating_add(day.income_minor);
            bucket.expenses_minor = bucket.expenses_minor.saturating_add(day.expenses_minor);
        }
    }

    let mut income = 0_i64;
    let mut expenses = 0_i64;
    for bucket in &mut buckets {
        income = income.saturating_add(bucket.income_minor);
        expenses = expenses.saturating_add(bucket.expenses_minor);
        bucket.cumulative_income_minor = income;
        bucket.cumulative_expenses_minor = expenses;
    }

    Ok(CashFlowSeries {
        entity_id,
        from: from_d,
        to: to_d,
        granularity,
        total_income_minor: income,
        total_expenses_minor: expenses,
        net_minor: income.saturating_sub(expenses),
        buckets,
    })
}

/// The window to draw when a date filter leaves one or both bounds empty.
///
/// An empty `from` becomes the book's first active entry date and an empty `to`
/// its last; with no entries, the open side falls back to the other bound, or
/// to `today`. A defaulted bound never lands on the wrong side of a given one.
///
/// # Errors
///
/// [`Error::Validation`] for a malformed date or an explicit `from > to`;
/// [`Error::NotFound`] for an unknown entity; database errors as [`Error::Io`].
pub fn activity_window(
    conn: &Connection,
    entity_id: EntityId,
    from: Option<&str>,
    to: Option<&str>,
    today: Date,
) -> Result<(Date, Date)> {
    let _ = get_entity(conn, entity_id)?;
    let from_d = from.map(parse_date).transpose()?;
    let to_d = to.map(parse_date).transpose()?;
    if let (Some(start), Some(end)) = (from_d, to_d) {
        if start > end {
            return Err(Error::Validation(ValidationError::DateRangeInverted));
        }
        return Ok((start, end));
    }

    let (earliest, latest) = active_entry_bounds(conn, entity_id)?;
    match (from_d, to_d) {
        (Some(start), None) => Ok((start, latest.unwrap_or(today).max(start))),
        (None, Some(end)) => Ok((earliest.unwrap_or(end).min(end), end)),
        _ => Ok((earliest.unwrap_or(today), latest.unwrap_or(today))),
    }
}

fn granularity_for(from: Date, to: Date) -> CashFlowGranularity {
    let days = to.to_julian_day() - from.to_julian_day() + 1;
    if days <= DAILY_BUCKET_MAX_DAYS {
        CashFlowGranularity::Day
    } else {
        CashFlowGranularity::Month
    }
}

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

fn last_day_of_month(date: Date) -> Date {
    date.replace_day(date.month().length(date.year()))
        .unwrap_or(date)
}

/// Income and expenses on one calendar day.
struct DayActivity {
    date: Date,
    income_minor: i64,
    expenses_minor: i64,
}

fn daily_activity(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
) -> Result<Vec<DayActivity>> {
    let sql = format!(
        "
        SELECT je.entry_date, a.account_type,
               COALESCE(SUM(jl.debit_minor), 0),
               COALESCE(SUM(jl.credit_minor), 0)
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        JOIN accounts a ON a.id = jl.account_id
        WHERE a.entity_id = ?1
          AND a.account_type IN ('income', 'expense')
          AND je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
          AND je.entry_date >= ?2
          AND je.entry_date <= ?3
        GROUP BY je.entry_date, a.account_type
        ORDER BY je.entry_date
        "
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| Error::Io(err.to_string()))?;
    let rows = stmt
        .query_map(
            rusqlite::params![entity_id.0.to_string(), format_date(from), format_date(to)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut days: Vec<DayActivity> = Vec::new();
    for row in rows {
        let (date_text, type_text, debits, credits) =
            row.map_err(|err| Error::Io(err.to_string()))?;
        let date = parse_date(&date_text)?;
        let account_type = parse_account_type(&type_text)?;
        let amount = normal_balance(account_type, debits, credits);

        if days.last().map(|day| day.date) != Some(date) {
            days.push(DayActivity {
                date,
                income_minor: 0,
                expenses_minor: 0,
            });
        }
        if let Some(day) = days.last_mut() {
            match account_type {
                AccountType::Income => day.income_minor = day.income_minor.saturating_add(amount),
                AccountType::Expense => {
                    day.expenses_minor = day.expenses_minor.saturating_add(amount);
                }
                AccountType::Asset | AccountType::Liability | AccountType::Equity => {}
            }
        }
    }
    Ok(days)
}

fn active_entry_bounds(
    conn: &Connection,
    entity_id: EntityId,
) -> Result<(Option<Date>, Option<Date>)> {
    let sql = format!(
        "
        SELECT MIN(je.entry_date), MAX(je.entry_date)
        FROM journal_entries je
        WHERE je.entity_id = ?1
          AND je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
        "
    );
    let (earliest, latest): (Option<String>, Option<String>) = conn
        .query_row(&sql, rusqlite::params![entity_id.0.to_string()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|err| Error::Io(err.to_string()))?;

    Ok((
        earliest.as_deref().map(parse_date).transpose()?,
        latest.as_deref().map(parse_date).transpose()?,
    ))
}
