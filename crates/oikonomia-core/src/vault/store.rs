//! Vault open/create/lock against an on-disk `SQLCipher` database.

use std::fs;
use std::path::{Path, PathBuf};

use rand::Rng;
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension};

use super::crypto::{self, VaultKey};
use super::header::{MIN_PASSWORD_LEN, SALT_LEN, VaultHeader};
use super::paths::{vault_db_path, vault_header_path, vault_staged_header_path};
use super::permissions::{create_private_dir, create_private_file, restrict_to_owner};
use crate::db::register_fold;
use crate::error::{Error, Result, ValidationError};
use crate::vault::files::{
    discard_database_files, discard_file, rename_synced, write_private_file,
};
use crate::vault::paths::vault_init_header_path;

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
    /// Inspects the data directory without opening the database.
    ///
    /// Also clears what an interrupted first run left behind, so the vault
    /// reads as uninitialized again; see [`Vault::init`].
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the directory cannot be created or the header cannot
    /// be read; [`Error::VaultCorrupt`] when the header is not valid, comes
    /// from a vault format this build does not know, or is missing while a
    /// database exists.
    pub fn open_path(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let data_dir = data_dir.into();
        create_private_dir(&data_dir)?;

        let header_path = vault_header_path(&data_dir);
        let db_path = vault_db_path(&data_dir);

        // Vaults created before modes were restricted are world-readable.
        for path in [&header_path, &db_path] {
            if path.exists() {
                restrict_to_owner(path);
            }
        }

        let header = if header_path.exists() {
            Some(VaultHeader::load(&header_path)?)
        } else {
            // A staged first-run header with no published one marks a first
            // run that died part-way (see `Vault::init`). Its database holds
            // nothing but the schema, so the run is thrown away and repeated.
            if vault_init_header_path(&data_dir).exists() {
                discard_partial_init(&data_dir);
            }
            if db_path.exists() {
                return Err(Error::VaultCorrupt(
                    "database exists without vault header".into(),
                ));
            }
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

    /// Creates a new encrypted vault under the given master password and
    /// leaves it unlocked.
    ///
    /// The header is what makes a vault exist, so it is published last: it is
    /// staged as `vault.header.json.init`, the database is created and given
    /// its schema, and only then is the staged header renamed into place. A
    /// crash before the rename leaves the staged header beside a database
    /// with no user data, which [`Vault::open_path`] recognizes and discards.
    /// Publishing the header first would instead leave a header with no
    /// database: a vault that reads as locked and that no password opens.
    ///
    /// # Errors
    ///
    /// [`Error::Validation`] when a vault already exists or the password is
    /// too short; [`Error::Crypto`] or [`Error::Io`] when the files cannot be
    /// created. A failed attempt removes what it created.
    pub fn init(&mut self, password: &str) -> Result<()> {
        if self.header.is_some() || vault_db_path(&self.data_dir).exists() {
            return Err(Error::Validation(ValidationError::VaultAlreadyInitialized));
        }

        validate_password(password)?;

        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill_bytes(&mut salt);

        let header = VaultHeader::new_with_salt(&salt);
        let key = crypto::derive_key(password, &header)?;

        match create_vault_files(&self.data_dir, &header, &key) {
            Ok(conn) => {
                self.header = Some(header);
                self.conn = Some(conn);
                Ok(())
            }
            Err(err) => {
                discard_partial_init(&self.data_dir);
                Err(err)
            }
        }
    }

    /// Unlocks an existing vault and brings its schema up to date.
    ///
    /// On a vault that is already unlocked this returns `Ok(())` without
    /// looking at `password`, so it cannot be used to check a password again
    /// (to re-authenticate before a sensitive action, say). Lock first, or
    /// use [`Vault::change_password`], which always verifies the old one.
    ///
    /// # Errors
    ///
    /// [`Error::VaultUninitialized`] when there is no vault;
    /// [`Error::InvalidPassword`] when the password does not decrypt it;
    /// [`Error::VaultCorrupt`] when the header or database is unusable;
    /// [`Error::Crypto`] or [`Error::Io`] otherwise, including a failed
    /// migration.
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
                discard_file(&vault_staged_header_path(&self.data_dir));

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
    ///
    /// Called only after the real header's key failed to decrypt the
    /// database. No staged header, or one that is not a header (a crash can
    /// only truncate it before the rekey starts, while the old key is still
    /// the right one), therefore means the password was simply wrong.
    fn unlock_with_staged_header(&mut self, password: &str) -> Result<()> {
        let staged_path = vault_staged_header_path(&self.data_dir);
        if !staged_path.exists() {
            return Err(Error::InvalidPassword);
        }
        let staged = match VaultHeader::load(&staged_path) {
            Ok(staged) => staged,
            Err(Error::VaultCorrupt(reason)) => {
                log::warn!("ignoring unusable staged vault header: {reason}");
                return Err(Error::InvalidPassword);
            }
            Err(other) => return Err(other),
        };

        let db_path = vault_db_path(&self.data_dir);
        let conn = open_verified(&db_path, password, &staged)?;

        rename_synced(&staged_path, &vault_header_path(&self.data_dir))?;

        crate::db::migrate(&conn)?;
        self.header = Some(staged);
        self.conn = Some(conn);
        Ok(())
    }

    /// Close the database connection and forget process-local key material.
    pub fn lock(&mut self) {
        self.conn = None;
    }

    /// Re-encrypts the vault under a new master password (`SQLCipher` rekey).
    ///
    /// Crash-safety protocol: the new header is staged to a temp file before
    /// the rekey and renamed over the real header after it, and [`Vault::unlock`]
    /// falls back to the staged header when the real one no longer opens the
    /// database. Whatever step the process dies at, exactly one of the two
    /// passwords opens the vault.
    ///
    /// The lock state is kept: an unlocked vault is reopened under the new
    /// key, and a locked vault stays locked. Opening a locked vault is left to
    /// [`Vault::unlock`], which also migrates the schema; a vault opened here
    /// would skip that, and a caller that tracks the lock state would not
    /// expect a password change to alter it.
    ///
    /// # Errors
    ///
    /// [`Error::VaultUninitialized`], [`Error::InvalidPassword`] for a wrong
    /// old password, [`Error::Validation`] for a weak new password, or
    /// crypto/I/O failures. A failed verification leaves any open connection
    /// untouched. A failure after that reopens an unlocked vault under the
    /// old key when that key still fits, and leaves it locked otherwise.
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
        let rekey_conn = open_sqlcipher(&db_path, &old_key, false)?;

        // Only now drop our own connection: its page cache would go stale
        // across the rekey below.
        let was_unlocked = self.conn.take().is_some();

        match rekey_database(&self.data_dir, rekey_conn, new) {
            Ok((new_header, new_key)) => {
                // The files are under the new key from here on, so the header
                // in memory follows before anything else can fail. With the
                // old one, the next unlock would derive the wrong key.
                self.header = Some(new_header);
                if was_unlocked {
                    self.conn = Some(open_sqlcipher(&db_path, &new_key, false)?);
                }
                Ok(())
            }
            Err(err) => {
                if was_unlocked {
                    self.reopen_after_failed_rekey(&db_path, &old_key);
                }
                Err(err)
            }
        }
    }

    /// Puts back the session a failed password change closed.
    ///
    /// The old key no longer fits when the rekey itself went through and a
    /// later step failed. The vault then stays locked, and the next unlock
    /// recovers through the staged header.
    fn reopen_after_failed_rekey(&mut self, db_path: &Path, old_key: &VaultKey) {
        match open_sqlcipher(db_path, old_key, false) {
            Ok(conn) => self.conn = Some(conn),
            Err(err) => log::warn!("vault left locked after a failed password change: {err}"),
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
        return Err(Error::Validation(ValidationError::PasswordTooShort {
            min: MIN_PASSWORD_LEN,
        }));
    }
    Ok(())
}

/// Derives the key for `header`, opens the database with it and checks that
/// the vault's own schema is there.
///
/// # Errors
///
/// [`Error::InvalidPassword`] when the key does not decrypt the database;
/// [`Error::VaultCorrupt`] when it does but `vault_meta` is missing or empty;
/// otherwise whatever [`open_sqlcipher`] returns.
fn open_verified(db_path: &Path, password: &str, header: &VaultHeader) -> Result<Connection> {
    let key = crypto::derive_key(password, header)?;
    let conn = open_sqlcipher(db_path, &key, false)?;

    let has_meta_table = conn
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'vault_meta'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|err| key_check_error(&err))?;
    if !has_meta_table {
        return Err(Error::VaultCorrupt("vault_meta table is missing".into()));
    }

    conn.query_row("SELECT schema_version FROM vault_meta LIMIT 1", [], |row| {
        row.get::<_, i64>(0)
    })
    .optional()
    .map_err(|err| key_check_error(&err))?
    .ok_or_else(|| Error::VaultCorrupt("vault_meta has no schema version".into()))?;

    Ok(conn)
}

/// Classifies a failed read of a database whose key has just been set.
///
/// `SQLCipher` cannot tell a wrong key from a file that is not a database:
/// either way the first page decrypts to garbage and `SQLite` reports
/// `SQLITE_NOTADB`. That code alone means the password may be wrong. A busy
/// database or a disk error says nothing about the password, and reporting
/// it as one would send the user off retyping a password that is correct.
fn key_check_error(err: &rusqlite::Error) -> Error {
    if err.sqlite_error_code() == Some(ErrorCode::NotADatabase) {
        Error::InvalidPassword
    } else {
        Error::Io(err.to_string())
    }
}

/// Opens the `SQLCipher` database at `path` under `key` and proves the key
/// with a first read.
///
/// With `create`, `path` is created empty first, truncating whatever is
/// there; the caller checks that no database exists. Without it, a missing
/// or empty file is an error instead of a fresh database.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] when `create` is false and the file is missing or
/// empty; [`Error::InvalidPassword`] when `key` does not decrypt it;
/// [`Error::Crypto`] or [`Error::Io`] for everything else.
fn open_sqlcipher(path: &Path, key: &VaultKey, create: bool) -> Result<Connection> {
    if !create {
        let meta = fs::metadata(path)
            .map_err(|_| Error::VaultCorrupt("vault database is missing".into()))?;
        if meta.len() == 0 {
            return Err(Error::VaultCorrupt("vault database is empty".into()));
        }
    }

    // SQLite would create the file under the umask, readable by other
    // accounts until it is tightened. Creating it empty and owner-only first
    // closes that window: SQLite treats an empty file as a new database, and
    // its WAL and SHM sidecars copy the database file's mode.
    if create {
        drop(create_private_file(path)?);
    }

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI;
    let conn =
        Connection::open_with_flags(path, flags).map_err(|err| Error::Io(err.to_string()))?;

    silence_sqlcipher_log(&conn)?;

    // SQLCipher 4.5+ ships with this off. On, it locks and wipes its key and
    // page buffers instead of leaving plaintext in freed heap or swap. The
    // flag is process-global and can only be turned on, so set it before the
    // key so the key material itself is covered.
    conn.pragma_update(None, "cipher_memory_security", "ON")
        .map_err(|err| Error::Crypto(err.to_string()))?;

    let pragma_key = crypto::key_to_sqlcipher_pragma(key);
    // `SQLCipher` requires key before other operations.
    conn.pragma_update(None, "key", pragma_key.as_str())
        .map_err(|err| key_check_error(&err))?;

    // The key pragma only stores the key; the first read is what proves it.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|err| key_check_error(&err))?;

    // Prefer WAL; `SQLCipher` encrypts WAL pages when key is set.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|err| Error::Io(err.to_string()))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| Error::Io(err.to_string()))?;

    // Case-insensitive search and name checks call `fold(...)`, which exists
    // only on the connection that registered it. This is the one place every
    // vault connection is made (open, unlock, rekey and its restore), so no
    // query can meet a connection without it.
    register_fold(&conn).map_err(|err| Error::Io(err.to_string()))?;

    Ok(conn)
}

/// Turn off `SQLCipher`'s own log on Windows, before memory security is on.
///
/// With memory security on, `SQLCipher` locks every allocation in RAM. Windows
/// allows a process only a small locked set, so the lock soon fails, and
/// `SQLCipher` logs a warning for each failure. On Windows that log line is
/// built with `SQLite`'s allocator, which tries to lock the new buffer, fails,
/// and logs again: endless recursion that overflows the stack the first time
/// a vault is created. With the log off, a failed lock is ignored, and the
/// buffers are still wiped when freed.
///
/// Other platforms write the log without allocating (`fprintf`, or the system
/// log on macOS), so they keep it.
#[cfg(windows)]
fn silence_sqlcipher_log(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "cipher_log_level", "NONE")
        .map_err(|err| Error::Crypto(err.to_string()))
}

#[cfg(not(windows))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "same signature as the Windows version, which can fail"
)]
fn silence_sqlcipher_log(_conn: &Connection) -> Result<()> {
    Ok(())
}

/// Rekeys the database behind `conn` to a fresh salt and `new_password`,
/// following the staging protocol [`Vault::change_password`] documents.
///
/// Returns the header now on disk and the key derived from it, so the
/// caller neither re-reads the header nor runs the KDF a second time.
fn rekey_database(
    data_dir: &Path,
    conn: Connection,
    new_password: &str,
) -> Result<(VaultHeader, VaultKey)> {
    let mut salt = [0u8; SALT_LEN];
    rand::rng().fill_bytes(&mut salt);
    let new_header = VaultHeader::new_with_salt(&salt);
    let new_key = crypto::derive_key(new_password, &new_header)?;

    let staged_path = vault_staged_header_path(data_dir);
    let header_json =
        serde_json::to_string_pretty(&new_header).map_err(|err| Error::Io(err.to_string()))?;
    write_private_file(&staged_path, header_json.as_bytes())?;

    // Fold WAL pages into the main file so the rekey covers everything.
    let blocked: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|err| Error::Io(err.to_string()))?;
    if blocked != 0 {
        return Err(Error::Io("wal checkpoint blocked; will not rekey".into()));
    }
    let pragma_key = crypto::key_to_sqlcipher_pragma(&new_key);
    conn.pragma_update(None, "rekey", pragma_key.as_str())
        .map_err(|err| Error::Crypto(err.to_string()))?;
    drop(conn);

    rename_synced(&staged_path, &vault_header_path(data_dir))?;
    Ok((new_header, new_key))
}

/// Writes the files of a new vault in the order [`Vault::init`] documents and
/// returns the open connection.
fn create_vault_files(data_dir: &Path, header: &VaultHeader, key: &VaultKey) -> Result<Connection> {
    let header_json =
        serde_json::to_string_pretty(header).map_err(|err| Error::Io(err.to_string()))?;
    let staged_header = vault_init_header_path(data_dir);
    write_private_file(&staged_header, header_json.as_bytes())?;

    let conn = open_sqlcipher(&vault_db_path(data_dir), key, true)?;
    bootstrap_schema(&conn)?;

    rename_synced(&staged_header, &vault_header_path(data_dir))?;
    Ok(conn)
}

/// Removes what a first run that did not finish left behind: the staged
/// header and the half-made database.
///
/// Safe only while no header is published, which is the one state it is
/// called in; the published header is never touched.
fn discard_partial_init(data_dir: &Path) {
    discard_database_files(&vault_db_path(data_dir));
    discard_file(&vault_init_header_path(data_dir));
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
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    const PASSWORD: &str = "correct horse battery staple";

    fn init_vault() -> (TempDir, Vault) {
        let dir = tempdir().expect("tempdir");
        let mut vault = Vault::open_path(dir.path()).expect("open vault");
        vault.init(PASSWORD).expect("init");
        (dir, vault)
    }

    #[test]
    fn init_unlock_lock_wrong_password() {
        let dir = tempdir().expect("tempdir");
        let mut vault = Vault::open_path(dir.path()).expect("open vault");
        assert_eq!(vault.status(), VaultStatus::Uninitialized);

        vault.init(PASSWORD).expect("init");
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        vault.lock();
        assert_eq!(vault.status(), VaultStatus::Locked);

        assert_eq!(
            vault.unlock("wrong password!!"),
            Err(Error::InvalidPassword)
        );
        assert_eq!(vault.status(), VaultStatus::Locked);

        vault.unlock(PASSWORD).expect("unlock");
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        // File must not start with plaintext `SQLite` magic.
        let bytes = fs::read(vault_db_path(dir.path())).expect("read database");
        assert!(bytes.len() > 16);
        assert_ne!(&bytes[0..6], b"SQLite");
    }

    #[test]
    fn a_header_from_a_newer_format_is_rejected_on_open() {
        let (dir, vault) = init_vault();
        drop(vault);

        let header_path = vault_header_path(dir.path());
        let raw = fs::read_to_string(&header_path).expect("read header");
        let mut header: VaultHeader = serde_json::from_str(&raw).expect("parse header");
        header.version += 1;
        fs::write(
            &header_path,
            serde_json::to_string(&header).expect("encode"),
        )
        .expect("write header");

        let err = Vault::open_path(dir.path())
            .map(|vault| vault.status())
            .expect_err("a format this build does not know must not open");
        assert_eq!(
            err,
            Error::VaultCorrupt("unsupported vault format 2".into())
        );
    }

    #[test]
    fn a_vault_without_its_meta_table_is_corrupt_not_a_wrong_password() {
        let (_dir, mut vault) = init_vault();
        vault
            .connection()
            .expect("unlocked after init")
            .execute_batch("DROP TABLE vault_meta")
            .expect("drop meta table");
        vault.lock();

        let err = vault.unlock(PASSWORD).expect_err("meta table is gone");

        assert!(matches!(err, Error::VaultCorrupt(_)), "got {err:?}");
    }

    #[test]
    fn a_vault_with_an_empty_meta_table_is_corrupt() {
        let (_dir, mut vault) = init_vault();
        vault
            .connection()
            .expect("unlocked after init")
            .execute_batch("DELETE FROM vault_meta")
            .expect("empty meta table");
        vault.lock();

        let err = vault.unlock(PASSWORD).expect_err("meta row is gone");

        assert!(matches!(err, Error::VaultCorrupt(_)), "got {err:?}");
    }

    #[test]
    fn an_unusable_staged_header_does_not_change_a_wrong_password_error() {
        let (dir, mut vault) = init_vault();
        vault.lock();
        fs::write(vault_staged_header_path(dir.path()), b"{ truncated").expect("staged");

        assert_eq!(
            vault.unlock("wrong password!!"),
            Err(Error::InvalidPassword)
        );
        vault.unlock(PASSWORD).expect("the real header still works");
    }

    #[test]
    fn a_busy_database_is_an_io_error_not_a_wrong_password() {
        let (dir, mut vault) = init_vault();
        vault.lock();

        // A second connection that holds the database exclusively, the way
        // another process mid-write would.
        let header = VaultHeader::load(&vault_header_path(dir.path())).expect("header");
        let key = crypto::derive_key(PASSWORD, &header).expect("key");
        let holder = open_sqlcipher(&vault_db_path(dir.path()), &key, false).expect("holder");
        holder
            .execute_batch(
                "PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; \
                 CREATE TABLE held (id INTEGER); COMMIT;",
            )
            .expect("take the exclusive lock");

        let err = vault.unlock(PASSWORD).expect_err("database is held");

        assert!(matches!(err, Error::Io(_)), "got {err:?}");
        assert_eq!(vault.status(), VaultStatus::Locked);
    }

    #[test]
    fn open_enables_sqlcipher_memory_security() {
        let (_dir, vault) = init_vault();
        let conn = vault.connection().expect("unlocked after init");

        // SQLCipher reports "1" only once the pragma is on and its guarded
        // allocator has run, i.e. the key was handled under memory security.
        let state: String = conn
            .query_row("PRAGMA cipher_memory_security", [], |row| row.get(0))
            .expect("read pragma");
        assert_eq!(
            state, "1",
            "SQLCipher must lock and wipe its key and page buffers"
        );
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn init_creates_owner_only_directory_and_files() {
        let dir = tempdir().expect("tempdir");
        let data_dir = dir.path().join("vault");
        let mut vault = Vault::open_path(&data_dir).expect("open vault");
        vault.init(PASSWORD).expect("init");

        assert_eq!(mode_of(&data_dir), 0o700, "data directory");
        assert_eq!(mode_of(&vault_header_path(&data_dir)), 0o600, "header");
        assert_eq!(mode_of(&vault_db_path(&data_dir)), 0o600, "database");
    }

    #[cfg(unix)]
    #[test]
    fn open_tightens_permissions_of_an_existing_vault() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, vault) = init_vault();
        drop(vault);

        // Vaults created before modes were restricted are world-readable.
        for path in [vault_header_path(dir.path()), vault_db_path(dir.path())] {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("loosen");
        }

        Vault::open_path(dir.path()).expect("reopen");
        assert_eq!(mode_of(&vault_header_path(dir.path())), 0o600, "header");
        assert_eq!(mode_of(&vault_db_path(dir.path())), 0o600, "database");
    }

    #[test]
    fn discard_partial_init_removes_the_staged_header_and_the_database() {
        let dir = tempdir().expect("tempdir");
        let staged = vault_init_header_path(dir.path());
        let db = vault_db_path(dir.path());
        for path in [&staged, &db] {
            fs::write(path, b"x").expect("write");
        }

        discard_partial_init(dir.path());

        assert!(!staged.exists(), "staged init header must be removed");
        assert!(!db.exists(), "orphan db must be removed");
    }

    #[test]
    fn init_leaves_no_staged_header() {
        let (dir, _vault) = init_vault();

        assert!(vault_header_path(dir.path()).exists());
        assert!(!vault_init_header_path(dir.path()).exists());
    }

    #[test]
    fn a_first_run_that_died_before_its_header_was_published_starts_over() {
        // What a crash inside `init` leaves: the staged header and a database
        // that holds nothing but the schema, with no published header.
        let (dir, vault) = init_vault();
        drop(vault);
        fs::rename(
            vault_header_path(dir.path()),
            vault_init_header_path(dir.path()),
        )
        .expect("unpublish header");

        let mut vault = Vault::open_path(dir.path()).expect("open after the crash");

        assert_eq!(vault.status(), VaultStatus::Uninitialized);
        assert!(!vault_db_path(dir.path()).exists(), "half-made database");
        assert!(
            !vault_init_header_path(dir.path()).exists(),
            "staged header"
        );
        vault.init(PASSWORD).expect("the first run can be repeated");
    }

    #[test]
    fn a_database_without_any_header_is_still_corrupt() {
        let (dir, vault) = init_vault();
        drop(vault);
        fs::remove_file(vault_header_path(dir.path())).expect("lose header");

        let err = Vault::open_path(dir.path())
            .map(|vault| vault.status())
            .expect_err("a database with no header must not be discarded");

        assert!(matches!(err, Error::VaultCorrupt(_)), "got {err:?}");
        assert!(vault_db_path(dir.path()).exists(), "the database stays");
    }
}
