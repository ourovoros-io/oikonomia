//! Every file name the vault uses, and where the data directory is.
//!
//! The names are spelled in this module and nowhere else, so the file set in
//! the [`crate::vault`] module doc can be checked against one file, and two
//! protocols cannot pick the same temporary name by accident. Other modules
//! ask for a path by role (`vault_staged_header_path`, `RestorePaths`) and
//! never join a literal onto the data directory.
//!
//! Names are built on the `OsString` (`with_appended`), not through
//! `Display`, so a data directory whose path is not valid UTF-8 keeps its
//! bytes.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Reverse-domain qualifier of the application identity.
const QUALIFIER: &str = "io";
/// Organization part of the application identity.
const ORGANIZATION: &str = "ourovoros";
/// Application part of the application identity.
const APPLICATION: &str = "oikonomia";

/// Suffix of the sibling a file is written under before it is renamed over
/// its destination: an export, a backup archive.
///
/// The destination is one the user chose, so unlike the other names in this
/// module the result is not a file of the data directory.
pub(crate) const STAGED_SUFFIX: &str = ".tmp";

/// Returns the platform app-data directory for Oikonomia.
///
/// The machine-local directory, never the roaming one: on Windows a roaming
/// profile copies its files between machines at sign-in, which can replace a
/// live `SQLite` database with a stale copy. macOS and Linux have a single
/// data directory, so the choice changes nothing there.
///
/// # Errors
///
/// Returns [`Error::Io`] when the operating system gives no home directory
/// to derive the path from (`directories::ProjectDirs::from` returns `None`).
pub fn default_data_dir() -> Result<PathBuf> {
    directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .ok_or_else(|| Error::Io("could not resolve application data directory".into()))
}

/// Returns the path of the encrypted `SQLCipher` database, `vault.db`.
#[must_use]
pub fn vault_db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.db")
}

/// Returns the path of the public vault header, `vault.header.json`.
///
/// The header holds the salt and the key-derivation parameters. None of it
/// is secret.
#[must_use]
pub fn vault_header_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.header.json")
}

/// Returns the staging path for the next header during a password change.
///
/// If the app dies between the `SQLCipher` rekey and the header rename,
/// [`crate::vault::Vault::unlock`] falls back to this file so the vault stays
/// openable with the new password.
#[must_use]
pub(super) fn vault_staged_header_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.header.json.tmp")
}

/// Returns the staging path for the header of a vault that is being created.
///
/// [`crate::vault::Vault::init`] renames it to [`vault_header_path`] as its
/// last step, so this file on its own means an unfinished first run.
#[must_use]
pub(crate) fn vault_init_header_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.header.json.init")
}

/// Returns the path of the database snapshot an online backup packs and then
/// removes.
#[must_use]
pub(crate) fn backup_snapshot_db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.db.backup-tmp")
}

/// Every file a restore touches in one data directory.
///
/// The roles, and what each combination of these files means after a crash,
/// are the restore protocol in the `vault::backup` module doc.
#[derive(Debug)]
pub(crate) struct RestorePaths {
    /// Live header, [`vault_header_path`].
    pub(crate) header: PathBuf,
    /// Live database, [`vault_db_path`].
    pub(crate) db: PathBuf,
    /// Header as unpacked from the archive, not yet checked.
    pub(crate) unpacked_header: PathBuf,
    /// Database as unpacked from the archive; it keeps this name until it
    /// becomes the live database.
    pub(crate) unpacked_db: PathBuf,
    /// Unpacked header after the archive passed its checks. It exists
    /// exactly while the live pair is being swapped.
    pub(crate) verified_header: PathBuf,
    /// Previous live header, set aside until the swap is complete.
    pub(crate) old_header: PathBuf,
    /// Previous live database, set aside until the swap is complete.
    pub(crate) old_db: PathBuf,
    /// Write-ahead log of the live database.
    pub(crate) wal: PathBuf,
    /// Shared-memory index of the live database.
    pub(crate) shm: PathBuf,
    /// Previous write-ahead log, set aside with the database it belongs to.
    pub(crate) old_wal: PathBuf,
}

impl RestorePaths {
    /// Returns the restore paths of `data_dir`.
    #[must_use]
    pub(crate) fn new(data_dir: &Path) -> Self {
        let db = vault_db_path(data_dir);
        let [wal, shm] = db_sidecar_paths(&db);
        Self {
            header: vault_header_path(data_dir),
            db,
            wal,
            shm,
            old_wal: data_dir.join("vault.db-wal.restore-old"),
            unpacked_header: data_dir.join("vault.header.json.restore-tmp"),
            unpacked_db: data_dir.join("vault.db.restore-tmp"),
            verified_header: data_dir.join("vault.header.json.restore-new"),
            old_header: data_dir.join("vault.header.json.restore-old"),
            old_db: data_dir.join("vault.db.restore-old"),
        }
    }
}

/// Returns the paths of the write-ahead log and the shared-memory index that
/// `SQLite` keeps next to `db_path` in WAL mode, in that order.
#[must_use]
pub(crate) fn db_sidecar_paths(db_path: &Path) -> [PathBuf; 2] {
    [
        with_appended(db_path, "-wal"),
        with_appended(db_path, "-shm"),
    ]
}

/// Returns `path` with `suffix` appended to its last component.
///
/// Works on the `OsString`, so a path that is not valid UTF-8 keeps its
/// bytes; formatting it through `Display` would replace them.
#[must_use]
pub(crate) fn with_appended(path: &Path, suffix: &str) -> PathBuf {
    let mut appended = path.as_os_str().to_os_string();
    appended.push(suffix);
    PathBuf::from(appended)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecars_sit_next_to_the_database() {
        let [wal, shm] = db_sidecar_paths(Path::new("data/vault.db"));
        assert_eq!(wal, PathBuf::from("data/vault.db-wal"));
        assert_eq!(shm, PathBuf::from("data/vault.db-shm"));
    }

    #[test]
    #[cfg(unix)]
    fn sidecar_paths_keep_bytes_that_are_not_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let data_dir = Path::new(OsStr::from_bytes(b"/tmp/caf\xe9"));
        let [wal, _shm] = db_sidecar_paths(&vault_db_path(data_dir));

        assert_eq!(wal.as_os_str().as_bytes(), b"/tmp/caf\xe9/vault.db-wal");
    }

    #[test]
    fn default_data_dir_uses_ourovoros_identity() {
        let data_dir = default_data_dir().expect("app-data dir must be resolvable");
        let path_str = data_dir.to_string_lossy();

        // All platforms: the old bundle identifier must not appear anywhere in the path.
        assert!(
            !path_str.contains("com.georgiosdelkos"),
            "data directory must not contain old bundle identifier com.georgiosdelkos: {path_str}"
        );

        // macOS only: directories::ProjectDirs includes qualifier/organization in the path.
        // On Linux, the path is ~/.local/share/<app>, omitting qualifier/organization.
        #[cfg(target_os = "macos")]
        assert!(
            path_str.contains("io.ourovoros.oikonomia"),
            "on macOS, data directory must contain new identity io.ourovoros.oikonomia: {path_str}"
        );
    }

    #[test]
    #[cfg(windows)]
    fn windows_data_dir_is_machine_local_not_roaming() {
        let data_dir = default_data_dir().expect("app-data dir must be resolvable");
        let roaming = directories::BaseDirs::new()
            .expect("base dirs must be resolvable")
            .data_dir()
            .to_path_buf();

        assert!(
            !data_dir.starts_with(&roaming),
            "vault must not live under roaming AppData: {}",
            data_dir.display()
        );
        let local = directories::BaseDirs::new()
            .expect("base dirs must be resolvable")
            .data_local_dir()
            .to_path_buf();
        assert_eq!(
            data_dir,
            local.join("ourovoros").join("oikonomia").join("data")
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_data_dir_is_the_xdg_data_directory() {
        let data_dir = default_data_dir().expect("app-data dir must be resolvable");
        let xdg_data = directories::BaseDirs::new()
            .expect("base dirs must be resolvable")
            .data_local_dir()
            .to_path_buf();

        // `$XDG_DATA_HOME/oikonomia`, by default `~/.local/share/oikonomia`.
        assert_eq!(data_dir, xdg_data.join("oikonomia"));
    }
}
