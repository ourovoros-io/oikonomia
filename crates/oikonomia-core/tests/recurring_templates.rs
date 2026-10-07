//! Recurring templates: CRUD, due, post + advance, overrides, validation.

mod common;

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId};
use oikonomia_core::error::Error;
use oikonomia_core::error::ValidationError;
use oikonomia_core::ledger::{
    CreateEntity, CreateRecurringTemplate, CreateRecurringTemplateRequest, EntryFilter,
    RecurringCadence, SimpleBillStatus, SimpleEntryKind, UpdateRecurringTemplateRequest,
    archive_account, create_entity, create_recurring_template, delete_entity,
    delete_recurring_template, list_accounts, list_entries, list_recurring_templates,
    list_recurring_templates_as_of, post_recurring_template, update_recurring_template,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::util::parse_date;
use rusqlite::Connection;

struct AccountsByCode {
    checking: AccountId,
    food: AccountId,
    salary: AccountId,
    savings: AccountId,
}

fn entity_with_accounts(conn: &Connection) -> (EntityId, AccountsByCode) {
    let entity_id = common::book(conn, "Recurring", ChartTemplate::Personal);
    let by_code = |code: &str| common::account(conn, entity_id, code);

    (
        entity_id,
        AccountsByCode {
            checking: by_code("1010"),
            food: by_code("5100"),
            salary: by_code("4000"),
            savings: by_code("1020"),
        },
    )
}

fn monthly_rent(entity_id: EntityId, accounts: &AccountsByCode) -> CreateRecurringTemplate {
    common::strict(CreateRecurringTemplateRequest {
        entity_id,
        name: "Rent".into(),
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        amount_minor: 85_000,
        cadence: RecurringCadence::Monthly,
        day_of_month: Some(1),
        category_account_id: Some(accounts.food),
        wallet_account_id: Some(accounts.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
        memo: Some("apartment".into()),
        next_date: "2026-03-01".into(),
    })
}

#[test]
fn create_and_list_by_entity() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);

    let created =
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");
    assert_eq!(created.name, "Rent");
    assert_eq!(created.kind, SimpleEntryKind::Expense);
    assert_eq!(created.amount_minor, 85_000);
    assert_eq!(created.cadence, RecurringCadence::Monthly);
    assert_eq!(created.day_of_month, Some(1));
    assert_eq!(created.category_account_id, Some(accounts.food));
    assert_eq!(created.wallet_account_id, Some(accounts.checking));
    assert_eq!(created.memo.as_deref(), Some("apartment"));
    assert_eq!(created.next_date, parse_date("2026-03-01").expect("date"));

    let listed = list_recurring_templates(conn, entity_id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
}

#[test]
fn due_is_next_date_on_or_before_today() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);

    let mut past = monthly_rent(entity_id, &accounts);
    past.name = "Past".into();
    past.next_date = common::date("2026-03-01");
    create_recurring_template(conn, &past).expect("past");

    let mut today = monthly_rent(entity_id, &accounts);
    today.name = "Today".into();
    today.next_date = common::date("2026-03-10");
    create_recurring_template(conn, &today).expect("today");

    let mut future = monthly_rent(entity_id, &accounts);
    future.name = "Future".into();
    future.next_date = common::date("2026-03-11");
    create_recurring_template(conn, &future).expect("future");

    let as_of = parse_date("2026-03-10").expect("pin");
    let listed = list_recurring_templates_as_of(conn, entity_id, as_of).expect("list");
    assert_eq!(listed.len(), 3);
    let due_of = |name: &str| {
        listed
            .iter()
            .find(|row| row.name == name)
            .map(|row| row.due)
            .expect(name)
    };
    assert!(due_of("Past"));
    assert!(due_of("Today"));
    assert!(!due_of("Future"));
}

#[test]
fn post_creates_entry_and_advances_next_date() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let template =
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");

    let result = post_recurring_template(conn, template.id, None, None).expect("post");
    assert_eq!(result.entry.entry.description, "Rent");
    assert_eq!(
        result.entry.entry.entry_date,
        parse_date("2026-03-01").expect("date")
    );
    assert_eq!(result.entry.lines.len(), 2);
    assert_eq!(
        result.template.next_date,
        parse_date("2026-04-01").expect("advanced")
    );

    let entries = list_entries(conn, entity_id, &EntryFilter::default()).expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry.id, result.entry.entry.id);
}

#[test]
fn post_override_amount_and_date_do_not_rewrite_template_amount() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let template =
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");

    let result = post_recurring_template(
        conn,
        template.id,
        Some(common::date("2026-03-05")),
        Some(90_000),
    )
    .expect("post override");

    assert_eq!(
        result.entry.entry.entry_date,
        parse_date("2026-03-05").expect("override date")
    );
    let posted_amount: i64 = result
        .entry
        .lines
        .iter()
        .map(|line| line.debit.amount_minor())
        .sum();
    assert_eq!(posted_amount, 90_000);
    assert_eq!(
        result.template.amount_minor, 85_000,
        "template amount stays"
    );
    assert_eq!(
        result.template.next_date,
        parse_date("2026-04-01").expect("advance from stored next_date, not override")
    );
}

#[test]
fn weekly_post_advances_by_seven_days() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let mut weekly = monthly_rent(entity_id, &accounts);
    weekly.name = "Allowance".into();
    weekly.cadence = RecurringCadence::Weekly;
    weekly.day_of_month = None;
    weekly.next_date = common::date("2026-03-10");
    weekly.amount_minor = 2_000;
    let template = create_recurring_template(conn, &weekly).expect("create");

    let result = post_recurring_template(conn, template.id, None, None).expect("post");
    assert_eq!(
        result.template.next_date,
        parse_date("2026-03-17").expect("plus 7")
    );
}

#[test]
fn delete_removes_template_not_posted_entry() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let template =
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");
    post_recurring_template(conn, template.id, None, None).expect("post");

    delete_recurring_template(conn, template.id).expect("delete");
    let listed = list_recurring_templates(conn, entity_id).expect("list");
    assert!(listed.is_empty());
    let entries = list_entries(conn, entity_id, &EntryFilter::default()).expect("entries");
    assert_eq!(entries.len(), 1);
}

#[test]
fn update_rewrites_fields() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let template =
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");

    let updated = update_recurring_template(
        conn,
        &common::strict(UpdateRecurringTemplateRequest {
            id: template.id,
            name: "Groceries".into(),
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            amount_minor: 12_000,
            cadence: RecurringCadence::Weekly,
            day_of_month: None,
            category_account_id: Some(accounts.food),
            wallet_account_id: Some(accounts.checking),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
            memo: None,
            next_date: "2026-03-12".into(),
        }),
    )
    .expect("update");
    assert_eq!(updated.name, "Groceries");
    assert_eq!(updated.amount_minor, 12_000);
    assert_eq!(updated.cadence, RecurringCadence::Weekly);
    assert_eq!(updated.day_of_month, None);
}

#[test]
fn validation_rejects_empty_name_and_non_positive_amount() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);

    let mut nameless = monthly_rent(entity_id, &accounts);
    nameless.name = "   ".into();
    assert!(matches!(
        create_recurring_template(conn, &nameless),
        Err(Error::Validation(ValidationError::NameRequired { .. }))
    ));

    let mut zero = monthly_rent(entity_id, &accounts);
    zero.amount_minor = 0;
    assert!(matches!(
        create_recurring_template(conn, &zero),
        Err(Error::Validation(ValidationError::AmountNotPositive))
    ));
}

#[test]
fn validation_monthly_needs_day_of_month() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let mut missing = monthly_rent(entity_id, &accounts);
    missing.day_of_month = None;
    assert!(matches!(
        create_recurring_template(conn, &missing),
        Err(Error::Validation(ValidationError::DayOfMonthInvalid))
    ));

    let mut weekly_with_day = monthly_rent(entity_id, &accounts);
    weekly_with_day.cadence = RecurringCadence::Weekly;
    weekly_with_day.day_of_month = Some(10);
    assert!(matches!(
        create_recurring_template(conn, &weekly_with_day),
        Err(Error::Validation(ValidationError::DayOfMonthInvalid))
    ));
}

#[test]
fn validation_requires_role_accounts_for_kind() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let mut missing_wallet = monthly_rent(entity_id, &accounts);
    missing_wallet.wallet_account_id = None;
    assert!(matches!(
        create_recurring_template(conn, &missing_wallet),
        Err(Error::Validation(ValidationError::AccountRequired { .. }))
    ));
}

#[test]
fn transfer_template_posts() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let template = create_recurring_template(
        conn,
        &common::strict(CreateRecurringTemplateRequest {
            entity_id,
            name: "Savings sweep".into(),
            kind: SimpleEntryKind::Transfer,
            bill_status: None,
            amount_minor: 10_000,
            cadence: RecurringCadence::Monthly,
            day_of_month: Some(28),
            category_account_id: None,
            wallet_account_id: None,
            payable_account_id: None,
            from_account_id: Some(accounts.checking),
            to_account_id: Some(accounts.savings),
            memo: None,
            next_date: "2026-03-28".into(),
        }),
    )
    .expect("create");
    let result = post_recurring_template(conn, template.id, None, None).expect("post");
    assert_eq!(result.entry.lines.len(), 2);
}

#[test]
fn income_template_and_bill_status() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let salary = create_recurring_template(
        conn,
        &common::strict(CreateRecurringTemplateRequest {
            entity_id,
            name: "Salary".into(),
            kind: SimpleEntryKind::Income,
            bill_status: None,
            amount_minor: 200_000,
            cadence: RecurringCadence::Monthly,
            day_of_month: Some(25),
            category_account_id: Some(accounts.salary),
            wallet_account_id: Some(accounts.checking),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
            memo: None,
            next_date: "2026-03-25".into(),
        }),
    )
    .expect("income");
    assert_eq!(salary.kind, SimpleEntryKind::Income);

    let mut bill = monthly_rent(entity_id, &accounts);
    bill.name = "Phone".into();
    bill.kind = SimpleEntryKind::Bill;
    bill.bill_status = Some(SimpleBillStatus::Paid);
    let created = create_recurring_template(conn, &bill).expect("bill");
    assert_eq!(created.bill_status, Some(SimpleBillStatus::Paid));
}

#[test]
fn deleting_entity_removes_templates() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");

    // Read from the table itself: the listing would need the book to exist.
    let template_rows = || -> i64 {
        conn.query_row(
            "SELECT COUNT(1) FROM recurring_templates WHERE entity_id = ?1",
            [entity_id.to_string()],
            |row| row.get(0),
        )
        .expect("count templates")
    };
    assert_eq!(template_rows(), 1);

    delete_entity(conn, entity_id).expect("delete entity");

    assert_eq!(template_rows(), 0);
}

#[test]
fn post_rejects_non_positive_override_amount() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);
    let template =
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");
    assert!(matches!(
        post_recurring_template(conn, template.id, None, Some(0)),
        Err(Error::Validation(ValidationError::AmountNotPositive))
    ));
}
/// A template is checked like the entry it will post, so a template that
/// could never be posted is refused when it is saved, not at post time.
#[test]
fn a_template_must_use_accounts_that_posting_would_accept() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, accounts) = entity_with_accounts(conn);

    let other_book = create_entity(
        conn,
        &CreateEntity {
            name: "Other book".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("other entity");
    let foreign_checking = list_accounts(conn, other_book.id)
        .expect("accounts")
        .iter()
        .find(|account| account.code == "1010")
        .map(|account| account.id)
        .expect("1010");

    let mut another_books_account = monthly_rent(entity_id, &accounts);
    another_books_account.wallet_account_id = Some(foreign_checking);
    assert_eq!(
        create_recurring_template(conn, &another_books_account).map(|template| template.id),
        Err(Error::AccountWrongEntity)
    );

    let mut same_account_twice = monthly_rent(entity_id, &accounts);
    same_account_twice.kind = SimpleEntryKind::Transfer;
    same_account_twice.category_account_id = None;
    same_account_twice.wallet_account_id = None;
    same_account_twice.from_account_id = Some(accounts.checking);
    same_account_twice.to_account_id = Some(accounts.checking);
    assert_eq!(
        create_recurring_template(conn, &same_account_twice).map(|template| template.id),
        Err(Error::Validation(ValidationError::SameAccount))
    );

    let saved =
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts)).expect("create");
    archive_account(conn, accounts.food).expect("archive");
    let archived = Err(Error::Validation(ValidationError::AccountInactive {
        code: "5100".into(),
    }));
    assert_eq!(
        create_recurring_template(conn, &monthly_rent(entity_id, &accounts))
            .map(|template| template.id),
        archived
    );

    let unchanged = common::strict(UpdateRecurringTemplateRequest {
        id: saved.id,
        name: saved.name,
        kind: saved.kind,
        bill_status: saved.bill_status,
        amount_minor: saved.amount_minor,
        cadence: saved.cadence,
        day_of_month: saved.day_of_month,
        category_account_id: saved.category_account_id,
        wallet_account_id: saved.wallet_account_id,
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
        memo: None,
        next_date: "2026-03-01".into(),
    });
    assert_eq!(
        update_recurring_template(conn, &unchanged).map(|template| template.id),
        archived
    );
}
