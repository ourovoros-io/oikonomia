# Oikonomia — agent / contributor notes

Local-only personal + company finance app: **double-entry** multi-entity ledger,
**Tauri 2** + **React** UI (dark Aurora glass), **SQLCipher** vault,
master-password unlock.

## Layout

| Path | Role |
|------|------|
| `crates/oikonomia-core` | Domain, vault, ledger, reports |
| `crates/oikonomia-update` | Signed update check, the only network path |
| `crates/macos-dock-icon` | macOS Dock icon in dev mode |
| `crates/oikonomia-test-support` | Test-only macros shared by the crates (a dev-dependency) |
| `apps/desktop/src-tauri` | Tauri shell: windows, tray, watchdog (`state.rs`), updates |
| `apps/desktop/src-tauri/src/commands/` | IPC commands, one module per subject; shared helpers and the mock-IPC test support in `support.rs` |
| `web` | React + Vite + Tailwind frontend |
| `docs/` | Release runbook (`release.md`), brand assets, and the design overview (`DESIGN.md`) |

## Commands

Every `cargo` command takes `--locked`. The desktop crate needs `web/dist`
to exist (`mkdir -p web/dist`, or a web build).

```bash
# Tests: core alone, or everything CI runs
cargo test -p oikonomia-core --locked
cargo test --workspace --locked --lib --bins --tests

# Format + lint (workspace)
cargo fmt --all
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Docs, private items included, warnings as errors (`make doc`)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked

# Frontend
cd web && npm ci && npm run dev
cd web && npm run build

# Desktop in dev mode (from the repo root). Runs the Tauri CLI pinned in
# web/package.json, not a global `cargo tauri`.
make app

# Local gate, a subset of CI
make check
```

## Invariants (do not break)

- Money is **integer minor units** (`i64`); never `f64` for currency.
- Posted journal entries: **Σ debits == Σ credits**, ≥ 2 lines, debit XOR credit per line.
- Business rules live in **`oikonomia-core`**, not the TypeScript UI. The simple
  entry form posts through `post_simple_entry` (kind → debit/credit mapping in Rust).
- Multi-statement ledger writes run inside transactions (`unchecked_transaction`).
- Report queries put entry predicates in an **inner-join subquery**, never in a
  `LEFT JOIN ... ON` clause (that pattern silently disables the filters).
- Vault data is **encrypted at rest**; no plaintext DB on disk. Vault files are
  owner-only (`vault/permissions.rs`).
- Network exists ONLY on the click-driven update path (`oikonomia-update`, which uses `ureq`). `oikonomia-core` stays fully offline (`scripts/assert-core-offline.sh`); `deny.toml` wrappers confine every socket-capable crate to that path.
- An update is installed the way the running copy was installed (`update_exec.rs` `InstallKind`). A package-managed copy (`.deb`) never writes over itself.
- Path-taking IPC commands accept only paths the user handed over through a
  native drop or a native dialog, and only for the purpose each was handed
  over for (`AppState::grant_paths`, `GrantPurpose`): a dropped file or a CSV
  pick is never accepted by `vault_restore`.
- Journal CSV export neutralizes formula-leading cells; `parse_journal_export`
  reverses it.
- Idle auto-lock is enforced by the Rust watchdog (`spawn_auto_lock`), not the UI timer.
- Tray left-click opens the quick-add companion window (`quick-add` label); right-click is the tray menu. Linux trays report no clicks, so there the menu has a "Quick add" item.
- One process per user: a second launch on Windows or Linux shows the running app's window and exits (`with_single_instance`).
- Platform-conditional code is checked by CI on Linux, Windows and macOS runners; the `bundle` job installs and runs the real installers (`scripts/smoke-linux.sh`, `scripts/smoke-windows.ps1`).
- The UI never renders raw backend text. Backend text is a code plus
  parameters that the UI words, or stored text that core wrote (seeded account
  names, generated descriptions). Accounts are never chosen by name: defaults
  come from template code and account type.
- No emojis in UI chrome, code, or commits.

## Style

`rustfmt.toml`: `use_small_heuristics = "Default"` (never `"Max"`).
The lint set is the baseline in the root `Cargo.toml` (`[workspace.lints]`:
pedantic Clippy, `unwrap_used`, `panic` and `allow_attributes` denied,
`missing_docs`, `unsafe_code` forbidden) plus `clippy.toml` (`unwrap` and
`expect` allowed in tests, at most 5 parameters). Every crate inherits it;
silence a lint with `#[expect(..., reason = "...")]`, never `#[allow]`.

- IPC tests go through Tauri's mock runtime (`ipc_test_support::MockApp`), in a
  module gated `#[cfg(test)]` then `#[cfg(not(windows))]`.
- Function names are full words. Cryptic abbreviations (`ta`, `row_err`, `chk_bal`)
  are not allowed. Prefer associated constructors (`TemplateAccount::new`) over
  2–3 letter helpers. Public and `pub(crate)` items have rustdoc. Comments
  explain intent and invariants, not the identifier.

## Design overview

See [`docs/DESIGN.md`](docs/DESIGN.md).

## Schema

- `vault_meta.schema_version` starts at 1 on init; `db::migrate` upgrades to current (v8 = indexes on void links and entry lines; v7 = recurring templates).
- Migrations run on vault init and unlock.
