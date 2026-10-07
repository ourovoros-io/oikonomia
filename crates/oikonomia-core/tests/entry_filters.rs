//! `list_entries` filter behavior: text, date range, account.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    EntryFilter, PostedEntryView, list_accounts, list_entries, post_entry,
};
use rusqlite::Connection;

/// What one test entry is posted with, besides its book.
struct Posting<'a> {
    /// Accounting date as `YYYY-MM-DD`.
    date: &'a str,
    /// Description of the entry.
    description: &'a str,
    /// Reference of the entry, if any.
    reference: Option<&'a str>,
    /// Memo of the debit line, if any.
    memo: Option<&'a str>,
    /// Codes of the debited and then the credited account.
    sides: (&'a str, &'a str),
}

impl<'a> Posting<'a> {
    /// Groceries paid from checking, with no reference and no memo.
    fn groceries(date: &'a str, description: &'a str) -> Self {
        Self {
            date,
            description,
            reference: None,
            memo: None,
            sides: ("5100", "1010"),
        }
    }
}

/// Posts a balanced two-line entry of 10.00 between personal-template
/// accounts.
fn post_two_line(conn: &Connection, entity_id: EntityId, posting: &Posting<'_>) -> PostedEntryView {
    let mut entry = common::two_line(conn, entity_id, posting.date, posting.sides, 1_000);
    entry.description = posting.description.into();
    entry.reference = posting.reference.map(Into::into);
    if let Some(debit_line) = entry.lines.first_mut() {
        debit_line.memo = posting.memo.map(Into::into);
    }

    post_entry(conn, &entry).expect("post")
}

#[test]
fn text_filter_matches_description_reference_and_memo() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Filters", ChartTemplate::Personal);

    post_two_line(
        conn,
        entity_id,
        &Posting::groceries("2026-02-01", "Groceries March"),
    );
    post_two_line(
        conn,
        entity_id,
        &Posting {
            reference: Some("INV-42"),
            ..Posting::groceries("2026-02-02", "Utility bill")
        },
    );
    post_two_line(
        conn,
        entity_id,
        &Posting {
            memo: Some("office chair"),
            ..Posting::groceries("2026-02-03", "Shopping")
        },
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
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Filters", ChartTemplate::Personal);

    for date in ["2026-01-10", "2026-01-20", "2026-01-31"] {
        post_two_line(conn, entity_id, &Posting::groceries(date, "Entry"));
    }

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            date_from: Some(common::date("2026-01-10")),
            date_to: Some(common::date("2026-01-20")),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 2, "bounds are inclusive");
}

#[test]
fn account_filter_matches_entries_touching_the_account() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Filters", ChartTemplate::Personal);

    post_two_line(
        conn,
        entity_id,
        &Posting::groceries("2026-03-01", "Food shop"),
    );
    post_two_line(
        conn,
        entity_id,
        &Posting {
            sides: ("1020", "1010"),
            ..Posting::groceries("2026-03-02", "Move to savings")
        },
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
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Filters", ChartTemplate::Personal);

    post_two_line(
        conn,
        entity_id,
        &Posting::groceries("2026-04-01", "Groceries"),
    );
    post_two_line(
        conn,
        entity_id,
        &Posting::groceries("2026-05-01", "Groceries"),
    );

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            text: Some("groceries".into()),
            date_from: Some(common::date("2026-04-15")),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 1, "text AND date must both hold");
}
