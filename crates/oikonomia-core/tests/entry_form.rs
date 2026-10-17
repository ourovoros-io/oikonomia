//! The simple entry form an edit opens on: [`PostedEntryView::simple_form`].
//!
//! Each test checks what posting returns, what `get_entry` returns and what
//! `list_entries` returns, because the edit form opens from the list.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::ledger::{
    EntryFilter, ListedEntryKind, PostJournal, PostJournalLine, PostSimpleEntry, PostedEntryView,
    SimpleEntryAccounts, get_entry, list_entries, post_entry, post_simple_entry,
    replace_simple_entry, void_entry,
};
use oikonomia_core::money::Money;
use oikonomia_core::prefs::Locale;
use rusqlite::Connection;

/// A personal book and the accounts the tests post to.
struct Book {
    entity_id: EntityId,
    checking: AccountId,
    savings: AccountId,
    food: AccountId,
    salary: AccountId,
    receivable: AccountId,
    bills_payable: AccountId,
    card: AccountId,
}

fn book(conn: &Connection) -> Book {
    let entity_id = common::book(conn, "Forms", ChartTemplate::Personal);
    let by_code = |code: &str| common::account(conn, entity_id, code);

    Book {
        entity_id,
        checking: by_code("1010"),
        savings: by_code("1020"),
        food: by_code("5100"),
        salary: by_code("4000"),
        receivable: by_code("1200"),
        bills_payable: by_code("2050"),
        card: by_code("2000"),
    }
}

fn simple(book: &Book, accounts: SimpleEntryAccounts) -> PostSimpleEntry {
    PostSimpleEntry {
        entity_id: book.entity_id,
        accounts,
        entry_date: common::date("2026-03-15"),
        description: "entry".into(),
        reference: None,
        amount_minor: 2_500,
        settles_entry_id: None,
    }
}

fn post(conn: &Connection, book: &Book, accounts: SimpleEntryAccounts) -> PostedEntryView {
    post_simple_entry(conn, &simple(book, accounts)).expect("post")
}

/// Every way the simple entry form can post, a card-paid bill included: its
/// lines are those of an unpaid bill.
fn every_simple_entry(book: &Book) -> Vec<SimpleEntryAccounts> {
    use SimpleEntryAccounts as Accounts;

    vec![
        Accounts::Expense {
            category: book.food,
            wallet: book.checking,
        },
        Accounts::Income {
            category: book.salary,
            wallet: book.checking,
        },
        Accounts::IncomeReceivable {
            category: book.salary,
            receivable: book.receivable,
        },
        Accounts::BillPaid {
            category: book.food,
            wallet: book.checking,
        },
        Accounts::BillPaid {
            category: book.food,
            wallet: book.card,
        },
        Accounts::BillUnpaid {
            category: book.food,
            payable: book.bills_payable,
        },
        Accounts::BillUnpaid {
            category: book.food,
            payable: book.card,
        },
        Accounts::BillPayment {
            payable: book.bills_payable,
            wallet: book.checking,
        },
        Accounts::Transfer {
            from: book.checking,
            to: book.savings,
        },
    ]
}

/// The form `list_entries` reports for `id`.
fn listed_form(
    conn: &Connection,
    entity_id: EntityId,
    id: JournalEntryId,
) -> Option<SimpleEntryAccounts> {
    list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .into_iter()
        .find(|view| view.entry.id == id)
        .expect("the entry is listed")
        .simple_form
}

/// Fails unless posting, reading by id and listing all report `expected`.
#[track_caller]
fn assert_form(conn: &Connection, posted: &PostedEntryView, expected: Option<SimpleEntryAccounts>) {
    let id = posted.entry.id;
    assert_eq!(posted.simple_form, expected, "as posted");
    assert_eq!(
        get_entry(conn, id).expect("get").simple_form,
        expected,
        "by id"
    );
    assert_eq!(
        listed_form(conn, posted.entry.entity_id, id),
        expected,
        "in the list"
    );
}

#[test]
fn each_simple_entry_reopens_on_the_form_it_was_posted_with() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);

    for accounts in every_simple_entry(&book) {
        let posted = post(conn, &book, accounts);

        assert_form(conn, &posted, Some(accounts));
    }
}

#[test]
fn editing_a_bill_paid_at_once_keeps_it_a_paid_bill() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let paid = SimpleEntryAccounts::BillPaid {
        category: book.food,
        wallet: book.checking,
    };
    let original = post(conn, &book, paid);

    // The edit form sends back what it opened on.
    let form = original.simple_form.expect("a bill has a form");
    let replacement =
        replace_simple_entry(conn, original.entry.id, &simple(&book, form), Locale::En)
            .expect("replace");

    assert_form(conn, &replacement, Some(paid));
    assert_eq!(replacement.listed_kind, ListedEntryKind::Bill);
}

#[test]
fn a_voided_entry_and_its_reversal_have_no_form() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let bill = SimpleEntryAccounts::BillUnpaid {
        category: book.food,
        payable: book.bills_payable,
    };
    let posted = post(conn, &book, bill);

    let voided = void_entry(conn, posted.entry.id, Locale::En).expect("void");

    for id in [voided.original_id, voided.reverse_id] {
        let view = get_entry(conn, id).expect("get");
        assert_eq!(view.simple_form, None, "{id}");
        assert_eq!(listed_form(conn, book.entity_id, id), None, "{id}");
    }
}

#[test]
fn a_manual_entry_of_three_lines_has_no_form() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let money = |minor| Money::from_minor(minor).expect("money");
    let journal = PostJournal {
        entity_id: book.entity_id,
        entry_date: common::date("2026-03-02"),
        description: "split".into(),
        reference: None,
        lines: vec![
            PostJournalLine::debit(book.food, money(900)),
            PostJournalLine::credit(book.checking, money(600)),
            PostJournalLine::credit(book.savings, money(300)),
        ],
    };

    let posted = post_entry(conn, &journal).expect("post");

    assert_form(conn, &posted, None);
}

#[test]
fn an_older_entry_takes_its_form_from_its_lines() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let manual = common::post_two_line(conn, book.entity_id, "2026-03-02", ("5100", "1010"), 900);

    assert_form(
        conn,
        &manual,
        Some(SimpleEntryAccounts::Expense {
            category: book.food,
            wallet: book.checking,
        }),
    );
}
