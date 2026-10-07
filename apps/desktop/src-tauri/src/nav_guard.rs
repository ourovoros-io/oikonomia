//! Keeps every webview on the app's own origin.
//!
//! The content security policy limits what a page may load, not where the
//! top frame may navigate; this hook closes that gap for every window,
//! including the tray quick-add panel.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Runtime, Url};

/// Returns the plugin that denies navigation to anything but the app's own
/// origins, in every webview.
#[must_use]
pub(crate) fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("nav-guard")
        .on_navigation(|_webview, url| is_app_url(url))
        .build()
}

/// Returns whether `url` is one of the app's own origins.
///
/// `blob:` is the document viewer's iframe (wry reports subframe navigations
/// through the same hook); `about:blank` is the empty document every webview
/// starts from.
fn is_app_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" | "blob" | "about" => true,
        "http" | "https" => is_app_host(url.host_str().unwrap_or_default()),
        _ => false,
    }
}

/// Returns whether `host` is one the app is served from.
///
/// `tauri.localhost` serves the bundle on Windows; `localhost` is the Vite
/// dev server, reachable only from `cargo tauri dev` builds.
fn is_app_host(host: &str) -> bool {
    host == "tauri.localhost" || (cfg!(dev) && host == "localhost")
}

#[cfg(test)]
mod tests {
    use tauri::Url;

    use super::is_app_url;

    /// Parses a URL a test wrote out.
    fn url(text: &str) -> Url {
        Url::parse(text).expect("test URL parses")
    }

    #[test]
    fn app_origins_are_allowed() {
        let allowed = [
            "tauri://localhost/index.html",
            "http://tauri.localhost/",
            "https://tauri.localhost/assets/index.js",
            "blob:tauri://localhost/8f3c",
            "about:blank",
        ];
        for text in allowed {
            assert!(is_app_url(&url(text)), "{text}");
        }
    }

    #[test]
    fn remote_and_local_file_origins_are_denied() {
        let denied = [
            "https://example.com/",
            "http://127.0.0.1:5173/",
            "file:///etc/passwd",
            "data:text/html,hello",
            "ftp://example.com/",
            "https://tauri.localhost.example.com/",
        ];
        for text in denied {
            assert!(!is_app_url(&url(text)), "{text}");
        }
    }

    #[test]
    fn vite_dev_server_is_allowed_only_in_dev_builds() {
        assert_eq!(is_app_url(&url("http://localhost:5173/")), cfg!(dev));
    }
}
