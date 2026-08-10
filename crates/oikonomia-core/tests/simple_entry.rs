//! The expense/income/bill/transfer form maps to journal lines in Rust,
//! not in the UI (review: business logic must live in the core crate).

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    CreateEntity, PostSimpleEntry, SimpleBillStatus, SimpleEntryKind, create_entity, list_accounts,
    post_simple_entry,
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

fn entity_with_accounts(conn: &Connection) -> (EntityId, AccountsByCode) {
    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "Simple".into(),
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
        AccountsByCode {
            checking: by_code("1010"),
            food: by_code("5100"),
            salary: by_code("4000"),
            bills_payable: by_code("2050"),
            savings: by_code("1020"),
        },
    )
}

struct AccountsByCode {
    checking: AccountId,
    food: AccountId,
    salary: AccountId,
    bills_payable: AccountId,
    savings: AccountId,
}

fn base_input(entity_id: EntityId, kind: SimpleEntryKind) -> PostSimpleEntry {
    PostSimpleEntry {
        entity_id,
        kind,
        bill_status: None,
        entry_date: "2026-03-15".into(),
        description: "simple".into(),
        reference: None,
        amount_minor: 2_500,
        category_account_id: None,
        wallet_account_id: None,
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    }
}

#[test]
fn expense_debits_category_credits_wallet() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let mut input = base_input(entity_id, SimpleEntryKind::Expense);
    input.category_account_id = Some(acc.food);
    input.wallet_account_id = Some(acc.checking);

    let view = post_simple_entry(conn, &input).expect("post expense");
    let debit = view
        .lines
        .iter()
        .find(|l| l.debit.amount_minor() > 0)
        .expect("debit line");
    let credit = view
        .lines
        .iter()
        .find(|l| l.credit.amount_minor() > 0)
        .expect("credit line");
    assert_eq!(debit.account_id, acc.food);
    assert_eq!(credit.account_id, acc.checking);
    assert_eq!(debit.debit.amount_minor(), 2_500);
}

#[test]
fn income_debits_wallet_credits_category() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let mut input = base_input(entity_id, SimpleEntryKind::Income);
    input.category_account_id = Some(acc.salary);
    input.wallet_account_id = Some(acc.checking);

    let view = post_simple_entry(conn, &input).expect("post income");
    let debit = view
        .lines
        .iter()
        .find(|l| l.debit.amount_minor() > 0)
        .expect("debit line");
    assert_eq!(debit.account_id, acc.checking);
}

#[test]
fn bill_statuses_route_to_payable() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let mut unpaid = base_input(entity_id, SimpleEntryKind::Bill);
    unpaid.bill_status = Some(SimpleBillStatus::Unpaid);
    unpaid.category_account_id = Some(acc.food);
    unpaid.payable_account_id = Some(acc.bills_payable);
    let view = post_simple_entry(conn, &unpaid).expect("post unpaid bill");
    let credit = view
        .lines
        .iter()
        .find(|l| l.credit.amount_minor() > 0)
        .expect("credit line");
    assert_eq!(credit.account_id, acc.bills_payable);

    let mut pay = base_input(entity_id, SimpleEntryKind::Bill);
    pay.bill_status = Some(SimpleBillStatus::PayExisting);
    pay.payable_account_id = Some(acc.bills_payable);
    pay.wallet_account_id = Some(acc.checking);
    let view = post_simple_entry(conn, &pay).expect("pay existing bill");
    let debit = view
        .lines
        .iter()
        .find(|l| l.debit.amount_minor() > 0)
        .expect("debit line");
    assert_eq!(debit.account_id, acc.bills_payable);

    let mut missing = base_input(entity_id, SimpleEntryKind::Bill);
    missing.category_account_id = Some(acc.food);
    missing.wallet_account_id = Some(acc.checking);
    assert!(
        matches!(post_simple_entry(conn, &missing), Err(Error::Validation(_))),
        "bill without bill_status must be rejected"
    );

    // Paying a bill from the payable account itself would fake a settlement.
    let mut circular = base_input(entity_id, SimpleEntryKind::Bill);
    circular.bill_status = Some(SimpleBillStatus::PayExisting);
    circular.payable_account_id = Some(acc.bills_payable);
    circular.wallet_account_id = Some(acc.bills_payable);
    assert!(
        matches!(
            post_simple_entry(conn, &circular),
            Err(Error::Validation(_))
        ),
        "same account on both sides must be rejected"
    );
}

#[test]
fn transfer_debits_to_credits_from() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let mut input = base_input(entity_id, SimpleEntryKind::Transfer);
    input.from_account_id = Some(acc.checking);
    input.to_account_id = Some(acc.savings);

    let view = post_simple_entry(conn, &input).expect("post transfer");
    let debit = view
        .lines
        .iter()
        .find(|l| l.debit.amount_minor() > 0)
        .expect("debit line");
    assert_eq!(debit.account_id, acc.savings);

    let mut same = base_input(entity_id, SimpleEntryKind::Transfer);
    same.from_account_id = Some(acc.checking);
    same.to_account_id = Some(acc.checking);
    assert!(
        matches!(post_simple_entry(conn, &same), Err(Error::Validation(_))),
        "transfer between the same account must be rejected"
    );
}

#[test]
fn wrong_role_types_and_bad_amounts_are_rejected() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    // Income account offered as an expense category.
    let mut wrong_type = base_input(entity_id, SimpleEntryKind::Expense);
    wrong_type.category_account_id = Some(acc.salary);
    wrong_type.wallet_account_id = Some(acc.checking);
    assert!(matches!(
        post_simple_entry(conn, &wrong_type),
        Err(Error::Validation(_))
    ));

    // Liability wallet for income (money received into a debt account).
    let mut liab_income = base_input(entity_id, SimpleEntryKind::Income);
    liab_income.category_account_id = Some(acc.salary);
    liab_income.wallet_account_id = Some(acc.bills_payable);
    assert!(matches!(
        post_simple_entry(conn, &liab_income),
        Err(Error::Validation(_))
    ));

    // Non-positive amount.
    let mut zero = base_input(entity_id, SimpleEntryKind::Expense);
    zero.category_account_id = Some(acc.food);
    zero.wallet_account_id = Some(acc.checking);
    zero.amount_minor = 0;
    assert!(matches!(
        post_simple_entry(conn, &zero),
        Err(Error::Validation(_))
    ));

    // Missing role entirely.
    let missing = base_input(entity_id, SimpleEntryKind::Expense);
    assert!(matches!(
        post_simple_entry(conn, &missing),
        Err(Error::Validation(_))
    ));
}
