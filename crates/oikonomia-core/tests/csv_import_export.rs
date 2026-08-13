//! Bank CSV import + journal CSV export against an encrypted vault.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::csv::{
    CsvImportAccounts, JournalCsvStatus, export_journal_csv, parse_journal_export,
    post_import_rows, preview_bank_csv, preview_bank_csv_file,
};
use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, validate_lines_for_post};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntry, SimpleEntryKind, create_entity, list_accounts,
    list_entries, post_simple_entry, void_entry,
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

struct Accounts {
    checking: AccountId,
    food: AccountId,
    salary: AccountId,
}

fn entity_with_accounts(conn: &Connection) -> (EntityId, Accounts) {
    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "CSV Books".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity");
    let accounts = list_accounts(conn, entity.id).expect("accounts");
    let by_code = |code: &str| {
        accounts
            .iter()
            .find(|a| a.code == code)
            .map(|a| a.id)
            .expect(code)
    };
    (
        entity.id,
        Accounts {
            checking: by_code("1010"),
            food: by_code("5100"),
            salary: by_code("4000"),
        },
    )
}

fn roles(acc: &Accounts) -> CsvImportAccounts {
    CsvImportAccounts {
        wallet_account_id: Some(acc.checking),
        expense_account_id: Some(acc.food),
        income_account_id: Some(acc.salary),
    }
}

fn grocery_csv() -> &'static str {
    "Date,Description,Amount\n2026-03-15,Groceries,-25.00\n"
}

fn count_entries(conn: &Connection, entity_id: EntityId) -> usize {
    list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .len()
}

#[test]
fn preview_does_not_post() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let before = count_entries(conn, entity_id);
    let preview = preview_bank_csv(conn, entity_id, roles(&acc), grocery_csv()).expect("preview");
    assert_eq!(preview.rows.len(), 1);
    assert!(!preview.rows[0].duplicate);
    assert!(preview.rows[0].error.is_none());
    assert_eq!(
        preview.rows[0].suggested.as_ref().map(|s| s.kind),
        Some(SimpleEntryKind::Expense)
    );
    assert_eq!(count_entries(conn, entity_id), before);
}

#[test]
fn post_selected_rows_are_balanced() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let csv = "Date,Description,Amount\n2026-03-15,Groceries,-25.00\n2026-03-16,Salary,1000.00\n";
    let preview = preview_bank_csv(conn, entity_id, roles(&acc), csv).expect("preview");
    let rows: Vec<PostSimpleEntry> = preview
        .rows
        .iter()
        .filter_map(|r| r.suggested.clone())
        .collect();
    assert_eq!(rows.len(), 2);

    let result = post_import_rows(conn, &rows, false).expect("post");
    assert_eq!(result.posted.len(), 2);
    assert_eq!(result.skipped_duplicate_count, 0);
    assert_eq!(count_entries(conn, entity_id), 2);

    for view in &result.posted {
        validate_lines_for_post(&view.lines).expect("balanced");
        let debits: i64 = view.lines.iter().map(|l| l.debit.amount_minor()).sum();
        let credits: i64 = view.lines.iter().map(|l| l.credit.amount_minor()).sum();
        assert_eq!(debits, credits);
        assert!(debits > 0);
    }

    assert_eq!(result.posted[0].entry.description, "Groceries");
    assert_eq!(result.posted[1].entry.description, "Salary");
}

#[test]
fn junk_row_rejected_and_batch_rolls_back() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let csv = "Date,Description,Amount\n2026-03-15,Groceries,-25.00\nbad-date,Nope,1.00\n";
    let preview = preview_bank_csv(conn, entity_id, roles(&acc), csv).expect("preview");
    assert!(preview.rows[0].error.is_none());
    assert!(preview.rows[1].error.is_some());

    let good = preview.rows[0].suggested.clone().expect("good row");
    let mut junk = good.clone();
    junk.amount_minor = 0;
    let err = post_import_rows(conn, &[good, junk], false).expect_err("junk");
    assert!(matches!(err, Error::Validation(_)));
    assert_eq!(count_entries(conn, entity_id), 0);
}

#[test]
fn dedupe_flags_preview_and_skips_post_unless_opted_in() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let existing = PostSimpleEntry {
        entity_id,
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        entry_date: "2026-03-15".into(),
        description: "Groceries".into(),
        reference: None,
        amount_minor: 2_500,
        category_account_id: Some(acc.food),
        wallet_account_id: Some(acc.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    };
    post_simple_entry(conn, &existing).expect("seed");

    // Same date+amount; description differs only by whitespace and case.
    let csv = "Date,Description,Amount\n2026-03-15,  GROCERIES  ,-25.00\n2026-03-15,  GROCERIES  ,-25.00\n";
    let preview = preview_bank_csv(conn, entity_id, roles(&acc), csv).expect("preview");
    assert_eq!(preview.rows.len(), 2);
    assert!(preview.rows[0].duplicate, "matches existing ledger entry");
    assert!(
        preview.rows[1].duplicate,
        "intra-file duplicate of the first parsed row"
    );

    let rows: Vec<PostSimpleEntry> = preview
        .rows
        .iter()
        .filter_map(|r| r.suggested.clone())
        .collect();
    let skipped = post_import_rows(conn, &rows, false).expect("skip dupes");
    assert!(skipped.posted.is_empty());
    assert_eq!(skipped.skipped_duplicate_count, 2);
    assert_eq!(count_entries(conn, entity_id), 1);

    let posted = post_import_rows(conn, &rows, true).expect("opt in");
    assert_eq!(posted.posted.len(), 2);
    assert_eq!(posted.skipped_duplicate_count, 0);
    assert_eq!(count_entries(conn, entity_id), 3);
}

#[test]
fn export_round_trips_posted_lines_and_marks_voided() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let grocery = PostSimpleEntry {
        entity_id,
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        entry_date: "2026-03-15".into(),
        description: "Groceries".into(),
        reference: Some("POS-1".into()),
        amount_minor: 2_500,
        category_account_id: Some(acc.food),
        wallet_account_id: Some(acc.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    };
    let view = post_simple_entry(conn, &grocery).expect("post");
    void_entry(conn, view.entry.id).expect("void");

    let salary = PostSimpleEntry {
        entity_id,
        kind: SimpleEntryKind::Income,
        bill_status: None,
        entry_date: "2026-03-16".into(),
        description: "Salary".into(),
        reference: None,
        amount_minor: 100_000,
        category_account_id: Some(acc.salary),
        wallet_account_id: Some(acc.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    };
    post_simple_entry(conn, &salary).expect("salary");

    let csv = export_journal_csv(conn, entity_id).expect("export");
    let lines = parse_journal_export(&csv).expect("parse export");
    assert!(
        lines.iter().any(|l| {
            l.date == "2026-03-16"
                && l.description == "Salary"
                && l.account_code == "1010"
                && l.account_name == "Checking"
                && l.debit_minor == 100_000
                && l.credit_minor == 0
                && l.status == JournalCsvStatus::Posted
        }),
        "posted salary debit line: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| {
            l.date == "2026-03-16"
                && l.description == "Salary"
                && l.account_code == "4000"
                && l.debit_minor == 0
                && l.credit_minor == 100_000
                && l.status == JournalCsvStatus::Posted
        }),
        "posted salary credit line: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| {
            l.description == "Groceries"
                && l.reference.as_deref() == Some("POS-1")
                && l.status == JournalCsvStatus::Voided
        }),
        "voided original: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| {
            l.description.starts_with("VOID:") && l.status == JournalCsvStatus::Voided
        }),
        "void reverse: {lines:?}"
    );
}

#[test]
fn preview_from_file_path_still_does_not_post() {
    let (dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let path = dir.path().join("bank.csv");
    std::fs::write(&path, grocery_csv()).expect("write csv");

    let before = count_entries(conn, entity_id);
    let preview = preview_bank_csv_file(conn, entity_id, roles(&acc), &path).expect("preview file");
    assert_eq!(preview.rows.len(), 1);
    assert!(preview.source.contains("bank.csv"));
    assert_eq!(count_entries(conn, entity_id), before);
}
