//! The schema of the encrypted vault database, and its migrations.
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
//! 4. Name the new version in the Schema section of `AGENTS.md`.
//!
//! Never edit a step that has shipped: a vault that already ran it will not
//! run it again, so the change would reach new vaults only.
//!
//! # What the schema allows and the application does not write
//!
//! The tables are wider than what the application stores in them. No
//! migration narrows them, because a rebuilt table buys nothing the readers
//! do not already enforce.
//!
//! - `journal_entries.status` is `TEXT NOT NULL` with no `CHECK`, so the
//!   schema permits any text, `draft` included. The application writes
//!   `posted` and nothing else. The entry list, the register, the journal
//!   export and every report select `status = 'posted'`, so a row that
//!   holds another status is in none of them and in no balance; read by its
//!   id, it is reported as a corrupt vault. The document queries join the
//!   entry without that test, so its documents are still listed.
//! - `accounts.parent_id` is a nullable reference to another account. The
//!   column is reserved: no statement after the `CREATE TABLE` names it, so
//!   every row written from now on holds NULL, and whatever an older row
//!   holds there is not read. `SQLite` still enforces the reference, so a
//!   value put there by other means must be the id of an account.

use crate::db::{collect_rows, read_column};
use crate::error::{DatabaseContext, Error, PrivateDetail, Result, VaultCorruption};
use rusqlite::{Connection, Transaction};
use std::collections::HashSet;

/// The schema version [`migrate`] brings a vault to.
pub const CURRENT_SCHEMA_VERSION: i64 = 9;

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
    (8, migrate_v8),
    (9, migrate_v9),
];

/// Applies the migrations a vault has not run yet.
///
/// A vault already at [`CURRENT_SCHEMA_VERSION`] is left untouched, so this is
/// called on every unlock.
///
/// # Errors
///
/// - [`Error::VaultTooNew`] when the vault's schema version is newer than
///   [`CURRENT_SCHEMA_VERSION`]: it was written by a later build, and this one
///   does not know its schema. Nothing is changed.
/// - [`Error::VaultCorrupt`] when existing data cannot satisfy a constraint a
///   step adds (the v5 step and journal lines that are not debit XOR credit),
///   or a step reads a stored value of the wrong kind (the v4 step and a
///   document name that is not text).
/// - [`Error::Database`] when the schema version cannot be read or a statement of a
///   step fails.
///
/// A failed step is rolled back and the steps before it stay applied.
pub fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM vault_meta WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .database("read schema version")?;

    if version > CURRENT_SCHEMA_VERSION {
        return Err(Error::VaultTooNew {
            found: version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    for (target, step) in MIGRATIONS {
        if version < *target {
            apply_migration(conn, *target, *step)?;
        }
    }
    Ok(())
}

/// Runs `step` and records `target` as the vault's version, atomically.
///
/// # Errors
///
/// The error of `step`, or [`Error::Database`] when the transaction cannot be
/// opened, the version cannot be written, or the commit fails. In every case
/// the vault is as it was before the call.
fn apply_migration(conn: &Connection, target: i64, step: Migration) -> Result<()> {
    // Dropping the transaction on an early return rolls it back.
    let tx = conn
        .unchecked_transaction()
        .database("begin migration transaction")?;

    step(&tx)?;
    tx.execute(
        "UPDATE vault_meta SET schema_version = ?1 WHERE id = 1",
        [target],
    )
    .database("record schema version")?;

    tx.commit().database("commit migration")
}

/// v2: the ledger tables (entities, accounts, journal) and app settings.
fn migrate_v2(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(SCHEMA_V2).database("create ledger tables")
}

/// v3: documents stored in the vault.
fn migrate_v3(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(SCHEMA_V3)
        .database("create documents table")
}

/// The tables and indexes [`migrate_v2`] creates.
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

/// The `documents` table as [`migrate_v3`] creates it; [`migrate_v4`]
/// rebuilds it with stricter constraints.
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

/// v4: every document is linked to an entry and uniquely named in its book.
///
/// The constraints are `entry_id NOT NULL` and `UNIQUE(entity_id, filename)`.
/// `SQLite` cannot add a constraint to an existing table
/// (<https://www.sqlite.org/lang_altertable.html>), so the table is rebuilt
/// after the existing data is cleaned: documents without an entry are deleted
/// and duplicate names get a numeric suffix.
///
/// The clean-up, the rebuild and the version bump share the runner's
/// transaction. `SQLite` DDL is transactional
/// (<https://www.sqlite.org/lang_transaction.html>), so a crash part-way
/// rolls all of it back. Otherwise `documents_v4` could be left half-built with
/// `schema_version` still at 3, and every later unlock would run this step
/// again and fail on the table that already exists.
fn migrate_v4(tx: &Transaction<'_>) -> Result<()> {
    let deleted = tx
        .execute("DELETE FROM documents WHERE entry_id IS NULL", [])
        .database("delete unlinked documents")?;
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
    .database("rebuild documents table")
}

/// v5: every journal line is a debit or a credit, never both and never
/// neither.
///
/// `SQLite` cannot add a `CHECK` to an existing table
/// (<https://www.sqlite.org/lang_altertable.html>), so the table is rebuilt.
/// Existing
/// lines are copied only if every one already satisfies the rule: a line that
/// does not is a corrupt book, and the step fails with
/// [`Error::VaultCorrupt`] instead of dropping it.
fn migrate_v5(tx: &Transaction<'_>) -> Result<()> {
    let violations: i64 = tx
        .query_row(
            "
            SELECT COUNT(1) FROM journal_lines
            WHERE (debit_minor = 0) = (credit_minor = 0)
            ",
            [],
            |row| row.get(0),
        )
        .database("count invalid journal lines")?;
    if violations > 0 {
        return Err(Error::VaultCorrupt(VaultCorruption::InvalidJournalLines {
            count: violations,
        }));
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
    .database("rebuild journal lines table")
}

/// v6: the `hidden` flag on `journal_entries`.
///
/// `0` is visible and `1` is hidden: left out of the journal CSV export and
/// of the accountant profit and loss
/// ([`crate::ledger::profit_and_loss_export`]). Inside the app the owner still
/// sees a hidden entry. Entries that existed before this step become visible
/// through `DEFAULT 0`. The flag is not a second layer of encryption.
fn migrate_v6(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "
        ALTER TABLE journal_entries
            ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
        ",
    )
    .database("add hidden column to journal entries")
}

/// v7: the `recurring_templates` table.
///
/// A template is posted only when the user asks. Nothing but such a post
/// advances `next_date`; the user can also set it by editing the template.
/// The five role-account columns are the fields of
/// [`crate::ledger::SimpleEntryRoleAccounts`], under their wire names.
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
    .database("create recurring templates table")
}

/// v8: two indexes for lookups that were made once per listed entry.
///
/// Nothing stored changes. Both lookups read a whole table, or a whole
/// account, for each entry of a listing, so the time grew with the square of
/// the book:
///
/// - `idx_entries_voided_by` finds the entry that names a given one in
///   `voided_by_entry_id`. Every query that asks whether an entry is voided
///   or active makes that lookup. The index is partial because most entries
///   are never voided; `SQLite` uses an `IS NOT NULL` partial index for an
///   equality on the same column
///   (<https://www.sqlite.org/partialindex.html#queries_using_partial_indexes>).
/// - `idx_lines_entry_account` finds the lines of one entry on one account,
///   which the account filter of the entry list asks for. It starts with
///   `entry_id`, so it also serves every lookup `idx_lines_entry` served, and
///   that index is dropped.
fn migrate_v8(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_entries_voided_by
            ON journal_entries(voided_by_entry_id)
            WHERE voided_by_entry_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_lines_entry_account
            ON journal_lines(entry_id, account_id);
        DROP INDEX IF EXISTS idx_lines_entry;
        ",
    )
    .database("index void links and entry lines")
}

/// v9: `replaces_entry_id`, the entry a correction took the place of.
///
/// A correction is a void plus a new entry, and until now nothing tied the new
/// entry to the one it replaced, so the audit copy the void wrote could not be
/// shown next to it. Entries corrected before this step keep a NULL link.
/// The index serves the lookups along that link and is partial because most
/// entries replace nothing.
fn migrate_v9(tx: &Transaction<'_>) -> Result<()> {
    // `ADD COLUMN` has no `IF NOT EXISTS`. The older migration tests take a
    // current vault back to an earlier version without dropping this column.
    let has_column: i64 = tx
        .query_row(
            "
            SELECT COUNT(1) FROM pragma_table_info('journal_entries')
            WHERE name = 'replaces_entry_id'
            ",
            [],
            |row| row.get(0),
        )
        .database("look for the replaces_entry_id column")?;
    if has_column == 0 {
        tx.execute_batch(
            "
            ALTER TABLE journal_entries
                ADD COLUMN replaces_entry_id TEXT REFERENCES journal_entries(id);
            ",
        )
        .database("add the replaces_entry_id column")?;
    }

    tx.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_entries_replaces
            ON journal_entries(replaces_entry_id)
            WHERE replaces_entry_id IS NOT NULL;
        ",
    )
    .database("link replacements to the entries they replace")
}

/// Renames documents so that no two in one book share a filename.
///
/// The oldest document of a name keeps it. Each later one gets the lowest
/// numeric suffix, from 2, that gives a name no older document of the book
/// has ([`suffixed_name`]).
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] naming the column when a document's id, entity
///   or filename is not stored as text.
/// - [`Error::Database`] when the documents cannot be read or a rename fails.
fn dedup_document_names(conn: &Connection) -> Result<()> {
    let rows: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, entity_id, filename FROM documents
                 ORDER BY entity_id, created_at ASC, rowid ASC",
            )
            .database("list document names")?;
        let mapped = stmt
            .query_map([], |row| Ok(map_document_name("list document names", row)))
            .database("list document names")?;
        collect_rows("list document names", mapped)?
    };

    let mut taken: HashSet<(String, String)> = HashSet::new();
    for (id, entity_id, filename) in rows {
        let mut name = filename.clone();
        let mut suffix = 2;
        while taken.contains(&(entity_id.clone(), name.clone())) {
            name = suffixed_name(&filename, suffix);
            suffix += 1;
        }
        if name != filename {
            // A document's name is the user's.
            log::info!(
                "v4 migration: renamed duplicate document to {}",
                PrivateDetail(&name)
            );
            conn.execute(
                "UPDATE documents SET filename = ?1 WHERE id = ?2",
                rusqlite::params![name, id],
            )
            .database("rename duplicate document")?;
        }
        taken.insert((entity_id, name));
    }
    Ok(())
}

/// Maps a row selected as `id, entity_id, filename` to those three texts.
///
/// The ids are compared and written back as the text they are stored as, so
/// they are not parsed here.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column that is not stored as text.
fn map_document_name(
    operation: &'static str,
    row: &rusqlite::Row<'_>,
) -> Result<(String, String, String)> {
    Ok((
        read_column(operation, row, 0)?,
        read_column(operation, row, 1)?,
        read_column(operation, row, 2)?,
    ))
}

/// Returns `filename` with ` (suffix)` before its last extension:
/// `invoice.pdf` and 2 give `invoice (2).pdf`.
///
/// A name with no extension, or with nothing before its only dot
/// (`.hidden`), gets the suffix at the end.
fn suffixed_name(filename: &str, suffix: usize) -> String {
    match filename.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => format!("{stem} ({suffix}).{extension}"),
        _ => format!("{filename} ({suffix})"),
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
