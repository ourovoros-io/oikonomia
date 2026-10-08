//! Financial reports: trial balance, profit and loss, balance sheet and the
//! dashboard summary.
//!
//! A report is computed from the journal each time it is asked for; nothing
//! here is stored. All four count active entries only (posted, not voided,
//! not the reversal of a void). The hidden flag changes one of them:
//! [`profit_and_loss_export`] leaves hidden entries out, and every other
//! report counts them.
//!
//! # No year-end close
//!
//! The books never post a closing entry that moves a year's result into
//! equity. The reports as of a date do the equivalent while they run. They
//! split income and expenses at the first day of the fiscal year that holds
//! the date:
//!
//! - the result since that day is the current year's. The balance sheet shows
//!   it as the equity row `NI`; the trial balance lists the income and expense
//!   accounts themselves for that span;
//! - the result of everything before it is shown as the equity row `RE`,
//!   retained earnings, on both.
//!
//! Neither row has an account behind it; [`SyntheticLine`] marks them. With
//! them the balance sheet balances and the trial balance agrees with it on
//! any date.
//!
//! # Query shape
//!
//! The account lines of every report come from one query, in
//! `active_lines`. It joins the accounts to a subquery that sums
//! the journal per account, and every predicate on entries (active, hidden,
//! the date window) is inside that subquery. Written on a `LEFT JOIN ... ON`
//! instead, such a predicate does not filter: a line whose entry fails it
//! stays in the join with the entry's columns null, and its amounts are
//! summed all the same.
//!
//! # Damaged accounts
//!
//! Every query here selects accounts by their stored type, so an account
//! whose `account_type` is none of the five known strings matches none of
//! them and is missing from every report, with no error. The reports do not
//! look for such a row. They rely on the account reads to report it:
//! [`list_accounts`](crate::ledger::list_accounts) and
//! [`get_account`](crate::ledger::get_account) fail with
//! [`Error::VaultCorrupt`] for it, and posting loads each account through
//! `get_account`, so nothing new can be posted to it either.

use crate::db::{collect_rows, read_column};
use crate::domain::{AccountType, CurrencyCode, EntityId};
use crate::error::{DatabaseContext, Error, Result, ValidationError};
use crate::ledger::balance::{
    ACTIVE_ENTRY_PREDICATE, add_minor, count_hidden_pnl_entries, normal_balance,
    parse_account_type, subtract_minor, sum_minor, sum_type_as_of, sum_type_in_range,
};
use crate::ledger::calendar::{add_months, months_between};
use crate::ledger::entities::get_entity;
use crate::util::format_date;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::{Date, Month};

/// A report row the reports compute instead of reading from an account.
///
/// Unclosed profit and loss is shown as equity so the balance sheet balances.
/// The UI words these rows in the user's language; `code` and the English
/// `name` stay on the line for exports and as a fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntheticLine {
    /// Profit and loss of earlier fiscal years not yet closed (code `RE`).
    RetainedEarnings,
    /// Profit and loss of the current fiscal year so far (code `NI`).
    NetIncome,
}

impl SyntheticLine {
    /// Every kind. A test checks this list against `web/src/lib/uiTextCodes.json`.
    pub const ALL: &'static [Self] = &[Self::RetainedEarnings, Self::NetIncome];
}

/// One account, or one computed row, on a report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportLine {
    /// Code of the account; `RE` or `NI` on a computed row.
    pub code: String,
    /// Name of the account as stored; a fixed English name on a computed row.
    pub name: String,
    /// Type of the account. A computed row is equity.
    pub account_type: AccountType,
    /// Debits posted to the account within the report's span, in minor units.
    /// Never negative. On a computed row, the size of a net loss.
    pub debit_minor: i64,
    /// Credits posted to the account within the report's span, in minor units.
    /// Never negative. On a computed row, the size of a net profit.
    pub credit_minor: i64,
    /// The two totals netted towards the normal side of `account_type`, in
    /// minor units: negative when the account sits on its other side.
    pub balance_minor: i64,
    /// Set on a computed row, which has no account behind it; `None` on a real account.
    #[serde(default)]
    pub synthetic: Option<SyntheticLine>,
}

/// Result of [`trial_balance`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialBalance {
    /// Entity the report is for.
    pub entity_id: EntityId,
    /// Last day counted (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub as_of: Date,
    /// Every line with a debit or a credit: assets, liabilities and equity in
    /// chart order, then the retained-earnings row if there is one, then
    /// income and expenses.
    pub lines: Vec<ReportLine>,
    /// Sum of `debit_minor` over `lines`, in minor units.
    pub total_debits: i64,
    /// Sum of `credit_minor` over `lines`, in minor units. Equal to
    /// `total_debits` whenever every entry balances.
    pub total_credits: i64,
}

/// Result of [`profit_and_loss`] and [`profit_and_loss_export`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnL {
    /// Entity the report is for.
    pub entity_id: EntityId,
    /// First day counted (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub from: Date,
    /// Last day counted (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub to: Date,
    /// Income accounts with a debit or a credit in the window, in chart order.
    pub income: Vec<ReportLine>,
    /// Expense accounts with a debit or a credit in the window, in chart order.
    pub expenses: Vec<ReportLine>,
    /// Sum of the income lines' balances (credits minus debits), in minor units.
    pub total_income: i64,
    /// Sum of the expense lines' balances (debits minus credits), in minor units.
    pub total_expenses: i64,
    /// `total_income - total_expenses`; negative for a loss.
    pub net_income: i64,
    /// Hidden entries the figures above include, for a screen to say so; the
    /// export leaves them out and reports 0.
    pub hidden_entry_count: usize,
}

/// One of the three sections of a [`BalanceSheet`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSheetSection {
    /// Accounts of the section with a debit or a credit through the as-of
    /// date, in chart order. The equity section ends with the computed rows.
    pub lines: Vec<ReportLine>,
    /// Sum of the lines' balances, in minor units.
    pub total: i64,
}

/// Result of [`balance_sheet`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSheet {
    /// Entity the report is for.
    pub entity_id: EntityId,
    /// Last day counted (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub as_of: Date,
    /// Asset accounts.
    pub assets: BalanceSheetSection,
    /// Liability accounts.
    pub liabilities: BalanceSheetSection,
    /// Equity accounts, then the result of earlier fiscal years (`RE`) and of
    /// the current one (`NI`), each only when it is not zero.
    pub equity: BalanceSheetSection,
    /// The total of `assets`, in minor units.
    pub total_assets: i64,
    /// The totals of `liabilities` and `equity` added, in minor units. Equal
    /// to `total_assets` whenever every entry balances.
    pub total_liabilities_equity: i64,
    /// Hidden entries, dated through `as_of`, that move income or expenses and
    /// so the equity rows `RE` and `NI`. The balance sheet counts them although
    /// the P&L export leaves them out.
    pub hidden_entry_count: usize,
}

/// Result of [`dashboard_summary`]. Amounts are in minor units.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSummary {
    /// Entity the summary is for.
    pub entity_id: EntityId,
    /// ISO 4217 code of the entity's currency, which every amount here is in.
    pub base_currency: CurrencyCode,
    /// Sum of asset accounts as of the `assets_as_of` date given to
    /// [`dashboard_summary`], which need not be the window's `to`.
    pub cash_like_assets: i64,
    /// Income in the window: credits minus debits on income accounts.
    pub income: i64,
    /// Expenses in the window: debits minus credits on expense accounts.
    pub expenses: i64,
    /// `income - expenses`; negative for a loss.
    pub net_income: i64,
    /// Number of active entries dated in the window.
    pub recent_entry_count: usize,
    /// Net income as a share of income, in basis points; `None` when income is
    /// zero or less, or the share does not fit in `i64`.
    pub savings_rate_bps: Option<i64>,
    /// Expenses as a share of income, in basis points; `None` when income is
    /// zero or less, or the share does not fit in `i64`.
    pub spend_ratio_bps: Option<i64>,
    /// The Expense account with the most spending in the window; `None` when
    /// `expenses` is zero or less, or no account has a positive spend.
    pub top_expense: Option<TopExpense>,
    /// Change in net income against [`previous_window`], in basis points of the
    /// previous net's size; `None` when the previous net is zero, when there
    /// is no previous window, or when the change does not fit in `i64`.
    pub net_vs_previous_bps: Option<i64>,
    /// Hidden entries in the window that the income and expense figures
    /// count, which an export leaves out.
    pub hidden_entry_count: usize,
}

/// The Expense account with the most spending in a dashboard window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopExpense {
    /// Code of the expense account.
    pub code: String,
    /// Name of the expense account as stored.
    pub name: String,
    /// Spending on the account in the window, in minor units; always positive.
    pub amount_minor: i64,
    /// `amount_minor` as a share of the window's expenses, in basis points.
    pub share_bps: i64,
}

/// Computes the trial balance as of `as_of`.
///
/// Permanent accounts are cumulative. Income and expense show only the
/// current fiscal year through `as_of`; earlier unclosed P&L is folded into a
/// synthetic retained-earnings line so the sheet matches the balance sheet
/// after a year boundary (there is no permanent year-end close).
///
/// # Errors
///
/// [`Error::NotFound`] for an unknown entity; [`Error::MoneyOverflow`] when a
/// total does not fit in `i64`; [`Error::VaultCorrupt`] for a stored value
/// that does not parse; database errors as [`Error::Database`].
pub fn trial_balance(conn: &Connection, entity_id: EntityId, as_of: Date) -> Result<TrialBalance> {
    // An archived entity is found too; its reports stay readable.
    let entity = get_entity(conn, entity_id)?;
    let unclosed = unclosed_pnl(conn, entity_id, as_of, entity.fiscal_year_start_month)?;

    let mut lines = Vec::new();

    for account_type in [
        AccountType::Asset,
        AccountType::Liability,
        AccountType::Equity,
    ] {
        lines.extend(active_lines(
            conn,
            LineQuery::as_of(entity_id, account_type, as_of),
        )?);
    }

    if unclosed.prior_net != 0 {
        lines.push(retained_earnings_line(unclosed.prior_net)?);
    }

    for account_type in [AccountType::Income, AccountType::Expense] {
        lines.extend(active_lines(
            conn,
            LineQuery::in_period(entity_id, account_type, unclosed.year_start, as_of),
        )?);
    }

    // Every line has a debit or a credit: `active_lines` returns no other,
    // and the retained-earnings row is added only when it is not zero.
    let total_debits = sum_minor(lines.iter().map(|line| line.debit_minor))?;
    let total_credits = sum_minor(lines.iter().map(|line| line.credit_minor))?;

    Ok(TrialBalance {
        entity_id,
        as_of,
        lines,
        total_debits,
        total_credits,
    })
}

/// Computes profit and loss between `from` and `to` inclusive, hidden
/// entries included.
///
/// # Errors
///
/// [`ValidationError::DateRangeInverted`] for an inverted range;
/// [`Error::NotFound`] for an unknown entity; [`Error::MoneyOverflow`] when a
/// total does not fit in `i64`; [`Error::VaultCorrupt`] for a stored value
/// that does not parse; database errors as [`Error::Database`].
pub fn profit_and_loss(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
) -> Result<PnL> {
    profit_and_loss_filtered(conn, entity_id, from, to, false)
}

/// Computes the profit and loss that leaves the app: the window of
/// [`profit_and_loss`], with hidden entries left out.
///
/// The exported report is built from this. The in-app report uses
/// [`profit_and_loss`], so the owner still sees hidden entries there.
///
/// # Errors
///
/// [`ValidationError::DateRangeInverted`] for an inverted range;
/// [`Error::NotFound`] for an unknown entity; [`Error::MoneyOverflow`] when a
/// total does not fit in `i64`; [`Error::VaultCorrupt`] for a stored value
/// that does not parse; database errors as [`Error::Database`].
pub fn profit_and_loss_export(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
) -> Result<PnL> {
    profit_and_loss_filtered(conn, entity_id, from, to, true)
}

/// Computes the balance sheet as of `as_of`.
///
/// Equity includes the current fiscal year's result and the unclosed result
/// of earlier years as computed rows, because there is no permanent year-end
/// close.
///
/// # Errors
///
/// [`Error::NotFound`] for an unknown entity; [`Error::MoneyOverflow`] when a
/// total does not fit in `i64`; [`Error::VaultCorrupt`] for a stored value
/// that does not parse; database errors as [`Error::Database`].
pub fn balance_sheet(conn: &Connection, entity_id: EntityId, as_of: Date) -> Result<BalanceSheet> {
    // An archived entity is found too; its reports stay readable.
    let entity = get_entity(conn, entity_id)?;

    let asset_lines = active_lines(conn, LineQuery::as_of(entity_id, AccountType::Asset, as_of))?;
    let liability_lines = active_lines(
        conn,
        LineQuery::as_of(entity_id, AccountType::Liability, as_of),
    )?;
    let mut equity_lines = active_lines(
        conn,
        LineQuery::as_of(entity_id, AccountType::Equity, as_of),
    )?;
    let unclosed = unclosed_pnl(conn, entity_id, as_of, entity.fiscal_year_start_month)?;

    if unclosed.prior_net != 0 {
        equity_lines.push(retained_earnings_line(unclosed.prior_net)?);
    }

    if unclosed.current_net != 0 {
        equity_lines.push(net_income_line(unclosed.current_net)?);
    }

    let total_assets = sum_minor(asset_lines.iter().map(|line| line.balance_minor))?;
    let total_liabilities = sum_minor(liability_lines.iter().map(|line| line.balance_minor))?;
    let total_equity = sum_minor(equity_lines.iter().map(|line| line.balance_minor))?;
    let total_liabilities_equity = add_minor(total_liabilities, total_equity)?;

    Ok(BalanceSheet {
        entity_id,
        as_of,
        assets: BalanceSheetSection {
            total: total_assets,
            lines: asset_lines,
        },
        liabilities: BalanceSheetSection {
            total: total_liabilities,
            lines: liability_lines,
        },
        equity: BalanceSheetSection {
            total: total_equity,
            lines: equity_lines,
        },
        total_assets,
        total_liabilities_equity,
        hidden_entry_count: count_hidden_pnl_entries(conn, entity_id, None, Some(as_of))?,
    })
}

/// Computes the dashboard figures for the window `[from, to]`, with assets
/// as of `assets_as_of`.
///
/// The window is typically the full calendar month, so that a bill dated
/// later in the month is counted, while `assets_as_of` is typically today.
/// The summary also carries the arc metrics (savings rate, spend ratio, top
/// expense, and net against [`previous_window`]) in basis points, so the UI
/// never divides.
///
/// # Errors
///
/// [`ValidationError::DateRangeInverted`] for an inverted range;
/// [`Error::NotFound`] for an unknown entity; [`Error::MoneyOverflow`] when a
/// total does not fit in `i64`; [`Error::VaultCorrupt`] for a stored value
/// that does not parse; database errors as [`Error::Database`].
pub fn dashboard_summary(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
    assets_as_of: Date,
) -> Result<DashboardSummary> {
    // An archived entity is found too; its reports stay readable.
    let entity = get_entity(conn, entity_id)?;
    if from > to {
        return Err(ValidationError::DateRangeInverted.into());
    }

    let cash_like_assets = sum_type_as_of(conn, entity_id, AccountType::Asset, assets_as_of)?;
    let income = sum_type_in_range(conn, entity_id, AccountType::Income, from, to)?;
    let expenses = sum_type_in_range(conn, entity_id, AccountType::Expense, from, to)?;
    let count_sql = format!(
        "
        SELECT COUNT(1) FROM journal_entries je
        WHERE je.entity_id = ?1 AND {ACTIVE_ENTRY_PREDICATE}
          AND je.entry_date >= ?2 AND je.entry_date <= ?3
        "
    );
    let count: i64 = conn
        .query_row(
            &count_sql,
            rusqlite::params![entity_id.to_string(), format_date(from), format_date(to)],
            |row| row.get(0),
        )
        .database("count entries in period")?;

    let net_income = subtract_minor(income, expenses)?;
    let net_vs_previous_bps = match previous_window(from, to) {
        Some((previous_from, previous_to)) => {
            let previous_net = net_in_range(conn, entity_id, previous_from, previous_to)?;
            ratio_bps(
                subtract_minor(net_income, previous_net)?,
                previous_net.checked_abs().ok_or(Error::MoneyOverflow)?,
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
        top_expense: top_expense(conn, entity_id, from, to, expenses)?,
        net_vs_previous_bps,
        hidden_entry_count: count_hidden_pnl_entries(conn, entity_id, Some(from), Some(to))?,
    })
}

/// Returns the window a dashboard compares `[from, to]` against: the one just
/// before it.
///
/// A window of whole calendar months (from the first of a month to the last
/// day of a month) steps back by the same number of calendar months, so a
/// month compares with the month before, a quarter with the quarter before and
/// a year with the year before. Any other window steps back by the same number
/// of days. `None` only at the edge of the calendar.
///
/// The caller passes `from <= to`; the result for an inverted pair is not a
/// window before `from`.
#[must_use]
pub fn previous_window(from: Date, to: Date) -> Option<(Date, Date)> {
    let previous_to = from.previous_day()?;
    let whole_months = from.day() == 1 && to.next_day().is_none_or(|next| next.day() == 1);
    if whole_months {
        let months = months_between(from, to) + 1;
        let (year, month) = add_months(from.year(), from.month(), -months)?;
        let previous_from = Date::from_calendar_date(year, month, 1).ok()?;

        return Some((previous_from, previous_to));
    }
    let length = to.to_julian_day() - from.to_julian_day();
    let previous_from = Date::from_julian_day(previous_to.to_julian_day() - length).ok()?;
    Some((previous_from, previous_to))
}

/// Computes [`profit_and_loss`] or, with `omit_hidden`,
/// [`profit_and_loss_export`].
///
/// # Errors
///
/// Those of [`profit_and_loss`].
fn profit_and_loss_filtered(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
    omit_hidden: bool,
) -> Result<PnL> {
    if from > to {
        return Err(ValidationError::DateRangeInverted.into());
    }
    // Only checks that the entity exists; an archived one passes.
    get_entity(conn, entity_id)?;

    let period = |account_type| {
        LineQuery::in_period(entity_id, account_type, from, to).omitting_hidden(omit_hidden)
    };
    let income = active_lines(conn, period(AccountType::Income))?;
    let expenses = active_lines(conn, period(AccountType::Expense))?;

    let total_income = sum_minor(income.iter().map(|line| line.balance_minor))?;
    let total_expenses = sum_minor(expenses.iter().map(|line| line.balance_minor))?;
    let net_income = subtract_minor(total_income, total_expenses)?;
    let hidden_entry_count = if omit_hidden {
        0
    } else {
        count_hidden_pnl_entries(conn, entity_id, Some(from), Some(to))?
    };

    Ok(PnL {
        entity_id,
        from,
        to,
        income,
        expenses,
        total_income,
        total_expenses,
        net_income,
        hidden_entry_count,
    })
}

/// Returns the Expense account with the largest positive spend in the
/// window; on a tie the first in chart order wins.
///
/// `expenses` is the window's total, which the share is taken of. `None`
/// when it is zero or less, or when no account has a positive spend.
///
/// # Errors
///
/// Those of [`active_lines`].
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
    let lines = active_lines(
        conn,
        LineQuery::in_period(entity_id, AccountType::Expense, from, to),
    )?;
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

/// Returns `numerator / denominator` in basis points, rounded half away from
/// zero.
///
/// `None` when the denominator is zero or the result does not fit in `i64`.
fn ratio_bps(numerator: i64, denominator: i64) -> Option<i64> {
    if denominator == 0 {
        return None;
    }
    // In `i128` neither the scaling nor the rounding term can overflow:
    // both operands come from `i64`.
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

/// Which lines a report reads: the accounts of one type in one entity, with
/// their activity in a window of dates.
#[derive(Debug, Clone, Copy)]
struct LineQuery {
    /// Entity whose accounts are read.
    entity_id: EntityId,
    /// The one account type read.
    account_type: AccountType,
    /// First day counted; `None` counts from the first entry of the books.
    from: Option<Date>,
    /// Last day counted.
    to: Date,
    /// Whether hidden entries are left out.
    omit_hidden: bool,
}

impl LineQuery {
    /// Returns the query for the accounts of `account_type` with everything
    /// posted through `as_of`, hidden entries included.
    ///
    /// Asset, liability and equity lines are read this way.
    const fn as_of(entity_id: EntityId, account_type: AccountType, as_of: Date) -> Self {
        Self {
            entity_id,
            account_type,
            from: None,
            to: as_of,
            omit_hidden: false,
        }
    }

    /// Returns the query for the accounts of `account_type` with what was
    /// posted from `from` through `to`, hidden entries included.
    ///
    /// Income and expense lines are read this way.
    const fn in_period(
        entity_id: EntityId,
        account_type: AccountType,
        from: Date,
        to: Date,
    ) -> Self {
        Self {
            entity_id,
            account_type,
            from: Some(from),
            to,
            omit_hidden: false,
        }
    }

    /// Returns the same query with hidden entries left out when
    /// `omit_hidden` is true.
    const fn omitting_hidden(self, omit_hidden: bool) -> Self {
        Self {
            omit_hidden,
            ..self
        }
    }
}

/// Reads one line per account `query` selects that has a debit or a credit
/// in its window, in chart order, with the totals of the active entries
/// dated in the window.
///
/// Entry-level predicates (status, void, optional Hidden) live in the inner
/// subquery WHERE — never on the outer LEFT JOIN ON — so a filtered-out entry
/// contributes nothing. An account left with no activity is not returned.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored account type that does not parse,
///   or a column of the wrong storage class.
/// - [`Error::MoneyOverflow`] when a balance does not fit in `i64`.
/// - [`Error::Database`] on database errors, which include a total that overflows
///   `i64` inside `SQLite`'s `SUM`.
fn active_lines(conn: &Connection, query: LineQuery) -> Result<Vec<ReportLine>> {
    let mut stmt = conn
        .prepare(&active_lines_sql(query.omit_hidden))
        .database("read account activity")?;

    let rows = stmt
        .query_map(
            rusqlite::params![
                query.entity_id.to_string(),
                query.from.map(format_date),
                format_date(query.to),
                query.account_type.identifier(),
            ],
            |row| Ok(map_report_line("read account activity", row)),
        )
        .database("read account activity")?;

    let lines = collect_rows("read account activity", rows)?;
    Ok(lines.into_iter().filter(has_activity).collect())
}

/// The query of [`active_lines`]: the accounts of entity `?1` and type `?4`,
/// each with its debits and credits on active entries dated from `?2` (open
/// when `NULL`) through `?3`, hidden entries left out with `omit_hidden`.
fn active_lines_sql(omit_hidden: bool) -> String {
    let hidden_predicate = if omit_hidden { "AND je.hidden = 0" } else { "" };
    format!(
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
            WHERE {ACTIVE_ENTRY_PREDICATE}
              {hidden_predicate}
              AND (?2 IS NULL OR je.entry_date >= ?2)
              AND je.entry_date <= ?3
            GROUP BY jl.account_id
        ) t ON t.account_id = a.id
        WHERE a.entity_id = ?1
          AND a.account_type = ?4
        ORDER BY a.sort_order, a.code
        "
    )
}

/// Returns whether any debit or credit was posted to the line's account in
/// the window.
///
/// The balance needs no test of its own: it is the difference of the two
/// totals, so it is zero whenever both are.
fn has_activity(line: &ReportLine) -> bool {
    line.debit_minor != 0 || line.credit_minor != 0
}

/// The earliest date an entry can have: the first day of year zero.
///
/// This is the lower bound of what [`parse_date`](crate::util::parse_date)
/// reads, and every stored entry date went through it. `Date::MIN` would be
/// the wrong bound for the queries, which compare dates as text: it is
/// written with a leading minus sign (`-9999-01-01`), and text with a minus
/// sign does not sort by date.
const BOOKS_START: Date = time::macros::date!(0000 - 01 - 01);

/// The profit and loss that no closing entry has moved into equity, split at
/// the start of the fiscal year that holds the as-of date.
struct UnclosedPnl {
    /// First day of the fiscal year that holds the as-of date.
    year_start: Date,
    /// Income minus expenses from `year_start` through the as-of date, in
    /// minor units; negative for a loss.
    current_net: i64,
    /// Income minus expenses on every day before `year_start`, in minor
    /// units; negative for a loss.
    prior_net: i64,
}

/// Computes the unclosed profit and loss of an entity as of `as_of`, for a
/// fiscal year that starts in `fiscal_start`.
///
/// # Errors
///
/// - [`Error::MoneyOverflow`] when a net does not fit in `i64`.
/// - [`Error::Database`] on database errors.
fn unclosed_pnl(
    conn: &Connection,
    entity_id: EntityId,
    as_of: Date,
    fiscal_start: Month,
) -> Result<UnclosedPnl> {
    let year_start = fiscal_year_start(as_of, fiscal_start);
    let current_net = net_in_range(conn, entity_id, year_start, as_of)?;

    let prior_net = match year_start.previous_day() {
        Some(prior_end) if prior_end >= BOOKS_START => {
            net_in_range(conn, entity_id, BOOKS_START, prior_end)?
        }
        // The fiscal year holds the first day of the books, so nothing is
        // before it.
        _ => 0,
    };

    Ok(UnclosedPnl {
        year_start,
        current_net,
        prior_net,
    })
}

/// Returns income minus expenses between `from` and `to` inclusive, in minor
/// units, counting active entries only.
///
/// # Errors
///
/// - [`Error::MoneyOverflow`] when the difference does not fit in `i64`.
/// - [`Error::Database`] on database errors.
fn net_in_range(conn: &Connection, entity_id: EntityId, from: Date, to: Date) -> Result<i64> {
    let income = sum_type_in_range(conn, entity_id, AccountType::Income, from, to)?;
    let expenses = sum_type_in_range(conn, entity_id, AccountType::Expense, from, to)?;
    subtract_minor(income, expenses)
}

/// Builds a computed equity row that carries `net` as its balance.
///
/// Equity is credit-normal, so a profit is put on the credit side and a loss,
/// as a positive amount, on the debit side. That keeps the row's debit and
/// credit non-negative like those of a real account.
///
/// # Errors
///
/// [`Error::MoneyOverflow`] when `net` is `i64::MIN`, whose size does not fit
/// in `i64`.
fn equity_plug_line(
    code: &str,
    name: &str,
    net: i64,
    synthetic: SyntheticLine,
) -> Result<ReportLine> {
    let (debit_minor, credit_minor) = if net >= 0 {
        (0, net)
    } else {
        (net.checked_neg().ok_or(Error::MoneyOverflow)?, 0)
    };
    Ok(ReportLine {
        code: code.into(),
        name: name.into(),
        account_type: AccountType::Equity,
        debit_minor,
        credit_minor,
        balance_minor: net,
        synthetic: Some(synthetic),
    })
}

/// Builds the `RE` row: the result of the fiscal years before the current
/// one.
///
/// # Errors
///
/// Those of [`equity_plug_line`].
fn retained_earnings_line(prior_net: i64) -> Result<ReportLine> {
    equity_plug_line(
        "RE",
        "Retained Earnings (prior periods)",
        prior_net,
        SyntheticLine::RetainedEarnings,
    )
}

/// Builds the `NI` row: the result of the current fiscal year so far.
///
/// # Errors
///
/// Those of [`equity_plug_line`].
fn net_income_line(net: i64) -> Result<ReportLine> {
    equity_plug_line(
        "NI",
        "Net Income (current period)",
        net,
        SyntheticLine::NetIncome,
    )
}

/// Returns the first day of the fiscal year that contains `as_of`.
fn fiscal_year_start(as_of: Date, start_month: Month) -> Date {
    let year = if u8::from(as_of.month()) >= u8::from(start_month) {
        as_of.year()
    } else {
        as_of.year() - 1
    };

    // Day 1 exists in every month, so this fails only when `year` is below
    // `Date::MIN`'s year. That fiscal year began before the calendar does, and
    // the calendar's first day is then the earliest date inside it.
    Date::from_calendar_date(year, start_month, 1).unwrap_or(Date::MIN)
}

/// Maps a row selected as `code, name, account_type, debits, credits`.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for an account type that does not parse or a
///   column of the wrong storage class.
/// - [`Error::MoneyOverflow`] when the balance does not fit in `i64`.
fn map_report_line(operation: &'static str, row: &rusqlite::Row<'_>) -> Result<ReportLine> {
    let account_type = parse_account_type(&read_column::<String>(operation, row, 2)?)?;
    let debits: i64 = read_column(operation, row, 3)?;
    let credits: i64 = read_column(operation, row, 4)?;

    Ok(ReportLine {
        code: read_column(operation, row, 0)?,
        name: read_column(operation, row, 1)?,
        account_type,
        debit_minor: debits,
        credit_minor: credits,
        balance_minor: normal_balance(account_type, debits, credits)?,
        synthetic: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oikonomia_test_support::listed_variants;

    listed_variants! {
        units listed_lines for SyntheticLine {
            SyntheticLine::RetainedEarnings,
            SyntheticLine::NetIncome,
        }
    }

    /// Fails unless `SyntheticLine::ALL` is exactly the set of variants in the
    /// `listed_lines` list above, each once. The compiler checks that list
    /// against the enum with an exhaustive `match`, so a variant added to the
    /// enum but left out of the list does not compile. It does not check the
    /// order of `ALL`, nor that the UI has copy for a line; the shared-fixture
    /// test in `ui_text` does that.
    #[test]
    fn all_lists_every_synthetic_line() {
        let listed = listed_lines::variants();

        assert_eq!(
            SyntheticLine::ALL.len(),
            listed_lines::COUNT,
            "SyntheticLine::ALL and the listed variants differ in number"
        );
        for variant in listed {
            assert!(
                SyntheticLine::ALL.contains(&variant),
                "{variant:?} is missing from SyntheticLine::ALL"
            );
        }
        listed_lines::assert_every_position_once(
            SyntheticLine::ALL
                .iter()
                .map(listed_lines::position)
                .collect(),
        );
    }

    #[test]
    fn a_report_tests_for_a_void_through_the_index() {
        let conn = crate::db::migrated_connection();
        let entity = EntityId::generate().to_string();
        let from: Option<String> = None;

        for omit_hidden in [false, true] {
            let plan = crate::db::query_plan(
                &conn,
                &active_lines_sql(omit_hidden),
                &[&entity, &from, &"2026-12-31", &"expense"],
            );

            assert!(plan.contains("idx_entries_voided_by"), "{plan}");
        }
    }

    #[test]
    fn fiscal_year_start_is_the_latest_start_month_on_or_before_the_date() {
        let start = |as_of: &str, month: Month| {
            crate::util::parse_date(as_of).map(|date| format_date(fiscal_year_start(date, month)))
        };

        assert_eq!(start("2026-03-15", Month::January), Ok("2026-01-01".into()));
        assert_eq!(start("2026-03-15", Month::April), Ok("2025-04-01".into()));
        assert_eq!(start("2026-04-01", Month::April), Ok("2026-04-01".into()));
        assert_eq!(
            start("2026-12-31", Month::December),
            Ok("2026-12-01".into())
        );
    }

    #[test]
    fn fiscal_year_start_stops_at_the_first_day_of_the_calendar() {
        assert_eq!(fiscal_year_start(Date::MIN, Month::February), Date::MIN);
    }

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
