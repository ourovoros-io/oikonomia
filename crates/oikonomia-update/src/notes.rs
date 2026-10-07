//! Release notes are plain text. The webview must not navigate on them.

/// Strips HTML tags and escapes the remainder so notes cannot carry markup.
///
/// `<a href="...">` becomes the link text only. Remaining `&`, `<`, `>`, quotes
/// are escaped. The result is safe to show as text.
#[must_use]
pub fn sanitize_notes(input: &str) -> String {
    let mut stripped = String::new();
    let mut in_tag = false;
    let mut pending_lt = false;
    for ch in input.chars() {
        if pending_lt {
            pending_lt = false;
            if ch.is_ascii_alphabetic() || ch == '/' || ch == '!' || ch == '?' {
                in_tag = true;
            } else {
                stripped.push('<');
                stripped.push(ch);
                continue;
            }
        }
        if ch == '<' {
            pending_lt = true;
            continue;
        }
        if ch == '>' && in_tag {
            in_tag = false;
            continue;
        }
        if !in_tag {
            stripped.push(ch);
        }
    }
    if pending_lt {
        stripped.push('<');
    }

    let mut escaped = String::new();
    for ch in stripped.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::sanitize_notes;

    #[test]
    fn html_anchor_becomes_text() {
        let notes = sanitize_notes("See <a href=\"https://evil.example\">notes</a>");
        assert_eq!(notes, "See notes");
        assert!(!notes.contains("href"));
        assert!(!notes.contains("evil.example"));
    }

    #[test]
    fn leftover_brackets_are_escaped() {
        let notes = sanitize_notes("1 < 2 & 3");
        assert_eq!(notes, "1 &lt; 2 &amp; 3");
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::sanitize_notes;

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
    }
}
