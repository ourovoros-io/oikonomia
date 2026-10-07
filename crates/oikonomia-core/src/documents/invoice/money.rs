//! Money tokens: from a line of text to the amounts written on it.
//!
//! [`money_amounts_on_line`] blanks dates, clock times and percentages,
//! cuts what is left into runs of digits, `,` and `.`, and reads each run by
//! the separator table in the
//! [module above](crate::documents::invoice#reading-an-amount)
//! ([`parse_money_token`]). Amounts are minor units of a two-decimal
//! currency.
//!
//! Nothing here chooses between amounts. The one judgement made is the
//! plausibility band ([`PLAUSIBLE_MONEY_MINOR`]), which the stages that do
//! choose apply through [`is_plausible_money`].

use std::ops::RangeInclusive;

use crate::documents::invoice::dates::{mask_date_tokens, mask_time_tokens};
use crate::documents::invoice::normalization::THOUSANDS_GROUP_DIGITS;

/// Minor units in one euro. The reader always reads two decimals.
const MINOR_PER_EURO: i64 = 100;

/// The amounts taken as money: 0,50 to 10 000 000,00, in minor units.
///
/// Every stage that picks an amount ignores a number outside this band. Its
/// purpose is to keep out numbers that are not currency amounts, at the cost
/// of not reading a real total or fee under 0,50.
///
/// The reasons for these two bounds in particular are not recorded. Tests
/// pin 1,40 and 1 234 567,89 as inside and 0,40 as outside; none reaches the
/// upper bound.
pub(super) const PLAUSIBLE_MONEY_MINOR: RangeInclusive<i64> = 50..=1_000_000_000;

/// Whether `minor` is not a whole number of euros.
pub(super) const fn has_cents(minor: i64) -> bool {
    minor % MINOR_PER_EURO != 0
}

/// Whether `minor` is in [`PLAUSIBLE_MONEY_MINOR`].
pub(super) fn is_plausible_money(minor: i64) -> bool {
    PLAUSIBLE_MONEY_MINOR.contains(&minor)
}

/// The largest plausible amount written on `line`, or `None` when it has
/// none. An empty line has none.
pub(super) fn largest_plausible_amount(line: &str) -> Option<i64> {
    money_amounts_on_line(line)
        .into_iter()
        .filter(|amount| is_plausible_money(*amount))
        .max()
}

/// Blanks out percentages (`24%`, `13,5 %`) so a rate is never an amount.
fn mask_percent_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut masked = String::with_capacity(line.len());
    let mut index = 0;

    while let Some(&current) = chars.get(index) {
        if let Some(len) = percent_len_at(&chars, index) {
            masked.push_str(&" ".repeat(len));
            index += len;
        } else {
            masked.push(current);
            index += 1;
        }
    }
    masked
}

/// Length of the percentage that starts at `index`: a number, any spaces
/// after it, and the `%` sign.
fn percent_len_at(chars: &[char], index: usize) -> Option<usize> {
    let is_number_char = |c: &char| c.is_ascii_digit() || matches!(c, ',' | '.');

    // Digits right after a separator are not masked here even when no number
    // precedes it ("Φ.Π.Α.24%"): the money tokenizer reads them as ".24",
    // which is not an amount.
    let continues_a_number = index
        .checked_sub(1)
        .and_then(|before| chars.get(before))
        .is_some_and(is_number_char);
    if continues_a_number || !chars.get(index)?.is_ascii_digit() {
        return None;
    }

    let number_len = chars
        .get(index..)?
        .iter()
        .take_while(|c| is_number_char(c))
        .count();
    let spaces = chars
        .get(index + number_len..)?
        .iter()
        .take_while(|c| **c == ' ')
        .count();

    let percent_at = index + number_len + spaces;
    (chars.get(percent_at) == Some(&'%')).then_some(percent_at + 1 - index)
}

/// Every money amount written on `line`, in minor units, in the order
/// written.
///
/// Dates, clock times and percentages are blanked first. What is left is cut
/// into runs of digits, `,` and `.`, and each run is read by
/// [`parse_money_token`]; a run that is not money is dropped. The amounts
/// are not tested for plausibility here.
pub(super) fn money_amounts_on_line(line: &str) -> Vec<i64> {
    let line = mask_percent_tokens(&mask_time_tokens(&mask_date_tokens(line)));
    let mut amounts = Vec::new();
    let mut token = String::new();
    for character in line.chars() {
        if character.is_ascii_digit() || character == '.' || character == ',' {
            token.push(character);
        } else {
            if let Some(amount) = parse_money_token(&token) {
                amounts.push(amount);
            }
            token.clear();
        }
    }
    if let Some(amount) = parse_money_token(&token) {
        amounts.push(amount);
    }
    amounts
}

/// Most characters of a money token.
///
/// The longest token the other rules accept is `12.345.678,90`, with 13.
/// Why the limit is 14 and not 13 is not recorded; no test tells them apart.
const MAX_MONEY_TOKEN_CHARS: usize = 14;

/// Most digits of a token without separators that is read as whole euros.
///
/// A longer run of digits is an identifier (a tax number, a MARK number, a
/// piece of an IBAN), so a whole amount of 100 000 euros or more is read
/// only when it is written with a separator.
/// `a_money_token_is_read_by_the_separator_table` pins both sides: `99999`
/// is money and `123456` is not.
const MAX_BARE_EURO_DIGITS: usize = 5;

/// Whole-euro figures that are taken as years, not money.
///
/// A date that the masks did not recognize leaves its year behind as a token
/// of its own. The price is that a total of exactly 1900 to 2100 euros is
/// read only when it is written with decimals (`2026,00`).
/// `a_whole_amount_that_looks_like_a_year_needs_decimals` pins both ends.
pub(super) const YEAR_LIKE_EUROS: RangeInclusive<i64> = 1_900..=2_100;

/// Most digits of the whole part of a token with separators, after the
/// thousands marks are removed. Eight digits reach 99 999 999,99, which is
/// already above [`PLAUSIBLE_MONEY_MINOR`]; the table test pins that nine
/// are refused (`123456789,00`).
const MAX_WHOLE_PART_DIGITS: usize = 8;

/// Most digits of a fraction. Three or more after a lone separator make a
/// thousands group or no amount at all, per the module table.
const MAX_CENT_DIGITS: usize = 2;

/// Reads one run of digits, `,` and `.` as minor units, by the table in the
/// module documentation ("Reading an amount").
///
/// Trailing separators are dropped first. `None` when the token is empty,
/// longer than [`MAX_MONEY_TOKEN_CHARS`], or not money by the table.
pub(super) fn parse_money_token(token: &str) -> Option<i64> {
    // The tokenizer keeps `.` and `,`, so an amount that ends a sentence or a
    // list item arrives with that punctuation attached.
    let token = token.trim().trim_end_matches(['.', ',']);
    if token.is_empty() || token.len() > MAX_MONEY_TOKEN_CHARS {
        return None;
    }

    if token.contains([',', '.']) {
        separated_to_minor(token)
    } else {
        whole_euros_to_minor(token)
    }
}

/// Reads a token of digits only as whole euros.
///
/// `None` for more than [`MAX_BARE_EURO_DIGITS`] digits, for a leading zero
/// on more than one digit (`08` is a piece of a date), and for a figure in
/// [`YEAR_LIKE_EUROS`].
fn whole_euros_to_minor(digits: &str) -> Option<i64> {
    if digits.len() > MAX_BARE_EURO_DIGITS {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }

    let whole: i64 = digits.parse().ok()?;
    if YEAR_LIKE_EUROS.contains(&whole) {
        return None;
    }
    whole.checked_mul(MINOR_PER_EURO)
}

/// The role each separator plays in a money token, per the module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeparatorRoles {
    /// The decimal mark, when the token has a fraction.
    decimal: Option<char>,
    /// The mark between thousands groups.
    grouping: char,
}

/// Decides which separator of `token` is the decimal mark.
///
/// Returns `None` when no reading fits: one kind of separator whose last
/// occurrence is followed by four or more digits, or which appears several
/// times with one or two digits after the last; and a token with no
/// separator at all.
fn separator_roles(token: &str) -> Option<SeparatorRoles> {
    let lone = |mark: char, other: char| {
        let (_, tail) = token.rsplit_once(mark)?;
        let only_one = token.matches(mark).count() == 1;

        match tail.len() {
            1..=MAX_CENT_DIGITS if only_one => Some(SeparatorRoles {
                decimal: Some(mark),
                grouping: other,
            }),
            THOUSANDS_GROUP_DIGITS => Some(SeparatorRoles {
                decimal: None,
                grouping: mark,
            }),
            _ => None,
        }
    };

    match (token.rfind(','), token.rfind('.')) {
        (Some(comma), Some(dot)) if comma > dot => Some(SeparatorRoles {
            decimal: Some(','),
            grouping: '.',
        }),
        (Some(_), Some(_)) => Some(SeparatorRoles {
            decimal: Some('.'),
            grouping: ',',
        }),
        (Some(_), None) => lone(',', '.'),
        (None, Some(_)) => lone('.', ','),
        (None, None) => None,
    }
}

/// Reads a token that holds `,` or `.` as minor units, per the module table.
fn separated_to_minor(token: &str) -> Option<i64> {
    let roles = separator_roles(token)?;

    let (whole, cents) = match roles.decimal {
        Some(mark) => {
            let (whole, cents) = token.rsplit_once(mark)?;
            (whole, Some(cents))
        }
        None => (token, None),
    };

    let whole = if whole.contains(roles.grouping) {
        join_thousands_groups(whole, roles.grouping)?
    } else {
        whole.to_owned()
    };
    decimal_to_minor(&whole, cents)
}

/// Joins `1.234.567` into `1234567` when the groups are well formed: one to
/// three leading digits that do not start with zero, then groups of three.
///
/// The zero rule keeps a three-decimal fraction such as a unit price of
/// `0,085` from being read as thousands.
fn join_thousands_groups(whole: &str, grouping: char) -> Option<String> {
    let mut groups = whole.split(grouping);

    let first = groups.next()?;
    if !(1..=THOUSANDS_GROUP_DIGITS).contains(&first.len()) || first.starts_with('0') {
        return None;
    }

    let mut joined = first.to_owned();
    for group in groups {
        if group.len() != THOUSANDS_GROUP_DIGITS {
            return None;
        }
        joined.push_str(group);
    }
    Some(joined)
}

/// `whole` euros and an optional fraction of one or two digits, as minor
/// units. A one-digit fraction is tenths: `45,9` is 45,90.
///
/// `whole` has no separators left. `None` when it is empty, longer than
/// [`MAX_WHOLE_PART_DIGITS`] or not all digits, or when the fraction is not
/// one or two digits. Leading zeros in `whole` are accepted: `01,50` is
/// 1,50.
fn decimal_to_minor(whole: &str, cents: Option<&str>) -> Option<i64> {
    let all_digits = |digits: &str| digits.bytes().all(|byte| byte.is_ascii_digit());

    if whole.is_empty() || whole.len() > MAX_WHOLE_PART_DIGITS || !all_digits(whole) {
        return None;
    }

    let cents = match cents {
        Some(cents) if !(1..=MAX_CENT_DIGITS).contains(&cents.len()) || !all_digits(cents) => {
            return None;
        }
        Some(cents) => format!("{cents:0<MAX_CENT_DIGITS$}").parse::<i64>().ok()?,
        None => 0,
    };
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(MINOR_PER_EURO)?
        .checked_add(cents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_money_token_is_read_by_the_separator_table() {
        let cases = [
            ("45,90", Some(4_590)),
            ("45.90", Some(4_590)),
            ("45,9", Some(4_590)),
            ("1.234,56", Some(123_456)),
            ("1,234.56", Some(123_456)),
            ("1.234.567", Some(123_456_700)),
            ("1,234,567", Some(123_456_700)),
            ("1,234", Some(123_400)),
            ("50", Some(5_000)),
            ("99999", Some(9_999_900)),
            // One separator before three digits groups thousands, dot or comma.
            ("1.234", Some(123_400)),
            ("999.999", Some(99_999_900)),
            // Unless the whole part is zero or starts with one: a fraction.
            ("0,085", None),
            ("0.971", None),
            ("01,234", None),
            // Thousands groups hold exactly three digits after the first.
            ("1.2.3", None),
            ("1234.567", None),
            ("1,23,456", None),
            // More than two decimals is not money.
            ("1,2345", None),
            // Dates, years and identifiers are not money.
            ("08", None),
            ("2026", None),
            ("123456", None),
            ("123456789,00", None),
            ("12,345.678", None),
            ("", None),
            (",50", None),
            // A trailing comma or dot is sentence punctuation.
            ("5,", Some(500)),
            ("5.", Some(500)),
            ("45,90.", Some(4_590)),
            ("1.234,56,", Some(123_456)),
            (".", None),
            ("1.2.3,4.5", None),
            ("123456789012345", None),
        ];
        for (token, want) in cases {
            assert_eq!(parse_money_token(token), want, "{token:?}");
        }
    }

    #[test]
    fn a_tax_number_is_not_money() {
        assert_eq!(parse_money_token("000000000"), None);
        assert_eq!(parse_money_token("900000000000001"), None);
        assert_eq!(parse_money_token("1860,00"), Some(186_000));
    }
}

#[cfg(test)]
mod amounts_and_dates {
    use crate::documents::analyze::DocumentSuggestion;
    use crate::documents::analyze::parse_invoice_text;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn sentence_punctuation_after_an_amount_is_not_part_of_it() {
        assert_eq!(read("TOTAL 45,90.").amount_minor, Some(4_590));
        assert_eq!(read("Amount due: 45,90.").amount_minor, Some(4_590));
        assert_eq!(
            read("Amount due: 45.90, thank you").amount_minor,
            Some(4_590)
        );
        assert_eq!(read("Amount due: 45.").amount_minor, Some(4_500));
        assert_eq!(read("Amount due: 45,").amount_minor, Some(4_500));
        // A number after the full stop belongs to the next sentence.
        assert_eq!(read("TOTAL 45,90. 3 items").amount_minor, Some(4_590));
        assert_eq!(
            read("Amount due: 1.234,56. 2 pages").amount_minor,
            Some(123_456)
        );
        // A spaced decimal after a thousands group is still one amount.
        assert_eq!(read("Amount due: 1.234, 56").amount_minor, Some(123_456));
        assert_eq!(read("Amount due: 72, 53").amount_minor, Some(7_253));
    }

    #[test]
    fn one_separator_before_three_digits_groups_thousands_for_comma_and_dot() {
        assert_eq!(read("Amount due 1,234").amount_minor, Some(123_400));
        assert_eq!(read("Amount due 1.234").amount_minor, Some(123_400));
        assert_eq!(read("Amount due 12.345 €").amount_minor, Some(1_234_500));
        assert_eq!(read("Amount due 2,500").amount_minor, Some(250_000));
    }

    #[test]
    fn a_three_decimal_fraction_is_not_a_thousands_amount() {
        assert_eq!(read("Amount due 0,085").amount_minor, None);
        assert_eq!(read("Amount due 0.971").amount_minor, None);
        assert_eq!(read("Amount due 01,234").amount_minor, None);
        assert_eq!(
            read("Unit price 0,085\nAmount due 12,40").amount_minor,
            Some(1_240)
        );
    }

    #[test]
    fn thousands_groups_must_be_well_formed() {
        assert_eq!(read("Amount due 1.2.3").amount_minor, None);
        assert_eq!(read("Amount due 1234.567,00").amount_minor, None);
        assert_eq!(
            read("Amount due 1.234.567,00").amount_minor,
            Some(123_456_700)
        );
    }
}

#[cfg(test)]
mod documented_tradeoffs {
    use super::*;

    #[test]
    fn leading_zeros_in_a_whole_part_are_accepted_beside_a_decimal_mark() {
        assert_eq!(parse_money_token("01,50"), Some(150));
        assert_eq!(parse_money_token("007,00"), Some(700));
        assert_eq!(parse_money_token("08"), None);
        assert_eq!(parse_money_token("01.234"), None);
    }

    #[test]
    fn a_whole_amount_that_looks_like_a_year_needs_decimals() {
        assert_eq!(parse_money_token("2026"), None);
        assert_eq!(parse_money_token("1900"), None);
        assert_eq!(parse_money_token("2100"), None);
        assert_eq!(parse_money_token("2026,00"), Some(202_600));
        assert_eq!(parse_money_token("1899"), Some(189_900));
        assert_eq!(parse_money_token("2101"), Some(210_100));
    }
}
