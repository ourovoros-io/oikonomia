//! v11 → v12: `settles_entry_id`, the bill a payment settles.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{
    PostSimpleEntry, SimpleEntryAccounts, get_entry, list_open_bills, post_simple_entry,
};
use rusqlite::Connection;

/// Puts a current vault back on the shape a v11 vault has.
fn reset_to_v11(conn: &Connection) {
    conn.execute_batch(
        "
        DROP INDEX idx_entries_settles;
        ALTER TABLE journal_entries DROP COLUMN settles_entry_id;
        UPDATE vault_meta SET schema_version = 11 WHERE id = 1;
        ",
    )
    .expect("back to the v11 shape");
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
fn a_v11_vault_lists_its_unpaid_bills_as_open_and_links_later_payments() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Older", ChartTemplate::Personal);
    let power = common::account(conn, entity_id, "5100");
    let checking = common::account(conn, entity_id, "1010");
    let bills_payable = common::account(conn, entity_id, "2050");
    let post = |accounts, settles_entry_id| {
        post_simple_entry(
            conn,
            &PostSimpleEntry {
                entity_id,
                accounts,
                entry_date: common::date("2026-03-15"),
                description: "power".into(),
                reference: None,
                amount_minor: 4_000,
                settles_entry_id,
            },
        )
        .expect("post")
    };
    let older = post(
        SimpleEntryAccounts::BillUnpaid {
            category: power,
            payable: bills_payable,
        },
        None,
    );
    reset_to_v11(conn);

    migrate(conn).expect("v11 -> v12");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    let open = list_open_bills(conn, entity_id, None).expect("open bills");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].entry_id, older.entry.id);

    let payment = post(
        SimpleEntryAccounts::BillPayment {
            payable: bills_payable,
            wallet: checking,
        },
        Some(older.entry.id),
    );
    assert_eq!(
        get_entry(conn, payment.entry.id)
            .expect("get")
            .settles_entry_id,
        Some(older.entry.id)
    );
    assert_eq!(list_open_bills(conn, entity_id, None).expect("open"), []);
}

#[test]
fn running_the_v12_step_again_changes_nothing() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    // The vault is already current, so its column and index exist; the step
    // must tolerate both rather than fail on a second `ADD COLUMN`.
    conn.execute("UPDATE vault_meta SET schema_version = 11 WHERE id = 1", [])
        .expect("mark v11");
    migrate(conn).expect("the step runs again");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
}
