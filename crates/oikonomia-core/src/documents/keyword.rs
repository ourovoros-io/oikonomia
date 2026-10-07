//! Keywords and the two things needed to match them: the folded form of
//! text, and the rule for where a keyword may sit in it.
//!
//! [`folded`] lowercases text and removes Greek accents, so one spelling of
//! a keyword matches every way a document prints the word.
//!
//! A [`Keyword`] matches on word boundaries. A letter or digit directly next
//! to it, on a side its kind checks, stops the match.

/// How a keyword must sit in the text to count as a match.
///
/// Matching is on word boundaries, never on bare substrings: a letter or digit
/// (Unicode-aware, so Greek and accented letters count) directly next to the
/// keyword on a checked side stops it matching. That is what keeps "tax" out
/// of "taxi" and "syntax". Inflected forms that a whole-word keyword would
/// miss are either listed as their own keywords or marked [`Keyword::Prefix`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keyword {
    /// The keyword is a whole word: no letter or digit on either side.
    Word(&'static str),
    /// The keyword is the stem of a word (`consult` for "consulting"): it must
    /// start a word, and any letters may follow.
    Prefix(&'static str),
    /// A unit that is written glued to a number (`kwh` in "150kwh"): no letter
    /// before it, so digits are fine, and no letter or digit after it.
    Unit(&'static str),
}

impl Keyword {
    /// The text the keyword looks for.
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::Word(text) | Self::Prefix(text) | Self::Unit(text) => text,
        }
    }

    /// Whether this keyword occurs in `lowercased_text` under its rule.
    ///
    /// The caller lowercases the text; keywords are written in lowercase.
    #[expect(
        clippy::string_slice,
        reason = "`match_indices` yields the offset of a match of `needle`, \
                  so both ends of the match are character boundaries"
    )]
    pub(crate) fn occurs_in(self, lowercased_text: &str) -> bool {
        let needle = self.text();

        for (start, _) in lowercased_text.match_indices(needle) {
            let before = lowercased_text[..start].chars().next_back();
            let after = lowercased_text[start + needle.len()..].chars().next();

            let starts_a_word = match self {
                Self::Word(_) | Self::Prefix(_) => !before.is_some_and(char::is_alphanumeric),
                Self::Unit(_) => !before.is_some_and(char::is_alphabetic),
            };
            let ends_a_word = match self {
                Self::Prefix(_) => true,
                Self::Word(_) | Self::Unit(_) => !after.is_some_and(char::is_alphanumeric),
            };

            if starts_a_word && ends_a_word {
                return true;
            }
        }

        false
    }
}

/// Lowercases `text` and folds its Greek accents.
///
/// Every label and marker of the invoice reader and of `brands` is matched
/// against text in this form, and is itself written in it, so `Τελική`, `ΤΕΛΙΚΗ` and
/// `τελικη` all match the one needle `τελικη`.
pub(crate) fn folded(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'ά' | 'ὰ' | 'ᾶ' | 'ἀ' | 'ἁ' | 'ᾳ' => 'α',
            'έ' | 'ὲ' | 'ἐ' | 'ἑ' => 'ε',
            'ή' | 'ὴ' | 'ῆ' | 'ἠ' | 'ἡ' | 'ῃ' => 'η',
            'ί' | 'ὶ' | 'ῖ' | 'ϊ' | 'ΐ' | 'ἰ' | 'ἱ' => 'ι',
            'ό' | 'ὸ' | 'ὀ' | 'ὁ' => 'ο',
            'ύ' | 'ὺ' | 'ῦ' | 'ϋ' | 'ΰ' | 'ὐ' | 'ὑ' => 'υ',
            'ώ' | 'ὼ' | 'ῶ' | 'ὠ' | 'ὡ' | 'ῳ' => 'ω',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_lowercases_and_drops_greek_accents() {
        assert_eq!(folded("Τελική Αξία"), "τελικη αξια");
        assert_eq!(folded("ΤΕΛΙΚΗ ΑΞΙΑ"), "τελικη αξια");
        assert_eq!(folded("Ϊ ΰ Ώ"), "ι υ ω");
        assert_eq!(folded("Total 24%"), "total 24%");
    }
}
