//! Editing posted entries (void + repost in one transaction) and setting
//! opening balances against equity — the accounting stays in the core crate.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::documents::{attach_document, list_documents};
use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId};
use oikonomia_core::error::Error;
use oikonomia_core::error::ValidationError;
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntry, SimpleEntryKind, UpdateAccount, account_balance,
    archive_account, create_entity, list_accounts, list_entries, post_simple_entry,
    replace_simple_entry, set_account_opening_balance, trial_balance, update_account, void_entry,
};
use oikonomia_core::prefs::Locale;
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
    bills_payable: AccountId,
}

fn entity_with_accounts(conn: &Connection) -> (EntityId, Accounts) {
    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "Edits".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
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
            bills_payable: by_code("2050"),
        },
    )
}

fn expense(entity_id: EntityId, acc: &Accounts, amount_minor: i64) -> PostSimpleEntry {
    PostSimpleEntry {
        entity_id,
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        entry_date: "2026-03-15".into(),
        description: "groceries".into(),
        reference: None,
        amount_minor,
        category_account_id: Some(acc.food),
        wallet_account_id: Some(acc.checking),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    }
}

#[test]
fn replace_updates_amount_and_hides_original() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let original = post_simple_entry(conn, &expense(entity_id, &acc, 2_500)).expect("post");

    let mut corrected = expense(entity_id, &acc, 3_100);
    corrected.description = "groceries (corrected)".into();
    let replacement =
        replace_simple_entry(conn, original.entry.id, &corrected, Locale::En).expect("replace");

    let visible: Vec<_> = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .into_iter()
        .filter(|v| !v.is_voided)
        .collect();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].entry.id, replacement.entry.id);
    assert_eq!(visible[0].entry.description, "groceries (corrected)");

    // The books only carry the corrected amount: original + reverse cancel out.
    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("trial balance");
    assert_eq!(tb.total_debits, tb.total_credits);
    assert_eq!(
        account_balance(conn, acc.checking, "2026-12-31").expect("balance"),
        -3_100
    );
}

#[test]
fn replace_moves_documents_to_replacement() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let original = post_simple_entry(conn, &expense(entity_id, &acc, 2_500)).expect("post");
    attach_document(
        conn,
        entity_id,
        original.entry.id,
        "receipt.txt",
        "text/plain",
        b"total 25,00",
    )
    .expect("attach");

    let replacement = replace_simple_entry(
        conn,
        original.entry.id,
        &expense(entity_id, &acc, 2_600),
        Locale::En,
    )
    .expect("replace");

    let docs = list_documents(conn, entity_id).expect("docs");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].entry_id, replacement.entry.id);
}

#[test]
fn replace_rejects_voided_and_foreign_entries() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    let voided = post_simple_entry(conn, &expense(entity_id, &acc, 2_500)).expect("post");
    void_entry(conn, voided.entry.id, Locale::En).expect("void");
    assert!(matches!(
        replace_simple_entry(
            conn,
            voided.entry.id,
            &expense(entity_id, &acc, 2_600),
            Locale::En
        ),
        Err(Error::Validation(ValidationError::EntryAlreadyVoided))
    ));

    let original = post_simple_entry(conn, &expense(entity_id, &acc, 2_500)).expect("post");
    let other = create_entity(
        conn,
        &CreateEntity {
            name: "Other".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("other entity");
    let mut foreign = expense(entity_id, &acc, 2_600);
    foreign.entity_id = other.id;
    assert!(matches!(
        replace_simple_entry(conn, original.entry.id, &foreign, Locale::En),
        Err(Error::Validation(ValidationError::WrongBook))
    ));
}

#[test]
fn opening_balance_converges_on_the_stated_target() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);

    set_account_opening_balance(conn, acc.checking, 250_000, "2026-01-01", Locale::En)
        .expect("set");
    assert_eq!(
        account_balance(conn, acc.checking, "2026-01-01").expect("balance"),
        250_000
    );

    // Restating the balance posts only the delta, not another full amount.
    set_account_opening_balance(conn, acc.checking, 300_000, "2026-01-02", Locale::En)
        .expect("restate");
    assert_eq!(
        account_balance(conn, acc.checking, "2026-01-02").expect("balance"),
        300_000
    );

    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("trial balance");
    assert_eq!(tb.total_debits, tb.total_credits);
}

#[test]
fn opening_balance_handles_liability_negative_and_no_op_targets() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (_entity_id, acc) = entity_with_accounts(conn);

    // Liability: "I owe 500" is a credit-normal balance.
    set_account_opening_balance(conn, acc.bills_payable, 50_000, "2026-01-01", Locale::En)
        .expect("owe");
    assert_eq!(
        account_balance(conn, acc.bills_payable, "2026-01-01").expect("balance"),
        50_000
    );

    // An overdrawn asset target flips the entry sides.
    set_account_opening_balance(conn, acc.checking, -10_000, "2026-01-01", Locale::En)
        .expect("overdrawn");
    assert_eq!(
        account_balance(conn, acc.checking, "2026-01-01").expect("balance"),
        -10_000
    );

    // Stating the balance it already has is refused, not silently duplicated.
    assert!(matches!(
        set_account_opening_balance(conn, acc.bills_payable, 50_000, "2026-01-01", Locale::En),
        Err(Error::Validation(ValidationError::OpeningBalanceUnchanged))
    ));
}

#[test]
fn update_account_cannot_deactivate_system_accounts() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, _acc) = entity_with_accounts(conn);
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let system = accounts
        .iter()
        .find(|a| a.is_system)
        .expect("opening balances");

    let err = update_account(
        conn,
        &UpdateAccount {
            id: system.id,
            code: system.code.clone(),
            name: system.name.clone(),
            is_active: false,
            sort_order: system.sort_order,
        },
    )
    .expect_err("system deactivate");
    assert_eq!(
        err,
        Error::Validation(ValidationError::SystemAccountProtected)
    );
    let after = list_accounts(conn, entity_id)
        .expect("reload")
        .into_iter()
        .find(|a| a.id == system.id)
        .expect("still there");
    assert!(after.is_active, "system account must stay active");
}
fn account_by_code(conn: &Connection, entity_id: EntityId, code: &str) -> AccountId {
    list_accounts(conn, entity_id)
        .expect("accounts")
        .iter()
        .find(|account| account.code == code)
        .map(|account| account.id)
        .expect(code)
}

#[test]
fn an_entry_on_an_archived_account_can_still_be_voided() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);
    let posted = post_simple_entry(conn, &expense(entity_id, &acc, 4_200)).expect("post");
    archive_account(conn, acc.food).expect("archive");

    let voided = void_entry(conn, posted.entry.id, Locale::En).expect("void");

    assert_eq!(voided.original_id, posted.entry.id);
    assert_eq!(account_balance(conn, acc.food, "2026-12-31"), Ok(0));
    assert_eq!(account_balance(conn, acc.checking, "2026-12-31"), Ok(0));
}

#[test]
fn an_entry_on_an_archived_account_can_be_moved_to_an_active_one() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);
    let other_expenses = account_by_code(conn, entity_id, "5900");
    let posted = post_simple_entry(conn, &expense(entity_id, &acc, 4_200)).expect("post");
    archive_account(conn, acc.food).expect("archive");

    let mut moved = expense(entity_id, &acc, 4_200);
    moved.category_account_id = Some(other_expenses);
    replace_simple_entry(conn, posted.entry.id, &moved, Locale::En).expect("replace");

    assert_eq!(account_balance(conn, acc.food, "2026-12-31"), Ok(0));
    assert_eq!(
        account_balance(conn, other_expenses, "2026-12-31"),
        Ok(4_200)
    );
}

#[test]
fn a_replacement_cannot_post_to_an_archived_account_and_leaves_the_original() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, acc) = entity_with_accounts(conn);
    let posted = post_simple_entry(conn, &expense(entity_id, &acc, 4_200)).expect("post");
    archive_account(conn, acc.food).expect("archive");

    let still_on_food = expense(entity_id, &acc, 9_900);
    let refused = replace_simple_entry(conn, posted.entry.id, &still_on_food, Locale::En);

    assert_eq!(
        refused.map(|view| view.entry.id),
        Err(Error::Validation(ValidationError::AccountInactive {
            code: "5100".into()
        }))
    );
    let entries = list_entries(conn, entity_id, &EntryFilter::default()).expect("list");
    assert_eq!(entries.len(), 1, "the void was rolled back with the post");
    assert!(!entries[0].is_voided);
    assert_eq!(account_balance(conn, acc.food, "2026-12-31"), Ok(4_200));
}
