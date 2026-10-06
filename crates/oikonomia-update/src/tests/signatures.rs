//! What the signature check refuses, and with which error code.
//!
//! The signer in these tests (`minisign`) and the verifier in the app
//! (`minisign-verify`) are separate crates that Dependabot bumps on their own.
//! The tests here pin what the verifier refuses, so a bump of either one
//! cannot change it unnoticed.

use crate::tests::support::{sign, test_keys};
use crate::verify::{parse_public_key, verify_minisign};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign::SecretKey;

/// Signs `data` and returns the four lines of the minisign signature, so a
/// test can change one of them.
fn signature_lines(secret_key: &SecretKey, data: &[u8]) -> Vec<String> {
    sign(secret_key, data).lines().map(str::to_owned).collect()
}

/// Returns `lines` as the text of a signature file, each line ended.
fn joined(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[test]
fn one_flipped_byte_fails_the_signature_in_raw_and_tauri_form() {
    let (public_key, secret_key) = test_keys();
    let key = parse_public_key(&public_key).expect("public key");
    let artifact = b"artifact bytes".to_vec();
    let raw = sign(&secret_key, &artifact);
    let tauri = STANDARD.encode(&raw);
    verify_minisign(&key, &artifact, &raw).expect("raw signature verifies");
    verify_minisign(&key, &artifact, &tauri).expect("tauri signature verifies");

    for index in [0, artifact.len() - 1] {
        let mut tampered = artifact.clone();
        tampered[index] ^= 1;
        for signature in [&raw, &tauri] {
            let err = verify_minisign(&key, &tampered, signature).expect_err("flipped byte");
            assert_eq!(err.code(), "update_manifest_signature");
        }
    }
}

#[test]
fn signature_from_another_key_fails_even_under_this_key_id() {
    let (public_key, secret_key) = test_keys();
    let (_other_public_key, other_secret_key) = test_keys();
    let key = parse_public_key(&public_key).expect("public key");
    let data = b"manifest";

    let err = verify_minisign(&key, data, &sign(&other_secret_key, data)).expect_err("another key");
    assert_eq!(err.code(), "update_manifest_signature");

    // A different key id is refused before any Ed25519 check. Give the other
    // key's signature this key's id so the Ed25519 check is what refuses it.
    let ours = STANDARD
        .decode(&signature_lines(&secret_key, data)[1])
        .expect("signature line");
    let mut forged = signature_lines(&other_secret_key, data);
    let mut bytes = STANDARD.decode(&forged[1]).expect("signature line");
    bytes[2..10].copy_from_slice(&ours[2..10]);
    forged[1] = STANDARD.encode(&bytes);
    let err = verify_minisign(&key, data, &joined(&forged)).expect_err("forged key id");
    assert_eq!(err.code(), "update_manifest_signature");
}

#[test]
fn changed_trusted_comment_fails_the_signature() {
    let (public_key, secret_key) = test_keys();
    let key = parse_public_key(&public_key).expect("public key");
    let data = b"manifest";
    let mut lines = signature_lines(&secret_key, data);
    let changed = "trusted comment: timestamp:0\tfile:latest.json".to_owned();
    assert_ne!(lines[2], changed);
    lines[2] = changed;

    let raw = joined(&lines);
    for signature in [STANDARD.encode(&raw), raw] {
        let err = verify_minisign(&key, data, &signature).expect_err("changed comment");
        assert_eq!(err.code(), "update_manifest_signature");
    }
}

#[test]
fn malformed_signature_is_refused() {
    let (public_key, secret_key) = test_keys();
    let key = parse_public_key(&public_key).expect("public key");
    let data = b"manifest";
    let lines = signature_lines(&secret_key, data);
    let mut truncated = lines.clone();
    let keep = truncated[1].len() - 8;
    truncated[1].truncate(keep);
    let mut unknown_algorithm = lines.clone();
    let mut bytes = STANDARD
        .decode(&unknown_algorithm[1])
        .expect("signature line");
    bytes[..2].copy_from_slice(b"XX");
    unknown_algorithm[1] = STANDARD.encode(&bytes);

    for (case, signature) in [
        ("no trusted comment", joined(&lines[..2])),
        ("truncated signature", joined(&truncated)),
        ("unknown algorithm", joined(&unknown_algorithm)),
        ("not a signature", "not a signature".to_owned()),
        ("base64 of text", STANDARD.encode("not a signature")),
    ] {
        let err = verify_minisign(&key, data, &signature).expect_err(case);
        assert_eq!(err.code(), "update_manifest_signature", "{case}");
    }
}
