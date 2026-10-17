//! v9 → v10: `kind`, the simple-entry kind an entry was posted as.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{
    EntryFilter, ListedEntryKind, PostSimpleEntry, SimpleEntryAccounts, list_entries,
    post_simple_entry,
};
use rusqlite::Connection;

/// Puts a current vault back on the shape a v9 vault has.
fn reset_to_v9(conn: &Connection) {
    conn.execute_batch(
        "
        ALTER TABLE journal_entries DROP COLUMN kind;
        UPDATE vault_meta SET schema_version = 9 WHERE id = 1;
        ",
    )
    .expect("back to the v9 shape");
}

fn schema_version(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT schema_version FROM vault_meta WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .expect("schema_version")
}

#[test]
fn a_v9_vault_lists_its_entries_from_their_lines_and_stores_kinds_after() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Older", ChartTemplate::Personal);
    let food = common::account(conn, entity_id, "5100");
    let checking = common::account(conn, entity_id, "1010");
    let bills_payable = common::account(conn, entity_id, "2050");
    let post = |accounts| {
        post_simple_entry(
            conn,
            &PostSimpleEntry {
                entity_id,
                accounts,
                entry_date: common::date("2026-03-15"),
                description: "bill".into(),
                reference: None,
                amount_minor: 4_000,
                settles_entry_id: None,
            },
        )
        .expect("post")
    };
    let paid = SimpleEntryAccounts::BillPaid {
        category: food,
        wallet: checking,
    };
    let unpaid = SimpleEntryAccounts::BillUnpaid {
        category: food,
        payable: bills_payable,
    };
    let older_paid = post(paid);
    let older_unpaid = post(unpaid);
    reset_to_v9(conn);

    migrate(conn).expect("v9 -> v10");
    let newer_paid = post(paid);

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    let listed = list_entries(conn, entity_id, &EntryFilter::default()).expect("list");
    let kind_of = |id| {
        listed
            .iter()
            .find(|view| view.entry.id == id)
            .expect("listed")
            .listed_kind
    };
    // An older paid bill has only its lines to go on, and they are an
    // expense's; a bill posted after the step keeps its kind.
    assert_eq!(kind_of(older_paid.entry.id), ListedEntryKind::Expense);
    assert_eq!(kind_of(older_unpaid.entry.id), ListedEntryKind::Bill);
    assert_eq!(kind_of(newer_paid.entry.id), ListedEntryKind::Bill);

    let refused = conn.execute(
        "UPDATE journal_entries SET kind = 'other' WHERE id = ?1",
        [older_paid.entry.id.to_string()],
    );
    assert!(
        refused.is_err(),
        "the migrated column has its CHECK: {refused:?}"
    );
}

#[test]
fn running_the_v10_step_again_changes_nothing() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    // The vault is already current, so its column exists; the step must
    // tolerate that rather than fail on a second `ADD COLUMN`.
    conn.execute("UPDATE vault_meta SET schema_version = 9 WHERE id = 1", [])
        .expect("mark v9");
    migrate(conn).expect("the step runs again");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
}
