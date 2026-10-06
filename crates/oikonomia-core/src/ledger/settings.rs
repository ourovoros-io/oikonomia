//! App settings stored inside the encrypted vault.

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::error::{Error, Result, ValidationError};

/// Default idle lock timeout: 15 minutes.
pub const DEFAULT_LOCK_TIMEOUT_SECS: u64 = 15 * 60;

/// Shortest idle lock timeout the vault accepts, in seconds.
pub const MIN_LOCK_TIMEOUT_SECS: u64 = 60;

const KEY_LOCK_TIMEOUT: &str = "lock_timeout_secs";

/// Reads the idle lock timeout in seconds, or
/// [`DEFAULT_LOCK_TIMEOUT_SECS`] when none has been stored.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] when the stored value is not a whole, non-negative
/// number of seconds; database errors as [`Error::Io`].
pub fn get_lock_timeout_secs(conn: &Connection) -> Result<u64> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [KEY_LOCK_TIMEOUT],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| Error::Io(err.to_string()))?;

    match value {
        Some(s) => s
            .parse()
            .map_err(|_| Error::VaultCorrupt("invalid lock_timeout_secs".into())),
        None => Ok(DEFAULT_LOCK_TIMEOUT_SECS),
    }
}

/// Persist lock timeout (minimum 60 seconds).
///
/// # Errors
///
/// Validation or DB errors.
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
    .map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
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
