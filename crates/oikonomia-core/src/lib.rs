//! The domain library of Oikonomia, a local-only double-entry finance app.
//!
//! Everything the app knows about bookkeeping lives in this crate: the types,
//! the rules, the encrypted storage and the queries. The desktop shell
//! (`apps/desktop/src-tauri`) is a thin layer that turns IPC commands into
//! calls on this crate, and the web UI holds no accounting rule at all.
//!
//! # Module map
//!
//! Types and rules, which touch neither a file nor the database:
//!
//! - [`money`]: [`Money`], a non-negative amount in integer minor units.
//! - [`domain`]: entities (books), accounts, journal entries and lines, and
//!   the rule a set of lines must satisfy to be posted.
//! - [`coa`]: the starter charts of accounts and which seeded account plays
//!   which part by default.
//! - [`error`]: [`Error`], the one error type, and the codes the UI words.
//! - [`text`]: wording that core writes into a book (seeded account names,
//!   generated descriptions), in every supported language.
//! - [`ui_text`]: coded notes that core hands to the UI to word.
//! - [`util`]: `YYYY-MM-DD` dates, ids and timestamps as stored text.
//!
//! Storage and queries:
//!
//! - [`vault`]: the encrypted database file, its header, unlocking, password
//!   change, and backup and restore.
//! - [`db`]: the schema, its migrations, and helpers for reading rows.
//! - [`ledger`]: entities, accounts, postings, voids, recurring templates and
//!   reports, as functions over an open database connection.
//! - [`default_accounts`]: the default account for each part of an entry, for
//!   a given book.
//! - [`csv`]: bank statement import and journal export.
//! - [`documents`]: stored bills and receipts, and reading them with the
//!   bundled OCR.
//! - [`prefs`]: the few non-secret preferences kept outside the vault.
//!
//! # Invariants
//!
//! These hold across the crate; a change that breaks one is a bug.
//!
//! - **Money is integer minor units.** Amounts are `i64` counts of the
//!   currency's smallest unit and are never held in floating point. Sums are
//!   checked: an amount that does not fit is [`Error::MoneyOverflow`], never
//!   a wrapped or clamped figure.
//! - **A posted entry balances.** It has at least two lines, each line has an
//!   amount on exactly one side, and total debits equal total credits
//!   ([`domain::validate_lines_for_post`]).
//! - **Business rules live here.** Which account is debited and which
//!   credited, which account types a part of an entry accepts, and what
//!   counts as a duplicate are decided in Rust. The UI sends a request and
//!   shows the answer.
//! - **Core is offline.** No HTTP, TLS or socket crate is in this crate's
//!   dependency tree; `scripts/assert-core-offline.sh` fails the build when
//!   one of the crates it lists appears. The only network path in the app is
//!   the update check in `oikonomia-update`.
//! - **The vault is encrypted at rest.** Books are stored in the `SQLCipher`
//!   database in [`vault`], and a backup is a copy of that encrypted
//!   database with its header. The files this crate writes in
//!   the clear hold no ledger data: the vault header (key-derivation
//!   parameters and a salt) and the [`prefs`] file (a language choice and the
//!   ids of the last-used book and accounts). [`csv::export_journal_csv`]
//!   returns a book as plaintext to its caller, on the user's request.
//! - **Core writes no sentence for the UI.** A failure or a note leaves the
//!   crate as a stable code with named values ([`Error::code`],
//!   [`ui_text::UiText`]) that the UI words in the user's language. The
//!   exception is text stored in the book itself, which [`text`] supplies.
//! - **No `unsafe`.** The crate forbids it.

#![forbid(unsafe_code)]

pub mod coa;
pub mod csv;
pub mod db;
pub mod default_accounts;
pub mod documents;
pub mod domain;
pub mod error;
pub mod ledger;
pub mod money;
pub mod prefs;
pub mod text;
pub mod ui_text;
pub mod util;
pub mod vault;

pub use error::{Error, Result};
pub use money::Money;
pub use vault::{Vault, VaultStatus};
