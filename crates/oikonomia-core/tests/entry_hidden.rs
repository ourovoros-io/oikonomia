//! Per-entry hidden flag: list/get/register keep the row; CSV export omits it.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::csv::{export_journal_csv, parse_journal_export};
use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, EntryStatus, JournalEntryId};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntry, SimpleEntryKind, account_register, create_entity,
    get_entry, list_accounts, list_entries, post_simple_entry, set_entry_hidden, void_entry,
};
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

const PASSWORD: &str = "correct horse battery staple";

fn setup() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init(PASSWORD).expect("init");
    (dir, vault)
}

struct Book {
    entity_id: EntityId,
    wallet: AccountId,
    expense: AccountId,
}

fn create_book(conn: &Connection, name: &str, template: ChartTemplate) -> Book {
    let entity = create_entity(
        conn,
        &CreateEntity {
            name: name.into(),
            base_currency: "EUR".into(),
            chart_template: template,
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
    Book {
        entity_id: entity.id,
        wallet: by_code("1010"),
        expense: by_code("5100"),
    }
}

fn expense(
    entity_id: EntityId,
    wallet: AccountId,
    expense: AccountId,
    date: &str,
    description: &str,
    amount_minor: i64,
) -> PostSimpleEntry {
    PostSimpleEntry {
        entity_id,
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        entry_date: date.into(),
        description: description.into(),
        reference: None,
        amount_minor,
        category_account_id: Some(expense),
        wallet_account_id: Some(wallet),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    }
}

fn export_mentions(conn: &Connection, entity_id: EntityId, description: &str) -> bool {
    let csv = export_journal_csv(conn, entity_id).expect("export");
    parse_journal_export(&csv)
        .expect("parse")
        .iter()
        .any(|line| line.description == description)
}

fn db_hidden(conn: &Connection, id: JournalEntryId) -> i64 {
    conn.query_row(
        "SELECT hidden FROM journal_entries WHERE id = ?1",
        [id.0.to_string()],
        |row| row.get(0),
    )
    .expect("hidden column")
}

#[test]
fn export_omits_hidden_keeps_visible_posted_and_voided() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);

    let visible = post_simple_entry(
        conn,
        &expense(
            book.entity_id,
            book.wallet,
            book.expense,
            "2026-03-15",
            "Groceries",
            2_500,
        ),
    )
    .expect("visible");
    assert!(!visible.entry.hidden);

    let hidden = post_simple_entry(
        conn,
        &expense(
            book.entity_id,
            book.wallet,
            book.expense,
            "2026-03-16",
            "Secret",
            1_000,
        ),
    )
    .expect("hidden seed");
    set_entry_hidden(conn, hidden.entry.id, true).expect("hide");

    let voided = post_simple_entry(
        conn,
        &expense(
            book.entity_id,
            book.wallet,
            book.expense,
            "2026-03-17",
            "VoidMe",
            3_000,
        ),
    )
    .expect("void seed");
    void_entry(conn, voided.entry.id).expect("void");

    let hidden_voided = post_simple_entry(
        conn,
        &expense(
            book.entity_id,
            book.wallet,
            book.expense,
            "2026-03-18",
            "HiddenVoid",
            4_000,
        ),
    )
    .expect("hidden-void seed");
    void_entry(conn, hidden_voided.entry.id).expect("void hidden");
    set_entry_hidden(conn, hidden_voided.entry.id, true).expect("hide voided");

    let listed = list_entries(conn, book.entity_id, &EntryFilter::default()).expect("list");
    assert!(
        listed
            .iter()
            .any(|v| v.entry.description == "Secret" && v.entry.hidden),
        "owner still sees hidden in list: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .any(|v| v.entry.description == "Groceries" && !v.entry.hidden),
        "visible posted stays listed"
    );

    let got = get_entry(conn, hidden.entry.id).expect("get hidden");
    assert!(got.entry.hidden);

    assert!(export_mentions(conn, book.entity_id, "Groceries"));
    assert!(export_mentions(conn, book.entity_id, "VoidMe"));
    assert!(export_mentions(conn, book.entity_id, "VOID: VoidMe"));
    assert!(
        !export_mentions(conn, book.entity_id, "Secret"),
        "hidden posted omitted"
    );
    assert!(
        !export_mentions(conn, book.entity_id, "HiddenVoid"),
        "hidden-and-voided omitted"
    );

    assert_eq!(db_hidden(conn, hidden.entry.id), 1);
}

#[test]
fn voiding_hidden_entry_omits_original_and_reverse_from_export() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);

    let view = post_simple_entry(
        conn,
        &expense(
            book.entity_id,
            book.wallet,
            book.expense,
            "2026-03-15",
            "Secret",
            2_500,
        ),
    )
    .expect("post");
    set_entry_hidden(conn, view.entry.id, true).expect("hide");
    let voided = void_entry(conn, view.entry.id).expect("void");

    assert!(!export_mentions(conn, book.entity_id, "Secret"));
    assert!(!export_mentions(conn, book.entity_id, "VOID: Secret"));

    let original = get_entry(conn, view.entry.id).expect("original");
    let reverse = get_entry(conn, voided.reverse_id).expect("reverse");
    assert!(original.entry.hidden);
    assert!(reverse.entry.hidden);
    assert!(original.is_voided);
    assert!(reverse.is_voided);
    assert_eq!(db_hidden(conn, view.entry.id), 1);
    assert_eq!(db_hidden(conn, voided.reverse_id), 1);

    let listed = list_entries(conn, book.entity_id, &EntryFilter::default()).expect("list");
    assert!(
        listed
            .iter()
            .any(|v| v.entry.id == view.entry.id && v.entry.hidden),
        "list still returns hidden original: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .any(|v| v.entry.id == voided.reverse_id && v.entry.hidden),
        "list still returns hidden reverse: {listed:?}"
    );
}

#[test]
fn unhide_puts_entry_back_in_export() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);

    let view = post_simple_entry(
        conn,
        &expense(
            book.entity_id,
            book.wallet,
            book.expense,
            "2026-03-15",
            "HiddenThenShown",
            2_500,
        ),
    )
    .expect("post");

    set_entry_hidden(conn, view.entry.id, true).expect("hide");
    assert!(!export_mentions(conn, book.entity_id, "HiddenThenShown"));
    assert_eq!(db_hidden(conn, view.entry.id), 1);

    let listed = list_entries(conn, book.entity_id, &EntryFilter::default()).expect("list");
    assert!(
        listed
            .iter()
            .any(|v| v.entry.id == view.entry.id && v.entry.hidden),
        "list still returns hidden after export"
    );

    let shown = set_entry_hidden(conn, view.entry.id, false).expect("unhide");
    assert!(!shown.entry.hidden);
    assert!(export_mentions(conn, book.entity_id, "HiddenThenShown"));
    assert_eq!(db_hidden(conn, view.entry.id), 0);
}

#[test]
fn hidden_persists_across_reopen_and_backup_restore() {
    let (dir, mut vault) = setup();
    let book;
    let entry_id;
    {
        let conn = vault.connection().expect("conn");
        book = create_book(conn, "Personal", ChartTemplate::Personal);
        let view = post_simple_entry(
            conn,
            &expense(
                book.entity_id,
                book.wallet,
                book.expense,
                "2026-03-15",
                "PersistedSecret",
                2_500,
            ),
        )
        .expect("post");
        set_entry_hidden(conn, view.entry.id, true).expect("hide");
        entry_id = view.entry.id;
        assert!(!export_mentions(conn, book.entity_id, "PersistedSecret"));
        assert_eq!(db_hidden(conn, entry_id), 1);
    }

    vault.lock();
    vault.unlock(PASSWORD).expect("reopen session");
    {
        let conn = vault.connection().expect("conn");
        let listed = list_entries(conn, book.entity_id, &EntryFilter::default()).expect("list");
        assert!(
            listed
                .iter()
                .any(|v| v.entry.id == entry_id && v.entry.hidden),
            "hidden survives lock/unlock + migrate"
        );
        assert_eq!(db_hidden(conn, entry_id), 1);
    }

    let archive_dir = TempDir::new().expect("archive dir");
    let archive = archive_dir.path().join("books.oikonomia-backup");
    vault.backup_to(&archive).expect("backup");

    let restore_dir = TempDir::new().expect("restore");
    oikonomia_core::vault::restore_from_path(&archive, restore_dir.path(), false).expect("restore");
    let mut restored = Vault::open_path(restore_dir.path()).expect("open restored");
    restored.unlock(PASSWORD).expect("unlock restored");
    let conn = restored.connection().expect("restored conn");
    let listed = list_entries(conn, book.entity_id, &EntryFilter::default()).expect("list");
    assert!(
        listed
            .iter()
            .any(|v| v.entry.id == entry_id && v.entry.hidden),
        "hidden rows stay in a restored vault"
    );
    assert_eq!(db_hidden(conn, entry_id), 1);
    assert!(!export_mentions(conn, book.entity_id, "PersistedSecret"));

    // Source data dir still holds the row too (backup is a copy, not a filter).
    let _ = dir;
}

#[test]
fn hidden_works_for_personal_and_company_entities() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let personal = create_book(conn, "Household", ChartTemplate::Personal);
    let company = create_book(conn, "Acme Ltd", ChartTemplate::Company);

    let p = post_simple_entry(
        conn,
        &expense(
            personal.entity_id,
            personal.wallet,
            personal.expense,
            "2026-03-15",
            "PersonalSecret",
            1_100,
        ),
    )
    .expect("personal post");
    let c = post_simple_entry(
        conn,
        &expense(
            company.entity_id,
            company.wallet,
            company.expense,
            "2026-03-15",
            "CompanySecret",
            2_200,
        ),
    )
    .expect("company post");

    set_entry_hidden(conn, p.entry.id, true).expect("hide personal");
    set_entry_hidden(conn, c.entry.id, true).expect("hide company");

    let personal_list = list_entries(conn, personal.entity_id, &EntryFilter::default()).expect("p");
    let company_list = list_entries(conn, company.entity_id, &EntryFilter::default()).expect("c");
    assert!(
        personal_list
            .iter()
            .any(|v| v.entry.description == "PersonalSecret" && v.entry.hidden)
    );
    assert!(
        company_list
            .iter()
            .any(|v| v.entry.description == "CompanySecret" && v.entry.hidden)
    );

    assert!(!export_mentions(conn, personal.entity_id, "PersonalSecret"));
    assert!(!export_mentions(conn, company.entity_id, "CompanySecret"));
    assert_eq!(db_hidden(conn, p.entry.id), 1);
    assert_eq!(db_hidden(conn, c.entry.id), 1);
}

#[test]
fn set_hidden_missing_id_is_not_found() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let err = set_entry_hidden(conn, JournalEntryId::new(), true).expect_err("missing");
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

#[test]
fn set_hidden_allows_draft_rows() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Drafts", ChartTemplate::Personal);
    let id = JournalEntryId::new();
    conn.execute(
        "
        INSERT INTO journal_entries (
            id, entity_id, entry_date, description, reference,
            status, created_at, posted_at, voided_by_entry_id, hidden
        ) VALUES (?1, ?2, '2026-03-01', 'Draft memo', NULL, 'draft', 'unix:1', NULL, NULL, 0)
        ",
        rusqlite::params![id.0.to_string(), book.entity_id.0.to_string()],
    )
    .expect("insert draft");

    let hidden = set_entry_hidden(conn, id, true).expect("hide draft");
    assert!(hidden.entry.hidden);
    assert_eq!(hidden.entry.status, EntryStatus::Draft);
    assert_eq!(db_hidden(conn, id), 1);
}

#[test]
fn register_includes_hidden_and_exposes_flag() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Register", ChartTemplate::Personal);
    let hidden = post_simple_entry(
        conn,
        &expense(
            book.entity_id,
            book.wallet,
            book.expense,
            "2026-03-15",
            "HiddenReg",
            2_500,
        ),
    )
    .expect("post");
    set_entry_hidden(conn, hidden.entry.id, true).expect("hide");

    let lines = account_register(conn, book.wallet, None, None).expect("register");
    assert!(
        lines
            .iter()
            .any(|line| line.entry_id == hidden.entry.id && line.hidden),
        "register keeps hidden rows and exposes hidden: {lines:?}"
    );
}
