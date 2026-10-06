//! Default accounts are chosen by template code, so a renamed chart and an
//! English one resolve the same roles. Runs against a real vault.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::default_accounts::default_accounts_for_entity;
use oikonomia_core::domain::{AccountType, ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateAccount, UpdateAccount, archive_account, create_account, list_accounts, update_account,
};
use rusqlite::Connection;

/// The code of each role's default, in the order of the response fields.
fn default_codes(conn: &Connection, entity_id: EntityId) -> Vec<Option<String>> {
    let defaults = default_accounts_for_entity(conn, entity_id).expect("defaults");
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let code_of = |id: Option<oikonomia_core::domain::AccountId>| {
        let id = id?;
        accounts.iter().find(|a| a.id == id).map(|a| a.code.clone())
    };

    vec![
        code_of(defaults.category),
        code_of(defaults.payment),
        code_of(defaults.deposit),
        code_of(defaults.income),
        code_of(defaults.bill_category),
        code_of(defaults.bills_payable),
        code_of(defaults.transfer_source),
        code_of(defaults.transfer_destination),
    ]
}

#[test]
fn renaming_every_stored_account_keeps_every_default() {
    for template in [ChartTemplate::Personal, ChartTemplate::Company] {
        let (_dir, vault) = common::vault();
        let conn = vault.connection().expect("conn");
        let entity_id = common::book(conn, "Defaults", template);

        let before = default_codes(conn, entity_id);

        for account in list_accounts(conn, entity_id).expect("accounts") {
            update_account(
                conn,
                &UpdateAccount {
                    id: account.id,
                    code: account.code.clone(),
                    name: format!("Λογαριασμός {}", account.code),
                    is_active: account.is_active,
                    sort_order: account.sort_order,
                },
            )
            .expect("rename");
        }

        assert_eq!(default_codes(conn, entity_id), before, "{template:?}");
        assert!(
            before.iter().all(Option::is_some),
            "{template:?}: {before:?}"
        );
    }
}

#[test]
fn archiving_the_seeded_wallet_moves_the_default_on() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Defaults", ChartTemplate::Personal);

    let checking = list_accounts(conn, entity_id)
        .expect("accounts")
        .into_iter()
        .find(|a| a.code == "1010")
        .expect("1010");
    archive_account(conn, checking.id).expect("archive");

    let codes = default_codes(conn, entity_id);

    // Payment is the second role; Cash is the template's next choice.
    assert_eq!(codes[1].as_deref(), Some("1000"));
}

#[test]
fn a_blank_book_defaults_to_the_users_own_accounts_by_type() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Defaults", ChartTemplate::Blank);

    assert_eq!(default_codes(conn, entity_id), vec![None; 8]);

    for (code, name, account_type) in [
        ("A1", "Λογαριασμός ταμείου", AccountType::Asset),
        ("E1", "Έξοδα", AccountType::Expense),
        ("L1", "Υποχρεώσεις", AccountType::Liability),
    ] {
        create_account(
            conn,
            &CreateAccount {
                entity_id,
                code: code.into(),
                name: name.into(),
                account_type,
                sort_order: None,
            },
        )
        .expect("account");
    }

    // category, payment, deposit, income, bill category, payable, from, to
    let expected = [
        Some("E1"),
        Some("A1"),
        Some("A1"),
        None,
        Some("E1"),
        Some("L1"),
        Some("A1"),
        // The only asset is already the source, so a transfer has no
        // destination to offer.
        None,
    ];
    let actual = default_codes(conn, entity_id);

    assert_eq!(
        actual.iter().map(Option::as_deref).collect::<Vec<_>>(),
        expected,
    );
}
