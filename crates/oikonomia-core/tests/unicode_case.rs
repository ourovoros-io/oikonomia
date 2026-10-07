//! Case-insensitive search and duplicate checks for non-ASCII text.
//!
//! `SQLite` folds case for ASCII only, so these run on a real vault through
//! the public core functions, on a fresh connection, after lock and unlock,
//! and after a password change.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use common::PASSWORD;
use oikonomia_core::Error;
use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::error::ValidationError;
use oikonomia_core::ledger::{
    CreateEntity, CreateRecurringTemplateRequest, EntryFilter, RecurringCadence, SimpleEntryKind,
    create_entity, create_recurring_template, list_accounts, list_entities, list_entries,
    list_recurring_templates, post_entry, update_entity,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;

/// Descriptions in each language, plus text that holds LIKE wildcards.
const DESCRIPTIONS: &[&str] = &[
    "Λογαριασμός ρεύματος",
    "Épicerie du marché",
    "Ärztliche Rechnung",
    "Groceries",
    "Paid 100% in cash",
    "snake_case name",
    "a1b plain",
];

fn create_book(conn: &Connection, name: &str) -> Result<EntityId, Error> {
    create_entity(
        conn,
        &CreateEntity {
            name: name.into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .map(|entity| entity.id)
}

/// Post one balanced entry with `description` on the personal chart.
fn post(conn: &Connection, entity_id: EntityId, description: &str) {
    post_with(conn, entity_id, description, None, None);
}

/// Post one balanced entry with a `reference` and a `memo` on its first line.
fn post_with(
    conn: &Connection,
    entity_id: EntityId,
    description: &str,
    reference: Option<&str>,
    memo: Option<&str>,
) {
    let mut entry = common::two_line(conn, entity_id, "2026-02-01", ("5100", "1010"), 1_000);
    entry.description = description.into();
    entry.reference = reference.map(str::to_owned);
    if let Some(debit_line) = entry.lines.first_mut() {
        debit_line.memo = memo.map(str::to_owned);
    }

    post_entry(conn, &entry).expect("post");
}

/// Descriptions of the entries a search for `text` finds, sorted.
fn found(conn: &Connection, entity_id: EntityId, text: &str) -> Vec<String> {
    let mut descriptions: Vec<String> = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            text: Some(text.into()),
            ..EntryFilter::default()
        },
    )
    .expect("list")
    .into_iter()
    .map(|view| view.entry.description)
    .collect();

    descriptions.sort();

    descriptions
}

/// Seed the entries and return the book they live in.
fn seed(conn: &Connection) -> EntityId {
    let entity_id = create_book(conn, "Books").expect("book");

    for description in DESCRIPTIONS {
        post(conn, entity_id, description);
    }

    entity_id
}

/// What a search for the wildcard characters finds: `%`, `_` and `\` in the
/// term are literals, never patterns. This is the behaviour before and after
/// the case fold.
fn assert_literal_wildcards(conn: &Connection, entity_id: EntityId) {
    assert_eq!(found(conn, entity_id, "%"), vec!["Paid 100% in cash"]);
    assert_eq!(found(conn, entity_id, "100%"), vec!["Paid 100% in cash"]);
    assert_eq!(found(conn, entity_id, "_"), vec!["snake_case name"]);
    assert_eq!(
        found(conn, entity_id, "SNAKE_CASE"),
        vec!["snake_case name"]
    );
    assert_eq!(found(conn, entity_id, "a_b"), Vec::<String>::new());
    assert_eq!(found(conn, entity_id, "\\"), Vec::<String>::new());
    assert_eq!(found(conn, entity_id, "   ").len(), DESCRIPTIONS.len());
}

/// ASCII case-insensitivity, which already worked before the case fold.
fn assert_ascii_behaviour(conn: &Connection, entity_id: EntityId) {
    assert_eq!(found(conn, entity_id, "GROCERIES"), vec!["Groceries"]);
    assert_eq!(found(conn, entity_id, "groc"), vec!["Groceries"]);
    assert_eq!(
        found(conn, entity_id, "nothing like this"),
        Vec::<String>::new()
    );

    assert!(create_book(conn, "Home").is_ok());
    assert!(matches!(
        create_book(conn, "hOME"),
        Err(Error::Validation(ValidationError::NameTaken { .. }))
    ));
}

/// The journal search folds case for every script, against `conn`.
fn assert_search_folding(conn: &Connection, entity_id: EntityId) {
    // A word from each description, in lower, upper and mixed case.
    for (description, words) in [
        (
            "Λογαριασμός ρεύματος",
            ["λογαριασμός", "ΛΟΓΑΡΙΑΣΜΌΣ", "λΟΓΑΡΙΑΣΜΌς"],
        ),
        ("Épicerie du marché", ["épicerie", "ÉPICERIE", "éPICERIE"]),
        (
            "Ärztliche Rechnung",
            ["ärztliche", "ÄRZTLICHE", "äRZTLICHE"],
        ),
        ("Groceries", ["groceries", "GROCERIES", "gRoCeRiEs"]),
    ] {
        for word in words {
            assert_eq!(
                found(conn, entity_id, word),
                vec![description.to_owned()],
                "{word:?}",
            );
        }
    }

    // A capital sigma ending the typed text still matches a medial sigma.
    assert_eq!(
        found(conn, entity_id, "ΛΟΓΑΡΙΑΣ"),
        vec!["Λογαριασμός ρεύματος"]
    );
    assert_eq!(
        found(conn, entity_id, "ΡΕΎΜΑΤΟΣ"),
        vec!["Λογαριασμός ρεύματος"]
    );

    // Accents are still significant: only case is folded.
    assert_eq!(found(conn, entity_id, "epicerie"), Vec::<String>::new());

    assert_literal_wildcards(conn, entity_id);
}

/// The entity duplicate-name check folds case for every script, against `conn`.
fn assert_name_folding(conn: &Connection) {
    // A second book whose name differs only by case is refused, in every
    // script, with the same error as the ASCII case.
    for (name, clash) in [
        ("Βιβλίο Τροφίμων", "ΒΙΒΛΊΟ ΤΡΟΦΊΜΩΝ"),
        ("Épargne", "épargne"),
        ("Ärger", "ärger"),
    ] {
        create_book(conn, name).expect("first of the pair");

        assert!(
            matches!(
                create_book(conn, clash),
                Err(Error::Validation(ValidationError::NameTaken { .. }))
            ),
            "{clash:?} clashes with {name:?}",
        );
    }

    // A genuinely different name is accepted, and renaming a book to a case
    // variant of another book's name is refused too.
    let other = create_book(conn, "Ταμιευτήριο").expect("different name is accepted");
    assert!(matches!(
        update_entity(conn, other, "ÉPARGNE"),
        Err(Error::Validation(ValidationError::NameTaken { .. }))
    ));
    // Changing only the case of its own name is fine.
    update_entity(conn, other, "ΤΑΜΙΕΥΤΉΡΙΟ").expect("own name, other case");
}

/// Run `check` against a fresh vault, one that was locked and unlocked, one
/// whose password changed (the connection is reopened after the rekey), and
/// one reopened from disk: the case fold must exist on every connection.
fn on_every_connection(check: fn(&Connection, EntityId)) {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = seed(conn);
    check(conn, entity_id);

    let (_dir, mut vault) = common::vault();
    let entity_id = seed(vault.connection().expect("conn"));
    vault.lock();
    vault.unlock(PASSWORD).expect("unlock");
    check(vault.connection().expect("conn"), entity_id);

    let (_dir, mut vault) = common::vault();
    let entity_id = seed(vault.connection().expect("conn"));
    vault
        .change_password(PASSWORD, "a different long passphrase")
        .expect("change password");
    check(vault.connection().expect("conn"), entity_id);

    let (dir, vault) = common::vault();
    let entity_id = seed(vault.connection().expect("conn"));
    drop(vault);
    let mut reopened = Vault::open_path(dir.path()).expect("reopen");
    reopened.unlock(PASSWORD).expect("unlock");
    check(reopened.connection().expect("conn"), entity_id);
}

#[test]
fn journal_search_folds_case_for_every_script() {
    on_every_connection(assert_search_folding);
}

#[test]
fn entity_name_checks_fold_case_for_every_script() {
    on_every_connection(|conn, _| assert_name_folding(conn));
}

#[test]
fn wildcards_and_ascii_names_behave_as_they_always_did() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = seed(conn);

    assert_literal_wildcards(conn, entity_id);
    assert_ascii_behaviour(conn, entity_id);
}

#[test]
fn an_all_capitals_greek_search_needs_no_accent() {
    on_every_connection(|conn, entity_id| {
        for term in ["ΛΟΓΑΡΙΑΣΜΟΣ", "λογαριασμος", "ΛΟΓΑΡΙΑΣΜΌΣ", "ρευματος"]
        {
            assert_eq!(
                found(conn, entity_id, term),
                vec!["Λογαριασμός ρεύματος"],
                "{term:?}",
            );
        }

        // French and German accents stay distinct.
        assert_eq!(found(conn, entity_id, "epicerie"), Vec::<String>::new());
        assert_eq!(found(conn, entity_id, "arztliche"), Vec::<String>::new());
        assert_eq!(
            found(conn, entity_id, "ÉPICERIE"),
            vec!["Épicerie du marché"]
        );
    });
}

#[test]
fn a_greek_name_that_differs_by_accent_and_case_is_a_duplicate() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    create_book(conn, "Τρόφιμα").expect("first");

    for clash in ["ΤΡΟΦΙΜΑ", "Τροφιμα", "τρόφιμα"] {
        assert!(
            matches!(
                create_book(conn, clash),
                Err(Error::Validation(ValidationError::NameTaken { .. }))
            ),
            "{clash:?}",
        );
    }

    // French accents are still distinct names.
    create_book(conn, "Epargne").expect("plain");
    create_book(conn, "Épargne").expect("accented is a different name");
}

#[test]
fn references_and_memos_are_searched_in_any_case() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = create_book(conn, "Books").expect("book");

    // Each entry has a distinct description, so a hit names exactly one entry.
    post_with(
        conn,
        entity_id,
        "first",
        Some("Τιμολόγιο ΑΒ-1"),
        Some("Ενοίκιο Μαρτίου"),
    );
    post_with(
        conn,
        entity_id,
        "second",
        Some("Facture Été-2"),
        Some("Épicerie du coin"),
    );
    post_with(
        conn,
        entity_id,
        "third",
        Some("Rechnung Über-3"),
        Some("Ärztliche Kosten"),
    );
    post_with(
        conn,
        entity_id,
        "fourth",
        Some("plain ref"),
        Some("plain memo"),
    );

    for (term, expected) in [
        ("ΤΙΜΟΛΟΓΙΟ", "first"),
        ("τιμολογιο αβ-1", "first"),
        ("ΕΝΟΙΚΙΟ", "first"),
        ("ενοικιο μαρτιου", "first"),
        ("FACTURE ÉTÉ", "second"),
        ("facture été-2", "second"),
        ("ÉPICERIE DU COIN", "second"),
        ("épicerie du coin", "second"),
        ("RECHNUNG ÜBER", "third"),
        ("ärztliche kosten", "third"),
        ("ÄRZTLICHE KOSTEN", "third"),
        ("PLAIN REF", "fourth"),
    ] {
        assert_eq!(found(conn, entity_id, term), vec![expected], "{term:?}");
    }

    // Accents other than Greek ones still count.
    assert_eq!(found(conn, entity_id, "epicerie"), Vec::<String>::new());
    assert_eq!(found(conn, entity_id, "facture ete"), Vec::<String>::new());
}
/// Names in the order a reader expects, written so that code point order
/// would put every capitalised one first.
const NAMES_IN_ORDER: &[&str] = &["apple", "Banana", "αλφα", "Βήτα", "γάμα"];

#[test]
fn entities_are_listed_by_name_without_regard_to_case_in_any_script() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    for name in NAMES_IN_ORDER.iter().rev() {
        create_book(conn, name).expect("book");
    }

    let listed: Vec<String> = list_entities(conn)
        .expect("list")
        .into_iter()
        .map(|entity| entity.name)
        .collect();

    assert_eq!(listed, NAMES_IN_ORDER);
}

#[test]
fn templates_due_the_same_day_are_listed_by_name_without_regard_to_case() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = create_book(conn, "Templates").expect("book");
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let by_code = |code: &str| {
        accounts
            .iter()
            .find(|account| account.code == code)
            .map(|account| account.id)
            .expect(code)
    };
    for name in NAMES_IN_ORDER.iter().rev() {
        create_recurring_template(
            conn,
            &common::strict(CreateRecurringTemplateRequest {
                entity_id,
                name: (*name).into(),
                kind: SimpleEntryKind::Expense,
                bill_status: None,
                amount_minor: 1_000,
                cadence: RecurringCadence::Weekly,
                day_of_month: None,
                category_account_id: Some(by_code("5100")),
                wallet_account_id: Some(by_code("1010")),
                payable_account_id: None,
                from_account_id: None,
                to_account_id: None,
                memo: None,
                next_date: "2026-03-02".into(),
            }),
        )
        .expect("template");
    }

    let listed: Vec<String> = list_recurring_templates(conn, entity_id)
        .expect("list")
        .into_iter()
        .map(|template| template.fields.name)
        .collect();

    assert_eq!(listed, NAMES_IN_ORDER);
}
