//! Text normalization for the invoice reader: the cleaned text every later
//! step reads, and the folded form labels are matched against.
//!
//! [`normalize`] runs once per document, before anything else. Its four
//! steps, and why they run in that order, are in the
//! [module above](crate::documents::invoice#normalizing). After it a number
//! is one unbroken run of digits, `,` and `.`, which is what the money
//! tokenizer relies on.
//!
//! [`folded`] gives the second form of the text: lowercase, Greek accents
//! removed. Every label and marker constant of the reader is written in that
//! form.

use crate::documents::invoice::dates::{mask_date_tokens, mask_time_tokens};

/// Cleans up extracted text before anything reads it, by the four steps in
/// the module documentation ("Normalizing").
///
/// The result has no `\r`, no empty line, and no line with leading or
/// trailing whitespace.
pub(super) fn normalize(text: &str) -> String {
    let text = text.replace('\r', "\n");
    let text = text.replace(['\u{00a0}', '\u{202f}', '\u{2009}'], " ");
    // Euro symbol variants — standalone tokens only, so EUROBANK stays intact.
    // Before the number joins, so "EUR1 234,56" starts its amount at a symbol
    // and not at the tail of a word.
    let text = replace_eur_token(&text);
    // OCR and PDF extraction split numbers with spaces: "72, 53", "1 234,56".
    let text = collapse_spaced_decimals(&text);

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replaces each standalone `EUR`, in any letter case, with `€`.
///
/// Standalone means no letter directly before or after. So `EUROBANK` and
/// `Europe` keep their letters, which would otherwise show up damaged in a
/// merchant or description, while `10EUR` and `EUR1` are replaced: a digit
/// is a boundary.
fn replace_eur_token(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut replaced = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let is_eur = index + 2 < chars.len()
            && chars[index].eq_ignore_ascii_case(&'e')
            && chars[index + 1].eq_ignore_ascii_case(&'u')
            && chars[index + 2].eq_ignore_ascii_case(&'r');
        let boundary_before = index == 0 || !chars[index - 1].is_alphabetic();
        let boundary_after = index + 3 >= chars.len() || !chars[index + 3].is_alphabetic();

        if is_eur && boundary_before && boundary_after {
            replaced.push('€');
            index += 3;
        } else {
            replaced.push(chars[index]);
            index += 1;
        }
    }
    replaced
}

/// Digits in a thousands group: `1.234.567`, `1 234 567`.
pub(super) const THOUSANDS_GROUP_DIGITS: usize = 3;

/// Joins the pieces of a number that OCR or PDF extraction split with a space.
///
/// Two joins are made, each inside one line:
///
/// - a separator and the digits after it: `72, 53` becomes `72,53`. Once a
///   number has a separator, a later one joins across a space only after a
///   group of three digits (`1.234, 56`), so the full stop in
///   `45,90. 3 items` ends the amount;
/// - space-grouped thousands: `1 234,56` becomes `1234,56`.
///
/// A thousands join needs a prefix that can be one: one to three digits
/// that start their own token, with no `,` or `.` read yet, followed by one
/// space and exactly three digits. Dates and clock times are never a
/// prefix, and neither is the tail of a code such as `A-7` or `B7`.
///
/// A lone digit is a valid prefix, because `5 120,50` and `1 234,56` are
/// spelled the same way. A quantity column directly before a three-digit
/// price therefore reads as one amount; a labelled total on the document
/// still decides the suggested amount.
pub(super) fn collapse_spaced_decimals(text: &str) -> String {
    text.split('\n')
        .map(collapse_spaced_decimals_on_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// [`collapse_spaced_decimals`] for one line.
///
/// Characters that are not part of a number are copied as they are, dates
/// and clock times included.
fn collapse_spaced_decimals_on_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    // Dates and clocks are blanked in this copy, so their digits never start
    // or extend a number; the output below copies them from `chars`.
    let numbers: Vec<char> = mask_time_tokens(&mask_date_tokens(line)).chars().collect();

    let mut joined = String::with_capacity(line.len());
    let mut index = 0;
    while let Some(&original) = chars.get(index) {
        if numbers.get(index).is_some_and(char::is_ascii_digit) {
            index = push_spaced_number(&numbers, index, &mut joined);
        } else {
            joined.push(original);
            index += 1;
        }
    }
    joined
}

/// Appends the number starting at `start` to `joined` with its inner spaces
/// removed, and returns the index after it.
fn push_spaced_number(numbers: &[char], start: usize, joined: &mut String) -> usize {
    let may_group_thousands = starts_own_token(numbers, start);
    let mut has_separator = false;
    // Digits since the start of the number, the last join or the last separator.
    let mut group_digits = 0_usize;
    let mut index = start;

    while let Some(&current) = numbers.get(index) {
        if current.is_ascii_digit() {
            joined.push(current);
            group_digits += 1;
            index += 1;
        } else if let Some(next_digit) = digit_after_separator(numbers, index)
            && (next_digit == index + 1 || !has_separator || group_digits == THOUSANDS_GROUP_DIGITS)
        {
            joined.push(current);
            has_separator = true;
            group_digits = 0;
            index = next_digit;
        } else if !has_separator
            && may_group_thousands
            && (1..=THOUSANDS_GROUP_DIGITS).contains(&group_digits)
            && is_thousands_group(numbers, index)
        {
            group_digits = 0;
            index += 1;
        } else {
            break;
        }
    }
    index
}

/// Whether the digits at `start` begin a token: nothing before them makes
/// them the tail of a date, a dotted number or a code.
fn starts_own_token(numbers: &[char], start: usize) -> bool {
    let Some(before) = start.checked_sub(1).and_then(|index| numbers.get(index)) else {
        return true;
    };
    !before.is_alphanumeric() && !matches!(before, '/' | '.' | '-' | ',' | ':')
}

/// Index of the digit that continues a number across the `,` or `.` at
/// `index`, with at most one space in between.
fn digit_after_separator(numbers: &[char], index: usize) -> Option<usize> {
    if !matches!(numbers.get(index), Some(',' | '.')) {
        return None;
    }

    let is_digit = |at: usize| numbers.get(at).is_some_and(char::is_ascii_digit);
    let space_then_digit = numbers
        .get(index + 1)
        .is_some_and(|next| next.is_whitespace())
        && is_digit(index + 2);

    if is_digit(index + 1) {
        Some(index + 1)
    } else {
        space_then_digit.then_some(index + 2)
    }
}

/// Whether `index` holds one space followed by exactly three digits.
fn is_thousands_group(numbers: &[char], index: usize) -> bool {
    let is_digit = |at: usize| numbers.get(at).is_some_and(char::is_ascii_digit);

    numbers
        .get(index)
        .is_some_and(|space| space.is_whitespace())
        && (1..=THOUSANDS_GROUP_DIGITS).all(|offset| is_digit(index + offset))
        && !is_digit(index + THOUSANDS_GROUP_DIGITS + 1)
}

/// Lowercases `text` and folds its Greek accents.
///
/// Every label and marker in this module and in `brands` is matched against
/// text in this form, and is itself written in it, so `Τελική`, `ΤΕΛΙΚΗ` and
/// `τελικη` all match the one needle `τελικη`.
pub(in crate::documents) fn folded(text: &str) -> String {
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

/// Whether folded text contains any of `needles`.
pub(super) fn contains_any(folded_text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| folded_text.contains(needle))
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
    fn eur_token_replacement_keeps_words() {
        assert_eq!(replace_eur_token("TOTAL 10 EUR"), "TOTAL 10 €");
        assert_eq!(replace_eur_token("10eur"), "10€");
        assert_eq!(replace_eur_token("EUROBANK EUROPE"), "EUROBANK EUROPE");
        assert_eq!(replace_eur_token("EUR"), "€");
    }
}

#[cfg(test)]
mod amounts_and_dates {
    use crate::documents::analyze::DocumentSuggestion;
    use crate::documents::invoice::parse_invoice_text;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn a_date_before_an_amount_is_not_its_thousands_prefix() {
        let padded = read("13/08/2026 172,53 €");
        assert_eq!(padded.amount_minor, Some(17_253));
        assert_eq!(padded.entry_date.as_deref(), Some("2026-08-13"));

        let unpadded = read("27/8/2026 310,00");
        assert_eq!(unpadded.amount_minor, Some(31_000));
        assert_eq!(unpadded.entry_date.as_deref(), Some("2026-08-27"));

        // A two-digit year ends in a group short enough to be a prefix.
        let short_year = read("13/08/26 172,53 €");
        assert_eq!(short_year.amount_minor, Some(17_253));
        assert_eq!(short_year.entry_date.as_deref(), Some("2026-08-13"));
    }

    #[test]
    fn space_grouped_thousands_are_one_amount() {
        assert_eq!(read("TOTAL 1 234,56").amount_minor, Some(123_456));
        assert_eq!(read("TOTAL 12 345,00 €").amount_minor, Some(1_234_500));
        assert_eq!(read("TOTAL 1 234 567,89").amount_minor, Some(123_456_789));
    }

    #[test]
    fn a_number_that_cannot_be_a_thousands_prefix_stays_separate() {
        // More than three digits are not a thousands prefix.
        assert_eq!(read("Order 123456 500,00").amount_minor, Some(50_000));
        // A finished decimal amount is not one either.
        assert_eq!(read("Amount due 45,90 120,00").amount_minor, Some(12_000));
        // Nor is the tail of a code or of a dotted number.
        assert_eq!(read("Ref A-7 120,50").amount_minor, Some(12_050));
        assert_eq!(read("Item B7 120,50").amount_minor, Some(12_050));
        // A number on the line above is a different number.
        assert_eq!(read("Quantity 5\n120,50").amount_minor, Some(12_050));
    }

    #[test]
    fn a_lone_digit_before_three_digits_reads_as_thousands() {
        // "5 120,50" is spelled exactly like "1 234,56", so a quantity column
        // next to a three-digit price reads as one amount. A labelled total
        // on the document still decides.
        assert_eq!(read("Qty 5 120,50").amount_minor, Some(512_050));
        assert_eq!(
            read("Qty 5 120,50\nAmount due: 602,50").amount_minor,
            Some(60_250)
        );
    }
}

#[cfg(test)]
mod documented_tradeoffs {
    use crate::documents::analyze::DocumentSuggestion;
    use crate::documents::invoice::parse_invoice_text;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn a_space_before_the_separator_is_not_joined() {
        assert_eq!(read("Amount due: 72, 53").amount_minor, Some(7_253));
        assert_eq!(read("Amount due: 72 ,53").amount_minor, Some(7_200));
    }
}
