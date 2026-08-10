//! Dashboard figures, hand-verified: full-month range for activity numbers,
//! assets pinned to "today", voids and window edges handled.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::ledger::SimpleEntryKind::{Bill, Expense, Income, Transfer};
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntry, SimpleBillStatus, SimpleEntryKind, create_entity,
    dashboard_summary, list_accounts, list_entries, post_simple_entry, void_entry,
};
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

fn setup() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn account(conn: &Connection, entity_id: EntityId, code: &str) -> AccountId {
    list_accounts(conn, entity_id)
        .expect("accounts")
        .iter()
        .find(|a| a.code == code)
        .map(|a| a.id)
        .expect(code)
}

fn base(entity_id: EntityId, kind: SimpleEntryKind, date: &str, minor: i64) -> PostSimpleEntry {
    PostSimpleEntry {
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

fn post(conn: &Connection, input: &PostSimpleEntry) -> JournalEntryId {
    post_simple_entry(conn, input)
        .expect("post simple")
        .entry
        .id
}

/// August 2026 ledger: salary, groceries, a transfer, a card expense, a voided
/// entry, an OCR-style bill dated at its FUTURE due date, and a future paid
/// expense.
fn seed_august_ledger(conn: &Connection, e: EntityId) {
    let mut salary = base(e, Income, "2026-08-03", 100_000);
    salary.category_account_id = Some(account(conn, e, "4000"));
    salary.wallet_account_id = Some(account(conn, e, "1010"));
    post(conn, &salary);

    let mut groceries = base(e, Expense, "2026-08-05", 2_500);
    groceries.category_account_id = Some(account(conn, e, "5100"));
    groceries.wallet_account_id = Some(account(conn, e, "1010"));
    post(conn, &groceries);

    // Checking → savings: must not touch income/expenses.
    let mut move_savings = base(e, Transfer, "2026-08-06", 30_000);
    move_savings.from_account_id = Some(account(conn, e, "1010"));
    move_savings.to_account_id = Some(account(conn, e, "1020"));
    post(conn, &move_savings);

    // On the credit card: expense counts, assets untouched.
    let mut card = base(e, Expense, "2026-08-07", 4_000);
    card.category_account_id = Some(account(conn, e, "5600"));
    card.wallet_account_id = Some(account(conn, e, "2000"));
    post(conn, &card);

    // Voided: must vanish everywhere.
    let mut mistake = base(e, Expense, "2026-08-08", 9_999);
    mistake.category_account_id = Some(account(conn, e, "5100"));
    mistake.wallet_account_id = Some(account(conn, e, "1010"));
    let voided = post(conn, &mistake);
    void_entry(conn, voided).expect("void");

    // OCR-style bill posted with a future due date (Aug 13 > "today" Aug 10).
    let mut due_bill = base(e, Bill, "2026-08-13", 7_253);
    due_bill.bill_status = Some(SimpleBillStatus::Unpaid);
    due_bill.category_account_id = Some(account(conn, e, "5300"));
    due_bill.payable_account_id = Some(account(conn, e, "2050"));
    post(conn, &due_bill);

    // Future-dated expense PAID from checking: counts for the month, but must
    // not move "assets as of today".
    let mut future_paid = base(e, Expense, "2026-08-20", 1_000);
    future_paid.category_account_id = Some(account(conn, e, "5900"));
    future_paid.wallet_account_id = Some(account(conn, e, "1010"));
    post(conn, &future_paid);
}

/// The dashboard queries the FULL month (2026-08-01 → 2026-08-31) with assets
/// as of "today" (2026-08-10 here).
#[test]
fn dashboard_numbers_hand_checked() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = create_entity(
        conn,
        &CreateEntity {
            name: "Probe".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity")
    .id;
    seed_august_ledger(conn, e);

    let s = dashboard_summary(conn, e, "2026-08-01", "2026-08-31", "2026-08-10").expect("summary");

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
            date_from: Some("2026-08-01".into()),
            date_to: Some("2026-08-31".into()),
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
