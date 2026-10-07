//! A total that does not fit in `i64` is an error, never a clamped or wrapped
//! figure presented as correct.
//!
//! Each test posts amounts that are fine one by one and overflow only when the
//! report adds or subtracts them in Rust. Totals that `SQLite` itself adds
//! with `SUM` are kept below the limit, because that overflow fails the query
//! before the code under test runs.

mod common;

use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    account_register, balance_sheet, cash_flow_series, dashboard_summary, profit_and_loss,
    trial_balance,
};
use rusqlite::Connection;

/// Two of these add up to one more than `i64::MAX`.
const HALF: i64 = i64::MAX / 2 + 1;

const CHECKING: &str = "1010";
const SAVINGS: &str = "1020";
const CREDIT_CARD: &str = "2000";
const BILLS_PAYABLE: &str = "2050";
const SALARY: &str = "4000";
const FREELANCE: &str = "4100";
const FOOD: &str = "5100";

/// Posts `amount` on `date`, debiting the first code of `sides` and crediting
/// the second.
fn post(conn: &Connection, entity_id: EntityId, date: &str, sides: (&str, &str), amount: i64) {
    common::post_two_line(conn, entity_id, date, sides, amount);
}

/// Income of `i64::MAX` and expenses of `-i64::MAX` (a refund), so that
/// income minus expenses overflows while each side fits.
fn book_whose_net_overflows(conn: &Connection) -> EntityId {
    let entity_id = common::book(conn, "Large", ChartTemplate::Personal);
    post(conn, entity_id, "2026-03-10", (CHECKING, SALARY), i64::MAX);
    post(
        conn,
        entity_id,
        "2026-03-20",
        (BILLS_PAYABLE, FOOD),
        i64::MAX,
    );
    entity_id
}

/// Two asset accounts of `HALF` each, funded from two liability accounts, so
/// that only totals across accounts overflow.
fn book_whose_asset_total_overflows(conn: &Connection) -> EntityId {
    let entity_id = common::book(conn, "Large", ChartTemplate::Personal);
    post(conn, entity_id, "2026-03-10", (CHECKING, CREDIT_CARD), HALF);
    post(
        conn,
        entity_id,
        "2026-03-20",
        (SAVINGS, BILLS_PAYABLE),
        HALF,
    );
    entity_id
}

#[test]
fn profit_and_loss_refuses_a_total_income_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Large", ChartTemplate::Personal);
    post(conn, entity_id, "2026-03-10", (CHECKING, SALARY), HALF);
    post(conn, entity_id, "2026-03-20", (SAVINGS, FREELANCE), HALF);

    let pnl = profit_and_loss(
        conn,
        entity_id,
        common::date("2026-01-01"),
        common::date("2026-12-31"),
    );

    assert_eq!(pnl.map(|pnl| pnl.total_income), Err(Error::MoneyOverflow));
}

#[test]
fn profit_and_loss_refuses_a_net_income_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = book_whose_net_overflows(conn);

    let pnl = profit_and_loss(
        conn,
        entity_id,
        common::date("2026-01-01"),
        common::date("2026-12-31"),
    );

    assert_eq!(pnl.map(|pnl| pnl.net_income), Err(Error::MoneyOverflow));
}

#[test]
fn balance_sheet_refuses_a_section_total_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = book_whose_asset_total_overflows(conn);

    let sheet = balance_sheet(conn, entity_id, common::date("2026-12-31"));

    assert_eq!(
        sheet.map(|sheet| sheet.total_assets),
        Err(Error::MoneyOverflow)
    );
}

#[test]
fn trial_balance_refuses_column_totals_that_overflow() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = book_whose_asset_total_overflows(conn);

    let trial = trial_balance(conn, entity_id, common::date("2026-12-31"));

    assert_eq!(
        trial.map(|trial| trial.total_debits),
        Err(Error::MoneyOverflow)
    );
}

#[test]
fn trial_balance_refuses_an_unclosed_net_income_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = book_whose_net_overflows(conn);

    let trial = trial_balance(conn, entity_id, common::date("2026-12-31"));

    assert_eq!(
        trial.map(|trial| trial.total_debits),
        Err(Error::MoneyOverflow)
    );
}

#[test]
fn dashboard_refuses_a_net_income_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = book_whose_net_overflows(conn);

    let summary = dashboard_summary(
        conn,
        entity_id,
        common::date("2026-03-01"),
        common::date("2026-03-31"),
        common::date("2026-03-31"),
    );

    assert_eq!(
        summary.map(|summary| summary.net_income),
        Err(Error::MoneyOverflow)
    );
}

#[test]
fn cash_flow_refuses_a_running_total_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Large", ChartTemplate::Personal);
    post(conn, entity_id, "2026-03-10", (CHECKING, SALARY), HALF);
    post(conn, entity_id, "2026-03-20", (SAVINGS, FREELANCE), HALF);

    let series = cash_flow_series(
        conn,
        entity_id,
        common::date("2026-03-01"),
        common::date("2026-03-31"),
    );

    assert_eq!(
        series.map(|series| series.total_income_minor),
        Err(Error::MoneyOverflow)
    );
}

#[test]
fn cash_flow_refuses_a_net_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = book_whose_net_overflows(conn);

    let series = cash_flow_series(
        conn,
        entity_id,
        common::date("2026-03-01"),
        common::date("2026-03-31"),
    );

    assert_eq!(
        series.map(|series| series.net_minor),
        Err(Error::MoneyOverflow)
    );
}

#[test]
fn account_register_refuses_a_running_balance_that_overflows() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Large", ChartTemplate::Personal);
    post(conn, entity_id, "2026-03-10", (CHECKING, SALARY), HALF);
    post(conn, entity_id, "2026-03-20", (CHECKING, FREELANCE), HALF);

    let register = account_register(conn, common::account(conn, entity_id, CHECKING), None, None);

    assert_eq!(register.map(|lines| lines.len()), Err(Error::MoneyOverflow));
}

/// Pins the `SQLite` behaviour the ledger module's overflow policy relies on:
/// `SUM` over the two asset accounts fails the query rather than wrapping.
///
/// The text of `SQLite`'s error is read on purpose: the variant alone would
/// not show that the query failed for the overflow and not for another reason.
#[test]
fn a_total_sqlite_adds_up_fails_the_query_instead_of_wrapping() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = book_whose_asset_total_overflows(conn);

    let summary = dashboard_summary(
        conn,
        entity_id,
        common::date("2026-03-01"),
        common::date("2026-03-31"),
        common::date("2026-03-31"),
    );

    assert!(
        matches!(
            &summary,
            Err(Error::Database { detail, .. }) if detail.contains("integer overflow")
        ),
        "{summary:?}"
    );
}

#[test]
fn amounts_at_the_limit_still_report() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Large", ChartTemplate::Personal);
    post(conn, entity_id, "2026-03-10", (CHECKING, SALARY), i64::MAX);

    let pnl = profit_and_loss(
        conn,
        entity_id,
        common::date("2026-01-01"),
        common::date("2026-12-31"),
    )
    .expect("pnl");
    assert_eq!(pnl.net_income, i64::MAX);

    let sheet = balance_sheet(conn, entity_id, common::date("2026-12-31")).expect("sheet");
    assert_eq!(sheet.total_assets, i64::MAX);
    assert_eq!(sheet.total_liabilities_equity, i64::MAX);
}
