//! Paying an existing bill: the open bills of a book, the payment that
//! settles one in full, and what voiding or correcting either does.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::error::{Error, ValidationError};
use oikonomia_core::ledger::{
    EntryFilter, OpenBill, PostSimpleEntry, PostSimpleEntryRequest, PostedEntryView,
    SimpleBillStatus, SimpleEntryAccounts, SimpleEntryKind, archive_account, get_entry,
    list_entries, list_open_bills, post_simple_entry, replace_simple_entry, void_entry,
};
use oikonomia_core::prefs::Locale;
use rusqlite::Connection;

/// A personal book and the accounts the tests post to.
struct Book {
    entity_id: EntityId,
    checking: AccountId,
    power: AccountId,
    bills_payable: AccountId,
    card: AccountId,
}

fn book(conn: &Connection, name: &str) -> Book {
    let entity_id = common::book(conn, name, ChartTemplate::Personal);
    let by_code = |code: &str| common::account(conn, entity_id, code);

    Book {
        entity_id,
        checking: by_code("1010"),
        power: by_code("5100"),
        bills_payable: by_code("2050"),
        card: by_code("2000"),
    }
}

fn post(
    conn: &Connection,
    book: &Book,
    accounts: SimpleEntryAccounts,
    amount_minor: i64,
    settles_entry_id: Option<JournalEntryId>,
) -> Result<PostedEntryView, Error> {
    post_simple_entry(
        conn,
        &PostSimpleEntry {
            entity_id: book.entity_id,
            accounts,
            entry_date: common::date("2026-04-10"),
            description: "power".into(),
            reference: None,
            amount_minor,
            settles_entry_id,
        },
    )
}

/// Posts an unpaid bill of `amount_minor` owed on `payable`.
fn bill(conn: &Connection, book: &Book, payable: AccountId, amount_minor: i64) -> JournalEntryId {
    let unpaid = SimpleEntryAccounts::BillUnpaid {
        category: book.power,
        payable,
    };
    post(conn, book, unpaid, amount_minor, None)
        .expect("post the bill")
        .entry
        .id
}

/// A payment from checking of a bill owed on `payable`.
fn payment(book: &Book, payable: AccountId) -> SimpleEntryAccounts {
    SimpleEntryAccounts::BillPayment {
        payable,
        wallet: book.checking,
    }
}

/// Pays `bill_id` in full from checking.
fn pay(conn: &Connection, book: &Book, bill_id: JournalEntryId) -> Result<PostedEntryView, Error> {
    post(
        conn,
        book,
        payment(book, book.bills_payable),
        4_000,
        Some(bill_id),
    )
}

fn open_ids(conn: &Connection, book: &Book) -> Vec<JournalEntryId> {
    list_open_bills(conn, book.entity_id, None)
        .expect("open bills")
        .into_iter()
        .map(|open| open.entry_id)
        .collect()
}

#[test]
fn an_unpaid_bill_is_open_until_a_payment_settles_it() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);

    assert_eq!(
        list_open_bills(conn, book.entity_id, None).expect("open bills"),
        [OpenBill {
            entry_id: bill_id,
            entry_date: common::date("2026-04-10"),
            description: "power".into(),
            amount_minor: 4_000,
            payable_account_id: book.bills_payable,
        }]
    );

    let paid = pay(conn, &book, bill_id).expect("pay the bill");

    assert_eq!(paid.settles_entry_id, Some(bill_id));
    assert_eq!(
        get_entry(conn, paid.entry.id)
            .expect("get")
            .settles_entry_id,
        Some(bill_id)
    );
    assert_eq!(open_ids(conn, &book), []);
}

#[test]
fn only_an_unpaid_bill_of_v11_is_open() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let paid_now = SimpleEntryAccounts::BillPaid {
        category: book.power,
        wallet: book.checking,
    };
    post(conn, &book, paid_now, 4_000, None).expect("post a paid bill");
    let older = bill(conn, &book, book.bills_payable, 4_000);
    // A bill posted before v11 stores no status: its payment, if any, named
    // no bill, so it is never open.
    conn.execute(
        "UPDATE journal_entries SET bill_status = NULL WHERE id = ?1",
        [older.to_string()],
    )
    .expect("clear the status");
    let voided = bill(conn, &book, book.bills_payable, 4_000);
    void_entry(conn, voided, Locale::En).expect("void");

    assert_eq!(open_ids(conn, &book), []);
}

#[test]
fn voiding_the_payment_reopens_the_bill() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);
    let paid = pay(conn, &book, bill_id).expect("pay");

    void_entry(conn, paid.entry.id, Locale::En).expect("void the payment");

    assert_eq!(open_ids(conn, &book), [bill_id]);
    pay(conn, &book, bill_id).expect("pay it again");
}

#[test]
fn a_settled_bill_is_neither_voided_nor_corrected() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);
    pay(conn, &book, bill_id).expect("pay");
    let correction = PostSimpleEntry {
        entity_id: book.entity_id,
        accounts: SimpleEntryAccounts::BillUnpaid {
            category: book.power,
            payable: book.bills_payable,
        },
        entry_date: common::date("2026-04-10"),
        description: "power".into(),
        reference: None,
        amount_minor: 4_500,
        settles_entry_id: None,
    };

    assert!(matches!(
        void_entry(conn, bill_id, Locale::En),
        Err(Error::Validation(ValidationError::BillSettled))
    ));
    assert!(matches!(
        replace_simple_entry(conn, bill_id, &correction, Locale::En),
        Err(Error::Validation(ValidationError::BillSettled))
    ));
    assert!(!get_entry(conn, bill_id).expect("get").is_voided);
}

#[test]
fn correcting_a_payment_settles_its_bill_again() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);
    let paid = pay(conn, &book, bill_id).expect("pay");

    // The correcting form offers the bill its payment settles.
    let offered: Vec<_> = list_open_bills(conn, book.entity_id, Some(paid.entry.id))
        .expect("open bills")
        .into_iter()
        .map(|open| open.entry_id)
        .collect();
    assert_eq!(offered, [bill_id]);

    let mut correction = PostSimpleEntry {
        entity_id: book.entity_id,
        accounts: payment(&book, book.bills_payable),
        entry_date: common::date("2026-04-12"),
        description: "power, paid".into(),
        reference: None,
        amount_minor: 4_000,
        settles_entry_id: Some(bill_id),
    };
    let replacement =
        replace_simple_entry(conn, paid.entry.id, &correction, Locale::En).expect("correct");

    assert_eq!(replacement.settles_entry_id, Some(bill_id));
    assert_eq!(open_ids(conn, &book), []);

    // A correction that pays another amount is refused, and changes nothing.
    correction.amount_minor = 3_000;
    assert!(matches!(
        replace_simple_entry(conn, replacement.entry.id, &correction, Locale::En),
        Err(Error::Validation(ValidationError::PaymentDoesNotMatchBill))
    ));
    assert_eq!(open_ids(conn, &book), []);
}

#[test]
fn a_payment_must_settle_an_open_bill_of_its_book_in_full() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);
    let on_card = bill(conn, &book, book.card, 4_000);
    let other = self::book(conn, "Shop");
    let elsewhere = bill(conn, &other, other.bills_payable, 4_000);
    let from_checking = payment(&book, book.bills_payable);

    let refusal = |accounts, amount_minor, bill| {
        post(conn, &book, accounts, amount_minor, Some(bill)).map(|_| ())
    };

    assert!(matches!(
        refusal(from_checking, 3_999, bill_id),
        Err(Error::Validation(ValidationError::PaymentDoesNotMatchBill))
    ));
    // Owed on the card, paid as if owed on bills payable.
    assert!(matches!(
        refusal(from_checking, 4_000, on_card),
        Err(Error::Validation(ValidationError::PaymentDoesNotMatchBill))
    ));
    assert!(matches!(
        refusal(from_checking, 4_000, elsewhere),
        Err(Error::Validation(ValidationError::WrongBook))
    ));
    assert!(matches!(
        refusal(from_checking, 4_000, JournalEntryId::generate()),
        Err(Error::NotFound(_))
    ));

    pay(conn, &book, bill_id).expect("pay once");
    assert!(matches!(
        pay(conn, &book, bill_id),
        Err(Error::Validation(ValidationError::BillNotOpen))
    ));
    // The two bills and the one payment; no refused payment was posted.
    assert_eq!(entry_count(conn, &book), 3);
    assert_eq!(open_ids(conn, &book), [on_card]);
}

#[test]
fn correcting_one_payment_offers_its_bill_and_no_other() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let first = bill(conn, &book, book.bills_payable, 4_000);
    let second = bill(conn, &book, book.bills_payable, 4_000);
    let first_payment = pay(conn, &book, first).expect("pay the first");
    pay(conn, &book, second).expect("pay the second");

    let offered: Vec<_> = list_open_bills(conn, book.entity_id, Some(first_payment.entry.id))
        .expect("open bills")
        .into_iter()
        .map(|open| open.entry_id)
        .collect();

    assert_eq!(offered, [first]);
}

#[test]
fn correcting_a_payment_into_one_of_no_bill_reopens_the_bill() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);
    let paid = pay(conn, &book, bill_id).expect("pay");
    let unlinked = PostSimpleEntry {
        entity_id: book.entity_id,
        accounts: payment(&book, book.bills_payable),
        entry_date: common::date("2026-04-12"),
        description: "power".into(),
        reference: None,
        amount_minor: 4_000,
        settles_entry_id: None,
    };

    replace_simple_entry(conn, paid.entry.id, &unlinked, Locale::En).expect("correct");

    assert_eq!(open_ids(conn, &book), [bill_id]);
}

#[test]
fn a_bill_owed_on_an_archived_account_is_not_offered() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let on_card = bill(conn, &book, book.card, 4_000);
    let on_payable = bill(conn, &book, book.bills_payable, 4_000);

    archive_account(conn, book.card).expect("archive the card");

    // No payment could debit the archived card until it is restored.
    assert_eq!(open_ids(conn, &book), [on_payable]);
    assert!(matches!(
        post(conn, &book, payment(&book, book.card), 4_000, Some(on_card)),
        Err(Error::Validation(ValidationError::AccountInactive { .. }))
    ));
}

#[test]
fn a_bill_named_on_an_entry_that_is_not_a_payment_is_not_read() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);
    let expense = SimpleEntryAccounts::Expense {
        category: book.power,
        wallet: book.checking,
    };

    // Another amount than the bill's: were the bill read, this would be refused.
    let posted = post(conn, &book, expense, 1_000, Some(bill_id)).expect("post");

    assert_eq!(posted.settles_entry_id, None);
    assert_eq!(open_ids(conn, &book), [bill_id]);
}

fn entry_count(conn: &Connection, book: &Book) -> usize {
    list_entries(conn, book.entity_id, &EntryFilter::default())
        .expect("entries")
        .len()
}

#[test]
fn a_payment_without_a_bill_still_posts() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);

    let unlinked = post(conn, &book, payment(&book, book.bills_payable), 4_000, None)
        .expect("a payment of no bill in particular");

    assert_eq!(unlinked.settles_entry_id, None);
    assert_eq!(open_ids(conn, &book), [bill_id]);
}

#[test]
fn only_a_payment_of_an_existing_bill_keeps_the_bill_it_names() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn, "Home");
    let bill_id = bill(conn, &book, book.bills_payable, 4_000);
    let request = PostSimpleEntryRequest {
        entity_id: book.entity_id,
        kind: SimpleEntryKind::Bill,
        bill_status: Some(SimpleBillStatus::Paid),
        entry_date: "2026-04-10".into(),
        description: "power".into(),
        reference: None,
        amount_minor: 4_000,
        category_account_id: Some(book.power),
        wallet_account_id: Some(book.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
        settles_entry_id: Some(bill_id),
    };

    let paid_now = common::post_simple_request(conn, &request).expect("post");

    assert_eq!(paid_now.settles_entry_id, None);
    assert_eq!(open_ids(conn, &book), [bill_id]);
}
