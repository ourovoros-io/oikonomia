//! v4 → v5: `journal_lines` debit XOR credit CHECK.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::error::Error;
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
    assert_eq!(CURRENT_SCHEMA_VERSION, 7);

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
    conn.execute("ALTER TABLE journal_entries DROP COLUMN hidden", [])
        .expect("pre-v6 shape");
    conn.execute("UPDATE vault_meta SET schema_version = 4 WHERE id = 1", [])
        .expect("downgrade version");

    migrate(conn).expect("v4 -> current");
    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("version");
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn v5_migrate_aborts_on_xor_violating_v4_rows() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "BadV4".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity");
    let accounts = list_accounts(conn, entity.id).expect("accounts");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
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

    conn.execute_batch(
        "
        PRAGMA foreign_keys = OFF;
        CREATE TABLE journal_lines_v4 (
            id TEXT PRIMARY KEY NOT NULL,
            entry_id TEXT NOT NULL,
            account_id TEXT NOT NULL,
            debit_minor INTEGER NOT NULL DEFAULT 0 CHECK (debit_minor >= 0),
            credit_minor INTEGER NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
            memo TEXT,
            line_order INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO journal_lines_v4
            SELECT id, entry_id, account_id, debit_minor, credit_minor, memo, line_order
            FROM journal_lines;
        DROP TABLE journal_lines;
        ALTER TABLE journal_lines_v4 RENAME TO journal_lines;
        PRAGMA foreign_keys = ON;
        ",
    )
    .expect("downgrade lines to v4 shape");

    conn.execute(
        "
        INSERT INTO journal_lines (id, entry_id, account_id, debit_minor, credit_minor, memo, line_order)
        VALUES ('bad-v4', ?1, ?2, 10, 10, NULL, 99)
        ",
        rusqlite::params![view.entry.id.0.to_string(), food.id.0.to_string()],
    )
    .expect("insert both-sided v4 line");
    conn.execute("UPDATE vault_meta SET schema_version = 4 WHERE id = 1", [])
        .expect("mark v4");

    let err = migrate(conn).expect_err("xor rows must abort");
    assert!(
        matches!(err, Error::VaultCorrupt(ref msg) if msg.contains("1 journal line")),
        "{err:?}"
    );
}

#[test]
fn migrate_is_safe_on_fresh_v5_vault() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    migrate(conn).expect("idempotent");
    migrate(conn).expect("idempotent again");
}
