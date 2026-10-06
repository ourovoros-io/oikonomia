//! Reading stored rows back into domain values.
//!
//! A value that the application wrote and can no longer parse means the vault
//! is damaged, not that the caller passed bad input. Row mappers therefore
//! return the crate's [`Result`] and report such a value with
//! [`corrupt_column`], so it reaches the UI as `vault_corrupt`. Passing the
//! error through `rusqlite::Error` instead would flatten it to text that the
//! query's caller can only report as [`Error::Io`].
//!
//! A mapper is used from a rusqlite row closure as `|row| Ok(map_thing(row))`:
//! the outer `rusqlite::Result` carries driver failures and the inner one
//! carries the mapper's verdict on the row.

use std::fmt::Display;

use rusqlite::Row;
use rusqlite::types::FromSql;
use time::Date;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::util::{parse_date, parse_uuid};

/// The error for a stored value that is not what the application writes there.
///
/// `column` is the `table.column` the value was read from; `detail` says what
/// is wrong with it.
pub(crate) fn corrupt_column(column: &str, detail: impl Display) -> Error {
    Error::VaultCorrupt(format!("{column}: {detail}"))
}

/// Reads column `index` of `row` as `T`.
///
/// # Errors
///
/// [`Error::Io`] when the driver cannot produce a `T` from the column.
pub(crate) fn read_column<T: FromSql>(row: &Row<'_>, index: usize) -> Result<T> {
    row.get(index).map_err(|err| Error::Io(err.to_string()))
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
            Err(Error::VaultCorrupt("accounts.id: not an id: nope".into()))
        );
        assert_eq!(
            stored_date("journal_entries.entry_date", "2026-13-01"),
            Err(Error::VaultCorrupt(
                "journal_entries.entry_date: not a date: 2026-13-01".into()
            ))
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
