//! Dashboard figures, hand-verified: full-month range for activity numbers,
//! assets pinned to "today", voids and window edges handled.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::domain::{ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::ledger::SimpleEntryKind::{Bill, Expense, Income, Transfer};
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntryRequest, SimpleBillStatus, SimpleEntryKind,
    create_entity, dashboard_summary, list_entries, previous_window, void_entry,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::util::{format_date, parse_date};
use rusqlite::Connection;

fn base(
    entity_id: EntityId,
    kind: SimpleEntryKind,
    date: &str,
    minor: i64,
) -> PostSimpleEntryRequest {
    PostSimpleEntryRequest {
        entity_id,
        kind,
        bill_status: None,
        entry_date: date.into(),
        description: format!("{kind:?} {minor} on {date}"),
        reference: None,
        amount_minor: minor,
        category_account_id: None,
        wallet_account_id: None,
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    }
}

fn post(conn: &Connection, input: &PostSimpleEntryRequest) -> JournalEntryId {
    common::post_simple_request(conn, input)
        .expect("post simple")
        .entry
        .id
}

/// August 2026 ledger: salary, groceries, a transfer, a card expense, a voided
/// entry, an OCR-style bill dated at its FUTURE due date, and a future paid
/// expense.
fn seed_august_ledger(conn: &Connection, e: EntityId) {
    let mut salary = base(e, Income, "2026-08-03", 100_000);
    salary.category_account_id = Some(common::account(conn, e, "4000"));
    salary.wallet_account_id = Some(common::account(conn, e, "1010"));
    post(conn, &salary);

    let mut groceries = base(e, Expense, "2026-08-05", 2_500);
    groceries.category_account_id = Some(common::account(conn, e, "5100"));
    groceries.wallet_account_id = Some(common::account(conn, e, "1010"));
    post(conn, &groceries);

    // Checking → savings: must not touch income/expenses.
    let mut move_savings = base(e, Transfer, "2026-08-06", 30_000);
    move_savings.from_account_id = Some(common::account(conn, e, "1010"));
    move_savings.to_account_id = Some(common::account(conn, e, "1020"));
    post(conn, &move_savings);

    // On the credit card: expense counts, assets untouched.
    let mut card = base(e, Expense, "2026-08-07", 4_000);
    card.category_account_id = Some(common::account(conn, e, "5600"));
    card.wallet_account_id = Some(common::account(conn, e, "2000"));
    post(conn, &card);

    // Voided: must vanish everywhere.
    let mut mistake = base(e, Expense, "2026-08-08", 9_999);
    mistake.category_account_id = Some(common::account(conn, e, "5100"));
    mistake.wallet_account_id = Some(common::account(conn, e, "1010"));
    let voided = post(conn, &mistake);
    void_entry(conn, voided, Locale::En).expect("void");

    // OCR-style bill posted with a future due date (Aug 13 > "today" Aug 10).
    let mut due_bill = base(e, Bill, "2026-08-13", 7_253);
    due_bill.bill_status = Some(SimpleBillStatus::Unpaid);
    due_bill.category_account_id = Some(common::account(conn, e, "5300"));
    due_bill.payable_account_id = Some(common::account(conn, e, "2050"));
    post(conn, &due_bill);

    // Future-dated expense PAID from checking: counts for the month, but must
    // not move "assets as of today".
    let mut future_paid = base(e, Expense, "2026-08-20", 1_000);
    future_paid.category_account_id = Some(common::account(conn, e, "5900"));
    future_paid.wallet_account_id = Some(common::account(conn, e, "1010"));
    post(conn, &future_paid);
}

/// The dashboard queries the FULL month (2026-08-01 → 2026-08-31) with assets
/// as of "today" (2026-08-10 here).
#[test]
fn dashboard_numbers_hand_checked() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let e = create_entity(
        conn,
        &CreateEntity {
            name: "Probe".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("entity")
    .id;
    seed_august_ledger(conn, e);

    let s = dashboard_summary(
        conn,
        e,
        common::date("2026-08-01"),
        common::date("2026-08-31"),
        common::date("2026-08-10"),
    )
    .expect("summary");

    assert_eq!(s.income, 100_000, "income = salary only");
    assert_eq!(
        s.expenses, 14_753,
        "expenses = groceries 2500 + card 4000 + due bill 7253 + future paid 1000 (voided gone)"
    );
    assert_eq!(s.net_income, 85_247);
    assert_eq!(
        s.cash_like_assets, 97_500,
        "assets AS OF Aug 10 = 100000 salary − 2500 groceries; the Aug 20 payment must not count yet"
    );
    assert_eq!(
        s.recent_entry_count, 6,
        "salary + groceries + transfer + card + due bill + future expense (voided excluded)"
    );

    // The dashboard's Recent Activity list must show everything Transactions shows.
    let listed = list_entries(
        conn,
        e,
        &EntryFilter {
            date_from: Some(common::date("2026-08-01")),
            date_to: Some(common::date("2026-08-31")),
            ..EntryFilter::default()
        },
    )
    .expect("list");
    let visible = listed.iter().filter(|v| !v.is_voided).count();
    let all = list_entries(conn, e, &EntryFilter::default()).expect("all");
    let visible_all = all.iter().filter(|v| !v.is_voided).count();

    assert_eq!(
        visible, visible_all,
        "windowed dashboard list ({visible}) hides entries that Transactions shows ({visible_all})"
    );
    assert_eq!(visible, 6);
}

fn income_on(conn: &Connection, e: EntityId, date: &str, minor: i64) {
    let mut entry = base(e, Income, date, minor);
    entry.category_account_id = Some(common::account(conn, e, "4000"));
    entry.wallet_account_id = Some(common::account(conn, e, "1010"));
    post(conn, &entry);
}

fn expense_on(conn: &Connection, e: EntityId, date: &str, minor: i64) {
    let mut entry = base(e, Expense, date, minor);
    entry.category_account_id = Some(common::account(conn, e, "5100"));
    entry.wallet_account_id = Some(common::account(conn, e, "1010"));
    post(conn, &entry);
}

#[test]
fn arc_metrics_hand_checked() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let e = common::book(conn, "Probe", ChartTemplate::Personal);
    seed_august_ledger(conn, e);

    let s = dashboard_summary(
        conn,
        e,
        common::date("2026-08-01"),
        common::date("2026-08-31"),
        common::date("2026-08-10"),
    )
    .expect("summary");

    assert_eq!(
        s.savings_rate_bps,
        Some(8_525),
        "85 247 / 100 000 = 85.247 %"
    );
    assert_eq!(
        s.spend_ratio_bps,
        Some(1_475),
        "14 753 / 100 000 = 14.753 %"
    );
    let top = s.top_expense.expect("an expense account leads");
    assert_eq!(
        (
            top.code.as_str(),
            top.name.as_str(),
            top.amount_minor,
            top.share_bps
        ),
        ("5300", "Utilities", 7_253, 4_916),
        "the unpaid bill leads: 7 253 / 14 753 = 49.16 %"
    );
    assert_eq!(
        s.net_vs_previous_bps, None,
        "July is empty, so there is nothing to compare with"
    );
}

#[test]
fn net_vs_previous_compares_with_the_previous_calendar_month() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    let grew = common::book(conn, "Grew", ChartTemplate::Personal);
    income_on(conn, grew, "2026-07-10", 50_000);
    income_on(conn, grew, "2026-08-10", 60_000);

    let shrank = common::book(conn, "Shrank", ChartTemplate::Personal);
    income_on(conn, shrank, "2026-07-10", 50_000);
    income_on(conn, shrank, "2026-08-10", 40_000);

    let recovered = common::book(conn, "Recovered", ChartTemplate::Personal);
    expense_on(conn, recovered, "2026-07-10", 10_000);
    income_on(conn, recovered, "2026-08-10", 5_000);

    let change = |entity: EntityId| {
        dashboard_summary(
            conn,
            entity,
            common::date("2026-08-01"),
            common::date("2026-08-31"),
            common::date("2026-08-31"),
        )
        .expect("summary")
        .net_vs_previous_bps
    };
    assert_eq!(change(grew), Some(2_000));
    assert_eq!(change(shrank), Some(-2_000));
    assert_eq!(
        change(recovered),
        Some(15_000),
        "from -10 000 to +5 000 is a rise of 150 % of the previous net's size"
    );
}

#[test]
fn arc_metrics_are_empty_when_there_is_nothing_to_divide_by() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let summary = |entity: EntityId| {
        dashboard_summary(
            conn,
            entity,
            common::date("2026-08-01"),
            common::date("2026-08-31"),
            common::date("2026-08-31"),
        )
        .expect("summary")
    };

    let empty = summary(common::book(conn, "Empty", ChartTemplate::Personal));
    assert_eq!(
        (
            empty.savings_rate_bps,
            empty.spend_ratio_bps,
            empty.net_vs_previous_bps
        ),
        (None, None, None)
    );
    assert!(empty.top_expense.is_none());

    let spender = common::book(conn, "Spender", ChartTemplate::Personal);
    expense_on(conn, spender, "2026-08-05", 2_500);
    let spent = summary(spender);
    assert_eq!(
        (spent.savings_rate_bps, spent.spend_ratio_bps),
        (None, None),
        "no income to divide by"
    );
    assert_eq!(spent.top_expense.expect("top").share_bps, 10_000);

    let earner = common::book(conn, "Earner", ChartTemplate::Personal);
    income_on(conn, earner, "2026-08-03", 1_000);
    let earned = summary(earner);
    assert_eq!(
        (earned.savings_rate_bps, earned.spend_ratio_bps),
        (Some(10_000), Some(0))
    );
    assert!(earned.top_expense.is_none(), "no expenses, no top spend");
}

#[test]
fn previous_window_steps_back_by_calendar_months_or_by_days() {
    let window = |from: &str, to: &str| {
        previous_window(parse_date(from).expect("from"), parse_date(to).expect("to"))
            .map(|(start, end)| (format_date(start), format_date(end)))
    };
    let expect = |from: &str, to: &str| Some((from.to_owned(), to.to_owned()));

    assert_eq!(
        window("2026-09-01", "2026-09-30"),
        expect("2026-08-01", "2026-08-31"),
        "a month"
    );
    assert_eq!(
        window("2026-07-01", "2026-09-30"),
        expect("2026-04-01", "2026-06-30"),
        "a quarter"
    );
    assert_eq!(
        window("2026-01-01", "2026-12-31"),
        expect("2025-01-01", "2025-12-31"),
        "a year"
    );
    assert_eq!(
        window("2026-01-01", "2026-01-31"),
        expect("2025-12-01", "2025-12-31"),
        "across new year"
    );
    assert_eq!(
        window("2028-03-01", "2028-03-31"),
        expect("2028-02-01", "2028-02-29"),
        "into a leap February"
    );
    assert_eq!(
        window("2026-08-05", "2026-08-14"),
        expect("2026-07-26", "2026-08-04"),
        "ten days"
    );
    assert_eq!(
        window("2026-08-10", "2026-08-10"),
        expect("2026-08-09", "2026-08-09"),
        "one day"
    );
    assert_eq!(
        window("2026-03-01", "2026-04-15"),
        expect("2026-01-14", "2026-02-28"),
        "not whole months"
    );
}
