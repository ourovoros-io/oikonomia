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
    pub from: Date,
    /// To date.
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
    pub as_of: Date,
    /// Assets.
    pub assets: BalanceSheetSection,
    /// Liabilities.
    pub liabilities: BalanceSheetSection,
    /// Equity including current-period net income.
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
}

/// Trial balance as of `as_of` (`YYYY-MM-DD`).
///
/// # Errors
///
/// Validation or DB errors.
pub fn trial_balance(conn: &Connection, entity_id: EntityId, as_of: &str) -> Result<TrialBalance> {
    let as_of_d = parse_date(as_of)?;
    let _ = get_entity(conn, entity_id)?;

    let all_lines = account_activity_lines(conn, entity_id, None, None, as_of_d)?;

    let mut lines = Vec::new();
    let mut total_debits = 0_i64;
    let mut total_credits = 0_i64;

    for line in all_lines {
        if line.debit_minor == 0 && line.credit_minor == 0 {
            continue;
        }
        total_debits = total_debits.saturating_add(line.debit_minor);
        total_credits = total_credits.saturating_add(line.credit_minor);
        lines.push(line);
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
    let from_d = parse_date(from)?;
    let to_d = parse_date(to)?;
    if from_d > to_d {
        return Err(Error::Validation(
            "from date must be on or before to".into(),
        ));
    }
    let _ = get_entity(conn, entity_id)?;

    let income = period_lines(conn, entity_id, AccountType::Income, from_d, to_d)?;
    let expenses = period_lines(conn, entity_id, AccountType::Expense, from_d, to_d)?;

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

/// Balance sheet as of date (equity includes YTD net income from fiscal year start).
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

    let fy_start = fiscal_year_start(as_of_d, entity.fiscal_year_start_month);
    let net = sum_types_in_range(conn, entity_id, &[AccountType::Income], fy_start, as_of_d)?
        .saturating_sub(sum_types_in_range(
            conn,
            entity_id,
            &[AccountType::Expense],
            fy_start,
            as_of_d,
        )?);

    if net != 0 {
        equity_lines.push(ReportLine {
            code: "NI".into(),
            name: "Net Income (current period)".into(),
            account_type: AccountType::Equity,
            debit_minor: 0,
            credit_minor: 0,
            balance_minor: net,
        });
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

/// Dashboard summary for period.
///
/// # Errors
///
/// Validation or DB errors.
pub fn dashboard_summary(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
) -> Result<DashboardSummary> {
    let entity = get_entity(conn, entity_id)?;
    let from_d = parse_date(from)?;
    let to_d = parse_date(to)?;
    if from_d > to_d {
        return Err(Error::Validation(
            "from date must be on or before to".into(),
        ));
    }

    let cash_like_assets = sum_types_as_of(conn, entity_id, &[AccountType::Asset], to_d)?;
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

    Ok(DashboardSummary {
        entity_id,
        base_currency: entity.base_currency,
        cash_like_assets,
        income,
        expenses,
        net_income: income.saturating_sub(expenses),
        recent_entry_count: usize::try_from(count).unwrap_or(0),
    })
}

/// Per-account debit/credit sums over posted, non-voided entries in a window.
///
/// The entry-level predicates live in an inner join inside the subquery, so a
/// line whose entry falls outside the window contributes nothing — accounts
/// with no matching activity still appear with zero sums via the outer join.
fn account_activity_lines(
    conn: &Connection,
    entity_id: EntityId,
    account_type: Option<AccountType>,
    from: Option<Date>,
    to: Date,
) -> Result<Vec<ReportLine>> {
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
) -> Result<Vec<ReportLine>> {
    let lines = account_activity_lines(conn, entity_id, Some(account_type), Some(from), to)?;
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
    let lines = account_activity_lines(conn, entity_id, Some(account_type), None, as_of)?;
    Ok(lines
        .into_iter()
        .filter(|l| l.balance_minor != 0 || l.debit_minor != 0 || l.credit_minor != 0)
        .collect())
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
