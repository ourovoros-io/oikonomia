//! Guards for the security-relevant parts of `tauri.conf.json`.
//!
//! The app promises to never reach the network. These tests make loosening
//! the content security policy or the Windows installer a deliberate,
//! reviewed change rather than a silent one.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use std::collections::BTreeMap;

const CONFIG: &str = include_str!("../tauri.conf.json");

fn config() -> serde_json::Value {
    serde_json::from_str(CONFIG).expect("tauri.conf.json is valid JSON")
}

/// CSP directives as `name -> sources`.
fn csp_directives() -> BTreeMap<String, Vec<String>> {
    let csp = config()["app"]["security"]["csp"]
        .as_str()
        .expect("app.security.csp is a string")
        .to_owned();

    csp.split(';')
        .filter_map(|directive| {
            let mut parts = directive.split_whitespace();
            let name = parts.next()?;
            Some((name.to_owned(), parts.map(str::to_owned).collect()))
        })
        .collect()
}

#[test]
fn csp_lets_the_webview_talk_only_to_the_ipc_bridge() {
    let directives = csp_directives();

    assert_eq!(
        directives["connect-src"],
        ["'self'", "ipc:", "http://ipc.localhost"]
    );

    let remote: Vec<&String> = directives
        .values()
        .flatten()
        .filter(|source| source.starts_with("http") && *source != "http://ipc.localhost")
        .collect();
    assert!(remote.is_empty(), "remote CSP sources: {remote:?}");
}

#[test]
fn csp_closes_plugin_base_and_form_escape_hatches() {
    let directives = csp_directives();

    assert_eq!(directives["object-src"], ["'none'"]);
    assert_eq!(directives["base-uri"], ["'self'"]);
    assert_eq!(directives["form-action"], ["'none'"]);
    assert_eq!(directives["frame-ancestors"], ["'none'"]);
}

#[test]
fn csp_allows_the_document_viewer_blob_urls_explicitly() {
    let directives = csp_directives();

    // Chromium (Windows WebView2) follows CSP3: 'self' never matches blob:.
    assert!(directives["img-src"].iter().any(|source| source == "blob:"));
    assert_eq!(directives["frame-src"], ["blob:"]);
}

#[test]
fn windows_installer_bundles_the_webview2_runtime_offline() {
    let mode = &config()["bundle"]["windows"]["webviewInstallMode"]["type"];
    assert_eq!(mode, "offlineInstaller");
}
