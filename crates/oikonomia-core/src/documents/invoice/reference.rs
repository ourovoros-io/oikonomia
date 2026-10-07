//! The reference of a document: a supply code, a payment code, a MARK
//! number or an invoice number.
//!
//! [`find_invoice_reference`] tries six steps in order and takes the first
//! that yields one; the order, and what is known of the reasons for it, is in
//! the [module above](crate::documents::invoice#the-reference). The first
//! four steps look for codes with a shape or a label of their own, the last
//! two for bare runs of digits.

use std::ops::RangeInclusive;

use crate::documents::keyword::Keyword::{Prefix, Unit, Word};
use crate::documents::keyword::{Keyword, contains_any, folded};

/// The document's reference, by the first of these that yields one: a
/// labelled supply code, a bare NGS supply code, an RF payment code, a MARK
/// number, a number next to an invoice label, the longest long number.
pub(super) fn find_invoice_reference(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();

    labelled_supply_code(&lines)
        .or_else(|| bare_ngs_supply_code(&lines))
        .or_else(|| lines.iter().find_map(|line| rf_payment_code(line)))
        .or_else(|| mark_number(&lines))
        .or_else(|| labelled_reference_number(&lines))
        .or_else(|| longest_reference_number(&lines))
}

/// Labels of a supply or meter code. `ηκασπ` is the Greek abbreviation for a
/// gas delivery point code. It is matched as a unit, so a digit may stand
/// right before it: extraction glues the label to the code of the column
/// before (`SYN000000001ΗΚΑΣΠ:` in `tests/fixtures/ngs_gas_jumbled_extract.txt`).
const SUPPLY_CODE_LABELS: &[Keyword] = &[Word("κωδικος παροχης"), Word("supply"), Unit("ηκασπ")];

/// Lengths of an alphanumeric supply code.
///
/// The codes in the tests have 10 and 12 characters. The reasons for 8 and
/// 24 are not recorded, and no test pins either.
const SUPPLY_CODE_CHARS: RangeInclusive<usize> = 8..=24;

/// Fewest characters of an RF payment code, the `RF` included.
///
/// `reference_finds_an_rf_payment_code_as_a_token` pins that nine are too
/// few. The reason for 10 is not recorded.
const MIN_RF_CODE_CHARS: usize = 10;

/// The supply or meter code beside the first supply label that has one
/// (`NGS000000001`), on the label's line or the next: extraction often puts
/// the value on the line after its label.
fn labelled_supply_code(lines: &[&str]) -> Option<String> {
    for (index, line) in lines.iter().enumerate() {
        if !contains_any(&folded(line), SUPPLY_CODE_LABELS) {
            continue;
        }

        let next = lines.get(index + 1).copied().unwrap_or("");
        if let Some(code) = alnum_supply_code(line).or_else(|| alnum_supply_code(next)) {
            return Some(code);
        }
    }
    None
}

/// The first supply code anywhere that starts with `NGS`, labelled or not.
fn bare_ngs_supply_code(lines: &[&str]) -> Option<String> {
    lines
        .iter()
        .filter_map(|line| alnum_supply_code(line))
        .find(|code| code.to_ascii_uppercase().starts_with("NGS"))
}

/// The RF payment code of a Greek utility bill on `line`, in upper case: a
/// word of its own, or the run that starts at the first `RF` when
/// extraction glued the code to its neighbours.
fn rf_payment_code(line: &str) -> Option<String> {
    let token = line
        .split_whitespace()
        .map(str::to_ascii_uppercase)
        .find(|token| is_rf_payment_code(token));
    if token.is_some() {
        return token;
    }

    let upper = line.to_ascii_uppercase();
    let start = upper.find("RF")?;
    let glued: String = upper
        .get(start..)?
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    is_rf_payment_code(&glued).then_some(glued)
}

/// Whether uppercased `upper` is an RF payment code: `RF` then digits only,
/// at least [`MIN_RF_CODE_CHARS`] characters in all.
fn is_rf_payment_code(upper: &str) -> bool {
    upper.len() >= MIN_RF_CODE_CHARS && is_rf_then_digits(upper)
}

/// Whether uppercased `upper` is `RF` followed by digits only, whatever its
/// length. `RF` alone counts: nothing follows it that is not a digit.
pub(super) fn is_rf_then_digits(upper: &str) -> bool {
    upper.starts_with("RF") && upper.chars().skip(2).all(|c| c.is_ascii_digit())
}

/// Labels of the line that carries a MARK number, the registration number
/// the Greek tax authority gives an invoice. `α.α` (serial number) heads the
/// same table row as the MARK.
const MARK_LABELS: &[Keyword] = &[Word("μαρκ"), Word("mark"), Word("α.α"), Word("αα")];

/// The MARK number of a Greek invoice: a run of digits on the first MARK or
/// `Α.Α.` line that has one on it or on the line below.
fn mark_number(lines: &[&str]) -> Option<String> {
    for (index, line) in lines.iter().enumerate() {
        if !contains_any(&folded(line), MARK_LABELS) {
            continue;
        }

        let next = lines.get(index + 1).copied().unwrap_or("");
        if let Some(number) = long_digit_token(line).or_else(|| long_digit_token(next)) {
            return Some(number);
        }
    }
    None
}

/// Labels of an invoice or reference number.
const REFERENCE_LABELS: &[Keyword] = &[
    Word("invoice"),
    Prefix("αρ. παραστατ"),
    Word("αριθμος"),
    Word("number"),
    Word("ref"),
];

/// The run of digits on the first line that names an invoice or reference
/// number and has one.
fn labelled_reference_number(lines: &[&str]) -> Option<String> {
    lines.iter().find_map(|line| {
        if contains_any(&folded(line), REFERENCE_LABELS) {
            long_digit_token(line)
        } else {
            None
        }
    })
}

/// Fewest digits of a reference that no label names.
///
/// `reference_falls_back_to_the_longest_run_of_ten_or_more_digits` pins that
/// nine are too few. The reason for 10 is not recorded.
const MIN_UNLABELLED_REFERENCE_DIGITS: usize = 10;

/// The longest run of digits anywhere that can be an unlabelled reference:
/// [`MIN_UNLABELLED_REFERENCE_DIGITS`] to 20 digits. The first one wins a
/// tie. This is the last resort of [`find_invoice_reference`].
fn longest_reference_number(lines: &[&str]) -> Option<String> {
    let mut longest: Option<String> = None;
    for number in lines.iter().filter_map(|line| long_digit_token(line)) {
        if number.len() >= MIN_UNLABELLED_REFERENCE_DIGITS
            && longest
                .as_ref()
                .is_none_or(|longest| number.len() > longest.len())
        {
            longest = Some(number);
        }
    }
    longest
}

/// Lengths of a run of digits that can be a reference. A MARK number has 15.
///
/// Both ends are pinned: five digits are too few
/// (`reference_reads_a_number_next_to_an_invoice_label`) and twenty-one too
/// many (`reference_falls_back_to_the_longest_run_of_ten_or_more_digits`).
/// The reasons for 6 and 20 are not recorded.
const REFERENCE_DIGITS: RangeInclusive<usize> = 6..=20;

/// The longest run of digits on `line` whose length is in
/// [`REFERENCE_DIGITS`]; the last such run on a tie.
///
/// A run longer than the range is passed over whole; no part of it is
/// taken.
fn long_digit_token(line: &str) -> Option<String> {
    let mut longest: Option<String> = None;
    let mut run = String::new();

    let end_run = |run: &mut String, longest: &mut Option<String>| {
        if REFERENCE_DIGITS.contains(&run.len())
            && longest
                .as_ref()
                .is_none_or(|longest| run.len() >= longest.len())
        {
            *longest = Some(run.clone());
        }
        run.clear();
    };

    for character in line.chars() {
        if character.is_ascii_digit() {
            run.push(character);
        } else {
            end_run(&mut run, &mut longest);
        }
    }
    end_run(&mut run, &mut longest);
    longest
}

/// An alphanumeric supply or delivery-point code on `line`, such as
/// `NGS000000001`, as written.
///
/// First the whitespace-separated words are tried, with the punctuation
/// around each trimmed. Then every run of ASCII letters and digits in the
/// line is, which finds a code that extraction glued to other text with
/// punctuation (`text;NGS000000042;more`). A code has
/// [`SUPPLY_CODE_CHARS`] characters with at least one letter and one digit,
/// and is not an RF payment code.
fn alnum_supply_code(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        let token = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if SUPPLY_CODE_CHARS.contains(&token.len())
            && token.chars().any(|c| c.is_ascii_alphabetic())
            && token.chars().any(|c| c.is_ascii_digit())
            && token.chars().all(|c| c.is_ascii_alphanumeric())
        {
            // An RF payment code has this shape too, and has a step of its own.
            if is_rf_then_digits(&token.to_ascii_uppercase()) {
                continue;
            }
            return Some(token.to_owned());
        }
    }
    let mut run = String::new();
    for character in line.chars() {
        if character.is_ascii_alphanumeric() {
            run.push(character);
        } else {
            if let Some(code) = supply_code_from_run(&run) {
                return Some(code);
            }
            run.clear();
        }
    }
    supply_code_from_run(&run)
}

/// `run` as a supply code, when a run of ASCII letters and digits has the
/// shape of one (see [`alnum_supply_code`]).
fn supply_code_from_run(run: &str) -> Option<String> {
    if SUPPLY_CODE_CHARS.contains(&run.len())
        && run.chars().any(|c| c.is_ascii_alphabetic())
        && run.chars().any(|c| c.is_ascii_digit())
    {
        if is_rf_then_digits(&run.to_ascii_uppercase()) {
            return None;
        }
        Some(run.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(text: &str) -> Option<String> {
        find_invoice_reference(text)
    }

    #[test]
    fn reference_prefers_a_labelled_supply_code_on_the_same_or_next_line() {
        assert_eq!(
            reference("Supply number: NGS000000001\nInvoice 1234567890"),
            Some("NGS000000001".into())
        );
        assert_eq!(
            reference("Κωδικός παροχής\nAB12345678\nRF12345678901234"),
            Some("AB12345678".into())
        );
        assert_eq!(
            reference("ΚΩΔΙΚΟΣ ΠΑΡΟΧΗΣ: 9 XY99887766 1"),
            Some("XY99887766".into())
        );
    }

    #[test]
    fn reference_reads_a_supply_label_glued_to_the_code_before_it() {
        assert_eq!(
            reference("SYN000000001ΗΚΑΣΠ:\nNGS000000001"),
            Some("SYN000000001".into())
        );
    }

    #[test]
    fn reference_finds_an_unlabelled_ngs_code_anywhere() {
        assert_eq!(
            reference("Total 45,90\nsome text;NGS000000042;more\nInvoice 1234567890"),
            Some("NGS000000042".into())
        );
        // Other alphanumeric codes need a label; this one falls through to the number.
        assert_eq!(
            reference("ZZ12345678 somewhere\nInvoice 1234567890"),
            Some("1234567890".into())
        );
    }

    #[test]
    fn reference_finds_an_rf_payment_code_as_a_token() {
        assert_eq!(
            reference("Pay with rf12345678901234 today\nInvoice 777777"),
            Some("RF12345678901234".into())
        );
        // Too short to be a payment code.
        assert_eq!(reference("RF1234567"), None);
        // Letters after RF: not a payment code.
        assert_eq!(reference("RFID-reader 12"), None);
    }

    #[test]
    fn reference_finds_an_rf_payment_code_glued_to_ascii_text() {
        assert_eq!(
            reference("code:RF12345678901234/end"),
            Some("RF12345678901234".into())
        );
    }

    #[test]
    fn reference_finds_an_rf_payment_code_glued_to_greek_text() {
        // The byte offset of "RF" is larger than its character offset here;
        // treating one as the other skipped into the middle of the code.
        assert_eq!(
            reference("Κωδικός πληρωμής:RF12345678901234"),
            Some("RF12345678901234".into())
        );
    }

    #[test]
    fn reference_reads_a_mark_number_on_the_same_or_next_line() {
        assert_eq!(
            reference("ΜΑΡΚ: 400001234567890\nInvoice 123456"),
            Some("400001234567890".into())
        );
        assert_eq!(
            reference("M.AR.K (mark)\n400009876543210\nInvoice 123456"),
            Some("400009876543210".into())
        );
        // A MARK label with no number nearby falls through to the labelled invoice.
        assert_eq!(
            reference("mark\nno digits here\nInvoice 123456"),
            Some("123456".into())
        );
    }

    #[test]
    fn reference_reads_a_number_next_to_an_invoice_label() {
        for label in ["Invoice", "Αρ. παραστατικού", "Αριθμός", "Number", "Ref"]
        {
            assert_eq!(
                reference(&format!("{label}: 654321")),
                Some("654321".into()),
                "{label}"
            );
        }
        // Five digits is too short to be a reference.
        assert_eq!(reference("Invoice 12345"), None);
    }

    #[test]
    fn reference_falls_back_to_the_longest_run_of_ten_or_more_digits() {
        assert_eq!(
            reference("a 1234567890\nb 123456789012\nc 12345678901"),
            Some("123456789012".into())
        );
        // Unlabelled runs under ten digits are not references.
        assert_eq!(reference("a 123456789"), None);
        // Runs over twenty digits are not tokens at all.
        assert_eq!(reference("a 123456789012345678901"), None);
        assert_eq!(reference(""), None);
    }

    /// Every label and marker constant of this file. A constant added to the
    /// file has to be added here to be checked.
    const LABELS: &[(&str, &[Keyword])] = &[
        ("SUPPLY_CODE_LABELS", SUPPLY_CODE_LABELS),
        ("MARK_LABELS", MARK_LABELS),
        ("REFERENCE_LABELS", REFERENCE_LABELS),
    ];

    #[test]
    fn every_label_and_marker_is_in_folded_form() {
        for (name, keywords) in LABELS {
            for keyword in *keywords {
                assert_eq!(
                    folded(keyword.text()),
                    keyword.text(),
                    "{name}: {keyword:?} can never match folded text"
                );
            }
        }
    }
}
