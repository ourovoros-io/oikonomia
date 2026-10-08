//! Per-entry hidden flag: list/get/register keep the row; CSV export omits it.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use common::PASSWORD;
use oikonomia_core::csv::{export_journal_csv, parse_journal_export};
use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    EntryFilter, PostSimpleEntry, SimpleEntryAccounts, account_register, balance_sheet,
    cash_flow_series, dashboard_summary, get_entry, list_accounts, list_entries, post_simple_entry,
    profit_and_loss, profit_and_loss_export, replace_simple_entry, set_entry_hidden, void_entry,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

struct Book {
    entity_id: EntityId,
    wallet: AccountId,
    expense: AccountId,
}

fn create_book(conn: &Connection, name: &str, template: ChartTemplate) -> Book {
    let entity_id = common::book(conn, name, template);

    Book {
        entity_id,
        wallet: common::account(conn, entity_id, "1010"),
        expense: common::account(conn, entity_id, "5100"),
    }
}

/// An expense of `amount_minor` in the book's expense account, paid from its
/// wallet.
fn expense(book: &Book, date: &str, description: &str, amount_minor: i64) -> PostSimpleEntry {
    PostSimpleEntry {
        description: description.into(),
        ..common::simple_expense(
            book.entity_id,
            book.expense,
            book.wallet,
            date,
            amount_minor,
        )
    }
}

fn export_mentions(conn: &Connection, entity_id: EntityId, description: &str) -> bool {
    let csv = export_journal_csv(conn, entity_id).expect("export");
    parse_journal_export(&csv)
        .expect("parse")
        .iter()
        .any(|line| line.description == description)
}

/// Raw `journal_entries.hidden` column (0 or 1) for assertions.
fn entry_hidden_flag(conn: &Connection, id: JournalEntryId) -> i64 {
    conn.query_row(
        "SELECT hidden FROM journal_entries WHERE id = ?1",
        [id.to_string()],
        |row| row.get(0),
    )
    .expect("hidden column")
}

#[test]
fn export_omits_hidden_keeps_visible_posted_and_voided() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);

    let visible = post_simple_entry(conn, &expense(&book, "2026-03-15", "Groceries", 2_500))
        .expect("visible");
    assert!(!visible.entry.hidden);

    let hidden = post_simple_entry(conn, &expense(&book, "2026-03-16", "Secret", 1_000))
        .expect("hidden seed");
    set_entry_hidden(conn, hidden.entry.id, true).expect("hide");

    let voided =
        post_simple_entry(conn, &expense(&book, "2026-03-17", "VoidMe", 3_000)).expect("void seed");
    void_entry(conn, voided.entry.id, Locale::En).expect("void");

    let hidden_voided = post_simple_entry(conn, &expense(&book, "2026-03-18", "HiddenVoid", 4_000))
        .expect("hidden-void seed");
    void_entry(conn, hidden_voided.entry.id, Locale::En).expect("void hidden");
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

    assert_eq!(entry_hidden_flag(conn, hidden.entry.id), 1);
}

#[test]
fn pnl_includes_hidden_export_omits() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);
    let transport = list_accounts(conn, book.entity_id)
        .expect("accounts")
        .into_iter()
        .find(|a| a.code == "5200")
        .map(|a| a.id)
        .expect("5200");

    post_simple_entry(conn, &expense(&book, "2026-03-15", "Groceries", 2_500)).expect("visible");

    let hidden = post_simple_entry(
        conn,
        &expense(
            &Book {
                expense: transport,
                ..book
            },
            "2026-03-16",
            "Secret",
            1_000,
        ),
    )
    .expect("hidden seed");
    set_entry_hidden(conn, hidden.entry.id, true).expect("hide");

    let in_app = profit_and_loss(
        conn,
        book.entity_id,
        common::date("2026-03-01"),
        common::date("2026-03-31"),
    )
    .expect("in-app");
    assert_eq!(
        in_app.total_expenses, 3_500,
        "in-app Reports include Hidden: {in_app:?}"
    );
    assert!(
        in_app.expenses.iter().any(|l| l.code == "5200"),
        "in-app keeps the Hidden expense line: {:?}",
        in_app.expenses
    );

    let export = profit_and_loss_export(
        conn,
        book.entity_id,
        common::date("2026-03-01"),
        common::date("2026-03-31"),
    )
    .expect("export");
    assert_eq!(
        export.total_expenses, 2_500,
        "export P&L omits Hidden: {export:?}"
    );
    assert!(
        !export.expenses.iter().any(|l| l.code == "5200"),
        "Hidden expense must not appear on export: {:?}",
        export.expenses
    );
    assert!(
        export
            .expenses
            .iter()
            .any(|l| l.code == "5100" && l.balance_minor == 2_500),
        "visible expense stays on export: {:?}",
        export.expenses
    );

    assert!(
        !export_mentions(conn, book.entity_id, "Secret"),
        "CSV export also omits the Hidden description"
    );
}

/// The count a screen says "includes N hidden entries" with comes from core
/// and agrees with what the on-screen figures include and the export omits.
#[test]
fn every_on_screen_figure_says_how_many_hidden_entries_it_includes() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);
    let (march_1, march_31) = (common::date("2026-03-01"), common::date("2026-03-31"));

    post_simple_entry(conn, &expense(&book, "2026-03-15", "Groceries", 2_500)).expect("visible");
    for (date, description) in [
        ("2026-03-16", "Secret"),
        ("2026-03-17", "Voided"),
        ("2026-04-02", "April"),
    ] {
        let view =
            post_simple_entry(conn, &expense(&book, date, description, 1_000)).expect("post");
        set_entry_hidden(conn, view.entry.id, true).expect("hide");

        if description == "Voided" {
            void_entry(conn, view.entry.id, Locale::En).expect("void");
        }
    }

    let pnl = profit_and_loss(conn, book.entity_id, march_1, march_31).expect("pnl");
    let export = profit_and_loss_export(conn, book.entity_id, march_1, march_31).expect("export");
    let summary =
        dashboard_summary(conn, book.entity_id, march_1, march_31, march_31).expect("dashboard");
    let series = cash_flow_series(conn, book.entity_id, march_1, march_31).expect("series");
    let sheet = balance_sheet(conn, book.entity_id, common::date("2026-04-30")).expect("sheet");
    let before_april = balance_sheet(conn, book.entity_id, march_31).expect("sheet");

    // A voided hidden entry is no longer in any figure, so it is not counted.
    assert_eq!(pnl.hidden_entry_count, 1, "{pnl:?}");
    assert_eq!(summary.hidden_entry_count, 1);
    assert_eq!(series.hidden_entry_count, 1);
    assert_eq!(export.hidden_entry_count, 0, "the export omits them");
    assert_eq!(before_april.hidden_entry_count, 1);
    assert_eq!(sheet.hidden_entry_count, 2, "dated through the as-of day");
}

#[test]
fn a_hidden_transfer_moves_no_profit_and_is_not_counted() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);
    let savings = common::account(conn, book.entity_id, "1020");

    let transfer = post_simple_entry(
        conn,
        &PostSimpleEntry {
            accounts: SimpleEntryAccounts::Transfer {
                from: book.wallet,
                to: savings,
            },
            ..expense(&book, "2026-03-15", "Move", 500)
        },
    )
    .expect("transfer");
    set_entry_hidden(conn, transfer.entry.id, true).expect("hide");

    let pnl = profit_and_loss(
        conn,
        book.entity_id,
        common::date("2026-03-01"),
        common::date("2026-03-31"),
    )
    .expect("pnl");

    assert_eq!(pnl.hidden_entry_count, 0);
}

#[test]
fn voiding_hidden_entry_omits_original_and_reverse_from_export() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);

    let view =
        post_simple_entry(conn, &expense(&book, "2026-03-15", "Secret", 2_500)).expect("post");
    set_entry_hidden(conn, view.entry.id, true).expect("hide");
    let voided = void_entry(conn, view.entry.id, Locale::En).expect("void");

    assert!(!export_mentions(conn, book.entity_id, "Secret"));
    assert!(!export_mentions(conn, book.entity_id, "VOID: Secret"));

    let original = get_entry(conn, view.entry.id).expect("original");
    let reverse = get_entry(conn, voided.reverse_id).expect("reverse");
    assert!(original.entry.hidden);
    assert!(reverse.entry.hidden);
    assert!(original.is_voided);
    assert!(reverse.is_voided);
    assert_eq!(entry_hidden_flag(conn, view.entry.id), 1);
    assert_eq!(entry_hidden_flag(conn, voided.reverse_id), 1);

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
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);

    let view = post_simple_entry(
        conn,
        &expense(&book, "2026-03-15", "HiddenThenShown", 2_500),
    )
    .expect("post");

    set_entry_hidden(conn, view.entry.id, true).expect("hide");
    assert!(!export_mentions(conn, book.entity_id, "HiddenThenShown"));
    assert_eq!(entry_hidden_flag(conn, view.entry.id), 1);

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
    assert_eq!(entry_hidden_flag(conn, view.entry.id), 0);
}

#[test]
fn hidden_persists_across_reopen_and_backup_restore() {
    let (_dir, mut vault) = common::vault();
    let book;
    let entry_id;
    {
        let conn = vault.connection().expect("conn");
        book = create_book(conn, "Personal", ChartTemplate::Personal);
        let view = post_simple_entry(
            conn,
            &expense(&book, "2026-03-15", "PersistedSecret", 2_500),
        )
        .expect("post");
        set_entry_hidden(conn, view.entry.id, true).expect("hide");
        entry_id = view.entry.id;
        assert!(!export_mentions(conn, book.entity_id, "PersistedSecret"));
        assert_eq!(entry_hidden_flag(conn, entry_id), 1);
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
        assert_eq!(entry_hidden_flag(conn, entry_id), 1);
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
    assert_eq!(entry_hidden_flag(conn, entry_id), 1);
    assert!(!export_mentions(conn, book.entity_id, "PersistedSecret"));

    // A backup copies the vault; the source still holds the row.
    let source = vault.connection().expect("source conn");
    assert_eq!(entry_hidden_flag(source, entry_id), 1);
}

#[test]
fn hidden_works_for_personal_and_company_entities() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let personal = create_book(conn, "Household", ChartTemplate::Personal);
    let company = create_book(conn, "Acme Ltd", ChartTemplate::Company);

    let p = post_simple_entry(
        conn,
        &expense(&personal, "2026-03-15", "PersonalSecret", 1_100),
    )
    .expect("personal post");
    let c = post_simple_entry(
        conn,
        &expense(&company, "2026-03-15", "CompanySecret", 2_200),
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
    assert_eq!(entry_hidden_flag(conn, p.entry.id), 1);
    assert_eq!(entry_hidden_flag(conn, c.entry.id), 1);
}

#[test]
fn set_hidden_missing_id_is_not_found() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let err = set_entry_hidden(conn, JournalEntryId::generate(), true).expect_err("missing");
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

#[test]
fn register_includes_hidden_and_exposes_flag() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Register", ChartTemplate::Personal);
    let hidden =
        post_simple_entry(conn, &expense(&book, "2026-03-15", "HiddenReg", 2_500)).expect("post");
    set_entry_hidden(conn, hidden.entry.id, true).expect("hide");

    let lines = account_register(conn, book.wallet, None, None).expect("register");
    assert!(
        lines
            .iter()
            .any(|line| line.entry_id == hidden.entry.id && line.hidden),
        "register keeps hidden rows and exposes hidden: {lines:?}"
    );
}

#[test]
fn replacing_hidden_entry_keeps_replacement_hidden_and_omits_from_export() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = create_book(conn, "Personal", ChartTemplate::Personal);

    let hidden = post_simple_entry(conn, &expense(&book, "2026-03-15", "SecretOriginal", 2_500))
        .expect("post hidden");
    set_entry_hidden(conn, hidden.entry.id, true).expect("hide");

    let mut corrected = expense(&book, "2026-03-15", "SecretReplacement", 3_100);
    corrected.description = "SecretReplacement".into();
    let replacement = replace_simple_entry(conn, hidden.entry.id, &corrected, Locale::En)
        .expect("replace hidden");
    assert!(
        replacement.entry.hidden,
        "replacement of a hidden entry stays hidden"
    );
    assert_eq!(entry_hidden_flag(conn, replacement.entry.id), 1);
    assert_eq!(entry_hidden_flag(conn, hidden.entry.id), 1);

    assert!(
        !export_mentions(conn, book.entity_id, "SecretOriginal"),
        "voided hidden original omitted from CSV"
    );
    assert!(
        !export_mentions(conn, book.entity_id, "VOID: SecretOriginal"),
        "hidden VOID reverse omitted from CSV"
    );
    assert!(
        !export_mentions(conn, book.entity_id, "SecretReplacement"),
        "hidden replacement omitted from CSV"
    );

    let listed = list_entries(conn, book.entity_id, &EntryFilter::default()).expect("list");
    assert!(
        listed
            .iter()
            .any(|v| v.entry.id == hidden.entry.id && v.entry.hidden),
        "list still has hidden original: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .any(|v| v.entry.id == replacement.entry.id && v.entry.hidden),
        "list still has hidden replacement: {listed:?}"
    );

    let visible = post_simple_entry(
        conn,
        &expense(&book, "2026-03-16", "VisibleOriginal", 1_200),
    )
    .expect("post visible");
    let mut visible_fix = expense(&book, "2026-03-16", "VisibleReplacement", 1_400);
    visible_fix.description = "VisibleReplacement".into();
    let visible_repl = replace_simple_entry(conn, visible.entry.id, &visible_fix, Locale::En)
        .expect("replace visible");
    assert!(!visible_repl.entry.hidden);
    assert!(
        export_mentions(conn, book.entity_id, "VisibleReplacement"),
        "visible replacement still exports"
    );
    assert!(
        !export_mentions(conn, book.entity_id, "SecretReplacement"),
        "hidden replacement stays omitted after a visible edit"
    );
}
