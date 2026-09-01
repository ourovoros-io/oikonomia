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

#[test]
fn bundle_identity_belongs_to_ourovoros() {
    let conf = config();
    assert_eq!(conf["identifier"], "io.ourovoros.oikonomia");
    assert_eq!(conf["bundle"]["publisher"], "Ourovoros.io");
    assert_eq!(
        conf["bundle"]["copyright"],
        "Copyright (c) 2026 Ourovoros.io"
    );
    // Tauri's bundle.category takes the shorthand name ("Finance"); if
    // `cargo tauri build --bundles app` rejects it, switch BOTH the config
    // and this assertion to "public.app-category.finance".
    assert_eq!(conf["bundle"]["category"], "Finance");
    assert_eq!(conf["bundle"]["macOS"]["minimumSystemVersion"], "12.0");
    assert!(
        conf["bundle"]["shortDescription"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "shortDescription must be set"
    );
}

#[test]
fn versions_are_in_sync_everywhere() {
    let conf = config();
    let workspace = env!("CARGO_PKG_VERSION");
    assert_eq!(conf["version"], workspace, "tauri.conf.json vs workspace");

    let package: serde_json::Value =
        serde_json::from_str(include_str!("../../../../web/package.json"))
            .expect("web/package.json is valid JSON");
    assert_eq!(
        package["version"], workspace,
        "web/package.json vs workspace"
    );
}
