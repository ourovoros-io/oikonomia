//! A stored value the application can no longer parse is reported as a
//! corrupt vault that names the column, never as an I/O failure or as a
//! mistake in the caller's input.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use std::fmt::Debug;

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    CreateRecurringTemplate, EntryFilter, RecurringCadence, SimpleEntryKind, account_register,
    activity_window, balance_sheet, cash_flow_series, create_recurring_template, get_entity,
    get_entry, get_recurring_template, list_accounts, list_entities, list_entries,
    list_recurring_templates, post_simple_entry, trial_balance,
};
use oikonomia_core::util::parse_date;
use rusqlite::Connection;

/// A date that sorts inside 2026 as text but is not on the calendar, so
/// range filters still select the row and the reader has to parse it.
const IMPOSSIBLE_DATE: &str = "2026-02-31";

struct Book {
    entity_id: EntityId,
    checking: AccountId,
    food: AccountId,
    entry_id: JournalEntryId,
}

/// A personal book with one posted expense on 2026-02-10.
fn book(conn: &Connection) -> Book {
    let entity_id = common::book(conn, "Damaged", ChartTemplate::Personal);
    let checking = common::account(conn, entity_id, "1010");
    let food = common::account(conn, entity_id, "5100");

    let expense = common::simple_expense(entity_id, food, checking, "2026-02-10", 1_500);
    let posted = post_simple_entry(conn, &expense).expect("post");

    Book {
        entity_id,
        checking,
        food,
        entry_id: posted.entry.id,
    }
}

fn monthly_template(conn: &Connection, book: &Book) {
    create_recurring_template(
        conn,
        &CreateRecurringTemplate {
            entity_id: book.entity_id,
            name: "Rent".into(),
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            amount_minor: 85_000,
            cadence: RecurringCadence::Monthly,
            day_of_month: Some(1),
            category_account_id: Some(book.food),
            wallet_account_id: Some(book.checking),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
            memo: None,
            next_date: "2026-03-01".into(),
        },
    )
    .expect("template");
}

/// Writes a value the application would never write, as a damaged file could hold.
fn damage(conn: &Connection, sql: &str) {
    let changed = conn.execute(sql, []).expect("damage the row");
    assert!(changed > 0, "{sql} changed no row");
}

#[track_caller]
fn assert_corrupt<T: Debug>(result: Result<T, Error>, column: &str) {
    let err = result.expect_err("a damaged row must be refused");
    assert!(
        matches!(&err, Error::VaultCorrupt(detail) if detail.starts_with(column)),
        "expected a corrupt-vault error naming {column}, got: {err:?}"
    );
}

#[test]
fn an_unparseable_entry_date_is_corrupt_wherever_entries_are_read() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    damage(
        conn,
        &format!("UPDATE journal_entries SET entry_date = '{IMPOSSIBLE_DATE}'"),
    );
    let column = "journal_entries.entry_date";

    assert_corrupt(get_entry(conn, book.entry_id), column);
    assert_corrupt(
        list_entries(conn, book.entity_id, &EntryFilter::default()),
        column,
    );
    assert_corrupt(account_register(conn, book.food, None, None), column);
    assert_corrupt(
        cash_flow_series(conn, book.entity_id, "2026-02-01", "2026-03-31"),
        column,
    );
    assert_corrupt(
        activity_window(
            conn,
            book.entity_id,
            None,
            None,
            parse_date("2026-03-01").expect("today"),
        ),
        column,
    );
}

#[test]
fn an_unparseable_line_id_or_entry_status_is_corrupt() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);

    damage(conn, "UPDATE journal_lines SET id = 'line-' || rowid");
    assert_corrupt(get_entry(conn, book.entry_id), "journal_lines.id");
    assert_corrupt(
        list_entries(conn, book.entity_id, &EntryFilter::default()),
        "journal_lines.id",
    );

    damage(conn, "UPDATE journal_entries SET status = 'pending'");
    assert_corrupt(get_entry(conn, book.entry_id), "journal_entries.status");
}

/// A value of the wrong storage class is damage too. The driver reports it
/// before any parsing, and names the column as the query selects it.
#[test]
fn text_where_an_amount_belongs_is_corrupt() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    damage(
        conn,
        "UPDATE journal_lines SET debit_minor = 'plenty' WHERE credit_minor = 0",
    );

    assert_corrupt(get_entry(conn, book.entry_id), "debit_minor");
    assert_corrupt(
        list_entries(conn, book.entity_id, &EntryFilter::default()),
        "debit_minor",
    );
}

#[test]
fn an_unknown_account_type_is_corrupt() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    damage(
        conn,
        "UPDATE accounts SET account_type = 'treasure' WHERE code = '5100'",
    );
    let column = "accounts.account_type";

    assert_corrupt(list_accounts(conn, book.entity_id), column);
    assert_corrupt(account_register(conn, book.food, None, None), column);
}

#[test]
fn an_unknown_chart_template_is_corrupt() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    damage(conn, "UPDATE entities SET chart_template = 'household'");

    assert_corrupt(get_entity(conn, book.entity_id), "entities.chart_template");
    assert_corrupt(list_entities(conn), "entities.chart_template");
}

#[test]
fn a_fiscal_year_start_month_off_the_calendar_is_corrupt() {
    for month in [0, 13, 300] {
        let (_dir, vault) = common::vault();
        let conn = vault.connection().expect("conn");
        let book = book(conn);
        damage(
            conn,
            &format!("UPDATE entities SET fiscal_year_start_month = {month}"),
        );
        let column = "entities.fiscal_year_start_month";

        assert_corrupt(get_entity(conn, book.entity_id), column);
        assert_corrupt(trial_balance(conn, book.entity_id, "2026-12-31"), column);
        assert_corrupt(balance_sheet(conn, book.entity_id, "2026-12-31"), column);
    }
}

#[test]
fn a_damaged_recurring_template_is_corrupt() {
    let damaged_columns = [
        ("kind", "'gift'"),
        ("cadence", "'daily'"),
        ("next_date", "'2026-02-31'"),
        ("bill_status", "'overdue'"),
        ("day_of_month", "-1"),
        ("day_of_month", "0"),
        ("day_of_month", "32"),
    ];

    for (column, value) in damaged_columns {
        let (_dir, vault) = common::vault();
        let conn = vault.connection().expect("conn");
        let book = book(conn);
        monthly_template(conn, &book);
        // The schema's CHECK on day_of_month is what a damaged file bypasses.
        conn.execute_batch("PRAGMA ignore_check_constraints = ON")
            .expect("pragma");
        damage(
            conn,
            &format!("UPDATE recurring_templates SET {column} = {value}"),
        );

        let listed = list_recurring_templates(conn, book.entity_id);
        assert_corrupt(listed, &format!("recurring_templates.{column}"));
    }
}

#[test]
fn a_sound_book_still_reads_back() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    monthly_template(conn, &book);

    let entry = get_entry(conn, book.entry_id).expect("entry");
    assert_eq!(entry.lines.len(), 2);

    let templates = list_recurring_templates(conn, book.entity_id).expect("templates");
    let template = get_recurring_template(conn, templates[0].id).expect("template");
    assert_eq!(template.name, "Rent");
}
