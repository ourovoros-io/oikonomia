//! `list_entries` filter behavior: text, date range, account.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, EntryFilter, PostJournal, PostedEntryView, create_entity,
    list_accounts, list_entries, post_entry,
};
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn setup_entity(conn: &Connection) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: "Filters".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity")
    .id
}

/// Post a balanced two-line entry using personal-template account codes.
#[expect(
    clippy::too_many_arguments,
    reason = "test helper, many parameters by design"
)]
fn post_two_line(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    description: &str,
    reference: Option<&str>,
    memo: Option<&str>,
    debit_code: &str,
    credit_code: &str,
) -> PostedEntryView {
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let acc = |code: &str| {
        accounts
            .iter()
            .find(|a| a.code == code)
            .expect("template account")
            .id
    };

    post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: date.into(),
            description: description.into(),
            reference: reference.map(Into::into),
            lines: vec![
                CreateJournalLine {
                    account_id: acc(debit_code),
                    debit_minor: 1_000,
                    credit_minor: 0,
                    memo: memo.map(Into::into),
                },
                CreateJournalLine {
                    account_id: acc(credit_code),
                    debit_minor: 0,
                    credit_minor: 1_000,
                    memo: None,
                },
            ],
        },
    )
    .expect("post")
}

#[test]
fn text_filter_matches_description_reference_and_memo() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    post_two_line(
        conn,
        entity_id,
        "2026-02-01",
        "Groceries March",
        None,
        None,
        "5100",
        "1010",
    );
    post_two_line(
        conn,
        entity_id,
        "2026-02-02",
        "Utility bill",
        Some("INV-42"),
        None,
        "5100",
        "1010",
    );
    post_two_line(
        conn,
        entity_id,
        "2026-02-03",
        "Shopping",
        None,
        Some("office chair"),
        "5100",
        "1010",
    );

    let by = |text: &str| {
        list_entries(
            conn,
            entity_id,
            &EntryFilter {
                text: Some(text.into()),
                ..EntryFilter::default()
            },
        )
        .expect("list")
    };

    assert_eq!(by("groceries").len(), 1, "description, case-insensitive");
    assert_eq!(by("inv-42").len(), 1, "reference matches");
    assert_eq!(by("chair").len(), 1, "line memo matches");
    assert_eq!(by("no-such-text").len(), 0);
    assert_eq!(by("%").len(), 0, "LIKE wildcards are escaped literals");
    assert_eq!(by("   ").len(), 3, "blank text means no filter");
}

#[test]
fn date_range_is_inclusive_on_both_ends() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    for date in ["2026-01-10", "2026-01-20", "2026-01-31"] {
        post_two_line(conn, entity_id, date, "Entry", None, None, "5100", "1010");
    }

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            date_from: Some("2026-01-10".into()),
            date_to: Some("2026-01-20".into()),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 2, "bounds are inclusive");
}

#[test]
fn account_filter_matches_entries_touching_the_account() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    post_two_line(
        conn,
        entity_id,
        "2026-03-01",
        "Food shop",
        None,
        None,
        "5100",
        "1010",
    );
    post_two_line(
        conn,
        entity_id,
        "2026-03-02",
        "Move to savings",
        None,
        None,
        "1020",
        "1010",
    );

    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100").id;

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            account_id: Some(food),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 1);
    assert_eq!(got[0].entry.description, "Food shop");
}

#[test]
fn combined_filters_intersect() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    post_two_line(
        conn,
        entity_id,
        "2026-04-01",
        "Groceries",
        None,
        None,
        "5100",
        "1010",
    );
    post_two_line(
        conn,
        entity_id,
        "2026-05-01",
        "Groceries",
        None,
        None,
        "5100",
        "1010",
    );

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            text: Some("groceries".into()),
            date_from: Some("2026-04-15".into()),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 1, "text AND date must both hold");
}
