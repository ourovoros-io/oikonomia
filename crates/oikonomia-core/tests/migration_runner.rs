//! The migration runner: which vaults it accepts, and what a failed step leaves.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::error::Error;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

const PASSWORD: &str = "correct horse battery staple";

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init(PASSWORD).expect("init");
    (dir, vault)
}

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
    let (_dir, mut vault) = setup_vault();
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

#[test]
fn a_failed_step_keeps_the_version_of_the_last_step_that_committed() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    reset_to_v1(conn);
    // A view named `documents` makes the v3 step fail: it cannot be indexed.
    conn.execute_batch("CREATE VIEW documents AS SELECT 1 AS entity_id, 1 AS entry_id")
        .expect("create the view");

    let failed = migrate(conn);

    assert!(matches!(failed, Err(Error::Io(_))), "{failed:?}");
    assert_eq!(
        schema_version(conn),
        2,
        "the v2 step committed, so the next unlock resumes at v3"
    );
}

#[test]
fn a_v1_vault_migrates_through_every_step() {
    let (_dir, vault) = setup_vault();
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
