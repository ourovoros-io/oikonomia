//! Schema migrations and shared DB helpers.

mod fold;
mod row;
mod schema;

pub use fold::{fold_case, register_fold};
pub(crate) use row::{collect_rows, corrupt_column, read_column, stored_date, stored_uuid};
pub use schema::{CURRENT_SCHEMA_VERSION, migrate};
