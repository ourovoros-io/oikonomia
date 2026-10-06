//! App identity, window and support-mail commands.

use crate::error::{CommandError, CommandResult, DesktopError};
use serde::Serialize;

/// Where users send questions and bug reports. Every support pointer the app
/// shows derives from this one address.
pub(crate) const SUPPORT_EMAIL: &str = "info@ourovoros.io";

/// Static app metadata for the about screen / diagnostics.
#[derive(Debug, Serialize)]
pub(crate) struct AppInfo {
    /// Crate version.
    pub version: &'static str,
    /// Product name.
    pub name: &'static str,
    /// Support mailbox, shown verbatim so a user can copy it. Opening it is
    /// [`open_support_email`]'s job; the webview never builds the URL.
    pub support_email: &'static str,
}

/// Return build identity and the support address (no secrets).
#[tauri::command]
pub(crate) fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        name: "Oikonomia",
        support_email: SUPPORT_EMAIL,
    }
}

/// Open the default mail client on the support mailbox.
///
/// The URL is built here from [`SUPPORT_EMAIL`] and handed to the opener's
/// Rust API, which applies no capability scope. That is deliberate: the
/// webview never supplies a URL, so `capabilities/default.json` needs no
/// `mailto:` glob, and a glob that could admit extra recipients never exists.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn open_support_email(app: tauri::AppHandle) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;

    app.opener()
        .open_url(support_mailto(env!("CARGO_PKG_VERSION")), None::<&str>)
        .map_err(|err| {
            CommandError::desktop(
                DesktopError::MailClientFailed,
                format!("could not open the mail client: {err}"),
            )
        })
}

/// `mailto:` link to [`SUPPORT_EMAIL`] whose subject names the app version,
/// so every support thread opens with the one fact each report needs.
fn support_mailto(version: &str) -> String {
    let subject = percent_encode(&format!("Oikonomia v{version} support"));
    format!("mailto:{SUPPORT_EMAIL}?subject={subject}")
}

/// RFC 3986 percent-encoding for a `mailto:` query value: unreserved bytes
/// pass through, everything else becomes `%XX`.
fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
    out
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn open_main_window(app: tauri::AppHandle) {
    crate::tray::show_main_window(&app);
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn quick_add_hide(app: tauri::AppHandle) {
    crate::tray::hide_quick_add(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_carries_the_support_address_but_never_a_url() {
        let json = serde_json::to_value(app_info()).expect("serialize");
        assert_eq!(json["support_email"], "info@ourovoros.io");
        // The webview displays the address; only Rust turns it into a URL.
        assert!(json.get("support_mailto").is_none());
    }

    #[test]
    fn support_mailto_targets_the_support_mailbox_with_the_version() {
        let mailto = support_mailto("1.2.3");
        assert!(mailto.starts_with(&format!("mailto:{SUPPORT_EMAIL}?")));
        assert_eq!(
            mailto,
            "mailto:info@ourovoros.io?subject=Oikonomia%20v1.2.3%20support"
        );
    }

    #[test]
    fn percent_encode_keeps_unreserved_bytes_and_escapes_the_rest() {
        assert_eq!(percent_encode("a-b.c_d~1"), "a-b.c_d~1");
        assert_eq!(percent_encode("a b&c=d?é"), "a%20b%26c%3Dd%3F%C3%A9");
    }
}
