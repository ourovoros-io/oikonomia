//! Vault open/create/lock against an on-disk `SQLCipher` database.

use std::fs;
use std::path::{Path, PathBuf};

use rand::RngCore;
use rusqlite::Connection;
use zeroize::Zeroizing;

use super::crypto::{self, VaultKey};
use super::header::{MIN_PASSWORD_LEN, SALT_LEN, VaultHeader};
use super::paths::{vault_db_path, vault_header_path};
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

        let db_path = vault_db_path(&self.data_dir);
        let conn = open_sqlcipher(&db_path, &key)?;
        bootstrap_schema(&conn)?;

        let header_path = vault_header_path(&self.data_dir);
        let header_json =
            serde_json::to_string_pretty(&header).map_err(|err| Error::Io(err.to_string()))?;
        fs::write(&header_path, header_json).map_err(|err| Error::Io(err.to_string()))?;

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

        let key = crypto::derive_key(password, &header)?;
        let db_path = vault_db_path(&self.data_dir);

        match open_sqlcipher(&db_path, &key) {
            Ok(conn) => {
                // Touch the schema to confirm the key works.
                conn.query_row("SELECT schema_version FROM vault_meta LIMIT 1", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|_| Error::InvalidPassword)?;

                crate::db::migrate(&conn)?;
                self.conn = Some(conn);
                Ok(())
            }
            Err(Error::InvalidPassword) => Err(Error::InvalidPassword),
            Err(other) => Err(other),
        }
    }

    /// Close the database connection and forget process-local key material.
    pub fn lock(&mut self) {
        self.conn = None;
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

fn open_sqlcipher(path: &Path, key: &VaultKey) -> Result<Connection> {
    let conn = Connection::open(path).map_err(|err| Error::Io(err.to_string()))?;

    let pragma_key = Zeroizing::new(crypto::key_to_sqlcipher_pragma(key));
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
