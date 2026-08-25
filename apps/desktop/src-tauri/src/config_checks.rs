//! Guards for the security-relevant parts of `tauri.conf.json`.
//!
//! The app promises to never reach the network. These tests make loosening
//! the content security policy or the Windows installer a deliberate,
//! reviewed change rather than a silent one.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

const CONFIG: &str = include_str!("../tauri.conf.json");

fn config() -> serde_json::Value {
    serde_json::from_str(CONFIG).expect("tauri.conf.json is valid JSON")
}

#[test]
fn windows_installer_bundles_the_webview2_runtime_offline() {
    let mode = &config()["bundle"]["windows"]["webviewInstallMode"]["type"];
    assert_eq!(mode, "offlineInstaller");
}
