//! Release notes are plain text.
//!
//! The notes come from the signed manifest, so they are the publisher's own
//! words, but they are still text from the network on their way to a
//! webview. [`sanitize_notes`] escapes every character HTML gives meaning
//! to, so that whatever renders the notes shows them as text and cannot be
//! made to build an element or a link from them.
//!
//! Nothing is removed. Deleting what looks like a tag would take real text
//! with it (in `if a<b then ...` everything after the `<` reads as one), and
//! escaping alone already leaves no markup.

/// Escapes `input` so that it carries no HTML markup.
///
/// `&`, `<`, `>`, `"` and `'` become character references and every other
/// character is kept. The result therefore never contains a raw `<`, `>` or
/// `"`; a property test holds it to that.
#[must_use]
pub(crate) fn sanitize_notes(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());

    for character in input.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use super::sanitize_notes;

    #[test]
    fn an_anchor_becomes_inert_text() {
        let notes = sanitize_notes("See <a href=\"https://evil.example\">notes</a>");

        assert_eq!(
            notes,
            "See &lt;a href=&quot;https://evil.example&quot;&gt;notes&lt;/a&gt;"
        );
    }

    #[test]
    fn brackets_ampersands_and_quotes_are_escaped() {
        assert_eq!(sanitize_notes("1 < 2 & 3"), "1 &lt; 2 &amp; 3");
        assert_eq!(sanitize_notes("it's \"new\""), "it&#39;s &quot;new&quot;");
    }

    #[test]
    fn a_comparison_written_without_spaces_keeps_its_text() {
        assert_eq!(
            sanitize_notes("if a<b then the total is right"),
            "if a&lt;b then the total is right"
        );
    }

    #[test]
    fn text_without_markup_characters_is_unchanged() {
        let plain = "Faster imports.\nΝέα έκδοση: 0.2.0";

        assert_eq!(sanitize_notes(plain), plain);
    }
}

#[cfg(test)]
mod properties {
    use super::sanitize_notes;
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    /// Reverses [`sanitize_notes`], for the round-trip property only.
    fn unescape(escaped: &str) -> String {
        // `&amp;` last: replacing it first would turn `&amp;lt;` into `<`.
        escaped
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn sanitized_notes_hold_no_raw_markup_character(input in any::<String>()) {
            let notes = sanitize_notes(&input);

            prop_assert!(!notes.contains(['<', '>', '"']), "{:?}", notes);
        }

        #[test]
        fn sanitized_markup_holds_no_raw_markup_character(input in "[<>\"'&/!?a-z =]{0,40}") {
            let notes = sanitize_notes(&input);

            prop_assert!(!notes.contains(['<', '>', '"']), "{:?}", notes);
        }

        #[test]
        fn sanitizing_loses_no_text(input in "[<>\"'&/!?a-z =;#0-9]{0,40}") {
            let notes = sanitize_notes(&input);

            prop_assert_eq!(unescape(&notes), input);
        }
    }
}
