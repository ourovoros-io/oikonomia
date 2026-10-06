//! Ledger schema for the encrypted vault database, and its migrations.
//!
//! `vault_meta.schema_version` records the last migration a vault has run.
//! [`migrate`] runs every step of [`MIGRATIONS`] above that version, in order.
//! Each step runs in a transaction of its own, and the runner writes the
//! step's version inside that same transaction. A step is therefore either
//! applied and recorded, or not applied at all: a crash or an error part-way
//! can never leave a half-built schema that the next unlock would try to
//! build again on top of itself.
//!
//! # Adding a migration
//!
//! 1. Write `fn migrate_vN(tx: &Transaction<'_>) -> Result<()>` that makes the
//!    change through `tx`. It must not begin, commit or roll back a
//!    transaction, and must not write `schema_version`; the runner does both.
//! 2. Append `(N, migrate_vN)` to [`MIGRATIONS`] and set
//!    [`CURRENT_SCHEMA_VERSION`] to `N`.
//! 3. Add `tests/migration_vN.rs` that takes a vault from `N - 1` to `N`.
//!
//! Never edit a step that has shipped: a vault that already ran it will not
//! run it again, so the change would reach new vaults only.

use rusqlite::{Connection, Transaction};

use crate::error::{Error, Result};

/// Latest schema version applied by migrations.
pub const CURRENT_SCHEMA_VERSION: i64 = 7;

/// One schema change, made through the transaction the runner opened for it.
type Migration = fn(&Transaction<'_>) -> Result<()>;

/// Every migration with the version it brings a vault to, oldest first.
///
/// Versions start at 2 (a new vault is written at 1) and have no gaps; the
/// last one is [`CURRENT_SCHEMA_VERSION`].
const MIGRATIONS: &[(i64, Migration)] = &[
    (2, migrate_v2),
    (3, migrate_v3),
    (4, migrate_v4),
    (5, migrate_v5),
    (6, migrate_v6),
    (7, migrate_v7),
];

/// Applies pending migrations. Safe to call on every unlock.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] when the vault's schema version is newer than
///   [`CURRENT_SCHEMA_VERSION`]: it was written by a later build, and this one
///   does not know its schema. Nothing is changed.
/// - [`Error::VaultCorrupt`] when existing data cannot satisfy a constraint a
///   step adds (the v5 step and journal lines that are not debit XOR credit).
/// - [`Error::Io`] on SQL failures.
///
/// A failed step is rolled back and the steps before it stay applied.
pub fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    if version > CURRENT_SCHEMA_VERSION {
        return Err(Error::VaultCorrupt(format!(
            "vault schema version {version} is newer than this build supports \
             ({CURRENT_SCHEMA_VERSION})"
        )));
    }

    for (target, step) in MIGRATIONS {
        if version < *target {
            apply_migration(conn, *target, *step)?;
        }
    }
    Ok(())
}

/// Runs `step` and records `target` as the vault's version, atomically.
fn apply_migration(conn: &Connection, target: i64, step: Migration) -> Result<()> {
    // Dropping the transaction on an early return rolls it back.
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;

    step(&tx)?;
    tx.execute(
        "UPDATE vault_meta SET schema_version = ?1 WHERE id = 1",
        [target],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    tx.commit().map_err(|err| Error::Io(err.to_string()))
}

/// v2: the ledger tables (entities, accounts, journal) and app settings.
fn migrate_v2(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(SCHEMA_V2)
        .map_err(|err| Error::Io(err.to_string()))
}

/// v3: documents stored in the vault.
fn migrate_v3(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(SCHEMA_V3)
        .map_err(|err| Error::Io(err.to_string()))
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
/// The whole rebuild (orphan delete, dedup renames, table rebuild) and the
/// version bump share the runner's transaction: `SQLite` DDL is
/// transactional, so a crash mid-migration rolls back wholesale rather than
/// leave `documents_v4` half-built with `schema_version` still at 3. That
/// shape would make every later unlock re-enter this function and fail
/// forever on `documents_v4` already existing (a bricked vault).
fn migrate_v4(tx: &Transaction<'_>) -> Result<()> {
    let deleted = tx
        .execute("DELETE FROM documents WHERE entry_id IS NULL", [])
        .map_err(|err| Error::Io(err.to_string()))?;
    if deleted > 0 {
        log::info!("v4 migration: deleted {deleted} unlinked document(s)");
    }

    dedup_document_names(tx)?;

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
            SELECT id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at,
                   analysis_json
            FROM documents;
        DROP TABLE documents;
        ALTER TABLE documents_v4 RENAME TO documents;
        CREATE INDEX IF NOT EXISTS idx_documents_entity ON documents(entity_id);
        CREATE INDEX IF NOT EXISTS idx_documents_entry ON documents(entry_id);
        ",
    )
    .map_err(|err| Error::Io(err.to_string()))
}

/// v5: `journal_lines` must be debit XOR credit. `SQLite` cannot add a CHECK
/// in place, so the table is rebuilt. Existing data is copied only if every
/// line already satisfies the invariant — a violating row is a corrupt book
/// and must fail the migration rather than be silently dropped.
fn migrate_v5(tx: &Transaction<'_>) -> Result<()> {
    let bad: i64 = tx
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
        ",
    )
    .map_err(|err| Error::Io(err.to_string()))
}

/// v6: per-entry owner-only hidden flag on `journal_entries`.
///
/// `0` = visible (default), `1` = hidden: left out of the journal CSV export
/// and of the accountant profit and loss
/// ([`crate::ledger::profit_and_loss_export`]). The owner still sees hidden
/// rows in list/get/register. Existing pre-v6 rows become visible via
/// `DEFAULT 0`. Not extra encryption.
fn migrate_v6(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "
        ALTER TABLE journal_entries
            ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
        ",
    )
    .map_err(|err| Error::Io(err.to_string()))
}

/// v7: local-only recurring entry templates in the encrypted vault.
///
/// No calendar sync, no network, no auto-post. `next_date` is advanced only
/// after an explicit user post. Role-account columns match
/// [`crate::ledger::PostSimpleEntry`].
fn migrate_v7(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS recurring_templates (
            id TEXT PRIMARY KEY NOT NULL,
            entity_id TEXT NOT NULL REFERENCES entities(id),
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
            cadence TEXT NOT NULL,
            day_of_month INTEGER CHECK (
                day_of_month IS NULL OR (day_of_month >= 1 AND day_of_month <= 31)
            ),
            category_account_id TEXT REFERENCES accounts(id),
            wallet_account_id TEXT REFERENCES accounts(id),
            payable_account_id TEXT REFERENCES accounts(id),
            from_account_id TEXT REFERENCES accounts(id),
            to_account_id TEXT REFERENCES accounts(id),
            memo TEXT,
            next_date TEXT NOT NULL,
            bill_status TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_recurring_entity
            ON recurring_templates(entity_id);
        CREATE INDEX IF NOT EXISTS idx_recurring_entity_next
            ON recurring_templates(entity_id, next_date);
        ",
    )
    .map_err(|err| Error::Io(err.to_string()))
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
    use super::{CURRENT_SCHEMA_VERSION, MIGRATIONS, suffixed_name};

    #[test]
    fn migrations_run_from_two_to_the_current_version_without_gaps() {
        let versions: Vec<i64> = MIGRATIONS.iter().map(|(target, _)| *target).collect();
        let expected: Vec<i64> = (2..=CURRENT_SCHEMA_VERSION).collect();

        assert_eq!(versions, expected);
    }

    #[test]
    fn suffixed_name_inserts_before_extension() {
        assert_eq!(suffixed_name("invoice.pdf", 2), "invoice (2).pdf");
        assert_eq!(suffixed_name("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(suffixed_name("notes", 3), "notes (3)");
        assert_eq!(suffixed_name(".hidden", 2), ".hidden (2)");
    }
}
