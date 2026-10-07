//! Entries of one date created within one second keep the order they were
//! posted in, in the entry list, the register and the journal export.
//!
//! Creation time is stored to the second, so a CSV import puts many entries
//! on the same date and the same creation time. Each test posts such a batch
//! in one transaction and then stamps one creation time on all of it, so the
//! tie is certain however slowly the batch ran.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::csv::{export_journal_csv, parse_journal_export, post_import_rows};
use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    EntryFilter, PostSimpleEntryRequest, SimpleEntryKind, account_register, list_entries,
};
use rusqlite::Connection;

/// How many entries each test posts on the one date.
const BATCH: usize = 6;

/// The description of the entry posted in position `number`, from 1.
fn description(number: usize) -> String {
    format!("row {number}")
}

/// Posts [`BATCH`] expenses dated 15 March 2026 in one transaction, in the
/// order of their numbers, and gives all of them one creation time.
///
/// Returns the book and the wallet account every expense was paid from.
fn book_with_a_batch(conn: &Connection) -> (EntityId, AccountId) {
    let entity_id = common::book(conn, "Imported", ChartTemplate::Personal);
    let food = common::account(conn, entity_id, "5100");
    let checking = common::account(conn, entity_id, "1010");

    let rows: Vec<PostSimpleEntryRequest> = (1..=BATCH)
        .map(|number| PostSimpleEntryRequest {
            entity_id,
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            entry_date: "2026-03-15".into(),
            description: description(number),
            reference: None,
            amount_minor: 100,
            category_account_id: Some(food),
            wallet_account_id: Some(checking),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
        })
        .collect();
    let posted = post_import_rows(conn, &rows, true).expect("post the batch");
    assert_eq!(posted.posted.len(), BATCH);

    conn.execute("UPDATE journal_entries SET created_at = 'unix:1'", [])
        .expect("one creation time for the batch");

    (entity_id, checking)
}

/// The descriptions of the batch in the order it was posted.
fn posted_order() -> Vec<String> {
    (1..=BATCH).map(description).collect()
}

#[test]
fn the_list_shows_entries_of_one_second_latest_first() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, _checking) = book_with_a_batch(conn);
    let latest_first: Vec<String> = posted_order().into_iter().rev().collect();

    let listed = |conn: &Connection| -> Vec<String> {
        list_entries(conn, entity_id, &EntryFilter::default())
            .expect("list")
            .into_iter()
            .map(|view| view.entry.description)
            .collect()
    };
    assert_eq!(listed(conn), latest_first);

    // The order is the query's, not an accident of the index it reads
    // through: without the index the rows reach the sort oldest first.
    conn.execute_batch("DROP INDEX idx_entries_entity_date")
        .expect("drop the index");
    assert_eq!(listed(conn), latest_first);
}

#[test]
fn the_register_shows_entries_of_one_second_in_the_order_posted() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (_entity_id, checking) = book_with_a_batch(conn);

    let register = |conn: &Connection| -> Vec<String> {
        account_register(conn, checking, None, None)
            .expect("register")
            .into_iter()
            .map(|line| line.description)
            .collect()
    };
    assert_eq!(register(conn), posted_order());

    // As above: without the index on the account the lines are found through
    // their entries, in no order the caller can rely on.
    conn.execute_batch("DROP INDEX idx_lines_account")
        .expect("drop the index");
    assert_eq!(register(conn), posted_order());
}

#[test]
fn the_export_keeps_the_lines_of_each_entry_together_in_the_order_posted() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let (entity_id, _checking) = book_with_a_batch(conn);

    let csv = export_journal_csv(conn, entity_id).expect("export");
    let exported: Vec<String> = parse_journal_export(&csv)
        .expect("parse")
        .into_iter()
        .map(|line| line.description)
        .collect();

    // Two lines per entry, the debit then the credit.
    let expected: Vec<String> = posted_order()
        .into_iter()
        .flat_map(|description| [description.clone(), description])
        .collect();
    assert_eq!(exported, expected);
}
