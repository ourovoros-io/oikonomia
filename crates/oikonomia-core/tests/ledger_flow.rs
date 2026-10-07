//! End-to-end ledger flow against an encrypted vault.

mod common;

use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::error::{Error, ValidationError};
use oikonomia_core::ledger::{
    CreateEntity, balance_sheet, count_entities, create_entity, delete_entity, list_accounts,
    list_entities, profit_and_loss, trial_balance, void_entry,
};
use oikonomia_core::prefs::Locale;

#[test]
fn personal_books_expense_and_reports() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().unwrap();

    let entity_id = common::book(conn, "Personal", ChartTemplate::Personal);

    let accounts = list_accounts(conn, entity_id).unwrap();
    assert_ne!(accounts, [] as [oikonomia_core::domain::Account; 0]);

    let entry = common::post_two_line(conn, entity_id, "2026-03-15", ("5100", "1010"), 2_500);
    assert!(!entry.is_voided);

    let tb = trial_balance(conn, entity_id, "2026-03-31").unwrap();
    assert_eq!(tb.total_debits, tb.total_credits);
    assert!(tb.total_debits >= 2_500);

    let pnl = profit_and_loss(conn, entity_id, "2026-01-01", "2026-03-31").unwrap();
    assert_eq!(pnl.total_expenses, 2_500);
    assert_eq!(pnl.net_income, -2_500);

    let bs = balance_sheet(conn, entity_id, "2026-03-31").unwrap();
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);

    assert!(void_entry(conn, entry.entry.id, Locale::En).is_ok());

    let pnl2 = profit_and_loss(conn, entity_id, "2026-01-01", "2026-03-31").unwrap();
    assert_eq!(pnl2.total_expenses, 0);

    let entities = list_entities(conn).unwrap();
    assert_eq!(entities.len(), 1);

    // Duplicate name rejected (case-insensitive).
    let dup = create_entity(
        conn,
        &CreateEntity {
            name: "personal".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Blank,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    );
    assert_eq!(
        dup,
        Err(Error::Validation(ValidationError::NameTaken {
            name: "personal".into()
        }))
    );

    assert!(delete_entity(conn, entity_id).is_ok());
    assert_eq!(
        list_entities(conn).unwrap(),
        [] as [oikonomia_core::domain::Entity; 0]
    );
}

#[test]
fn a_vault_holds_any_number_of_entities() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().unwrap();

    for name in ["Personal", "Company", "Side project"] {
        let created = create_entity(
            conn,
            &CreateEntity {
                name: name.into(),
                base_currency: "EUR".into(),
                chart_template: ChartTemplate::Personal,
                fiscal_year_start_month: None,
            },
            Locale::En,
        );
        assert!(created.is_ok(), "creating {name} failed: {created:?}");
    }

    assert_eq!(count_entities(conn).ok(), Some(3));
}

#[test]
fn a_base_currency_is_three_ascii_letters_stored_in_capitals() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().unwrap();
    let book = |name: &str, base_currency: &str| {
        create_entity(
            conn,
            &CreateEntity {
                name: name.into(),
                base_currency: base_currency.into(),
                chart_template: ChartTemplate::Blank,
                fiscal_year_start_month: None,
            },
            Locale::En,
        )
        .map(|entity| entity.base_currency.to_string())
    };

    // "€" and "12$" are three bytes long; "ευρ" is three letters, not ASCII.
    for not_a_code in ["", "EU", "EURO", "12$", "€", "ευρ", "E R", "EU1"] {
        assert_eq!(
            book("Refused", not_a_code),
            Err(Error::Validation(ValidationError::CurrencyInvalid)),
            "{not_a_code:?}"
        );
    }

    assert_eq!(book("Lowercase", " eur "), Ok("EUR".to_owned()));
    assert_eq!(book("Capitals", "USD"), Ok("USD".to_owned()));
}
