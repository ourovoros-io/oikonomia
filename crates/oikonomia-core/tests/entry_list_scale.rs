//! Listing entries binds the same few variables however many entries match,
//! so a book with more entries than `SQLite` accepts bound variables lists.
//!
//! The real limit is 32766. Listing that many entries takes minutes today for
//! an unrelated reason (the voided flag scans `journal_entries` once per
//! entry), so the test lowers the limit on its own connection instead and
//! lists a handful of entries.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId};
use oikonomia_core::ledger::{EntryFilter, PostSimpleEntry, list_entries, post_simple_entry};
use rusqlite::Connection;
use rusqlite::limits::Limit;

/// More entries than [`LOWERED_VARIABLE_LIMIT`], and more than the five
/// variables a listing binds.
const ENTRY_COUNT: usize = 12;

/// Fewer variables than entries, and at least the five a listing binds.
const LOWERED_VARIABLE_LIMIT: i32 = 8;

/// A book with [`ENTRY_COUNT`] expenses, each with a memo-free pair of lines.
fn book_with_entries(conn: &Connection) -> (EntityId, AccountId) {
    let entity_id = common::book(conn, "Busy", ChartTemplate::Personal);
    let food = common::account(conn, entity_id, "5100");
    let checking = common::account(conn, entity_id, "1010");

    for number in 1..=ENTRY_COUNT {
        let lunch = PostSimpleEntry {
            description: format!("lunch {number}"),
            ..common::simple_expense(entity_id, food, checking, "2026-03-15", 100)
        };
        post_simple_entry(conn, &lunch).expect("post");
    }

    (entity_id, food)
}

#[test]
fn the_bundled_sqlite_accepts_32766_bound_variables() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    assert_eq!(conn.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER), Ok(32_766));
}

#[test]
fn a_listing_of_more_entries_than_the_variable_limit_returns_every_entry_with_its_lines() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, food) = book_with_entries(conn);
    conn.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, LOWERED_VARIABLE_LIMIT)
        .expect("lower the limit");

    let every_filter_bound = EntryFilter {
        text: Some("LUNCH".into()),
        date_from: Some(common::date("2026-03-01")),
        date_to: Some(common::date("2026-03-31")),
        account_id: Some(food),
    };
    for filter in [EntryFilter::default(), every_filter_bound] {
        let listed = list_entries(conn, entity_id, &filter).expect("list");

        assert_eq!(listed.len(), ENTRY_COUNT, "{filter:?}");
        let incomplete = listed.iter().filter(|view| view.lines.len() != 2).count();
        assert_eq!(incomplete, 0, "every entry comes back with both its lines");
    }
}
