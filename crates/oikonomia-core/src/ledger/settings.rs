//! Application settings kept inside the encrypted vault.
//!
//! A setting is one row of the `app_settings` table: a key and a value, both
//! text. A setting kept here instead of in the preferences file is encrypted
//! with the books and cannot be read or changed without the master password.
//! The idle lock timeout is the only one so far.
//!
//! The functions here read and write a single row, so none of them opens a
//! transaction.

use crate::error::{DatabaseContext, Error, Result, ValidationError};
use rusqlite::{Connection, OptionalExtension};

/// The idle lock timeout, in seconds, of a vault that has stored none: 15
/// minutes.
pub const DEFAULT_LOCK_TIMEOUT_SECS: u64 = 15 * 60;

/// Shortest idle lock timeout the vault accepts, in seconds.
pub(super) const MIN_LOCK_TIMEOUT_SECS: u64 = 60;

/// Key of the idle lock timeout in `app_settings`. Its value is the number of
/// seconds, written in decimal.
const KEY_LOCK_TIMEOUT: &str = "lock_timeout_secs";

/// Reads the idle lock timeout in seconds, or
/// [`DEFAULT_LOCK_TIMEOUT_SECS`] when none has been stored.
///
/// A stored value below the minimum (60 seconds, `MIN_LOCK_TIMEOUT_SECS`) is
/// returned as that minimum.
/// [`set_lock_timeout_secs`] is the only writer and refuses such a value, so
/// one can only come from a damaged or hand-edited vault. The desktop watchdog
/// locks as soon as the idle time reaches what this returns, and a stored `0`
/// would lock the vault again the moment it is unlocked, leaving no way to
/// reach the setting and repair it.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] when the stored value is not a whole,
///   non-negative number of seconds.
/// - [`Error::Database`] on database errors.
pub fn get_lock_timeout_secs(conn: &Connection) -> Result<u64> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [KEY_LOCK_TIMEOUT],
            |row| row.get(0),
        )
        .optional()
        .database("read lock timeout")?;

    let Some(stored) = stored else {
        return Ok(DEFAULT_LOCK_TIMEOUT_SECS);
    };
    let seconds: u64 = stored
        .parse()
        .map_err(|_| Error::VaultCorrupt("invalid lock_timeout_secs".into()))?;

    Ok(seconds.max(MIN_LOCK_TIMEOUT_SECS))
}

/// Stores the idle lock timeout, in seconds, replacing any earlier value.
///
/// # Errors
///
/// - [`ValidationError::LockTimeoutTooShort`] when `secs` is below the
///   minimum (60, `MIN_LOCK_TIMEOUT_SECS`).
/// - [`Error::Database`] on database errors.
pub fn set_lock_timeout_secs(conn: &Connection, secs: u64) -> Result<()> {
    if secs < MIN_LOCK_TIMEOUT_SECS {
        return Err(Error::Validation(ValidationError::LockTimeoutTooShort {
            min_secs: MIN_LOCK_TIMEOUT_SECS,
        }));
    }

    conn.execute(
        "
        INSERT INTO app_settings (key, value) VALUES (?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        ",
        rusqlite::params![KEY_LOCK_TIMEOUT, secs.to_string()],
    )
    .database("write lock timeout")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;
    use tempfile::TempDir;

    const PASSWORD: &str = "correct horse battery staple";

    #[test]
    fn set_lock_timeout_rejects_below_sixty_seconds() {
        let conn = Connection::open_in_memory().expect("memory");
        let err = set_lock_timeout_secs(&conn, 59);
        assert_eq!(
            err,
            Err(Error::Validation(ValidationError::LockTimeoutTooShort {
                min_secs: MIN_LOCK_TIMEOUT_SECS,
            }))
        );
    }

    /// A settings table holding `stored` as the timeout, written past the
    /// check in [`set_lock_timeout_secs`] as a hand-edited vault would be.
    fn settings_with_timeout(stored: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("memory");
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);",
        )
        .expect("create table");
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
            [KEY_LOCK_TIMEOUT, stored],
        )
        .expect("insert");
        conn
    }

    #[test]
    fn a_stored_timeout_below_the_minimum_reads_as_the_minimum() {
        for stored in ["0", "1", "59"] {
            let conn = settings_with_timeout(stored);

            assert_eq!(
                get_lock_timeout_secs(&conn),
                Ok(MIN_LOCK_TIMEOUT_SECS),
                "stored {stored}"
            );
        }
    }

    #[test]
    fn a_stored_timeout_at_or_above_the_minimum_reads_as_stored() {
        assert_eq!(get_lock_timeout_secs(&settings_with_timeout("60")), Ok(60));
        assert_eq!(
            get_lock_timeout_secs(&settings_with_timeout("86400")),
            Ok(86_400)
        );
    }

    #[test]
    fn a_stored_timeout_that_is_not_a_number_is_a_corrupt_vault() {
        for stored in ["", "-1", "soon", "1.5"] {
            let read = get_lock_timeout_secs(&settings_with_timeout(stored));

            assert!(
                matches!(read, Err(Error::VaultCorrupt(_))),
                "stored {stored:?}: {read:?}"
            );
        }
    }

    #[test]
    fn set_lock_timeout_persists_after_lock_and_unlock() {
        let dir = TempDir::new().expect("tempdir");
        let mut vault = Vault::open_path(dir.path()).expect("open");
        vault.init(PASSWORD).expect("init");

        set_lock_timeout_secs(vault.connection().expect("conn"), 120).expect("set");
        assert_eq!(
            get_lock_timeout_secs(vault.connection().expect("conn")).expect("get"),
            120
        );

        vault.lock();
        vault.unlock(PASSWORD).expect("unlock");
        assert_eq!(
            get_lock_timeout_secs(vault.connection().expect("conn")).expect("get after unlock"),
            120
        );
    }
}
