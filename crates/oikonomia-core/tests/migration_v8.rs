//! v7 → v8: indexes for the void link and for the lines of one entry.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{EntryFilter, list_entries, void_entry};
use oikonomia_core::prefs::Locale;
use rusqlite::Connection;

/// The schema version the vault records.
fn schema_version(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT schema_version FROM vault_meta WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .expect("schema_version")
}

/// The names of the indexes on `table` that a migration created, sorted.
fn indexes_on(conn: &Connection, table: &str) -> Vec<String> {
    let mut statement = conn
        .prepare(
            "
            SELECT name FROM sqlite_master
            WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL
            ORDER BY name
            ",
        )
        .expect("prepare");
    let names = statement
        .query_map([table], |row| row.get(0))
        .expect("query");

    names.collect::<Result<_, _>>().expect("index names")
}

/// Puts a current vault back on the indexes a v7 vault has.
fn reset_to_v7(conn: &Connection) {
    conn.execute_batch(
        "
        DROP INDEX idx_entries_voided_by;
        DROP INDEX idx_lines_entry_account;
        CREATE INDEX idx_lines_entry ON journal_lines(entry_id);
        UPDATE vault_meta SET schema_version = 7 WHERE id = 1;
        ",
    )
    .expect("back to the v7 indexes");
}

#[test]
fn a_v7_vault_gains_the_indexes_and_keeps_its_entries() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    let entity_id = common::book(conn, "Indexed", ChartTemplate::Personal);
    let kept = common::post_two_line(conn, entity_id, "2026-03-01", ("5100", "1010"), 700);
    let voided = common::post_two_line(conn, entity_id, "2026-03-02", ("5100", "1010"), 900);
    void_entry(conn, voided.entry.id, Locale::En).expect("void");
    reset_to_v7(conn);
    assert_eq!(
        indexes_on(conn, "journal_lines"),
        ["idx_lines_account", "idx_lines_entry"]
    );

    migrate(conn).expect("v7 -> v8");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    assert_eq!(
        indexes_on(conn, "journal_entries"),
        ["idx_entries_entity_date", "idx_entries_voided_by"]
    );
    assert_eq!(
        indexes_on(conn, "journal_lines"),
        ["idx_lines_account", "idx_lines_entry_account"]
    );

    let listed = list_entries(conn, entity_id, &EntryFilter::default()).expect("list");
    let voided_flags: Vec<_> = listed
        .iter()
        .map(|view| (view.entry.id == kept.entry.id, view.is_voided))
        .collect();
    assert_eq!(voided_flags, [(false, true), (false, true), (true, false)]);
}

#[test]
fn running_the_v8_step_again_changes_nothing() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    reset_to_v7(conn);
    migrate(conn).expect("v7 -> v8");
    let after_first = (
        indexes_on(conn, "journal_entries"),
        indexes_on(conn, "journal_lines"),
    );

    // A vault that crashed after the step ran but is marked v7 again runs it
    // a second time; every statement of the step tolerates that.
    conn.execute("UPDATE vault_meta SET schema_version = 7 WHERE id = 1", [])
        .expect("mark v7");
    migrate(conn).expect("the step runs again");
    migrate(conn).expect("and a current vault is left alone");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    let after_second = (
        indexes_on(conn, "journal_entries"),
        indexes_on(conn, "journal_lines"),
    );
    assert_eq!(after_second, after_first);
}
