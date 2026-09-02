# Documents Surfacing, Entry Detail, and Entry Filters — Implementation Plan

> Completed and merged (see git history). Checkboxes below were never ticked during execution; do not re-execute.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface the encrypted documents already stored in the vault — paperclip badges and an entry-detail modal with an in-memory viewer, a Documents library page, attach-to-existing-entry — plus SQL-side search/date/account filters on the transactions list.

**Architecture:** New read-side functions in `oikonomia-core::documents` (list/get/delete/unlink) and an `EntryFilter` for `list_entries`, exposed through thin Tauri commands following the existing `with_vault_blocking` pattern. The React UI adds two modals (entry detail, document viewer), one page (Documents), and a filter toolbar — all reusing the existing `Modal`/`ConfirmDialog`/`ui.tsx` primitives. Decrypted bytes reach the UI only as base64 over IPC and live in a blob URL that is revoked when the viewer closes; plaintext reaches disk only via an explicit export command using `tauri-plugin-dialog`.

**Tech Stack:** Rust (rusqlite/SQLCipher), Tauri 2, React 19 + Vite + Tailwind v4, lucide-react icons.

Spec: `docs/superpowers/specs/2026-08-10-documents-entry-detail-design.md`

## Global Constraints

- All business logic lives in Rust (`oikonomia-core`), never the TypeScript UI (repo CLAUDE.md).
- Money is integer minor units (`i64`); never `f64` for currency.
- Dates cross IPC as ISO `YYYY-MM-DD` strings; core normalizes via `parse_date`/`format_date`.
- Report-style queries put entry predicates in inner-join/`EXISTS` subqueries, never in a `LEFT JOIN ... ON` clause.
- Vault data is encrypted at rest; decrypted document bytes must not be written to disk except via the explicit export action.
- v1 Tauri capabilities: no network permission. `tauri-plugin-dialog` is the only new dependency and needs no network.
- No emojis in UI copy, code, comments, or commit messages. No `Co-Authored-By` lines in commits.
- Workspace lints: `unwrap_used = deny`, `panic = deny`; tests may `#![expect(clippy::expect_used, ...)]`.
- Gate for every commit: `cargo fmt --all` then `cargo clippy --all-targets --all-features -- -D warnings` then `cargo test -p oikonomia-core`; for frontend tasks also `cd web && npm run build` (runs `tsc -b`).
- Every Tauri command runs vault work through `with_vault_blocking` / `await_blocking` (never lock the vault mutex on the main thread).
- Comment style: explain non-obvious intent and invariants only; blank lines between logical steps; `use_small_heuristics = "Default"` formatting.

---

### Task 1: Core document read-side (list, get, delete, unlink) + `created_at` on `DocumentMeta`

**Files:**
- Modify: `crates/oikonomia-core/src/documents/store.rs`
- Modify: `crates/oikonomia-core/src/documents/mod.rs` (exports)
- Test: `crates/oikonomia-core/tests/documents_flow.rs`

**Interfaces:**
- Consumes: existing `DocumentMeta`, `DocumentId`, `save_document`, `link_document_to_entry`, `crate::util::parse_uuid`.
- Produces (used by Task 3's commands):
  - `DocumentMeta` gains field `pub created_at: String` (RFC 3339, set by `save_document`).
  - `pub fn list_documents(conn: &Connection, entity_id: EntityId) -> Result<Vec<DocumentMeta>>` — newest first, never reads `data`.
  - `pub fn get_document(conn: &Connection, id: DocumentId) -> Result<(DocumentMeta, Vec<u8>)>`
  - `pub fn delete_document(conn: &Connection, id: DocumentId) -> Result<()>` — `Error::NotFound("document")` if missing.
  - `pub fn unlink_document(conn: &Connection, id: DocumentId) -> Result<()>` — sets `entry_id = NULL`; `NotFound` if missing.

- [ ] **Step 1: Write the failing tests**

Append to `crates/oikonomia-core/tests/documents_flow.rs`. Extend the existing import block (the file already imports `link_document_to_entry, save_document` and the ledger helpers):

```rust
use oikonomia_core::documents::{
    DocumentId, delete_document, get_document, link_document_to_entry, list_documents,
    save_document, unlink_document,
};
```

New tests (the file already has `setup_vault`/`setup_entity` helpers and a posted-entry example in `delete_entity_with_linked_document`):

```rust
#[test]
fn list_get_delete_round_trip() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let a = save_document(conn, entity_id, "a.txt", "text/plain", b"alpha").expect("save a");
    let b = save_document(conn, entity_id, "b.txt", "text/plain", b"bravo").expect("save b");

    let listed = list_documents(conn, entity_id).expect("list");
    assert_eq!(listed.len(), 2, "both documents listed");
    assert!(
        listed.iter().all(|m| !m.created_at.is_empty()),
        "created_at populated"
    );
    assert!(listed.iter().any(|m| m.id == a.id && m.filename == "a.txt"));

    let (meta, data) = get_document(conn, b.id).expect("get b");
    assert_eq!(meta.filename, "b.txt");
    assert_eq!(data, b"bravo");

    delete_document(conn, a.id).expect("delete a");
    assert!(get_document(conn, a.id).is_err(), "deleted document is gone");
    assert_eq!(list_documents(conn, entity_id).expect("list").len(), 1);
}

#[test]
fn unlink_orphans_but_preserves_document() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let checking = accounts.iter().find(|a| a.code == "1010").expect("1010");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100");

    let entry = post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: "2026-02-01".into(),
            description: "Lunch".into(),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: food.id,
                    debit_minor: 500,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: checking.id,
                    debit_minor: 0,
                    credit_minor: 500,
                    memo: None,
                },
            ],
        },
    )
    .expect("post");

    let meta = save_document(conn, entity_id, "r.txt", "text/plain", b"x").expect("save");
    link_document_to_entry(conn, meta.id, entry.entry.id).expect("link");

    let linked = &list_documents(conn, entity_id).expect("list")[0];
    assert_eq!(linked.entry_id, Some(entry.entry.id), "linked after link");

    unlink_document(conn, meta.id).expect("unlink");
    let orphan = &list_documents(conn, entity_id).expect("list")[0];
    assert_eq!(orphan.entry_id, None, "unlinked but still listed");
    assert!(get_document(conn, meta.id).is_ok(), "blob preserved");

    // Relink works after an unlink.
    link_document_to_entry(conn, meta.id, entry.entry.id).expect("relink");
    let relinked = &list_documents(conn, entity_id).expect("list")[0];
    assert_eq!(relinked.entry_id, Some(entry.entry.id));
}

#[test]
fn delete_and_unlink_missing_document_return_not_found() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    setup_entity(conn);

    let missing = DocumentId::new();
    assert!(delete_document(conn, missing).is_err());
    assert!(unlink_document(conn, missing).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oikonomia-core --test documents_flow`
Expected: COMPILE ERROR — `list_documents`, `get_document`, `delete_document`, `unlink_document` not found in `oikonomia_core::documents`.

- [ ] **Step 3: Implement in `store.rs`**

3a. Add `created_at` to `DocumentMeta` (after `size_bytes`):

```rust
    /// Byte length.
    pub size_bytes: i64,
    /// RFC 3339 creation time.
    pub created_at: String,
```

3b. In `save_document`, `rusqlite::params![...]` borrows its arguments, so `created` is still usable after the `execute`; extend the returned struct:

```rust
    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id: None,
        filename: name.to_owned(),
        mime_type: mime,
        size_bytes,
        created_at: created,
    })
```

3c. Extend the util import at the top of `store.rs`:

```rust
use crate::util::{now_utc_string, parse_uuid};
```

3d. Add the four functions plus a shared row-mapping helper (place them after `link_document_to_entry`):

```rust
type MetaColumns = (String, String, Option<String>, String, String, i64, String);

fn meta_from_columns(raw: MetaColumns) -> Result<DocumentMeta> {
    let (id_s, entity_s, entry_s, filename, mime_type, size_bytes, created_at) = raw;
    Ok(DocumentMeta {
        id: DocumentId(parse_uuid(&id_s)?),
        entity_id: EntityId(parse_uuid(&entity_s)?),
        entry_id: entry_s
            .as_deref()
            .map(parse_uuid)
            .transpose()?
            .map(JournalEntryId),
        filename,
        mime_type,
        size_bytes,
        created_at,
    })
}

/// All documents for an entity, newest first (metadata only — no blobs).
///
/// # Errors
///
/// DB errors.
pub fn list_documents(conn: &Connection, entity_id: EntityId) -> Result<Vec<DocumentMeta>> {
    let mut stmt = conn
        .prepare(
            "
            SELECT id, entity_id, entry_id, filename, mime_type, size_bytes, created_at
            FROM documents
            WHERE entity_id = ?1
            ORDER BY created_at DESC
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map([entity_id.0.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let raw = row.map_err(|err| Error::Io(err.to_string()))?;
        out.push(meta_from_columns(raw)?);
    }
    Ok(out)
}

/// One document's metadata plus raw bytes (for viewing/export).
///
/// # Errors
///
/// Not found or DB error.
pub fn get_document(conn: &Connection, id: DocumentId) -> Result<(DocumentMeta, Vec<u8>)> {
    let (raw, data) = conn
        .query_row(
            "
            SELECT id, entity_id, entry_id, filename, mime_type, size_bytes, created_at, data
            FROM documents
            WHERE id = ?1
            ",
            [id.0.to_string()],
            |row| {
                Ok((
                    (
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                    ),
                    row.get::<_, Vec<u8>>(7)?,
                ))
            },
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound("document".into()),
            other => Error::Io(other.to_string()),
        })?;

    Ok((meta_from_columns(raw)?, data))
}

/// Permanently remove a document blob. Linked entries are unaffected.
///
/// # Errors
///
/// Not found or DB error.
pub fn delete_document(conn: &Connection, id: DocumentId) -> Result<()> {
    let n = conn
        .execute("DELETE FROM documents WHERE id = ?1", [id.0.to_string()])
        .map_err(|err| Error::Io(err.to_string()))?;
    if n == 0 {
        return Err(Error::NotFound("document".into()));
    }
    Ok(())
}

/// Detach a document from its entry; the file stays in the vault as an orphan.
///
/// # Errors
///
/// Not found or DB error.
pub fn unlink_document(conn: &Connection, id: DocumentId) -> Result<()> {
    let n = conn
        .execute(
            "UPDATE documents SET entry_id = NULL WHERE id = ?1",
            [id.0.to_string()],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if n == 0 {
        return Err(Error::NotFound("document".into()));
    }
    Ok(())
}
```

3e. Update the `store` re-exports in `crates/oikonomia-core/src/documents/mod.rs`:

```rust
pub use store::{
    DocumentId, DocumentMeta, MAX_DOCUMENT_BYTES, delete_document, get_document,
    link_document_to_entry, list_documents, resolve_mime, save_analysis_json, save_document,
    suggest_accounts_for_entity, unlink_document, validate_document_file,
};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p oikonomia-core --test documents_flow`
Expected: PASS (5 tests: 2 existing + 3 new).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
git add crates/oikonomia-core/src/documents/store.rs crates/oikonomia-core/src/documents/mod.rs crates/oikonomia-core/tests/documents_flow.rs
git commit -m "feat: document read-side in core - list, get, delete, unlink

Documents were write-only: stored and linked at analyze time but never
retrievable. These functions power the entry-detail attachments, the
Documents library page, and paperclip badges. Metadata gains created_at
so the library can sort and display when a file entered the vault."
```

---

### Task 2: Core entry filters (`EntryFilter`) end-to-end

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/journals.rs` (`list_entries`, new `EntryFilter`, new `like_pattern`)
- Modify: `crates/oikonomia-core/src/ledger/mod.rs` (export `EntryFilter`)
- Modify: `crates/oikonomia-core/tests/dashboard_correctness.rs:141-143` (two call sites)
- Modify: `apps/desktop/src-tauri/src/commands.rs:258-270` (`entry_list`) and its import list
- Modify: `web/src/lib/api.ts:180-181` (`entryList`)
- Modify: `web/src/pages/DashboardPage.tsx:108` (positional `from`/`to` → options object)
- Create test: `crates/oikonomia-core/tests/entry_filters.rs`

**Interfaces:**
- Consumes: `list_entries`, `parse_date`/`format_date`, personal chart codes `1010` (Checking), `1020` (Savings), `5100` (Food).
- Produces:
  - `pub struct EntryFilter { pub text: Option<String>, pub date_from: Option<String>, pub date_to: Option<String>, pub account_id: Option<AccountId> }` with `#[derive(Debug, Clone, Default, Serialize, Deserialize)]`.
  - `pub fn list_entries(conn: &Connection, entity_id: EntityId, filter: &EntryFilter) -> Result<Vec<PostedEntryView>>` (signature change — the old `from`/`to` params fold into the struct).
  - Tauri command `entry_list(entity_id, from?, to?, search?, account_id?)` — TS callers pass `{ entityId, from, to, search, accountId }`.
  - TS `api.entryList(entityId, opts?: { from?: string; to?: string; search?: string; accountId?: string })`.

- [ ] **Step 1: Write the failing tests**

Create `crates/oikonomia-core/tests/entry_filters.rs`:

```rust
//! `list_entries` filter behavior: text, date range, account.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, CreateJournalLine, EntryFilter, PostJournal, PostedEntryView, create_entity,
    list_accounts, list_entries, post_entry,
};
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;

fn setup_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn setup_entity(conn: &Connection) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: "Filters".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity")
    .id
}

/// Post a balanced two-line entry using personal-template account codes.
fn post_two_line(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    description: &str,
    reference: Option<&str>,
    memo: Option<&str>,
    debit_code: &str,
    credit_code: &str,
) -> PostedEntryView {
    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let acc = |code: &str| {
        accounts
            .iter()
            .find(|a| a.code == code)
            .expect("template account")
            .id
    };

    post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: date.into(),
            description: description.into(),
            reference: reference.map(Into::into),
            lines: vec![
                CreateJournalLine {
                    account_id: acc(debit_code),
                    debit_minor: 1_000,
                    credit_minor: 0,
                    memo: memo.map(Into::into),
                },
                CreateJournalLine {
                    account_id: acc(credit_code),
                    debit_minor: 0,
                    credit_minor: 1_000,
                    memo: None,
                },
            ],
        },
    )
    .expect("post")
}

#[test]
fn text_filter_matches_description_reference_and_memo() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    post_two_line(
        conn, entity_id, "2026-02-01", "Groceries March", None, None, "5100", "1010",
    );
    post_two_line(
        conn,
        entity_id,
        "2026-02-02",
        "Utility bill",
        Some("INV-42"),
        None,
        "5100",
        "1010",
    );
    post_two_line(
        conn,
        entity_id,
        "2026-02-03",
        "Shopping",
        None,
        Some("office chair"),
        "5100",
        "1010",
    );

    let by = |text: &str| {
        list_entries(
            conn,
            entity_id,
            &EntryFilter {
                text: Some(text.into()),
                ..EntryFilter::default()
            },
        )
        .expect("list")
    };

    assert_eq!(by("groceries").len(), 1, "description, case-insensitive");
    assert_eq!(by("inv-42").len(), 1, "reference matches");
    assert_eq!(by("chair").len(), 1, "line memo matches");
    assert_eq!(by("no-such-text").len(), 0);
    assert_eq!(by("%").len(), 0, "LIKE wildcards are escaped literals");
    assert_eq!(by("   ").len(), 3, "blank text means no filter");
}

#[test]
fn date_range_is_inclusive_on_both_ends() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    for date in ["2026-01-10", "2026-01-20", "2026-01-31"] {
        post_two_line(conn, entity_id, date, "Entry", None, None, "5100", "1010");
    }

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            date_from: Some("2026-01-10".into()),
            date_to: Some("2026-01-20".into()),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 2, "bounds are inclusive");
}

#[test]
fn account_filter_matches_entries_touching_the_account() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    post_two_line(
        conn, entity_id, "2026-03-01", "Food shop", None, None, "5100", "1010",
    );
    post_two_line(
        conn,
        entity_id,
        "2026-03-02",
        "Move to savings",
        None,
        None,
        "1020",
        "1010",
    );

    let accounts = list_accounts(conn, entity_id).expect("accounts");
    let food = accounts.iter().find(|a| a.code == "5100").expect("5100").id;

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            account_id: Some(food),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 1);
    assert_eq!(got[0].entry.description, "Food shop");
}

#[test]
fn combined_filters_intersect() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);

    post_two_line(
        conn, entity_id, "2026-04-01", "Groceries", None, None, "5100", "1010",
    );
    post_two_line(
        conn, entity_id, "2026-05-01", "Groceries", None, None, "5100", "1010",
    );

    let got = list_entries(
        conn,
        entity_id,
        &EntryFilter {
            text: Some("groceries".into()),
            date_from: Some("2026-04-15".into()),
            ..EntryFilter::default()
        },
    )
    .expect("list");

    assert_eq!(got.len(), 1, "text AND date must both hold");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oikonomia-core --test entry_filters`
Expected: COMPILE ERROR — `EntryFilter` not found; `list_entries` takes 4 arguments.

- [ ] **Step 3: Implement the core filter**

In `crates/oikonomia-core/src/ledger/journals.rs`, add above `list_entries`:

```rust
/// Optional predicates for [`list_entries`]; every `None` means "no filter".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntryFilter {
    /// Case-insensitive substring over description, reference, and line memos.
    pub text: Option<String>,
    /// Inclusive ISO lower bound (`YYYY-MM-DD`).
    pub date_from: Option<String>,
    /// Inclusive ISO upper bound.
    pub date_to: Option<String>,
    /// Only entries with at least one line on this account.
    pub account_id: Option<AccountId>,
}

/// Wrap trimmed user text in `%…%`, escaping LIKE wildcards so `%`/`_`
/// in a search are literals, not patterns.
fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}
```

Replace `list_entries` (keep the doc comment style; note the account predicate is an `EXISTS` subquery, never a join-clause filter):

```rust
/// List posted entries for an entity (newest first) matching `filter`.
///
/// # Errors
///
/// DB / validation errors.
pub fn list_entries(
    conn: &Connection,
    entity_id: EntityId,
    filter: &EntryFilter,
) -> Result<Vec<PostedEntryView>> {
    // Normalize before binding: SQL compares date TEXT lexicographically, so a
    // lenient input like `2026-3-5` must become `2026-03-05` first.
    let from = filter
        .date_from
        .as_deref()
        .map(parse_date)
        .transpose()?
        .map(format_date);
    let to = filter
        .date_to
        .as_deref()
        .map(parse_date)
        .transpose()?
        .map(format_date);

    let pattern = filter
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(like_pattern);
    let account = filter.account_id.map(|id| id.0.to_string());

    let mut stmt = conn
        .prepare(
            "
            SELECT je.id, je.entity_id, je.entry_date, je.description, je.reference,
                   je.status, je.voided_by_entry_id
            FROM journal_entries je
            WHERE je.entity_id = ?1
              AND je.status = 'posted'
              AND (?2 IS NULL OR je.entry_date >= ?2)
              AND (?3 IS NULL OR je.entry_date <= ?3)
              AND (?4 IS NULL
                   OR je.description LIKE ?4 ESCAPE '\\'
                   OR je.reference LIKE ?4 ESCAPE '\\'
                   OR EXISTS (
                       SELECT 1 FROM journal_lines jl
                       WHERE jl.entry_id = je.id AND jl.memo LIKE ?4 ESCAPE '\\'
                   ))
              AND (?5 IS NULL OR EXISTS (
                   SELECT 1 FROM journal_lines jl
                   WHERE jl.entry_id = je.id AND jl.account_id = ?5
              ))
            ORDER BY je.entry_date DESC, je.created_at DESC
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map(
            rusqlite::params![entity_id.0.to_string(), from, to, pattern, account],
            |row| {
                let voided: Option<String> = row.get(6)?;
                Ok((map_entry_row(row)?, voided.is_some()))
            },
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let (entry, mut is_voided) = row.map_err(|err| Error::Io(err.to_string()))?;
        // Also treat void-reversals as voided (including older data that only
        // linked the original → reverse, not reverse → original).
        if !is_voided {
            is_voided = entry_is_void_reverse(conn, entry.id)?;
        }
        let lines = load_lines(conn, entry.id)?;
        out.push(PostedEntryView {
            entry,
            lines,
            is_voided,
        });
    }
    Ok(out)
}
```

Export in `crates/oikonomia-core/src/ledger/mod.rs` — add `EntryFilter` to the `journals` re-export list:

```rust
pub use journals::{
    CreateJournalLine, EntryFilter, PostJournal, PostSimpleEntry, PostedEntryView, RegisterLine,
    SimpleBillStatus, SimpleEntryKind, VoidResult, account_register, get_entry, list_entries,
    post_entry, post_simple_entry, void_entry,
};
```

- [ ] **Step 4: Update the two existing core test call sites**

In `crates/oikonomia-core/tests/dashboard_correctness.rs`, add `EntryFilter` to the `oikonomia_core::ledger` import list, then:

```rust
    let listed = list_entries(
        conn,
        e,
        &EntryFilter {
            date_from: Some("2026-08-01".into()),
            date_to: Some("2026-08-31".into()),
            ..EntryFilter::default()
        },
    )
    .expect("list");
```

and:

```rust
    let all = list_entries(conn, e, &EntryFilter::default()).expect("all");
```

- [ ] **Step 5: Run core tests to verify they pass**

Run: `cargo test -p oikonomia-core`
Expected: PASS including all 4 new `entry_filters` tests.

- [ ] **Step 6: Update the Tauri command**

In `apps/desktop/src-tauri/src/commands.rs`, add `EntryFilter` to the `oikonomia_core::ledger` import list, then replace `entry_list`:

```rust
/// List journal entries matching optional search/date/account filters.
#[tauri::command]
pub async fn entry_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: Option<String>,
    to: Option<String>,
    search: Option<String>,
    account_id: Option<AccountId>,
) -> CommandResult<Vec<PostedEntryView>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        let filter = EntryFilter {
            text: search,
            date_from: from,
            date_to: to,
            account_id,
        };
        list_entries(conn, entity_id, &filter)
    })
    .await
}
```

- [ ] **Step 7: Update the TS API and its callers**

In `web/src/lib/api.ts`, replace the `entryList` member:

```ts
  entryList: (
    entityId: string,
    opts?: { from?: string; to?: string; search?: string; accountId?: string },
  ) =>
    call<PostedEntryView[]>('entry_list', {
      entityId,
      from: opts?.from ?? null,
      to: opts?.to ?? null,
      search: opts?.search ?? null,
      accountId: opts?.accountId ?? null,
    }),
```

In `web/src/pages/DashboardPage.tsx:108`, the call passes `from`/`to` positionally — change it to:

```ts
          api.entryList(entity.id, { from, to }),
```

(`web/src/pages/TransactionsPage.tsx:144` calls `api.entryList(entity.id)` with one argument — still valid, untouched here; Task 6 rewrites it.)

- [ ] **Step 8: Full gate and commit**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run build && cd ..
git add crates/oikonomia-core/src/ledger/journals.rs crates/oikonomia-core/src/ledger/mod.rs crates/oikonomia-core/tests/dashboard_correctness.rs crates/oikonomia-core/tests/entry_filters.rs apps/desktop/src-tauri/src/commands.rs web/src/lib/api.ts web/src/pages/DashboardPage.tsx
git commit -m "feat: SQL-side entry filters - text, date range, account

Filtering happens in list_entries so the UI never re-implements ledger
logic. Text search covers description, reference, and line memos with
LIKE wildcards escaped; the account predicate is an EXISTS subquery per
the report-query invariant. The old from/to params fold into EntryFilter."
```

---

### Task 3: Document IPC commands + TS document API

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs` (imports + 5 commands + `DocumentContent`)
- Modify: `apps/desktop/src-tauri/src/lib.rs:82-85` (register commands)
- Modify: `web/src/lib/api.ts` (types + methods)

**Interfaces:**
- Consumes (Task 1): `list_documents`, `get_document`, `delete_document`, `unlink_document`, `DocumentMeta` (with `created_at`), `DocumentId`; existing `save_document`, `link_document_to_entry`, `get_entry`, `MAX_DOCUMENT_BYTES`.
- Produces (used by Tasks 5-7):
  - Commands: `document_list(entity_id)`, `document_get(document_id) -> DocumentContent { meta, data_base64 }`, `document_delete(document_id)`, `document_unlink(document_id)`, `document_attach(entity_id, entry_id, filename, mime_type, data_base64) -> DocumentMeta`.
  - TS: `DocumentMeta` type (snake_case fields incl. `created_at: string`), `DocumentContent`, `api.documentList/documentGet/documentDelete/documentUnlink/documentAttach`.

- [ ] **Step 1: Extend imports in `commands.rs`**

Update the `oikonomia_core::documents` import to:

```rust
use oikonomia_core::documents::{
    AnalyzerStatus, DocumentId, DocumentMeta, DocumentSuggestion, analyze_document_bytes,
    analyzer_status, delete_document, get_document, link_document_to_entry, list_documents,
    save_analysis_json, save_document, suggest_accounts_for_entity, unlink_document,
};
```

- [ ] **Step 2: Add the payload struct and five commands**

Append after `document_link_entry` in `commands.rs`:

```rust
/// Metadata plus base64 payload for the in-app viewer.
#[derive(Debug, Serialize)]
pub struct DocumentContent {
    /// Metadata.
    pub meta: DocumentMeta,
    /// Raw bytes, base64-encoded for IPC (bounded by the 8 MiB cap).
    pub data_base64: String,
}

/// All stored documents for an entity (metadata only).
#[tauri::command]
pub async fn document_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<DocumentMeta>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_documents(conn, entity_id)
    })
    .await
}

/// One document's bytes for the in-app viewer. Decrypted content crosses
/// IPC only; nothing is written to disk.
#[tauri::command]
pub async fn document_get(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<DocumentContent> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        let (meta, data) = get_document(conn, document_id)?;
        Ok(DocumentContent {
            meta,
            data_base64: base64::engine::general_purpose::STANDARD.encode(data),
        })
    })
    .await
}

/// Permanently delete a stored document.
#[tauri::command]
pub async fn document_delete(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        delete_document(conn, document_id)
    })
    .await
}

/// Detach a document from its entry (file stays in the vault).
#[tauri::command]
pub async fn document_unlink(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        unlink_document(conn, document_id)
    })
    .await
}

/// Attach a file to an existing posted entry. No OCR pass — analysis only
/// runs on the drop-zone flow; `analysis_json` stays NULL here.
#[tauri::command]
pub async fn document_attach(
    state: State<'_, AppState>,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    filename: String,
    mime_type: String,
    data_base64: String,
) -> CommandResult<DocumentMeta> {
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

        // 404 on a bad entry before storing anything.
        get_entry(conn, entry_id)?;

        // Save + link atomically so a failure can't leave a half-attached file.
        let tx = conn
            .unchecked_transaction()
            .map_err(|err| CoreError::Io(err.to_string()))?;
        let meta = save_document(&tx, entity_id, &filename, &mime_type, &data)?;
        link_document_to_entry(&tx, meta.id, entry_id)?;
        tx.commit().map_err(|err| CoreError::Io(err.to_string()))?;

        Ok(DocumentMeta {
            entry_id: Some(entry_id),
            ..meta
        })
    })
    .await
}
```

- [ ] **Step 3: Register the commands**

In `apps/desktop/src-tauri/src/lib.rs`, extend the `generate_handler!` list after `commands::document_link_entry`:

```rust
            commands::document_link_entry,
            commands::document_list,
            commands::document_get,
            commands::document_delete,
            commands::document_unlink,
            commands::document_attach,
```

- [ ] **Step 4: Verify the Rust side compiles clean**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean. (The command layer is thin; behavior is covered by Task 1's core tests.)

- [ ] **Step 5: Add the TS types and API methods**

In `web/src/lib/api.ts`, add types near `DocumentSuggestion`:

```ts
export type DocumentMeta = {
  id: string
  entity_id: string
  entry_id: string | null
  filename: string
  mime_type: string
  size_bytes: number
  created_at: string
}

export type DocumentContent = {
  meta: DocumentMeta
  data_base64: string
}
```

Add methods to the `api` object after `documentLinkEntry`:

```ts
  documentList: (entityId: string) => call<DocumentMeta[]>('document_list', { entityId }),
  documentGet: (documentId: string) => call<DocumentContent>('document_get', { documentId }),
  documentDelete: (documentId: string) => call<void>('document_delete', { documentId }),
  documentUnlink: (documentId: string) => call<void>('document_unlink', { documentId }),
  documentAttach: (input: {
    entityId: string
    entryId: string
    filename: string
    mimeType: string
    dataBase64: string
  }) => call<DocumentMeta>('document_attach', input),
```

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run build && cd ..
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs web/src/lib/api.ts
git commit -m "feat: document IPC surface - list, get, delete, unlink, attach

Thin wrappers over the core read-side; attach saves and links in one
transaction with the same base64 size gate as analyze, and skips OCR
deliberately (analysis belongs to the drop-zone flow only)."
```

---

### Task 4: Export via native save dialog (`tauri-plugin-dialog`)

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml` (add dependency)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register plugin)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (`document_export`)
- Modify: `web/src/lib/api.ts` (`documentExport`)

**Interfaces:**
- Consumes: `get_document` (Task 1), `with_vault_blocking`/`await_blocking`.
- Produces: command `document_export(document_id) -> Option<String>` (chosen path, or `None` on dialog cancel); TS `api.documentExport(documentId) -> Promise<string | null>`.

- [ ] **Step 1: Add the dependency**

In `apps/desktop/src-tauri/Cargo.toml` under `[dependencies]`, after `tauri-plugin-log`:

```toml
tauri-plugin-dialog = "2"
```

- [ ] **Step 2: Register the plugin**

In `apps/desktop/src-tauri/src/lib.rs`, the builder gains the plugin before `.setup(...)`:

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
```

No capability change: the dialog is invoked from Rust only, and capabilities gate webview-side plugin calls. The no-network capability stays exactly as is.

- [ ] **Step 3: Add the export command**

Append to `apps/desktop/src-tauri/src/commands.rs`:

```rust
/// Export a document to a user-chosen path. This is the only path by which
/// decrypted bytes reach disk, and it always goes through an explicit
/// native save dialog.
#[tauri::command]
pub async fn document_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (meta, data) = with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_document(conn, document_id)
    })
    .await?;

    // The blocking dialog must stay off the async runtime workers.
    let picked = await_blocking(tauri::async_runtime::spawn_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name(meta.filename.as_str())
            .blocking_save_file())
    }))
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid save location: {e}"),
    })?;

    std::fs::write(&path, &data).map_err(|e| CommandError {
        code: "io".into(),
        message: format!("could not save file: {e}"),
    })?;

    Ok(Some(path.display().to_string()))
}
```

Register it in `lib.rs` after `commands::document_attach`:

```rust
            commands::document_export,
```

API note: `blocking_save_file()` returns `Option<tauri_plugin_dialog::FilePath>`; `FilePath::into_path()` converts to `PathBuf` (it errors only on non-file URLs, which the save dialog never returns). If the compiler disagrees with `into_path()` on the pinned plugin version, match the `FilePath::Path(p)` variant instead — do not switch APIs beyond that.

- [ ] **Step 4: Add the TS method**

In `web/src/lib/api.ts` after `documentAttach`:

```ts
  documentExport: (documentId: string) => call<string | null>('document_export', { documentId }),
```

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run build && cd ..
git add apps/desktop/src-tauri/Cargo.toml Cargo.lock apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/commands.rs web/src/lib/api.ts
git commit -m "feat: explicit document export through the native save dialog

Save-a-copy is the single deliberate path for plaintext to leave the
vault; viewing stays in memory. Dialog and file write both run in Rust
via tauri-plugin-dialog (official, no network permission required)."
```

---

### Task 5: Shared file helpers + `DocumentViewerModal`

**Files:**
- Create: `web/src/lib/files.ts`
- Modify: `web/src/components/DocumentDropZone.tsx` (import helpers instead of local copies)
- Create: `web/src/components/DocumentViewerModal.tsx`

**Interfaces:**
- Consumes: `api.documentGet`, `api.documentExport` (Tasks 3-4), `Modal`, `Button`.
- Produces (used by Tasks 6-7):
  - `web/src/lib/files.ts`: `fileToBase64(file: File): Promise<string>`, `mimeFromName(name: string, fallback?: string): string`, `formatBytes(n: number): string`.
  - `DocumentViewerModal({ documentId, onClose, onError })` — renders when `documentId` is non-null; fetches bytes, shows image/PDF/text from a blob URL revoked on close; footer has "Save a copy".

- [ ] **Step 1: Extract the file helpers**

Create `web/src/lib/files.ts` — move `fileToBase64` and `mimeFromName` verbatim from `DocumentDropZone.tsx:15-40` and add `formatBytes`:

```ts
/** File → base64 payload and MIME helpers shared by drop-zone, attach, and viewer. */

export function mimeFromName(name: string, fallback = ''): string {
  const lower = name.toLowerCase()
  if (lower.endsWith('.pdf')) return 'application/pdf'
  if (lower.endsWith('.png')) return 'image/png'
  if (lower.endsWith('.jpg') || lower.endsWith('.jpeg')) return 'image/jpeg'
  if (lower.endsWith('.webp')) return 'image/webp'
  if (lower.endsWith('.txt')) return 'text/plain'
  return fallback || 'application/octet-stream'
}

export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const result = reader.result
      if (typeof result !== 'string') {
        reject(new Error('Could not read file'))
        return
      }
      const comma = result.indexOf(',')
      resolve(comma >= 0 ? result.slice(comma + 1) : result)
    }
    reader.onerror = () => reject(new Error('Could not read file'))
    reader.readAsDataURL(file)
  })
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}
```

In `DocumentDropZone.tsx`, delete the local `mimeFromName` and `fileToBase64` definitions and add:

```ts
import { fileToBase64, mimeFromName } from '../lib/files'
```

- [ ] **Step 2: Create the viewer modal**

Create `web/src/components/DocumentViewerModal.tsx`:

```tsx
import { useEffect, useMemo, useState } from 'react'
import { Download, Loader2 } from 'lucide-react'
import { api, type DocumentMeta } from '../lib/api'
import type { CommandError } from '../lib/tauri'
import { formatBytes } from '../lib/files'
import { Modal } from './Modal'
import { Button } from './ui'

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i += 1) bytes[i] = bin.charCodeAt(i)
  return bytes
}

type Props = {
  /** Non-null id opens the viewer for that document. */
  documentId: string | null
  onClose: () => void
  onError: (message: string) => void
}

/**
 * In-memory document viewer: decrypted bytes live only in a blob URL that is
 * revoked when the viewer closes. "Save a copy" is the explicit export path.
 */
export function DocumentViewerModal({ documentId, onClose, onError }: Props) {
  const [meta, setMeta] = useState<DocumentMeta | null>(null)
  const [bytes, setBytes] = useState<Uint8Array | null>(null)
  const [exporting, setExporting] = useState(false)

  useEffect(() => {
    if (!documentId) {
      setMeta(null)
      setBytes(null)
      return
    }
    let cancelled = false
    void api
      .documentGet(documentId)
      .then((doc) => {
        if (cancelled) return
        setMeta(doc.meta)
        setBytes(base64ToBytes(doc.data_base64))
      })
      .catch((err) => {
        if (cancelled) return
        onError((err as CommandError).message || 'Could not open document')
        onClose()
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [documentId])

  const blobUrl = useMemo(() => {
    if (!bytes || !meta) return null
    return URL.createObjectURL(new Blob([bytes as BlobPart], { type: meta.mime_type }))
  }, [bytes, meta])

  useEffect(() => {
    return () => {
      if (blobUrl) URL.revokeObjectURL(blobUrl)
    }
  }, [blobUrl])

  async function onExport() {
    if (!documentId) return
    setExporting(true)
    try {
      await api.documentExport(documentId)
    } catch (err) {
      onError((err as CommandError).message || 'Could not save a copy')
    } finally {
      setExporting(false)
    }
  }

  if (!documentId) return null

  const isImage = meta?.mime_type.startsWith('image/') ?? false
  const isPdf = meta?.mime_type === 'application/pdf'
  const isText = meta?.mime_type === 'text/plain'

  return (
    <Modal
      open
      title={meta?.filename ?? 'Document'}
      description={meta ? `${meta.mime_type} · ${formatBytes(meta.size_bytes)}` : undefined}
      maxWidth="max-w-4xl"
      onClose={onClose}
    >
      {!meta || !bytes ? (
        <div className="flex h-40 items-center justify-center text-[var(--color-muted)]">
          <Loader2 className="size-5 animate-spin" />
        </div>
      ) : (
        <div className="space-y-4">
          {isImage && blobUrl ? (
            <img
              src={blobUrl}
              alt={meta.filename}
              className="mx-auto max-h-[65vh] w-auto max-w-full rounded-xl border border-[var(--color-border)]"
            />
          ) : null}

          {isPdf && blobUrl ? (
            <iframe
              src={blobUrl}
              title={meta.filename}
              className="h-[65vh] w-full rounded-xl border border-[var(--color-border)] bg-white"
            />
          ) : null}

          {isText ? (
            <pre className="max-h-[65vh] overflow-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4 text-xs whitespace-pre-wrap text-[var(--color-fg-secondary)]">
              {new TextDecoder().decode(bytes)}
            </pre>
          ) : null}

          {!isImage && !isPdf && !isText ? (
            <p className="py-8 text-center text-sm text-[var(--color-muted)]">
              No in-app preview for {meta.mime_type} — use Save a copy to open it elsewhere.
            </p>
          ) : null}

          <div className="flex justify-end border-t border-[var(--color-border)] pt-4">
            <Button variant="secondary" busy={exporting} onClick={() => void onExport()}>
              <Download className="size-4" />
              {exporting ? 'Saving…' : 'Save a copy'}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  )
}
```

- [ ] **Step 3: Verify the frontend builds**

Run: `cd web && npm run build && npm run lint && cd ..`
Expected: clean `tsc -b`, vite build, oxlint.

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/files.ts web/src/components/DocumentDropZone.tsx web/src/components/DocumentViewerModal.tsx
git commit -m "feat: in-memory document viewer modal; shared file helpers

Viewer renders images, PDFs (WKWebView native), and text from a blob URL
revoked on close, so decrypted bytes never persist. fileToBase64 and
mimeFromName move to lib/files for reuse by the attach flow."
```

---

### Task 6: Entry detail modal, paperclip badges, and filter toolbar on Transactions

**Files:**
- Create: `web/src/components/EntryDetailModal.tsx`
- Modify: `web/src/pages/TransactionsPage.tsx`

**Interfaces:**
- Consumes: `api.entryList` opts (Task 2), `api.documentList/documentAttach/documentUnlink/documentExport` (Tasks 3-4), `DocumentViewerModal`, `fileToBase64`/`mimeFromName` (Task 5), existing `Modal`, `ConfirmDialog`, `ui.tsx` primitives.
- Produces: `EntryDetailModal({ view, accounts, documents, currency, onClose, onView, onChanged, onError })` where `accounts: Map<string, Account>`, `documents: DocumentMeta[]` (this entry's docs), `onView(documentId)` opens the viewer, `onChanged()` reloads page data.

- [ ] **Step 1: Create `EntryDetailModal`**

Create `web/src/components/EntryDetailModal.tsx`:

```tsx
import { useRef, useState } from 'react'
import { Download, Eye, Paperclip, Plus, X } from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  type Account,
  type DocumentMeta,
  type PostedEntryView,
} from '../lib/api'
import type { CommandError } from '../lib/tauri'
import { fileToBase64, formatBytes, mimeFromName } from '../lib/files'
import { ConfirmDialog } from './ConfirmDialog'
import { Modal } from './Modal'
import { Button } from './ui'

type Props = {
  /** Non-null view opens the modal. */
  view: PostedEntryView | null
  accounts: Map<string, Account>
  /** Documents linked to this entry. */
  documents: DocumentMeta[]
  currency: string
  onClose: () => void
  onView: (documentId: string) => void
  onChanged: () => Promise<void>
  onError: (message: string) => void
}

/** Full journal view: lines with account names plus the attachments list. */
export function EntryDetailModal({
  view,
  accounts,
  documents,
  currency,
  onClose,
  onView,
  onChanged,
  onError,
}: Props) {
  const [busyId, setBusyId] = useState<string | null>(null)
  const [attachBusy, setAttachBusy] = useState(false)
  const [unlinkId, setUnlinkId] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  if (!view) return null
  const entry = view.entry

  async function onAttach(file: File) {
    if (!view) return
    // Resource guard only — the backend enforces the same cap.
    if (file.size > 8 * 1024 * 1024) {
      onError('File too large (max 8 MB)')
      return
    }
    setAttachBusy(true)
    try {
      const dataBase64 = await fileToBase64(file)
      await api.documentAttach({
        entityId: view.entry.entity_id,
        entryId: view.entry.id,
        filename: file.name,
        mimeType: file.type || mimeFromName(file.name),
        dataBase64,
      })
      await onChanged()
    } catch (err) {
      onError((err as CommandError).message || 'Could not attach file')
    } finally {
      setAttachBusy(false)
    }
  }

  async function confirmUnlink() {
    if (!unlinkId) return
    setBusyId(unlinkId)
    try {
      await api.documentUnlink(unlinkId)
      setUnlinkId(null)
      await onChanged()
    } catch (err) {
      onError((err as CommandError).message || 'Could not remove attachment')
    } finally {
      setBusyId(null)
    }
  }

  async function onExport(id: string) {
    setBusyId(id)
    try {
      await api.documentExport(id)
    } catch (err) {
      onError((err as CommandError).message || 'Could not save a copy')
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Modal
      open
      title={entry.description}
      description={`${formatDate(entry.entry_date)}${entry.reference ? ` · Ref ${entry.reference}` : ''}`}
      onClose={onClose}
    >
      <ConfirmDialog
        open={unlinkId !== null}
        title="Remove attachment?"
        body="The file stays in your vault under Documents as unlinked — nothing is deleted."
        confirmLabel="Remove"
        busy={busyId !== null && busyId === unlinkId}
        onCancel={() => setUnlinkId(null)}
        onConfirm={() => void confirmUnlink()}
      />

      <div className="space-y-6">
        <div className="overflow-hidden rounded-xl border border-[var(--color-border)]">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-[var(--color-surface-2)] text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
                <th className="px-4 py-2.5 text-left">Account</th>
                <th className="px-4 py-2.5 text-right">Debit</th>
                <th className="px-4 py-2.5 text-right">Credit</th>
                <th className="px-4 py-2.5 text-left">Memo</th>
              </tr>
            </thead>
            <tbody>
              {view.lines.map((line) => {
                const acc = accounts.get(line.account_id)
                return (
                  <tr key={line.id} className="border-t border-[var(--color-border)]">
                    <td className="px-4 py-2.5 text-[var(--color-fg)]">
                      {acc ? `${acc.code} · ${acc.name}` : 'Unknown account'}
                    </td>
                    <td className="px-4 py-2.5 text-right tabular-nums">
                      {line.debit.amount_minor > 0
                        ? formatMoney(line.debit.amount_minor, currency)
                        : ''}
                    </td>
                    <td className="px-4 py-2.5 text-right tabular-nums">
                      {line.credit.amount_minor > 0
                        ? formatMoney(line.credit.amount_minor, currency)
                        : ''}
                    </td>
                    <td className="px-4 py-2.5 text-[var(--color-muted)]">{line.memo ?? ''}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>

        <div>
          <div className="mb-2 flex items-center justify-between">
            <span className="text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
              Attachments
            </span>
            <Button
              variant="secondary"
              size="sm"
              busy={attachBusy}
              onClick={() => fileRef.current?.click()}
            >
              <Plus className="size-3.5" />
              Attach file
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept="image/png,image/jpeg,image/webp,application/pdf,text/plain,.pdf,.png,.jpg,.jpeg,.webp,.txt"
              className="hidden"
              onChange={(e) => {
                const file = e.target.files?.[0]
                if (file) void onAttach(file)
                e.target.value = ''
              }}
            />
          </div>

          {documents.length === 0 ? (
            <p className="rounded-xl border border-dashed border-[var(--color-border-strong)] px-4 py-6 text-center text-xs text-[var(--color-muted)]">
              No files attached to this entry.
            </p>
          ) : (
            <ul className="divide-y divide-[var(--color-border)] rounded-xl border border-[var(--color-border)]">
              {documents.map((doc) => (
                <li key={doc.id} className="flex items-center gap-3 px-4 py-2.5">
                  <Paperclip className="size-4 shrink-0 text-[var(--color-muted)]" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm text-[var(--color-fg)]">{doc.filename}</div>
                    <div className="text-xs text-[var(--color-muted)]">
                      {formatBytes(doc.size_bytes)}
                    </div>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    onClick={() => onView(doc.id)}
                    aria-label="View document"
                    title="View"
                  >
                    <Eye className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    busy={busyId === doc.id}
                    onClick={() => void onExport(doc.id)}
                    aria-label="Save a copy"
                    title="Save a copy"
                  >
                    <Download className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    onClick={() => setUnlinkId(doc.id)}
                    aria-label="Remove attachment"
                    title="Remove"
                  >
                    <X className="size-4" />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </Modal>
  )
}
```

- [ ] **Step 2: Wire `TransactionsPage`**

All edits in `web/src/pages/TransactionsPage.tsx`:

2a. Extend imports:

```tsx
import { ArrowDownLeft, ArrowLeftRight, ArrowUpRight, FileText, Paperclip, Plus, Trash2 } from 'lucide-react'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { EntryDetailModal } from '../components/EntryDetailModal'
```

and add `type DocumentMeta` to the `../lib/api` import list.

2b. New state, after the existing `scanNotes` state:

```tsx
  const [docs, setDocs] = useState<DocumentMeta[]>([])
  const [detailId, setDetailId] = useState<string | null>(null)
  const [viewerDocId, setViewerDocId] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [fromDate, setFromDate] = useState('')
  const [toDate, setToDate] = useState('')
  const [accountFilter, setAccountFilter] = useState('')
```

2c. Debounce + derived maps, after the `visibleEntries` memo:

```tsx
  // Debounce typing so each keystroke doesn't hit SQLite.
  useEffect(() => {
    const t = setTimeout(() => setDebouncedSearch(search), 300)
    return () => clearTimeout(t)
  }, [search])

  const docsByEntry = useMemo(() => {
    const map = new Map<string, DocumentMeta[]>()
    for (const d of docs) {
      if (!d.entry_id) continue
      const list = map.get(d.entry_id) ?? []
      list.push(d)
      map.set(d.entry_id, list)
    }
    return map
  }, [docs])

  const detailView = useMemo(
    () => visibleEntries.find((e) => e.entry.id === detailId) ?? null,
    [visibleEntries, detailId],
  )

  const filtersActive = Boolean(
    debouncedSearch.trim() || fromDate || toDate || accountFilter,
  )
```

2d. Replace `reload` so it passes filters and loads documents:

```tsx
  async function reload() {
    if (!entity) return
    const [e, a, d] = await Promise.all([
      api.entryList(entity.id, {
        search: debouncedSearch.trim() || undefined,
        from: fromDate || undefined,
        to: toDate || undefined,
        accountId: accountFilter || undefined,
      }),
      api.accountList(entity.id),
      api.documentList(entity.id),
    ])
    setEntries(e)
    setAccounts(a)
    setDocs(d)
    if (!categoryId && !walletId) {
      applyKindDefaults(kind, a)
    }
  }
```

2e. Replace the load effect (same eslint-disable pattern as today) so filter changes reload:

```tsx
  useEffect(() => {
    if (!entity) {
      setEntries([])
      setAccounts([])
      setDocs([])
      return
    }
    void reload().catch((err) => setError((err as CommandError).message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id, debouncedSearch, fromDate, toDate, accountFilter])
```

2f. Filter toolbar — insert between `<DocumentDropZone …/>` and the `<Modal …>` block:

```tsx
      <div className="flex flex-wrap items-end gap-3">
        <Field label="Search" className="min-w-[220px] flex-1">
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Description, reference, memo…"
          />
        </Field>
        <Field label="From" className="w-40">
          <Input type="date" value={fromDate} onChange={(e) => setFromDate(e.target.value)} />
        </Field>
        <Field label="To" className="w-40">
          <Input type="date" value={toDate} onChange={(e) => setToDate(e.target.value)} />
        </Field>
        <Field label="Account" className="w-56">
          <Select value={accountFilter} onChange={(e) => setAccountFilter(e.target.value)}>
            <option value="">All accounts</option>
            {accounts
              .filter((a) => a.is_active)
              .map((a) => (
                <option key={a.id} value={a.id}>
                  {a.code} · {a.name}
                </option>
              ))}
          </Select>
        </Field>
      </div>
```

2g. Mount the two modals after the `<Modal …>` new-entry block:

```tsx
      <EntryDetailModal
        view={detailView}
        accounts={accountMap}
        documents={detailId ? (docsByEntry.get(detailId) ?? []) : []}
        currency={ccy}
        onClose={() => setDetailId(null)}
        onView={(id) => setViewerDocId(id)}
        onChanged={reload}
        onError={(msg) => setError(msg)}
      />

      <DocumentViewerModal
        documentId={viewerDocId}
        onClose={() => setViewerDocId(null)}
        onError={(msg) => setError(msg)}
      />
```

2h. Make rows open the detail modal and show the paperclip. In the entries `<li>`:
- Add to the `<li>` props: `onClick={() => setDetailId(view.entry.id)}` and extend its className with `cursor-pointer`.
- In the delete button's `onClick`, stop the row click: `onClick={(e) => { e.stopPropagation(); setVoidId(view.entry.id) }}`.
- Insert the paperclip immediately before the amount `<div>`:

```tsx
                  {(docsByEntry.get(view.entry.id)?.length ?? 0) > 0 ? (
                    <Paperclip
                      className="size-3.5 shrink-0 text-[var(--color-muted)]"
                      aria-label="Has attached document"
                    />
                  ) : null}
```

2i. Distinguish "no entries at all" from "no matches": replace the `visibleEntries.length === 0` empty state condition so an active filter shows a lighter message:

```tsx
      {visibleEntries.length === 0 && filtersActive ? (
        <EmptyState
          icon={<FileText className="size-5" />}
          title="No matching entries"
          body="No entries match the current search or filters."
        />
      ) : visibleEntries.length === 0 ? (
        /* existing onboarding EmptyState block, unchanged */
```

- [ ] **Step 3: Verify the frontend builds**

Run: `cd web && npm run build && npm run lint && cd ..`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/EntryDetailModal.tsx web/src/pages/TransactionsPage.tsx
git commit -m "feat: entry detail modal with attachments; filter toolbar and paperclips

Clicking an entry now shows its full journal lines and attached files
with view, save-a-copy, remove (unlink), and attach-file actions. The
toolbar drives list_entries filters in Rust; badges come from one grouped
document_list call, so rows add no per-entry queries."
```

---

### Task 7: Documents library page + sidebar navigation

**Files:**
- Create: `web/src/pages/DocumentsPage.tsx`
- Modify: `web/src/App.tsx` (NAV entry + render)

**Interfaces:**
- Consumes: `api.documentList/documentGet (via viewer)/documentDelete/documentExport`, existing `api.documentLinkEntry`, `api.entryList`; `DocumentViewerModal`, `formatBytes`, `Modal`, `ConfirmDialog`, `ui.tsx`.
- Produces: `DocumentsPage({ entity })`, NAV id `'documents'`.

- [ ] **Step 1: Create the page**

Create `web/src/pages/DocumentsPage.tsx`:

```tsx
import { useEffect, useMemo, useState } from 'react'
import { Download, Eye, FolderOpen, Link2, Trash2 } from 'lucide-react'
import {
  api,
  formatDate,
  type DocumentMeta,
  type Entity,
  type PostedEntryView,
} from '../lib/api'
import type { CommandError } from '../lib/tauri'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { Modal } from '../components/Modal'
import { formatBytes } from '../lib/files'
import {
  Button,
  EmptyState,
  ErrorBanner,
  Input,
  PageHeader,
  Panel,
} from '../components/ui'

type Props = { entity: Entity | null }

/** Every file in the entity's vault, including orphans never linked to an entry. */
export function DocumentsPage({ entity }: Props) {
  const [docs, setDocs] = useState<DocumentMeta[]>([])
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [error, setError] = useState<string | null>(null)
  const [viewerDocId, setViewerDocId] = useState<string | null>(null)
  const [deleteId, setDeleteId] = useState<string | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [linkDocId, setLinkDocId] = useState<string | null>(null)
  const [linkSearch, setLinkSearch] = useState('')
  const [busyId, setBusyId] = useState<string | null>(null)

  const entryById = useMemo(() => new Map(entries.map((e) => [e.entry.id, e])), [entries])

  const linkCandidates = useMemo(() => {
    const q = linkSearch.trim().toLowerCase()
    const pool = entries.filter((e) => !e.is_voided)
    if (!q) return pool.slice(0, 25)
    return pool
      .filter(
        (e) =>
          e.entry.description.toLowerCase().includes(q) ||
          e.entry.entry_date.includes(q),
      )
      .slice(0, 25)
  }, [entries, linkSearch])

  async function reload() {
    if (!entity) return
    const [d, e] = await Promise.all([api.documentList(entity.id), api.entryList(entity.id)])
    setDocs(d)
    setEntries(e)
  }

  useEffect(() => {
    if (!entity) {
      setDocs([])
      setEntries([])
      return
    }
    void reload().catch((err) => setError((err as CommandError).message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id])

  async function confirmDelete() {
    if (!deleteId) return
    setDeleteBusy(true)
    setError(null)
    try {
      await api.documentDelete(deleteId)
      setDeleteId(null)
      await reload()
    } catch (err) {
      setError((err as CommandError).message || 'Could not delete document')
    } finally {
      setDeleteBusy(false)
    }
  }

  async function linkTo(entryId: string) {
    if (!linkDocId) return
    setBusyId(linkDocId)
    setError(null)
    try {
      await api.documentLinkEntry(linkDocId, entryId)
      setLinkDocId(null)
      setLinkSearch('')
      await reload()
    } catch (err) {
      setError((err as CommandError).message || 'Could not link document')
    } finally {
      setBusyId(null)
    }
  }

  async function onExport(id: string) {
    setBusyId(id)
    try {
      await api.documentExport(id)
    } catch (err) {
      setError((err as CommandError).message || 'Could not save a copy')
    } finally {
      setBusyId(null)
    }
  }

  if (!entity) {
    return (
      <EmptyState
        icon={<FolderOpen className="size-5" />}
        title="No book selected"
        body="Create or select a book first."
      />
    )
  }

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Vault"
        title="Documents"
        description="Every file stored in this book's encrypted vault."
        meta="Encrypted at rest"
      />

      <ErrorBanner message={error} />

      <ConfirmDialog
        open={deleteId !== null}
        title="Delete document?"
        body="This permanently removes the file from your vault. It cannot be undone. Journal entries are not affected."
        confirmLabel="Delete"
        danger
        busy={deleteBusy}
        onCancel={() => {
          if (!deleteBusy) setDeleteId(null)
        }}
        onConfirm={() => void confirmDelete()}
      />

      <Modal
        open={linkDocId !== null}
        title="Link to entry"
        description="Pick the journal entry this file belongs to"
        maxWidth="max-w-xl"
        onClose={() => {
          setLinkDocId(null)
          setLinkSearch('')
        }}
      >
        <div className="space-y-4">
          <Input
            value={linkSearch}
            onChange={(e) => setLinkSearch(e.target.value)}
            placeholder="Search by description or date…"
          />
          {linkCandidates.length === 0 ? (
            <p className="py-6 text-center text-sm text-[var(--color-muted)]">
              No entries match.
            </p>
          ) : (
            <ul className="max-h-80 divide-y divide-[var(--color-border)] overflow-y-auto rounded-xl border border-[var(--color-border)]">
              {linkCandidates.map((v) => (
                <li key={v.entry.id}>
                  <button
                    type="button"
                    className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition hover:bg-[var(--color-surface-2)]/60"
                    onClick={() => void linkTo(v.entry.id)}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm text-[var(--color-fg)]">
                        {v.entry.description}
                      </div>
                      <div className="text-xs text-[var(--color-muted)]">
                        {formatDate(v.entry.entry_date)}
                      </div>
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Modal>

      <DocumentViewerModal
        documentId={viewerDocId}
        onClose={() => setViewerDocId(null)}
        onError={(msg) => setError(msg)}
      />

      {docs.length === 0 ? (
        <EmptyState
          icon={<FolderOpen className="size-5" />}
          title="No documents yet"
          body="Files you drop on the Transactions page are stored here, encrypted. Attachments to entries also appear in this list."
        />
      ) : (
        <Panel
          title="Vault files"
          description={`${docs.length} stored · encrypted`}
          icon={<FolderOpen className="size-4" />}
        >
          <ul className="divide-y divide-[var(--color-border)]">
            {docs.map((doc) => {
              const linked = doc.entry_id ? entryById.get(doc.entry_id) : undefined
              return (
                <li key={doc.id} className="flex items-center gap-4 px-5 py-3.5">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                      {doc.filename}
                    </div>
                    <div className="truncate text-xs text-[var(--color-muted)]">
                      {formatDate(doc.created_at.slice(0, 10))}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {formatBytes(doc.size_bytes)}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {doc.mime_type}
                    </div>
                  </div>

                  {doc.entry_id ? (
                    <span className="max-w-48 truncate rounded-full bg-[var(--color-accent-soft)] px-2.5 py-1 text-xs text-[var(--color-accent)]">
                      {linked?.entry.description ?? 'Linked entry'}
                    </span>
                  ) : (
                    <span className="rounded-full bg-[var(--color-surface-elevated)] px-2.5 py-1 text-xs text-[var(--color-muted)]">
                      Not linked
                    </span>
                  )}

                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => setViewerDocId(doc.id)}
                    aria-label="View document"
                    title="View"
                  >
                    <Eye className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => setLinkDocId(doc.id)}
                    aria-label="Link to entry"
                    title="Link to entry"
                  >
                    <Link2 className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    busy={busyId === doc.id}
                    onClick={() => void onExport(doc.id)}
                    aria-label="Save a copy"
                    title="Save a copy"
                  >
                    <Download className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => setDeleteId(doc.id)}
                    aria-label="Delete document"
                    title="Delete"
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </li>
              )
            })}
          </ul>
        </Panel>
      )}
    </div>
  )
}
```

- [ ] **Step 2: Add navigation**

In `web/src/App.tsx`:
- Add `FolderOpen` to the lucide import.
- Add `import { DocumentsPage } from './pages/DocumentsPage'` with the page imports.
- In `NAV`, insert after the transactions entry:

```ts
  { id: 'documents', label: 'Documents', icon: FolderOpen },
```

- In `<main>`, after the transactions line:

```tsx
            {active === 'documents' ? <DocumentsPage entity={entity} /> : null}
```

- [ ] **Step 3: Verify the frontend builds**

Run: `cd web && npm run build && npm run lint && cd ..`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add web/src/pages/DocumentsPage.tsx web/src/App.tsx
git commit -m "feat: Documents library page - view, link, export, delete vault files

Makes orphaned uploads visible for the first time: every stored file is
listed with its linked entry or a not-linked badge, a searchable picker
to link it, and a permanent-delete confirm."
```

---

### Task 8: Full gate and live verification

**Files:** none (verification only).

- [ ] **Step 1: Run the complete gate**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm run lint && npm run test && npm run build && cd ..
```

Expected: all clean/passing.

- [ ] **Step 2: Live walkthrough**

Run: `cargo tauri dev --manifest-path apps/desktop/src-tauri/Cargo.toml` (or `make dev` if preferred). Unlock the vault and verify, in order:

1. Drop a PDF on Transactions → post the suggested entry → the row shows a paperclip.
2. Click the row → detail modal shows lines with account names and the attachment.
3. View → PDF renders in the viewer; close → reopen works (blob URL fresh each open).
4. Save a copy → native save dialog → file lands where chosen; cancel → no error.
5. Attach file on the detail modal → second attachment appears.
6. Remove an attachment → confirm → it disappears here, appears "Not linked" under Documents.
7. Documents page → orphan visible; Link to entry → picker → badge switches to the entry description.
8. Delete a document → confirm → gone permanently; the linked entry is unaffected.
9. Transactions toolbar: search text matches description/reference/memo; date range narrows; account filter narrows; combined filters intersect; clearing restores the full list.
10. Lock the vault (or wait for idle lock) with the viewer open → app drops to the unlock screen without errors.

- [ ] **Step 3: Fix anything found, re-run the gate, and commit fixes**

Any defect found in the walkthrough gets its own minimal fix commit after re-running the gate.
