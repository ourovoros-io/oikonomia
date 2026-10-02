//! Case-insensitive search and duplicate checks for non-ASCII text.
//!
//! `SQLite` folds case for ASCII only, so these run on a real vault through
//! the public core functions, on a fresh connection, after lock and unlock,
//! and after a password change.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::Error;
use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::error::ValidationError;
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, EntryFilter, PostJournal, create_entity, list_accounts,
    list_entries, post_entry, update_entity,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

const PASSWORD: &str = "correct horse battery staple";

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

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init(PASSWORD).expect("init");

    (dir, vault)
}

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
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let account = |code: &str| {
        accounts
            .iter()
            .find(|account| account.code == code)
            .expect("template account")
            .id
    };

    post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: "2026-02-01".into(),
            description: description.into(),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: account("5100"),
                    debit_minor: 1_000,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: account("1010"),
                    debit_minor: 0,
                    credit_minor: 1_000,
                    memo: None,
                },
            ],
        },
    )
    .expect("post");
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
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = seed(conn);
    check(conn, entity_id);

    let (_dir, mut vault) = setup_vault();
    let entity_id = seed(vault.connection().expect("conn"));
    vault.lock();
    vault.unlock(PASSWORD).expect("unlock");
    check(vault.connection().expect("conn"), entity_id);

    let (_dir, mut vault) = setup_vault();
    let entity_id = seed(vault.connection().expect("conn"));
    vault
        .change_password(PASSWORD, "a different long passphrase")
        .expect("change password");
    check(vault.connection().expect("conn"), entity_id);

    let (dir, vault) = setup_vault();
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
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = seed(conn);

    assert_literal_wildcards(conn, entity_id);
    assert_ascii_behaviour(conn, entity_id);
}
