//! Filesystem locations for the encrypted vault.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "georgiosdelkos";
const APPLICATION: &str = "oikonomia";

/// Platform app-data directory for Oikonomia.
///
/// # Errors
///
/// Returns [`Error::Io`] when the OS path cannot be resolved.
pub fn default_data_dir() -> Result<PathBuf> {
    directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| Error::Io("could not resolve application data directory".into()))
}

/// Path to the encrypted `SQLite` / `SQLCipher` database file.
#[must_use]
pub fn vault_db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.db")
}

/// Path to the public vault header (salt + KDF params, not secret).
#[must_use]
pub fn vault_header_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.header.json")
}

/// Staging path for the next header during a password change.
///
/// If the app dies between the `SQLCipher` rekey and the header rename,
/// [`crate::vault::Vault::unlock`] falls back to this file so the vault stays
/// openable with the new password.
#[must_use]
pub fn vault_staged_header_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vault.header.json.tmp")
}
