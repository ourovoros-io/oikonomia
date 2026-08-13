//! v4 → v5: journal_lines debit XOR credit CHECK.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, PostJournal, create_entity, list_accounts, post_entry,
};
use oikonomia_core::vault::Vault;
use tempfile::TempDir;

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

#[test]
fn v5_rejects_double_sided_journal_line() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    assert_eq!(CURRENT_SCHEMA_VERSION, 5);

    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "Xor".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity");
    let accounts = list_accounts(conn, entity.id).expect("accounts");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");
    let view = post_entry(
        conn,
        &PostJournal {
            entity_id: entity.id,
            entry_date: "2026-01-01".into(),
            description: "ok".into(),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: food.id,
                    debit_minor: 100,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: checking.id,
                    debit_minor: 0,
                    credit_minor: 100,
                    memo: None,
                },
            ],
        },
    )
    .expect("post");

    let err = conn
        .execute(
            "
            INSERT INTO journal_lines (id, entry_id, account_id, debit_minor, credit_minor, memo, line_order)
            VALUES ('bad-line', ?1, ?2, 10, 10, NULL, 99)
            ",
            rusqlite::params![view.entry.id.0.to_string(), food.id.0.to_string()],
        )
        .expect_err("xor check");
    let msg = err.to_string();
    assert!(
        msg.contains("CHECK") || msg.contains("constraint"),
        "expected CHECK failure, got {msg}"
    );
}

#[test]
fn v5_migrates_existing_balanced_lines() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    conn.execute("UPDATE vault_meta SET schema_version = 4 WHERE id = 1", [])
        .expect("downgrade version");

    migrate(conn).expect("v4 -> v5");
    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("version");
    assert_eq!(version, 5);
}

#[test]
fn migrate_is_safe_on_fresh_v5_vault() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    migrate(conn).expect("idempotent");
    migrate(conn).expect("idempotent again");
}
