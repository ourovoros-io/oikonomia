//! The kind an entry is listed as: the kind it was posted as, stored since
//! schema v10, and for an entry without one, the kind its lines suggest.
//!
//! The list and the dashboard both read [`PostedEntryView::listed_kind`], so
//! each test checks what posting returns, what `get_entry` returns and what
//! `list_entries` returns.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::csv::post_import_rows;
use oikonomia_core::documents::{NewDocument, post_simple_entry_with_document};
use oikonomia_core::domain::{AccountId, AccountType, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::ledger::{
    CreateAccount, CreateRecurringTemplate, CreateRecurringTemplateRequest, EntryFilter,
    ListedEntryKind, PostSimpleEntry, PostSimpleEntryRequest, PostedEntryView, RecurringCadence,
    SimpleBillStatus, SimpleEntryAccounts, SimpleEntryKind, archive_account, create_account,
    create_recurring_template, get_entry, list_entries, post_recurring_template, post_simple_entry,
    replace_simple_entry, set_account_opening_balance, void_entry,
};
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
    /// The seeded Bills Payable, the book's default bill account.
    bills_payable: AccountId,
    /// A liability that is not the default bill account: the credit card.
    card: AccountId,
    /// Another liability that is not the default bill account.
    loan: AccountId,
}

fn book(conn: &Connection) -> Book {
    let entity_id = common::book(conn, "Kinds", ChartTemplate::Personal);
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
        loan: by_code("2100"),
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
    }
}

/// Every way the simple entry form can post, with the kind each must list as.
fn every_simple_entry(book: &Book) -> Vec<(SimpleEntryAccounts, ListedEntryKind)> {
    use SimpleEntryAccounts as Accounts;

    vec![
        (
            Accounts::Expense {
                category: book.food,
                wallet: book.checking,
            },
            ListedEntryKind::Expense,
        ),
        (
            Accounts::Income {
                category: book.salary,
                wallet: book.checking,
            },
            ListedEntryKind::Income,
        ),
        (
            Accounts::IncomeReceivable {
                category: book.salary,
                receivable: book.receivable,
            },
            ListedEntryKind::Income,
        ),
        (
            Accounts::BillPaid {
                category: book.food,
                wallet: book.checking,
            },
            ListedEntryKind::Bill,
        ),
        (
            Accounts::BillUnpaid {
                category: book.food,
                payable: book.bills_payable,
            },
            ListedEntryKind::Bill,
        ),
        (
            Accounts::BillPayment {
                payable: book.bills_payable,
                wallet: book.checking,
            },
            ListedEntryKind::Bill,
        ),
        (
            Accounts::Transfer {
                from: book.checking,
                to: book.savings,
            },
            ListedEntryKind::Transfer,
        ),
    ]
}

/// The kind `list_entries` reports for `id`.
fn listed_kind(conn: &Connection, entity_id: EntityId, id: JournalEntryId) -> ListedEntryKind {
    list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .into_iter()
        .find(|view| view.entry.id == id)
        .expect("the entry is listed")
        .listed_kind
}

/// The text stored in `journal_entries.kind` for `id`.
fn stored_kind(conn: &Connection, id: JournalEntryId) -> Option<String> {
    conn.query_row(
        "SELECT kind FROM journal_entries WHERE id = ?1",
        [id.to_string()],
        |row| row.get(0),
    )
    .expect("read the stored kind")
}

/// Fails unless posting, reading by id and listing all report `expected`.
#[track_caller]
fn assert_listed_as(conn: &Connection, posted: &PostedEntryView, expected: ListedEntryKind) {
    let id = posted.entry.id;
    assert_eq!(posted.listed_kind, expected, "as posted");
    assert_eq!(
        get_entry(conn, id).expect("get").listed_kind,
        expected,
        "by id"
    );
    assert_eq!(
        listed_kind(conn, posted.entry.entity_id, id),
        expected,
        "in the list"
    );
}

/// Puts an entry back on the shape an entry posted before v10 has.
fn forget_kind(conn: &Connection, id: JournalEntryId) {
    conn.execute(
        "UPDATE journal_entries SET kind = NULL WHERE id = ?1",
        [id.to_string()],
    )
    .expect("clear the stored kind");
}

#[test]
fn each_simple_entry_stores_and_lists_its_kind() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);

    for (accounts, expected) in every_simple_entry(&book) {
        let posted = post_simple_entry(conn, &simple(&book, accounts)).expect("post");

        assert_eq!(
            stored_kind(conn, posted.entry.id).as_deref(),
            Some(accounts.kind().identifier()),
            "{accounts:?}"
        );
        assert_listed_as(conn, &posted, expected);
    }
}

#[test]
fn a_bill_paid_at_once_lists_as_a_bill_not_an_expense() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let paid = SimpleEntryAccounts::BillPaid {
        category: book.food,
        wallet: book.checking,
    };

    let posted = post_simple_entry(conn, &simple(&book, paid)).expect("post");

    assert_listed_as(conn, &posted, ListedEntryKind::Bill);
}

#[test]
fn a_correction_stores_the_kind_of_the_replacement() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let expense = SimpleEntryAccounts::Expense {
        category: book.food,
        wallet: book.checking,
    };
    let bill = SimpleEntryAccounts::BillPaid {
        category: book.food,
        wallet: book.checking,
    };
    let original = post_simple_entry(conn, &simple(&book, expense)).expect("post");

    let replacement =
        replace_simple_entry(conn, original.entry.id, &simple(&book, bill), Locale::En)
            .expect("replace");

    assert_listed_as(conn, &replacement, ListedEntryKind::Bill);
}

#[test]
fn a_recurring_bill_stores_its_kind() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let request = CreateRecurringTemplateRequest {
        entity_id: book.entity_id,
        name: "Power".into(),
        kind: SimpleEntryKind::Bill,
        bill_status: Some(SimpleBillStatus::Paid),
        amount_minor: 9_900,
        cadence: RecurringCadence::Monthly,
        day_of_month: Some(10),
        category_account_id: Some(book.food),
        wallet_account_id: Some(book.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
        memo: None,
        next_date: "2026-03-10".into(),
    };
    let template =
        create_recurring_template(conn, &common::strict::<_, CreateRecurringTemplate>(request))
            .expect("template");

    let posted = post_recurring_template(conn, template.id, None, None).expect("post");

    assert_listed_as(conn, &posted.entry, ListedEntryKind::Bill);
}

#[test]
fn an_imported_row_and_an_entry_with_a_document_store_their_kind() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let row = PostSimpleEntryRequest {
        entity_id: book.entity_id,
        kind: SimpleEntryKind::Bill,
        bill_status: Some(SimpleBillStatus::Paid),
        entry_date: "2026-03-05".into(),
        description: "imported".into(),
        reference: None,
        amount_minor: 1_200,
        category_account_id: Some(book.food),
        wallet_account_id: Some(book.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    };
    let receipt = NewDocument {
        filename: "receipt.txt",
        mime_type: "text/plain",
        data: b"TOTAL 10,00 EUR",
    };
    let paid = SimpleEntryAccounts::BillPaid {
        category: book.food,
        wallet: book.savings,
    };

    let imported = post_import_rows(conn, &[row], false).expect("import");
    let (with_document, _) =
        post_simple_entry_with_document(conn, &simple(&book, paid), &receipt, None)
            .expect("post with document");

    assert_listed_as(conn, &imported.posted[0], ListedEntryKind::Bill);
    assert_listed_as(conn, &with_document, ListedEntryKind::Bill);
}

#[test]
fn the_reversal_of_a_paid_bill_is_listed_as_a_bill_too() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let paid = SimpleEntryAccounts::BillPaid {
        category: book.food,
        wallet: book.checking,
    };
    let original = post_simple_entry(conn, &simple(&book, paid)).expect("post");

    let voided = void_entry(conn, original.entry.id, Locale::En).expect("void");

    assert_eq!(
        stored_kind(conn, voided.reverse_id).as_deref(),
        Some("bill")
    );
    let reverse = get_entry(conn, voided.reverse_id).expect("get");
    assert_eq!(reverse.listed_kind, ListedEntryKind::Bill);
}

#[test]
fn an_archived_payable_keeps_its_bills_and_does_not_make_the_card_one() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let unpaid = SimpleEntryAccounts::BillUnpaid {
        category: book.food,
        payable: book.bills_payable,
    };
    let on_card = SimpleEntryAccounts::Expense {
        category: book.food,
        wallet: book.card,
    };
    let bill = post_simple_entry(conn, &simple(&book, unpaid)).expect("post");
    let card = post_simple_entry(conn, &simple(&book, on_card)).expect("post");
    forget_kind(conn, bill.entry.id);
    forget_kind(conn, card.entry.id);

    // With Bills Payable archived, the entry form's default for the part
    // falls back to the first liability, the card.
    archive_account(conn, book.bills_payable).expect("archive");

    assert_eq!(
        listed_kind(conn, book.entity_id, bill.entry.id),
        ListedEntryKind::Bill
    );
    assert_eq!(
        listed_kind(conn, book.entity_id, card.entry.id),
        ListedEntryKind::Expense
    );
}

#[test]
fn a_blank_book_has_no_bill_account_but_its_recurring_bills() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Blank", ChartTemplate::Blank);
    for (code, account_type) in [
        ("2000", AccountType::Liability),
        ("1000", AccountType::Asset),
        ("5000", AccountType::Expense),
    ] {
        let input = CreateAccount {
            entity_id,
            code: code.into(),
            name: code.into(),
            account_type,
            sort_order: None,
        };
        create_account(conn, &input).expect("account");
    }

    let card_expense = common::post_two_line(conn, entity_id, "2026-03-02", ("5000", "2000"), 900);

    assert_listed_as(conn, &card_expense, ListedEntryKind::Expense);
}

#[test]
fn manual_entries_reversals_and_opening_balances_store_no_kind() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let manual = common::post_two_line(conn, book.entity_id, "2026-03-02", ("5100", "1010"), 900);
    let voided = void_entry(conn, manual.entry.id, Locale::En).expect("void");
    let opening = set_account_opening_balance(
        conn,
        book.checking,
        50_000,
        common::date("2026-01-01"),
        Locale::En,
    )
    .expect("opening balance");

    for id in [manual.entry.id, voided.reverse_id, opening.entry.id] {
        assert_eq!(stored_kind(conn, id), None);
    }
    assert_listed_as(conn, &manual, ListedEntryKind::Expense);
    assert_listed_as(conn, &opening, ListedEntryKind::Other);
}

#[test]
fn an_entry_without_a_kind_is_classified_from_its_lines() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);

    // What the lines say for each kind: a bill paid at once has the lines of
    // an expense, and the rest can be told apart.
    let expected_from_lines = [
        ListedEntryKind::Expense,
        ListedEntryKind::Income,
        ListedEntryKind::Income,
        ListedEntryKind::Expense,
        ListedEntryKind::Bill,
        ListedEntryKind::Bill,
        ListedEntryKind::Transfer,
    ];
    for ((accounts, _), expected) in every_simple_entry(&book)
        .into_iter()
        .zip(expected_from_lines)
    {
        let posted = post_simple_entry(conn, &simple(&book, accounts)).expect("post");
        forget_kind(conn, posted.entry.id);

        let read = get_entry(conn, posted.entry.id).expect("get");

        assert_eq!(read.listed_kind, expected, "{accounts:?}");
        assert_eq!(
            listed_kind(conn, book.entity_id, posted.entry.id),
            expected,
            "{accounts:?}"
        );
    }
}

#[test]
fn the_payable_of_a_recurring_bill_counts_as_a_bill_account_for_older_entries() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let on_loan = SimpleEntryAccounts::BillUnpaid {
        category: book.food,
        payable: book.loan,
    };
    let on_card = SimpleEntryAccounts::BillUnpaid {
        category: book.food,
        payable: book.card,
    };
    let loan_entry = post_simple_entry(conn, &simple(&book, on_loan)).expect("post");
    let card_entry = post_simple_entry(conn, &simple(&book, on_card)).expect("post");
    forget_kind(conn, loan_entry.entry.id);
    forget_kind(conn, card_entry.entry.id);
    let request = CreateRecurringTemplateRequest {
        entity_id: book.entity_id,
        name: "Loan".into(),
        kind: SimpleEntryKind::Bill,
        bill_status: Some(SimpleBillStatus::Unpaid),
        amount_minor: 9_900,
        cadence: RecurringCadence::Monthly,
        day_of_month: Some(10),
        category_account_id: Some(book.food),
        wallet_account_id: None,
        payable_account_id: Some(book.loan),
        from_account_id: None,
        to_account_id: None,
        memo: None,
        next_date: "2026-03-10".into(),
    };

    // Before the template, the loan is just a liability, like the card.
    assert_eq!(
        listed_kind(conn, book.entity_id, loan_entry.entry.id),
        ListedEntryKind::Expense
    );
    create_recurring_template(conn, &common::strict::<_, CreateRecurringTemplate>(request))
        .expect("template");

    assert_eq!(
        listed_kind(conn, book.entity_id, loan_entry.entry.id),
        ListedEntryKind::Bill
    );
    assert_eq!(
        listed_kind(conn, book.entity_id, card_entry.entry.id),
        ListedEntryKind::Expense
    );
}

#[test]
fn the_schema_refuses_a_kind_that_is_not_a_simple_entry_kind() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let book = book(conn);
    let manual = common::post_two_line(conn, book.entity_id, "2026-03-02", ("5100", "1010"), 900);

    let refused = conn.execute(
        "UPDATE journal_entries SET kind = 'other' WHERE id = ?1",
        [manual.entry.id.to_string()],
    );

    assert!(refused.is_err(), "{refused:?}");
}
