//! Runs the Tauri build step and refuses to compile without an updater key.

/// Runs the Tauri build step, then checks the updater key.
fn main() {
    tauri_build::build();
    enforce_baked_updater_public_key();
}

/// Fails the build when `plugins.updater.pubkey` in `tauri.conf.json` is
/// empty.
///
/// Only the minisign public key is in this repository; the private key is
/// not. A shipped build without the public key could not verify an update,
/// so a blank value has to stop the build, not reach a release.
///
/// # Panics
///
/// Panics, which fails the build, when the configuration cannot be read or
/// parsed, or the key is empty.
#[expect(clippy::expect_used, reason = "empty updater pubkey must fail compile")]
fn enforce_baked_updater_public_key() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let text = std::fs::read_to_string(&path).expect("read tauri.conf.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse tauri.conf.json");
    let pubkey = value
        .pointer("/plugins/updater/pubkey")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    assert!(
        !pubkey.trim().is_empty(),
        "plugins.updater.pubkey is empty; Ops must bake the release minisign public key"
    );
}
