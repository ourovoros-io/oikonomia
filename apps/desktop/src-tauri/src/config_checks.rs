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
fn windows_installer_embeds_the_webview2_bootstrapper() {
    // Tauri 2 (tauri-utils 2.10) schema: `type` is the serde tag, and
    // `silent` defaults to true for every mode that runs an installer.
    // embedBootstrapper puts Microsoft's bootstrapper in the NSIS setup.
    // Windows 11 ships WebView2 and Windows 10 receives it through Windows
    // Update, so the bootstrapper downloads the runtime only when it is
    // missing. The full offline runtime made the setup about 224 MiB, which
    // the update client refuses.
    let mode = &config()["bundle"]["windows"]["webviewInstallMode"];
    assert_eq!(mode["type"], "embedBootstrapper");
    let silent = mode.get("silent");
    assert!(
        silent.is_none_or(|value| value.as_bool() == Some(true)),
        "webview bootstrapper must stay silent, got {silent:?}"
    );
}

#[test]
fn windows_installs_per_user_so_updates_need_no_elevation() {
    // `update_exec` starts the new installer as a plain child process. A
    // per-machine install would need elevation, which that spawn cannot ask
    // for, and every update would fail.
    let mode = &config()["bundle"]["windows"]["nsis"]["installMode"];
    assert_eq!(mode, "currentUser");
}

#[test]
fn bundle_targets_are_exactly_the_formats_the_updater_and_release_know() {
    // No MSI and no RPM: nothing in the update path or the release
    // workflows handles them, so they must not be built by accident.
    let targets = &config()["bundle"]["targets"];
    assert_eq!(
        targets,
        &serde_json::json!(["app", "dmg", "deb", "appimage", "nsis"])
    );
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
fn webview_has_no_opener_permission() {
    // Nothing in the UI opens a URL any more. The support mailto is built
    // and opened from Rust (`open_support_email`), whose plugin API is not
    // capability-scoped, so the webview needs no opener grant at all.
    let capabilities: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("capabilities json");

    let opener: Vec<String> = capabilities["permissions"]
        .as_array()
        .expect("permissions array")
        .iter()
        .filter_map(|permission| {
            let identifier = permission
                .as_str()
                .or_else(|| permission["identifier"].as_str())?;
            identifier
                .starts_with("opener:")
                .then(|| identifier.to_owned())
        })
        .collect();

    assert!(opener.is_empty(), "webview opener permissions: {opener:?}");
}

#[test]
fn the_first_run_language_command_is_registered() {
    let registrations = include_str!("lib.rs");

    assert!(
        registrations.contains("commands::settings_resolve_locale,"),
        "settings_resolve_locale is not in the command list"
    );
}

#[test]
fn an_app_command_needs_no_new_webview_permission() {
    // App commands registered in the invoke handler are callable by every
    // window in the capability; the permission list stays exactly as audited.
    let capabilities: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("capabilities json");

    let permissions: Vec<&str> = capabilities["permissions"]
        .as_array()
        .expect("permissions array")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();

    assert_eq!(
        permissions,
        [
            "core:default",
            "core:event:default",
            "core:window:allow-show",
            "core:window:allow-hide",
            "core:window:allow-close",
            "core:window:allow-set-focus",
            "core:window:allow-set-size",
            "core:window:allow-set-position",
            "core:window:allow-outer-position",
            "core:window:allow-outer-size",
            "core:window:allow-is-visible",
        ]
    );
}

#[test]
fn no_licensing_commands_are_registered() {
    let registrations = include_str!("lib.rs");

    for command in ["license_status", "license_install", "eula_text"] {
        assert!(
            !registrations.contains(command),
            "{command} is still registered"
        );
    }
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

#[test]
fn bundle_ships_the_gpl_licence_text() {
    assert_eq!(config()["bundle"]["licenseFile"], "../../../LICENSE");

    let licence = include_str!("../../../../LICENSE");
    assert!(
        licence
            .trim_start()
            .starts_with("GNU GENERAL PUBLIC LICENSE"),
        "LICENSE is not the GPL text"
    );
    assert!(licence.contains("Version 3, 29 June 2007"));

    assert_eq!(env!("CARGO_PKG_LICENSE"), "GPL-3.0-or-later");
}
