# Oikonomia — agent / contributor notes

Local-only personal + company finance app: **double-entry** multi-entity ledger,
**Tauri 2** + **React** UI (Stripe-inspired, dark mode), **SQLCipher** vault,
master-password unlock.

## Layout

| Path | Role |
|------|------|
| `crates/oikonomia-core` | Domain, money, validation, (later) vault/repo/reports |
| `apps/desktop/src-tauri` | Tauri shell + IPC commands |
| `web` | React + Vite + Tailwind frontend |
| `docs/` | Design specs and plans |

## Commands

```bash
# Core library tests
cargo test -p oikonomia-core

# Format + lint (workspace)
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings

# Frontend
cd web && npm install && npm run dev
cd web && npm run build

# Desktop (from apps/desktop/src-tauri or with path)
cargo tauri dev --manifest-path apps/desktop/src-tauri/Cargo.toml
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
- v1 Tauri capabilities: **no network** permission. `deny.toml` bans every
  socket-capable crate on the desktop targets (`cargo deny check` in CI), the
  `nav_guard` plugin keeps every webview on the app origin, and
  `config_checks.rs` pins the CSP.
- Path-taking IPC commands accept only paths the user handed over through a
  native drop or a native dialog (`AppState::grant_paths`).
- Journal CSV export neutralizes formula-leading cells; `parse_journal_export`
  reverses it.
- Idle auto-lock is enforced by the Rust watchdog (`spawn_auto_lock`), not the UI timer.
- Tray left-click opens the quick-add companion window (`quick-add` label); right-click is the tray menu.
- No emojis in UI chrome, code, or commits.

## Style

Follow global `~/.grok/rules/AGENTS.md` and the `rust-style` skill.
`rustfmt.toml`: `use_small_heuristics = "Default"` (never `"Max"`).
Workspace Clippy: `unwrap_used = deny`, `panic = deny`, etc.

- Function names are full words. Cryptic abbreviations (`ta`, `row_err`, `sign_lic`)
  are not allowed. Prefer associated constructors (`TemplateAccount::new`) over
  2–3 letter helpers. Public and `pub(crate)` items have rustdoc. Comments
  explain intent and invariants, not the identifier.

## Product decisions (locked)

See `docs/superpowers/specs/2026-08-10-oikonomia-design.md`.

## Schema

- `vault_meta.schema_version` starts at 1 on init; `db::migrate` upgrades to current (v7 = recurring templates).
- Migrations run on vault init and unlock.
