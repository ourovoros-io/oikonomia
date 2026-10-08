<p align="center">
  <img src="docs/assets/logo.svg" width="150"
       alt="Oikonomia logo: a shield around a Greek house whose doorway is a keyhole with a euro coin">
</p>

<h1 align="center">Oikonomia</h1>

<p align="center"><em>οἶκος (house) + νόμος (order) — a shield (encryption)
around the oikos: two columns (double-entry) flank a keyhole doorway (the
vault), with a euro at the door.</em></p>

Oikonomia is a free, open-source desktop app for personal and company finance.
It keeps **double-entry** books for several entities, **encrypted at rest**,
with a dark "Aurora glass" interface. It needs no account, no cloud, and sends
no telemetry.

Status: 0.1.x, early releases.

Stack: **Rust** (`oikonomia-core`) + **Tauri 2** + **React / Vite / Tailwind**.

Design: [`docs/DESIGN.md`](docs/DESIGN.md)

## Features

- Encrypted vault (Argon2id → SQLCipher); init / unlock / lock; change master password
- Encrypted vault backup/restore (one `.oikonomia-backup` file: `vault.db` + header, already SQLCipher); Settings + login restore; no second password; pick-then-confirm then unlock with master password
- Auto-lock on idle, enforced by a Rust watchdog thread (survives webview stalls)
- Multi-entity books with personal / company / blank chart templates
- Chart of accounts (create, deactivate)
- Journal entries: two-line post + void with reverse, editing (replace, hides the original), opening balances
- Recurring transaction templates (weekly, monthly, yearly; shows what is due, and you post each occurrence yourself)
- Bank CSV import (confirm before post) and journal CSV export (formula-neutralized cells)
- Document capture: attach files to entries, offline OCR (bundled `.rten` text-detection/recognition models) reads receipts and invoices with no network call
- Mark entries Hidden so journal CSV omits them; the vault backup still includes those lines
- Per-account register: running balance, drill from Accounts into any account's entries
- Reports: trial balance, profit & loss, balance sheet; expense breakdown chart and expense PDF export (Latin, Greek and Cyrillic text)
- Dashboard: month, quarter, or year income, expenses, and assets, with savings and spending arcs and a cash-flow chart; every empty page offers a first-run "create a book" CTA
- Cash-flow chart on the Transactions page
- Documents page: every stored file in the book, with an in-app viewer and export
- Closing the window hides the app to the tray (on Linux, where a desktop may show no tray, it minimizes the window); launching it again brings the window back; window size and position are remembered
- Settings: optional donation addresses with copy buttons
- Master password must be at least 12 characters
- Tray quick-add window: left-click the tray (on Linux, choose "Quick add" in the tray menu) to post a simple entry or drop a document without opening the full app
- Signed, click-driven update check on the unlock screen (minisign-verified, talks only to GitHub's release hosts); every build except the `.deb` updates in place, and the `.deb` only reports a newer version
- Dark-only "Aurora glass" UI; locale money formatting
- UI language: English, Ελληνικά, Français, and Deutsch; the first launch follows the system language when it is one of these four, switchable on the first screen and in Settings; persists across relaunch; the interface, error messages, and the names seeded into new books follow the selected language

## Install

Releases appear on this repository's
Releases page, each with a `SHA256SUMS` file so you can check what you
downloaded.

| Platform | File | Updates |
|----------|------|---------|
| macOS on Apple Silicon (12 or later) | `.dmg` | In the app |
| Linux x86_64 | `.AppImage` | In the app |
| Linux x86_64, Debian and Ubuntu | `.deb` | The app tells you when a newer version exists; install the new `.deb` over the old one |
| Windows 10 and 11, x86_64 | `-setup.exe` | In the app |

The Windows installer is not yet signed with a code-signing (Authenticode)
certificate, so on a first install Windows shows "Windows protected your PC":
choose More info, then Run anyway. Check the file against `SHA256SUMS` first.
Updates from inside the app are verified with the same minisign signature as
on macOS and Linux, which does not depend on that certificate.

The installer embeds Microsoft's WebView2 bootstrapper. Windows 11 includes
the runtime and Windows 10 receives it through Windows Update, so the
bootstrapper downloads it only when the machine does not already have it.

On Linux the tray icon needs a desktop that shows one (KDE, or GNOME with the
AppIndicator extension). Without it the app works the same; reach quick-add by
launching the app, which brings the window back.

## Support

Questions and bug reports go to
**info@ourovoros.io**. The app shows the same address under Settings → Support,
with a one-click email whose subject already carries the app version. Never
attach a vault or backup file to a report.
Security reports: see [SECURITY.md](SECURITY.md).

## Prerequisites

- Rust stable (the workspace declares 1.94 as its minimum; CI tracks the latest stable)
- Node 22.22.2 or newer on 22.x, 24.15 or newer, or 26 or newer
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Xcode CLT on macOS; the WebKitGTK packages on Linux; the MSVC build tools on Windows)
- On Windows, [Strawberry Perl](https://strawberryperl.com/) ahead of any other `perl` on `PATH`: the vault's SQLCipher builds OpenSSL from source, and the `perl` that ships with Git Bash lacks the modules that build needs
- cargo-deny: `cargo install cargo-deny --locked`

## Develop

The Tauri shell crate needs `web/dist` to exist at compile time, so run
`cd web && npm ci && npm run build` once (or `mkdir -p web/dist`) before any
workspace-wide `cargo` command such as `cargo clippy --workspace` or
`cargo test --workspace`. Every `cargo` command takes `--locked`, as CI's
do, so a build never changes `Cargo.lock` behind your back.

The Tauri CLI is not a prerequisite. It is an exact devDependency in
`web/package.json`, and `make app` and `make bundle` run that copy; a
globally installed `cargo tauri` may be another version.

```bash
# Full desktop app in dev mode, from the repo root. Runs
# `node web/node_modules/@tauri-apps/cli/tauri.js dev -- --locked`.
make app

# Core library tests (domain + vault + ledger + reports + documents)
cargo test -p oikonomia-core --locked        # or: make test
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Dependency policy: network crates are banned everywhere except the signed
# update path (crates/oikonomia-update)
cargo deny check

# Frontend
cd web && npm ci && npm run dev
cd web && npm test        # vitest: UI and library tests
cd web && npm run build

# Local quality gate: fmt check, clippy, cargo-deny, offline-core assertion,
# `cargo test -p oikonomia-core`, web tsc/test/build. CI additionally runs
# `cargo test --workspace`, `cargo build -p oikonomia`, web lint, and cargo audit.
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
- Lost password means lost data (there is no recovery key).
- Vault files (`vault.db` and `vault.header.json`) live in the OS application-data
  directory, together with a small plaintext `ui-prefs.json`.
- The only other file the app writes on its own is a local error log, which
  holds nothing from your books; see [Error log](#error-log).
- Offline by design: the app performs **no background network activity**. The
  only network actions are the update check you click on the unlock screen and,
  if you accept an update, its download, both started by a click. They talk only
  to GitHub's release hosts (this repository's published releases) and verify a
  minisign signature over both the update manifest and the downloaded artifact
  before anything is installed. The `.deb` build checks for a newer version but
  never downloads or installs one.
- The vault and ledger paths (`oikonomia-core`) contain no network code at all;
  `scripts/assert-core-offline.sh` and `cargo deny check` enforce this in CI.
- Every webview is pinned to the app's own origin (`nav_guard`), and the CSP
  allows no remote source. The webview cannot supply a URL or key to the
  updater.
- Journal CSV exports neutralize cells that spreadsheets would run as formulas.
- On first run you will be warned: choose a strong password.
- Report security issues to info@ourovoros.io.

## Error log

Installed builds keep a small error log on your machine, so that a failed start
or a failed update leaves something to go on. It never leaves the machine: the
app sends nothing anywhere, and the log is read only if you open it or decide
to send it with a bug report.

| Platform | Folder |
|----------|--------|
| macOS | `~/Library/Logs/io.ourovoros.oikonomia/` |
| Linux | `~/.local/share/io.ourovoros.oikonomia/logs/` (under `$XDG_DATA_HOME` when that is set) |
| Windows | `%LOCALAPPDATA%\io.ourovoros.oikonomia\logs\` |

- **Files.** `errors.log`, at most 1 MB, and `errors.previous.log`, the file
  before it. Older lines are discarded, so the two together stay under about
  2 MB. On macOS and Linux only your user account can read them.
- **Contains.** Warnings and errors from the app itself, one per line: the
  time, the part of the app, and what failed, such as a step of an update or
  opening the vault, with the operating system's message ("permission
  denied"). Paths of the app's own files can appear (its data folder, the
  update cache, where it is installed), and those include your user name.
- **Does not contain.** Anything from your books: no amounts, descriptions,
  merchants, account or book names, document names or contents, no password
  or key, and no path of a file you chose (a backup, a statement, a
  document). Nothing the interface shows or sends is written there.
- **Deleting it.** Delete the two files, or the folder, whenever you like. An
  empty `errors.log` is created again the next time the app starts.

Development builds (`make app`) log more, with full error detail, to the
terminal and to `Oikonomia.log` in the same folder.

## Threat model

**Protects against:** stolen disk / backup of app data, casual browsing of the vault file.  
**Does not protect against:** malware while unlocked, keyloggers, memory forensics while the app is open.  
**Update channel:** a compromised GitHub account cannot ship a malicious update (artifacts are minisign-verified against the baked key), but a compromised signing key can — the signing key is generated offline and stored only as a secret on the repository's `release` environment (see docs/release.md).

## Donate

Oikonomia is free. If it is useful to you, a donation helps keep it maintained.
Check the coin and network before sending: crypto transfers cannot be reversed.

| Coin | Network | Address |
|------|---------|---------|
| BTC | Bitcoin | `bc1q9gey0j6vvd2nh7eh76tp932r2ttmdj75u55een` |
| ETH, USDC, USDT | Ethereum | `0x544506F873EF9157E3639B9D0Af13562245baf07` |
| XMR | Monero | `8ABaPsJS6754dY7YsZLKuHRrYFMtE5BBmi8SwZ7n79ukMAHkN987PZFHPMwaD4QhLegX6MPAjwEup69RbMAEnRcENDdfavg` |
| DASH | Dash | `XkfYevHXMAFwMcY6nj95oedwxaicTpqbSs` |
| LTC | Litecoin | `ltc1q2lhxq7crwaam9upc8ks0gmyvtxn9htatfgns33` |
| SOL, USDC, USDT | Solana | `EZVrqTLW3sTHdFZydoR31Nv3QLnWShfV4riah1otTC26` |
| ZEC | Zcash (transparent) | `t1MyGx1wXSQRyQZeXJjyKjHKTZEWSyEiBkj` |

The same addresses are shown in the app under Settings → Donate.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Business rules live in Rust
(`oikonomia-core`); the React UI only renders.

## License

Oikonomia is free software, released under the
[GNU General Public License v3.0 or later](LICENSE).
Copyright (c) 2026 Ourovoros.io.

Bundled fonts are under the SIL Open Font License; see `web/public/fonts` and
`web/src/assets/fonts`.

The bundled OCR models are the unmodified ocrs models by Robert Knight, which
the author's model card declares as CC BY-SA 4.0; see `apps/desktop/src-tauri/resources/ocr/README.md`.

## Layout

| Path | Role |
|------|------|
| `crates/oikonomia-core` | Domain, vault, ledger, reports |
| `crates/oikonomia-update` | Signed update check (network isolated from core) |
| `crates/macos-dock-icon` | Sets the macOS Dock icon in dev mode (`make app`) |
| `crates/oikonomia-test-support` | Test-only macros shared by the crates |
| `apps/desktop/src-tauri` | Tauri shell + IPC |
| `web` | React UI |
| `docs` | Design overview (`DESIGN.md`), release runbook, brand assets |

## Not yet supported

Budgets, invoicing, multi-currency, recovery key, a code-signed Windows
installer, Linux on ARM, App Sandbox.
