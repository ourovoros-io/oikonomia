//! Integer amount parsing for bank CSV. Never uses `f64`.
//!
//! A bank writes `1.234,56`, another `1,234.56`, a third `1 234,56 EUR` or
//! `(25.00)`. The parser reads all of them into signed minor units by
//! working on the digits as text: the fraction is padded to the currency's
//! number of decimals and the digits are parsed as one integer, so no value
//! passes through a float and none is rounded.
//!
//! The one thing a cell cannot say is whether `1.234` means one thousand or
//! one and a bit. The parser does not guess from the rest of the file. It
//! applies fixed rules, and where a cell fits no rule it is rejected, on the
//! view that a row the user has to fix is cheaper than an amount silently
//! off by a factor of a thousand.
//!
//! # Grammar
//!
//! The cell is reduced in this order, for a currency with `exponent`
//! decimals:
//!
//! | Step | Taken from the cell           | Accepted           | Rejected          |
//! |------|-------------------------------|--------------------|-------------------|
//! | 1    | Whitespace, everywhere        | `1 234,56`         | An empty cell     |
//! | 2    | Parentheses around the cell   | `(25,00)`          |                   |
//! | 3    | The signs `€ $ £ ¥ ₹ ₺ ₩`     | `€25`, `25 $`      | `₽25`             |
//! | 4    | Three ASCII letters, each end | `EUR 25`, `25 lei` | `25 kr`, `25 zł`  |
//! | 5    | One sign                      | `-25`, `+25`, `25-`| `-25-`, `25+`     |
//! | 6    | Step 4 again                  | `-EUR 25`          |                   |
//! | 7    | Nothing: digits, `.`, `,` left| `25`, `.5`, `5.`   | `1e3`, `25%`, `.` |
//!
//! Notes on the steps:
//!
//! 1. Whitespace inside the number goes too. A cell that is empty or only
//!    whitespace is a missing amount, not an invalid one.
//! 2. Parentheses mean negative. They do not cancel a minus: `(-25)` is
//!    negative.
//! 3. The signs are removed wherever they stand.
//! 4. The letters are not checked against ISO 4217. A code of another
//!    length, non-ASCII letters, and letters in the middle of the number are
//!    left in place and fail step 7.
//! 5. The sign is a leading `-`, U+2212 or `+`, or else a trailing `-` or
//!    U+2212. A second sign (`-25-`, `+25-`, `--25`) is left in place and
//!    fails step 7, as does a trailing `+`.
//! 6. The second pass lets a code stand on either side of the sign
//!    (`25 EUR-`).
//! 7. At least one digit is required. `1'234.56` fails here.
//!
//! Then the separators are read. With `exponent` 2:
//!
//! | Shape                               | Reading           | Example             |
//! |-------------------------------------|-------------------|---------------------|
//! | No separator                        | Whole major units | `25` → 2500         |
//! | Three digits after the last one     | Thousands groups  | `1.234` → 123400    |
//! | Up to `exponent` digits after it    | Decimal mark      | `1.234,56` → 123456 |
//! | Anything else                       | Rejected          | `1.2345`, `0.125`   |
//!
//! In the second row the whole number has to be one grouping on that one
//! separator (`1,234,567` is 123456700; `1,234.567` is rejected), and the
//! row does not apply when `exponent` is 3. In the third row the part before
//! the mark is plain digits (`1234.5` is 123450) or a grouping on the other
//! separator.
//!
//! A thousands grouping is strict: a first group of one to three digits that
//! does not start with `0`, then groups of exactly three. `1,2,3`, `12,34.56`
//! and `012,345` are rejected.
//!
//! With `exponent` 3 a three-digit tail is always the fraction, so `12,345`
//! is 12.345 and never twelve thousand. With `exponent` 0 no digit may
//! follow a decimal mark, so `1234.56` is rejected.
//!
//! There is no exponent notation and no percent sign. Zero is a valid
//! result here; the row parser is what refuses a zero amount.

use crate::csv::CsvError;

/// Returns the number of decimal digits (the minor-unit exponent) of an ISO
/// 4217 currency code, 2 for a code it does not know.
///
/// The code is trimmed and compared without regard to ASCII case.
///
/// This is the one exponent table of the crate: CSV import and the document
/// analyzer both read it. It knows twelve codes whose exponent is not 2:
/// `CLP`, `ISK`, `JPY`, `KRW` and `VND` have no minor unit; `BHD`, `IQD`,
/// `JOD`, `KWD`, `LYD`, `OMR` and `TND` have three decimals. Every other
/// code, known or not, gets 2 (EUR, USD).
///
/// The table is not all of ISO 4217. Other currencies without a minor unit
/// (`XOF`, `XAF`, `PYG`, `UGX`, `RWF` among them) and the four-decimal `CLF`
/// are read with two decimals, so a book in one of them has its amounts off
/// by a power of ten.
#[must_use]
pub fn currency_minor_exponent(code: &str) -> u8 {
    let code = code.trim().to_ascii_uppercase();
    if matches!(code.as_str(), "CLP" | "ISK" | "JPY" | "KRW" | "VND") {
        0
    } else if matches!(
        code.as_str(),
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND"
    ) {
        3
    } else {
        2
    }
}

/// Parses a bank-CSV amount into signed minor units of a currency with
/// `exponent` decimals.
///
/// The table in the [`crate::csv`] module doc lists the accepted forms.
///
/// The last `.` or `,` followed by at most `exponent` digits is the decimal
/// mark, and the digits before it are either unseparated or grouped in
/// thousands by the other separator. A last separator followed by exactly
/// three digits (when `exponent != 3`) is a thousands separator instead, and
/// then every separator in the cell must be that same character.
///
/// Thousands groups are strict: one to three digits with no leading zero,
/// then groups of exactly three. Anything else is rejected instead of
/// guessed at, so `0.125`, `1,234.567` and `1,2,3.45` are invalid with
/// `exponent == 2`.
///
/// With `exponent == 3` three digits after the last separator are always
/// the fraction, so `12,345` is 12.345 and never twelve thousand; that
/// reading is a rule of this parser, not something the cell can settle.
///
/// Supported examples with `exponent == 2`:
/// - `1.234,56` / `1234,56` / `1234.56` / `1,234.56`
/// - `1.234` / `1,234` → `123400`
/// - `-25` → `-2500`
///
/// Whitespace and the currency signs `€$£¥₹₺₩` are ignored. So are three
/// ASCII letters directly before or after the number; they are not checked
/// against the ISO 4217 list, so `1.00 abc` parses like `1.00 EUR`.
/// Parentheses mean negative (`(25,00)`), as does one minus, written as `-`
/// or U+2212 MINUS SIGN, before the number or after it (`25-`). A second
/// sign makes the cell invalid.
///
/// # Errors
///
/// [`CsvError::MissingAmount`] when the cell is empty or only whitespace;
/// [`CsvError::InvalidAmount`] when the cell is not a number, including a
/// cell with a separator but no digit;
/// [`CsvError::AmountOverflow`] when the magnitude does not fit in `i64`.
pub fn parse_signed_minor(raw: &str, exponent: u8) -> Result<i64, CsvError> {
    let (negative, digits) = prepare_amount(raw)?;
    let (integer_digits, fraction_digits) =
        split_decimal(&digits, exponent).ok_or_else(|| CsvError::InvalidAmount(raw.to_owned()))?;

    // Checked before the fraction is padded: the padding zeros would turn a
    // bare `.` or `,` into a zero amount.
    if integer_digits.is_empty() && fraction_digits.is_empty() {
        return Err(CsvError::InvalidAmount(raw.to_owned()));
    }

    let mut fraction = fraction_digits.to_owned();
    while fraction.len() < usize::from(exponent) {
        fraction.push('0');
    }

    let combined = format!("{integer_digits}{fraction}");
    let magnitude: i64 = match combined.parse() {
        Ok(value) => value,
        Err(_) => return Err(CsvError::AmountOverflow),
    };
    if negative {
        magnitude.checked_neg().ok_or(CsvError::AmountOverflow)
    } else {
        Ok(magnitude)
    }
}

/// Runs steps 1 to 7 of the grammar in the module doc: returns whether the
/// amount is negative and the bare digits and separators of its magnitude.
///
/// # Errors
///
/// [`CsvError::MissingAmount`] when nothing but whitespace is in the cell;
/// [`CsvError::InvalidAmount`], carrying the cell as written, when anything
/// other than digits and separators is left over.
fn prepare_amount(raw: &str) -> Result<(bool, String), CsvError> {
    let compact: String = raw
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if compact.is_empty() {
        return Err(CsvError::MissingAmount);
    }

    let (parenthesized, unwrapped) = strip_parentheses(&compact);
    let without_symbols = strip_currency_symbols(unwrapped);
    // A code is dropped on both sides of the sign, so `EUR -12`, `-EUR 12`,
    // `12- EUR` and `12 EUR-` all leave the bare digits.
    let (signed_negative, unsigned) = strip_sign(strip_letter_code(&without_symbols));
    let body = strip_letter_code(unsigned);

    let is_digit_or_separator =
        |character: char| character.is_ascii_digit() || character == '.' || character == ',';
    if body.is_empty() || !body.chars().all(is_digit_or_separator) {
        return Err(CsvError::InvalidAmount(raw.to_owned()));
    }
    Ok((parenthesized || signed_negative, body.to_owned()))
}

/// Removes one pair of parentheses around `text` and returns whether there
/// was one. In accounting notation a wrapped amount is negative.
fn strip_parentheses(text: &str) -> (bool, &str) {
    let inner = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'));
    match inner {
        Some(inner) => (true, inner),
        None => (false, text),
    }
}

/// Currency signs dropped from an amount cell, wherever they stand.
///
/// The rule: every currency of the book picker (`web/src/lib/currencies.ts`)
/// that has a sign of its own is here: `€` EUR, `$` USD, CAD and AUD, `£`
/// GBP, `¥` JPY and CNY, `₹` INR, `₺` TRY. `₩` is the one sign beyond the
/// picker; core accepts any three-letter code for a book, and KRW is in
/// [`currency_minor_exponent`]'s table.
///
/// The picker's other currencies are written with letters (`kr`, `zł`, `Kč`,
/// `Ft`, `lei`, `R$`, `CHF`). Letters are never treated as a sign; they pass
/// only as a three-letter code (see [`strip_letter_code`]).
const CURRENCY_SIGNS: [char; 7] = ['€', '$', '£', '¥', '₹', '₺', '₩'];

/// Returns `text` without the [`CURRENCY_SIGNS`].
fn strip_currency_symbols(text: &str) -> String {
    text.chars()
        .filter(|character| !CURRENCY_SIGNS.contains(character))
        .collect()
}

/// The two characters bank exports write a minus with: the ASCII hyphen and
/// U+2212 MINUS SIGN.
const MINUS_SIGNS: [char; 2] = ['-', '\u{2212}'];

/// Splits off one sign and returns whether it was a minus: a leading `-` or
/// `+`, or else a trailing `-`.
///
/// Only one sign is taken. A second one stays in the text, where the caller's
/// digits-and-separators check rejects it, so `-25-` and `+25-` are not
/// amounts.
fn strip_sign(text: &str) -> (bool, &str) {
    if let Some(rest) = text.strip_prefix(MINUS_SIGNS) {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else if let Some(rest) = text.strip_suffix(MINUS_SIGNS) {
        (true, rest)
    } else {
        (false, text)
    }
}

/// Drops a three-letter code from the front, and one from the back of what
/// is left.
///
/// A cell that is nothing but three letters is returned as it is. It has no
/// digits, so the caller rejects it as an invalid amount either way.
fn strip_letter_code(text: &str) -> &str {
    let Some((code, rest)) = split_leading_letter_code(text) else {
        return strip_trailing_letter_code(text);
    };
    if rest.is_empty() {
        return code;
    }
    strip_trailing_letter_code(rest)
}

/// Splits `text` into a three-letter code at its start and the rest, or
/// returns `None` when it does not start with one.
///
/// `split_at_checked` is `None` for a text shorter than three bytes and when
/// byte 3 falls inside a multi-byte character; in both cases the start is
/// not three ASCII letters.
fn split_leading_letter_code(text: &str) -> Option<(&str, &str)> {
    let (code, rest) = text.split_at_checked(3)?;
    is_letter_code(code).then_some((code, rest))
}

/// Returns `text` without a three-letter code at its end.
fn strip_trailing_letter_code(text: &str) -> &str {
    // `split_at_checked` is `None` when three bytes from the end falls inside
    // a multi-byte character, which also means the tail is not three letters.
    let split = text
        .len()
        .checked_sub(3)
        .and_then(|code_start| text.split_at_checked(code_start));
    match split {
        Some((head, code)) if is_letter_code(code) => head,
        _ => text,
    }
}

/// Whether `text` is exactly three ASCII letters, in either case.
///
/// The shape of an ISO 4217 code. The letters are not looked up or compared
/// with anything: the book's currency, not the cell, decides how the digits
/// are read, so a cell marked `USD` in a EUR book is read as euros.
fn is_letter_code(text: &str) -> bool {
    text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// Splits a body of digits, `.` and `,` into integer and fraction digits,
/// or returns `None` when the separators do not form an amount.
///
/// The last separator is the decimal mark unless it is followed by exactly
/// three digits in a currency without three decimals; then the whole body
/// has to be a thousands grouping, because `0.125` or `1,234.567` read as
/// thousands would silently multiply the amount.
fn split_decimal(body: &str, exponent: u8) -> Option<(String, &str)> {
    let Some(decimal_mark_at) = body.rfind(['.', ',']) else {
        return Some((body.to_owned(), ""));
    };
    let (integer_part, marked_fraction) = body.split_at(decimal_mark_at);
    let (decimal_mark, fraction) = marked_fraction.split_at(1);

    if fraction.len() == 3 && exponent != 3 {
        return Some((grouped_digits(body, decimal_mark)?, ""));
    }
    if fraction.len() > usize::from(exponent) {
        return None;
    }

    let integer_digits = if integer_part.contains(['.', ',']) {
        let separator = if decimal_mark == "." { "," } else { "." };
        grouped_digits(integer_part, separator)?
    } else {
        integer_part.to_owned()
    };
    Some((integer_digits, fraction))
}

/// Returns the digits of `grouped` when it is a thousands grouping on
/// `separator`, and `None` otherwise.
///
/// A grouping is a first group of one to three digits with no leading zero,
/// then groups of exactly three digits. The other separator anywhere in it
/// makes it `None`.
fn grouped_digits(grouped: &str, separator: &str) -> Option<String> {
    let all_digits = |group: &str| group.bytes().all(|byte| byte.is_ascii_digit());

    let mut groups = grouped.split(separator);
    let first = groups.next()?;
    if !(1..=3).contains(&first.len()) || first.starts_with('0') || !all_digits(first) {
        return None;
    }

    let mut digits = first.to_owned();
    for group in groups {
        if group.len() != 3 || !all_digits(group) {
            return None;
        }
        digits.push_str(group);
    }
    Some(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_eur_minor(raw: &str) -> i64 {
        parse_signed_minor(raw, 2).expect(raw)
    }

    #[test]
    fn comma_and_dot_decimals() {
        assert_eq!(parse_eur_minor("1.234,56"), 123_456);
        assert_eq!(parse_eur_minor("1234,56"), 123_456);
        assert_eq!(parse_eur_minor("1234.56"), 123_456);
        assert_eq!(parse_eur_minor("1,234.56"), 123_456);
        assert_eq!(parse_eur_minor("1.234"), 123_400);
        assert_eq!(parse_eur_minor("1,234"), 123_400);
        assert_eq!(parse_eur_minor("25"), 2_500);
        assert_eq!(parse_eur_minor("25.5"), 2_550);
        assert_eq!(parse_eur_minor("25,5"), 2_550);
    }

    #[test]
    fn signs_symbols_and_parens() {
        assert_eq!(parse_eur_minor("-25.00"), -2_500);
        assert_eq!(parse_eur_minor("+25.00"), 2_500);
        assert_eq!(parse_eur_minor("(25,00)"), -2_500);
        assert_eq!(parse_eur_minor("€1.234,56"), 123_456);
        assert_eq!(parse_eur_minor("1234.56EUR"), 123_456);
        assert_eq!(parse_eur_minor("EUR -12.00"), -1_200);
        assert_eq!(parse_eur_minor("1 234,56"), 123_456);
    }

    #[test]
    fn zero_and_overflow_and_junk() {
        assert_eq!(parse_eur_minor("0"), 0);
        assert_eq!(parse_eur_minor("0,00"), 0);
        assert!(matches!(
            parse_signed_minor("abc", 2),
            Err(CsvError::InvalidAmount(_))
        ));
        assert_eq!(parse_signed_minor("", 2), Err(CsvError::MissingAmount));
        assert_eq!(parse_signed_minor("  ", 2), Err(CsvError::MissingAmount));
        assert!(matches!(
            parse_signed_minor("1.2345", 2),
            Err(CsvError::InvalidAmount(_))
        ));
        let too_big = "9".repeat(20);
        assert_eq!(
            parse_signed_minor(&too_big, 2),
            Err(CsvError::AmountOverflow)
        );
    }

    fn assert_invalid_amount(raw: &str, exponent: u8) {
        let parsed = parse_signed_minor(raw, exponent);
        assert!(
            matches!(parsed, Err(CsvError::InvalidAmount(_))),
            "{raw:?} with exponent {exponent} must be an invalid amount, got {parsed:?}"
        );
    }

    #[test]
    fn multi_byte_cells_are_rejected_without_panicking() {
        assert_invalid_amount("₽50", 2);
        assert_invalid_amount("éé", 2);
        assert_invalid_amount("50₽", 2);
        assert_invalid_amount("é", 2);
        assert_invalid_amount("(é)", 2);
    }

    #[test]
    fn unicode_minus_sign_is_negative() {
        assert_eq!(parse_eur_minor("\u{2212}25"), -2_500);
        assert_eq!(parse_eur_minor("\u{2212}1.234,56"), -123_456);
        assert_eq!(parse_eur_minor("EUR \u{2212}12.00"), -1_200);
        assert_eq!(parse_eur_minor("€\u{2212}3,50"), -350);
    }

    #[test]
    fn the_sign_of_every_currency_in_the_picker_is_ignored() {
        // EUR, USD/CAD/AUD, GBP, JPY/CNY, INR and TRY: the currencies of
        // `web/src/lib/currencies.ts` that have a sign of their own.
        for sign in ['€', '$', '£', '¥', '₹', '₺'] {
            assert_eq!(parse_eur_minor(&format!("{sign}25,00")), 2_500, "{sign}");
            assert_eq!(parse_eur_minor(&format!("25,00 {sign}")), 2_500, "{sign}");
            assert_eq!(parse_eur_minor(&format!("-{sign}25,00")), -2_500, "{sign}");
        }
    }

    #[test]
    fn a_currency_written_with_letters_is_read_only_as_a_three_letter_code() {
        assert_eq!(parse_eur_minor("25,00 lei"), 2_500);
        assert_eq!(parse_eur_minor("CHF 25.00"), 2_500);

        assert_invalid_amount("25,00 zł", 2);
        assert_invalid_amount("25 kr", 2);
        assert_invalid_amount("25 Ft", 2);
        assert_invalid_amount("R$ 25,00", 2);
    }

    #[test]
    fn the_examples_of_the_grammar_table_hold() {
        assert_eq!(parse_eur_minor("25 $"), 2_500);
        assert_eq!(parse_eur_minor("-EUR 25"), -2_500);
        assert_eq!(parse_eur_minor("(-25)"), -2_500);
        assert_eq!(parse_eur_minor("1234.5"), 123_450);
        assert_eq!(parse_eur_minor("1,234,567"), 123_456_700);

        assert_invalid_amount("1e3", 2);
        assert_invalid_amount("1'234.56", 2);
        assert_invalid_amount("25%", 2);
        assert_invalid_amount("1,2,3", 2);
    }

    #[test]
    fn a_trailing_minus_is_negative() {
        assert_eq!(parse_eur_minor("25-"), -2_500);
        assert_eq!(parse_eur_minor("1.234,56-"), -123_456);
        assert_eq!(parse_eur_minor("25\u{2212}"), -2_500);
        assert_eq!(parse_eur_minor("25 -"), -2_500);
        assert_eq!(parse_eur_minor("€25-"), -2_500);
    }

    #[test]
    fn a_trailing_minus_is_read_on_either_side_of_a_currency_code() {
        assert_eq!(parse_eur_minor("25- EUR"), -2_500);
        assert_eq!(parse_eur_minor("25 EUR-"), -2_500);
        assert_eq!(parse_eur_minor("EUR 25-"), -2_500);
        assert_eq!(parse_eur_minor("12,00-EUR"), -1_200);
    }

    #[test]
    fn a_second_sign_is_rejected() {
        assert_invalid_amount("-25-", 2);
        assert_invalid_amount("+25-", 2);
        assert_invalid_amount("25--", 2);
        assert_invalid_amount("--25", 2);
        assert_invalid_amount("\u{2212}25-", 2);
        assert_invalid_amount("-25- EUR", 2);
        assert_invalid_amount("25+", 2);
        assert_invalid_amount("-", 2);
    }

    #[test]
    fn three_digit_tail_is_thousands_only_in_a_well_formed_grouping() {
        assert_eq!(parse_eur_minor("1.234.567"), 123_456_700);
        assert_eq!(parse_eur_minor("12,345"), 1_234_500);
        assert_eq!(parse_eur_minor("123.456"), 12_345_600);

        assert_invalid_amount("0.125", 2);
        assert_invalid_amount("0.500", 2);
        assert_invalid_amount("1,234.567", 2);
        assert_invalid_amount("1234.567", 2);
        assert_invalid_amount("012,345", 2);
        assert_invalid_amount("0.125", 0);
    }

    #[test]
    fn an_invalid_amount_reports_the_cell_as_written() {
        for raw in ["0.125 EUR", "€ 1,2,3.45", "(1.2345)", "12 abc 34"] {
            assert_eq!(
                parse_signed_minor(raw, 2),
                Err(CsvError::InvalidAmount(raw.to_owned()))
            );
        }
    }

    #[test]
    fn malformed_grouping_is_rejected() {
        assert_invalid_amount("1,2,3.45", 2);
        assert_invalid_amount("1.2.3", 2);
        assert_invalid_amount("1.234.56", 2);
        assert_invalid_amount("12,34.56", 2);
        assert_invalid_amount("0.123,45", 2);
        assert_invalid_amount("1,234,567", 3);
    }

    #[test]
    fn three_decimal_currencies_read_a_three_digit_tail_as_the_fraction() {
        assert_eq!(parse_signed_minor("0.125", 3).expect("dinar"), 125);
        assert_eq!(
            parse_signed_minor("1,234.567", 3).expect("dinar"),
            1_234_567
        );
    }

    #[test]
    fn a_separator_without_digits_is_not_zero() {
        assert_invalid_amount(".", 2);
        assert_invalid_amount(",", 2);
        assert_invalid_amount("-.", 2);
        assert_invalid_amount("€,", 2);
        assert_invalid_amount(".", 0);
        assert_eq!(parse_eur_minor(".5"), 50);
        assert_eq!(parse_eur_minor("5."), 500);
    }

    #[test]
    fn yen_has_no_decimal() {
        assert_eq!(parse_signed_minor("1,234", 0).expect("yen"), 1_234);
        assert!(parse_signed_minor("1234.56", 0).is_err());
    }

    #[test]
    fn exponent_lookup() {
        assert_eq!(currency_minor_exponent("eur"), 2);
        assert_eq!(currency_minor_exponent("JPY"), 0);
        assert_eq!(currency_minor_exponent("KWD"), 3);
        assert_eq!(currency_minor_exponent("XXX"), 2);
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// `minor` written with two decimals after `decimal`, the whole part
    /// grouped in thousands by `group` (not grouped when `group` is empty).
    fn written(minor: i64, group: &str, decimal: char) -> String {
        let magnitude = minor.unsigned_abs();
        let whole = (magnitude / 100).to_string();
        let groups: Vec<&str> = whole
            .as_bytes()
            .rchunks(3)
            .rev()
            .map(|digits| std::str::from_utf8(digits).unwrap())
            .collect();

        format!("{}{decimal}{:02}", groups.join(group), magnitude % 100)
    }

    /// The four separator conventions the parser documents for two decimals.
    const CONVENTIONS: [(&str, char); 4] = [("", '.'), ("", ','), (",", '.'), (".", ',')];

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn parsing_any_text_returns_instead_of_panicking(
            raw in any::<String>(),
            exponent in any::<u8>(),
        ) {
            let _ = parse_signed_minor(&raw, exponent);
        }

        #[test]
        fn parsing_amount_shaped_text_returns_instead_of_panicking(
            raw in "[0-9.,()€$ A-Za-z\u{2212}-]{0,24}",
            exponent in 0_u8..=4,
        ) {
            let _ = parse_signed_minor(&raw, exponent);
        }

        // `i64::MIN` is left out: its magnitude does not fit an `i64`, and the
        // parser reports that as an overflow.
        #[test]
        fn an_amount_written_in_any_convention_parses_back(minor in (i64::MIN + 1)..=i64::MAX) {
            for (group, decimal) in CONVENTIONS {
                let unsigned = written(minor, group, decimal);
                let signed = if minor < 0 { format!("-{unsigned}") } else { unsigned.clone() };
                prop_assert_eq!(parse_signed_minor(&signed, 2), Ok(minor), "{}", signed);

                if minor < 0 {
                    let bracketed = format!("({unsigned})");
                    prop_assert_eq!(parse_signed_minor(&bracketed, 2), Ok(minor), "{}", bracketed);

                    let trailing = format!("{unsigned}-");
                    prop_assert_eq!(parse_signed_minor(&trailing, 2), Ok(minor), "{}", trailing);
                }
            }
        }
    }
}
