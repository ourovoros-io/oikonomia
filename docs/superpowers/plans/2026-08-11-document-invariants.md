# Document Invariants (No Orphans, Unique Names) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce two vault invariants at the database level — every document row is linked to an entry (`entry_id NOT NULL`) and filenames are unique per book (`UNIQUE(entity_id, filename)`) — with a pure analyze step and transactional entry+document posting.

**Architecture:** Schema v4 rebuilds the `documents` table (auto-clean orphans, suffix duplicate names, add both constraints). `save_document` gains a mandatory `entry_id` — so `link_document_to_entry`/`unlink_document` are deleted outright, `DocumentMeta.entry_id` becomes non-optional, and linking only ever happens inside two transactional paths: `attach_document` and the new `post_simple_entry_with_document` (which composes an untransactioned `pub(crate)` variant of `post_simple_entry` to avoid nested BEGIN). The analyze commands stop persisting; the frontend re-supplies bytes or the file path at post time.

**Tech Stack:** Rust (rusqlite/SQLCipher), Tauri 2, React 19 + Vite + Tailwind.

Spec: `docs/superpowers/specs/2026-08-11-document-invariants-design.md`
Branch: continues on `feature/documents-entry-detail`.

**Deviations from the spec, forced by grounding (already sound, note in reports):**
- `link_document_to_entry` is deleted, not made `pub(crate)`: with `entry_id NOT NULL`, `save_document` must write the link in its own INSERT, so a separate link step has nothing to do.
- `DocumentMeta.entry_id` changes from `Option<JournalEntryId>` to `JournalEntryId` (TS `entry_id: string`).
- Between Tasks 3 and 4 the web app compiles but is not runnable (TS still references commands Task 3 deleted); Task 4 restores runtime coherence. Gates are compile/test only in between.

## Global Constraints

- All business logic lives in Rust (`oikonomia-core`); the UI never re-implements validation or filtering.
- Money is integer minor units (`i64`). Dates cross IPC as ISO strings.
- Vault data encrypted at rest; decrypted bytes reach disk only via `document_export`.
- No capability changes; no network permission; no new dependencies (core already has `log` via workspace? — if `log` is absent from `crates/oikonomia-core/Cargo.toml`, add `log = "0.4"` there in Task 1; it is a facade with no runtime cost).
- No emojis anywhere. No `Co-Authored-By` in commits.
- Gate for every commit: `cargo fmt --all`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test -p oikonomia-core`; `cd web && npm run build && npm run lint`.
- Workspace lints deny unwrap/panic in non-test code; tests may `expect`.
- Every command keeps the `with_vault_blocking` / `await_blocking` patterns.

---

### Task 1: Storage v4 — schema migration, suffix helper, `save_document` rework

**Files:**
- Modify: `crates/oikonomia-core/src/db/schema.rs` (v4 migration + suffix helper + unit tests)
- Modify: `crates/oikonomia-core/src/documents/store.rs` (`save_document` signature, `DocumentMeta.entry_id`, delete `link_document_to_entry`/`unlink_document`, unique-name rejection, `attach_document` simplification, `meta_from_columns`)
- Modify: `crates/oikonomia-core/src/documents/mod.rs` (exports)
- Modify: `crates/oikonomia-core/Cargo.toml` (add `log = "0.4"` under `[dependencies]` if not present)
- Test: `crates/oikonomia-core/tests/documents_flow.rs` (rework), Create: `crates/oikonomia-core/tests/migration_v4.rs`

**Interfaces:**
- Consumes: existing `db::migrate` (pub), `validate_document_file`, `get_entry`.
- Produces (relied on by Tasks 2-4):
  - `CURRENT_SCHEMA_VERSION = 4`; `documents` has `entry_id TEXT NOT NULL` + `UNIQUE(entity_id, filename)`.
  - `pub fn save_document(conn, entity_id: EntityId, entry_id: JournalEntryId, filename: &str, mime_type: &str, data: &[u8]) -> Result<DocumentMeta>` — rejects a duplicate name with `Error::Validation("a document named {name} already exists in this book")`.
  - `pub struct DocumentMeta { ..., pub entry_id: JournalEntryId, ... }` (non-optional).
  - `pub fn attach_document(conn, entity_id, entry_id, filename, mime_type, data) -> Result<DocumentMeta>` — unchanged signature; now `get_entry` + entity check + `save_document` (single INSERT, no tx needed).
  - `link_document_to_entry` and `unlink_document` no longer exist.

- [ ] **Step 1: Write the failing migration test**

Create `crates/oikonomia-core/tests/migration_v4.rs`:

```rust
//! v3 → v4 migration: orphan cleanup, duplicate-name suffixing, constraints.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::db::migrate;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

/// Rebuild the v3 documents shape (nullable entry_id, no unique index) and
/// seed it with an orphan and a same-book name clash.
fn downgrade_to_v3_with_bad_data(conn: &Connection) {
    conn.execute_batch(
        "
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
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    downgrade_to_v3_with_bad_data(conn);

    migrate(conn).expect("migrate v3 -> v4");

    let version: i64 = conn
        .query_row("SELECT schema_version FROM vault_meta WHERE id = 1", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 4);

    let orphans: i64 = conn
        .query_row("SELECT COUNT(1) FROM documents WHERE id = 'd1'", [], |r| r.get(0))
        .expect("orphans");
    assert_eq!(orphans, 0, "orphan deleted by migration");

    let names: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT filename FROM documents ORDER BY created_at ASC, rowid ASC")
            .expect("stmt");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("rows");
        rows.map(|r| r.expect("row")).collect()
    };
    assert_eq!(
        names,
        vec!["invoice.pdf".to_owned(), "invoice (2).pdf".to_owned(), "notes".to_owned()],
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p oikonomia-core --test migration_v4`
Expected: FAIL — migration leaves version 3 untouched or the v4 assertions fail (no v4 path exists yet).

- [ ] **Step 3: Implement schema v4 in `db/schema.rs`**

3a. Bump the constant: `pub const CURRENT_SCHEMA_VERSION: i64 = 4;`

3b. Add the v4 step to `migrate` after the v3 block:

```rust
    if version < 4 {
        migrate_v4(conn)?;
    }
```

3c. Add the migration, dedup logic, and suffix helper at the bottom of the file:

```rust
/// v4: documents must be linked (`entry_id NOT NULL`) and uniquely named per
/// book (`UNIQUE(entity_id, filename)`). SQLite cannot add constraints in
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
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
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
```

If `log` is missing from `crates/oikonomia-core/Cargo.toml`, add `log = "0.4"` (or `log = { workspace = true }` if the workspace defines it) under `[dependencies]`.

- [ ] **Step 4: Rework `documents/store.rs`**

4a. `DocumentMeta.entry_id` becomes non-optional:

```rust
    /// Linked entry (a document cannot exist without one).
    pub entry_id: JournalEntryId,
```

4b. `save_document` gains `entry_id`, writes it, and rejects duplicate names before inserting (constraint stays as backstop):

```rust
/// Store a document blob linked to `entry_id` (already protected by `SQLCipher`).
///
/// # Errors
///
/// [`Error::Validation`] for invalid files or a duplicate filename in the
/// book; DB errors otherwise.
pub fn save_document(
    conn: &Connection,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> Result<DocumentMeta> {
    let name = filename.trim();
    let mime = resolve_mime(mime_type, name);
    validate_document_file(name, &mime, data.len() as u64)?;

    let clash: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM documents WHERE entity_id = ?1 AND filename = ?2",
            rusqlite::params![entity_id.0.to_string(), name],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if clash > 0 {
        return Err(Error::Validation(format!(
            "a document named {name} already exists in this book"
        )));
    }

    // Validation caps the size at 8 MiB, so the length always fits an i64.
    let size_bytes = i64::try_from(data.len()).unwrap_or(i64::MAX);

    let id = DocumentId::new();
    let created = now_utc_string();

    conn.execute(
        "
        INSERT INTO documents (
            id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at, analysis_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)
        ",
        rusqlite::params![
            id.0.to_string(),
            entity_id.0.to_string(),
            entry_id.0.to_string(),
            name,
            mime,
            size_bytes,
            data,
            created,
        ],
    )
    .map_err(|err| match err.sqlite_error_code() {
        // Backstop: the pre-check races nothing (single-writer vault), but a
        // constraint violation must still read as validation, not IO.
        Some(rusqlite::ErrorCode::ConstraintViolation) => Error::Validation(format!(
            "a document named {name} already exists in this book"
        )),
        _ => Error::Io(err.to_string()),
    })?;

    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id,
        filename: name.to_owned(),
        mime_type: mime,
        size_bytes,
        created_at: created,
    })
}
```

4c. Delete `link_document_to_entry` and `unlink_document` entirely.

4d. `attach_document` simplifies (the single INSERT is atomic; no transaction needed):

```rust
/// Validate and store a document linked to an existing entry (no OCR —
/// analysis belongs to the drop-zone flow).
///
/// # Errors
///
/// [`Error::NotFound`] for a missing entry, [`Error::Validation`] for an
/// entry in a different book, an invalid file, or a duplicate filename.
pub fn attach_document(
    conn: &Connection,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> Result<DocumentMeta> {
    let entry = get_entry(conn, entry_id)?;
    if entry.entry.entity_id != entity_id {
        return Err(Error::Validation("entry belongs to a different book".into()));
    }

    save_document(conn, entity_id, entry_id, filename, mime_type, data)
}
```

4e. `meta_from_columns` maps the now non-nullable column:

```rust
type MetaColumns = (String, String, String, String, String, i64, String);

fn meta_from_columns(raw: MetaColumns) -> Result<DocumentMeta> {
    let (id_s, entity_s, entry_s, filename, mime_type, size_bytes, created_at) = raw;
    Ok(DocumentMeta {
        id: DocumentId(parse_uuid(&id_s)?),
        entity_id: EntityId(parse_uuid(&entity_s)?),
        entry_id: JournalEntryId(parse_uuid(&entry_s)?),
        filename,
        mime_type,
        size_bytes,
        created_at,
    })
}
```

Update the two `query_map`/`query_row` tuple reads in `list_documents` and `get_document` from `Option<String>` to `String` for the `entry_id` column (position 2).

4f. Update `crates/oikonomia-core/src/documents/mod.rs` exports: remove `link_document_to_entry` and `unlink_document` from the `store` re-export list.

- [ ] **Step 5: Rework `documents_flow.rs`**

- Update the import block: drop `link_document_to_entry`, `unlink_document`; keep `DocumentId, attach_document, delete_document, get_document, list_documents, save_document`.
- Every existing `save_document(conn, entity_id, name, mime, data)` call gains an `entry_id` argument — each test posts (or reuses) an entry via the file's existing `post_entry` pattern and passes `entry.entry.id`.
- Delete `unlink_orphans_but_preserves_document` and the unlink half of `delete_and_unlink_missing_document_return_not_found` (rename it `delete_missing_document_returns_not_found`, keep the `delete_document` assertion).
- `delete_entity_with_linked_document`: replace `save_document` + `link_document_to_entry` with one `save_document(conn, entity_id, entry.entry.id, ...)` call.
- `list_get_delete_round_trip` and `list_documents_is_newest_first`: post one entry in setup and pass its id to every save; give the two documents in each test distinct filenames (they already differ).
- `save_document_rejects_unsupported_and_oversize`: post one entry and pass its id.
- `attach_document_saves_links_and_skips_analysis` / `attach_document_rejects_missing_and_wrong_entity_entry`: unchanged apart from compiling against the new signatures.
- Add the duplicate-name rejection test:

```rust
#[test]
fn save_document_rejects_duplicate_name_in_book() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let entry = post_reference_entry(conn, entity_id);

    save_document(conn, entity_id, entry.entry.id, "invoice.pdf", "application/pdf", b"%PDF-1.4")
        .expect("first save");
    let dup = save_document(conn, entity_id, entry.entry.id, "invoice.pdf", "application/pdf", b"%PDF-1.4");
    assert!(dup.is_err(), "same name in the same book must be rejected");
}
```

(`post_reference_entry` = extract the existing two-line posting used by `unlink_orphans_but_preserves_document` into a shared helper `fn post_reference_entry(conn: &Connection, entity_id: EntityId) -> PostedEntryView` at the top of the file, and reuse it in every test that needs an entry.)

- [ ] **Step 6: Run the core suite**

Run: `cargo test -p oikonomia-core`
Expected: PASS, including `migration_v4` and the schema unit tests. NOTE: `commands.rs` in the desktop crate will NOT compile yet (it still calls the old `save_document`/`link_document_to_entry`) — that is Step 7's job; `cargo test -p oikonomia-core` alone must be green first.

- [ ] **Step 7: Patch the desktop crate to compile against the new core surface**

In `apps/desktop/src-tauri/src/commands.rs` (temporary state — Task 3 finishes the redesign):
- Import list: drop `link_document_to_entry`, `unlink_document`, `save_document`, `save_analysis_json` if now unused after these edits (verify each with a grep before removing).
- `analyze_with_vault`: this is the load-bearing temporary change — it can no longer save before an entry exists. Replace its body so it only reads what analysis needs (Task 3 renames it `analyze_readonly` and deletes the persistence for good):

```rust
fn analyze_with_vault(
    vault: &Mutex<Vault>,
    model_dir: &Path,
    entity_id: EntityId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> CommandResult<DocumentSuggestion> {
    let mime = oikonomia_core::documents::resolve_mime(mime_type, filename);
    oikonomia_core::documents::validate_document_file(filename, &mime, data.len() as u64)?;

    let (accounts, entity) = {
        let guard = crate::state::lock_vault(vault);
        let conn = guard.connection()?;
        (
            suggest_accounts_for_entity(conn, entity_id)?,
            get_entity(conn, entity_id)?,
        )
    };

    let suggestion = analyze_document_bytes(
        oikonomia_core::documents::DocumentId::new(),
        filename,
        &mime,
        data,
        &accounts,
        &entity.base_currency,
        Some(model_dir),
    )?;

    Ok(suggestion)
}
```

(The throwaway `DocumentId::new()` placates the current `analyze_document_bytes` signature; Task 3 removes the parameter and the field.)
- Delete the `document_link_entry` and `document_unlink` commands and their two lines in `lib.rs`'s `generate_handler!`.

Frontend note: `web` still compiles — `api.ts` keeps `documentLinkEntry`/`documentUnlink` methods pointing at now-deleted commands (dead at runtime until Task 4; compile-only gates in between, per the header deviation note).

- [ ] **Step 8: Full gate and commit**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run build && npm run lint && cd ..
git add crates/oikonomia-core apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: schema v4 - documents require an entry and a unique name

A document row can no longer exist unlinked (entry_id NOT NULL) or share
a filename within a book (UNIQUE). The migration deletes existing
orphans and suffixes later-created duplicates, keeping the oldest name.
save_document now takes the entry id directly, so the separate
link/unlink steps disappear; analyze no longer persists anything."
```

---

### Task 2: Core `post_simple_entry_with_document` (transactional post + save)

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/journals.rs` (extract `pub(crate) post_simple_entry_unchecked`)
- Modify: `crates/oikonomia-core/src/documents/store.rs` (new fn)
- Modify: `crates/oikonomia-core/src/documents/mod.rs` (export)
- Test: `crates/oikonomia-core/tests/documents_flow.rs`

**Interfaces:**
- Consumes: Task 1's `save_document(conn, entity_id, entry_id, ...)`, existing `post_simple_entry`, `save_analysis_json`, `PostSimpleEntry`, `PostedEntryView`.
- Produces (relied on by Task 3):
  - `pub fn post_simple_entry_with_document(conn, input: &PostSimpleEntry, filename: &str, mime_type: &str, data: &[u8], analysis_json: Option<&str>) -> Result<(PostedEntryView, DocumentMeta)>` — one transaction; any failure (including a name clash) rolls back the entry too.
  - `pub(crate) fn post_simple_entry_unchecked(conn, input) -> Result<PostedEntryView>` in `ledger::journals` (no own transaction; caller owns it).

- [ ] **Step 1: Write the failing tests**

Append to `crates/oikonomia-core/tests/documents_flow.rs` (add `post_simple_entry_with_document` to the documents import and `PostSimpleEntry, SimpleEntryKind` plus `list_entries, EntryFilter` to the ledger import):

```rust
fn simple_expense_input(conn: &Connection, entity_id: EntityId, description: &str) -> PostSimpleEntry {
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    PostSimpleEntry {
        entity_id,
        kind: SimpleEntryKind::Expense,
        bill_status: None,
        entry_date: "2026-03-01".into(),
        description: description.into(),
        reference: None,
        amount_minor: 1_000,
        category_account_id: Some(food.id),
        wallet_account_id: Some(checking.id),
        payable_account_id: None,
        from_account_id: None,
        to_account_id: None,
    }
}

#[test]
fn post_with_document_is_atomic_and_stores_analysis() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let input = simple_expense_input(conn, entity_id, "Scanned groceries");
    let (view, meta) = post_simple_entry_with_document(
        conn,
        &input,
        "receipt.txt",
        "text/plain",
        b"TOTAL 10,00",
        Some("{\"notes\":\"scan\"}"),
    )
    .expect("post with document");

    assert_eq!(meta.entry_id, view.entry.id, "document linked to the new entry");

    let analysis: Option<String> = conn
        .query_row(
            "SELECT analysis_json FROM documents WHERE id = ?1",
            [meta.id.0.to_string()],
            |row| row.get(0),
        )
        .expect("row");
    assert_eq!(analysis.as_deref(), Some("{\"notes\":\"scan\"}"));
}

#[test]
fn post_with_document_name_clash_rolls_back_the_entry() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let first = simple_expense_input(conn, entity_id, "First");
    post_simple_entry_with_document(conn, &first, "bill.txt", "text/plain", b"a", None)
        .expect("first post");

    let before = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .len();

    let second = simple_expense_input(conn, entity_id, "Second");
    let clash =
        post_simple_entry_with_document(conn, &second, "bill.txt", "text/plain", b"b", None);
    assert!(clash.is_err(), "duplicate name must fail");

    let after = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .len();
    assert_eq!(after, before, "the entry must roll back with the document");
}

#[test]
fn post_with_document_invalid_file_rolls_back_everything() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let input = simple_expense_input(conn, entity_id, "Bad file");
    let result =
        post_simple_entry_with_document(conn, &input, "evil.exe", "application/x-msdownload", b"MZ", None);
    assert!(result.is_err(), "unsupported file must fail");

    let entries = list_entries(conn, entity_id, &EntryFilter::default()).expect("list");
    assert!(entries.is_empty(), "no entry may survive a failed document save");
    assert!(list_documents(conn, entity_id).expect("docs").is_empty());
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p oikonomia-core --test documents_flow`
Expected: COMPILE ERROR — `post_simple_entry_with_document` not found.

- [ ] **Step 3: Extract the untransactioned posting variant**

In `crates/oikonomia-core/src/ledger/journals.rs`, `post_simple_entry` currently validates, builds the two lines, and calls `post_entry` (which opens its own transaction — nesting one inside `post_simple_entry_with_document`'s transaction would fail with "cannot start a transaction within a transaction"). Split it:

```rust
/// Build and insert the simple-form entry without transaction management.
///
/// Callers own the transaction: [`post_simple_entry`] wraps this, and
/// `documents::post_simple_entry_with_document` composes it with the
/// document save inside one transaction.
pub(crate) fn post_simple_entry_unchecked(
    conn: &Connection,
    input: &PostSimpleEntry,
) -> Result<PostedEntryView> {
    if input.amount_minor <= 0 {
        return Err(Error::Validation("amount must be positive".into()));
    }

    let (debit_account, credit_account) = simple_entry_sides(conn, input)?;
    if debit_account == credit_account {
        return Err(Error::Validation(
            "entry needs two different accounts".into(),
        ));
    }

    let lines = vec![
        CreateJournalLine {
            account_id: debit_account,
            debit_minor: input.amount_minor,
            credit_minor: 0,
            memo: None,
        },
        CreateJournalLine {
            account_id: credit_account,
            debit_minor: 0,
            credit_minor: input.amount_minor,
            memo: None,
        },
    ];

    insert_posted_entry(
        conn,
        &PostJournal {
            entity_id: input.entity_id,
            entry_date: input.entry_date.clone(),
            description: input.description.clone(),
            reference: input.reference.clone(),
            lines,
        },
    )
}
```

and `post_simple_entry` becomes the transactional wrapper:

```rust
pub fn post_simple_entry(conn: &Connection, input: &PostSimpleEntry) -> Result<PostedEntryView> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let view = post_simple_entry_unchecked(&tx, input)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(view)
}
```

(Keep the existing doc comment on `post_simple_entry`; move nothing else.)

- [ ] **Step 4: Add the composed function in `documents/store.rs`**

Extend the ledger import to `use crate::ledger::{get_entry, list_accounts};` plus `use crate::ledger::journals::post_simple_entry_unchecked;` — if `journals` is not a public path inside the crate, add a `pub(crate) use journals::post_simple_entry_unchecked;` line to `crates/oikonomia-core/src/ledger/mod.rs` and import it from `crate::ledger` instead. Also import `PostSimpleEntry, PostedEntryView` from `crate::ledger`.

```rust
/// Post a simple entry and store its document in one transaction.
///
/// A duplicate filename (or any other failure) rolls back the entry too —
/// the vault never holds a document without its entry or vice versa from
/// this path.
///
/// # Errors
///
/// All [`post_simple_entry`](crate::ledger::post_simple_entry) and
/// [`save_document`] errors.
pub fn post_simple_entry_with_document(
    conn: &Connection,
    input: &PostSimpleEntry,
    filename: &str,
    mime_type: &str,
    data: &[u8],
    analysis_json: Option<&str>,
) -> Result<(PostedEntryView, DocumentMeta)> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;

    let view = post_simple_entry_unchecked(&tx, input)?;
    let meta = save_document(&tx, input.entity_id, view.entry.id, filename, mime_type, data)?;
    if let Some(json) = analysis_json {
        save_analysis_json(&tx, meta.id, json)?;
    }

    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok((view, meta))
}
```

Export `post_simple_entry_with_document` from `documents/mod.rs`.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p oikonomia-core`
Expected: PASS (all suites).

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run build && npm run lint && cd ..
git add crates/oikonomia-core
git commit -m "feat: transactional post-with-document in core

Posting a scanned entry and storing its file is one transaction: a
duplicate filename rolls back the entry too, so the no-orphan invariant
holds from the drop-zone path. post_simple_entry splits out an
untransactioned variant to compose without nested BEGIN."
```

---

### Task 3: Commands — pure analyze, with-document posting, suggestion field removal

**Files:**
- Modify: `crates/oikonomia-core/src/documents/analyze.rs` (drop `document_id` from `DocumentSuggestion` and `analyze_document_bytes`)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (rename `analyze_with_vault` → `analyze_readonly`, drop the throwaway id; two new commands)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register)

**Interfaces:**
- Consumes: Task 2's `post_simple_entry_with_document`, existing base64 gate pattern, `validate_document_file`, `resolve_mime`.
- Produces (relied on by Task 4):
  - Commands `entry_post_simple_with_document(input, filename, mime_type, data_base64, analysis_json?) -> PostedEntryView` and `entry_post_simple_with_document_path(input, path, analysis_json?) -> PostedEntryView`.
  - `DocumentSuggestion` without `document_id` (TS side updated in Task 4).

- [ ] **Step 1: Remove `document_id` from the suggestion**

In `crates/oikonomia-core/src/documents/analyze.rs`:
- Delete the `pub document_id: DocumentId` field (analyze.rs:46-47) and the `document_id: DocumentId` parameter of `analyze_document_bytes` (analyze.rs:111).
- `finalize_suggestion` (analyze.rs:221,226): remove the `document_id` parameter and the `s.document_id = document_id;` line; fix its call site (analyze.rs:186).
- The default-suggestion constructor around analyze.rs:261: remove the `document_id: DocumentId(uuid::Uuid::nil())` field.
- Remove the now-unused `DocumentId` (and `uuid`) imports from this file if nothing else uses them.

- [ ] **Step 2: Finish the command-side redesign**

In `apps/desktop/src-tauri/src/commands.rs`:
- Rename `analyze_with_vault` to `analyze_readonly`, update both call sites (`document_analyze`, `document_analyze_path`), and drop the throwaway `DocumentId::new()` argument now that the parameter is gone. Update the function's doc comment to:

```rust
/// Analyze a document in memory and suggest a draft entry. Persists
/// nothing: the file is stored only when the entry is posted
/// (`entry_post_simple_with_document`), keeping the no-orphan invariant.
```

- Add the two posting commands after `entry_post_simple`:

```rust
/// Post a simple entry together with its analyzed document (one transaction).
#[tauri::command]
pub async fn entry_post_simple_with_document(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
    filename: String,
    mime_type: String,
    data_base64: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    // Base64 inflates by 4/3: reject oversized picks before decoding so a huge
    // file cannot balloon memory (same gate as document_analyze).
    let max_base64_len = oikonomia_core::documents::MAX_DOCUMENT_BYTES / 3 * 4 + 4;
    if data_base64.len() > max_base64_len {
        return Err(CommandError {
            code: "validation".into(),
            message: "file too large (max 8 MB)".into(),
        });
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| CommandError {
            code: "validation".into(),
            message: format!("invalid file data: {e}"),
        })?;

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        let (view, _meta) = post_simple_entry_with_document(
            conn,
            &input,
            &filename,
            &mime_type,
            &data,
            analysis_json.as_deref(),
        )?;
        Ok(view)
    })
    .await
}

/// Post a simple entry with a document from a filesystem path (native drop).
/// The file is re-read and re-validated at post time; if it moved since the
/// drop, a clean error surfaces and nothing is written.
#[tauri::command]
pub async fn entry_post_simple_with_document_path(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
    path: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    let vault = state.vault();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&path);
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("document")
            .to_owned();

        // Reject oversized/unsupported files from metadata alone before reading.
        let meta = std::fs::metadata(&path).map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not read the dropped file: {e}"),
        })?;
        let mime = oikonomia_core::documents::resolve_mime("", &filename);
        oikonomia_core::documents::validate_document_file(&filename, &mime, meta.len())?;

        let data = std::fs::read(&path).map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not read the dropped file: {e}"),
        })?;

        let guard = crate::state::lock_vault(&vault);
        let conn = guard.connection()?;
        let (view, _meta) = post_simple_entry_with_document(
            conn,
            &input,
            &filename,
            &mime,
            &data,
            analysis_json.as_deref(),
        )?;
        Ok(view)
    }))
    .await
}
```

- Extend the `oikonomia_core::documents` import with `post_simple_entry_with_document` (and prune anything the compiler now flags as unused).
- Register both commands in `lib.rs`'s `generate_handler!` after `commands::entry_post_simple`.

- [ ] **Step 3: Gate and commit**

`web` still compiles (TS `DocumentSuggestion.document_id` is now a phantom field the backend never sends — removed in Task 4).

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run build && npm run lint && cd ..
git add crates/oikonomia-core/src/documents/analyze.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: pure analyze and with-document posting commands

Analysis no longer touches the vault; the suggestion carries no document
id because no document exists yet. Two new commands post entry+file in
one core transaction - bytes re-supplied for picked files, the path
re-read and re-validated for native drops."
```

---

### Task 4: Frontend — pending document flow, delete-not-unlink, simplified library

**Files:**
- Modify: `web/src/lib/api.ts`
- Modify: `web/src/components/DocumentDropZone.tsx`
- Modify: `web/src/pages/TransactionsPage.tsx`
- Modify: `web/src/components/EntryDetailModal.tsx`
- Modify: `web/src/pages/DocumentsPage.tsx`

**Interfaces:**
- Consumes: Task 3's commands; existing `fileToBase64`/`mimeFromName`.
- Produces: `PendingDocSource` type; `api.entryPostSimpleWithDocument`, `api.entryPostSimpleWithDocumentPath`; `DocumentMeta.entry_id: string`; no `documentUnlink`/`documentLinkEntry`/`DocumentSuggestion.document_id`.

- [ ] **Step 1: API surface (`web/src/lib/api.ts`)**

- `DocumentMeta.entry_id` becomes `entry_id: string` (non-null).
- Delete the `documentUnlink` and `documentLinkEntry` methods.
- Remove `document_id: string` from the `DocumentSuggestion` type.
- Add below `entryPostSimple`:

```ts
  /** Post a simple entry together with its analyzed document (one transaction). */
  entryPostSimpleWithDocument: (
    input: SimpleEntryInput,
    doc: { filename: string; mimeType: string; dataBase64: string },
    analysisJson?: string,
  ) =>
    call<PostedEntryView>('entry_post_simple_with_document', {
      input,
      filename: doc.filename,
      mimeType: doc.mimeType,
      dataBase64: doc.dataBase64,
      analysisJson: analysisJson ?? null,
    }),
  /** Same, for native drops: the backend re-reads the path at post time. */
  entryPostSimpleWithDocumentPath: (
    input: SimpleEntryInput,
    path: string,
    analysisJson?: string,
  ) =>
    call<PostedEntryView>('entry_post_simple_with_document_path', {
      input,
      path,
      analysisJson: analysisJson ?? null,
    }),
```

Hoist the existing inline input shape of `entryPostSimple` into a named exported type so all three methods share it (avoids a self-referential `Parameters<typeof api...>` inside the object literal):

```ts
/** Simple-form posting input; the kind → debit/credit mapping lives in Rust. */
export type SimpleEntryInput = {
  entity_id: string
  kind: 'expense' | 'income' | 'bill' | 'transfer'
  bill_status: 'paid' | 'unpaid' | 'pay_existing' | null
  entry_date: string
  description: string
  reference: string | null
  amount_minor: number
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  from_account_id: string | null
  to_account_id: string | null
}
```

and change `entryPostSimple` to `(input: SimpleEntryInput) => call<PostedEntryView>('entry_post_simple', { input })`. Note `entryPostSimple`'s current inline type marks `reference` and `bill_status` non-optional — keep the shape exactly as above (TransactionsPage already passes every field explicitly).

- Add near `DocumentMeta`:

```ts
/** Where a pending (not yet saved) document lives until the entry is posted. */
export type PendingDocSource =
  | { kind: 'file'; file: File }
  | { kind: 'path'; path: string }
```

- [ ] **Step 2: `DocumentDropZone` hands back the source**

- Props: `onSuggestion: (suggestion: DocumentSuggestion, source: PendingDocSource) => void` (import the type from `../lib/api`).
- `processFile`: `onSuggestion(suggestion, { kind: 'file', file })`.
- `processPath`: `onSuggestion(suggestion, { kind: 'path', path })`.

- [ ] **Step 3: `TransactionsPage` pending-document flow**

- Replace `const [linkedDocumentId, setLinkedDocumentId] = useState<string | null>(null)` with:

```tsx
  const [pendingDoc, setPendingDoc] = useState<PendingDocSource | null>(null)
  const [pendingAnalysis, setPendingAnalysis] = useState<string | null>(null)
```

(import `PendingDocSource` from `../lib/api`.)
- `applySuggestion(s: DocumentSuggestion, source: PendingDocSource)`: replace `setLinkedDocumentId(s.document_id)` with `setPendingDoc(source)` and `setPendingAnalysis(JSON.stringify(s))`; the drop-zone call site becomes `applySuggestion(s, source)` with the new second argument.
- In `onPost`, replace the `api.entryPostSimple` call plus the best-effort link block with:

```tsx
      const input = {
        entity_id: entity.id,
        kind,
        bill_status: kind === 'bill' ? billStatus : null,
        entry_date: date,
        description: description.trim(),
        reference: reference.trim() || null,
        amount_minor: minor,
        category_account_id: categoryId || null,
        wallet_account_id: walletId || null,
        payable_account_id: payableId || null,
        from_account_id: fromId || null,
        to_account_id: toId || null,
      }

      if (pendingDoc?.kind === 'file') {
        const dataBase64 = await fileToBase64(pendingDoc.file)
        await api.entryPostSimpleWithDocument(
          input,
          {
            filename: pendingDoc.file.name,
            mimeType: pendingDoc.file.type || mimeFromName(pendingDoc.file.name),
            dataBase64,
          },
          pendingAnalysis ?? undefined,
        )
      } else if (pendingDoc?.kind === 'path') {
        await api.entryPostSimpleWithDocumentPath(input, pendingDoc.path, pendingAnalysis ?? undefined)
      } else {
        await api.entryPostSimple(input)
      }
```

(import `fileToBase64, mimeFromName` from `../lib/files`; the success path replaces `setLinkedDocumentId(null)` with `setPendingDoc(null); setPendingAnalysis(null)`. On error the form stays filled — the existing catch already does that — so a name clash can be retried after the user resolves it.)
- The scan-notes banner (currently keyed on `linkedDocumentId`) keys on `pendingDoc` and its copy changes from "Document stored encrypted in your vault" to "Document will be stored encrypted when you save".
- Cancel/close paths (`setShowForm(false)` sites) also clear `setPendingDoc(null); setPendingAnalysis(null)` — nothing was written, so this is the whole cleanup.
- `docsByEntry`: the `if (!d.entry_id) continue` guard goes away (`entry_id` is always set now).

- [ ] **Step 4: `EntryDetailModal` — Remove becomes Delete**

- Rename state `unlinkId` → `deleteId`, `confirmUnlink` → `confirmDelete`; the call becomes `await api.documentDelete(deleteId)`.
- `ConfirmDialog`: `title="Delete document?"`, `body="This permanently deletes the file from your vault. It cannot be undone. The entry itself stays."`, `confirmLabel="Delete"`, add `danger`.
- The row button: `aria-label="Delete document"`, `title="Delete"`, icon `Trash2` (replace the `X` import if unused elsewhere in the file).

- [ ] **Step 5: `DocumentsPage` — strip the link machinery**

- Delete state `linkDocId`, `linkSearch`, the `linkCandidates` memo, the `linkTo` function, the entire link-picker `<Modal>` block, and the Link (`Link2`) row button; remove `Link2` and `Modal` imports if now unused, drop `linkDocId`/`linkSearch` resets from the entity effect, and remove `Field`/`Input` imports if the picker was their only use.
- The badge block simplifies — `doc.entry_id` is always set:

```tsx
                  <span className="max-w-48 truncate rounded-full bg-[var(--color-accent-soft)] px-2.5 py-1 text-xs text-[var(--color-accent)]">
                    {entryById.get(doc.entry_id)?.entry.description ?? 'Linked entry'}
                  </span>
```

- `anyBusy` stays `busyId !== null || deleteBusy`; remaining actions are View / Save a copy / Delete.

- [ ] **Step 6: Gate and commit**

```bash
cd web && npm run build && npm run lint && cd ..
cargo clippy --all-targets --all-features -- -D warnings
git add web/src
git commit -m "feat: pending-document posting, delete-not-unlink, simplified library

The form holds the picked file or dropped path in memory and posts
entry+document in one backend transaction; cancelling saves nothing.
Remove on the entry detail now deletes (orphans cannot exist), and the
Documents page drops the link picker and not-linked state entirely."
```

---

### Task 5: Full gate + combined live walkthrough

**Files:** none (verification only).

- [ ] **Step 1: Full gate**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run lint && npm run test && npm run build && cd ..
```

Expected: all clean (core suite includes `migration_v4` and the reworked `documents_flow`).

- [ ] **Step 2: Live walkthrough (user drives; combined with round 1's pending walkthrough)**

Launch via `make app`. Round-2 additions to verify, after the round-1 checklist:

1. Unlock an existing vault → migration runs silently; any previously "Not linked" files are gone from Documents; previously duplicated names show suffixes.
2. Drop a PDF → suggestion appears → CANCEL the form → Documents page shows nothing new (nothing was saved).
3. Drop the same PDF again → post it → entry + document appear together; paperclip on the row.
4. Post another entry attaching a file with the SAME filename → clear error naming the file; the entry list is unchanged (rolled back); rename the file on disk → retry succeeds.
5. Attach a duplicate-named file to an existing entry from the detail modal → same clear error.
6. Detail modal → Delete on an attachment → danger confirm → file gone from Documents too (not orphaned).
7. Documents page shows no "Not linked" badge and no Link action; every row names its entry.

- [ ] **Step 3: Fix anything found, re-run the gate, commit fixes**
