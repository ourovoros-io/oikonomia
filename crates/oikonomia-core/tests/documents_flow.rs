//! Regression tests for the document flow (review finding F3).

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::documents::{
    DocumentId, attach_document, delete_document, get_document, list_documents,
    post_simple_entry_with_document, save_document,
};
use oikonomia_core::domain::{ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, EntryFilter, PostJournal, PostSimpleEntry, PostedEntryView,
    SimpleEntryKind, create_entity, delete_entity, list_accounts, list_entities, list_entries,
    post_entry,
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
    setup_named_entity(conn, "Docs")
}

fn setup_named_entity(conn: &Connection, name: &str) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: name.into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity")
    .id
}

/// Post a trivial balanced expense entry so a document has something to
/// attach to.
fn post_expense_entry(
    conn: &Connection,
    entity_id: EntityId,
    description: &str,
) -> oikonomia_core::ledger::PostedEntryView {
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");

    post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: "2026-03-01".into(),
            description: description.into(),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: food.id,
                    debit_minor: 500,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: checking.id,
                    debit_minor: 0,
                    credit_minor: 500,
                    memo: None,
                },
            ],
        },
    )
    .expect("post")
}

/// Post a trivial balanced entry so a document has something to link to.
fn post_reference_entry(conn: &Connection, entity_id: EntityId) -> PostedEntryView {
    post_expense_entry(conn, entity_id, "Reference entry")
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

    save_document(
        conn,
        entity_id,
        entry.entry.id,
        "receipt.txt",
        "text/plain",
        b"TOTAL 10,00 EUR",
    )
    .expect("save document");

    delete_entity(conn, entity_id).expect("delete entity with linked document");
    assert!(list_entities(conn).expect("list").is_empty());
}

#[test]
fn save_document_rejects_unsupported_and_oversize() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry = post_reference_entry(conn, entity_id);

    let exe = save_document(
        conn,
        entity_id,
        entry.entry.id,
        "evil.exe",
        "application/x-msdownload",
        b"MZ",
    );
    assert!(exe.is_err(), "executables must be rejected");

    let empty = save_document(
        conn,
        entity_id,
        entry.entry.id,
        "empty.txt",
        "text/plain",
        b"",
    );
    assert!(empty.is_err(), "empty files must be rejected");
}

#[test]
fn save_document_rejects_duplicate_name_in_book() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry = post_reference_entry(conn, entity_id);

    save_document(
        conn,
        entity_id,
        entry.entry.id,
        "invoice.pdf",
        "application/pdf",
        b"%PDF-1.4",
    )
    .expect("first save");
    let dup = save_document(
        conn,
        entity_id,
        entry.entry.id,
        "invoice.pdf",
        "application/pdf",
        b"%PDF-1.4",
    );
    assert!(dup.is_err(), "same name in the same book must be rejected");
}

#[test]
fn list_get_delete_round_trip() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry = post_reference_entry(conn, entity_id);

    let a = save_document(
        conn,
        entity_id,
        entry.entry.id,
        "a.txt",
        "text/plain",
        b"alpha",
    )
    .expect("save a");
    let b = save_document(
        conn,
        entity_id,
        entry.entry.id,
        "b.txt",
        "text/plain",
        b"bravo",
    )
    .expect("save b");

    let listed = list_documents(conn, entity_id).expect("list");
    assert_eq!(listed.len(), 2, "both documents listed");
    assert!(
        listed.iter().all(|m| !m.created_at.is_empty()),
        "created_at populated"
    );
    assert!(listed.iter().any(|m| m.id == a.id && m.filename == "a.txt"));

    let (meta, data) = get_document(conn, b.id).expect("get b");
    assert_eq!(meta.filename, "b.txt");
    assert_eq!(data, b"bravo");

    delete_document(conn, a.id).expect("delete a");
    assert!(
        get_document(conn, a.id).is_err(),
        "deleted document is gone"
    );
    assert_eq!(list_documents(conn, entity_id).expect("list").len(), 1);
}

#[test]
fn delete_missing_document_returns_not_found() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    setup_entity(conn);

    let missing = DocumentId::new();
    assert!(delete_document(conn, missing).is_err());
}

#[test]
fn list_documents_is_newest_first() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry = post_reference_entry(conn, entity_id);

    let first = save_document(
        conn,
        entity_id,
        entry.entry.id,
        "first.txt",
        "text/plain",
        b"1",
    )
    .expect("save");
    let second = save_document(
        conn,
        entity_id,
        entry.entry.id,
        "second.txt",
        "text/plain",
        b"2",
    )
    .expect("save");

    let listed = list_documents(conn, entity_id).expect("list");
    assert_eq!(listed[0].id, second.id, "most recent save first");
    assert_eq!(listed[1].id, first.id);
}

#[test]
fn attach_document_saves_links_and_skips_analysis() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry = post_expense_entry(conn, entity_id, "Lunch invoice");

    let meta = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        "inv.txt",
        "text/plain",
        b"total 5",
    )
    .expect("attach");
    assert_eq!(meta.entry_id, entry.entry.id);

    let listed = list_documents(conn, entity_id).expect("list");
    assert_eq!(listed[0].entry_id, entry.entry.id, "linked in one step");

    let analysis: Option<String> = conn
        .query_row(
            "SELECT analysis_json FROM documents WHERE id = ?1",
            [meta.id.0.to_string()],
            |row| row.get(0),
        )
        .expect("row");
    assert!(analysis.is_none(), "attach must not run analysis");
}

#[test]
fn attach_document_rejects_missing_and_wrong_entity_entry() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_a = setup_entity(conn);
    let entity_b = setup_named_entity(conn, "Other");

    let missing = JournalEntryId::new();
    assert!(
        attach_document(conn, entity_a, missing, "a.txt", "text/plain", b"data").is_err(),
        "missing entry must fail"
    );

    let entry_b = post_expense_entry(conn, entity_b, "Other book expense");
    assert!(
        attach_document(
            conn,
            entity_a,
            entry_b.entry.id,
            "b.txt",
            "text/plain",
            b"data"
        )
        .is_err(),
        "entry from a different book must fail"
    );
}

fn simple_expense_input(
    conn: &Connection,
    entity_id: EntityId,
    description: &str,
) -> PostSimpleEntry {
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    PostSimpleEntry {
        entity_id,
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        entry_date: "2026-03-01".into(),
        description: description.into(),
        reference: None,
        amount_minor: 1_000,
        category_account_id: Some(food.id),
        wallet_account_id: Some(checking.id),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    }
}

#[test]
fn post_with_document_is_atomic_and_stores_analysis() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let input = simple_expense_input(conn, entity_id, "Scanned groceries");
    let (view, meta) = post_simple_entry_with_document(
        conn,
        &input,
        "receipt.txt",
        "text/plain",
        b"TOTAL 10,00",
        Some("{\"notes\":\"scan\"}"),
    )
    .expect("post with document");

    assert_eq!(
        meta.entry_id, view.entry.id,
        "document linked to the new entry"
    );

    let analysis: Option<String> = conn
        .query_row(
            "SELECT analysis_json FROM documents WHERE id = ?1",
            [meta.id.0.to_string()],
            |row| row.get(0),
        )
        .expect("row");
    assert_eq!(analysis.as_deref(), Some("{\"notes\":\"scan\"}"));
}

#[test]
fn post_with_document_name_clash_rolls_back_the_entry() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let first = simple_expense_input(conn, entity_id, "First");
    post_simple_entry_with_document(conn, &first, "bill.txt", "text/plain", b"a", None)
        .expect("first post");

    let before = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .len();

    let second = simple_expense_input(conn, entity_id, "Second");
    let clash =
        post_simple_entry_with_document(conn, &second, "bill.txt", "text/plain", b"b", None);
    assert!(clash.is_err(), "duplicate name must fail");

    let after = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .len();
    assert_eq!(after, before, "the entry must roll back with the document");
}

#[test]
fn post_with_document_invalid_file_rolls_back_everything() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let input = simple_expense_input(conn, entity_id, "Bad file");
    let result = post_simple_entry_with_document(
        conn,
        &input,
        "evil.exe",
        "application/x-msdownload",
        b"MZ",
        None,
    );
    assert!(result.is_err(), "unsupported file must fail");

    let entries = list_entries(conn, entity_id, &EntryFilter::default()).expect("list");
    assert!(
        entries.is_empty(),
        "no entry may survive a failed document save"
    );
    assert!(list_documents(conn, entity_id).expect("docs").is_empty());
}
