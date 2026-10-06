//! Conversions from a lower-level failure to [`Error`] at the call site.
//!
//! A call that can fail below this crate names what core was doing and picks
//! the variant by what failed:
//!
//! ```ignore
//! conn.execute(sql, params).database("insert journal entry")?;
//! ```
//!
//! [`DatabaseContext`] is implemented for `rusqlite` results only, so the
//! variant follows from the type of the failure and not from a choice at the
//! call site.
//!
//! The operation is a lowercase phrase with no trailing period that reads
//! after "cannot", such as `"open vault database"`.

use crate::error::{Error, Result};

/// Turns a `rusqlite` failure into [`Error::Database`].
pub(crate) trait DatabaseContext<T> {
    /// Reports the failure as a database failure during `operation`.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] carrying `operation` and the driver's text.
    fn database(self, operation: &'static str) -> Result<T>;
}

impl<T> DatabaseContext<T> for rusqlite::Result<T> {
    fn database(self, operation: &'static str) -> Result<T> {
        self.map_err(|err| Error::database(operation, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_database_failure_keeps_the_operation_and_the_driver_text() {
        let failed: rusqlite::Result<()> = Err(rusqlite::Error::QueryReturnedNoRows);

        let err = failed.database("read schema version").expect_err("failed");

        assert_eq!(err.code(), "database");
        assert_eq!(
            err.to_string(),
            "read schema version: Query returned no rows"
        );
    }
}
