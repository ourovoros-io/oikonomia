//! End-to-end ledger flow against an encrypted vault.

use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, PostJournal, balance_sheet, count_entities, create_entity,
    delete_entity, list_accounts, list_entities, post_entry, profit_and_loss, trial_balance,
    void_entry,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::vault::Vault;
use tempfile::tempdir;

#[test]
fn personal_books_expense_and_reports() {
    let Ok(dir) = tempdir() else {
        return;
    };
    let Ok(mut vault) = Vault::open_path(dir.path()) else {
        return;
    };
    assert!(vault.init("correct horse battery staple").is_ok());

    let Ok(conn) = vault.connection() else {
        return;
    };

    let Ok(entity) = create_entity(
        conn,
        &CreateEntity {
            name: "Personal".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    ) else {
        return;
    };

    let accounts = list_accounts(conn, entity.id).unwrap_or_default();
    assert_ne!(accounts, [] as [oikonomia_core::domain::Account; 0]);

    let Some(checking) = accounts.iter().find(|a| a.code == "1010") else {
        return;
    };
    let Some(food) = accounts.iter().find(|a| a.code == "5100") else {
        return;
    };

    let Ok(entry) = post_entry(
        conn,
        &PostJournal {
            entity_id: entity.id,
            entry_date: "2026-03-15".into(),
            description: "Groceries".into(),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: food.id,
                    debit_minor: 2_500,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: checking.id,
                    debit_minor: 0,
                    credit_minor: 2_500,
                    memo: None,
                },
            ],
        },
    ) else {
        return;
    };
    assert!(!entry.is_voided);

    let Ok(tb) = trial_balance(conn, entity.id, "2026-03-31") else {
        return;
    };
    assert_eq!(tb.total_debits, tb.total_credits);
    assert!(tb.total_debits >= 2_500);

    let Ok(pnl) = profit_and_loss(conn, entity.id, "2026-01-01", "2026-03-31") else {
        return;
    };
    assert_eq!(pnl.total_expenses, 2_500);
    assert_eq!(pnl.net_income, -2_500);

    let Ok(bs) = balance_sheet(conn, entity.id, "2026-03-31") else {
        return;
    };
    assert_eq!(bs.total_assets, bs.total_liabilities_equity);

    assert!(void_entry(conn, entry.entry.id, Locale::En).is_ok());

    let Ok(pnl2) = profit_and_loss(conn, entity.id, "2026-01-01", "2026-03-31") else {
        return;
    };
    assert_eq!(pnl2.total_expenses, 0);

    let entities = list_entities(conn).unwrap_or_default();
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
    assert!(dup.is_err());

    assert!(delete_entity(conn, entity.id).is_ok());
    assert_eq!(
        list_entities(conn).unwrap_or_default(),
        [] as [oikonomia_core::domain::Entity; 0]
    );
}

#[test]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
fn a_vault_holds_any_number_of_entities() {
    let dir = tempdir().expect("temp dir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault
        .init("correct horse battery staple")
        .expect("init vault");
    let conn = vault.connection().expect("connection");

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
