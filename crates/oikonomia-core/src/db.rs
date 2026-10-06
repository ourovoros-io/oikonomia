//! Database plumbing shared by everything that reads or writes a vault.
//!
//! The queries themselves live with the code that owns the data (the ledger,
//! the documents, the CSV import and export). This module holds what they
//! have in common, in three private modules:
//!
//! - `schema`: the tables of a vault and the migrations that bring an older
//!   vault to [`CURRENT_SCHEMA_VERSION`]. [`migrate`] runs when a vault is
//!   created and again on every unlock.
//! - `fold`: the `fold(text)` SQL function and [`fold_case`], which folds the
//!   same way in Rust, for comparisons that ignore case in every script.
//!   The vault calls [`register_fold`] on each connection it opens.
//! - `row`: helpers for reading a stored row back into a domain value. A
//!   stored value the application could not have written is reported as a
//!   corrupt vault, never as bad input from the caller.

mod fold;
mod row;
mod schema;

pub use fold::{fold_case, register_fold};
pub(crate) use row::{collect_rows, corrupt_column, read_column, stored_date, stored_uuid};
pub use schema::{CURRENT_SCHEMA_VERSION, migrate};
