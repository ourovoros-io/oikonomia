//! Reading stored rows back into domain values.
//!
//! A value that the application wrote and can no longer parse means the vault
//! is damaged, not that the caller passed bad input. Row mappers therefore
//! return the crate's [`Result`] and report such a value with
//! [`corrupt_column`], so it reaches the UI as `vault_corrupt`. Passing the
//! error through `rusqlite::Error` instead would flatten it to text that the
//! query's caller can only report as [`Error::Database`].
//!
//! A mapper is used from a rusqlite row closure as `|row| Ok(map_thing(row))`:
//! the outer `rusqlite::Result` carries driver failures and the inner one
//! carries the mapper's verdict on the row. [`collect_rows`] unwraps both for
//! a query that returns many rows.
//!
//! Damage shows up at two levels, and both are `vault_corrupt`:
//!
//! - the value has the wrong storage class or range for its Rust type (text
//!   where an amount belongs). [`read_column`] reports it under the column's
//!   name in the query, which has no table prefix;
//! - the value reads, but does not parse (text that is not a date). The
//!   mapper reports it with [`corrupt_column`] under `table.column`.

use crate::error::{DatabaseContext, Error, Result, VaultCorruption};
use crate::util::{parse_date, parse_uuid};
use rusqlite::Row;
use rusqlite::types::FromSql;
use std::fmt::Display;
use time::Date;
use uuid::Uuid;

/// The error for a stored value that is not what the application writes there.
///
/// `column` says where the value was read from: `table.column` when a mapper
/// calls this, the column's name in the query when [`read_column`] does.
/// `detail` says what is wrong with the value.
pub(crate) fn corrupt_column(column: &str, detail: impl Display) -> Error {
    Error::VaultCorrupt(VaultCorruption::Column {
        column: column.to_owned(),
        detail: detail.to_string(),
    })
}

/// Reads column `index` of `row` as `T`.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] when the stored value cannot be a `T`: it has
///   another storage class, or is out of `T`'s range.
/// - [`Error::Database`] for any other driver failure, such as an `index` the query
///   does not select.
pub(crate) fn read_column<T: FromSql>(row: &Row<'_>, index: usize) -> Result<T> {
    row.get(index).map_err(|err| match err {
        rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::IntegralValueOutOfRange(..)
        | rusqlite::Error::FromSqlConversionFailure(..) => {
            // These three are raised for a column the query does select, so
            // the name lookup fails only if rusqlite changes that; the index
            // then stands in for the name.
            let statement: &rusqlite::Statement<'_> = row.as_ref();
            let column = statement
                .column_name(index)
                .map_or_else(|_| format!("column {index}"), str::to_owned);
            corrupt_column(&column, &err)
        }
        other => Error::database("read stored column", other),
    })
}

/// Collects the rows of a query whose row closure is `|row| Ok(mapper(row))`.
///
/// Stops at the first row that fails, so a damaged row fails the whole query
/// instead of being left out of the result.
///
/// # Errors
///
/// - [`Error::Database`] when the driver fails to step to a row.
/// - The mapper's own error for the first row it refuses.
pub(crate) fn collect_rows<T>(
    rows: impl Iterator<Item = rusqlite::Result<Result<T>>>,
) -> Result<Vec<T>> {
    rows.map(|row| row.database("read query rows")?).collect()
}

/// Parses an id stored as text in `column`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `column` when `text` is not a UUID.
pub(crate) fn stored_uuid(column: &str, text: &str) -> Result<Uuid> {
    parse_uuid(text).map_err(|_| corrupt_column(column, format_args!("not an id: {text}")))
}

/// Parses a `YYYY-MM-DD` date stored as text in `column`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `column` when `text` is not a date.
pub(crate) fn stored_date(column: &str, text: &str) -> Result<Date> {
    parse_date(text).map_err(|_| corrupt_column(column, format_args!("not a date: {text}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_value_is_reported_as_a_corrupt_vault_naming_the_column() {
        assert_eq!(
            stored_uuid("accounts.id", "nope"),
            Err(corrupt_column("accounts.id", "not an id: nope"))
        );
        assert_eq!(
            stored_date("journal_entries.entry_date", "2026-13-01"),
            Err(corrupt_column(
                "journal_entries.entry_date",
                "not a date: 2026-13-01"
            ))
        );
        assert_eq!(
            corrupt_column("a.b", "bad"),
            Error::VaultCorrupt(VaultCorruption::Column {
                column: "a.b".into(),
                detail: "bad".into(),
            })
        );
        assert_eq!(corrupt_column("a.b", "bad").code(), "vault_corrupt");
    }

    #[test]
    fn a_sound_value_parses() {
        let id = "22222222-2222-4222-8222-222222222222";
        assert_eq!(
            stored_uuid("accounts.id", id).map(|uuid| uuid.to_string()),
            Ok(id.to_owned())
        );
        assert_eq!(
            stored_date("journal_entries.entry_date", "2026-08-10").map(crate::util::format_date),
            Ok("2026-08-10".to_owned())
        );
    }
}
