//! Keywords and the two things needed to match them: the folded form of
//! text, and the rule for where a keyword may sit in it.
//!
//! Every table of words in this module is a table of [`Keyword`]s: the
//! topics of the account matcher, the labels and markers of the invoice
//! reader, and the brand tokens and service keywords. None of them is
//! matched as a bare substring, because a short word is too often the inside
//! of a longer one: `mark` is in `supermarket`, `total` in `subtotal`, `tax`
//! in `syntax`, and `ποσο` (amount) begins `ποσοστο` (percentage).
//!
//! # The folded form
//!
//! [`folded`] lowercases text and removes the accents of precomposed Greek
//! letters, so one spelling of a keyword matches the word in capitals, with
//! its accent and without. A letter followed by a combining accent is left
//! as it is. The reader, the brand table and the account matcher fold the
//! text and write their keywords folded; a test in each file that defines
//! such keywords checks that.
//!
//! # Where a keyword may sit
//!
//! A keyword matches where its text occurs with a boundary on each side its
//! kind checks. A boundary is the start or end of the text, or any character
//! that is not a letter or a digit; letters and digits are judged by
//! Unicode, so Greek and accented letters count.
//!
//! | Kind | Before the keyword | After the keyword | For |
//! |------|--------------------|-------------------|-----|
//! | [`Word`](Keyword::Word) | boundary | boundary | a whole word or phrase |
//! | [`Prefix`](Keyword::Prefix) | boundary | anything | a stem that takes endings |
//! | [`Unit`](Keyword::Unit) | no letter | boundary | a unit glued to a number |
//! | [`Fragment`](Keyword::Fragment) | anything | anything | a sign, or a piece of a name |
//!
//! Greek inflects, so most Greek keywords are stems (`πληρωμ` for `πληρωμή`,
//! `πληρωμής`, `πληρωμών`). A keyword may hold spaces and punctuation of its
//! own (`amount due`, `α.φ.μ`); only its two ends are checked.
//!
//! A [`Fragment`](Keyword::Fragment) is the bare substring match the other
//! kinds exist to avoid. It is for what is not a word: a multiplication
//! sign, or the `bank` that ends `Eurobank`. Each use says why.

/// A word to look for in text, and how it must sit there to count.
///
/// The module documentation has the rule for each kind. A keyword is written
/// in the form of the text it is matched against: lowercase, and folded
/// where the text is folded.
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
    /// A piece of text that counts wherever it is, inside a word too.
    Fragment(&'static str),
}

impl Keyword {
    /// The text the keyword looks for.
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::Word(text) | Self::Prefix(text) | Self::Unit(text) | Self::Fragment(text) => text,
        }
    }

    /// Whether this keyword occurs in `text` under its rule.
    ///
    /// The caller brings the text into the form the keyword is written in.
    pub(crate) fn occurs_in(self, text: &str) -> bool {
        self.sides().occurs(self.text(), text)
    }

    /// Whether `text` begins with this keyword, under its rule.
    pub(crate) fn starts(self, text: &str) -> bool {
        text.strip_prefix(self.text())
            .is_some_and(|rest| self.sides().after.allows(rest.chars().next()))
    }

    /// What the kind requires on each side of the keyword.
    const fn sides(self) -> Sides {
        let (before, after) = match self {
            Self::Word(_) => (Side::Boundary, Side::Boundary),
            Self::Prefix(_) => (Side::Boundary, Side::Anything),
            Self::Unit(_) => (Side::NoLetter, Side::Boundary),
            Self::Fragment(_) => (Side::Anything, Side::Anything),
        };

        Sides { before, after }
    }
}

/// Whether `text` holds any of `keywords`, each under its own rule.
pub(crate) fn contains_any(text: &str, keywords: &[Keyword]) -> bool {
    keywords.iter().any(|keyword| keyword.occurs_in(text))
}

/// What may stand directly next to a keyword on one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The end of the text, or a character that is not a letter or a digit.
    Boundary,
    /// The end of the text, or a character that is not a letter.
    NoLetter,
    /// Any character, or none.
    Anything,
}

impl Side {
    /// Whether `neighbour`, the character on this side or `None` at the end
    /// of the text, is allowed there.
    fn allows(self, neighbour: Option<char>) -> bool {
        match self {
            Self::Boundary => !neighbour.is_some_and(char::is_alphanumeric),
            Self::NoLetter => !neighbour.is_some_and(char::is_alphabetic),
            Self::Anything => true,
        }
    }
}

/// The requirement on each side of a keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sides {
    /// What may come directly before the keyword.
    before: Side,
    /// What may come directly after it.
    after: Side,
}

impl Sides {
    /// Whether `needle` occurs in `text` with both sides satisfied.
    ///
    /// Every occurrence is tried, so a word that first appears inside
    /// another and later on its own is found. An empty needle never occurs.
    fn occurs(self, needle: &str, text: &str) -> bool {
        if needle.is_empty() {
            return false;
        }

        text.match_indices(needle).any(|(start, _)| {
            // `match_indices` yields the offset of a match of `needle`, so
            // both ends of the match are character boundaries and `get`
            // returns the text on each side.
            let before = text.get(..start).and_then(|head| head.chars().next_back());
            let after = text
                .get(start + needle.len()..)
                .and_then(|tail| tail.chars().next());

            self.before.allows(before) && self.after.allows(after)
        })
    }
}

/// Lowercases `text` and folds its Greek accents.
///
/// Every label and marker of the invoice reader and of `brands`, and every
/// keyword of the account matcher, is matched against text in this form and
/// is itself written in it, so `Τελική`, `ΤΕΛΙΚΗ` and `τελικη` all match the
/// one needle `τελικη`.
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

    #[test]
    fn a_word_needs_a_boundary_on_both_sides() {
        let total = Keyword::Word("total");

        for text in [
            "total",
            "total: 5",
            "the total.",
            "(total)",
            "sub-total",
            "a/total/b",
        ] {
            assert!(total.occurs_in(text), "{text:?}");
        }
        for text in [
            "subtotal", "totals", "total7", "7total", "τotal", "totalα", "",
        ] {
            assert!(!total.occurs_in(text), "{text:?}");
        }
    }

    #[test]
    fn a_word_inside_another_is_still_found_where_it_stands_alone() {
        assert!(Keyword::Word("total").occurs_in("subtotal 10 total 12"));
        assert!(Keyword::Word("mark").occurs_in("supermarket mark 400"));
        assert!(Keyword::Word("ποσο").occurs_in("ποσοστο 24 ποσο 310"));
    }

    #[test]
    fn a_phrase_is_checked_at_its_two_ends_only() {
        let due = Keyword::Word("amount due");

        assert!(due.occurs_in("amount due: 45,90"));
        assert!(!due.occurs_in("amount dues"));
        assert!(!due.occurs_in("subamount due"));

        let tax_number = Keyword::Word("α.φ.μ");
        assert!(tax_number.occurs_in("α.φ.μ.: 000000000"));
        assert!(!tax_number.occurs_in("κα.φ.μ"));
    }

    #[test]
    fn a_prefix_starts_a_word_and_takes_any_ending() {
        let payment = Keyword::Prefix("πληρωμ");

        for text in ["πληρωμη", "ποσο πληρωμης", "πληρωμων", "(πληρωμ"]
        {
            assert!(payment.occurs_in(text), "{text:?}");
        }
        for text in ["προπληρωμη", "7πληρωμη", "πληρω"] {
            assert!(!payment.occurs_in(text), "{text:?}");
        }
    }

    #[test]
    fn a_unit_may_follow_a_number_and_not_a_letter() {
        let kwh = Keyword::Unit("kwh");

        for text in ["150kwh", "150 kwh", "kwh", "0,085/kwh"] {
            assert!(kwh.occurs_in(text), "{text:?}");
        }
        for text in ["xkwh", "kwhs", "kwh7"] {
            assert!(!kwh.occurs_in(text), "{text:?}");
        }
    }

    #[test]
    fn a_fragment_counts_anywhere() {
        let bank = Keyword::Fragment("bank");

        for text in ["bank", "eurobank", "banking", "piraeusbank s.a."] {
            assert!(bank.occurs_in(text), "{text:?}");
        }
        assert!(!bank.occurs_in("ban k"));
    }

    #[test]
    fn a_keyword_starts_a_text_only_under_its_own_rule() {
        assert!(Keyword::Word("name").starts("name: acme"));
        assert!(Keyword::Word("name").starts("name"));
        assert!(!Keyword::Word("name").starts("names: acme"));
        assert!(!Keyword::Word("name").starts("the name: acme"));
        assert!(Keyword::Prefix("name").starts("names: acme"));
    }

    #[test]
    fn any_of_several_keywords_is_enough() {
        let keywords = [Keyword::Word("total"), Keyword::Prefix("συνολ")];

        assert!(contains_any("συνολικο ποσο", &keywords));
        assert!(contains_any("grand total", &keywords));
        assert!(!contains_any("subtotal", &keywords));
        assert!(!contains_any("anything", &[]));
    }

    #[test]
    fn an_empty_keyword_never_occurs() {
        for keyword in [
            Keyword::Word(""),
            Keyword::Prefix(""),
            Keyword::Unit(""),
            Keyword::Fragment(""),
        ] {
            assert!(!keyword.occurs_in("any text"), "{keyword:?}");
            assert!(!keyword.occurs_in(""), "{keyword:?}");
        }
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// The sides of a [`Keyword::Word`].
    const WORD: Sides = Sides {
        before: Side::Boundary,
        after: Side::Boundary,
    };

    /// A run of one to eight letters and digits, Latin or Greek.
    fn letters_and_digits() -> impl Strategy<Value = String> {
        proptest::string::string_regex("[a-z0-9α-ω]{1,8}").expect("the pattern is a valid regex")
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn a_word_never_matches_inside_a_longer_word(
            word in letters_and_digits(),
            before in letters_and_digits(),
            after in letters_and_digits(),
        ) {
            // Each text is one unbroken run of letters and digits that is
            // longer than the word, so wherever the word occurs in it, a
            // letter or digit stands on at least one side.
            for longer in [
                format!("{before}{word}"),
                format!("{word}{after}"),
                format!("{before}{word}{after}"),
            ] {
                prop_assert!(!WORD.occurs(&word, &longer), "{word:?} in {longer:?}");
            }
        }

        #[test]
        fn a_word_always_matches_between_boundaries(
            word in letters_and_digits(),
            before in letters_and_digits(),
            after in letters_and_digits(),
            separator in prop::sample::select(vec![" ", ":", ".", "/", "(", "\n", "€"]),
        ) {
            let text = format!("{before}{separator}{word}{separator}{after}");

            prop_assert!(WORD.occurs(&word, &text), "{word:?} in {text:?}");
        }

        #[test]
        fn matching_any_text_returns_instead_of_panicking(
            needle in any::<String>(),
            text in any::<String>(),
        ) {
            for before in [Side::Boundary, Side::NoLetter, Side::Anything] {
                for after in [Side::Boundary, Side::Anything] {
                    let _ = Sides { before, after }.occurs(&needle, &text);
                }
            }
        }
    }
}
