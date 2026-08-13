//! Regression tests for report filtering (review finding F1).
//!
//! Reports must only aggregate posted, non-voided entries inside the
//! requested date window; the dashboard and the report pages must agree.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, PostJournal, balance_sheet, create_entity, list_accounts,
    post_entry, profit_and_loss, trial_balance, void_entry,
};
use oikonomia_core::vault::Vault;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rusqlite::Connection;
use tempfile::TempDir;

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

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
    )
    .expect("entity")
    .id
}

fn post_expense(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    minor: i64,
) -> oikonomia_core::domain::JournalEntryId {
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");

    let view = post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: date.into(),
            description: format!("spend {minor} on {date}"),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: food.id,
                    debit_minor: minor,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: checking.id,
                    debit_minor: 0,
                    credit_minor: minor,
                    memo: None,
                },
            ],
        },
    )
    .expect("post");
    view.entry.id
}

#[test]
fn pnl_respects_date_range() {
    let (_dir, vault) = {
        let (dir, vault) = setup_vault();
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
    let (_dir, vault) = setup_vault();
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
    let (_dir, vault) = setup_vault();
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

#[test]
fn balance_sheet_balances_after_fiscal_year_boundary() {
    let (_dir, vault) = setup_vault();
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
    let (_dir, vault) = setup_vault();
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
}

#[test]
fn voided_entry_leaves_no_trace_in_trial_balance() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry_id = post_expense(conn, entity_id, "2026-01-15", 1_000);
    void_entry(conn, entry_id).expect("void");

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
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let accounts = list_accounts(conn, entity_id).expect("accounts");

    let mut rng = StdRng::seed_from_u64(0x0110_2026);
    for _ in 0..50 {
        let debit_idx = rng.gen_range(0..accounts.len());
        let credit_idx = (debit_idx + rng.gen_range(1..accounts.len())) % accounts.len();
        let minor = rng.gen_range(1..=100_000);
        let month = rng.gen_range(1..=12);
        let day = rng.gen_range(1..=28);

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
}
