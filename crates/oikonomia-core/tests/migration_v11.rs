//! v10 → v11: `bill_status`, the payment state an entry was posted with.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{
    PostSimpleEntry, PostedEntryView, SimpleEntryAccounts, get_entry, post_simple_entry,
};
use rusqlite::Connection;

/// Puts a current vault back on the shape a v10 vault has.
fn reset_to_v10(conn: &Connection) {
    conn.execute_batch(
        "
        ALTER TABLE journal_entries DROP COLUMN bill_status;
        UPDATE vault_meta SET schema_version = 10 WHERE id = 1;
        ",
    )
    .expect("back to the v10 shape");
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
fn a_v10_bill_reads_its_status_from_its_lines_and_later_bills_store_it() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Older", ChartTemplate::Personal);
    let food = common::account(conn, entity_id, "5100");
    let bills_payable = common::account(conn, entity_id, "2050");
    let card = common::account(conn, entity_id, "2000");
    let post = |accounts| -> PostedEntryView {
        post_simple_entry(
            conn,
            &PostSimpleEntry {
                entity_id,
                accounts,
                entry_date: common::date("2026-03-15"),
                description: "bill".into(),
                reference: None,
                amount_minor: 4_000,
            },
        )
        .expect("post")
    };
    let form = |id| get_entry(conn, id).expect("get").simple_form;
    let unpaid = SimpleEntryAccounts::BillUnpaid {
        category: food,
        payable: bills_payable,
    };
    let owed_on_card = SimpleEntryAccounts::BillUnpaid {
        category: food,
        payable: card,
    };
    let older_unpaid = post(unpaid);
    let older_owed_on_card = post(owed_on_card);
    reset_to_v10(conn);

    migrate(conn).expect("v10 -> v11");
    let newer_owed_on_card = post(owed_on_card);

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    // An older bill has only its lines to go on: one owed on the default
    // payable reads as unpaid, one owed on a card as paid with it.
    assert_eq!(form(older_unpaid.entry.id), Some(unpaid));
    assert_eq!(
        form(older_owed_on_card.entry.id),
        Some(SimpleEntryAccounts::BillPaid {
            category: food,
            wallet: card,
        })
    );
    assert_eq!(form(newer_owed_on_card.entry.id), Some(owed_on_card));

    let refused = conn.execute(
        "UPDATE journal_entries SET bill_status = 'owed' WHERE id = ?1",
        [older_unpaid.entry.id.to_string()],
    );
    assert!(
        refused.is_err(),
        "the migrated column has its CHECK: {refused:?}"
    );
}

#[test]
fn running_the_v11_step_again_changes_nothing() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    // The vault is already current, so its column exists; the step must
    // tolerate that rather than fail on a second `ADD COLUMN`.
    conn.execute("UPDATE vault_meta SET schema_version = 10 WHERE id = 1", [])
        .expect("mark v10");
    migrate(conn).expect("the step runs again");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
}
