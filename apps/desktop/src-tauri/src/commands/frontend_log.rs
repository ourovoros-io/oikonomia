//! Writes a webview crash to the local error log.
//!
//! A page that fails to render, or an error nothing caught, used to reach
//! only the webview console, which a release build's user never opens. The
//! webview now reports it here, and the line lands in the size-capped log
//! (`crate::error_log`) at the error level.
//!
//! # Nothing from the ledger
//!
//! The log must never hold ledger data, and the webview is the one place that
//! holds it decrypted. So the command accepts only what it can reduce:
//!
//! - where it happened is a [`FrontendLocation`], a closed set that serde
//!   checks, so an unknown value is refused before the command runs;
//! - the error's name is cut to identifier characters;
//! - the message is reduced by [`sanitize_message`].
//!
//! The component stack is deliberately not accepted: the location already
//! says which page failed, and a stack adds only more text to filter.
//!
//! The message filter is conservative, not a proof. A JavaScript engine's own
//! messages name properties and types ("Cannot read properties of
//! undefined"), which is what makes a line useful, but any code that
//! interpolates a value into an error puts that value in the message. The
//! filter therefore keeps only the first line, drops everything inside quotes
//! (the usual way a value is shown), turns every run of four or more digits
//! (amounts in minor units, dates, ids, card numbers) into `#`, keeps
//! printable ASCII only (so a book or account named in Greek never reaches the
//! file), and cuts the rest at [`MAX_MESSAGE_CHARS`]. A short ASCII word
//! outside quotes would pass. The app's own code does not throw errors that
//! carry ledger text; this filter limits the damage if one ever does.

use serde::Deserialize;

/// Longest message kept, in characters. By then they are all ASCII, so also
/// in bytes.
const MAX_MESSAGE_CHARS: usize = 200;

/// Longest error name kept, in characters.
const MAX_NAME_CHARS: usize = 40;

/// The name logged for an error whose name has no usable characters.
const FALLBACK_NAME: &str = "Error";

/// Digit runs at least this long are replaced: they are the shape of an
/// amount in minor units, a date, an id or a card number.
const MASKED_DIGIT_RUN: usize = 4;

/// Where in the webview the error happened: a page of the main window, or the
/// window outside any page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FrontendLocation {
    /// The dashboard page.
    Dashboard,
    /// The transactions page.
    Transactions,
    /// The documents page.
    Documents,
    /// The accounts page.
    Accounts,
    /// The reports page.
    Reports,
    /// The settings page.
    Settings,
    /// Outside any page: an uncaught error or an unhandled rejection.
    Window,
}

impl FrontendLocation {
    /// Returns the word the log line shows.
    fn as_str(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Transactions => "transactions",
            Self::Documents => "documents",
            Self::Accounts => "accounts",
            Self::Reports => "reports",
            Self::Settings => "settings",
            Self::Window => "window",
        }
    }
}

/// Logs an error the webview caught, at the error level.
///
/// Holds no secret and needs no vault. It never fails: a log that cannot be
/// written has nowhere to be reported (`crate::error_log`).
#[tauri::command]
pub(crate) fn log_frontend_error(location: FrontendLocation, name: &str, message: &str) {
    log::error!("{}", log_text(location, name, message));
}

/// Returns the text of the log line for a webview error.
fn log_text(location: FrontendLocation, name: &str, message: &str) -> String {
    format!(
        "webview error in {}: {}: {}",
        location.as_str(),
        sanitize_name(name),
        sanitize_message(message)
    )
}

/// Returns `name` reduced to ASCII letters, digits, `_` and `$` (what a
/// JavaScript identifier is made of), at most [`MAX_NAME_CHARS`] long, or
/// [`FALLBACK_NAME`] when nothing is left.
fn sanitize_name(name: &str) -> String {
    let identifier: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        .collect();
    let name: String = mask_digit_runs(&identifier)
        .chars()
        .take(MAX_NAME_CHARS)
        .collect();

    if name.is_empty() {
        FALLBACK_NAME.to_owned()
    } else {
        name
    }
}

/// Returns the first line of `message` with quoted text and long digit runs
/// removed and anything but printable ASCII dropped, at most
/// [`MAX_MESSAGE_CHARS`] long. The module docs say why.
fn sanitize_message(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or("");
    let ascii: String = drop_quoted(first_line)
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect();

    mask_digit_runs(&ascii)
        .chars()
        .take(MAX_MESSAGE_CHARS)
        .collect()
}

/// Returns `text` with every quoted part emptied, so `reading 'balance'`
/// becomes `reading ''`. A quote that is never closed drops the rest of the
/// text: a value after it cannot be told from a word.
fn drop_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut open: Option<char> = None;

    for c in text.chars() {
        match open {
            Some(quote) if c == quote => {
                out.push(c);
                open = None;
            }
            Some(_) => {}
            None => {
                out.push(c);
                if matches!(c, '\'' | '"' | '`') {
                    open = Some(c);
                }
            }
        }
    }
    out
}

/// Returns `text` with every run of [`MASKED_DIGIT_RUN`] or more ASCII digits
/// replaced by a single `#`, and every shorter run by zeros of its length.
/// The short digits go too: which digits a message holds is not worth the
/// risk, and their count is enough to read a line.
fn mask_digit_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0;

    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
            continue;
        }
        flush_digits(&mut out, run);
        run = 0;
        out.push(c);
    }
    flush_digits(&mut out, run);
    out
}

/// Appends the stand-in for a run of `run` digits to `out`.
fn flush_digits(out: &mut String, run: usize) {
    if run >= MASKED_DIGIT_RUN {
        out.push('#');
    } else {
        out.extend(std::iter::repeat_n('0', run));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn a_plain_engine_message_survives() {
        assert_eq!(
            sanitize_message("Cannot read properties of undefined"),
            "Cannot read properties of undefined"
        );
    }

    #[test]
    fn only_the_first_line_is_kept() {
        assert_eq!(sanitize_message("first\nRent 1200 EUR"), "first");
        assert_eq!(sanitize_message("first\r\nsecond"), "first");
        assert_eq!(sanitize_message("\nsecond"), "");
    }

    #[test]
    fn digits_are_masked_by_run_length() {
        assert_eq!(sanitize_message("total 1250 due"), "total # due");
        assert_eq!(sanitize_message("on 2026-10-08"), "on #-00-00");
        assert_eq!(sanitize_message("at 123 of 99"), "at 000 of 00");
        assert_eq!(sanitize_message("id 12345678901234"), "id #");
    }

    #[test]
    fn quoted_text_is_dropped_whatever_the_quote() {
        assert_eq!(
            sanitize_message("reading 'balance' of \"Groceries\" and `x`"),
            "reading '' of \"\" and ``"
        );
    }

    #[test]
    fn an_unclosed_quote_drops_the_rest() {
        assert_eq!(sanitize_message("bad token 'Rent paid"), "bad token '");
    }

    #[test]
    fn non_ascii_text_is_dropped() {
        assert_eq!(sanitize_message("Ενοίκιο unpaid"), " unpaid");
        assert_eq!(sanitize_message("tab\there"), "tabhere");
    }

    #[test]
    fn a_long_message_is_cut() {
        assert_eq!(sanitize_message(&"a".repeat(1000)).len(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn a_name_is_an_identifier_or_the_fallback() {
        assert_eq!(sanitize_name("TypeError"), "TypeError");
        assert_eq!(sanitize_name("Rent 1250!"), "Rent#");
        assert_eq!(sanitize_name("Ενοίκιο"), FALLBACK_NAME);
        assert_eq!(sanitize_name(""), FALLBACK_NAME);
        assert_eq!(sanitize_name(&"N".repeat(500)).len(), MAX_NAME_CHARS);
    }

    #[test]
    fn the_line_names_the_location_name_and_message() {
        assert_eq!(
            log_text(
                FrontendLocation::Reports,
                "RangeError",
                "Invalid array length"
            ),
            "webview error in reports: RangeError: Invalid array length"
        );
    }

    #[test]
    fn locations_come_from_a_closed_set() {
        let known: FrontendLocation = serde_json::from_str("\"dashboard\"").unwrap();
        assert_eq!(known, FrontendLocation::Dashboard);
        assert!(serde_json::from_str::<FrontendLocation>("\"Rent\"").is_err());
    }

    proptest! {
        #[test]
        fn any_message_stays_within_the_cap_and_printable(message in any::<String>()) {
            let out = sanitize_message(&message);

            prop_assert!(out.len() <= MAX_MESSAGE_CHARS);
            prop_assert!(out.chars().all(|c| c.is_ascii_graphic() || c == ' '));
        }

        #[test]
        fn no_digit_run_of_four_survives(
            // Digit-heavy on purpose: arbitrary strings rarely hold four digits in a row.
            message in "[0-9a-z '\n]{0,80}",
            name in "[0-9A-Za-z]{0,60}",
        ) {
            for text in [sanitize_message(&message), sanitize_name(&name)] {
                let longest = text
                    .split(|c: char| !c.is_ascii_digit())
                    .map(str::len)
                    .max()
                    .unwrap_or(0);
                prop_assert!(longest < MASKED_DIGIT_RUN);
            }
        }

        #[test]
        fn any_name_is_a_short_identifier(name in any::<String>()) {
            let out = sanitize_name(&name);

            prop_assert!(!out.is_empty() && out.chars().count() <= MAX_NAME_CHARS);
        }

        #[test]
        fn a_quoted_value_never_reaches_the_line(secret in "[A-Za-z]{6,12}") {
            let out = sanitize_message(&format!("bad value '{secret}' here"));

            prop_assert!(!out.contains(&secret));
        }
    }
}

#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use super::*;
    use crate::commands::support::ipc_test_support::MockApp;
    use serde_json::json;

    fn mock() -> MockApp {
        MockApp::start(
            "frontend-log",
            tauri::generate_handler![log_frontend_error],
            |_| (),
        )
        .0
    }

    #[test]
    fn the_webview_payload_binds_to_the_command() {
        let app = mock();

        let answer = app.invoke(
            "log_frontend_error",
            json!({ "location": "reports", "name": "TypeError", "message": "boom 12345" }),
        );

        assert_eq!(answer, Ok(serde_json::Value::Null));
    }

    #[test]
    fn a_location_outside_the_set_is_refused() {
        let app = mock();

        let answer = app.invoke(
            "log_frontend_error",
            json!({ "location": "Rent 1200", "name": "E", "message": "m" }),
        );

        assert!(answer.is_err());
    }
}
