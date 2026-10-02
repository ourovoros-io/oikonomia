//! Schema migrations and shared DB helpers.

mod fold;
mod schema;

pub use fold::{fold_case, register_fold};
pub use schema::{CURRENT_SCHEMA_VERSION, migrate};
