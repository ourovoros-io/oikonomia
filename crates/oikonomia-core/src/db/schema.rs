//! Ledger schema for the encrypted vault database.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Latest schema version applied by migrations.
pub const CURRENT_SCHEMA_VERSION: i64 = 3;

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
