//! Regression tests for the document flow (review finding F3).

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::documents::{
    DocumentId, MAX_DOCUMENT_BYTES, NewDocument, attach_document, delete_document, get_document,
    list_documents, post_simple_entry_with_document, save_analysis_json,
    suggest_accounts_for_entity,
};
use oikonomia_core::domain::{ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::error::{Error, Resource, ValidationError};
use oikonomia_core::ledger::{
    EntryFilter, JournalLineRequest, PostJournal, PostJournalRequest, PostSimpleEntry,
    PostedEntryView, archive_account, delete_entity, list_accounts, list_entities, list_entries,
    post_entry,
};
use rusqlite::Connection;

/// Post a trivial balanced expense entry so a document has something to
/// attach to.
fn post_expense_entry(
    conn: &Connection,
    entity_id: EntityId,
    description: &str,
) -> oikonomia_core::ledger::PostedEntryView {
    let entry = PostJournal {
        description: description.into(),
        ..common::two_line(conn, entity_id, "2026-03-01", ("5100", "1010"), 500)
    };

    post_entry(conn, &entry).expect("post")
}

/// Post a trivial balanced entry so a document has something to link to.
fn post_reference_entry(conn: &Connection, entity_id: EntityId) -> PostedEntryView {
    post_expense_entry(conn, entity_id, "Reference entry")
}

#[test]
fn delete_entity_with_linked_document() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);

    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");

    let entry = post_entry(
        conn,
        &common::strict(PostJournalRequest {
            entity_id,
            entry_date: "2026-01-15".into(),
            description: "Groceries".into(),
            reference: None,
            lines: vec![
                JournalLineRequest {
                    account_id: food.id,
                    debit_minor: 1_000,
                    credit_minor: 0,
                    memo: None,
                },
                JournalLineRequest {
                    account_id: checking.id,
                    debit_minor: 0,
                    credit_minor: 1_000,
                    memo: None,
                },
            ],
        }),
    )
    .expect("post");

    attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "receipt.txt",
            mime_type: "text/plain",
            data: b"TOTAL 10,00 EUR",
        },
    )
    .expect("save document");

    delete_entity(conn, entity_id).expect("delete entity with linked document");
    assert_eq!(
        list_entities(conn).expect("list"),
        [] as [oikonomia_core::domain::Entity; 0]
    );
}

#[test]
fn attach_document_rejects_unsupported_empty_and_oversize_files() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);
    let entry = post_reference_entry(conn, entity_id);

    let exe = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "evil.exe",
            mime_type: "application/x-msdownload",
            data: b"MZ",
        },
    );
    assert_eq!(
        exe.expect_err("executables must be rejected"),
        Error::Validation(ValidationError::FileTypeUnsupported)
    );

    let empty = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "empty.txt",
            mime_type: "text/plain",
            data: b"",
        },
    );
    assert_eq!(
        empty.expect_err("empty files must be rejected"),
        Error::Validation(ValidationError::FileEmpty)
    );

    let one_byte_over = vec![0_u8; MAX_DOCUMENT_BYTES + 1];
    let oversize = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "huge.pdf",
            mime_type: "application/pdf",
            data: &one_byte_over,
        },
    );
    let megabyte = 1024 * 1024;
    assert_eq!(
        oversize.expect_err("a file over the limit must be rejected"),
        Error::Validation(ValidationError::FileTooLarge {
            max_mb: u64::try_from(MAX_DOCUMENT_BYTES / megabyte).unwrap()
        })
    );

    let at_the_limit = vec![0_u8; MAX_DOCUMENT_BYTES];
    attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "largest.pdf",
            mime_type: "application/pdf",
            data: &at_the_limit,
        },
    )
    .expect("a file of exactly the limit is stored");
}

#[test]
fn attach_document_rejects_duplicate_name_in_book() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);
    let entry = post_reference_entry(conn, entity_id);

    attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "invoice.pdf",
            mime_type: "application/pdf",
            data: b"%PDF-1.4",
        },
    )
    .expect("first save");
    let dup = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "invoice.pdf",
            mime_type: "application/pdf",
            data: b"%PDF-1.4",
        },
    );
    assert_eq!(
        dup.expect_err("same name in the same book must be rejected"),
        Error::Validation(ValidationError::NameTaken {
            name: "invoice.pdf".into()
        })
    );
}

#[test]
fn list_get_delete_round_trip() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);
    let entry = post_reference_entry(conn, entity_id);

    let a = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "a.txt",
            mime_type: "text/plain",
            data: b"alpha",
        },
    )
    .expect("save a");
    let b = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "b.txt",
            mime_type: "text/plain",
            data: b"bravo",
        },
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
    assert_eq!(
        get_document(conn, a.id).expect_err("deleted document is gone"),
        Error::NotFound(Resource::Document)
    );
    assert_eq!(list_documents(conn, entity_id).expect("list").len(), 1);
}

#[test]
fn delete_missing_document_returns_not_found() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    common::book(conn, "Docs", ChartTemplate::Personal);

    let missing = DocumentId::generate();
    assert_eq!(
        delete_document(conn, missing),
        Err(Error::NotFound(Resource::Document))
    );
}

#[test]
fn saving_analysis_for_a_missing_document_returns_not_found() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    common::book(conn, "Docs", ChartTemplate::Personal);

    let missing = DocumentId::generate();

    assert_eq!(
        save_analysis_json(conn, missing, "{}"),
        Err(Error::NotFound(Resource::Document))
    );
    assert_eq!(
        delete_document(conn, missing),
        Err(Error::NotFound(Resource::Document)),
        "the same unknown id gets the same answer from delete"
    );
}

#[test]
fn saving_analysis_for_a_stored_document_succeeds() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);
    let entry = post_expense_entry(conn, entity_id, "Groceries");
    let meta = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "receipt.txt",
            mime_type: "text/plain",
            data: b"TOTAL 5,00",
        },
    )
    .expect("document");

    assert_eq!(
        save_analysis_json(conn, meta.id, "{\"kind\":\"expense\"}"),
        Ok(())
    );
}

#[test]
fn an_archived_account_is_not_offered_for_matching() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);

    let offered = suggest_accounts_for_entity(conn, entity_id).expect("accounts");
    let archived = offered
        .iter()
        .find(|account| !account.is_system)
        .expect("a seeded chart has accounts that can be archived")
        .clone();
    archive_account(conn, archived.id).expect("archive");

    let offered = suggest_accounts_for_entity(conn, entity_id).expect("accounts");

    assert!(
        offered.iter().all(|account| account.is_active),
        "only active accounts are offered"
    );
    assert!(offered.iter().all(|account| account.id != archived.id));
    assert!(
        list_accounts(conn, entity_id)
            .expect("all accounts")
            .iter()
            .any(|account| account.id == archived.id),
        "the archived account still exists"
    );
}

#[test]
fn list_documents_is_newest_first() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);
    let entry = post_reference_entry(conn, entity_id);

    let first = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "first.txt",
            mime_type: "text/plain",
            data: b"1",
        },
    )
    .expect("save");
    let second = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "second.txt",
            mime_type: "text/plain",
            data: b"2",
        },
    )
    .expect("save");

    let listed = list_documents(conn, entity_id).expect("list");
    assert_eq!(listed[0].id, second.id, "most recent save first");
    assert_eq!(listed[1].id, first.id);
    assert_eq!(
        listed[0].entry_description, "Reference entry",
        "list must carry the linked entry description"
    );
}

#[test]
fn attach_document_saves_links_and_skips_analysis() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);
    let entry = post_expense_entry(conn, entity_id, "Lunch invoice");

    let meta = attach_document(
        conn,
        entity_id,
        entry.entry.id,
        &NewDocument {
            filename: "inv.txt",
            mime_type: "text/plain",
            data: b"total 5",
        },
    )
    .expect("attach");
    assert_eq!(meta.entry_id, entry.entry.id);

    let listed = list_documents(conn, entity_id).expect("list");
    assert_eq!(listed[0].entry_id, entry.entry.id, "linked in one step");

    let analysis: Option<String> = conn
        .query_row(
            "SELECT analysis_json FROM documents WHERE id = ?1",
            [meta.id.to_string()],
            |row| row.get(0),
        )
        .expect("row");
    assert!(analysis.is_none(), "attach must not run analysis");
}

#[test]
fn attach_document_rejects_missing_and_wrong_entity_entry() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_a = common::book(conn, "Docs", ChartTemplate::Personal);
    let entity_b = common::book(conn, "Other", ChartTemplate::Personal);

    let missing = JournalEntryId::generate();
    let no_entry = attach_document(
        conn,
        entity_a,
        missing,
        &NewDocument {
            filename: "a.txt",
            mime_type: "text/plain",
            data: b"data",
        },
    );
    assert!(
        matches!(no_entry, Err(Error::NotFound(_))),
        "missing entry must fail: {no_entry:?}"
    );

    let entry_b = post_expense_entry(conn, entity_b, "Other book expense");
    let foreign_entry = attach_document(
        conn,
        entity_a,
        entry_b.entry.id,
        &NewDocument {
            filename: "b.txt",
            mime_type: "text/plain",
            data: b"data",
        },
    );
    assert_eq!(
        foreign_entry.expect_err("entry from a different book must fail"),
        Error::Validation(ValidationError::WrongBook)
    );

    // The entry is checked before the file: a file that would itself be
    // refused does not hide that the entry is in another book.
    let empty_file = attach_document(
        conn,
        entity_a,
        entry_b.entry.id,
        &NewDocument {
            filename: "cross.exe",
            mime_type: "",
            data: b"",
        },
    );
    assert_eq!(
        empty_file.expect_err("a foreign entry must fail whatever the file"),
        Error::Validation(ValidationError::WrongBook)
    );
}

fn simple_expense_input(
    conn: &Connection,
    entity_id: EntityId,
    description: &str,
) -> PostSimpleEntry {
    let food = common::account(conn, entity_id, "5100");
    let checking = common::account(conn, entity_id, "1010");

    PostSimpleEntry {
        description: description.into(),
        ..common::simple_expense(entity_id, food, checking, "2026-03-01", 1_000)
    }
}

#[test]
fn post_with_document_is_atomic_and_stores_analysis() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);

    let input = simple_expense_input(conn, entity_id, "Scanned groceries");
    let (view, meta) = post_simple_entry_with_document(
        conn,
        &input,
        &NewDocument {
            filename: "receipt.txt",
            mime_type: "text/plain",
            data: b"TOTAL 10,00",
        },
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
            [meta.id.to_string()],
            |row| row.get(0),
        )
        .expect("row");
    assert_eq!(analysis.as_deref(), Some("{\"notes\":\"scan\"}"));
}

#[test]
fn post_with_document_name_clash_rolls_back_the_entry() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);

    let first = simple_expense_input(conn, entity_id, "First");
    post_simple_entry_with_document(
        conn,
        &first,
        &NewDocument {
            filename: "bill.txt",
            mime_type: "text/plain",
            data: b"a",
        },
        None,
    )
    .expect("first post");

    let before = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .len();

    let second = simple_expense_input(conn, entity_id, "Second");
    let clash = post_simple_entry_with_document(
        conn,
        &second,
        &NewDocument {
            filename: "bill.txt",
            mime_type: "text/plain",
            data: b"b",
        },
        None,
    );
    assert_eq!(
        clash.expect_err("duplicate name must fail"),
        Error::Validation(ValidationError::NameTaken {
            name: "bill.txt".into()
        })
    );

    let after = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .len();
    assert_eq!(after, before, "the entry must roll back with the document");
}

#[test]
fn post_with_document_invalid_file_rolls_back_everything() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Docs", ChartTemplate::Personal);

    let input = simple_expense_input(conn, entity_id, "Bad file");
    let result = post_simple_entry_with_document(
        conn,
        &input,
        &NewDocument {
            filename: "evil.exe",
            mime_type: "application/x-msdownload",
            data: b"MZ",
        },
        None,
    );
    assert_eq!(
        result.expect_err("unsupported file must fail"),
        Error::Validation(ValidationError::FileTypeUnsupported)
    );

    let entries = list_entries(conn, entity_id, &EntryFilter::default()).expect("list");
    assert!(
        entries.is_empty(),
        "no entry may survive a failed document save"
    );
    assert!(list_documents(conn, entity_id).expect("docs").is_empty());
}
