//! Ledger schema for the encrypted vault database.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Latest schema version applied by migrations.
pub const CURRENT_SCHEMA_VERSION: i64 = 4;

/// Apply pending migrations. Safe to call on every unlock.
///
/// # Errors
///
/// Returns [`Error::Io`] on SQL failures.
pub fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    if version < 2 {
        conn.execute_batch(SCHEMA_V2)
            .map_err(|err| Error::Io(err.to_string()))?;
    }

    if version < 3 {
        conn.execute_batch(SCHEMA_V3)
            .map_err(|err| Error::Io(err.to_string()))?;
    }

    if version < 4 {
        migrate_v4(conn)?;
    }

    if version < CURRENT_SCHEMA_VERSION {
        conn.execute(
            "UPDATE vault_meta SET schema_version = ?1 WHERE id = 1",
            [CURRENT_SCHEMA_VERSION],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    }

    Ok(())
}

const SCHEMA_V2: &str = r"
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    base_currency TEXT NOT NULL,
    fiscal_year_start_month INTEGER NOT NULL DEFAULT 1,
    chart_template TEXT NOT NULL,
    created_at TEXT NOT NULL,
    archived_at TEXT
);

CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY NOT NULL,
    entity_id TEXT NOT NULL REFERENCES entities(id),
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    account_type TEXT NOT NULL,
    parent_id TEXT REFERENCES accounts(id),
    is_active INTEGER NOT NULL DEFAULT 1,
    is_system INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    UNIQUE (entity_id, code)
);

CREATE TABLE IF NOT EXISTS journal_entries (
    id TEXT PRIMARY KEY NOT NULL,
    entity_id TEXT NOT NULL REFERENCES entities(id),
    entry_date TEXT NOT NULL,
    description TEXT NOT NULL,
    reference TEXT,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    posted_at TEXT,
    voided_by_entry_id TEXT REFERENCES journal_entries(id)
);

CREATE TABLE IF NOT EXISTS journal_lines (
    id TEXT PRIMARY KEY NOT NULL,
    entry_id TEXT NOT NULL REFERENCES journal_entries(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    debit_minor INTEGER NOT NULL DEFAULT 0 CHECK (debit_minor >= 0),
    credit_minor INTEGER NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
    memo TEXT,
    line_order INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_accounts_entity ON accounts(entity_id);
CREATE INDEX IF NOT EXISTS idx_entries_entity_date ON journal_entries(entity_id, entry_date);
CREATE INDEX IF NOT EXISTS idx_lines_entry ON journal_lines(entry_id);
CREATE INDEX IF NOT EXISTS idx_lines_account ON journal_lines(account_id);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
";

const SCHEMA_V3: &str = r"
CREATE TABLE IF NOT EXISTS documents (
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

CREATE INDEX IF NOT EXISTS idx_documents_entity ON documents(entity_id);
CREATE INDEX IF NOT EXISTS idx_documents_entry ON documents(entry_id);
";

/// v4: documents must be linked (`entry_id NOT NULL`) and uniquely named per
/// book (`UNIQUE(entity_id, filename)`). `SQLite` cannot add constraints in
/// place, so the table is rebuilt after cleaning existing data.
fn migrate_v4(conn: &Connection) -> Result<()> {
    let deleted = conn
        .execute("DELETE FROM documents WHERE entry_id IS NULL", [])
        .map_err(|err| Error::Io(err.to_string()))?;
    if deleted > 0 {
        log::info!("v4 migration: deleted {deleted} unlinked document(s)");
    }

    dedup_document_names(conn)?;

    conn.execute_batch(
        "
        CREATE TABLE documents_v4 (
            id TEXT PRIMARY KEY NOT NULL,
            entity_id TEXT NOT NULL REFERENCES entities(id),
            entry_id TEXT NOT NULL REFERENCES journal_entries(id),
            filename TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            data BLOB NOT NULL,
            created_at TEXT NOT NULL,
            analysis_json TEXT,
            UNIQUE (entity_id, filename)
        );
        INSERT INTO documents_v4
            SELECT id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at, analysis_json
            FROM documents;
        DROP TABLE documents;
        ALTER TABLE documents_v4 RENAME TO documents;
        CREATE INDEX IF NOT EXISTS idx_documents_entity ON documents(entity_id);
        CREATE INDEX IF NOT EXISTS idx_documents_entry ON documents(entry_id);
        ",
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    Ok(())
}

/// Give later-created duplicates a numeric suffix; the oldest keeps its name.
fn dedup_document_names(conn: &Connection) -> Result<()> {
    use std::collections::HashSet;

    let rows: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, entity_id, filename FROM documents
                 ORDER BY entity_id, created_at ASC, rowid ASC",
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        let mapped = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|err| Error::Io(err.to_string()))?;
        let mut out = Vec::new();
        for row in mapped {
            out.push(row.map_err(|err| Error::Io(err.to_string()))?);
        }
        out
    };

    let mut taken: HashSet<(String, String)> = HashSet::new();
    for (id, entity_id, filename) in rows {
        let mut name = filename.clone();
        let mut n = 2;
        while taken.contains(&(entity_id.clone(), name.clone())) {
            name = suffixed_name(&filename, n);
            n += 1;
        }
        if name != filename {
            log::info!("v4 migration: renamed duplicate document to {name}");
            conn.execute(
                "UPDATE documents SET filename = ?1 WHERE id = ?2",
                rusqlite::params![name, id],
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        }
        taken.insert((entity_id, name));
    }
    Ok(())
}

/// `invoice.pdf` + 2 → `invoice (2).pdf`; extensionless names get ` (2)`.
fn suffixed_name(filename: &str, n: usize) -> String {
    match filename.rfind('.') {
        Some(dot) if dot > 0 => {
            format!("{} ({n}){}", &filename[..dot], &filename[dot..])
        }
        _ => format!("{filename} ({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::suffixed_name;

    #[test]
    fn suffixed_name_inserts_before_extension() {
        assert_eq!(suffixed_name("invoice.pdf", 2), "invoice (2).pdf");
        assert_eq!(suffixed_name("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(suffixed_name("notes", 3), "notes (3)");
        assert_eq!(suffixed_name(".hidden", 2), ".hidden (2)");
    }
}
