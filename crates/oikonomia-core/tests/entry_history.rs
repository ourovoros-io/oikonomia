//! The history of a corrected entry: its earlier versions and the reversals
//! core wrote when they were replaced.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::domain::{ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    EntryChange, delete_entity, entry_history, post_simple_entry, replace_simple_entry,
};
use oikonomia_core::prefs::Locale;
use rusqlite::Connection;

fn expense(
    conn: &Connection,
    entity_id: EntityId,
    description: &str,
    minor: i64,
) -> oikonomia_core::ledger::PostSimpleEntry {
    let food = common::account(conn, entity_id, "5100");
    let checking = common::account(conn, entity_id, "1010");
    let mut entry = common::simple_expense(entity_id, food, checking, "2026-03-15", minor);
    entry.description = description.into();

    entry
}

fn summary(conn: &Connection, id: JournalEntryId) -> Vec<(EntryChange, String)> {
    entry_history(conn, id)
        .expect("history")
        .into_iter()
        .map(|item| (item.change, item.description))
        .collect()
}

#[test]
fn an_entry_that_replaced_nothing_has_no_history() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Plain", ChartTemplate::Personal);
    let posted = post_simple_entry(conn, &expense(conn, entity_id, "lunch", 500)).expect("post");

    assert_eq!(summary(conn, posted.entry.id), []);
}

#[test]
fn a_corrected_entry_lists_its_original_and_the_stored_reversal() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Edited", ChartTemplate::Personal);
    let original = post_simple_entry(conn, &expense(conn, entity_id, "lunch", 500)).expect("post");

    let replacement = replace_simple_entry(
        conn,
        original.entry.id,
        &expense(conn, entity_id, "lunch with Ana", 650),
        Locale::De,
    )
    .expect("replace");

    assert_eq!(
        summary(conn, replacement.entry.id),
        [
            (EntryChange::Original, "lunch".to_owned()),
            (EntryChange::Reversal, "STORNO: lunch".to_owned()),
        ]
    );
}

#[test]
fn a_twice_corrected_entry_lists_every_version_in_order() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Twice", ChartTemplate::Personal);
    let first = post_simple_entry(conn, &expense(conn, entity_id, "a", 100)).expect("post");
    let second = replace_simple_entry(
        conn,
        first.entry.id,
        &expense(conn, entity_id, "b", 200),
        Locale::En,
    )
    .expect("first correction");
    let third = replace_simple_entry(
        conn,
        second.entry.id,
        &expense(conn, entity_id, "c", 300),
        Locale::En,
    )
    .expect("second correction");

    assert_eq!(
        summary(conn, third.entry.id),
        [
            (EntryChange::Original, "a".to_owned()),
            (EntryChange::Reversal, "VOID: a".to_owned()),
            (EntryChange::Replacement, "b".to_owned()),
            (EntryChange::Reversal, "VOID: b".to_owned()),
        ]
    );
}

#[test]
fn an_unknown_entry_is_not_found() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    let result = entry_history(conn, JournalEntryId::generate());

    assert!(matches!(result, Err(Error::NotFound(_))), "{result:?}");
}

#[test]
fn deleting_a_book_with_corrected_entries_still_works() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Doomed", ChartTemplate::Personal);
    let original = post_simple_entry(conn, &expense(conn, entity_id, "x", 100)).expect("post");
    replace_simple_entry(
        conn,
        original.entry.id,
        &expense(conn, entity_id, "y", 200),
        Locale::En,
    )
    .expect("replace");

    delete_entity(conn, entity_id).expect("delete");
}

#[test]
fn a_loop_in_the_replacement_links_is_reported_as_corruption() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Loop", ChartTemplate::Personal);
    let original = post_simple_entry(conn, &expense(conn, entity_id, "a", 100)).expect("post");
    let replacement = replace_simple_entry(
        conn,
        original.entry.id,
        &expense(conn, entity_id, "b", 200),
        Locale::En,
    )
    .expect("replace");
    conn.execute(
        "UPDATE journal_entries SET replaces_entry_id = ?1 WHERE id = ?2",
        [
            replacement.entry.id.to_string(),
            original.entry.id.to_string(),
        ],
    )
    .expect("damage the links");

    let result = entry_history(conn, replacement.entry.id);

    assert!(matches!(result, Err(Error::VaultCorrupt(_))), "{result:?}");
}
