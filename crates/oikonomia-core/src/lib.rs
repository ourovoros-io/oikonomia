//! Oikonomia core: double-entry domain types, vault crypto, ledger, and reports.
//!
//! Business rules live here so the Tauri shell stays a thin IPC boundary.

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
#[cfg(test)]
mod test_macros;
pub mod text;
pub mod ui_text;
pub mod util;
pub mod vault;

pub use error::{Error, Result};
pub use money::Money;
pub use vault::{Vault, VaultStatus};
