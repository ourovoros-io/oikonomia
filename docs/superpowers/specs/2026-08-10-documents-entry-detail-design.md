# Documents surfacing, entry detail, and entry filters — design

Date: 2026-08-10
Status: approved (brainstorm with owner)

## Problem

Dropped files are stored as encrypted blobs in the vault (`documents` table)
and linked to the entry they create, but the app never shows them again:
there is no IPC to list or fetch a document, entry rows give no hint an
attachment exists, and orphaned uploads (analyzed but never posted) sit
invisible in the vault forever. Separately, the transactions list has no
search or filtering, which hurts as the ledger grows.

## Goals

1. See and open the file(s) attached to an entry, in-app.
2. An entry detail view: full journal lines, memo, reference, attachments.
3. A Documents library page: every stored file, including orphans; link,
   export, delete.
4. Attach a file to an already-posted entry.
5. Search text, date range, and account filter on the transactions list.

## Non-goals

- Editing posted entries (amounts/accounts) — still void-and-repost.
- OCR/analysis on the attach-to-existing flow (drop-zone flow only).
- Document versioning, tags, or full-text search inside file contents.
- Multi-file selection in one attach action.

## Decisions

- **Viewing is in-app, from memory.** Decrypted bytes become a blob URL
  rendered inside a modal; the URL is revoked when the modal closes.
  Plaintext reaches disk only through an explicit "Save a copy" / Export
  action (native save dialog).
- **Entry detail opens in a modal** (existing `Modal` component), not a
  drawer — consistent with the new-entry form.
- **Documents is a sidebar page**, listed for the active entity.
- **One metadata call powers everything.** `document_list(entity_id)`
  returns metas only (no blobs); the UI groups by `entry_id` for paperclip
  badges and the detail modal, and lists them flat on the Documents page.
- **Delete is hard** (blob removed, entry keeps existing); **remove from an
  entry is unlink** (file returns to the library as "Not linked").
- No schema change; `documents` v3 already carries everything needed.

## Backend (`oikonomia-core`)

New in `documents::store`:

| Function | Behavior |
|----------|----------|
| `list_documents(conn, entity_id)` | All `DocumentMeta` for the entity, newest first; never reads `data`. |
| `get_document(conn, id)` | `(DocumentMeta, Vec<u8>)` for viewing/export. |
| `delete_document(conn, id)` | Hard `DELETE`; `NotFound` if missing. |
| `unlink_document(conn, id)` | Sets `entry_id = NULL`; `NotFound` if missing. |

`ledger::journals::list_entries` already takes `from`/`to` as ISO date
strings (normalized via `parse_date`/`format_date` — dates cross IPC as ISO
strings). Those two params fold into an optional filter struct alongside the
new predicates; the one existing caller updates:

```rust
pub struct EntryFilter {
    pub text: Option<String>,     // LIKE over description, reference, line memos
    pub date_from: Option<String>, // ISO, normalized like today's `from`
    pub date_to: Option<String>,   // inclusive bounds
    pub account_id: Option<AccountId>, // EXISTS subquery on journal_lines
}
```

Filtering is SQL-side. The account predicate uses an `EXISTS` subquery on
`journal_lines`, never a `LEFT JOIN ... ON` predicate (report invariant).

## IPC (Tauri commands)

All use the existing `with_vault_blocking` pattern and `CommandError`.

| Command | Notes |
|---------|-------|
| `document_list(entity_id)` | `Vec<DocumentMeta>`. |
| `document_get(document_id)` | Meta + base64 data; 8 MiB cap bounds payload. |
| `document_delete(document_id)` | — |
| `document_unlink(document_id)` | — |
| `document_attach(entity_id, entry_id, filename, mime_type, data_base64)` | Validates via `validate_document_file`, saves, links; no OCR, `analysis_json` stays NULL. Same base64 pre-decode size gate as `document_analyze`. |
| `document_export(document_id)` | Opens native save dialog (`tauri-plugin-dialog`, official), writes bytes Rust-side, returns chosen path or `None` on cancel. |
| `entry_list(entity_id, filter?)` | Filter args optional with serde defaults; existing callers unchanged. |

`tauri-plugin-dialog` is the only new dependency; it needs no network
permission, so the v1 no-network capability stands.

## Frontend

**TransactionsPage**
- Toolbar above the list: debounced search input, date-from/to, account
  select; all drive `entry_list` (no client-side filtering).
- Paperclip badge on rows with linked documents (from the grouped
  `document_list` result — no per-row queries).
- Row click opens `EntryDetailModal`; existing delete button unchanged.

**EntryDetailModal** (new component)
- Header: description, date, reference.
- Lines table: account name (resolved from the page's loaded accounts),
  debit, credit, memo.
- Attachments list: filename + size, actions **View**, **Save a copy**,
  **Remove** (confirm → unlink). **Attach file** button opens the OS file
  picker and calls `document_attach`.

**DocumentViewerModal** (new component)
- Blob URL from `document_get` bytes: `<img>` for images, `<iframe>` for
  PDF (WKWebView renders natively), `<pre>` for `text/plain`.
- URL revoked on close. Footer: filename, size, **Save a copy**.

**DocumentsPage** (new page, sidebar item after Transactions)
- Table: filename, type, size, added date, linked entry description or a
  "Not linked" badge.
- Row actions: View, Link to entry (searchable picker over recent entries),
  Export, Delete (confirm; copy warns it is permanent).

Styling reuses existing tokens, `ui.tsx` primitives, `Modal`,
`ConfirmDialog`. No new visual patterns, no emojis.

## Error handling

- Deleted-underneath / missing id → `NotFound` message, list refreshes.
- Vault auto-lock mid-view → modal closes with the lock screen (existing
  global behavior).
- Attach validation failures (size/type/blank name) surface the Rust
  validation message, rejected from metadata before bytes are read where
  possible.
- Export cancel → quiet no-op.

## Security

- Decrypted bytes live only in memory while a viewer modal is open.
- Disk writes of plaintext happen only via the explicit export action.
- All validation, linking, and filtering logic lives in `oikonomia-core`.

## Testing

Core (`documents_flow.rs`):
- save → list → get round-trip; list returns metas without blob reads.
- delete removes the row; get afterwards is `NotFound`.
- unlink orphans the document but preserves it; relink works.
- attach path: validate + save + link in one step, `analysis_json` NULL.

Core (`ledger_flow.rs` neighborhood):
- text filter matches description, reference, and line memo independently.
- date bounds inclusive on both ends.
- account filter via lines; combined filters intersect.

Gate: `cargo fmt --all`, `cargo clippy --all-targets --all-features --
-D warnings`, `cargo test -p oikonomia-core`, `cd web && npm run build`.
UI verified in the running app (drop → post → badge → detail → view →
export → unlink → library → relink → delete).
