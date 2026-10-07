//! An archived entity: readable everywhere, never writable.
//!
//! Each test archives a book that already holds an entry, a recurring
//! template and a document, then tries one kind of operation. A write that
//! takes the entity is refused as if the entity did not exist, and leaves the
//! book as it was; a read answers as it did before the book was archived.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::csv::{export_journal_csv, post_import_rows};
use oikonomia_core::documents::{
    NewDocument, attach_document, list_documents, post_simple_entry_with_document,
};
use oikonomia_core::domain::{AccountId, AccountType, ChartTemplate, EntityId};
use oikonomia_core::error::{Error, Resource};
use oikonomia_core::ledger::{
    CreateAccount, CreateRecurringTemplate, CreateRecurringTemplateRequest, EntryFilter,
    PostSimpleEntryRequest, PostedEntryView, RecurringCadence, RecurringTemplateView,
    SimpleEntryKind, UpdateRecurringTemplate, UpdateRecurringTemplateRequest, account_register,
    archive_entity, balance_sheet, cash_flow_series, create_account, create_recurring_template,
    delete_entity, get_entity, get_entry, list_accounts, list_entities, list_entries,
    list_recurring_templates, post_entry, post_recurring_template, post_simple_entry,
    profit_and_loss, replace_simple_entry, set_account_opening_balance, trial_balance,
    update_recurring_template, void_entry,
};
use oikonomia_core::prefs::Locale;
use rusqlite::Connection;

/// The error every refused write returns: the one for an unknown entity.
const ENTITY_NOT_FOUND: Error = Error::NotFound(Resource::Entity);

/// A book that was archived after it was used, and what it holds.
struct ArchivedBook {
    /// The archived entity.
    entity_id: EntityId,
    /// Its food account.
    food: AccountId,
    /// Its checking account.
    checking: AccountId,
    /// The expense posted before the book was archived.
    entry: PostedEntryView,
    /// The template saved before the book was archived.
    template: RecurringTemplateView,
}

/// A receipt small enough to store.
const RECEIPT: NewDocument<'static> = NewDocument {
    filename: "receipt.txt",
    mime_type: "text/plain",
    data: b"TOTAL 10,00 EUR",
};

/// The template request for a monthly expense of the book.
fn monthly_rent(
    entity_id: EntityId,
    food: AccountId,
    checking: AccountId,
) -> CreateRecurringTemplateRequest {
    CreateRecurringTemplateRequest {
        entity_id,
        name: "Rent".into(),
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        amount_minor: 85_000,
        cadence: RecurringCadence::Monthly,
        day_of_month: Some(1),
        category_account_id: Some(food),
        wallet_account_id: Some(checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
        memo: None,
        next_date: "2026-03-01".into(),
    }
}

/// Creates a book with one expense, one template and one document, and
/// archives it.
fn archived_book(conn: &Connection) -> ArchivedBook {
    let entity_id = common::book(conn, "Closed shop", ChartTemplate::Personal);
    let food = common::account(conn, entity_id, "5100");
    let checking = common::account(conn, entity_id, "1010");

    let expense = common::simple_expense(entity_id, food, checking, "2026-02-10", 2_500);
    let entry = post_simple_entry(conn, &expense).expect("post before archiving");
    attach_document(conn, entity_id, entry.entry.id, &RECEIPT).expect("attach before archiving");
    let template = create_recurring_template(
        conn,
        &common::strict::<_, CreateRecurringTemplate>(monthly_rent(entity_id, food, checking)),
    )
    .expect("template before archiving");

    archive_entity(conn, entity_id).expect("archive");

    ArchivedBook {
        entity_id,
        food,
        checking,
        entry,
        template,
    }
}

/// Fails unless the book holds exactly what [`archived_book`] put in it.
fn assert_untouched(conn: &Connection, book: &ArchivedBook) {
    let entries = list_entries(conn, book.entity_id, &EntryFilter::default()).expect("entries");
    let entry_ids: Vec<_> = entries.iter().map(|view| view.entry.id).collect();
    assert_eq!(entry_ids, [book.entry.entry.id], "an entry was written");

    let documents = list_documents(conn, book.entity_id).expect("documents");
    assert_eq!(documents.len(), 1, "a document was written");

    let templates = list_recurring_templates(conn, book.entity_id).expect("templates");
    assert_eq!(
        templates,
        std::slice::from_ref(&book.template),
        "a template was written"
    );

    let accounts = list_accounts(conn, book.entity_id).expect("accounts");
    assert!(
        accounts.iter().all(|account| account.code != "5999"),
        "an account was written"
    );
}

#[test]
fn posting_an_entry_to_an_archived_entity_is_refused() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);

    let journal = common::two_line(conn, book.entity_id, "2026-03-01", ("5100", "1010"), 900);
    let simple =
        common::simple_expense(book.entity_id, book.food, book.checking, "2026-03-02", 700);

    assert_eq!(post_entry(conn, &journal).err(), Some(ENTITY_NOT_FOUND));
    assert_eq!(
        post_simple_entry(conn, &simple).err(),
        Some(ENTITY_NOT_FOUND)
    );
    assert_untouched(conn, &book);
}

#[test]
fn every_operation_that_posts_for_the_caller_is_refused_on_an_archived_entity() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);
    let entry_id = book.entry.entry.id;
    let replacement = common::simple_expense(
        book.entity_id,
        book.food,
        book.checking,
        "2026-02-11",
        2_600,
    );

    // A void posts a reversing entry, a correction a reversal and a
    // replacement, an opening balance an adjustment, a template its entry.
    assert_eq!(
        void_entry(conn, entry_id, Locale::En).err(),
        Some(ENTITY_NOT_FOUND)
    );
    assert_eq!(
        replace_simple_entry(conn, entry_id, &replacement, Locale::En).err(),
        Some(ENTITY_NOT_FOUND)
    );
    assert_eq!(
        set_account_opening_balance(
            conn,
            book.checking,
            50_000,
            common::date("2026-01-01"),
            Locale::En
        )
        .err(),
        Some(ENTITY_NOT_FOUND)
    );
    assert_eq!(
        post_recurring_template(conn, book.template.id, None, None).err(),
        Some(ENTITY_NOT_FOUND)
    );

    assert_untouched(conn, &book);
    let kept = get_entry(conn, entry_id).expect("the entry is still there");
    assert!(!kept.is_voided, "the entry was voided");
}

#[test]
fn creating_or_updating_a_template_of_an_archived_entity_is_refused() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);

    let create = common::strict::<_, CreateRecurringTemplate>(monthly_rent(
        book.entity_id,
        book.food,
        book.checking,
    ));
    let update = common::strict::<_, UpdateRecurringTemplate>(UpdateRecurringTemplateRequest {
        id: book.template.id,
        name: "Renamed".into(),
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        amount_minor: 1,
        cadence: RecurringCadence::Weekly,
        day_of_month: None,
        category_account_id: Some(book.food),
        wallet_account_id: Some(book.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
        memo: None,
        next_date: "2026-04-01".into(),
    });

    assert_eq!(
        create_recurring_template(conn, &create).err(),
        Some(ENTITY_NOT_FOUND)
    );
    assert_eq!(
        update_recurring_template(conn, &update).err(),
        Some(ENTITY_NOT_FOUND)
    );
    assert_untouched(conn, &book);
}

#[test]
fn creating_an_account_in_an_archived_entity_is_refused() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);

    let refused = create_account(
        conn,
        &CreateAccount {
            entity_id: book.entity_id,
            code: "5999".into(),
            name: "Late addition".into(),
            account_type: AccountType::Expense,
            sort_order: None,
        },
    );

    assert_eq!(refused.err(), Some(ENTITY_NOT_FOUND));
    assert_untouched(conn, &book);
}

#[test]
fn importing_csv_rows_into_an_archived_entity_is_refused() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);
    let row = |entry_date: &str, amount_minor| PostSimpleEntryRequest {
        entity_id: book.entity_id,
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        entry_date: entry_date.into(),
        description: "groceries".into(),
        reference: None,
        amount_minor,
        category_account_id: Some(book.food),
        wallet_account_id: Some(book.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    };

    let new_rows = post_import_rows(conn, &[row("2026-03-05", 1_200)], false);
    // The row repeats the entry the book holds, so nothing would be posted;
    // the batch is refused all the same.
    let only_duplicates = post_import_rows(conn, &[row("2026-02-10", 2_500)], false);

    assert_eq!(new_rows.err(), Some(ENTITY_NOT_FOUND));
    assert_eq!(only_duplicates.err(), Some(ENTITY_NOT_FOUND));
    assert_untouched(conn, &book);
}

#[test]
fn attaching_a_document_in_an_archived_entity_is_refused() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);
    let second = NewDocument {
        filename: "second.txt",
        ..RECEIPT
    };
    let expense =
        common::simple_expense(book.entity_id, book.food, book.checking, "2026-03-03", 400);

    let attached = attach_document(conn, book.entity_id, book.entry.entry.id, &second);
    let posted_with = post_simple_entry_with_document(conn, &expense, &second, None);

    assert_eq!(attached.err(), Some(ENTITY_NOT_FOUND));
    assert_eq!(posted_with.err(), Some(ENTITY_NOT_FOUND));
    assert_untouched(conn, &book);
}

#[test]
fn an_archived_entity_is_read_as_it_was_before_it_was_archived() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);
    let (id, from, to) = (
        book.entity_id,
        common::date("2026-01-01"),
        common::date("2026-12-31"),
    );

    assert_eq!(get_entity(conn, id).expect("entity").name, "Closed shop");
    assert_untouched(conn, &book);

    let pnl = profit_and_loss(conn, id, from, to).expect("profit and loss");
    assert_eq!(pnl.expenses.len(), 1);
    let trial = trial_balance(conn, id, to).expect("trial balance");
    assert_eq!(trial.lines.len(), 2);
    balance_sheet(conn, id, to).expect("balance sheet");
    let series = cash_flow_series(conn, id, from, to).expect("cash flow");
    assert_eq!(series.total_expenses_minor, 2_500);

    let register = account_register(conn, book.food, None, None).expect("register");
    assert_eq!(register.len(), 1);
    let exported = export_journal_csv(conn, id).expect("journal export");
    assert!(exported.contains("groceries"), "{exported}");

    // It is left out of the list of books, which is how the app hides it.
    let listed = list_entities(conn).expect("entities");
    assert!(listed.iter().all(|entity| entity.id != id));
}

#[test]
fn an_archived_entity_can_still_be_deleted() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = archived_book(conn);

    delete_entity(conn, book.entity_id).expect("delete");

    assert_eq!(
        get_entity(conn, book.entity_id).err(),
        Some(ENTITY_NOT_FOUND)
    );
}
