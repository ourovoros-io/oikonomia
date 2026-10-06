//! v3 → v4 migration: orphan cleanup, duplicate-name suffixing, constraints.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

mod common;

use oikonomia_core::db::{CURRENT_SCHEMA_VERSION, migrate};
use rusqlite::Connection;

/// Rebuild the v3 documents shape (nullable `entry_id`, no unique index) and
/// seed it with an orphan and a same-book name clash.
fn downgrade_to_v3_with_bad_data(conn: &Connection) {
    conn.execute_batch(
        "
        ALTER TABLE journal_entries DROP COLUMN hidden;
        DROP TABLE documents;
        CREATE TABLE documents (
            id TEXT PRIMARY KEY NOT NULL,
            entity_id TEXT NOT NULL REFERENCES entities(id),
            entry_id TEXT REFERENCES journal_entries(id),
            filename TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            data BLOB NOT NULL,
            created_at TEXT NOT NULL,
            analysis_json TEXT
        );
        INSERT INTO entities (id, name, base_currency, fiscal_year_start_month, chart_template, created_at)
        VALUES ('e1', 'Book', 'EUR', 1, 'blank', 'unix:1');
        INSERT INTO journal_entries (id, entity_id, entry_date, description, reference, status, created_at, posted_at, voided_by_entry_id)
        VALUES ('j1', 'e1', '2026-01-01', 'Entry', NULL, 'posted', 'unix:1', 'unix:1', NULL);
        INSERT INTO documents (id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at, analysis_json)
        VALUES ('d1', 'e1', NULL, 'orphan.pdf', 'application/pdf', 1, x'00', 'unix:1', NULL),
               ('d2', 'e1', 'j1', 'invoice.pdf', 'application/pdf', 1, x'00', 'unix:2', NULL),
               ('d3', 'e1', 'j1', 'invoice.pdf', 'application/pdf', 1, x'00', 'unix:3', NULL),
               ('d4', 'e1', 'j1', 'notes', 'text/plain', 1, x'00', 'unix:4', NULL);
        UPDATE vault_meta SET schema_version = 3 WHERE id = 1;
        ",
    )
    .expect("downgrade to v3 shape");
}

#[test]
fn v4_migration_cleans_orphans_and_suffixes_duplicates() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    downgrade_to_v3_with_bad_data(conn);

    migrate(conn).expect("migrate v3 -> v4");

    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("version");
    assert_eq!(version, CURRENT_SCHEMA_VERSION);

    let orphans: i64 = conn
        .query_row("SELECT COUNT(1) FROM documents WHERE id = 'd1'", [], |r| {
            r.get(0)
        })
        .expect("orphans");
    assert_eq!(orphans, 0, "orphan deleted by migration");

    let names: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT filename FROM documents ORDER BY created_at ASC, rowid ASC")
            .expect("stmt");
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).expect("rows");
        rows.map(|r| r.expect("row")).collect()
    };
    assert_eq!(
        names,
        vec![
            "invoice.pdf".to_owned(),
            "invoice (2).pdf".to_owned(),
            "notes".to_owned()
        ],
        "oldest keeps its name; later duplicate suffixed before the extension"
    );

    // The constraints now actively reject violations.
    let orphan_insert = conn.execute(
        "INSERT INTO documents (id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at, analysis_json)
         VALUES ('dx', 'e1', NULL, 'x.pdf', 'application/pdf', 1, x'00', 'unix:9', NULL)",
        [],
    );
    assert!(orphan_insert.is_err(), "NOT NULL rejects orphans");

    let dup_insert = conn.execute(
        "INSERT INTO documents (id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at, analysis_json)
         VALUES ('dy', 'e1', 'j1', 'invoice.pdf', 'application/pdf', 1, x'00', 'unix:9', NULL)",
        [],
    );
    assert!(dup_insert.is_err(), "UNIQUE rejects duplicate names");
}

#[test]
fn migrate_is_idempotent_after_v4() {
    let (_dir, vault) = common::vault();
    let conn = vault.connection().expect("conn");
    downgrade_to_v3_with_bad_data(conn);

    migrate(conn).expect("first migrate");
    migrate(conn).expect("second migrate is a no-op");

    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("version");
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
}
