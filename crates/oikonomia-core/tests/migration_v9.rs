//! v8 → v9: `replaces_entry_id`, the entry a correction took the place of.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{entry_history, void_entry};
use oikonomia_core::prefs::Locale;
use rusqlite::Connection;

/// Puts a current vault back on the shape a v8 vault has.
fn reset_to_v8(conn: &Connection) {
    conn.execute_batch(
        "
        DROP INDEX idx_entries_replaces;
        ALTER TABLE journal_entries DROP COLUMN replaces_entry_id;
        UPDATE vault_meta SET schema_version = 8 WHERE id = 1;
        ",
    )
    .expect("back to the v8 shape");
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
fn a_v8_vault_keeps_its_voided_entries_and_has_no_history_for_them() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Older", ChartTemplate::Personal);
    let original = common::post_two_line(conn, entity_id, "2026-03-02", ("5100", "1010"), 900);
    let voided = void_entry(conn, original.entry.id, Locale::En).expect("void");
    reset_to_v8(conn);

    migrate(conn).expect("v8 -> v9");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    let history = entry_history(conn, voided.reverse_id).expect("history");
    assert_eq!(history.len(), 0, "{history:?}");
}

#[test]
fn running_the_v9_step_again_changes_nothing() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");

    // The vault is already current, so its column exists; the step must
    // tolerate that rather than fail on a second `ADD COLUMN`.
    conn.execute("UPDATE vault_meta SET schema_version = 8 WHERE id = 1", [])
        .expect("mark v8");
    migrate(conn).expect("the step runs again");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
}
