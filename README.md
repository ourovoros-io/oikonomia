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
- Journal entries: two-line post + void with reverse, editing (replace, hides the original), opening balances
- Recurring transaction templates (schedule generates due entries)
- Bank CSV import (confirm before post) and journal CSV export (formula-neutralized cells)
- Document capture: attach files to entries, offline OCR (bundled `.rten` text-detection/recognition models) reads receipts and invoices with no network call
- Mark entries Hidden so journal CSV omits them; the vault backup still includes those lines
- Per-account register: running balance, drill from Accounts into any account's entries
- Reports: trial balance, profit & loss, balance sheet; P&L PDF export
- Dashboard (MTD income/expense, assets); every empty page offers a first-run "create a book" CTA
- Tray quick-add window: left-click the tray to post a simple entry or drop a document without opening the full app
- Signed, click-driven update check on the unlock screen (minisign-verified, talks only to github.com)
- Offline licensing: 30-day trial, offline `.lic` file import in Settings, buy link; EULA viewable in-app from Settings
- Localized error messages
- Global trial banner (last 7 days of trial, or after expiry) with a buy-license link
- Dark / light theme; locale money formatting
- UI language: English, Ελληνικά, Français, and Deutsch; switch in Settings; persists across relaunch

## Support

Questions, bug reports, license problems, and security reports all go to
**info@ourovoros.io**. The app shows the same address under Settings → Support,
with a one-click email whose subject already carries the app version. Never
attach a vault or backup file to a report.

## Prerequisites

- Rust stable (1.94+)
- Node 22+
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Xcode CLT on macOS)

## Develop

```bash
# Full desktop app (from the repo root; `make app` does the same)
cargo tauri dev

# Core library tests (domain + vault + ledger + reports + documents)
cargo test -p oikonomia-core        # or: make test
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Dependency policy: network crates are banned everywhere except the signed
# update path (crates/oikonomia-update, tauri-plugin-updater); needs
# `cargo install cargo-deny`
cargo deny check

# Frontend
cd web && npm install && npm run dev
cd web && npm test        # vitest: money parsing, date normalization
cd web && npm run build

# Full quality gate mirroring CI: fmt, clippy, cargo-deny, offline-core
# assertion, core tests, web tsc/test/build
make check

# Build the .app bundle and verify it actually renders (never smoke-test
# the bare `cargo build` binary — it opens a blank window)
make smoke
```

## Pre-commit hooks

Run `prek install` once to enable the local hooks in `.pre-commit-config.yaml`
(cargo fmt, cargo clippy, tsc); `prek run --all-files` checks the whole tree.

## Security notes

- Master password is never stored; vault key is derived with Argon2id.
- Password change re-encrypts the vault via SQLCipher rekey (Settings).
- Lost password means lost data (no recovery key in v1).
- Vault files live under the OS app-data directory for `io.ourovoros.oikonomia`.
- Offline by design: the app performs **no background network activity**. The
  single network action is the update check you click on the unlock screen; it
  talks only to GitHub's release hosts (the public
  `ourovoros-io/oikonomia-releases` repo) and verifies a minisign signature
  over both the update manifest and the downloaded artifact before anything is
  installed. The vault, ledger, and license paths (`oikonomia-core`) contain
  no network code at all —
  `scripts/assert-core-offline.sh` and `cargo deny check` enforce this in CI.
- Every webview is pinned to the app's own origin (`nav_guard`), and the CSP
  allows no remote source. The webview cannot supply a URL or key to the
  updater.
- Journal CSV exports neutralize cells that spreadsheets would run as formulas.
- On first run you will be warned: choose a strong password.
- Report security issues to info@ourovoros.io.

## Threat model (v1)

**Protects against:** stolen disk / backup of app data, casual browsing of the vault file.  
**Does not protect against:** malware while unlocked, keyloggers, memory forensics while the app is open.  
**Update channel:** a compromised GitHub account cannot ship a malicious update (artifacts are minisign-verified against the baked key), but a compromised signing key can — the key ceremony in docs/release.md keeps it offline.

## Layout

| Path | Role |
|------|------|
| `crates/oikonomia-core` | Domain, vault, ledger, reports |
| `crates/oikonomia-update` | Signed update check (network isolated from core) |
| `crates/oikonomia-mint` | Offline license minting (operator-only binary, never shipped) |
| `crates/macos-dock-icon` | Sets the macOS Dock icon for `cargo tauri dev` |
| `apps/desktop/src-tauri` | Tauri shell + IPC |
| `web` | React UI |

## Out of v1 (backlog)

Attachments, budgets, invoicing, multi-currency, recovery key, Windows/Linux
go-live, webhook fulfillment, App Sandbox.
