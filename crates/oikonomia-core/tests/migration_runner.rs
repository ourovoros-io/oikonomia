//! The migration runner: which vaults it accepts, and what a failed step leaves.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use common::PASSWORD;
use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::error::Error;
use rusqlite::Connection;

fn schema_version(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT schema_version FROM vault_meta WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .expect("schema_version")
}

fn set_schema_version(conn: &Connection, version: i64) {
    conn.execute(
        "UPDATE vault_meta SET schema_version = ?1 WHERE id = 1",
        [version],
    )
    .expect("set schema_version");
}

/// Strips a fresh vault back to what `init` writes before it migrates: the
/// `vault_meta` row at version 1 and none of the ledger tables.
fn reset_to_v1(conn: &Connection) {
    conn.execute_batch(
        "
        DROP TABLE recurring_templates;
        DROP TABLE documents;
        DROP TABLE journal_lines;
        DROP TABLE journal_entries;
        DROP TABLE accounts;
        DROP TABLE entities;
        DROP TABLE app_settings;
        ",
    )
    .expect("drop the ledger tables");
    set_schema_version(conn, 1);
}

#[test]
fn a_vault_from_a_newer_build_is_refused_and_left_untouched() {
    let (_dir, mut vault) = common::vault();
    let newer = CURRENT_SCHEMA_VERSION + 1;
    let conn = vault.connection().expect("conn");
    set_schema_version(conn, newer);

    let refused = migrate(conn);

    assert!(
        matches!(&refused, Err(Error::VaultCorrupt(detail)) if detail.contains("newer")),
        "{refused:?}"
    );
    assert_eq!(schema_version(conn), newer);

    vault.lock();
    let unlocked = vault.unlock(PASSWORD);
    assert!(
        matches!(&unlocked, Err(Error::VaultCorrupt(detail)) if detail.contains("newer")),
        "unlock runs the same check: {unlocked:?}"
    );
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get(0),
        )
        .expect("sqlite_master");
    count == 1
}

#[test]
fn a_step_that_fails_part_way_is_undone_and_the_steps_before_it_stay() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    reset_to_v1(conn);
    // The v3 step creates `documents` and one index before it reaches the
    // index whose name this table takes, so it fails with work to undo.
    conn.execute_batch("CREATE TABLE idx_documents_entry (occupied INTEGER)")
        .expect("occupy the index name");

    let failed = migrate(conn);

    assert!(matches!(failed, Err(Error::Io(_))), "{failed:?}");
    assert_eq!(
        schema_version(conn),
        2,
        "the v2 step committed, so the next unlock resumes at v3"
    );
    assert!(table_exists(conn, "journal_entries"), "v2 stays applied");
    assert!(!table_exists(conn, "documents"), "v3 left nothing behind");

    conn.execute_batch("DROP TABLE idx_documents_entry")
        .expect("free the index name");
    migrate(conn).expect("the next run resumes at v3");
    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
}

#[test]
fn a_v1_vault_migrates_through_every_step() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    reset_to_v1(conn);

    migrate(conn).expect("v1 -> current");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    let tables: i64 = conn
        .query_row(
            "
            SELECT COUNT(1) FROM sqlite_master
            WHERE type = 'table'
              AND name IN ('entities', 'accounts', 'journal_entries', 'journal_lines',
                           'app_settings', 'documents', 'recurring_templates')
            ",
            [],
            |row| row.get(0),
        )
        .expect("sqlite_master");
    assert_eq!(tables, 7);
}

#[test]
fn migrating_a_vault_that_is_already_current_changes_nothing() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);

    migrate(conn).expect("first run on a current vault");
    migrate(conn).expect("second run on a current vault");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    assert!(table_exists(conn, "recurring_templates"));
}
