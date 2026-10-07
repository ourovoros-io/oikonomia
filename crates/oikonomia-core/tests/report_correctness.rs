//! Regression tests for report filtering (review finding F1).
//!
//! Reports must only aggregate posted, non-voided entries inside the
//! requested date window; the dashboard and the report pages must agree.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::error::Error;
use oikonomia_core::error::ValidationError;
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, PostJournal, ReportLine, SyntheticLine, balance_sheet,
    cash_flow_series, create_entity, dashboard_summary, list_accounts, post_entry, profit_and_loss,
    set_account_opening_balance, set_entry_hidden, trial_balance, void_entry,
};
use oikonomia_core::prefs::Locale;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use rusqlite::Connection;

fn setup_entity(conn: &Connection) -> EntityId {
    setup_entity_fy(conn, 1)
}

fn setup_entity_fy(conn: &Connection, fiscal_year_start_month: u8) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: "Probe".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(fiscal_year_start_month),
        },
        Locale::En,
    )
    .expect("entity")
    .id
}

fn line<'a>(lines: &'a [ReportLine], code: &str) -> &'a ReportLine {
    lines.iter().find(|l| l.code == code).expect(code)
}

/// The light and the dashboard must never disagree, whatever the ledger holds.
fn assert_series_matches_dashboard(conn: &Connection, entity_id: EntityId, from: &str, to: &str) {
    let series = cash_flow_series(conn, entity_id, from, to).expect("series");
    let dash = dashboard_summary(conn, entity_id, from, to, to).expect("dash");
    let last = series.buckets.last().expect("bucket");
    assert_eq!(
        last.cumulative_income_minor, dash.income,
        "series income {from}..{to}"
    );
    assert_eq!(
        last.cumulative_expenses_minor, dash.expenses,
        "series expenses {from}..{to}"
    );
    assert_eq!(series.net_minor, dash.net_income, "series net {from}..{to}");
}

fn post_expense(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    minor: i64,
) -> oikonomia_core::domain::JournalEntryId {
    common::post_two_line(conn, entity_id, date, ("5100", "1010"), minor)
        .entry
        .id
}

#[test]
fn pnl_respects_date_range() {
    let (_dir, vault) = {
        let (dir, vault) = common::vault();
        (dir, vault)
    };
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-01-15", 1_000);
    post_expense(conn, entity_id, "2026-03-15", 2_000);

    let pnl = profit_and_loss(conn, entity_id, "2026-03-01", "2026-03-31").expect("pnl");
    assert_eq!(pnl.total_expenses, 2_000, "March P&L must exclude January");
    assert_eq!(pnl.net_income, -2_000);
}

#[test]
fn trial_balance_respects_as_of() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-01-15", 1_000);
    post_expense(conn, entity_id, "2026-03-15", 2_000);

    let tb = trial_balance(conn, entity_id, "2026-02-01").expect("tb");
    assert_eq!(tb.total_debits, 1_000, "TB as of Feb 1 must exclude March");
    assert_eq!(tb.total_credits, 1_000);
}

#[test]
fn balance_sheet_balances_for_past_as_of() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-01-15", 1_000);
    post_expense(conn, entity_id, "2026-03-15", 2_000);

    let bs = balance_sheet(conn, entity_id, "2026-02-01").expect("bs");
    assert_eq!(
        bs.total_assets, bs.total_liabilities_equity,
        "BS as of Feb 1 must balance: assets {} vs liab+equity {}",
        bs.total_assets, bs.total_liabilities_equity
    );
    assert_eq!(
        bs.total_assets, -1_000,
        "assets as of Feb 1 reflect only the January credit to checking"
    );
}

/// Year zero is the earliest year an entry date can be in, so an entry there
/// is a prior-period result like any other.
#[test]
fn an_entry_dated_in_year_zero_reaches_retained_earnings() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "0000-01-01", 1_000);

    let sheet = balance_sheet(conn, entity_id, "0002-01-31").expect("sheet");

    assert_eq!(line(&sheet.equity.lines, "RE").balance_minor, -1_000);
    assert_eq!(sheet.total_assets, sheet.total_liabilities_equity);

    let trial = trial_balance(conn, entity_id, "0002-01-31").expect("trial");

    assert_eq!(line(&trial.lines, "RE").debit_minor, 1_000);
    assert_eq!(trial.total_debits, trial.total_credits);
}

/// Before year zero there are no books: the fiscal year that holds the first
/// day of year zero has no prior period.
///
/// This pins the edge and does not depend on where the prior-period sum
/// starts: the day before year zero is written with a leading minus sign and
/// sorts, as text, below every stored date, so no entry is on or before it.
#[test]
fn the_first_fiscal_year_has_no_retained_earnings() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "0000-01-01", 1_000);

    let sheet = balance_sheet(conn, entity_id, "0000-12-31").expect("sheet");

    assert!(sheet.equity.lines.iter().all(|line| line.code != "RE"));
    assert_eq!(line(&sheet.equity.lines, "NI").balance_minor, -1_000);
    assert_eq!(sheet.total_assets, sheet.total_liabilities_equity);
}

#[test]
fn balance_sheet_balances_after_fiscal_year_boundary() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-06-01", 1_000);

    let bs_2026 = balance_sheet(conn, entity_id, "2026-12-31").expect("bs 2026");
    assert_eq!(bs_2026.total_assets, bs_2026.total_liabilities_equity);
    assert_eq!(bs_2026.total_assets, -1_000);

    let bs_2027 = balance_sheet(conn, entity_id, "2027-01-31").expect("bs 2027");
    assert_eq!(
        bs_2027.total_assets, bs_2027.total_liabilities_equity,
        "2027 as-of must still balance: assets {} vs L+E {}",
        bs_2027.total_assets, bs_2027.total_liabilities_equity
    );
    assert_eq!(bs_2027.total_assets, -1_000);
    assert!(
        bs_2027
            .equity
            .lines
            .iter()
            .any(|l| l.code == "RE" && l.balance_minor == -1_000),
        "prior-year P&L must appear as RE: {:?}",
        bs_2027.equity.lines
    );
    assert!(
        !bs_2027.equity.lines.iter().any(|l| l.code == "NI"),
        "current-FY NI must be omitted when zero: {:?}",
        bs_2027.equity.lines
    );
}

#[test]
fn balance_sheet_balances_when_activity_is_before_fy_start_month() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity_fy(conn, 7);
    post_expense(conn, entity_id, "2026-03-15", 1_000);

    let bs = balance_sheet(conn, entity_id, "2026-08-01").expect("bs");
    assert_eq!(
        bs.total_assets, bs.total_liabilities_equity,
        "July-FY as-of must balance: assets {} vs L+E {}",
        bs.total_assets, bs.total_liabilities_equity
    );
    assert_eq!(bs.total_assets, -1_000);
    assert!(
        bs.equity
            .lines
            .iter()
            .any(|l| l.code == "RE" && l.balance_minor == -1_000),
        "March expense is prior-period RE: {:?}",
        bs.equity.lines
    );

    let tb = trial_balance(conn, entity_id, "2026-08-01").expect("tb");
    assert_eq!(tb.total_debits, tb.total_credits);
    assert!(
        tb.lines
            .iter()
            .any(|l| l.code == "RE" && l.balance_minor == -1_000),
        "TB RE must match the July-FY balance sheet: {:?}",
        tb.lines
    );
    assert!(
        !tb.lines
            .iter()
            .any(|l| l.code == "5100" && l.debit_minor != 0),
        "March expense is prior-period on a July FY: {:?}",
        tb.lines
    );
}

fn post_income(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    minor: i64,
) -> oikonomia_core::domain::JournalEntryId {
    common::post_two_line(conn, entity_id, date, ("1010", "4000"), minor)
        .entry
        .id
}

fn post_transfer(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    minor: i64,
) -> oikonomia_core::domain::JournalEntryId {
    common::post_two_line(conn, entity_id, date, ("1020", "1010"), minor)
        .entry
        .id
}

fn post_unpaid_bill(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    minor: i64,
) -> oikonomia_core::domain::JournalEntryId {
    common::post_two_line(conn, entity_id, date, ("5100", "2050"), minor)
        .entry
        .id
}

#[test]
fn trial_balance_folds_prior_year_pnl_like_balance_sheet() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-06-01", 1_000);

    let tb_2026 = trial_balance(conn, entity_id, "2026-12-31").expect("tb 2026");
    assert_eq!(tb_2026.total_debits, tb_2026.total_credits);
    assert!(
        tb_2026
            .lines
            .iter()
            .any(|l| l.code == "5100" && l.debit_minor == 1_000),
        "current-FY expense stays on the trial balance: {:?}",
        tb_2026.lines
    );
    assert!(
        !tb_2026.lines.iter().any(|l| l.code == "RE"),
        "RE is omitted while the year is still open: {:?}",
        tb_2026.lines
    );

    let tb_2027 = trial_balance(conn, entity_id, "2027-01-31").expect("tb 2027");
    assert_eq!(
        tb_2027.total_debits, tb_2027.total_credits,
        "2027 TB must still balance: {:?}",
        tb_2027.lines
    );
    assert!(
        !tb_2027
            .lines
            .iter()
            .any(|l| l.code == "5100" && l.debit_minor != 0),
        "prior-year P&L must leave the expense account: {:?}",
        tb_2027.lines
    );
    assert!(
        tb_2027
            .lines
            .iter()
            .any(|l| l.code == "RE" && l.debit_minor == 1_000 && l.balance_minor == -1_000),
        "prior-year net loss must appear as RE: {:?}",
        tb_2027.lines
    );
}

#[test]
fn historical_pnl_and_as_of_reports_agree_across_years() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_income(conn, entity_id, "2025-06-01", 5_000);
    post_expense(conn, entity_id, "2026-03-01", 1_000);

    let pnl_2025 = profit_and_loss(conn, entity_id, "2025-01-01", "2025-12-31").expect("pnl 2025");
    assert_eq!(pnl_2025.total_income, 5_000);
    assert_eq!(pnl_2025.total_expenses, 0);
    assert_eq!(pnl_2025.net_income, 5_000);

    let pnl_2026 = profit_and_loss(conn, entity_id, "2026-01-01", "2026-12-31").expect("pnl 2026");
    assert_eq!(pnl_2026.total_income, 0);
    assert_eq!(pnl_2026.total_expenses, 1_000);
    assert_eq!(pnl_2026.net_income, -1_000);

    let bs = balance_sheet(conn, entity_id, "2026-12-31").expect("bs");
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);
    assert_eq!(
        bs.equity
            .lines
            .iter()
            .find(|l| l.code == "RE")
            .map(|l| l.balance_minor),
        Some(5_000),
        "2025 income is prior-period RE: {:?}",
        bs.equity.lines
    );
    assert_eq!(
        bs.equity
            .lines
            .iter()
            .find(|l| l.code == "NI")
            .map(|l| l.balance_minor),
        Some(-1_000),
        "2026 expense is current-FY NI: {:?}",
        bs.equity.lines
    );

    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("tb");
    assert_eq!(tb.total_debits, tb.total_credits);
    assert_eq!(
        tb.lines
            .iter()
            .find(|l| l.code == "RE")
            .map(|l| (l.credit_minor, l.balance_minor)),
        Some((5_000, 5_000)),
        "TB RE must match the balance sheet as a credit: {:?}",
        tb.lines
    );
    assert!(
        tb.lines
            .iter()
            .any(|l| l.code == "5100" && l.debit_minor == 1_000),
        "current-FY expense stays on TB: {:?}",
        tb.lines
    );
    assert!(
        !tb.lines
            .iter()
            .any(|l| l.code == "4000" && l.credit_minor != 0),
        "prior-year income must leave the income account: {:?}",
        tb.lines
    );
}

#[test]
fn voided_entry_leaves_no_trace_in_trial_balance() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry_id = post_expense(conn, entity_id, "2026-01-15", 1_000);
    void_entry(conn, entry_id, Locale::En).expect("void");

    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("tb");
    assert!(
        tb.lines.is_empty(),
        "voided pair must not appear as gross activity: {:?}",
        tb.lines
    );
    assert_eq!(tb.total_debits, 0);
    assert_eq!(tb.total_credits, 0);
}

#[test]
fn randomized_entries_keep_reports_consistent() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let accounts = list_accounts(conn, entity_id).expect("accounts");

    let mut rng = StdRng::seed_from_u64(0x0110_2026);
    for _ in 0..50 {
        let debit_idx = rng.random_range(0..accounts.len());
        let credit_idx = (debit_idx + rng.random_range(1..accounts.len())) % accounts.len();
        let minor = rng.random_range(1..=100_000);
        let month = rng.random_range(1..=12);
        let day = rng.random_range(1..=28);

        post_entry(
            conn,
            &PostJournal {
                entity_id,
                entry_date: format!("2026-{month:02}-{day:02}"),
                description: "random".into(),
                reference: None,
                lines: vec![
                    CreateJournalLine {
                        account_id: accounts[debit_idx].id,
                        debit_minor: minor,
                        credit_minor: 0,
                        memo: None,
                    },
                    CreateJournalLine {
                        account_id: accounts[credit_idx].id,
                        debit_minor: 0,
                        credit_minor: minor,
                        memo: None,
                    },
                ],
            },
        )
        .expect("post random");
    }

    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("tb");
    assert_eq!(tb.total_debits, tb.total_credits, "TB must balance");

    for as_of in ["2026-04-15", "2026-08-15", "2026-12-31"] {
        let bs = balance_sheet(conn, entity_id, as_of).expect("bs");
        assert_eq!(
            bs.total_assets, bs.total_liabilities_equity,
            "BS as of {as_of} must balance"
        );
    }

    for (from, to) in [
        ("2026-01-01", "2026-12-31"),
        ("2026-03-01", "2026-03-31"),
        ("2026-04-01", "2026-06-30"),
        ("2026-02-10", "2026-11-20"),
    ] {
        assert_series_matches_dashboard(conn, entity_id, from, to);
    }
}

#[test]
fn pnl_window_is_inclusive_on_both_ends() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-02-28", 100);
    post_expense(conn, entity_id, "2026-03-01", 200);
    post_expense(conn, entity_id, "2026-03-31", 400);
    post_expense(conn, entity_id, "2026-04-01", 800);

    let pnl = profit_and_loss(conn, entity_id, "2026-03-01", "2026-03-31").expect("pnl");
    assert_eq!(pnl.total_expenses, 600, "both March endpoints count");
    assert_eq!(pnl.net_income, -600);
}

#[test]
fn pnl_single_day_is_a_valid_window() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-03-14", 100);
    post_expense(conn, entity_id, "2026-03-15", 250);
    post_expense(conn, entity_id, "2026-03-16", 400);

    let pnl = profit_and_loss(conn, entity_id, "2026-03-15", "2026-03-15").expect("pnl");
    assert_eq!(pnl.from.to_string(), "2026-03-15");
    assert_eq!(pnl.to.to_string(), "2026-03-15");
    assert_eq!(pnl.total_expenses, 250);
}

#[test]
fn pnl_rejects_inverted_and_invalid_dates() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    assert_eq!(
        profit_and_loss(conn, entity_id, "2026-03-31", "2026-03-01").expect_err("inverted"),
        Error::Validation(ValidationError::DateRangeInverted)
    );
    assert_eq!(
        profit_and_loss(conn, entity_id, "2026-13-01", "2026-03-31").expect_err("bad date"),
        Error::Validation(ValidationError::InvalidDate {
            value: "2026-13-01".into()
        })
    );
}

#[test]
fn reports_reject_unknown_entity() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let missing = EntityId::new();

    assert_eq!(
        trial_balance(conn, missing, "2026-12-31").expect_err("tb"),
        Error::NotFound("entity".into())
    );
    assert_eq!(
        profit_and_loss(conn, missing, "2026-01-01", "2026-12-31").expect_err("pnl"),
        Error::NotFound("entity".into())
    );
    assert_eq!(
        balance_sheet(conn, missing, "2026-12-31").expect_err("bs"),
        Error::NotFound("entity".into())
    );
}

#[test]
fn empty_books_reports_are_zero_and_balanced() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let pnl = profit_and_loss(conn, entity_id, "2026-01-01", "2026-12-31").expect("pnl");
    assert!(pnl.income.is_empty());
    assert!(pnl.expenses.is_empty());
    assert_eq!(pnl.net_income, 0);

    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("tb");
    assert!(tb.lines.is_empty());
    assert_eq!(tb.total_debits, 0);
    assert_eq!(tb.total_credits, 0);

    let bs = balance_sheet(conn, entity_id, "2026-12-31").expect("bs");
    assert!(bs.assets.lines.is_empty());
    assert!(bs.liabilities.lines.is_empty());
    assert!(bs.equity.lines.is_empty());
    assert_eq!(bs.total_assets, 0);
    assert_eq!(bs.total_liabilities_equity, 0);
}

#[test]
fn as_of_is_inclusive_and_excludes_the_next_day() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-03-15", 1_000);

    let before = balance_sheet(conn, entity_id, "2026-03-14").expect("before");
    assert_eq!(before.total_assets, 0);
    assert!(
        trial_balance(conn, entity_id, "2026-03-14")
            .expect("tb before")
            .lines
            .is_empty()
    );

    let on_day = balance_sheet(conn, entity_id, "2026-03-15").expect("on day");
    assert_eq!(on_day.total_assets, -1_000);
    assert_eq!(on_day.total_assets, on_day.total_liabilities_equity);
}

#[test]
fn transfers_do_not_move_pnl_and_keep_the_balance_sheet_balanced() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_income(conn, entity_id, "2026-03-01", 10_000);
    post_transfer(conn, entity_id, "2026-03-02", 4_000);

    let pnl = profit_and_loss(conn, entity_id, "2026-03-01", "2026-03-31").expect("pnl");
    assert_eq!(pnl.total_income, 10_000);
    assert_eq!(pnl.total_expenses, 0);
    assert_eq!(pnl.net_income, 10_000);

    let bs = balance_sheet(conn, entity_id, "2026-03-31").expect("bs");
    assert_eq!(bs.total_assets, 10_000);
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);
    assert_eq!(line(&bs.assets.lines, "1010").balance_minor, 6_000);
    assert_eq!(line(&bs.assets.lines, "1020").balance_minor, 4_000);
}

#[test]
fn unpaid_bill_is_expense_and_liability_not_an_asset_hit() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_unpaid_bill(conn, entity_id, "2026-03-10", 7_253);

    let pnl = profit_and_loss(conn, entity_id, "2026-03-01", "2026-03-31").expect("pnl");
    assert_eq!(pnl.total_expenses, 7_253);
    assert_eq!(pnl.net_income, -7_253);

    let bs = balance_sheet(conn, entity_id, "2026-03-31").expect("bs");
    assert_eq!(bs.total_assets, 0);
    assert_eq!(line(&bs.liabilities.lines, "2050").balance_minor, 7_253);
    assert_eq!(line(&bs.equity.lines, "NI").balance_minor, -7_253);
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);
}

#[test]
fn opening_balance_is_equity_not_pnl() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    set_account_opening_balance(
        conn,
        common::account(conn, entity_id, "1010"),
        250_000,
        "2026-01-01",
        Locale::En,
    )
    .expect("opening");
    post_expense(conn, entity_id, "2026-02-01", 1_000);

    let pnl = profit_and_loss(conn, entity_id, "2026-01-01", "2026-12-31").expect("pnl");
    assert_eq!(pnl.total_income, 0);
    assert_eq!(pnl.total_expenses, 1_000);

    let bs = balance_sheet(conn, entity_id, "2026-12-31").expect("bs");
    assert_eq!(bs.total_assets, 249_000);
    assert_eq!(line(&bs.equity.lines, "3000").balance_minor, 250_000);
    assert_eq!(line(&bs.equity.lines, "NI").balance_minor, -1_000);
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);
}

#[test]
fn reports_do_not_leak_across_entities() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let a = setup_entity(conn);
    let b = create_entity(
        conn,
        &CreateEntity {
            name: "Other".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("other")
    .id;
    post_expense(conn, a, "2026-03-01", 1_000);
    post_expense(conn, b, "2026-03-01", 9_000);

    let pnl_a = profit_and_loss(conn, a, "2026-01-01", "2026-12-31").expect("a");
    let pnl_b = profit_and_loss(conn, b, "2026-01-01", "2026-12-31").expect("b");
    assert_eq!(pnl_a.total_expenses, 1_000);
    assert_eq!(pnl_b.total_expenses, 9_000);

    let bs_a = balance_sheet(conn, a, "2026-12-31").expect("bs a");
    assert_eq!(bs_a.total_assets, -1_000);
}

#[test]
fn fy_start_date_is_current_period_the_day_before_is_retained_earnings() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2025-12-31", 1_000);
    post_expense(conn, entity_id, "2026-01-01", 400);

    let bs = balance_sheet(conn, entity_id, "2026-01-01").expect("bs");
    assert_eq!(line(&bs.equity.lines, "RE").balance_minor, -1_000);
    assert_eq!(line(&bs.equity.lines, "NI").balance_minor, -400);
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);

    let tb = trial_balance(conn, entity_id, "2026-01-01").expect("tb");
    assert_eq!(tb.total_debits, tb.total_credits);
    assert_eq!(line(&tb.lines, "RE").debit_minor, 1_000);
    assert_eq!(line(&tb.lines, "5100").debit_minor, 400);
}

#[test]
fn july_fy_start_day_is_current_period() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity_fy(conn, 7);
    post_expense(conn, entity_id, "2026-06-30", 1_000);
    post_expense(conn, entity_id, "2026-07-01", 250);

    let bs = balance_sheet(conn, entity_id, "2026-07-01").expect("bs");
    assert_eq!(line(&bs.equity.lines, "RE").balance_minor, -1_000);
    assert_eq!(line(&bs.equity.lines, "NI").balance_minor, -250);
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);

    let tb = trial_balance(conn, entity_id, "2026-07-01").expect("tb");
    assert_eq!(tb.total_debits, tb.total_credits);
    assert_eq!(line(&tb.lines, "5100").debit_minor, 250);
}

#[test]
fn voided_entry_leaves_no_trace_on_pnl_or_balance_sheet() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let id = post_expense(conn, entity_id, "2026-03-15", 1_000);
    void_entry(conn, id, Locale::En).expect("void");

    let pnl = profit_and_loss(conn, entity_id, "2026-01-01", "2026-12-31").expect("pnl");
    assert_eq!(pnl.total_expenses, 0);
    assert!(pnl.expenses.is_empty());

    let bs = balance_sheet(conn, entity_id, "2026-12-31").expect("bs");
    assert_eq!(bs.total_assets, 0);
    assert_eq!(bs.total_liabilities_equity, 0);
    assert!(bs.equity.lines.is_empty());
}

#[test]
fn hidden_entries_stay_on_in_app_reports() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let id = post_expense(conn, entity_id, "2026-03-15", 1_000);
    set_entry_hidden(conn, id, true).expect("hide");

    let pnl = profit_and_loss(conn, entity_id, "2026-01-01", "2026-12-31").expect("pnl");
    assert_eq!(pnl.total_expenses, 1_000);

    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("tb");
    assert_eq!(line(&tb.lines, "5100").debit_minor, 1_000);

    let bs = balance_sheet(conn, entity_id, "2026-12-31").expect("bs");
    assert_eq!(bs.total_assets, -1_000);
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);
}

#[test]
fn pnl_omits_accounts_with_no_period_activity() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-03-15", 1_000);

    let pnl = profit_and_loss(conn, entity_id, "2026-03-01", "2026-03-31").expect("pnl");
    assert!(pnl.income.is_empty());
    assert_eq!(pnl.expenses.len(), 1);
    assert_eq!(pnl.expenses[0].code, "5100");
}

#[test]
fn ytd_pnl_matches_balance_sheet_net_income_and_dashboard() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_income(conn, entity_id, "2026-01-10", 8_000);
    post_expense(conn, entity_id, "2026-02-10", 3_000);
    post_expense(conn, entity_id, "2025-12-01", 500);

    let pnl = profit_and_loss(conn, entity_id, "2026-01-01", "2026-06-30").expect("pnl");
    let bs = balance_sheet(conn, entity_id, "2026-06-30").expect("bs");
    let dash =
        dashboard_summary(conn, entity_id, "2026-01-01", "2026-06-30", "2026-06-30").expect("dash");

    assert_eq!(pnl.net_income, 5_000);
    assert_eq!(line(&bs.equity.lines, "NI").balance_minor, pnl.net_income);
    assert_eq!(line(&bs.equity.lines, "RE").balance_minor, -500);
    assert_eq!(dash.income, pnl.total_income);
    assert_eq!(dash.expenses, pnl.total_expenses);
    assert_eq!(dash.net_income, pnl.net_income);
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);
}

#[test]
fn company_chart_synthetic_re_does_not_use_the_posted_re_account() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = create_entity(
        conn,
        &CreateEntity {
            name: "Co".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Company,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("entity")
    .id;
    post_expense(conn, entity_id, "2025-06-01", 1_000);

    let bs = balance_sheet(conn, entity_id, "2026-01-31").expect("bs");
    assert_eq!(line(&bs.equity.lines, "RE").balance_minor, -1_000);
    assert!(
        !bs.equity.lines.iter().any(|l| l.code == "3200"),
        "posted Retained Earnings stays off the sheet until journaled: {:?}",
        bs.equity.lines
    );
}

#[test]
fn randomized_multi_year_entries_keep_tb_bs_and_ytd_pnl_aligned() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let accounts = list_accounts(conn, entity_id).expect("accounts");

    let mut rng = StdRng::seed_from_u64(0x2025_2027);
    for _ in 0..60 {
        let debit_idx = rng.random_range(0..accounts.len());
        let credit_idx = (debit_idx + rng.random_range(1..accounts.len())) % accounts.len();
        let minor = rng.random_range(1..=100_000);
        let year = rng.random_range(2025..=2027);
        let month = rng.random_range(1..=12);
        let day = rng.random_range(1..=28);

        post_entry(
            conn,
            &PostJournal {
                entity_id,
                entry_date: format!("{year}-{month:02}-{day:02}"),
                description: "random".into(),
                reference: None,
                lines: vec![
                    CreateJournalLine {
                        account_id: accounts[debit_idx].id,
                        debit_minor: minor,
                        credit_minor: 0,
                        memo: None,
                    },
                    CreateJournalLine {
                        account_id: accounts[credit_idx].id,
                        debit_minor: 0,
                        credit_minor: minor,
                        memo: None,
                    },
                ],
            },
        )
        .expect("post random");
    }

    for as_of in ["2025-12-31", "2026-06-15", "2026-12-31", "2027-03-01"] {
        let tb = trial_balance(conn, entity_id, as_of).expect("tb");
        assert_eq!(
            tb.total_debits, tb.total_credits,
            "TB as of {as_of} must balance"
        );

        let bs = balance_sheet(conn, entity_id, as_of).expect("bs");
        assert_eq!(
            bs.total_assets, bs.total_liabilities_equity,
            "BS as of {as_of} must balance"
        );

        let tb_re = tb
            .lines
            .iter()
            .find(|l| l.code == "RE")
            .map_or(0, |l| l.balance_minor);
        let bs_re = bs
            .equity
            .lines
            .iter()
            .find(|l| l.code == "RE")
            .map_or(0, |l| l.balance_minor);
        assert_eq!(tb_re, bs_re, "TB RE must match BS RE as of {as_of}");

        let year = as_of
            .get(..4)
            .expect("a date starts with a four-digit year");
        let fy_start = format!("{year}-01-01");
        let pnl = profit_and_loss(conn, entity_id, &fy_start, as_of).expect("pnl");
        let bs_ni = bs
            .equity
            .lines
            .iter()
            .find(|l| l.code == "NI")
            .map_or(0, |l| l.balance_minor);
        assert_eq!(
            pnl.net_income, bs_ni,
            "YTD P&L must equal BS NI as of {as_of}"
        );
    }

    for (from, to) in [
        ("2025-01-01", "2027-12-31"),
        ("2026-07-01", "2026-09-30"),
        ("2025-11-15", "2026-02-14"),
    ] {
        assert_series_matches_dashboard(conn, entity_id, from, to);
    }
}

#[test]
fn randomized_ledgers_with_voids_keep_the_series_equal_to_the_dashboard() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let accounts = list_accounts(conn, entity_id).expect("accounts");

    let mut rng = StdRng::seed_from_u64(0x0915_2026);
    for index in 0..60 {
        let debit_idx = rng.random_range(0..accounts.len());
        let credit_idx = (debit_idx + rng.random_range(1..accounts.len())) % accounts.len();
        let minor = rng.random_range(1..=100_000);
        let month = rng.random_range(1..=12);
        let day = rng.random_range(1..=28);

        let view = post_entry(
            conn,
            &PostJournal {
                entity_id,
                entry_date: format!("2026-{month:02}-{day:02}"),
                description: "random".into(),
                reference: None,
                lines: vec![
                    CreateJournalLine {
                        account_id: accounts[debit_idx].id,
                        debit_minor: minor,
                        credit_minor: 0,
                        memo: None,
                    },
                    CreateJournalLine {
                        account_id: accounts[credit_idx].id,
                        debit_minor: 0,
                        credit_minor: minor,
                        memo: None,
                    },
                ],
            },
        )
        .expect("post random");
        if index % 5 == 0 {
            void_entry(conn, view.entry.id, Locale::En).expect("void random");
        }
    }

    for (from, to) in [
        ("2026-01-01", "2026-12-31"),
        ("2026-05-01", "2026-05-31"),
        ("2026-10-01", "2026-12-31"),
    ] {
        assert_series_matches_dashboard(conn, entity_id, from, to);
    }
}

#[test]
fn only_the_synthetic_rows_carry_the_synthetic_marker() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-06-01", 1_000);

    // A year later the expense is a prior-period result, so RE appears.
    let next_year = balance_sheet(conn, entity_id, "2027-01-31").expect("next year");
    let retained = line(&next_year.equity.lines, "RE");
    assert_eq!(retained.synthetic, Some(SyntheticLine::RetainedEarnings));
    assert_eq!(
        retained.name, "Retained Earnings (prior periods)",
        "the English name stays for exports and as a fallback"
    );

    // In the same year it is the current result, so NI appears.
    let same_year = balance_sheet(conn, entity_id, "2026-12-31").expect("same year");
    let net_income = line(&same_year.equity.lines, "NI");
    assert_eq!(net_income.synthetic, Some(SyntheticLine::NetIncome));
    assert_eq!(net_income.name, "Net Income (current period)");

    let ledger_rows = same_year
        .assets
        .lines
        .iter()
        .chain(&same_year.liabilities.lines)
        .chain(same_year.equity.lines.iter().filter(|l| l.code != "NI"));
    for row in ledger_rows {
        assert_eq!(row.synthetic, None, "{} is a real account", row.code);
    }

    // The trial balance lists the same synthetic rows.
    let trial = trial_balance(conn, entity_id, "2027-01-31").expect("trial");
    assert_eq!(
        line(&trial.lines, "RE").synthetic,
        Some(SyntheticLine::RetainedEarnings)
    );
}

#[test]
fn a_report_line_serializes_its_marker_as_a_snake_case_code() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-06-01", 1_000);

    let sheet = balance_sheet(conn, entity_id, "2026-12-31").expect("sheet");
    let json = serde_json::to_value(line(&sheet.equity.lines, "NI")).expect("json");

    assert_eq!(json["synthetic"], "net_income");
    assert_eq!(json["code"], "NI");

    let checking = serde_json::to_value(&sheet.assets.lines[0]).expect("json");
    assert_eq!(checking["synthetic"], serde_json::Value::Null);
}
