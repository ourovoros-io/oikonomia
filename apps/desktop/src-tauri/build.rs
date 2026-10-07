//! Runs the Tauri build step and refuses to compile without an updater key.

fn main() {
    tauri_build::build();
    enforce_baked_updater_public_key();
}

/// Empty `plugins.updater.pubkey` must fail the desktop crate compile.
///
/// Ops bakes the minisign **public** key only. The private key is not in this
/// repository. A future blank-out is a CI fail.
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
