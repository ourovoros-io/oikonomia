//! Tauri command handlers (thin wrappers over core + state).

mod accounts;
mod app;
mod csv;
mod documents;
mod entities;
mod journal;
mod recurring;
mod reports;
mod settings;
mod support;
mod vault;

// A command is a function plus a hidden macro that `generate_handler!`
// looks up beside it; a glob carries both, a named re-export does not.
pub(crate) use self::accounts::*;
pub(crate) use self::app::*;
pub(crate) use self::csv::*;
pub(crate) use self::documents::*;
pub(crate) use self::entities::*;
pub(crate) use self::journal::*;
pub(crate) use self::recurring::*;
pub(crate) use self::reports::*;
pub(crate) use self::settings::*;
pub(crate) use self::vault::*;
