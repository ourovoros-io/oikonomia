//! Text that core writes into the user's books is written in the language
//! the app is set to at that moment, and never rewritten afterwards. Runs
//! against a real vault.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use std::collections::BTreeSet;

use oikonomia_core::coa::template_accounts;
use oikonomia_core::default_accounts::default_accounts_for_entity;
use oikonomia_core::documents::parse_invoice_text;
use oikonomia_core::domain::{Account, AccountId, ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntry, SimpleEntryKind, create_entity, get_entry,
    list_accounts, list_entries, post_simple_entry, replace_simple_entry,
    set_account_opening_balance, void_entry,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

const LOCALES: [Locale; 4] = [Locale::En, Locale::El, Locale::Fr, Locale::De];

const TEMPLATES: [ChartTemplate; 2] = [ChartTemplate::Personal, ChartTemplate::Company];

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn new_entity(conn: &Connection, name: &str, template: ChartTemplate, locale: Locale) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: name.into(),
            base_currency: "EUR".into(),
            chart_template: template,
            fiscal_year_start_month: Some(1),
        },
        locale,
    )
    .expect("entity")
    .id
}

fn account_by_code(accounts: &[Account], code: &str) -> Account {
    accounts
        .iter()
        .find(|account| account.code == code)
        .cloned()
        .expect(code)
}

fn corpus_text(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/documents")
        .join(relative);

    std::fs::read_to_string(path).expect("corpus fixture")
}

#[test]
fn every_seeded_account_has_a_clean_unique_name_in_every_language() {
    for template in TEMPLATES {
        for locale in LOCALES {
            let accounts = template_accounts(template, locale);
            let mut seen = BTreeSet::new();

            for account in &accounts {
                assert!(
                    !account.name.is_empty(),
                    "{template:?} {} has no {locale:?} name",
                    account.code
                );
                assert_eq!(
                    account.name,
                    account.name.trim(),
                    "{template:?} {} {locale:?} has stray whitespace",
                    account.code
                );
                assert!(
                    !account.name.contains('\u{202f}'),
                    "{template:?} {} {locale:?} has a narrow no-break space",
                    account.code
                );
                assert!(
                    seen.insert(account.name),
                    "{template:?} {locale:?}: duplicate name {}",
                    account.name
                );
            }
        }
    }
}

#[test]
fn english_names_are_the_ones_seeded_before_localization() {
    let personal: Vec<&str> = template_accounts(ChartTemplate::Personal, Locale::En)
        .iter()
        .map(|account| account.name)
        .collect();
    assert_eq!(
        personal,
        [
            "Cash",
            "Checking",
            "Savings",
            "Investments",
            "Credit Card",
            "Bills Payable",
            "Loans",
            "Opening Balances",
            "Owner Equity",
            "Salary",
            "Freelance",
            "Interest",
            "Other Income",
            "Housing",
            "Food",
            "Transport",
            "Utilities",
            "Bills & services",
            "Health",
            "Subscriptions",
            "Entertainment",
            "Taxes",
            "Other",
        ]
    );

    let company: Vec<&str> = template_accounts(ChartTemplate::Company, Locale::En)
        .iter()
        .map(|account| account.name)
        .collect();
    assert_eq!(
        company,
        [
            "Cash",
            "Bank",
            "Accounts Receivable",
            "Equipment",
            "Accounts Payable",
            "Credit Card",
            "Loans",
            "Taxes Payable",
            "Opening Balances",
            "Owner Capital",
            "Retained Earnings",
            "Sales / Services",
            "Other Income",
            "COGS",
            "Payroll",
            "Rent",
            "Software",
            "Marketing",
            "Professional Fees",
            "Travel",
            "Taxes",
            "Other OpEx",
        ]
    );
}

#[test]
fn codes_types_order_and_flags_do_not_depend_on_the_language() {
    for template in TEMPLATES {
        let english = template_accounts(template, Locale::En);

        for locale in LOCALES {
            let localized = template_accounts(template, locale);

            assert_eq!(localized.len(), english.len());
            for (left, right) in localized.iter().zip(&english) {
                assert_eq!(left.code, right.code);
                assert_eq!(left.account_type, right.account_type);
                assert_eq!(left.is_system, right.is_system);
                assert_eq!(left.sort_order, right.sort_order);
            }
        }
    }
}

#[test]
fn a_greek_book_has_greek_names_and_the_same_codes_and_defaults() {
    for template in TEMPLATES {
        let (_dir, vault) = setup_vault();
        let conn = vault.connection().expect("conn");
        let english = new_entity(conn, "English", template, Locale::En);
        let greek = new_entity(conn, "Greek", template, Locale::El);

        let english_accounts = list_accounts(conn, english).expect("accounts");
        let greek_accounts = list_accounts(conn, greek).expect("accounts");

        let expected: Vec<&str> = template_accounts(template, Locale::El)
            .iter()
            .map(|account| account.name)
            .collect();
        let stored: Vec<&str> = greek_accounts
            .iter()
            .map(|account| account.name.as_str())
            .collect();
        assert_eq!(stored, expected);
        assert!(
            greek_accounts
                .iter()
                .any(|account| account.name == "Μετρητά")
        );

        let codes = |accounts: &[Account]| -> Vec<(String, String)> {
            accounts
                .iter()
                .map(|account| (account.code.clone(), format!("{:?}", account.account_type)))
                .collect()
        };
        assert_eq!(codes(&greek_accounts), codes(&english_accounts));

        let code_of = |accounts: &[Account], id: Option<AccountId>| {
            id.and_then(|id| accounts.iter().find(|a| a.id == id))
                .map(|account| account.code.clone())
        };
        let english_defaults = default_accounts_for_entity(conn, english).expect("defaults");
        let greek_defaults = default_accounts_for_entity(conn, greek).expect("defaults");

        for (left, right) in [
            (english_defaults.category, greek_defaults.category),
            (english_defaults.payment, greek_defaults.payment),
            (english_defaults.deposit, greek_defaults.deposit),
            (english_defaults.income, greek_defaults.income),
            (english_defaults.bill_category, greek_defaults.bill_category),
            (english_defaults.bills_payable, greek_defaults.bills_payable),
            (
                english_defaults.transfer_source,
                greek_defaults.transfer_source,
            ),
            (
                english_defaults.transfer_destination,
                greek_defaults.transfer_destination,
            ),
        ] {
            let english_code = code_of(&english_accounts, left);
            assert_ne!(english_code, None, "{template:?} english role unresolved");
            assert_eq!(code_of(&greek_accounts, right), english_code);
        }
    }
}

#[test]
fn the_opening_balance_description_is_written_in_the_given_language() {
    let expected = [
        (Locale::En, "Opening balance — Checking"),
        (Locale::El, "Υπόλοιπο έναρξης — Λογαριασμός όψεως"),
        (Locale::Fr, "Solde d'ouverture — Compte courant"),
        (Locale::De, "Anfangssaldo — Girokonto"),
    ];

    for (locale, description) in expected {
        let (_dir, vault) = setup_vault();
        let conn = vault.connection().expect("conn");
        let entity = new_entity(conn, "Book", ChartTemplate::Personal, locale);
        let accounts = list_accounts(conn, entity).expect("accounts");
        let checking = account_by_code(&accounts, "1010");

        let posted = set_account_opening_balance(conn, checking.id, 10_000, "2026-01-01", locale)
            .expect("opening balance");

        assert_eq!(posted.entry.description, description);
    }
}

#[test]
fn a_void_is_written_in_the_given_language() {
    let expected = [
        (Locale::En, "VOID: Groceries", "Void"),
        (Locale::El, "ΑΚΥΡΩΣΗ: Groceries", "Ακύρωση"),
        (Locale::Fr, "ANNULATION : Groceries", "Annulation"),
        (Locale::De, "STORNO: Groceries", "Storno"),
    ];

    for (locale, description, memo) in expected {
        let (_dir, vault) = setup_vault();
        let conn = vault.connection().expect("conn");
        let entity = new_entity(conn, "Book", ChartTemplate::Personal, locale);
        let accounts = list_accounts(conn, entity).expect("accounts");

        let posted = post_simple_entry(
            conn,
            &PostSimpleEntry {
                entity_id: entity,
                kind: SimpleEntryKind::Expense,
                bill_status: None,
                entry_date: "2026-02-01".into(),
                amount_minor: 1_250,
                description: "Groceries".into(),
                reference: None,
                category_account_id: Some(account_by_code(&accounts, "5100").id),
                wallet_account_id: Some(account_by_code(&accounts, "1010").id),
                payable_account_id: None,
                from_account_id: None,
                to_account_id: None,
            },
        )
        .expect("post");

        let voided = void_entry(conn, posted.entry.id, locale).expect("void");
        let reverse = get_entry(conn, voided.reverse_id).expect("reverse");

        assert_eq!(reverse.entry.description, description);
        for line in &reverse.lines {
            assert_eq!(line.memo.as_deref(), Some(memo));
        }
    }
}

#[test]
fn changing_the_language_renames_nothing_and_only_new_text_follows_it() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let greek = new_entity(conn, "Greek", ChartTemplate::Personal, Locale::El);

    let before: Vec<String> = list_accounts(conn, greek)
        .expect("accounts")
        .iter()
        .map(|account| account.name.clone())
        .collect();

    // The app is now English: a new book is English, the old one is not touched.
    let english = new_entity(conn, "English", ChartTemplate::Personal, Locale::En);
    let after: Vec<String> = list_accounts(conn, greek)
        .expect("accounts")
        .iter()
        .map(|account| account.name.clone())
        .collect();
    assert_eq!(after, before);

    let english_accounts = list_accounts(conn, english).expect("accounts");
    assert_eq!(account_by_code(&english_accounts, "1000").name, "Cash");

    // New generated text in the old Greek book follows the current language
    // and quotes the stored (Greek) account name as it is.
    let greek_accounts = list_accounts(conn, greek).expect("accounts");
    let checking = account_by_code(&greek_accounts, "1010");
    let posted = set_account_opening_balance(conn, checking.id, 5_000, "2026-01-01", Locale::En)
        .expect("opening balance");
    assert_eq!(
        posted.entry.description,
        "Opening balance — Λογαριασμός όψεως"
    );
    assert_eq!(checking.name, "Λογαριασμός όψεως");
}

#[test]
fn suggested_invoice_and_utility_descriptions_follow_the_language() {
    let invoice = "Επωνυμία ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ\n\
         Τιμολόγιο Παροχής / Ενδοκοινοτική Παροχή Υπηρεσιών\n\
         900000000000001 Επί πιστώσειB 51 25/06/2026\n\
         Στοιχεία Πελάτη\nΑ.Φ.Μ.: 000000000\nΕπωνυμία: ACME CONSULTING LTD\n\
         Πληρωτέο (€): 1860,00";
    let gas = corpus_text("synthetic/text/volton_myon_gas.txt");

    let expected = [
        (Locale::En, "Invoice", "Volton — Gas bill"),
        (
            Locale::El,
            "Τιμολόγιο",
            "Volton — Λογαριασμός φυσικού αερίου",
        ),
        (Locale::Fr, "Facture", "Volton — Facture de gaz"),
        (Locale::De, "Rechnung", "Volton — Gasrechnung"),
    ];

    for (locale, word, bill) in expected {
        let suggestion = parse_invoice_text(invoice, locale);
        let description = suggestion.description.expect("invoice description");
        assert!(
            description.starts_with(&format!("ACME CONSULTING LTD — {word}")),
            "{locale:?}: {description}"
        );

        let suggestion = parse_invoice_text(&gas, locale);
        assert_eq!(suggestion.description.as_deref(), Some(bill), "{locale:?}");
    }
}

#[test]
fn a_bank_transfer_suggestion_is_in_the_given_language_not_always_greek() {
    let text = corpus_text("synthetic/text/greek_bank_embasma.txt");

    let expected = [
        (Locale::En, "Bank transfer — HELIOS TRADING IKE"),
        (Locale::El, "Έμβασμα — HELIOS TRADING IKE"),
        (Locale::Fr, "Virement bancaire — HELIOS TRADING IKE"),
        (Locale::De, "Überweisung — HELIOS TRADING IKE"),
    ];

    for (locale, description) in expected {
        let suggestion = parse_invoice_text(&text, locale);

        assert_eq!(suggestion.description.as_deref(), Some(description));
        // The payee comes from the document and is never translated.
        assert_eq!(suggestion.merchant.as_deref(), Some("HELIOS TRADING IKE"));
    }
}

#[test]
fn generated_text_never_starts_with_a_formula_character() {
    for locale in LOCALES {
        for template in TEMPLATES {
            for account in template_accounts(template, locale) {
                assert!(!account.name.starts_with(['=', '+', '-', '@']));
            }
        }
    }
}

/// An edit voids the original and posts a replacement; the reversing entry is
/// written in the language the edit happens in, like a plain void.
#[test]
fn the_void_inside_an_edit_is_written_in_the_given_language() {
    let expected = [
        (Locale::En, "VOID: Groceries", "Void"),
        (Locale::El, "ΑΚΥΡΩΣΗ: Groceries", "Ακύρωση"),
        (Locale::Fr, "ANNULATION : Groceries", "Annulation"),
        (Locale::De, "STORNO: Groceries", "Storno"),
    ];

    for (locale, description, memo) in expected {
        let (_dir, vault) = setup_vault();
        let conn = vault.connection().expect("conn");
        let entity = new_entity(conn, "Book", ChartTemplate::Personal, locale);
        let accounts = list_accounts(conn, entity).expect("accounts");

        let expense = PostSimpleEntry {
            entity_id: entity,
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            entry_date: "2026-02-01".into(),
            amount_minor: 1_250,
            description: "Groceries".into(),
            reference: None,
            category_account_id: Some(account_by_code(&accounts, "5100").id),
            wallet_account_id: Some(account_by_code(&accounts, "1010").id),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
        };
        let original = post_simple_entry(conn, &expense).expect("post");

        let corrected = PostSimpleEntry {
            amount_minor: 1_300,
            ..expense
        };
        let replacement =
            replace_simple_entry(conn, original.entry.id, &corrected, locale).expect("replace");

        let entries = list_entries(conn, entity, &EntryFilter::default()).expect("list");
        let reversals: Vec<_> = entries
            .iter()
            .filter(|view| view.entry.description == description)
            .collect();
        assert_eq!(reversals.len(), 1, "{locale:?}: one reversing entry");
        assert_ne!(reversals[0].entry.id, replacement.entry.id);

        let reverse = get_entry(conn, reversals[0].entry.id).expect("reverse");
        for line in &reverse.lines {
            assert_eq!(line.memo.as_deref(), Some(memo), "{locale:?}");
        }
    }
}

/// Unrecognised utility suppliers are named in the app's language.
#[test]
fn suggested_merchants_follow_the_language() {
    let gas = "Λογαριασμός Φυσικού Αερίου\nΚωδικός παροχής 123456\nΠληρωτέο (€): 42,00";
    let electricity = "Power Business\nkWh 310\nΠληρωτέο (€): 88,00";

    let expected = [
        (Locale::En, "Natural gas", "Electricity supplier"),
        (Locale::El, "Φυσικό αέριο", "Πάροχος ηλεκτρικής ενέργειας"),
        (Locale::Fr, "Gaz naturel", "Fournisseur d'électricité"),
        (Locale::De, "Erdgas", "Stromversorger"),
    ];

    for (locale, gas_merchant, electricity_merchant) in expected {
        let suggestion = parse_invoice_text(gas, locale);
        assert_eq!(
            suggestion.merchant.as_deref(),
            Some(gas_merchant),
            "{locale:?}"
        );

        let suggestion = parse_invoice_text(electricity, locale);
        assert_eq!(
            suggestion.merchant.as_deref(),
            Some(electricity_merchant),
            "{locale:?}"
        );
    }
}

/// The ledger-facing generated text, as written for an account named `Cash`.
fn generated_ledger_text(locale: Locale) -> Vec<String> {
    vec![
        oikonomia_core::text::opening_balance_description(locale, "Cash"),
        oikonomia_core::text::void_description(locale, "Groceries"),
        oikonomia_core::text::void_memo(locale).to_owned(),
        oikonomia_core::text::bank_transfer_description(locale, None),
    ]
}

#[test]
fn generated_text_is_safe_for_spreadsheets_and_the_pdf_font() {
    use oikonomia_core::text::{BillKind, bill_description};

    for locale in LOCALES {
        let mut generated = generated_ledger_text(locale);
        for kind in [
            BillKind::Electricity,
            BillKind::Gas,
            BillKind::Telecom,
            BillKind::Water,
            BillKind::Utility,
        ] {
            generated.push(bill_description(locale, kind, None));
        }

        for text in &generated {
            assert!(
                !text.starts_with(['=', '+', '-', '@']),
                "{locale:?}: {text} could be read as a formula"
            );
            assert!(
                !text.contains(['\u{202f}', '\u{a0}']),
                "{locale:?}: {text} has a narrow or no-break space"
            );
        }
    }
}
