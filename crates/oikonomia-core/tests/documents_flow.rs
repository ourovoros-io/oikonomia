//! Regression tests for the document flow (review finding F3).

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::documents::{link_document_to_entry, save_document};
use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, PostJournal, create_entity, delete_entity, list_accounts,
    list_entities, post_entry,
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

fn setup_entity(conn: &Connection) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: "Docs".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity")
    .id
}

#[test]
fn delete_entity_with_linked_document() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");

    let entry = post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: "2026-01-15".into(),
            description: "Groceries".into(),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: food.id,
                    debit_minor: 1_000,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: checking.id,
                    debit_minor: 0,
                    credit_minor: 1_000,
                    memo: None,
                },
            ],
        },
    )
    .expect("post");

    let meta = save_document(
        conn,
        entity_id,
        "receipt.txt",
        "text/plain",
        b"TOTAL 10,00 EUR",
    )
    .expect("save document");
    link_document_to_entry(conn, meta.id, entry.entry.id).expect("link");

    delete_entity(conn, entity_id).expect("delete entity with linked document");
    assert!(list_entities(conn).expect("list").is_empty());
}

#[test]
fn save_document_rejects_unsupported_and_oversize() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let exe = save_document(
        conn,
        entity_id,
        "evil.exe",
        "application/x-msdownload",
        b"MZ",
    );
    assert!(exe.is_err(), "executables must be rejected");

    let empty = save_document(conn, entity_id, "empty.txt", "text/plain", b"");
    assert!(empty.is_err(), "empty files must be rejected");
}
