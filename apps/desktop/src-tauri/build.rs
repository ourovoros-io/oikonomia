fn main() {
    tauri_build::build();
    enforce_baked_updater_public_key();
}

/// Empty `plugins.updater.pubkey` must fail the desktop crate compile.
///
/// Ops bakes the minisign **public** key only. The private key is not in this
/// repository. A future blank-out is a CI fail.
fn enforce_baked_updater_public_key() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let text = std::fs::read_to_string(&path).expect("read tauri.conf.json");
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("parse tauri.conf.json");
    let Some(pubkey) = value
        .pointer("/plugins/updater/pubkey")
        .and_then(serde_json::Value::as_str)
    else {
        panic!("plugins.updater.pubkey is missing; Ops must bake the minisign public key");
    };
    if pubkey.trim().is_empty() {
        panic!(
            "plugins.updater.pubkey is empty; Ops must bake the release minisign public key"
        );
    }
}
