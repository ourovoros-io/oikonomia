//! Public vault header stored beside the encrypted database.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Schema version for the on-disk vault format.
pub const VAULT_FORMAT_VERSION: u32 = 1;

/// Minimum master password length (characters).
pub const MIN_PASSWORD_LEN: usize = 12;

/// Argon2id memory cost in kibibytes. Production targets the design's
/// ~200–500ms band; tests keep the weaker OWASP floor so the suite stays fast.
#[cfg(not(test))]
pub const DEFAULT_M_COST: u32 = 65_536;
/// Test-only KDF memory (KiB).
#[cfg(test)]
pub const DEFAULT_M_COST: u32 = 19_456;

/// Argon2id time cost (iterations).
#[cfg(not(test))]
pub const DEFAULT_T_COST: u32 = 3;
/// Test-only KDF time cost.
#[cfg(test)]
pub const DEFAULT_T_COST: u32 = 2;

/// Argon2id parallelism.
pub const DEFAULT_P_COST: u32 = 1;

/// Derived key length for `SQLCipher` raw key (bytes).
pub const KEY_LEN: usize = 32;

/// Salt length (bytes).
pub const SALT_LEN: usize = 16;

/// Non-secret parameters needed to derive the database key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultHeader {
    /// On-disk format version.
    pub version: u32,
    /// KDF name (always `argon2id` for v1).
    pub kdf: String,
    /// Base64-encoded salt.
    pub salt_b64: String,
    /// Argon2 memory cost (KiB).
    pub m_cost: u32,
    /// Argon2 time cost.
    pub t_cost: u32,
    /// Argon2 parallelism.
    pub p_cost: u32,
    /// Derived key length in bytes.
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
        let raw = fs::read_to_string(path).map_err(|err| Error::Io(err.to_string()))?;
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
