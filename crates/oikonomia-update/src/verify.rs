//! Signature and digest checks for the feed and the artifact.
//!
//! Two independent checks stand between a download and the installer. The
//! minisign signature says who published the bytes: it is made with the
//! updater secret key, which only the release workflow holds, and verified
//! here with the public key built into the application. The SHA-256 says
//! which bytes the signed manifest meant: the manifest states the digest of
//! each artifact, so a different file signed with the same key (an older
//! release, say) is still refused.
//!
//! Keys and signatures arrive in two spellings. Minisign writes a text file
//! of a comment line and a base64 line. The Tauri signer, which the release
//! workflow uses, writes the base64 of that whole file. Both are accepted
//! everywhere, so a key or signature can be pasted in either form.

use crate::error::{Result, UpdateError};
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

/// The number of bytes in a SHA-256 digest.
const SHA256_BYTES: usize = 32;

/// Parses the updater public key: a minisign public-key file, or the base64
/// of one as the Tauri signer writes it.
///
/// # Errors
///
/// Returns [`UpdateError::MissingPublicKey`] when `raw` is empty or is a
/// minisign key in neither form.
pub(crate) fn parse_public_key(raw: &str) -> Result<PublicKey> {
    decode_plain_or_base64(raw.trim(), |text| PublicKey::decode(text).ok())
        .ok_or(UpdateError::MissingPublicKey)
}

/// Verifies `data` against `signature`: a minisign signature file, or the
/// base64 of one as the Tauri signer writes a `.sig`.
///
/// Only prehashed signatures are accepted, the kind the Tauri signer
/// writes; a test in this module verifies one it made. `minisign-verify`
/// documents the `allow_legacy` argument of `PublicKey::verify` as one that
/// "should only be set to `true` in order to support signatures made by
/// older versions of Minisign", and nothing in the release lane makes one.
///
/// # Errors
///
/// Returns [`UpdateError::ManifestSignature`] when `signature` is empty, is a
/// minisign signature in neither form, was made with another key or in the
/// legacy mode, or does not verify `data`. The caller maps it to
/// [`UpdateError::ArtifactIntegrity`] when `data` is an artifact.
pub(crate) fn verify_minisign(public_key: &PublicKey, data: &[u8], signature: &str) -> Result<()> {
    let signature = decode_plain_or_base64(signature.trim(), |text| Signature::decode(text).ok())
        .ok_or(UpdateError::ManifestSignature)?;

    public_key
        .verify(data, &signature, false)
        .map_err(|_| UpdateError::ManifestSignature)
}

/// Verifies `data` against a minisign signature with the key in `public_key`.
///
/// This is the client's own check, offered to the release lane so that a
/// feed is tested before it is published with the code that will read it.
/// The key and the signature are each accepted as a minisign file or as the
/// base64 of one; the key is passed as text so that no type of the minisign
/// library appears in this crate's interface.
///
/// # Errors
///
/// Returns [`UpdateError::MissingPublicKey`] when `public_key` is empty or
/// not a minisign public key, and [`UpdateError::ManifestSignature`] when
/// `signature` is empty, is not a minisign signature, or does not verify
/// `data` with that key.
pub fn verify_signature(public_key: &str, data: &[u8], signature: &str) -> Result<()> {
    let public_key = parse_public_key(public_key)?;

    verify_minisign(&public_key, data, signature)
}

/// Returns the lowercase hex SHA-256 of `bytes`, the form `latest.json` and
/// `SHA256SUMS` hold.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&sha256(bytes))
}

/// Returns the SHA-256 of `bytes`.
#[must_use]
pub(crate) fn sha256(bytes: &[u8]) -> [u8; SHA256_BYTES] {
    Sha256::digest(bytes).into()
}

/// Parses a SHA-256 digest written as 64 hex characters of either case,
/// ignoring surrounding whitespace.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactIntegrity`] when the text is not exactly
/// 64 hex characters.
pub(crate) fn parse_sha256_hex(hex: &str) -> Result<[u8; SHA256_BYTES]> {
    let hex = hex.trim().as_bytes();
    if hex.len() != SHA256_BYTES * 2 {
        return Err(UpdateError::ArtifactIntegrity);
    }
    // The length is even, so the pairs cover every byte and none is left over.
    let (pairs, _) = hex.as_chunks::<2>();

    let mut digest = [0_u8; SHA256_BYTES];
    for (byte, &[high, low]) in digest.iter_mut().zip(pairs) {
        *byte = (hex_digit(high)? << 4) | hex_digit(low)?;
    }

    Ok(digest)
}

/// Returns `bytes` as lowercase hex, two characters per byte.
#[must_use]
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    bytes
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(DIGITS[usize::from(nibble)]))
        .collect()
}

/// Decodes `text` with `decode`, either as it stands or, failing that, after
/// unwrapping one layer of standard base64.
///
/// Returns `None` when `text` is empty or decodes in neither form.
fn decode_plain_or_base64<T>(text: &str, decode: impl Fn(&str) -> Option<T>) -> Option<T> {
    if text.is_empty() {
        return None;
    }

    decode(text).or_else(|| {
        let unwrapped = base64::engine::general_purpose::STANDARD
            .decode(text)
            .ok()?;
        let unwrapped = String::from_utf8(unwrapped).ok()?;

        decode(unwrapped.trim())
    })
}

/// Returns the value of one ASCII hex digit of either case.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactIntegrity`] when `digit` is not a hex digit.
fn hex_digit(digit: u8) -> Result<u8> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(UpdateError::ArtifactIntegrity),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_public_key, sha256_hex, to_hex, verify_signature};

    /// A public key, a file and its signature as `tauri signer` 2.12.1 (the
    /// version pinned in `web/package.json`) wrote them, made with a key
    /// generated for this fixture and then discarded.
    const TAURI_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQ0Q0I4RDg2ODhGQTQ4NzgKUldSNFNQcUlobzNMMUZHVVBmblZ4T2xxT2dlenB3NC96S0dFRDY2cGp4YTliR1JIVndaRzhxQk4K";
    const TAURI_SIGNED_BYTES: &[u8] = b"bytes of an artifact signed by the pinned Tauri CLI\n";
    const TAURI_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSNFNQcUlobzNMMUI0K3Qra0tlOXp4anIzMkRYVVY3V2hxaVIxcnBwZWdENkRCaEt0bk1RZHJHcks1WkVqdmFnMHBBRjJiMUc3TzdhSzBNeUVJdmdWWlJ6cGpGNzVDa1FjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxMzEzMDcwCWZpbGU6YXJ0aWZhY3QuYmluClVOUnFiOHM1WlZ6TEVrY0VqVDVubitqWmhlYWNGTytabkdkOGpzSWhwenRDU3lkODFUZHVpRFg5VGliMXhySWNENGxXSFZ1VFd6SUV0K1BSTmJ2TkN3PT0K";

    #[test]
    fn a_signature_written_by_the_tauri_signer_verifies() {
        // Pins that the signer's output is the prehashed kind, the only one
        // `verify_minisign` accepts.
        verify_signature(TAURI_PUBLIC_KEY, TAURI_SIGNED_BYTES, TAURI_SIGNATURE)
            .expect("a Tauri signature must verify");
    }

    #[test]
    fn a_tauri_signature_does_not_verify_other_bytes() {
        let err = verify_signature(TAURI_PUBLIC_KEY, b"other bytes", TAURI_SIGNATURE)
            .expect_err("other bytes");

        assert_eq!(err.code(), "update_manifest_signature");
    }

    #[test]
    fn an_empty_or_malformed_signature_is_refused() {
        for signature in ["", "  \n", "not a signature", "bm90IGEgc2lnbmF0dXJl"] {
            let err = verify_signature(TAURI_PUBLIC_KEY, TAURI_SIGNED_BYTES, signature)
                .expect_err(signature);

            assert_eq!(err.code(), "update_manifest_signature", "{signature:?}");
        }
    }

    #[test]
    fn empty_public_key_is_rejected() {
        let err = parse_public_key("").expect_err("empty");
        assert_eq!(err.code(), "update_missing_public_key");
        let err = parse_public_key("   ").expect_err("whitespace");
        assert_eq!(err.code(), "update_missing_public_key");
    }

    #[test]
    fn raw_ed25519_hex_is_not_a_minisign_public_key() {
        const RAW_KEY_HEX: &str =
            "7d5b038e9ab30eef536cc559baac20e44070adedcdf548af48744029804ec671";
        let err = parse_public_key(RAW_KEY_HEX).expect_err("raw hex key");
        assert_eq!(err.code(), "update_missing_public_key");
        assert_eq!(err.to_string(), "updater public key is missing or invalid");
    }

    #[test]
    fn sha256_hex_matches_the_published_test_vector() {
        // FIPS 180-2, appendix B.1: the digest of "abc".
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hex_is_lowercase_and_two_characters_per_byte() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
        assert_eq!(to_hex(&[]), "");
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::{parse_sha256_hex, to_hex};

    fn hex(digest: [u8; 32], uppercase: bool) -> String {
        let lowercase = to_hex(&digest);
        if uppercase {
            lowercase.to_uppercase()
        } else {
            lowercase
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn a_digest_written_as_hex_parses_back(
            digest in any::<[u8; 32]>(),
            uppercase in any::<bool>(),
        ) {
            let parsed = parse_sha256_hex(&hex(digest, uppercase));

            prop_assert!(matches!(parsed, Ok(bytes) if bytes == digest), "{:?}", parsed);
        }

        #[test]
        fn parsing_any_text_returns_instead_of_panicking(text in any::<String>()) {
            let _ = parse_sha256_hex(&text);
        }

        #[test]
        fn parsing_64_bytes_that_are_not_all_hex_returns_instead_of_panicking(
            text in "[0-9a-fA-Fg-z ]{64}",
        ) {
            let _ = parse_sha256_hex(&text);
        }

        // 64 bytes, so the length check passes, with a two-byte character at
        // each position in turn for the byte-wise decoding to meet.
        #[test]
        fn parsing_64_bytes_with_a_wide_character_is_refused(at in 0_usize..=62) {
            let text = format!("{}\u{e9}{}", "a".repeat(at), "a".repeat(62 - at));
            prop_assert_eq!(text.len(), 64);

            prop_assert!(parse_sha256_hex(&text).is_err());
        }
    }
}
