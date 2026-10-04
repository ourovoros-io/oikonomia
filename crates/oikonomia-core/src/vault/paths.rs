//! Filesystem locations for the encrypted vault.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const QUALIFIER: &str = "io";
const ORGANIZATION: &str = "ourovoros";
const APPLICATION: &str = "oikonomia";

/// Platform app-data directory for Oikonomia.
///
/// The machine-local directory, never the roaming one: on Windows a roaming
/// profile copies its files between machines at sign-in, which can replace a
/// live `SQLite` database with a stale copy. macOS and Linux have a single
/// data directory, so the choice changes nothing there.
///
/// # Errors
///
/// Returns [`Error::Io`] when the OS path cannot be resolved.
pub fn default_data_dir() -> Result<PathBuf> {
    directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|dirs| dirs.data_local_dir().to_path_buf())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(clippy::expect_used, reason = "test fails loudly by design")]
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
    #[expect(clippy::expect_used, reason = "test fails loudly by design")]
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
    #[expect(clippy::expect_used, reason = "test fails loudly by design")]
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
