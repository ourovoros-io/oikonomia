# Document invariants: no orphans, unique names — design

Date: 2026-08-11
Status: approved (brainstorm with owner)
Builds on: `2026-08-10-documents-entry-detail-design.md`, implemented on
`feature/documents-entry-detail` (unmerged; this round continues there).

## Problem

Two gaps in the documents feature as shipped on the branch:

1. Documents can exist without an entry. The drop-zone saves the file at
   analyze time — before any entry exists — so cancelling the form leaves
   an orphan, and "Remove" on the entry detail deliberately unlinks a file
   into a "Not linked" state.
2. Nothing prevents two documents with the same filename in one book,
   which invites which-one-is-which conflicts.

## Decisions (owner rulings)

- **Hard guarantee**: a document row can only exist linked to an entry.
  Enforced by the database (`entry_id NOT NULL`), not by convention.
- **Name clashes are rejected** with a clear validation error naming the
  file (`UNIQUE(entity_id, filename)`), never auto-renamed or replaced.
- **Migration auto-cleans** existing vaults: currently-unlinked documents
  are deleted; duplicate names keep the oldest file's name intact and
  suffix later ones (`invoice.pdf` → `invoice (2).pdf`, bumping until
  free). The owner links/exports any wanted "Not linked" files in the app
  before this ships.
- **Stateless re-supply** (approach A): analyze is pure; the frontend
  re-supplies bytes (or the file path) at post time, and entry + document
  are written in one transaction. No pending-bytes cache in the backend.

## Schema v4

SQLite cannot add `NOT NULL`/`UNIQUE` in place, so `db::migrate` (v3 → 4)
rebuilds the table:

1. `DELETE FROM documents WHERE entry_id IS NULL` (auto-clean, logged).
2. Per-book name dedup: for each `(entity_id, filename)` group with more
   than one row, the oldest row (by `created_at`, `rowid` tiebreak) keeps
   its name; later rows get a suffix inserted before the extension,
   incrementing until the name is free in that book (logged).
3. Create `documents_new` with `entry_id TEXT NOT NULL REFERENCES
   journal_entries(id)` and `UNIQUE(entity_id, filename)`; copy rows;
   drop old; rename; recreate `idx_documents_entity` /
   `idx_documents_entry`.
4. `schema_version` → 4. Runs on unlock like prior migrations.

`analysis_json` stays (scanner audit snapshot); it is written at post
time now.

## Core (`oikonomia-core`)

- `analyze_document_bytes` path unchanged; the `document_analyze` /
  `document_analyze_path` commands stop persisting anything.
  `DocumentSuggestion` loses `document_id`.
- New `documents::post_simple_entry_with_document(conn, input, filename,
  mime_type, data, analysis_json: Option<&str>)
  -> Result<(PostedEntryView, DocumentMeta)>`: one transaction wrapping
  `post_simple_entry` + `save_document` + `link_document_to_entry` +
  optional `save_analysis_json`. A name clash (or any failure) rolls back
  the entry too.
- `save_document` maps the UNIQUE-constraint violation to
  `Error::Validation("a document named {name} already exists in this
  book")`; `attach_document` inherits it.
- **Removed**: `unlink_document` (core fn, export, command, TS method) —
  un-attaching without deleting is an orphan by definition. The
  standalone `document_link_entry` command is removed as well; linking
  happens only inside the two transactional paths. `link_document_to_entry`
  stays core-internal (`pub(crate)`); existing integration tests that
  called it or `unlink_document` directly are reworked onto
  `attach_document` (the unlink tests are deleted with the function).
- `delete_document` unchanged: entries without documents are fine — the
  invariant points one way.
- Name-suffix helper (`filename`, taken names → next free name) is a pure
  function with unit tests; shared by the migration.

## IPC commands

| Command | Change |
|---|---|
| `document_analyze`, `document_analyze_path` | No longer save; validate + OCR + suggest only. |
| `entry_post_simple_with_document(input, filename, mime_type, data_base64, analysis_json?)` | New; base64 gate as `document_analyze`, then the core transactional fn. |
| `entry_post_simple_with_document_path(input, path, analysis_json?)` | New; re-reads and re-validates the path at post time (metadata gate before read); if the file moved, clean io error and nothing is written. |
| `document_unlink`, `document_link_entry` | Deleted (registration, commands, TS methods). |
| Everything else | Unchanged. |

## Frontend

- **TransactionsPage**: replace the saved-document id with `pendingDoc`
  state — `{ kind: 'file', file: File } | { kind: 'path', path: string }
  | null` plus scan notes. Save calls the with-document command when a
  doc is pending (encode at save time), plain `entryPostSimple`
  otherwise. Cancel just clears state (nothing was written). Name-clash
  validation surfaces in the existing banner with the filename; the form
  stays filled for retry.
- **EntryDetailModal**: "Remove" becomes **Delete** — danger confirm
  ("permanently deletes the file from your vault"), `documentDelete`.
  Attach unchanged apart from surfacing the duplicate-name error.
- **DocumentsPage**: remove the "Not linked" badge, the Link action, and
  the link-picker modal; every row always shows its entry description.
  Actions: View, Save a copy, Delete.
- **DocumentDropZone / viewer**: mechanics unchanged; the suggestion no
  longer carries a document id.

## Error handling

- Duplicate name → `Error::Validation` naming the file; on the post path
  the entry rolls back with it.
- Dropped-path file missing/changed at save → io error, nothing written.
- Migration deletions/renames logged via `log::info`.

## Testing

Core:
- Migration test: rebuild the v3 table shape in a fresh vault (drop the
  v4 table, recreate the old shape, set `schema_version = 3`), insert an
  orphan and a same-book name clash, re-run migrations; assert the orphan
  is gone, the later duplicate is `x (2).pdf`, and afterwards inserting
  an orphan or a duplicate name is rejected by the constraints (requires
  the migration entry point to be callable from integration tests).
- `post_simple_entry_with_document`: success posts entry + doc + link +
  analysis atomically; a name clash rolls back the entry (entry count
  unchanged); an invalid file rolls back everything.
- `attach_document` rejects a duplicate name.
- Suffix helper: extension insertion, no-extension names, collision
  bumping.

Gate: `cargo fmt --all`, `cargo clippy --all-targets --all-features --
-D warnings`, `cargo test -p oikonomia-core`, `cd web && npm run build
&& npm run lint`. Combined live walkthrough of both rounds before merge.
