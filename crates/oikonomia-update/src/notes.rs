//! Release notes are plain text. The webview must not navigate on them.

/// Strips HTML tags and escapes the remainder so notes cannot carry markup.
///
/// `<a href="...">` becomes the link text only. Remaining `&`, `<`, `>`, quotes
/// are escaped. The result is safe to show as text.
#[must_use]
pub fn sanitize_notes(input: &str) -> String {
    let mut stripped = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        if ch == '<' {
            in_tag = true;
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
