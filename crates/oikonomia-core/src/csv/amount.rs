//! Integer amount parsing for bank CSV. Never uses `f64`.

use super::CsvError;

/// Decimal digits (minor-unit exponent) for an ISO 4217 code.
///
/// This is the one exponent table of the crate: CSV import and the document
/// analyzer both read it. It lists the codes whose exponent is not 2, as
/// given in ISO 4217 list one (published 2026-09-17): `CLP`, `ISK`, `JPY`,
/// `KRW` and `VND` have no minor unit; `BHD`, `IQD`, `JOD`, `KWD`, `LYD`,
/// `OMR` and `TND` have three decimals. Every other code, known or not,
/// gets 2 (EUR, USD).
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

/// Parse a bank-CSV amount into signed minor units.
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
/// Whitespace and the currency symbols `€$£¥₹₩` are ignored. So are three
/// ASCII letters directly before or after the number; they are not checked
/// against the ISO 4217 list, so `1.00 abc` parses like `1.00 EUR`.
/// Parentheses mean negative (`(25,00)`), as do a leading `-` and a leading
/// U+2212 MINUS SIGN.
///
/// # Errors
///
/// [`CsvError::MissingAmount`] when the cell is empty or only whitespace;
/// [`CsvError::InvalidAmount`] when the cell is not a number, including a
/// cell with a separator but no digit;
/// [`CsvError::AmountOverflow`] when the magnitude does not fit in `i64`.
pub fn parse_signed_minor(raw: &str, exponent: u8) -> Result<i64, CsvError> {
    let (negative, digits) = prepare_amount(raw)?;
    let (int_digits, frac_digits) =
        split_decimal(&digits, exponent).ok_or_else(|| CsvError::InvalidAmount(raw.to_owned()))?;

    // Checked before the fraction is padded: the padding zeros would turn a
    // bare `.` or `,` into a zero amount.
    if int_digits.is_empty() && frac_digits.is_empty() {
        return Err(CsvError::InvalidAmount(raw.to_owned()));
    }

    let mut frac = frac_digits.to_owned();
    while frac.len() < usize::from(exponent) {
        frac.push('0');
    }

    let combined = format!("{int_digits}{frac}");
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

fn prepare_amount(raw: &str) -> Result<(bool, String), CsvError> {
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err(CsvError::MissingAmount);
    }

    let (parenthesized, unwrapped) = strip_parentheses(&compact);
    let without_symbols = strip_currency_symbols(unwrapped);
    let (signed_negative, unsigned) = strip_sign(strip_letter_code(&without_symbols));
    let body = strip_letter_code(unsigned);

    if body.is_empty()
        || !body
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return Err(CsvError::InvalidAmount(raw.to_owned()));
    }
    Ok((parenthesized || signed_negative, body.to_owned()))
}

/// Accounting notation: an amount wrapped in parentheses is negative.
fn strip_parentheses(s: &str) -> (bool, &str) {
    match s.strip_prefix('(').and_then(|rest| rest.strip_suffix(')')) {
        Some(inner) => (true, inner),
        None => (false, s),
    }
}

fn strip_currency_symbols(s: &str) -> String {
    const SYMBOLS: [char; 6] = ['€', '$', '£', '¥', '₹', '₩'];
    s.chars().filter(|c| !SYMBOLS.contains(c)).collect()
}

/// Splits off a leading sign. Bank exports write the minus as either the
/// ASCII hyphen or U+2212 MINUS SIGN.
fn strip_sign(s: &str) -> (bool, &str) {
    if let Some(rest) = s.strip_prefix(['-', '\u{2212}']) {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else {
        (false, s)
    }
}

/// Drops a three-letter code from the front, and one from the back of what
/// is left.
///
/// A cell that is nothing but three letters is kept, so it is reported as an
/// invalid amount instead of an empty one.
fn strip_letter_code(s: &str) -> &str {
    let Some((code, rest)) = split_leading_letter_code(s) else {
        return strip_trailing_letter_code(s);
    };
    if rest.is_empty() {
        return code;
    }
    strip_trailing_letter_code(rest)
}

fn split_leading_letter_code(s: &str) -> Option<(&str, &str)> {
    let (code, rest) = s.split_at_checked(3)?;
    is_letter_code(code).then_some((code, rest))
}

fn strip_trailing_letter_code(s: &str) -> &str {
    // `split_at_checked` is `None` when three bytes from the end falls inside
    // a multi-byte character, which also means the tail is not three letters.
    let split = s.len().checked_sub(3).and_then(|at| s.split_at_checked(at));
    match split {
        Some((head, code)) if is_letter_code(code) => head,
        _ => s,
    }
}

fn is_letter_code(s: &str) -> bool {
    s.len() == 3 && s.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// Splits an all-ASCII body of digits, `.` and `,` into integer digits and
/// fraction digits, or returns `None` when the separators do not form an
/// amount.
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
/// `separator`: a first group of one to three digits with no leading zero,
/// then groups of exactly three digits. Any other separator makes it `None`.
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
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
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
