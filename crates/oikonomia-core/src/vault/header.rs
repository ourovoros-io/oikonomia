//! Public vault header stored beside the encrypted database.

use serde::{Deserialize, Serialize};

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
    /// Build a new header with random salt and default Argon2id params.
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
}
