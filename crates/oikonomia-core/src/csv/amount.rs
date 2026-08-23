//! Integer amount parsing for bank CSV. Never uses `f64`.

use super::CsvError;

/// Decimal digits (minor-unit exponent) for an ISO 4217 code.
///
/// Unknown codes default to 2 (EUR/USD). JPY/KRW-style currencies use 0;
/// a short list of three-decimal Gulf dinars uses 3.
#[must_use]
pub fn currency_minor_exponent(code: &str) -> u8 {
    let code = code.trim().to_ascii_uppercase();
    if matches!(code.as_str(), "JPY" | "KRW" | "VND" | "CLP") {
        0
    } else if matches!(code.as_str(), "BHD" | "JOD" | "KWD" | "OMR" | "TND") {
        3
    } else {
        2
    }
}

/// Parse a bank-CSV amount into signed minor units.
///
/// The last `.` or `,` followed by at most `exponent` digits is the decimal
/// mark; a trailing group of three digits (when `exponent != 3`) is thousands,
/// not a decimal. Other separators are thousands grouping and are stripped.
///
/// Supported examples with `exponent == 2`:
/// - `1.234,56` / `1234,56` / `1234.56` / `1,234.56`
/// - `-25` → `-2500`
///
/// Currency symbols (`€$£`) and trailing/leading ISO codes are ignored.
/// Parentheses mean negative (`(25,00)`).
///
/// # Errors
///
/// [`CsvError::InvalidAmount`] when the cell is not a number;
/// [`CsvError::AmountOverflow`] when the magnitude does not fit in `i64`.
pub fn parse_signed_minor(raw: &str, exponent: u8) -> Result<i64, CsvError> {
    let (negative, digits) = prepare_amount(raw)?;
    let (int_digits, frac_digits) = split_decimal(&digits, exponent)?;

    let mut frac = frac_digits.to_owned();
    while frac.len() < usize::from(exponent) {
        frac.push('0');
    }

    let combined = format!("{int_digits}{frac}");
    if combined.is_empty() {
        return Err(CsvError::InvalidAmount(raw.to_owned()));
    }
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
    let compact: String = raw.trim().chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err(CsvError::InvalidAmount(raw.to_owned()));
    }

    let mut negative = false;
    let mut body = compact;
    if body.starts_with('(') && body.ends_with(')') && body.len() >= 2 {
        negative = true;
        body = body[1..body.len() - 1].to_owned();
    }

    body = strip_currency_noise(&body);
    if let Some(rest) = body.strip_prefix('-') {
        negative = true;
        body = rest.to_owned();
    } else if let Some(rest) = body.strip_prefix('+') {
        body = rest.to_owned();
    }
    body = strip_currency_noise(&body);

    if body.is_empty()
        || !body
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return Err(CsvError::InvalidAmount(raw.to_owned()));
    }
    Ok((negative, body))
}

fn strip_currency_noise(s: &str) -> String {
    const SYMBOLS: [char; 6] = ['€', '$', '£', '¥', '₹', '₩'];
    let stripped: String = s.chars().filter(|c| !SYMBOLS.contains(c)).collect();
    strip_iso_code(&stripped)
}

fn strip_iso_code(s: &str) -> String {
    let Some((first, rest)) = split_leading_iso(s) else {
        return strip_trailing_iso(s);
    };
    if rest.is_empty() {
        return first.to_owned();
    }
    strip_trailing_iso(rest)
}

fn split_leading_iso(s: &str) -> Option<(&str, &str)> {
    let mut chars = s.chars();
    let a = chars.next()?;
    let b = chars.next()?;
    let c = chars.next()?;
    if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() && c.is_ascii_alphabetic() {
        Some((&s[..3], &s[3..]))
    } else {
        None
    }
}

fn strip_trailing_iso(s: &str) -> String {
    if s.len() < 3 {
        return s.to_owned();
    }
    let tail_start = s.len() - 3;
    if s[tail_start..].chars().all(|c| c.is_ascii_alphabetic()) {
        return s[..tail_start].to_owned();
    }
    s.to_owned()
}

fn split_decimal(body: &str, exponent: u8) -> Result<(String, &str), CsvError> {
    let Some(sep_at) = body.rfind(['.', ',']) else {
        let int_digits: String = body.chars().filter(char::is_ascii_digit).collect();
        return Ok((int_digits, ""));
    };
    let frac = &body[sep_at + 1..];
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(CsvError::InvalidAmount(body.to_owned()));
    }

    let thousands_group = frac.len() == 3 && exponent != 3;
    if thousands_group {
        let int_digits: String = body.chars().filter(char::is_ascii_digit).collect();
        return Ok((int_digits, ""));
    }
    if frac.len() > usize::from(exponent) {
        return Err(CsvError::InvalidAmount(body.to_owned()));
    }

    let int_part = &body[..sep_at];
    let int_digits: String = int_part.chars().filter(char::is_ascii_digit).collect();
    Ok((int_digits, frac))
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
        assert!(matches!(
            parse_signed_minor("", 2),
            Err(CsvError::InvalidAmount(_))
        ));
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
