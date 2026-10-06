//! Minisign verify for the detached manifest and the artifact.

use crate::error::{Result, UpdateError};
use base64::Engine;
use minisign_verify::{PublicKey, Signature};

/// Parses a Tauri-format (base64 of the minisign public-key file) or raw minisign key.
///
/// # Errors
///
/// Returns [`UpdateError::MissingPublicKey`] when `raw` is empty or does not decode.
pub fn parse_public_key(raw: &str) -> Result<PublicKey> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(UpdateError::MissingPublicKey);
    }
    if let Ok(key) = PublicKey::decode(raw) {
        return Ok(key);
    }
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(raw.as_bytes()) else {
        return Err(UpdateError::MissingPublicKey);
    };
    let Ok(text) = String::from_utf8(decoded) else {
        return Err(UpdateError::MissingPublicKey);
    };
    let text = text.trim();
    PublicKey::decode(text).map_err(|_| UpdateError::MissingPublicKey)
}

/// Verifies `data` against a raw minisign signature or a Tauri base64-wrapped `.sig`.
///
/// # Errors
///
/// Returns [`UpdateError::ManifestSignature`] when the signature is missing or invalid.
pub fn verify_minisign(public_key: &PublicKey, data: &[u8], signature: &str) -> Result<()> {
    let signature = signature.trim();
    if signature.is_empty() {
        return Err(UpdateError::ManifestSignature);
    }
    let decoded = if let Ok(signature) = Signature::decode(signature) {
        signature
    } else {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(signature.as_bytes())
        else {
            return Err(UpdateError::ManifestSignature);
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return Err(UpdateError::ManifestSignature);
        };
        let text = text.trim();
        Signature::decode(text).map_err(|_| UpdateError::ManifestSignature)?
    };
    public_key
        .verify(data, &decoded, true)
        .map_err(|_| UpdateError::ManifestSignature)
}

/// Decodes a 64-character lowercase or mixed hex SHA-256 digest.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactIntegrity`] when the string is not 32 bytes of hex.
pub(crate) fn parse_sha256_hex(hex: &str) -> Result<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err(UpdateError::ArtifactIntegrity);
    }
    let mut out = [0_u8; 32];
    let bytes = hex.as_bytes();
    let mut index = 0;
    while index < 32 {
        let hi = hex_nibble(bytes[index * 2])?;
        let lo = hex_nibble(bytes[index * 2 + 1])?;
        out[index] = (hi << 4) | lo;
        index += 1;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(UpdateError::ArtifactIntegrity),
    }
}

/// Lowercase hex encoding for cache file names and tests.
#[must_use]
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Verify a detached minisign signature over raw manifest bytes.
///
/// # Errors
///
/// [`UpdateError::ManifestSignature`] when verification fails.
pub fn verify_manifest_bytes(key: &PublicKey, body: &[u8], signature: &str) -> Result<()> {
    verify_minisign(key, body, signature)
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::parse_sha256_hex;

    fn hex(digest: [u8; 32], uppercase: bool) -> String {
        digest
            .iter()
            .map(|byte| {
                if uppercase {
                    format!("{byte:02X}")
                } else {
                    format!("{byte:02x}")
                }
            })
            .collect()
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
