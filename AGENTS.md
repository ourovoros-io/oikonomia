# Oikonomia — agent / contributor notes

Local-only personal + company finance app: **double-entry** multi-entity ledger,
**Tauri 2** + **React** UI (dark Aurora glass), **SQLCipher** vault,
master-password unlock.

## Layout

| Path | Role |
|------|------|
| `crates/oikonomia-core` | Domain, vault, ledger, reports |
| `crates/oikonomia-update` | Signed update check, the only network path |
| `crates/macos-dock-icon` | macOS Dock icon for `cargo tauri dev` |
| `crates/oikonomia-test-support` | Test-only macros shared by the crates (a dev-dependency) |
| `apps/desktop/src-tauri` | Tauri shell + IPC commands |
| `web` | React + Vite + Tailwind frontend |
| `docs/` | Release runbook (`release.md`), brand assets, and the design overview (`DESIGN.md`) |

## Commands

```bash
# Core library tests
cargo test -p oikonomia-core

# Format + lint (workspace)
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Frontend
cd web && npm install && npm run dev
cd web && npm run build

# Desktop (from the repo root)
cargo tauri dev
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
  native drop or a native dialog (`AppState::grant_paths`).
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
Workspace Clippy: `unwrap_used = deny`, `panic = deny`, etc.

- Function names are full words. Cryptic abbreviations (`ta`, `row_err`, `chk_bal`)
  are not allowed. Prefer associated constructors (`TemplateAccount::new`) over
  2–3 letter helpers. Public and `pub(crate)` items have rustdoc. Comments
  explain intent and invariants, not the identifier.

## Design overview

See [`docs/DESIGN.md`](docs/DESIGN.md).

## Schema

- `vault_meta.schema_version` starts at 1 on init; `db::migrate` upgrades to current (v8 = indexes on void links and entry lines; v7 = recurring templates).
- Migrations run on vault init and unlock.
