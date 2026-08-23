//! v6 → v7: local recurring entry templates.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use oikonomia_core::vault::Vault;
use tempfile::TempDir;

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn schema_version(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT schema_version FROM vault_meta WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .expect("schema_version")
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get(0),
        )
        .expect("sqlite_master");
    n == 1
}

#[test]
fn current_schema_is_v7() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 7);
}

#[test]
fn v6_vault_gains_recurring_templates() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    assert!(table_exists(conn, "recurring_templates"));

    conn.execute("DROP TABLE recurring_templates", [])
        .expect("drop");
    conn.execute("UPDATE vault_meta SET schema_version = 6 WHERE id = 1", [])
        .expect("mark v6");
    assert!(!table_exists(conn, "recurring_templates"));

    migrate(conn).expect("v6 -> v7");

    assert_eq!(schema_version(conn), 7);
    assert!(table_exists(conn, "recurring_templates"));

    let indexed: i64 = conn
        .query_row(
            "
            SELECT COUNT(1) FROM sqlite_master
            WHERE type = 'index'
              AND name IN ('idx_recurring_entity', 'idx_recurring_entity_next')
            ",
            [],
            |row| row.get(0),
        )
        .expect("indexes");
    assert_eq!(indexed, 2);
}

#[test]
fn migrate_is_safe_on_fresh_v7_vault() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    migrate(conn).expect("idempotent");
    migrate(conn).expect("idempotent again");
    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
    assert!(table_exists(conn, "recurring_templates"));
}
