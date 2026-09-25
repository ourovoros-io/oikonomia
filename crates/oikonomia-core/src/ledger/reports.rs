//! Financial reports: trial balance, P&L, balance sheet, dashboard.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::domain::{AccountType, EntityId};
use crate::error::{Error, Result};
use crate::ledger::balance::{
    ACTIVE_ENTRY_PREDICATE, account_type_str, normal_balance, parse_account_type, sum_types_as_of,
    sum_types_in_range,
};
use crate::ledger::entities::get_entity;
use crate::util::{format_date, parse_date};

/// One line on a trial balance or section report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportLine {
    /// Account code.
    pub code: String,
    /// Account name.
    pub name: String,
    /// Account type.
    pub account_type: AccountType,
    /// Debit total (minor) where applicable.
    pub debit_minor: i64,
    /// Credit total (minor) where applicable.
    pub credit_minor: i64,
    /// Signed normal balance.
    pub balance_minor: i64,
}

/// Trial balance as of a date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialBalance {
    /// Entity id.
    pub entity_id: EntityId,
    /// As-of date.
    #[serde(with = "crate::util::serde_date")]
    pub as_of: Date,
    /// Lines with activity or non-zero balance.
    pub lines: Vec<ReportLine>,
    /// Total debits.
    pub total_debits: i64,
    /// Total credits.
    pub total_credits: i64,
}

/// Profit and loss for a period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnL {
    /// Entity id.
    pub entity_id: EntityId,
    /// From date.
    #[serde(with = "crate::util::serde_date")]
    pub from: Date,
    /// To date.
    #[serde(with = "crate::util::serde_date")]
    pub to: Date,
    /// Income lines.
    pub income: Vec<ReportLine>,
    /// Expense lines.
    pub expenses: Vec<ReportLine>,
    /// Total income (normal).
    pub total_income: i64,
    /// Total expenses (normal).
    pub total_expenses: i64,
    /// Net income = income − expenses.
    pub net_income: i64,
}

/// Balance sheet section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSheetSection {
    /// Section title.
    pub title: String,
    /// Lines.
    pub lines: Vec<ReportLine>,
    /// Section total.
    pub total: i64,
}

/// Balance sheet as of date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSheet {
    /// Entity id.
    pub entity_id: EntityId,
    /// As-of date.
    #[serde(with = "crate::util::serde_date")]
    pub as_of: Date,
    /// Assets.
    pub assets: BalanceSheetSection,
    /// Liabilities.
    pub liabilities: BalanceSheetSection,
    /// Equity including current-FY net income and prior-period unclosed P&L.
    pub equity: BalanceSheetSection,
    /// Assets total.
    pub total_assets: i64,
    /// Liabilities + equity total.
    pub total_liabilities_equity: i64,
}

/// Dashboard summary cards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSummary {
    /// Entity id.
    pub entity_id: EntityId,
    /// Currency code.
    pub base_currency: String,
    /// Sum of asset accounts as of `to`.
    pub cash_like_assets: i64,
    /// Income in period.
    pub income: i64,
    /// Expenses in period.
    pub expenses: i64,
    /// Net income in period.
    pub net_income: i64,
    /// Posted, non-voided entries in the period.
    pub recent_entry_count: usize,
    /// Net income as a share of income, in basis points; `None` when income is
    /// zero or less.
    pub savings_rate_bps: Option<i64>,
    /// Expenses as a share of income, in basis points; `None` when income is
    /// zero or less.
    pub spend_ratio_bps: Option<i64>,
    /// The Expense account with the most spending in the window; `None` when
    /// there are no expenses.
    pub top_expense: Option<TopExpense>,
    /// Change in net income against [`previous_window`], in basis points of the
    /// previous net's size; `None` when the previous net is zero.
    pub net_vs_previous_bps: Option<i64>,
}

/// The Expense account with the most spending in a dashboard window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopExpense {
    /// Account code.
    pub code: String,
    /// Account name.
    pub name: String,
    /// Spending on the account in the window.
    pub amount_minor: i64,
    /// `amount_minor` as a share of the window's expenses, in basis points.
    pub share_bps: i64,
}

/// Trial balance as of `as_of` (`YYYY-MM-DD`).
///
/// Permanent accounts are cumulative. Income and expense show only the
/// current fiscal year through `as_of`; earlier unclosed P&L is folded into a
/// synthetic retained-earnings line so the sheet matches the balance sheet
/// after a year boundary (there is no permanent year-end close).
///
/// # Errors
///
/// Validation or DB errors.
pub fn trial_balance(conn: &Connection, entity_id: EntityId, as_of: &str) -> Result<TrialBalance> {
    let as_of_d = parse_date(as_of)?;
    let entity = get_entity(conn, entity_id)?;
    let close = unclosed_pnl(conn, entity_id, as_of_d, entity.fiscal_year_start_month)?;

    let mut lines = Vec::new();
    let mut total_debits = 0_i64;
    let mut total_credits = 0_i64;

    let mut push = |line: ReportLine| {
        if line.debit_minor == 0 && line.credit_minor == 0 {
            return;
        }
        total_debits = total_debits.saturating_add(line.debit_minor);
        total_credits = total_credits.saturating_add(line.credit_minor);
        lines.push(line);
    };

    for account_type in [
        AccountType::Asset,
        AccountType::Liability,
        AccountType::Equity,
    ] {
        for line in as_of_lines(conn, entity_id, account_type, as_of_d)? {
            push(line);
        }
    }

    if close.prior_net != 0 {
        push(retained_earnings_line(close.prior_net));
    }

    for account_type in [AccountType::Income, AccountType::Expense] {
        for line in period_lines(
            conn,
            entity_id,
            account_type,
            close.fy_start,
            as_of_d,
            false,
        )? {
            push(line);
        }
    }

    Ok(TrialBalance {
        entity_id,
        as_of: as_of_d,
        lines,
        total_debits,
        total_credits,
    })
}

/// Profit and loss between `from` and `to` inclusive.
///
/// # Errors
///
/// Validation or DB errors.
pub fn profit_and_loss(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
) -> Result<PnL> {
    profit_and_loss_filtered(conn, entity_id, from, to, false)
}

/// Accountant / PDF export P&L: same window as [`profit_and_loss`], Hidden omitted.
///
/// In-app Reports keep using [`profit_and_loss`] so the owner still sees Hidden.
///
/// # Errors
///
/// Validation or DB errors.
pub fn profit_and_loss_export(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
) -> Result<PnL> {
    profit_and_loss_filtered(conn, entity_id, from, to, true)
}

fn profit_and_loss_filtered(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
    omit_hidden: bool,
) -> Result<PnL> {
    let from_d = parse_date(from)?;
    let to_d = parse_date(to)?;
    if from_d > to_d {
        return Err(Error::Validation(
            "from date must be on or before to".into(),
        ));
    }
    let _ = get_entity(conn, entity_id)?;

    let income = period_lines(
        conn,
        entity_id,
        AccountType::Income,
        from_d,
        to_d,
        omit_hidden,
    )?;
    let expenses = period_lines(
        conn,
        entity_id,
        AccountType::Expense,
        from_d,
        to_d,
        omit_hidden,
    )?;

    let total_income: i64 = income.iter().map(|l| l.balance_minor).sum();
    let total_expenses: i64 = expenses.iter().map(|l| l.balance_minor).sum();
    let net_income = total_income.saturating_sub(total_expenses);

    Ok(PnL {
        entity_id,
        from: from_d,
        to: to_d,
        income,
        expenses,
        total_income,
        total_expenses,
        net_income,
    })
}

/// Balance sheet as of date (equity includes current-FY net income and
/// unclosed prior-period P&L — there is no permanent year-end close).
///
/// # Errors
///
/// Validation or DB errors.
pub fn balance_sheet(conn: &Connection, entity_id: EntityId, as_of: &str) -> Result<BalanceSheet> {
    let as_of_d = parse_date(as_of)?;
    let entity = get_entity(conn, entity_id)?;

    let assets_lines = as_of_lines(conn, entity_id, AccountType::Asset, as_of_d)?;
    let liab_lines = as_of_lines(conn, entity_id, AccountType::Liability, as_of_d)?;
    let mut equity_lines = as_of_lines(conn, entity_id, AccountType::Equity, as_of_d)?;
    let close = unclosed_pnl(conn, entity_id, as_of_d, entity.fiscal_year_start_month)?;

    if close.prior_net != 0 {
        equity_lines.push(retained_earnings_line(close.prior_net));
    }

    if close.current_net != 0 {
        equity_lines.push(net_income_line(close.current_net));
    }

    let total_assets: i64 = assets_lines.iter().map(|l| l.balance_minor).sum();
    let total_liab: i64 = liab_lines.iter().map(|l| l.balance_minor).sum();
    let total_equity: i64 = equity_lines.iter().map(|l| l.balance_minor).sum();
    let total_liabilities_equity = total_liab.saturating_add(total_equity);

    Ok(BalanceSheet {
        entity_id,
        as_of: as_of_d,
        assets: BalanceSheetSection {
            title: "Assets".into(),
            total: total_assets,
            lines: assets_lines,
        },
        liabilities: BalanceSheetSection {
            title: "Liabilities".into(),
            total: total_liab,
            lines: liab_lines,
        },
        equity: BalanceSheetSection {
            title: "Equity".into(),
            total: total_equity,
            lines: equity_lines,
        },
        total_assets,
        total_liabilities_equity,
    })
}

/// Dashboard summary: income/expenses/count over `[from, to]` (typically the
/// full calendar month, so future-dated bills inside the month are counted),
/// with assets reported as of `assets_as_of` (typically today). It also
/// carries the arc metrics (savings rate, spend ratio, top expense, and net
/// against [`previous_window`]) in basis points, so the UI never divides.
///
/// # Errors
///
/// Validation or DB errors.
pub fn dashboard_summary(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
    assets_as_of: &str,
) -> Result<DashboardSummary> {
    let entity = get_entity(conn, entity_id)?;
    let from_d = parse_date(from)?;
    let to_d = parse_date(to)?;
    let assets_as_of_d = parse_date(assets_as_of)?;
    if from_d > to_d {
        return Err(Error::Validation(
            "from date must be on or before to".into(),
        ));
    }

    let cash_like_assets = sum_types_as_of(conn, entity_id, &[AccountType::Asset], assets_as_of_d)?;
    let income = sum_types_in_range(conn, entity_id, &[AccountType::Income], from_d, to_d)?;
    let expenses = sum_types_in_range(conn, entity_id, &[AccountType::Expense], from_d, to_d)?;
    let count_sql = format!(
        "
        SELECT COUNT(1) FROM journal_entries je
        WHERE je.entity_id = ?1 AND je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
          AND je.entry_date >= ?2 AND je.entry_date <= ?3
        "
    );
    let count: i64 = conn
        .query_row(
            &count_sql,
            rusqlite::params![
                entity_id.0.to_string(),
                format_date(from_d),
                format_date(to_d)
            ],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let net_income = income.saturating_sub(expenses);
    let net_vs_previous_bps = match previous_window(from_d, to_d) {
        Some((previous_from, previous_to)) => {
            let previous_net = sum_types_in_range(
                conn,
                entity_id,
                &[AccountType::Income],
                previous_from,
                previous_to,
            )?
            .saturating_sub(sum_types_in_range(
                conn,
                entity_id,
                &[AccountType::Expense],
                previous_from,
                previous_to,
            )?);
            ratio_bps(
                net_income.saturating_sub(previous_net),
                previous_net.saturating_abs(),
            )
        }
        None => None,
    };
    let (savings_rate_bps, spend_ratio_bps) = if income > 0 {
        (ratio_bps(net_income, income), ratio_bps(expenses, income))
    } else {
        (None, None)
    };

    Ok(DashboardSummary {
        entity_id,
        base_currency: entity.base_currency,
        cash_like_assets,
        income,
        expenses,
        net_income,
        recent_entry_count: usize::try_from(count).unwrap_or(0),
        savings_rate_bps,
        spend_ratio_bps,
        top_expense: top_expense(conn, entity_id, from_d, to_d, expenses)?,
        net_vs_previous_bps,
    })
}

/// The window a dashboard compares `[from, to]` against: the one just before it.
///
/// A window of whole calendar months (from the first of a month to the last
/// day of a month) steps back by the same number of calendar months, so a
/// month compares with the month before, a quarter with the quarter before and
/// a year with the year before. Any other window steps back by the same number
/// of days. `None` only at the edge of the calendar.
#[must_use]
pub fn previous_window(from: Date, to: Date) -> Option<(Date, Date)> {
    let previous_to = from.previous_day()?;
    let whole_months = from.day() == 1 && to.next_day().is_none_or(|next| next.day() == 1);
    if whole_months {
        let months = month_index(to) - month_index(from) + 1;
        return Some((
            date_from_month_index(month_index(from) - months)?,
            previous_to,
        ));
    }
    let length = to.to_julian_day() - from.to_julian_day();
    let previous_from = Date::from_julian_day(previous_to.to_julian_day() - length).ok()?;
    Some((previous_from, previous_to))
}

fn month_index(date: Date) -> i64 {
    i64::from(date.year()) * 12 + i64::from(u8::from(date.month())) - 1
}

fn date_from_month_index(index: i64) -> Option<Date> {
    let year = i32::try_from(index.div_euclid(12)).ok()?;
    let month = u8::try_from(index.rem_euclid(12) + 1).ok()?;
    Date::from_calendar_date(year, time::Month::try_from(month).ok()?, 1).ok()
}

/// The Expense account with the largest positive spend in the window; on a tie
/// the first in chart order wins.
fn top_expense(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
    expenses: i64,
) -> Result<Option<TopExpense>> {
    if expenses <= 0 {
        return Ok(None);
    }
    let lines = period_lines(conn, entity_id, AccountType::Expense, from, to, false)?;
    let mut top: Option<&ReportLine> = None;
    for line in &lines {
        if line.balance_minor > 0 && top.is_none_or(|lead| line.balance_minor > lead.balance_minor)
        {
            top = Some(line);
        }
    }
    Ok(top.and_then(|line| {
        Some(TopExpense {
            code: line.code.clone(),
            name: line.name.clone(),
            amount_minor: line.balance_minor,
            share_bps: ratio_bps(line.balance_minor, expenses)?,
        })
    }))
}

/// `numerator / denominator` in basis points, rounded half away from zero;
/// `None` when the denominator is zero or the result does not fit in `i64`.
fn ratio_bps(numerator: i64, denominator: i64) -> Option<i64> {
    if denominator == 0 {
        return None;
    }
    let scaled = i128::from(numerator) * 10_000;
    let divisor = i128::from(denominator);
    let magnitude = (scaled.abs() + divisor.abs() / 2) / divisor.abs();
    let signed = if (scaled < 0) == (divisor < 0) {
        magnitude
    } else {
        -magnitude
    };
    i64::try_from(signed).ok()
}

/// Per-account debit/credit sums over posted, non-voided entries in a window.
///
/// Entry-level predicates (status, void, optional Hidden) live in the inner
/// subquery WHERE — never on the outer LEFT JOIN ON — so a filtered-out entry
/// contributes nothing and accounts with no matching activity stay at zero.
fn account_activity_lines(
    conn: &Connection,
    entity_id: EntityId,
    account_type: Option<AccountType>,
    from: Option<Date>,
    to: Date,
    omit_hidden: bool,
) -> Result<Vec<ReportLine>> {
    let hidden_predicate = if omit_hidden {
        "AND (je.hidden = 0 OR je.hidden IS NULL)"
    } else {
        ""
    };
    let sql = format!(
        "
        SELECT a.code, a.name, a.account_type,
               COALESCE(t.debits, 0),
               COALESCE(t.credits, 0)
        FROM accounts a
        LEFT JOIN (
            SELECT jl.account_id,
                   SUM(jl.debit_minor) AS debits,
                   SUM(jl.credit_minor) AS credits
            FROM journal_lines jl
            JOIN journal_entries je ON je.id = jl.entry_id
            WHERE je.status = 'posted'
              AND {ACTIVE_ENTRY_PREDICATE}
              {hidden_predicate}
              AND (?2 IS NULL OR je.entry_date >= ?2)
              AND je.entry_date <= ?3
            GROUP BY jl.account_id
        ) t ON t.account_id = a.id
        WHERE a.entity_id = ?1
          AND (?4 IS NULL OR a.account_type = ?4)
        ORDER BY a.sort_order, a.code
        "
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map(
            rusqlite::params![
                entity_id.0.to_string(),
                from.map(format_date),
                format_date(to),
                account_type.map(account_type_str),
            ],
            map_report_line,
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut lines = Vec::new();
    for row in rows {
        lines.push(row.map_err(|err| Error::Io(err.to_string()))?);
    }
    Ok(lines)
}

fn period_lines(
    conn: &Connection,
    entity_id: EntityId,
    account_type: AccountType,
    from: Date,
    to: Date,
    omit_hidden: bool,
) -> Result<Vec<ReportLine>> {
    let lines = account_activity_lines(
        conn,
        entity_id,
        Some(account_type),
        Some(from),
        to,
        omit_hidden,
    )?;
    Ok(lines
        .into_iter()
        .filter(|l| l.balance_minor != 0 || l.debit_minor != 0 || l.credit_minor != 0)
        .collect())
}

fn as_of_lines(
    conn: &Connection,
    entity_id: EntityId,
    account_type: AccountType,
    as_of: Date,
) -> Result<Vec<ReportLine>> {
    let lines = account_activity_lines(conn, entity_id, Some(account_type), None, as_of, false)?;
    Ok(lines
        .into_iter()
        .filter(|l| l.balance_minor != 0 || l.debit_minor != 0 || l.credit_minor != 0)
        .collect())
}

/// Current-FY net income and unclosed P&L from before `fy_start`.
struct UnclosedPnl {
    fy_start: Date,
    current_net: i64,
    prior_net: i64,
}

fn unclosed_pnl(
    conn: &Connection,
    entity_id: EntityId,
    as_of: Date,
    fy_start_month: u8,
) -> Result<UnclosedPnl> {
    let fy_start = fiscal_year_start(as_of, fy_start_month);
    let current_net = sum_types_in_range(conn, entity_id, &[AccountType::Income], fy_start, as_of)?
        .saturating_sub(sum_types_in_range(
            conn,
            entity_id,
            &[AccountType::Expense],
            fy_start,
            as_of,
        )?);

    let prior_net = if let Some(prior_end) = fy_start.previous_day() {
        let books_start = Date::from_calendar_date(1, time::Month::January, 1).unwrap_or(prior_end);
        sum_types_in_range(
            conn,
            entity_id,
            &[AccountType::Income],
            books_start,
            prior_end,
        )?
        .saturating_sub(sum_types_in_range(
            conn,
            entity_id,
            &[AccountType::Expense],
            books_start,
            prior_end,
        )?)
    } else {
        0
    };

    Ok(UnclosedPnl {
        fy_start,
        current_net,
        prior_net,
    })
}

fn equity_plug_line(code: &str, name: &str, net: i64) -> ReportLine {
    let (debit_minor, credit_minor) = if net >= 0 {
        (0, net)
    } else {
        (net.saturating_neg(), 0)
    };
    ReportLine {
        code: code.into(),
        name: name.into(),
        account_type: AccountType::Equity,
        debit_minor,
        credit_minor,
        balance_minor: net,
    }
}

fn retained_earnings_line(prior_net: i64) -> ReportLine {
    equity_plug_line("RE", "Retained Earnings (prior periods)", prior_net)
}

fn net_income_line(net: i64) -> ReportLine {
    equity_plug_line("NI", "Net Income (current period)", net)
}

fn fiscal_year_start(as_of: Date, start_month: u8) -> Date {
    let month = match time::Month::try_from(start_month) {
        Ok(m) => m,
        Err(_) => time::Month::January,
    };
    let year = if as_of.month() as u8 >= start_month {
        as_of.year()
    } else {
        as_of.year() - 1
    };
    match Date::from_calendar_date(year, month, 1) {
        Ok(d) => d,
        Err(_) => as_of,
    }
}

fn map_report_line(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReportLine> {
    let type_s: String = row.get(2)?;
    let account_type = parse_account_type(&type_s).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        )))
    })?;
    let debits: i64 = row.get(3)?;
    let credits: i64 = row.get(4)?;
    Ok(ReportLine {
        code: row.get(0)?,
        name: row.get(1)?,
        account_type,
        debit_minor: debits,
        credit_minor: credits,
        balance_minor: normal_balance(account_type, debits, credits),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_bps_rounds_half_away_from_zero() {
        assert_eq!(ratio_bps(1, 3), Some(3_333));
        assert_eq!(ratio_bps(2, 3), Some(6_667));
        assert_eq!(ratio_bps(1, 2), Some(5_000));
        assert_eq!(
            ratio_bps(1, 20_000),
            Some(1),
            "half a basis point rounds up"
        );
        assert_eq!(
            ratio_bps(-1, 20_000),
            Some(-1),
            "and away from zero when negative"
        );
        assert_eq!(ratio_bps(-1, 3), Some(-3_333));
        assert_eq!(ratio_bps(1, -3), Some(-3_333));
        assert_eq!(ratio_bps(-2, -3), Some(6_667));
        assert_eq!(ratio_bps(0, 7), Some(0));
    }

    #[test]
    fn ratio_bps_refuses_what_it_cannot_express() {
        assert_eq!(ratio_bps(5, 0), None, "no denominator");
        assert_eq!(ratio_bps(i64::MAX, 1), None, "does not fit in i64");
    }
}
