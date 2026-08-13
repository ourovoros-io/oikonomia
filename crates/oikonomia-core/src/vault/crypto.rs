//! Password → vault key derivation.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use zeroize::{Zeroize, Zeroizing};

use super::header::{KEY_LEN, SALT_LEN, VaultHeader};
use crate::error::{Error, Result};

/// 32-byte `SQLCipher` raw key, zeroized on drop.
pub type VaultKey = Zeroizing<[u8; KEY_LEN]>;

/// Decode the salt from a header.
///
/// # Errors
///
/// Returns [`Error::VaultCorrupt`] if the salt is not valid base64 of the expected length.
pub fn decode_salt(header: &VaultHeader) -> Result<[u8; SALT_LEN]> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(header.salt_b64.as_bytes())
        .map_err(|_| Error::VaultCorrupt("invalid salt encoding".into()))?;

    if bytes.len() != SALT_LEN {
        return Err(Error::VaultCorrupt("unexpected salt length".into()));
    }

    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&bytes);
    Ok(salt)
}

/// Derive a `SQLCipher` key from the master password and header params.
///
/// # Errors
///
/// Returns [`Error::Crypto`] if Argon2 parameters are invalid or hashing fails.
pub fn derive_key(password: &str, header: &VaultHeader) -> Result<VaultKey> {
    if header.kdf != "argon2id" {
        return Err(Error::VaultCorrupt(format!(
            "unsupported kdf: {}",
            header.kdf
        )));
    }

    if header.output_len != KEY_LEN {
        return Err(Error::VaultCorrupt("unsupported key length".into()));
    }

    let salt = decode_salt(header)?;
    let params = Params::new(header.m_cost, header.t_cost, header.p_cost, Some(KEY_LEN))
        .map_err(|err| Error::Crypto(err.to_string()))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];

    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|err| {
            key.zeroize();
            Error::Crypto(err.to_string())
        })?;

    Ok(Zeroizing::new(key))
}

/// Format a raw key for `SQLCipher` `PRAGMA key = "x'…'"`.
#[must_use]
pub fn key_to_sqlcipher_pragma(key: &VaultKey) -> Zeroizing<String> {
    let mut hex = String::with_capacity(KEY_LEN * 2 + 3);
    hex.push_str("x'");
    for byte in key.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex.push('\'');
    Zeroizing::new(hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::header::VaultHeader;

    #[test]
    fn derive_is_deterministic() {
        let salt = [7u8; SALT_LEN];
        let header = VaultHeader::new_with_salt(&salt);
        let a = derive_key("correct horse battery staple", &header);
        let b = derive_key("correct horse battery staple", &header);
        assert!(a.is_ok());
        assert_eq!(a, b);
    }

    #[test]
    fn different_password_different_key() {
        let salt = [9u8; SALT_LEN];
        let header = VaultHeader::new_with_salt(&salt);
        let a = derive_key("aaaaaaaaaaaa", &header);
        let b = derive_key("bbbbbbbbbbbb", &header);
        assert!(a.is_ok() && b.is_ok());
        assert_ne!(a.ok(), b.ok());
    }
}
