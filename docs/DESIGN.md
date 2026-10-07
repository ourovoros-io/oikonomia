# Design overview

A map of how Oikonomia is built, for someone about to change it. For build
commands and the invariants a change must not break, read
[`AGENTS.md`](../AGENTS.md); for the threat model, [`SECURITY.md`](../SECURITY.md).

## What it is

Oikonomia is a local-only desktop app for personal and company finance. It keeps
double-entry books for several entities in one encrypted vault on the user's
machine. It has no account system and no telemetry. The only network use is a
signed update check that the user starts by clicking. Outside the vault it
writes a small preferences file and a local error log, neither of which holds
ledger data ([The error log](#the-error-log)). It is a Tauri 2 shell around a
Rust core, with a React interface in a webview.

## Layout

| Path | Owns |
|------|------|
| `crates/oikonomia-core` | Vault, schema and migrations, ledger, reports, CSV, documents and OCR, UI preferences. No network crates (`scripts/assert-core-offline.sh`). |
| `crates/oikonomia-update` | The update client: signed feed, host allow-list, verified download. The only crate that talks to the network. |
| `crates/macos-dock-icon` | Sets the Dock icon for dev mode (`make app`). |
| `apps/desktop/src-tauri` | The Tauri shell: IPC commands, app state, tray and quick-add window, idle auto-lock, capabilities, bundle config, OCR models. |
| `web` | The React, Vite and Tailwind interface. |
| `docs` | This overview, the release runbook, brand assets. |

## Business logic lives in Rust

The interface renders and collects input; it does not decide anything about the
books. Validation, posting, voiding, balances, reports, CSV parsing, document
analysis and the lock timeout are in `oikonomia-core` or the Rust shell. The
simple entry form sends a kind (such as expense, income or transfer) and
accounts, and `post_simple_entry` maps that to debit and credit lines. The
webview reaches the database, the key and the file system only through the
commands below. The exception is presentation: the expense PDF is laid out in
TypeScript from report lines Rust returns (`web/src/lib/expensePdf.ts`).

## The vault

The vault is one SQLCipher database, `vault.db`, plus a small public header,
`vault.header.json`, in the platform data directory.

- **Key derivation.** The master password (at least 12 characters) goes through
  Argon2id with a random 16-byte salt to produce a 32-byte key, which is handed
  to SQLCipher as a raw key. The header records the KDF name, salt, cost
  parameters and key length; it holds nothing secret. Parameters outside fixed
  bounds are rejected as corrupt (`vault/crypto.rs`, `vault/header.rs`).
- **At rest.** Neither the password nor the key is written to disk. The derived
  key is a zeroizing buffer dropped once the database is open. SQLCipher memory
  security is on, and WAL mode is used (SQLCipher encrypts it too). On Unix the
  data directory is `0700` and vault files `0600`.
- **Lock and unlock.** Locking closes the connection; unlocking derives the key,
  opens the database and runs migrations. Idle auto-lock (default 15 minutes,
  stored in the vault) is enforced by a Rust watchdog thread, not the webview.
- **Changing the password** rekeys the database. The new header is staged first
  and unlock falls back to it, so a crash mid-change leaves exactly one of the
  two passwords working.
- **Backup.** A `.oikonomia-backup` file is the 8-byte magic `OIKOBACK`, a
  little-endian `u16` format version, then two length-prefixed members:
  `vault.header.json` and `vault.db`, stored as on disk: ciphertext, with no
  second layer or password. An open vault is snapshotted with `VACUUM INTO`
  (`vault/backup.rs`).
- **UI preferences** (language, last book and accounts used in quick-add) are
  plain JSON in `ui-prefs.json` beside the vault, so the tray menu and unlock
  screen can use the right language before a password is entered. Nothing
  sensitive goes there (`prefs.rs`).

## The ledger

- **Money** is `i64` minor units, never floating point. Line amounts are
  non-negative (`money.rs`). Each entity has one base currency.
- **Entities** are separate books with a name, currency, fiscal-year start month
  and a chart template (personal, company or blank).
- **Accounts** belong to one entity and have a code, name and type: asset,
  liability, equity, income or expense. Templates seed the chart (`coa.rs`);
  system accounts such as Opening Balances cannot be deactivated.
- **Journal entries** have a date, description, optional reference and two or
  more lines. A line is a debit or a credit on one account, never both, never
  neither. Posting requires total debits to equal total credits
  (`domain/journal.rs`) inside one SQLite transaction; the schema also enforces
  debit XOR credit per line.
- **Corrections.** Posted entries are not edited in place. Voiding posts a
  reverse entry and links the two so the pair drops out of the books. Editing is
  `replace_simple_entry`: void the original, post the replacement and move any
  attached documents, in one transaction. Hidden entries are left out of the
  journal CSV export and the expense PDF; in-app lists and reports still show
  them, and the vault backup still contains them.
- **Reports** (trial balance, profit and loss, balance sheet, dashboard
  summary, cash-flow series) are computed in SQL over posted entries only
  (`crates/oikonomia-core/src/ledger`).
- **Recurring templates** show what is due; the user posts each occurrence.
- **Schema version** is stored in `vault_meta.schema_version`. `db::migrate`
  runs on vault creation and on every unlock and applies each step above the
  stored version in order up to `CURRENT_SCHEMA_VERSION` (`db/schema.rs`).
  Migration tests are in `crates/oikonomia-core/tests/migration_v*.rs`.

## Documents and OCR

Receipts and invoices are attached to journal entries (a document cannot exist
without one). Files are capped at 8 MiB and stored as blobs inside the encrypted
database, so backups include them.
Analysis runs offline: PDFs and text have their text extracted; a PDF with no
usable text layer has its embedded JPEG images run through OCR as well. Images go
through the `ocrs` engine using two `.rten` models that ship in the app bundle
(`apps/desktop/src-tauri/resources/ocr`), and an invoice reader pulls totals and
kind from the text. The result only pre-fills the entry form
(`crates/oikonomia-core/src/documents`).

## The IPC boundary

The webview talks to Rust only through Tauri commands. The app's own commands
are registered in one list in `apps/desktop/src-tauri/src/lib.rs`. Beyond
those, the webview can call only the Tauri built-ins that
`capabilities/default.json` grants.

- **Path grants.** A command that takes a file path accepts it only if the user
  handed it over through a native drop or a native dialog. Those paths are
  recorded in `AppState`, each with the purpose it was handed over for (a
  backup to restore, a statement to import, a dropped document), and checked by
  `require_granted_path`, which asks for the command's own purpose. The webview
  cannot name an arbitrary file, nor pass a file picked for one thing to a
  command that does another.
- **Capabilities.** `capabilities/default.json` gives the `main` and `quick-add`
  windows core defaults and a short list of window and event permissions. No
  file-system, shell or opener permission reaches the webview; the opener is used
  from Rust only, for the support mail link.
- **Content security.** The CSP in `tauri.conf.json` allows only the app's own
  origin plus Tauri IPC for `connect-src`. A navigation guard
  (`nav_guard.rs`) keeps every window on the app's own origin.

## Updates and the network

`oikonomia-core` has no network dependencies, and `deny.toml` bans the HTTP, TLS,
socket and websocket crates it lists everywhere except under `oikonomia-update`,
so the network is confined to the update path.

When the user clicks to check, `oikonomia-update` fetches `latest.json` and its
detached signature from the project's GitHub releases, verifies the signature
with a minisign public key compiled into the app, and compares versions. Every
URL, redirects included, must be HTTPS on an allow-listed GitHub host
(`hosts.rs`). The webview cannot supply a feed URL or key. Installing downloads
the artifact, checks its hash and minisign signature against the signed
manifest, and installs it the way the running copy was installed
(`update_exec.rs`): a macOS bundle and a Linux AppImage are replaced in place
and restarted; on Windows the new installer runs unattended while the app
exits, then starts the new version. A copy the system package manager owns
(a `.deb`) is only told that a newer version exists; the update machine never
keeps an installable offer for it.
Nothing checks for updates at startup. GitHub is only a host; the signing key is
the trust root. See [`release.md`](release.md) for how releases are cut.

## The error log

A release build keeps one local log, so that a failed start or a failed update
install leaves a record on the machine it happened on. It is never sent
anywhere; the update path is still the only code that opens a socket.

- **Where.** `errors.log` in the directory Tauri names for the app's logs:
  `~/Library/Logs/io.ourovoros.oikonomia` on macOS,
  `$XDG_DATA_HOME/io.ourovoros.oikonomia/logs` (by default under
  `~/.local/share`) on Linux, `%LOCALAPPDATA%\io.ourovoros.oikonomia\logs` on
  Windows. This is not the vault's data directory.
- **How much.** The file is capped at 1 MiB. At the cap it is renamed to
  `errors.previous.log`, replacing the file of that name, so at most two files
  and about 2 MiB exist. On Unix the directory is `0700` and the files `0600`,
  like the vault's.
- **What passes.** Warnings and errors from this workspace's crates only. Lower
  levels and every record from a third-party crate are dropped
  (`error_log.rs`, `is_logged`).
- **What a line may hold.** Fixed phrases, numbers, operating-system error
  text, and paths of the app's own files. Nothing from the ledger: no amount,
  description, merchant, account or entity name, document name or contents,
  password or key material, and no path the user chose. A log line is
  formatted where the failure happens, so the rule is kept there: a core error
  is logged through `Error::log_text`, which writes its code and operation and
  leaves out the lower-level detail, and a path that may be the user's goes
  through `PrivateDetail` (`crates/oikonomia-core/src/error/log_text.rs`). A
  new `log::` call follows the same rule.
- **The webview has no path to it.** The release logger is not a Tauri plugin
  and registers no command. `CommandError.message`, the full error text, goes
  to the webview console only and is never logged by Rust.
- **Debug builds** log as before: `tauri-plugin-log` at the info level, to
  standard output and `Oikonomia.log` in the same directory, with full error
  detail (`enable_log_detail`). The webview holds no `log:` permission, so it
  cannot write there either.

## Platforms

The same code ships on macOS, Linux and Windows. The differences are few and
each lives in one place:

- **Vault location and file modes.** `vault/paths.rs` uses the machine-local
  data directory (on Windows, local rather than roaming AppData, because a
  roaming profile can replace a live database with a stale copy).
  `vault/permissions.rs` sets owner-only modes on Unix; Windows relies on the
  per-user ACL of AppData.
- **One process.** Closing the window hides it. macOS routes a second launch
  to the running app; on Windows and Linux `tauri-plugin-single-instance`
  does, so a relaunch shows the hidden window instead of opening the vault
  twice. This is also the way back in on a Linux desktop that shows no tray.
- **Tray.** Linux trays report no clicks, so "Quick add" is a menu item there
  (`tray.rs`). The quick-add window is placed inside the monitor's work area.
- **Installers.** `.dmg`, `.AppImage`, `.deb`, and a per-user NSIS installer
  that embeds the WebView2 bootstrapper. Windows 11 includes the runtime and
  Windows 10 receives it through Windows Update; the bootstrapper downloads
  it only when it is missing.
  `scripts/smoke-linux.sh` and `scripts/smoke-windows.ps1` install and run
  the real installers in CI.

## The interface

`web/src` holds `App.tsx` (shell and sidebar), `pages/`, `components/` and
`lib/`; `lib/tauri.ts` and `lib/api.ts` wrap the commands.

- **Pages** are Dashboard, Transactions (with the Recurring view), Documents,
  Accounts, Reports and Settings, plus the unlock screen and the separate
  quick-add window (`QuickAddApp.tsx`). Each page renders `TopBar`.
- **i18n.** Four locales: English, Greek, French and German, in
  `web/src/locales`. `en.json` has flat keys; the others are nested.
  `lib/i18n.ts` flattens them and `I18nProvider` supplies `t()`. The chosen
  language is stored in `ui-prefs.json`. On the first launch (no `locale` key
  in that file) the webview reports `navigator.languages` to the
  `settings_resolve_locale` command; Rust maps them to a supported language
  (`Locale::from_system_languages`), stores it and returns it, and from then on
  the system language is never read again, so a stored English stays English. On macOS the webview reports only the single
  most preferred system language, so only that language is considered; if it is
  not supported the app starts in English and the language can be changed on the
  first screen.
  The unlock and create-vault screen shows the same language switch as
  Settings, so the first book is seeded in the language the user sees. Text that comes from Rust reaches the
  screen in one of three ways, and the UI never renders raw backend text:
  - *Transient text* travels as a code plus named parameters, and the UI words
    it. This covers command errors (`CommandError` in the desktop crate's
    `error.rs`, built from `ValidationError` and the other core and update
    errors), analyzer notes, CSV import row problems and the analyzer status hint
    (`UiTextCode` in `ui_text.rs`, `AnalyzerHint` in `documents/analyze.rs`), and the computed
    report rows (`SyntheticLine` in `ledger/reports.rs`). `lib/commandError.ts`
    and `lib/uiText.ts` map each code to a catalog key. The lists of codes are
    shared JSON files in `web/src/lib` (`errorCodes.json`,
    `errorCodeParams.json`, `accountRoles.json`, `uiTextCodes.json`). Rust
    tests check each file against the enumerations, and `commandError.test.ts`
    and `uiText.test.ts` check the UI side, so a code added on one side fails a
    test until the other has it. `noRawErrorMessage.test.ts` fails when a
    screen reads an error's raw message, and `i18n.catalog.test.ts` checks that
    every key has a reader, exists in all four languages and has the same
    placeholders in each.
  - *Stored text* is written by the Rust core in the app's language when it is
    created, read from the stored preference (never from the webview): the account names a chart template seeds, the descriptions
    core generates for opening balances and voids, and the descriptions and
    merchants the document reader suggests. The wording is a table in
    `crates/oikonomia-core/src/text.rs`, keyed by language. It is not renamed
    when the language changes later.
  - *Default accounts* for the entry forms are chosen in Rust by template code
    and account type (`default_accounts.rs`), never by name, because names are
    free text.
- **Design language.** The interface is dark only and is called "Aurora glass":
  an animated colored backdrop (`components/Aurora.tsx`) under translucent panes
  with a hairline edge. Brand color marks chrome and never money; money in and
  out have their own pair of colors. The design tokens are the `@theme` block
  and the glass and aurora variables in `web/src/index.css`; fonts are bundled
  (`web/src/fonts.css`, `web/public/fonts`; Inter for the expense PDF is in
  `web/src/assets/fonts`). `web/tests/tokens.test.ts` checks
  text contrast against the glass surface.

## Decided, not changed

Three questions that were raised, weighed and closed. Each is recorded here
with its reason so that it is not raised again without something new to say.

- **An archived entity is fully read-only, voids and corrections included.**
  Posting, voiding and correcting an entry, setting an opening balance and
  posting from a template are all refused for an archived entity, and reads
  answer as before. To correct an entry, the book is un-archived first.
  Reason: archived should mean the figures cannot change, and a void or a
  correction changes them. The rule is `ensure_writable_entity` in
  `crates/oikonomia-core/src/ledger/entities.rs`, whose module documentation
  tabulates it; every entry is inserted through `post_entry_in_tx`
  (`ledger/journals.rs`), which makes the check first. No un-archive
  operation exists yet: until one does, an archived book can be read,
  exported and deleted, and its entries cannot be corrected.
- **PDF load-time decompression is bounded only by the 8 MiB upload cap.**
  `lopdf` inflates object streams and cross-reference streams while it loads a
  file, before the page, decoded-size and nesting budgets can run on the
  parsed document, and that step has no limit of its own. Bounding it would
  take a parser of our own in front of `lopdf`, which was judged a larger risk
  than the exposure: a slow or memory-hungry load of a hostile file that the
  user chose to open on their own machine. The cap is `MAX_DOCUMENT_BYTES`
  (`crates/oikonomia-core/src/documents/file.rs`); `load_pdf`
  (`documents/pdf_load.rs`) checks it before `lopdf` sees a byte and runs the
  budgets (`documents/pdf_budget.rs`, `documents/pdf_nesting.rs`) on what
  `lopdf` returns.
- **A recurring template's amount is validated, not typed.** The amount is
  checked to be positive when the template is saved (`check_template_values`
  in `crates/oikonomia-core/src/ledger/recurring.rs`, and a `CHECK` on the
  column in `db/schema.rs`) and again when it posts (`post_recurring_template`
  for an amount given at posting, and `post_simple_entry_in_tx` in
  `ledger/journals.rs` for every simple entry). A positive-amount type would
  say the same thing once, but it would touch every place money is
  constructed, for little gain over two checks that already exist.
