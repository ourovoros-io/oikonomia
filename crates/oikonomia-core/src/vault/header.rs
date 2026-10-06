//! The public vault header, `vault.header.json`, stored beside the database.
//!
//! The header is everything needed to turn the master password into the
//! database key, other than the password: the salt and the Argon2id costs.
//! None of it is secret, and it is stored as JSON. A backup archive carries
//! it unchanged next to the database.
//!
//! The costs are stored per vault instead of compiled in so that the
//! defaults can be raised later without locking out vaults made under the
//! old ones: a vault is always opened with the costs it was created or last
//! rekeyed with.
//!
//! `version` covers the header and the meaning of its fields. It is checked
//! on load, before anything is derived from the header. The database has a
//! schema version of its own in `vault_meta`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};

/// Version of the on-disk vault format that this build reads and writes.
pub(super) const VAULT_FORMAT_VERSION: u32 = 1;

/// Minimum master password length, in characters.
pub(super) const MIN_PASSWORD_LEN: usize = 12;

/// Argon2id memory cost of a new vault, in kibibytes: 64 MiB.
#[cfg(not(test))]
pub(super) const DEFAULT_M_COST: u32 = 65_536;
/// Argon2id memory cost under test, in kibibytes: 19 MiB.
///
/// With [`DEFAULT_T_COST`] of 2 and one lane this is one of the minimum
/// Argon2id configurations in the OWASP Password Storage Cheat Sheet
/// (<https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html>).
/// The suite derives a key in most vault tests, and the weaker setting keeps
/// it fast.
#[cfg(test)]
pub(super) const DEFAULT_M_COST: u32 = 19_456;

/// Argon2id time cost of a new vault, in passes.
#[cfg(not(test))]
pub(super) const DEFAULT_T_COST: u32 = 3;
/// Argon2id time cost under test, in passes.
#[cfg(test)]
pub(super) const DEFAULT_T_COST: u32 = 2;

/// Argon2id parallelism of a new vault, in lanes.
pub(super) const DEFAULT_P_COST: u32 = 1;

/// Length of the derived key in bytes: a `SQLCipher` raw key is 256 bits.
pub(super) const KEY_LEN: usize = 32;

/// Length of the salt in bytes.
pub(super) const SALT_LEN: usize = 16;

/// Non-secret parameters needed to derive the database key.
///
/// The fields are public because the header is a file format: its JSON keys
/// are these field names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultHeader {
    /// On-disk format version. Only version 1 is read.
    pub version: u32,
    /// Name of the key-derivation function. Only `argon2id` is accepted.
    pub kdf: String,
    /// Salt, 16 bytes in standard base64 with padding.
    pub salt_b64: String,
    /// Argon2 memory cost in kibibytes.
    pub m_cost: u32,
    /// Argon2 time cost in passes.
    pub t_cost: u32,
    /// Argon2 parallelism in lanes.
    pub p_cost: u32,
    /// Length of the derived key in bytes. Only 32 is accepted.
    pub output_len: usize,
}

impl VaultHeader {
    /// Builds a header for a new key from the caller's salt and the default
    /// Argon2id parameters. The salt must come from a CSPRNG.
    #[must_use]
    pub fn new_with_salt(salt: &[u8; SALT_LEN]) -> Self {
        use base64::Engine;

        Self {
            version: VAULT_FORMAT_VERSION,
            kdf: "argon2id".to_owned(),
            salt_b64: base64::engine::general_purpose::STANDARD.encode(salt),
            m_cost: DEFAULT_M_COST,
            t_cost: DEFAULT_T_COST,
            p_cost: DEFAULT_P_COST,
            output_len: KEY_LEN,
        }
    }

    /// Reads and checks the header file at `path`.
    ///
    /// The format version is checked here, before any key is derived: a
    /// header written by a newer build would otherwise be used with this
    /// build's rules and fail later as a wrong password.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the file cannot be read; [`Error::VaultCorrupt`]
    /// when it is not a header or its `version` is not
    /// [`VAULT_FORMAT_VERSION`].
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).io("read vault header")?;
        let header: Self =
            serde_json::from_str(&raw).map_err(|err| Error::VaultCorrupt(err.to_string()))?;

        if header.version != VAULT_FORMAT_VERSION {
            return Err(Error::VaultCorrupt(format!(
                "unsupported vault format {}",
                header.version
            )));
        }
        Ok(header)
    }
}
