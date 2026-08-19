//! v5 → v6: per-entry `hidden` flag on `journal_entries`.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, PostJournal, create_entity, list_accounts, post_entry,
};
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn hidden_of(conn: &Connection, entry_id: &str) -> i64 {
    conn.query_row(
        "SELECT hidden FROM journal_entries WHERE id = ?1",
        [entry_id],
        |row| row.get(0),
    )
    .expect("hidden")
}

#[test]
fn pre_v6_row_becomes_visible() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    assert_eq!(CURRENT_SCHEMA_VERSION, 6);

    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "PreV6".into(),
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
            description: "legacy".into(),
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
    let entry_id = view.entry.id.0.to_string();

    conn.execute("ALTER TABLE journal_entries DROP COLUMN hidden", [])
        .expect("pre-v6 shape");
    conn.execute("UPDATE vault_meta SET schema_version = 5 WHERE id = 1", [])
        .expect("mark v5");

    migrate(conn).expect("v5 -> v6");

    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("version");
    assert_eq!(version, 6);
    assert_eq!(
        hidden_of(conn, &entry_id),
        0,
        "existing row defaults visible"
    );
}

#[test]
fn migrate_is_safe_on_fresh_v6_vault() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    migrate(conn).expect("idempotent");
    migrate(conn).expect("idempotent again");
    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("version");
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
}
