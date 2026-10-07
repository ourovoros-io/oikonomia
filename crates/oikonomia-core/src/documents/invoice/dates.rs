//! Dates and clock times in document text: finding the date to suggest, and
//! blanking date and time tokens so their digits are never read as money.
//!
//! Two questions are kept apart. Whether a token is *written like* a date
//! ([`DateShape`]) decides that its digits are not an amount, and is all the
//! masks ask ([`mask_date_tokens`], [`mask_time_tokens`]). Whether it *is* a
//! date, a day the calendar has, is decided only when a date is returned
//! ([`first_date_on_line`]). So `31/02/2026` is blanked like a date and is
//! never suggested as one.
//!
//! Dates are read day first. [`find_best_date`] chooses among the dates of
//! a document.

use std::ops::RangeInclusive;

use time::Date;

use crate::documents::keyword::Keyword::{Prefix, Word};
use crate::documents::keyword::{Keyword, contains_any, folded};

/// The first date written on `line`.
///
/// A date is a whitespace-separated word that, with everything but digits
/// and `/ . -` trimmed from its ends, is a day-month-year date or an ISO
/// date. A day the calendar does not have is not a date.
pub(super) fn first_date_on_line(line: &str) -> Option<Date> {
    for word in line.split_whitespace() {
        let token =
            word.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '.' && c != '-');
        if let Some(date) = parse_eu_date(token).or_else(|| parse_iso_date(token)) {
            return Some(date);
        }
    }
    None
}

/// Labels of the value date of a bank transaction.
pub(super) const VALUE_DATE_LABELS: &[Keyword] = &[Word("ημερομηνια αξιας"), Word("value date")];

/// Whether a folded line is the value-date line of a bank transaction.
pub(super) fn is_value_date_line(folded_line: &str) -> bool {
    contains_any(folded_line, VALUE_DATE_LABELS)
}

/// Whether [`first_date_on_line`] finds a date on `line`.
pub(super) fn line_has_date(line: &str) -> bool {
    first_date_on_line(line).is_some()
}

/// Characters in an ISO date, `YYYY-MM-DD`.
const ISO_DATE_CHARS: usize = 10;

/// Blanks out day-first and ISO date tokens, so the numbers of a date are
/// not read as euros.
///
/// Extraction often puts `13/08/2026 72,53 €` on one line, where `13`, `08`
/// and `2026` would each be an amount. Bank receipts also print unpadded
/// dates such as `27/8/2026`.
///
/// A token only has to be written like a date ([`DateShape`]): `31/02/2026`
/// is not a date, and its digits are still not money.
pub(super) fn mask_date_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut masked = String::with_capacity(line.len());
    let mut index = 0;

    while let Some(&current) = chars.get(index) {
        if is_iso_date_at(&chars, index) {
            masked.push_str(&" ".repeat(ISO_DATE_CHARS));
            index += ISO_DATE_CHARS;
        } else if let Some(len) = eu_date_len_at(&chars, index) {
            masked.push_str(&" ".repeat(len));
            index += len;
        } else {
            masked.push(current);
            index += 1;
        }
    }
    masked
}

/// Whether the ten characters from `start` are written `dddd-dd-dd`.
///
/// Only the shape is tested: `2026-99-99` qualifies, and is blanked like a
/// date.
fn is_iso_date_at(chars: &[char], start: usize) -> bool {
    let Some([y1, y2, y3, y4, '-', m1, m2, '-', d1, d2]) = chars.get(start..start + ISO_DATE_CHARS)
    else {
        return false;
    };

    [y1, y2, y3, y4, m1, m2, d1, d2]
        .into_iter()
        .all(char::is_ascii_digit)
}

/// Length of the day-month-year token that starts at `start`, if one does.
///
/// The token is one or two digits, a separator (`/`, `.` or `-`), one or two
/// digits, the same separator, and two or four digits, with no digit
/// directly before or after it. Its numbers must fit a [`DateShape`]. So
/// `27/8/2026`, `13.08.26` and `31/02/2026` are tokens, and `45/90/2026` is
/// not.
fn eu_date_len_at(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && chars[start - 1].is_ascii_digit() {
        return None;
    }
    if start >= chars.len() || !chars[start].is_ascii_digit() {
        return None;
    }

    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() && end - start < 2 {
        end += 1;
    }
    if end == start || end >= chars.len() {
        return None;
    }
    let separator = chars[end];
    if separator != '/' && separator != '.' && separator != '-' {
        return None;
    }
    end += 1;

    let month_start = end;
    while end < chars.len() && chars[end].is_ascii_digit() && end - month_start < 2 {
        end += 1;
    }
    if end == month_start || end >= chars.len() || chars[end] != separator {
        return None;
    }
    end += 1;

    let year_start = end;
    while end < chars.len() && chars[end].is_ascii_digit() && end - year_start < 4 {
        end += 1;
    }
    let year_len = end - year_start;
    if year_len != 2 && year_len != 4 {
        return None;
    }
    if end < chars.len() && chars[end].is_ascii_digit() {
        return None;
    }

    let token: String = chars[start..end].iter().collect();
    eu_date_shape(&token).map(|_| end - start)
}

/// Blanks out clock times (`h:mm`, `hh:mm`, `hh:mm:ss`), so `7:00` is not
/// read as 7,00.
pub(super) fn mask_time_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut masked = String::with_capacity(line.len());
    let mut index = 0;

    while let Some(&current) = chars.get(index) {
        if let Some(len) = time_len_at(&chars, index) {
            masked.push_str(&" ".repeat(len));
            index += len;
        } else {
            masked.push(current);
            index += 1;
        }
    }
    masked
}

/// Length of the `H:MM`, `HH:MM` or `HH:MM:SS` time starting at `start`, if
/// one starts there and is not part of a longer run of digits.
fn time_len_at(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && digit_at(chars, start - 1).is_some() {
        return None;
    }

    let colon = hour_colon_at(chars, start)?;
    let minutes = two_digits_at(chars, colon + 1)?;
    if minutes > 59 {
        return None;
    }

    let mut end = colon + 3;
    let has_seconds = chars.get(end) == Some(&':')
        && two_digits_at(chars, end + 1).is_some_and(|seconds| seconds <= 59);
    if has_seconds {
        end += 3;
    }

    if digit_at(chars, end).is_some() {
        return None;
    }
    Some(end - start)
}

/// Index of the colon that ends the hour starting at `start`: a two-digit
/// hour up to 23, or else a single digit.
fn hour_colon_at(chars: &[char], start: usize) -> Option<usize> {
    let first = digit_at(chars, start)?;

    let two_digit_hour = digit_at(chars, start + 1).is_some_and(|second| first * 10 + second <= 23);
    if two_digit_hour && chars.get(start + 2) == Some(&':') {
        return Some(start + 2);
    }

    (chars.get(start + 1) == Some(&':')).then_some(start + 1)
}

/// The decimal digit at `index`, or `None` past the end or on another
/// character.
fn digit_at(chars: &[char], index: usize) -> Option<u32> {
    chars.get(index)?.to_digit(10)
}

/// The two-digit number at `index`, or `None` when either character is not a
/// digit.
fn two_digits_at(chars: &[char], index: usize) -> Option<u32> {
    Some(digit_at(chars, index)? * 10 + digit_at(chars, index + 1)?)
}

/// The date to suggest for a document. In order of preference:
///
/// 1. the first date on a line that also has a `€`: on a utility payment
///    slip that is the due date printed beside the amount to pay;
/// 2. the first date on a line with a date label ([`DATE_LABELS`]);
/// 3. the first date anywhere.
///
/// `None` when the text has no date.
pub(super) fn find_best_date(text: &str) -> Option<Date> {
    for line in text.lines() {
        if line.contains('€')
            && let Some(date) = first_date_on_line(line)
        {
            return Some(date);
        }
    }

    for line in text.lines() {
        if contains_any(&folded(line), DATE_LABELS)
            && let Some(date) = first_date_on_line(line)
        {
            return Some(date);
        }
    }

    text.lines().find_map(first_date_on_line)
}

/// Labels of a date line: "date", "issued", "expires", "due". Stems, so they
/// match every inflection.
pub(super) const DATE_LABELS: &[Keyword] = &[
    Prefix("ημερομην"),
    Word("date"),
    Prefix("εκδοσ"),
    Prefix("ληξ"),
    Word("due"),
];

/// The years a document date may have.
///
/// A day-first token with a year outside this range is not a date, so it is
/// not blanked either and its numbers can be read as money. An ISO-shaped
/// token is blanked by its shape whatever its year, and is then not a date.
///
/// The reason for these two years is not recorded;
/// `a_date_is_read_only_within_the_document_years` pins both.
pub(super) const DOCUMENT_YEARS: RangeInclusive<i32> = 1990..=2100;

/// What a two-digit year is counted from: `26` is 2026.
const TWO_DIGIT_YEAR_CENTURY: i32 = 2000;

/// The numbers of a token written like a date: a year in 1990..=2100, a
/// month in 1..=12 and a day in 1..=31.
///
/// The day need not exist in that month. The shape alone decides that the
/// digits are not money; only [`DateShape::to_date`] decides that they are a
/// date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DateShape {
    /// The year, in [`DOCUMENT_YEARS`].
    year: i32,
    /// The month, 1 to 12.
    month: u8,
    /// The day, 1 to 31, whatever the month.
    day: u8,
}

impl DateShape {
    /// The shape, or `None` when a number is out of its range.
    fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let in_range =
            DOCUMENT_YEARS.contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day);

        in_range.then_some(Self { year, month, day })
    }

    /// The date, or `None` when the calendar has no such day (31 February,
    /// 29 February outside a leap year).
    fn to_date(self) -> Option<Date> {
        let month = time::Month::try_from(self.month).ok()?;

        Date::from_calendar_date(self.year, month, self.day).ok()
    }
}

/// The numbers of a `YYYY-MM-DD` token, or `None` when it is not three
/// numbers joined by `-` or they do not fit a [`DateShape`].
fn iso_date_shape(token: &str) -> Option<DateShape> {
    let mut parts = token.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    DateShape::new(year, month, day)
}

/// The numbers of a day-month-year token, or `None` when it is not three
/// numbers joined by one of `/`, `.` and `-`, or they do not fit a
/// [`DateShape`].
///
/// The order is always day, month, year; a month-first date is misread or
/// refused. A year under 100 is counted from [`TWO_DIGIT_YEAR_CENTURY`].
fn eu_date_shape(token: &str) -> Option<DateShape> {
    let separator = ['/', '.', '-']
        .into_iter()
        .find(|separator| token.contains(*separator))?;

    let mut parts = token.split(separator);
    let day = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    let year = if year < 100 {
        year + TWO_DIGIT_YEAR_CENTURY
    } else {
        year
    };
    DateShape::new(year, month, day)
}

/// A `YYYY-MM-DD` token as a date, or `None` when it is not a day the
/// calendar has.
fn parse_iso_date(token: &str) -> Option<Date> {
    iso_date_shape(token)?.to_date()
}

/// A day-month-year token as a date, or `None` when it is not a day the
/// calendar has.
fn parse_eu_date(token: &str) -> Option<Date> {
    eu_date_shape(token)?.to_date()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time_len(text: &str, at: usize) -> Option<usize> {
        let chars: Vec<char> = text.chars().collect();
        time_len_at(&chars, at)
    }

    #[test]
    fn a_date_is_read_only_within_the_document_years() {
        use time::macros::date;

        assert_eq!(first_date_on_line("31/12/1989"), None);
        assert_eq!(
            first_date_on_line("01/01/1990"),
            Some(date!(1990 - 01 - 01))
        );
        assert_eq!(
            first_date_on_line("31/12/2100"),
            Some(date!(2100 - 12 - 31))
        );
        assert_eq!(first_date_on_line("01/01/2101"), None);

        assert_eq!(first_date_on_line("1989-12-31"), None);
        assert_eq!(
            first_date_on_line("1990-01-01"),
            Some(date!(1990 - 01 - 01))
        );
        assert_eq!(
            first_date_on_line("2100-12-31"),
            Some(date!(2100 - 12 - 31))
        );
        assert_eq!(first_date_on_line("2101-01-01"), None);

        // A day-first token outside the years is not blanked either, so its
        // numbers stay on the line; an ISO-shaped one is blanked by its shape.
        assert_eq!(mask_date_tokens("31/12/1989"), "31/12/1989");
        assert_eq!(mask_date_tokens("01/01/1990"), " ".repeat(10));
        assert_eq!(mask_date_tokens("1989-12-31"), " ".repeat(10));
    }

    #[test]
    fn a_clock_time_is_measured_only_where_one_starts() {
        let cases = [
            ("9:05", 0, Some(4)),
            ("09:05", 0, Some(5)),
            ("23:59", 0, Some(5)),
            ("23:59:59", 0, Some(8)),
            ("at 7:30 pm", 3, Some(4)),
            // Seconds out of range: the time ends after the minutes.
            ("12:30:61", 0, Some(5)),
            // Not a valid hour or minute.
            ("24:00", 0, None),
            ("12:60", 0, None),
            ("12:5", 0, None),
            ("12:", 0, None),
            ("12", 0, None),
            ("x", 0, None),
            // A digit before or after makes it part of a longer number.
            ("112:30", 1, None),
            ("12:301", 0, None),
            ("12:30:451", 0, None),
            // Past the end.
            ("12:30", 5, None),
        ];
        for (text, at, want) in cases {
            assert_eq!(time_len(text, at), want, "{text:?} at {at}");
        }
    }
}

#[cfg(test)]
mod amounts_and_dates {
    use crate::documents::analyze::DocumentSuggestion;
    use crate::documents::analyze::parse_invoice_text;
    use crate::documents::invoice::money::{money_amounts_on_line, parse_money_token};
    use time::macros::date;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn date_tokens_are_not_money() {
        assert!(money_amounts_on_line("13/08/2026 72,53 €").contains(&7_253));
        assert!(!money_amounts_on_line("13/08/2026 72,53 €").contains(&1_300));
        assert!(!money_amounts_on_line("26/05/2026 30/06/2026").contains(&2_600));
        assert!(!money_amounts_on_line("27/8/2026 310,00").contains(&2_700));
        assert!(!money_amounts_on_line("27/8/2026 310,00").contains(&800));
        assert!(money_amounts_on_line("27/8/2026 310,00").contains(&31_000));
        assert_eq!(parse_money_token("08"), None);
        assert_eq!(parse_money_token("2026"), None);
    }

    #[test]
    fn a_day_the_month_does_not_have_is_not_a_date() {
        for impossible in ["31/02/2026", "29/02/2026", "31.04.2026", "2026-02-31"] {
            let suggestion = read(&format!("Invoice\nDate {impossible}\nTOTAL 45,90"));

            assert_eq!(suggestion.entry_date, None, "{impossible}");
            assert_eq!(suggestion.amount_minor, Some(4_590), "{impossible}");
        }

        assert_eq!(
            read("Date 29/02/2024").entry_date,
            Some(date!(2024 - 02 - 29))
        );
        assert_eq!(
            read("Date 2024-02-29").entry_date,
            Some(date!(2024 - 02 - 29))
        );
    }

    #[test]
    fn the_digits_of_an_impossible_date_are_still_not_money() {
        assert_eq!(read("Amount due 31/02/2026").amount_minor, None);
        assert_eq!(read("Amount due 2026-02-31").amount_minor, None);
    }

    #[test]
    fn clock_tokens_are_not_money() {
        assert!(!money_amounts_on_line("Ημερομηνία Αξίας 28/8/2026 7:00 μ.μ.").contains(&700));
        assert_eq!(money_amounts_on_line("7:00"), [] as [i64; 0]);
        assert_eq!(money_amounts_on_line("19:30"), [] as [i64; 0]);
    }
}

#[cfg(test)]
mod documented_tradeoffs {
    use crate::documents::analyze::DocumentSuggestion;
    use crate::documents::analyze::parse_invoice_text;
    use time::macros::date;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn dates_are_read_day_first() {
        assert_eq!(
            read("Date 03/04/2026").entry_date,
            Some(date!(2026 - 04 - 03))
        );
        // Month first, with a day over 12: not a date.
        assert_eq!(read("Date 04/13/2026").entry_date, None);
    }
}
