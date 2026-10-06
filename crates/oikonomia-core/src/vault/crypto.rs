//! Derives the database key from the master password.
//!
//! The key is Argon2id (version 0x13) over the password and the header's
//! salt, with the header's cost parameters, and it is used as a `SQLCipher`
//! raw key: `SQLCipher`'s own password hashing is bypassed, so the header is
//! the complete description of how a password becomes a key.
//!
//! The header is not authenticated, and a restored backup brings its own,
//! so its parameters are treated as input: [`derive_key`] accepts
//! costs only inside a fixed range before it runs Argon2. A header asking
//! for more memory than that is reported as corrupt instead of being obeyed.
//!
//! Key material is produced inside [`zeroize::Zeroizing`], which wipes the
//! value it owns when dropped. That does not reach copies a move leaves
//! behind: the key is a 32-byte array returned by value, and the compiler
//! may copy it between stack frames. The heap copy that escapes through
//! rusqlite is described on [`key_to_sqlcipher_pragma`].

use std::fmt::{Display, Write};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use zeroize::Zeroizing;

use crate::error::{CryptoContext, Error, Result, VaultCorruption};
use crate::vault::header::{KEY_LEN, SALT_LEN, VaultHeader};

/// 32-byte `SQLCipher` raw key, zeroized on drop.
pub(super) type VaultKey = Zeroizing<[u8; KEY_LEN]>;

/// Decodes the salt of `header` from standard base64.
///
/// # Errors
///
/// Returns [`Error::VaultCorrupt`] when the salt is not valid base64 or does
/// not decode to [`SALT_LEN`] bytes.
pub(super) fn decode_salt(header: &VaultHeader) -> Result<[u8; SALT_LEN]> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(header.salt_b64.as_bytes())
        .map_err(|_| unusable_field("salt", "not base64"))?;

    if bytes.len() != SALT_LEN {
        return Err(unusable_field(
            "salt",
            format_args!("{} bytes, not {SALT_LEN}", bytes.len()),
        ));
    }

    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&bytes);
    Ok(salt)
}

/// Derives the `SQLCipher` key from the master password and the header's
/// KDF parameters.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] when the header cannot be used: an unknown KDF, a
/// key length other than [`KEY_LEN`], Argon2 costs outside the accepted
/// range, or a salt that does not decode to [`SALT_LEN`] bytes.
/// [`Error::Crypto`] when Argon2 itself rejects the parameters or fails to
/// hash.
pub(super) fn derive_key(password: &str, header: &VaultHeader) -> Result<VaultKey> {
    if header.kdf != "argon2id" {
        return Err(unusable_field("key derivation function", &header.kdf));
    }

    if header.output_len != KEY_LEN {
        return Err(unusable_field("key length", header.output_len));
    }

    // 8 MiB to 1 GiB of memory, 1 to 8 passes, 1 to 4 lanes. The defaults in
    // `vault::header` sit inside; the ceiling is what stops an edited header
    // from making an unlock attempt exhaust the machine.
    if !(8_192..=1_048_576).contains(&header.m_cost)
        || !(1..=8).contains(&header.t_cost)
        || !(1..=4).contains(&header.p_cost)
    {
        return Err(unusable_field("argon2 costs", "out of range"));
    }

    let salt = decode_salt(header)?;
    let params = Params::new(header.m_cost, header.t_cost, header.p_cost, Some(KEY_LEN))
        .crypto("set key derivation parameters")?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    // Hash straight into the wiping wrapper. Filling a plain array and
    // moving it in afterwards would copy the key and leave the original
    // stack slot unwiped.
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(password.as_bytes(), &salt, key.as_mut_slice())
        .crypto("derive vault key")?;

    Ok(key)
}

/// Formats `key` as the raw-key literal `x'<64 hex digits>'` that `SQLCipher`
/// takes as the value of `PRAGMA key` and `PRAGMA rekey`.
///
/// The form tells `SQLCipher` to use the 32 bytes as the key itself instead
/// of hashing them as a passphrase
/// (<https://www.zetetic.net/sqlcipher/sqlcipher-api/>, "PRAGMA key", raw key
/// data).
///
/// # What is not wiped
///
/// The returned string wipes itself on drop, and it is built at its final
/// capacity so that no reallocation leaves a partial copy behind. That is
/// as far as this crate's control goes. The caller hands the string to
/// `rusqlite::Connection::pragma_update`, which copies it into the statement
/// text it builds (`Sql::push_value` into `Sql::buf`, a plain `String`, in
/// rusqlite 0.40 `src/pragma.rs`), runs the statement and drops that
/// `String` without wiping it. One hex copy of the key therefore stays in
/// freed heap memory until the allocator reuses it.
///
/// The way around a statement string is `sqlite3_key`, which takes the key
/// bytes directly. rusqlite has no safe wrapper for it, so calling it means
/// an `unsafe` FFI call, and the workspace sets `unsafe_code = "forbid"`.
#[must_use]
pub(super) fn key_to_sqlcipher_pragma(key: &VaultKey) -> Zeroizing<String> {
    // `x'` + two digits per byte + `'`.
    let mut hex = String::with_capacity(KEY_LEN * 2 + 3);
    hex.push_str("x'");
    for byte in key.iter() {
        // `fmt::Write` for `String` never returns an error, so there is
        // nothing to propagate.
        let _ = write!(hex, "{byte:02x}");
    }
    hex.push('\'');
    Zeroizing::new(hex)
}

/// The error for a header field whose value no build writes.
fn unusable_field(field: &'static str, detail: impl Display) -> Error {
    Error::VaultCorrupt(VaultCorruption::HeaderField {
        field,
        detail: detail.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::header::VaultHeader;

    #[test]
    fn derive_is_deterministic() {
        let salt = [7u8; SALT_LEN];
        let header = VaultHeader::new_with_salt(&salt);
        let first = derive_key("correct horse battery staple", &header).unwrap();
        let second = derive_key("correct horse battery staple", &header).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn derive_rejects_huge_m_cost() {
        let mut header = VaultHeader::new_with_salt(&[1u8; SALT_LEN]);
        header.m_cost = 50_000_000;
        let err = derive_key("correct horse battery staple", &header);
        assert!(matches!(err, Err(Error::VaultCorrupt(_))), "{err:?}");
    }

    #[test]
    fn derive_rejects_zero_t_cost() {
        let mut header = VaultHeader::new_with_salt(&[2u8; SALT_LEN]);
        header.t_cost = 0;
        let err = derive_key("correct horse battery staple", &header);
        assert!(matches!(err, Err(Error::VaultCorrupt(_))), "{err:?}");
    }

    #[test]
    fn different_password_different_key() {
        let salt = [9u8; SALT_LEN];
        let header = VaultHeader::new_with_salt(&salt);
        let first = derive_key("aaaaaaaaaaaa", &header).unwrap();
        let second = derive_key("bbbbbbbbbbbb", &header).unwrap();
        assert_ne!(first, second);
    }
}
