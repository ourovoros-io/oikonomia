<p align="center">
  <img src="docs/assets/logo.svg" width="150"
       alt="Oikonomia logo: a shield around a Greek house whose doorway is a keyhole with a euro coin">
</p>

<h1 align="center">Oikonomia</h1>

<p align="center"><em>οἶκος (house) + νόμος (order) — a shield (encryption)
around the oikos: two columns (double-entry) flank a keyhole doorway (the
vault), with a euro at the door.</em></p>

Local-only personal and company finance: **double-entry** multi-entity books,
**encrypted at rest**, with a Stripe-inspired desktop UI (dark mode first).

Stack: **Rust** (`oikonomia-core`) + **Tauri 2** + **React / Vite / Tailwind**.

Design: [`docs/superpowers/specs/2026-08-10-oikonomia-design.md`](docs/superpowers/specs/2026-08-10-oikonomia-design.md)

## v1 features

- Encrypted vault (Argon2id → SQLCipher); init / unlock / lock; change master password
- Encrypted vault backup/restore (one `.oikonomia-backup` file: `vault.db` + header, already SQLCipher); Settings + login restore; no second password; pick-then-confirm then unlock with master password
- Auto-lock on idle, enforced by a Rust watchdog thread (survives webview stalls)
- Multi-entity books with personal / company / blank chart templates
- Chart of accounts (create, deactivate)
- Journal entries (two-line post + void with reverse)
- Reports: trial balance, profit & loss, balance sheet
- Dashboard (MTD income/expense, assets)
- Dark / light theme; English UI; locale money formatting

## Prerequisites

- Rust stable (1.88+)
- Node 22+
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Xcode CLT on macOS)

## Develop

```bash
# Core library tests (domain + vault + ledger + reports + documents)
cargo test -p oikonomia-core
cargo clippy --all-targets --all-features -- -D warnings

# Frontend
cd web && npm install && npm run dev
cd web && npm test        # vitest: money parsing, date normalization
cd web && npm run build

# Full desktop app (from the repo root; `make app` does the same)
cargo tauri dev
```

## Pre-commit hooks

Run `prek install` once to enable the local hooks in `.pre-commit-config.yaml`
(cargo fmt, cargo clippy, tsc); `prek run --all-files` checks the whole tree.

## Security notes

- Master password is never stored; vault key is derived with Argon2id.
- Password change re-encrypts the vault via SQLCipher rekey (Settings).
- Lost password means lost data (no recovery key in v1).
- Vault files live under the OS app-data directory for `com.georgiosdelkos.oikonomia`.
- v1 has no network capability in Tauri permissions; idle auto-lock is enforced
  from Rust, not the webview.
- `reqwest` appears in `Cargo.lock` only as an optional, never-enabled Tauri
  dependency; `cargo tree --target all -i reqwest` confirms it is not built.
- On first run you will be warned: choose a strong password.

## Threat model (v1)

**Protects against:** stolen disk / backup of app data, casual browsing of the vault file.  
**Does not protect against:** malware while unlocked, keyloggers, memory forensics while the app is open.

## Layout

| Path | Role |
|------|------|
| `crates/oikonomia-core` | Domain, vault, ledger, reports |
| `apps/desktop/src-tauri` | Tauri shell + IPC |
| `web` | React UI |

## Out of v1 (backlog)

Recurring transactions, attachments, budgets, CSV import/export, invoicing, multi-currency, Greek UI, recovery key.
