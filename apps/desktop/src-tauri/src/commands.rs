//! The IPC layer: the commands the webview can invoke.
//!
//! Three commands live with their subject, outside this module:
//! `update_check` and `update_install` in `crate::update`, and
//! `donation_addresses` in `crate::donations`. Every other command is here.
//!
//! A command is a thin wrapper. It moves its arguments onto the blocking pool,
//! calls one function of `oikonomia-core`, or a few, and converts the result.
//! The rules of the ledger live in core, not here.
//!
//! The commands are grouped by subject, one module each. This module
//! re-exports them, so the handler list in `lib.rs` names every command as
//! `commands::<name>` wherever it is defined. The command names, argument
//! names, return shapes and error codes are a contract with the frontend
//! (`web/src/lib/tauri.ts`); changing one changes that contract.
//!
//! # Rules every command follows
//!
//! **Blocking work runs on the blocking pool.** Waiting for the vault mutex
//! (a password change holds it for seconds), a native dialog and file I/O
//! never run on an async worker, where they would stall every other command
//! scheduled on it, nor on the main thread, which runs the event loop
//! ([`support::run_blocking`]). Tauri runs a command declared as a plain `fn`
//! on the main thread, so a synchronous command must not wait for the vault.
//! [`vault_touch`], [`app_info`], [`open_support_email`],
//! [`open_main_window`] and [`quick_add_hide`] neither block nor take a lock.
//!
//! **The vault is locked in one place.** A command reaches the vault through
//! [`support::with_vault_blocking`] or one of the two helpers built for the
//! common cases, [`support::with_connection`] and
//! [`support::with_localized_connection`]. Each takes the vault with
//! [`GatedVault::acquire`](crate::state::GatedVault::acquire), whose guard
//! keeps the idle watchdog in step with the vault's status, so no command
//! updates the watchdog itself. Each also records the call as activity
//! ([`AppState::touch`](crate::state::AppState::touch)); [`vault_status`]
//! deliberately does not.
//!
//! **Errors cross IPC as a code plus parameters.** Every fallible command
//! returns [`CommandResult`](crate::error::CommandResult). A core error
//! converts with `?`; a failure only the shell can produce is built with
//! [`CommandError::desktop`](crate::error::CommandError::desktop). The UI words
//! the error from its code and never shows the English message.
//!
//! **A path from the webview is accepted only if the user handed it over for
//! that use.** A command that takes a path checks it against the paths
//! recorded from native drops and native dialogs
//! ([`support::require_granted_path`]) and opens the resolved path that check
//! returns, not the text it was given. Each grant carries a purpose
//! ([`GrantPurpose`](crate::state::GrantPurpose)) and each command asks for
//! its own, so a dropped document or a picked statement is never accepted as
//! the backup to restore.
//!
//! **Ledger text is written in the stored language.** A command whose core
//! call writes text into the books reads the language from the preferences
//! file ([`support::stored_text_locale`]); the webview cannot choose it.
//!
//! # Common vault errors
//!
//! A command that needs the unlocked vault can return, besides the codes its
//! own `# Errors` section names:
//!
//! - `vault_locked` when the vault is locked or does not exist yet;
//! - `database` when the database cannot be read or written;
//! - `vault_corrupt` when a stored value cannot be parsed;
//! - `task_failed` when its blocking task panics.
//!
//! A command that takes the Tauri state fails before it runs when the state
//! was never set up, which is the case after a failed start
//! (`crate::startup`). Tauri reports that as plain text, which the frontend
//! shows under the code `unknown`.

mod accounts;
mod app;
mod csv;
mod documents;
mod entities;
mod frontend_log;
mod journal;
#[cfg(test)]
#[cfg(not(windows))]
mod path_grant_tests;
mod recurring;
mod reports;
mod settings;
mod support;
mod vault;

// A command is a function plus a hidden macro that `generate_handler!`
// looks up beside it; a glob carries both, a named re-export does not.
pub(crate) use crate::commands::accounts::*;
pub(crate) use crate::commands::app::*;
pub(crate) use crate::commands::csv::*;
pub(crate) use crate::commands::documents::*;
pub(crate) use crate::commands::entities::*;
pub(crate) use crate::commands::frontend_log::*;
pub(crate) use crate::commands::journal::*;
pub(crate) use crate::commands::recurring::*;
pub(crate) use crate::commands::reports::*;
pub(crate) use crate::commands::settings::*;
pub(crate) use crate::commands::vault::*;
