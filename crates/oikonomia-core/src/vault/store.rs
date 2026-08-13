//! Vault open/create/lock against an on-disk `SQLCipher` database.

use std::fs;
use std::path::{Path, PathBuf};

use rand::RngCore;
use rusqlite::{Connection, OpenFlags};

use super::crypto::{self, VaultKey};
use super::header::{MIN_PASSWORD_LEN, SALT_LEN, VaultHeader};
use super::paths::{vault_db_path, vault_header_path, vault_staged_header_path};
use crate::error::{Error, Result};

/// Lifecycle status for the vault (serializable to the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultStatus {
    /// No vault files on disk yet.
    Uninitialized,
    /// Vault exists; password required.
    Locked,
    /// Open connection held in this process.
    Unlocked,
}

/// Encrypted vault handle. Dropping or [`Vault::lock`] closes the DB.
pub struct Vault {
    data_dir: PathBuf,
    header: Option<VaultHeader>,
    conn: Option<Connection>,
}

impl Vault {
    /// Inspect the data directory without opening the database.
    ///
    /// # Errors
    ///
    /// Returns I/O errors when the directory cannot be created or the header is unreadable.
    pub fn open_path(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir).map_err(|err| Error::Io(err.to_string()))?;

        let header_path = vault_header_path(&data_dir);
        let db_path = vault_db_path(&data_dir);

        let header = if header_path.exists() {
            let raw = fs::read_to_string(&header_path).map_err(|err| Error::Io(err.to_string()))?;
            let header: VaultHeader =
                serde_json::from_str(&raw).map_err(|err| Error::VaultCorrupt(err.to_string()))?;
            Some(header)
        } else if db_path.exists() {
            return Err(Error::VaultCorrupt(
                "database exists without vault header".into(),
            ));
        } else {
            None
        };

        Ok(Self {
            data_dir,
            header,
            conn: None,
        })
    }

    /// Current status relative to this process.
    #[must_use]
    pub fn status(&self) -> VaultStatus {
        if self.conn.is_some() {
            VaultStatus::Unlocked
        } else if self.header.is_some() {
            VaultStatus::Locked
        } else {
            VaultStatus::Uninitialized
        }
    }

    /// Data directory for this vault.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Create a new encrypted vault with the given master password.
    ///
    /// # Errors
    ///
    /// Weak password, already initialized, crypto, or I/O failures.
    pub fn init(&mut self, password: &str) -> Result<()> {
        if self.header.is_some() || vault_db_path(&self.data_dir).exists() {
            return Err(Error::Validation("vault is already initialized".into()));
        }

        validate_password(password)?;

        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);

        let header = VaultHeader::new_with_salt(&salt);
        let key = crypto::derive_key(password, &header)?;

        let header_path = vault_header_path(&self.data_dir);
        let header_json =
            serde_json::to_string_pretty(&header).map_err(|err| Error::Io(err.to_string()))?;
        let staged_header = header_path.with_extension("json.init");
        fs::write(&staged_header, &header_json).map_err(|err| Error::Io(err.to_string()))?;
        fs::rename(&staged_header, &header_path).map_err(|err| Error::Io(err.to_string()))?;

        let db_path = vault_db_path(&self.data_dir);
        let conn = match open_sqlcipher(&db_path, &key, true) {
            Ok(conn) => conn,
            Err(err) => {
                remove_vault_db_sidecars(&db_path);
                return Err(err);
            }
        };
        if let Err(err) = bootstrap_schema(&conn) {
            drop(conn);
            remove_vault_db_sidecars(&db_path);
            return Err(err);
        }

        self.header = Some(header);
        self.conn = Some(conn);
        Ok(())
    }

    /// Unlock an existing vault.
    ///
    /// # Errors
    ///
    /// Uninitialized vault, wrong password, or I/O/crypto failures.
    pub fn unlock(&mut self, password: &str) -> Result<()> {
        if self.conn.is_some() {
            return Ok(());
        }

        let header = self
            .header
            .as_ref()
            .ok_or(Error::VaultUninitialized)?
            .clone();
        let db_path = vault_db_path(&self.data_dir);

        match open_verified(&db_path, password, &header) {
            Ok(conn) => {
                // A stale staged header can only be leftover from an
                // interrupted password change that never rekeyed.
                let _ = fs::remove_file(vault_staged_header_path(&self.data_dir));

                crate::db::migrate(&conn)?;
                self.conn = Some(conn);
                Ok(())
            }
            Err(Error::InvalidPassword) => self.unlock_with_staged_header(password),
            Err(other) => Err(other),
        }
    }

    /// Recovery path for a password change that died between rekey and rename:
    /// the database is already under the staged header's key, so accept that
    /// header and promote it to be the real one.
    fn unlock_with_staged_header(&mut self, password: &str) -> Result<()> {
        let staged_path = vault_staged_header_path(&self.data_dir);
        let raw = fs::read_to_string(&staged_path).map_err(|_| Error::InvalidPassword)?;
        let staged: VaultHeader = serde_json::from_str(&raw).map_err(|_| Error::InvalidPassword)?;

        let db_path = vault_db_path(&self.data_dir);
        let conn =
            open_verified(&db_path, password, &staged).map_err(|_| Error::InvalidPassword)?;

        fs::rename(&staged_path, vault_header_path(&self.data_dir))
            .map_err(|err| Error::Io(err.to_string()))?;

        crate::db::migrate(&conn)?;
        self.header = Some(staged);
        self.conn = Some(conn);
        Ok(())
    }

    /// Close the database connection and forget process-local key material.
    pub fn lock(&mut self) {
        self.conn = None;
    }

    /// Re-encrypt the vault under a new master password (`SQLCipher` rekey).
    ///
    /// Crash-safety protocol: the new header is staged to a temp file before
    /// the rekey and renamed over the real header after it, and [`Vault::unlock`]
    /// falls back to the staged header when the real one no longer opens the
    /// database. Whatever step the process dies at, exactly one of the two
    /// passwords opens the vault. On success the vault is left unlocked under
    /// the new key.
    ///
    /// # Errors
    ///
    /// [`Error::VaultUninitialized`], [`Error::InvalidPassword`] for a wrong
    /// old password, [`Error::Validation`] for a weak new password, or
    /// crypto/I/O failures. A failed verification leaves any open connection
    /// untouched.
    pub fn change_password(&mut self, old: &str, new: &str) -> Result<()> {
        let header = self
            .header
            .as_ref()
            .ok_or(Error::VaultUninitialized)?
            .clone();
        validate_password(new)?;

        // Verify the old password first — a typo must not lock the vault.
        let old_key = crypto::derive_key(old, &header)?;
        let db_path = vault_db_path(&self.data_dir);
        let conn = open_sqlcipher(&db_path, &old_key, false)?;

        // Only now drop our own connection: its page cache would go stale
        // across the rekey below.
        let was_unlocked = self.conn.is_some();
        self.conn = None;

        let result = (|| -> Result<VaultHeader> {
            let mut salt = [0u8; SALT_LEN];
            rand::thread_rng().fill_bytes(&mut salt);
            let new_header = VaultHeader::new_with_salt(&salt);
            let new_key = crypto::derive_key(new, &new_header)?;

            let header_path = vault_header_path(&self.data_dir);
            let staged_path = vault_staged_header_path(&self.data_dir);
            let header_json = serde_json::to_string_pretty(&new_header)
                .map_err(|err| Error::Io(err.to_string()))?;
            write_synced(&staged_path, header_json.as_bytes())?;

            // Fold WAL pages into the main file so the rekey covers everything.
            let blocked: i64 = conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
                .map_err(|err| Error::Io(err.to_string()))?;
            if blocked != 0 {
                return Err(Error::Io(
                    "wal checkpoint blocked; will not rekey".into(),
                ));
            }
            let pragma_key = crypto::key_to_sqlcipher_pragma(&new_key);
            conn.pragma_update(None, "rekey", pragma_key.as_str())
                .map_err(|err| Error::Crypto(err.to_string()))?;
            drop(conn);

            fs::rename(&staged_path, &header_path).map_err(|err| Error::Io(err.to_string()))?;
            Ok(new_header)
        })();

        match result {
            Ok(new_header) => {
                let new_key = crypto::derive_key(new, &new_header)?;
                self.header = Some(new_header);
                self.conn = Some(open_sqlcipher(&db_path, &new_key, false)?);
                Ok(())
            }
            Err(err) => {
                if was_unlocked {
                    if let Ok(restored) = open_sqlcipher(&db_path, &old_key, false) {
                        self.conn = Some(restored);
                    }
                }
                Err(err)
            }
        }
    }

    /// Borrow the open connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error::VaultLocked`] when not unlocked.
    pub fn connection(&self) -> Result<&Connection> {
        self.conn.as_ref().ok_or(Error::VaultLocked)
    }

    /// Mutable borrow of the open connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error::VaultLocked`] when not unlocked.
    pub fn connection_mut(&mut self) -> Result<&mut Connection> {
        self.conn.as_mut().ok_or(Error::VaultLocked)
    }
}

fn validate_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(Error::Validation(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    Ok(())
}

/// Derive the key for `header` and open the database, verifying the schema
/// is readable (i.e. the password actually matches this header).
fn open_verified(db_path: &Path, password: &str, header: &VaultHeader) -> Result<Connection> {
    let key = crypto::derive_key(password, header)?;
    let conn = open_sqlcipher(db_path, &key, false)?;

    conn.query_row("SELECT schema_version FROM vault_meta LIMIT 1", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|_| Error::InvalidPassword)?;

    Ok(conn)
}

fn open_sqlcipher(path: &Path, key: &VaultKey, create: bool) -> Result<Connection> {
    if !create {
        let meta = fs::metadata(path).map_err(|_| {
            Error::VaultCorrupt("vault database is missing".into())
        })?;
        if meta.len() == 0 {
            return Err(Error::VaultCorrupt("vault database is empty".into()));
        }
    }

    let flags = if create {
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI
    };

    let conn = Connection::open_with_flags(path, flags).map_err(|err| Error::Io(err.to_string()))?;

    let pragma_key = crypto::key_to_sqlcipher_pragma(key);
    // `SQLCipher` requires key before other operations.
    conn.pragma_update(None, "key", pragma_key.as_str())
        .map_err(|_| Error::InvalidPassword)?;

    // Fail fast on wrong key / corrupt file.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|_| Error::InvalidPassword)?;

    // Prefer WAL; `SQLCipher` encrypts WAL pages when key is set.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|err| Error::Io(err.to_string()))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| Error::Io(err.to_string()))?;

    Ok(conn)
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    let mut file = fs::File::create(path).map_err(|err| Error::Io(err.to_string()))?;
    file.write_all(bytes).map_err(|err| Error::Io(err.to_string()))?;
    file.sync_all().map_err(|err| Error::Io(err.to_string()))?;
    if let Some(parent) = path.parent() {
        let dir = fs::File::open(parent).map_err(|err| Error::Io(err.to_string()))?;
        let _ = dir.sync_all();
    }
    Ok(())
}

fn remove_vault_db_sidecars(db_path: &Path) {
    let _ = fs::remove_file(db_path);
    let wal = PathBuf::from(format!("{}-wal", db_path.display()));
    let shm = PathBuf::from(format!("{}-shm", db_path.display()));
    let _ = fs::remove_file(wal);
    let _ = fs::remove_file(shm);
}

fn bootstrap_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS vault_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            schema_version INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );
        INSERT INTO vault_meta (id, schema_version, created_at)
        VALUES (1, 1, datetime('now'));
        ",
    )
    .map_err(|err| Error::Io(err.to_string()))?;
    crate::db::migrate(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_unlock_lock_wrong_password() {
        let Ok(dir) = tempdir() else {
            return;
        };

        let Ok(mut vault) = Vault::open_path(dir.path()) else {
            return;
        };

        assert_eq!(vault.status(), VaultStatus::Uninitialized);

        let password = "correct horse battery staple";
        assert!(vault.init(password).is_ok());
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        vault.lock();
        assert_eq!(vault.status(), VaultStatus::Locked);

        assert!(vault.unlock("wrong password!!").is_err());
        assert_eq!(vault.status(), VaultStatus::Locked);

        assert!(vault.unlock(password).is_ok());
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        // File must not start with plaintext `SQLite` magic.
        let Ok(bytes) = fs::read(vault_db_path(dir.path())) else {
            return;
        };
        assert!(bytes.len() > 16);
        assert_ne!(&bytes[0..6], b"SQLite");
    }
}
