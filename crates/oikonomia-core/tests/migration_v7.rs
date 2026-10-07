//! v6 → v7: local recurring entry templates.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};

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
fn v6_vault_gains_recurring_templates() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    assert!(table_exists(conn, "recurring_templates"));

    conn.execute("DROP TABLE recurring_templates", [])
        .expect("drop");
    conn.execute("UPDATE vault_meta SET schema_version = 6 WHERE id = 1", [])
        .expect("mark v6");
    assert!(!table_exists(conn, "recurring_templates"));

    migrate(conn).expect("v6 -> current");

    assert_eq!(schema_version(conn), CURRENT_SCHEMA_VERSION);
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
