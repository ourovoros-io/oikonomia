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
- Business rules live in **`oikonomia-core`**, not the TypeScript UI.
- Vault data is **encrypted at rest**; no plaintext DB on disk.
- v1 Tauri capabilities: **no network** permission.
- No emojis in UI chrome, code, or commits.

## Style

Follow global `~/.grok/rules/AGENTS.md` and the `rust-style` skill.
`rustfmt.toml`: `use_small_heuristics = "Default"` (never `"Max"`).
Workspace Clippy: `unwrap_used = deny`, `panic = deny`, etc.

## Product decisions (locked)

See `docs/superpowers/specs/2026-08-10-oikonomia-design.md`.

## Schema

- `vault_meta.schema_version` starts at 1 on init; `db::migrate` upgrades to current (v2 = ledger tables).
- Migrations run on vault init and unlock.
