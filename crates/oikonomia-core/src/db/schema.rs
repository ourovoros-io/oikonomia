//! Ledger schema for the encrypted vault database.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Latest schema version applied by migrations.
pub const CURRENT_SCHEMA_VERSION: i64 = 5;

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

    if version < 5 {
        migrate_v5(conn)?;
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
///
/// The whole rebuild (orphan delete, dedup renames, table rebuild, and the
/// version bump) runs in one transaction: `SQLite` DDL is transactional, so a
/// crash mid-migration must roll back wholesale rather than leave
/// `documents_v4` half-built with `schema_version` still at 3 — that shape
/// would make every subsequent unlock re-enter this function and fail
/// forever on `documents_v4` already existing (a bricked vault).
fn migrate_v4(conn: &Connection) -> Result<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;

    let deleted = tx
        .execute("DELETE FROM documents WHERE entry_id IS NULL", [])
        .map_err(|err| Error::Io(err.to_string()))?;
    if deleted > 0 {
        log::info!("v4 migration: deleted {deleted} unlinked document(s)");
    }

    dedup_document_names(&tx)?;

    tx.execute_batch(
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

    // Bump the version inside the same transaction: a crash after the
    // rebuild but before the version write must not strand a vault where
    // re-running the migration hits "documents_v4 already exists".
    tx.execute("UPDATE vault_meta SET schema_version = 4 WHERE id = 1", [])
        .map_err(|err| Error::Io(err.to_string()))?;

    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

/// v5: `journal_lines` must be debit XOR credit. `SQLite` cannot add a CHECK
/// in place, so the table is rebuilt. Existing data is copied only if every
/// line already satisfies the invariant — a violating row is a corrupt book
/// and must fail the migration rather than be silently dropped.
fn migrate_v5(conn: &Connection) -> Result<()> {
    let bad: i64 = conn
        .query_row(
            "
            SELECT COUNT(1) FROM journal_lines
            WHERE (debit_minor = 0) = (credit_minor = 0)
            ",
            [],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if bad > 0 {
        return Err(Error::VaultCorrupt(format!(
            "cannot migrate to v5: {bad} journal line(s) are not debit XOR credit"
        )));
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;

    tx.execute_batch(
        "
        CREATE TABLE journal_lines_v5 (
            id TEXT PRIMARY KEY NOT NULL,
            entry_id TEXT NOT NULL REFERENCES journal_entries(id) ON DELETE CASCADE,
            account_id TEXT NOT NULL REFERENCES accounts(id),
            debit_minor INTEGER NOT NULL DEFAULT 0 CHECK (debit_minor >= 0),
            credit_minor INTEGER NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
            memo TEXT,
            line_order INTEGER NOT NULL DEFAULT 0,
            CHECK ((debit_minor = 0) != (credit_minor = 0))
        );
        INSERT INTO journal_lines_v5
            SELECT id, entry_id, account_id, debit_minor, credit_minor, memo, line_order
            FROM journal_lines;
        DROP TABLE journal_lines;
        ALTER TABLE journal_lines_v5 RENAME TO journal_lines;
        CREATE INDEX IF NOT EXISTS idx_lines_entry ON journal_lines(entry_id);
        CREATE INDEX IF NOT EXISTS idx_lines_account ON journal_lines(account_id);
        UPDATE vault_meta SET schema_version = 5 WHERE id = 1;
        ",
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
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
