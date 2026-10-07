//! Integer amount parsing for bank CSV. Never uses `f64`.
//!
//! A bank writes `1.234,56`, another `1,234.56`, a third `1 234,56 EUR`,
//! `1'234.56` or `(25.00)`. The parser reads all of them into signed minor units by
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
//! | 1    | Whitespace not between digits | `- 25`, `25 EUR`   | An empty cell     |
//! | 2    | Parentheses around the cell   | `(25,00)`          |                   |
//! | 3    | The signs `€ $ £ ¥ ₹ ₺ ₩`     | `€25`, `25 $`      | `₽25`             |
//! | 4    | Three ASCII letters, each end | `EUR 25`, `25 lei` | `25 USD`, `25 kr` |
//! | 5    | One sign                      | `-25`, `+25`, `25-`| `-25-`, `25+`     |
//! | 6    | Step 4 again                  | `-EUR 25`          |                   |
//! | 7    | Nothing: digits and           | `25`, `.5`, `5.`   | `1e3`, `25%`, `.` |
//! |      | separators are left           |                    |                   |
//!
//! Notes on the steps:
//!
//! 1. A run of whitespace between two digits stays, as one space: it is a
//!    grouping separator, read below. All other whitespace goes. A cell that
//!    is empty or only whitespace is a missing amount, not an invalid one.
//!    The typographic apostrophe U+2019 is written as `'` in this step.
//! 2. Parentheses mean negative. They do not cancel a minus: `(-25)` is
//!    negative.
//! 3. The signs are removed wherever they stand.
//! 4. Three ASCII letters at an end of the number are dropped, with one
//!    exception for a statement: three capitals are a currency code, written
//!    the way ISO 4217 writes one, and a code that is not the book's makes
//!    the cell invalid. `25 USD` in a book in euros is rejected, because its
//!    digits read as euros would be a wrong amount that looks right. Three
//!    letters with a lowercase one among them are a word and are dropped
//!    whatever the book, so `25,00 lei` reads in a book in `RON` and in any
//!    other.
//!
//!    The capitals are all the parser has to go on, since the letters are
//!    not checked against a list of currencies, and that sets two limits. A
//!    code written in lower case is taken for a word: `25 usd` in a book in
//!    euros is read as 25 euros. A capitalised three-letter word that is no
//!    currency is taken for a code: `25 PCS`, and `25 LEI` in a book in
//!    `RON`, are rejected.
//!
//!    [`parse_signed_minor`], which is given no book, drops any three
//!    letters. Letters of another count, non-ASCII letters, and letters in
//!    the middle of the number are left in place and fail step 7.
//! 5. The sign is a leading `-`, U+2212 or `+`, or else a trailing `-` or
//!    U+2212. A second sign (`-25-`, `+25-`, `--25`) is left in place and
//!    fails step 7, as does a trailing `+`.
//! 6. The second pass lets a code stand on either side of the sign
//!    (`25 EUR-`).
//! 7. At least one digit is required. The separators are `.`, `,`, `'` and
//!    the space step 1 kept.
//!
//! Then the separators are read. There are two kinds. `.` and `,` can each
//! be the decimal mark or a thousands separator. The apostrophe (Swiss
//! exports: `1'234.56`) and the space (French and Nordic exports, which
//! write a space, a no-break space U+00A0 or a narrow no-break space U+202F:
//! `1 234,56`) are thousands separators and nothing else.
//!
//! A cell without an apostrophe or a space, with `exponent` 2:
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
//! A cell with an apostrophe or a space, with `exponent` 2:
//!
//! | Shape                                 | Reading          | Example                |
//! |---------------------------------------|------------------|------------------------|
//! | Groups only                           | Whole major units| `1'234` → 123400       |
//! | Groups, a `.` or `,`, up to           | Decimal mark     | `1'234.56` → 123456    |
//! | `exponent` digits                     |                  | `1 234,5` → 123450     |
//! | Both an apostrophe and a space        | Rejected         | `1 234'567`            |
//! | A second `.` or `,`, or a group after | Rejected         | `1'234.567,89`,        |
//! | the mark                              |                  | `1.234'567`            |
//! | More digits after the mark than       | Rejected         | `1'234.567`            |
//! | `exponent`                            |                  |                        |
//!
//! Here nothing is ambiguous, so the three-digit tail rule does not apply:
//! the one `.` or `,` is the decimal mark, and `1'234.567` is 1234.567 for a
//! currency with three decimals and rejected for any other.
//!
//! A thousands grouping is strict, whatever its separator: a first group of
//! one to three digits that does not start with `0`, then groups of exactly
//! three. `1,2,3`, `12,34.56`, `012,345`, `12'34` and `1 2 3,45` are
//! rejected.
//!
//! With `exponent` 3 a three-digit tail is always the fraction, so `12,345`
//! is 12.345 and never twelve thousand. With `exponent` 0 no digit may
//! follow a decimal mark, so `1234.56` is rejected. With `exponent` 4 (`CLF`,
//! `UYW`) a three-digit tail is a grouping like anywhere else: `1.234` is
//! one thousand two hundred and thirty-four units, `0.125` is rejected, and
//! a fraction of three digits has to be written with its fourth, `0.1250`.
//!
//! There is no exponent notation and no percent sign. Zero is a valid
//! result here; the row parser is what refuses a zero amount.

use crate::csv::CsvError;
use crate::domain::CurrencyCode;

/// Returns the number of decimal digits (the minor-unit exponent) of a
/// currency code, 2 for a code the table does not list.
///
/// This is the one exponent table of the crate: CSV import and the document
/// analyzer both read it. It has to agree with the webview, which turns
/// `amount_minor` into a displayed amount (and typed amounts back into minor
/// units) with the digits `Intl.NumberFormat` reports for the currency. Those
/// digits come from CLDR, not ISO 4217, and the two differ: CLDR gives `IQD`,
/// `IRR`, `RSD` and a dozen more no decimals where ISO gives two or three. If
/// this table followed ISO, an imported IQD amount would show 1000 times too
/// large.
///
/// So the table is CLDR's, version 46 as shipped in ICU 76. It lists every
/// code whose digits there are not 2: 43 codes with none, 6 with three
/// (`BHD`, `JOD`, `KWD`, `LYD`, `OMR`, `TND`) and 2 with four (`CLF`, `UYW`).
/// The test `the_non_two_digit_codes_are_exactly_cldrs` pins the three
/// lists. Withdrawn codes CLDR still knows are kept so an old export lines
/// up with what the webview shows.
///
/// The agreement holds for a webview on that CLDR version, and CLDR changes
/// these digits between versions. Node 22.22 (ICU 78, CLDR 48) reports no
/// decimals for `COP`, `HUF`, `IDR` and `PKR`, which this table reads with
/// two, and two decimals for `RSD`, which this table reads with none.
#[must_use]
pub fn currency_minor_exponent(code: CurrencyCode) -> u8 {
    match code.as_str() {
        "ADP" | "AFN" | "ALL" | "BIF" | "BYR" | "CLP" | "DJF" | "ESP" | "GNF" | "IQD" | "IRR"
        | "ISK" | "ITL" | "JPY" | "KMF" | "KPW" | "KRW" | "LAK" | "LBP" | "LUF" | "MGA" | "MGF"
        | "MMK" | "MRO" | "PYG" | "RSD" | "RWF" | "SLL" | "SOS" | "STD" | "SYP" | "TMM" | "TRL"
        | "UGX" | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" | "YER" | "ZMK" | "ZWD" => 0,
        "BHD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        "CLF" | "UYW" => 4,
        _ => 2,
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
/// An apostrophe, or whitespace between two digits, is a thousands separator
/// and never a decimal mark: `1'234.56` and `1 234,56` have one reading. The
/// same strict grouping applies, and the one `.` or `,` such a cell may hold
/// is its decimal mark.
///
/// Supported examples with `exponent == 2`:
/// - `1.234,56` / `1234,56` / `1234.56` / `1,234.56`
/// - `1'234.56` / `1 234,56` → `123456`
/// - `1.234` / `1,234` / `1'234` → `123400`
/// - `-25` → `-2500`
///
/// Whitespace anywhere else and the currency signs `€$£¥₹₺₩` are ignored. So
/// are three ASCII letters directly before or after the number: this
/// function is given no book to compare them with and does not check them
/// against the ISO 4217 list, so `1.00 abc` parses like `1.00 EUR`. A
/// statement is read through `parse_book_amount`, which accepts only the
/// code of the book's currency.
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
    parse_amount(raw, exponent, None)
}

/// Parses an amount cell of a statement for a book in `currency`, into
/// signed minor units of that currency.
///
/// This is [`parse_signed_minor`] with the decimals of `currency`
/// ([`currency_minor_exponent`]) and one more rule: three capital letters in
/// the cell are a currency code and have to be `currency`. A cell marked
/// with another code is not an amount of this book, and reading its digits
/// as one would post `25 USD` as 25 euros. Three letters that are not all
/// capitals are a word, such as `lei`, and are dropped; note 4 of the module
/// doc has the two limits of telling a code by its capitals.
///
/// # Errors
///
/// Those of [`parse_signed_minor`], and [`CsvError::InvalidAmount`],
/// carrying the cell as written, for a cell with three capital letters that
/// are not `currency`.
pub(crate) fn parse_book_amount(raw: &str, currency: CurrencyCode) -> Result<i64, CsvError> {
    parse_amount(raw, currency_minor_exponent(currency), Some(currency))
}

/// Parses an amount cell into signed minor units of a currency with
/// `exponent` decimals.
///
/// With a `book` currency, a three-letter code in the cell has to be that
/// currency; without one any three letters pass.
///
/// # Errors
///
/// Those of [`parse_signed_minor`] and of [`parse_book_amount`].
fn parse_amount(raw: &str, exponent: u8, book: Option<CurrencyCode>) -> Result<i64, CsvError> {
    let (negative, digits) = prepare_amount(raw, book)?;
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
/// [`CsvError::InvalidAmount`], carrying the cell as written, when a
/// three-letter code is not the currency of `book`, or when anything other
/// than digits and separators is left over.
fn prepare_amount(raw: &str, book: Option<CurrencyCode>) -> Result<(bool, String), CsvError> {
    let invalid = || CsvError::InvalidAmount(raw.to_owned());

    let compact = compact_amount(raw);
    if compact.is_empty() {
        return Err(CsvError::MissingAmount);
    }

    let (parenthesized, unwrapped) = strip_parentheses(&compact);
    let without_symbols = strip_currency_symbols(unwrapped);
    // A code is dropped on both sides of the sign, so `EUR -12`, `-EUR 12`,
    // `12- EUR` and `12 EUR-` all leave the bare digits.
    let coded = strip_letter_code(&without_symbols, book).ok_or_else(invalid)?;
    let (signed_negative, unsigned) = strip_sign(coded);
    let body = strip_letter_code(unsigned, book).ok_or_else(invalid)?;

    let is_digit_or_separator = |character: char| {
        character.is_ascii_digit() || matches!(character, '.' | ',' | SPACE | APOSTROPHE)
    };
    if body.is_empty() || !body.chars().all(is_digit_or_separator) {
        return Err(invalid());
    }
    Ok((parenthesized || signed_negative, body.to_owned()))
}

/// The grouping separator a run of whitespace between two digits is
/// reduced to.
const SPACE: char = ' ';

/// The grouping separator of Swiss exports. U+2019, the typographic
/// apostrophe some of them write instead, is reduced to it.
const APOSTROPHE: char = '\'';

/// Runs step 1 of the grammar: returns `raw` without its whitespace, except
/// that a run of whitespace between two digits is kept as one [`SPACE`].
///
/// Whitespace between digits is how French and Nordic exports group
/// thousands, with a space, a no-break space (U+00A0) or a narrow no-break
/// space (U+202F), so it is kept for the grouping rule to check. Anywhere
/// else it separates the number from a sign or a currency and means nothing.
/// The typographic apostrophe is written as [`APOSTROPHE`] on the way.
fn compact_amount(raw: &str) -> String {
    let mut compact = String::with_capacity(raw.len());
    let mut gap_after_digit = false;

    for character in raw.chars() {
        if character.is_whitespace() {
            gap_after_digit = compact.ends_with(|kept: char| kept.is_ascii_digit());
            continue;
        }
        if gap_after_digit && character.is_ascii_digit() {
            compact.push(SPACE);
        }
        gap_after_digit = false;
        compact.push(if character == '\u{2019}' {
            APOSTROPHE
        } else {
            character
        });
    }
    compact
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

/// Drops three letters from the front, and three from the back of what is
/// left, or returns `None` when letters it would drop are the code of
/// another currency than `book`'s.
///
/// Three letters are taken for a currency code only when all three are
/// capitals, the way ISO 4217 codes are written; a [`CurrencyCode`] holds
/// its letters in capitals, so such a code is the book's exactly when the
/// two texts are equal. Letters with a lowercase one among them are a word
/// (`lei`) and are dropped whatever the book. Without a `book` every three
/// letters are dropped.
///
/// A cell that is nothing but three letters is returned as it is. It has no
/// digits, so the caller rejects it as an invalid amount either way.
fn strip_letter_code(text: &str, book: Option<CurrencyCode>) -> Option<&str> {
    let is_another_currency = |letters: &str| {
        let written_as_a_code = letters.bytes().all(|byte| byte.is_ascii_uppercase());
        book.is_some_and(|book| written_as_a_code && letters != book.as_str())
    };

    let rest = match split_leading_letter_code(text) {
        Some((_, "")) => return Some(text),
        Some((letters, _)) if is_another_currency(letters) => return None,
        Some((_, rest)) => rest,
        None => text,
    };
    match split_trailing_letter_code(rest) {
        Some((head, letters)) => (!is_another_currency(letters)).then_some(head),
        None => Some(rest),
    }
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

/// Splits `text` into what stands before a three-letter code at its end and
/// that code, or returns `None` when it does not end with one.
///
/// `split_at_checked` is `None` when three bytes from the end falls inside a
/// multi-byte character, which also means the tail is not three letters.
fn split_trailing_letter_code(text: &str) -> Option<(&str, &str)> {
    let code_start = text.len().checked_sub(3)?;
    let (head, code) = text.split_at_checked(code_start)?;
    is_letter_code(code).then_some((head, code))
}

/// Whether `text` is exactly three ASCII letters, in either case.
///
/// The shape of an ISO 4217 code, and all [`CurrencyCode`] asks of one. The
/// letters are not looked up in a list of currencies; the caller compares
/// them with the currency of the book.
fn is_letter_code(text: &str) -> bool {
    text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// Splits a body of digits and separators into integer and fraction digits,
/// or returns `None` when the separators do not form an amount.
///
/// A space or an apostrophe is only ever a thousands separator, so a body
/// that has one is read by [`split_grouped_by`]; a body with both is not an
/// amount. Otherwise the separators are `.` and `,`, either of which can be
/// the decimal mark: the last one is, unless it is followed by exactly three
/// digits in a currency without three decimals; then the whole body has to
/// be a thousands grouping, because `0.125` or `1,234.567` read as thousands
/// would silently multiply the amount.
fn split_decimal(body: &str, exponent: u8) -> Option<(String, &str)> {
    match (body.contains(SPACE), body.contains(APOSTROPHE)) {
        (true, true) => return None,
        (true, false) => return split_grouped_by(body, " ", exponent),
        (false, true) => return split_grouped_by(body, "'", exponent),
        (false, false) => {}
    }

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

/// Splits a body whose whole part is grouped by `separator`, a space or an
/// apostrophe, into integer and fraction digits.
///
/// Such a separator cannot be a decimal mark, so nothing here is ambiguous:
/// everything before the first `.` or `,` has to be one grouping on
/// `separator`, and what follows that mark is the fraction, of at most
/// `exponent` digits. A three-digit tail is therefore never read as a group:
/// `1'234.567` is rejected for a currency with two decimals.
fn split_grouped_by<'body>(
    body: &'body str,
    separator: &str,
    exponent: u8,
) -> Option<(String, &'body str)> {
    let (whole, fraction) = match body.find(['.', ',']) {
        Some(decimal_mark_at) => {
            let (whole, marked_fraction) = body.split_at(decimal_mark_at);
            (whole, marked_fraction.split_at(1).1)
        }
        None => (body, ""),
    };

    let fraction_is_digits = fraction.bytes().all(|byte| byte.is_ascii_digit());
    if !fraction_is_digits || fraction.len() > usize::from(exponent) {
        return None;
    }
    Some((grouped_digits(whole, separator)?, fraction))
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

    /// The currency with code `code`.
    fn currency(code: &str) -> CurrencyCode {
        code.parse().expect("a currency code")
    }

    #[test]
    fn a_cell_marked_with_the_books_currency_is_read() {
        let eur = currency("EUR");

        for (raw, minor) in [
            ("25 EUR", 2_500),
            ("eur 25", 2_500),
            ("EUR -12.00", -1_200),
            ("-EUR 12", -1_200),
            ("12- EUR", -1_200),
            ("12 EUR-", -1_200),
            ("(1.234,56 Eur)", -123_456),
            ("EUR 25 EUR", 2_500),
            ("€25", 2_500),
            ("25", 2_500),
        ] {
            assert_eq!(parse_book_amount(raw, eur), Ok(minor), "{raw}");
        }
    }

    #[test]
    fn a_cell_marked_with_another_currency_is_not_an_amount_of_the_book() {
        let eur = currency("EUR");

        for raw in [
            "25 USD",
            "USD 25",
            "USD -12.00",
            "-USD 12",
            "12- USD",
            "12 USD-",
            "(25,00 USD)",
            "EUR 25 USD",
            "USD 25 EUR",
            "EUR -USD 25",
            "lei 25 USD",
        ] {
            assert_eq!(
                parse_book_amount(raw, eur),
                Err(CsvError::InvalidAmount(raw.to_owned())),
                "{raw}"
            );
        }
        assert_eq!(parse_book_amount("25 USD", currency("usd")), Ok(2_500));
        assert_eq!(parse_book_amount("25 EUR", eur), Ok(2_500));
    }

    #[test]
    fn three_letters_that_are_not_all_capitals_are_dropped_as_a_word() {
        // A currency written as a word, such as the Romanian `lei`, is not a
        // code: it is dropped and the amount is read in the book's currency.
        for book in [currency("RON"), currency("EUR")] {
            for raw in ["25,00 lei", "lei 25,00", "25,00 Lei", "25,00 leI"] {
                assert_eq!(parse_book_amount(raw, book), Ok(2_500), "{raw} in {book}");
            }
        }
    }

    #[test]
    fn the_limits_of_telling_a_code_by_its_capitals() {
        let eur = currency("EUR");

        // A code in lower case is taken for a word and read as euros.
        assert_eq!(parse_book_amount("25 usd", eur), Ok(2_500));
        assert_eq!(parse_book_amount("25 Usd", eur), Ok(2_500));
        // A capitalised word that is no currency is refused like a code.
        assert_eq!(
            parse_book_amount("25 LEI", eur),
            Err(CsvError::InvalidAmount("25 LEI".to_owned()))
        );
        assert_eq!(
            parse_book_amount("25 PCS", eur),
            Err(CsvError::InvalidAmount("25 PCS".to_owned()))
        );
    }

    #[test]
    fn a_book_amount_has_the_decimals_of_the_books_currency() {
        assert_eq!(parse_book_amount("1,234 JPY", currency("JPY")), Ok(1_234));
        assert_eq!(parse_book_amount("0.125 KWD", currency("KWD")), Ok(125));
        assert_eq!(parse_book_amount("0.1250", currency("CLF")), Ok(1_250));
        assert_eq!(
            parse_book_amount("", currency("EUR")),
            Err(CsvError::MissingAmount)
        );
    }

    #[test]
    fn without_a_book_any_three_letters_pass_as_a_code() {
        assert_eq!(parse_signed_minor("25 USD", 2), Ok(2_500));
        assert_eq!(parse_signed_minor("abc 25", 2), Ok(2_500));
    }

    #[test]
    fn the_examples_of_the_grammar_table_hold() {
        assert_eq!(parse_eur_minor("25 $"), 2_500);
        assert_eq!(parse_eur_minor("-EUR 25"), -2_500);
        assert_eq!(parse_eur_minor("(-25)"), -2_500);
        assert_eq!(parse_eur_minor("1234.5"), 123_450);
        assert_eq!(parse_eur_minor("1,234,567"), 123_456_700);

        assert_invalid_amount("1e3", 2);
        assert_invalid_amount("25%", 2);
        assert_invalid_amount("1,2,3", 2);
    }

    #[test]
    fn an_apostrophe_groups_thousands() {
        assert_eq!(parse_eur_minor("1'234.56"), 123_456);
        assert_eq!(parse_eur_minor("1'234,56"), 123_456);
        assert_eq!(parse_eur_minor("1\u{2019}234.56"), 123_456);
        assert_eq!(parse_eur_minor("1'234'567.89"), 123_456_789);
        assert_eq!(parse_eur_minor("1'234"), 123_400);
        assert_eq!(parse_eur_minor("1'234.5"), 123_450);
        assert_eq!(parse_eur_minor("1'234."), 123_400);
        assert_eq!(parse_eur_minor("-1'234.50"), -123_450);
        assert_eq!(parse_eur_minor("CHF 1'234.50"), 123_450);
        assert_eq!(parse_eur_minor("(1'234.50)"), -123_450);
    }

    #[test]
    fn a_space_between_digits_groups_thousands() {
        assert_eq!(parse_eur_minor("1 234,56"), 123_456);
        assert_eq!(parse_eur_minor("1\u{202f}234,56"), 123_456);
        assert_eq!(parse_eur_minor("1\u{a0}234,56"), 123_456);
        assert_eq!(parse_eur_minor("1\u{202f}234\u{202f}567,89"), 123_456_789);
        assert_eq!(parse_eur_minor("1 234.56"), 123_456);
        assert_eq!(parse_eur_minor("12 345"), 1_234_500);
        assert_eq!(
            parse_eur_minor("1  234"),
            123_400,
            "a run of spaces is one separator"
        );
        assert_eq!(parse_eur_minor("-1 234,50 EUR"), -123_450);
    }

    #[test]
    fn a_space_that_is_not_between_digits_is_still_ignored() {
        assert_eq!(parse_eur_minor(" 25 "), 2_500);
        assert_eq!(parse_eur_minor("- 25"), -2_500);
        assert_eq!(parse_eur_minor("25 -"), -2_500);
        assert_eq!(parse_eur_minor("€ 25"), 2_500);
        assert_eq!(parse_eur_minor("( 25,00 )"), -2_500);
        assert_eq!(parse_eur_minor("25 , 50"), 2_550);
    }

    #[test]
    fn an_apostrophe_or_a_space_is_a_grouping_only_when_it_is_well_formed() {
        for raw in [
            "1'23.45",
            "12'34",
            "'234",
            "234'",
            "1''234",
            "1'234'56",
            "0'123",
            "1234'567",
            "1 2 3,45",
            "12 34",
            "0 500",
            "1 234'567",
            "1'234 567",
            "1'234.567",
            "1'234.56.78",
            "1'234,567.89",
            "1.234'567",
            "1'234.5'6",
            "1 234,5 6",
        ] {
            assert_invalid_amount(raw, 2);
        }
    }

    #[test]
    fn an_apostrophe_grouping_follows_the_decimals_of_the_currency() {
        assert_eq!(parse_signed_minor("1'234.567", 3), Ok(1_234_567));
        assert_eq!(parse_signed_minor("1'234", 3), Ok(1_234_000));
        assert_eq!(parse_signed_minor("1'234", 0), Ok(1_234));
        assert_invalid_amount("1'234.5", 0);
        assert_eq!(parse_signed_minor("1 234,5678", 4), Ok(12_345_678));
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
        assert_eq!(currency_minor_exponent("eur".parse().unwrap()), 2);
        assert_eq!(currency_minor_exponent("JPY".parse().unwrap()), 0);
        assert_eq!(currency_minor_exponent("KWD".parse().unwrap()), 3);
        assert_eq!(currency_minor_exponent("XXX".parse().unwrap()), 2);
    }

    /// The codes where ISO 4217 and CLDR disagree, or where the table used to
    /// say 2 while the webview formats with no decimals. Each one, read with
    /// the wrong exponent, shows an amount 100 or 1000 times off.
    #[test]
    fn exponents_match_what_the_webview_formats_with() {
        for (code, digits) in [
            ("IQD", 0),
            ("XOF", 0),
            ("XAF", 0),
            ("UGX", 0),
            ("PYG", 0),
            ("RSD", 0),
            ("IRR", 0),
            ("ISK", 0),
            ("LYD", 3),
            ("CLF", 4),
        ] {
            assert_eq!(
                currency_minor_exponent(code.parse().unwrap()),
                digits,
                "{code}"
            );
        }
    }

    /// Every code with digits other than 2, as `Intl.NumberFormat` reports
    /// them in Node 20 (ICU 76, CLDR 46), checked across all three-letter
    /// codes so a code cannot drift in or out unnoticed.
    #[test]
    fn the_non_two_digit_codes_are_exactly_cldrs() {
        let letters = || 'A'..='Z';
        let mut by_digits: [Vec<String>; 5] = Default::default();
        for code in letters()
            .flat_map(|first| {
                letters().flat_map(move |second| letters().map(move |third| [first, second, third]))
            })
            .map(String::from_iter)
        {
            let digits = currency_minor_exponent(code.parse().unwrap());
            if digits != 2 {
                by_digits[usize::from(digits)].push(code);
            }
        }

        assert_eq!(
            by_digits[0],
            [
                "ADP", "AFN", "ALL", "BIF", "BYR", "CLP", "DJF", "ESP", "GNF", "IQD", "IRR", "ISK",
                "ITL", "JPY", "KMF", "KPW", "KRW", "LAK", "LBP", "LUF", "MGA", "MGF", "MMK", "MRO",
                "PYG", "RSD", "RWF", "SLL", "SOS", "STD", "SYP", "TMM", "TRL", "UGX", "UYI", "VND",
                "VUV", "XAF", "XOF", "XPF", "YER", "ZMK", "ZWD"
            ]
        );
        assert_eq!(by_digits[3], ["BHD", "JOD", "KWD", "LYD", "OMR", "TND"]);
        assert_eq!(by_digits[4], ["CLF", "UYW"]);
        assert!(by_digits[1].is_empty() && by_digits[2].is_empty());
    }

    #[test]
    fn a_three_digit_tail_is_a_grouping_in_a_four_decimal_currency() {
        assert_eq!(parse_signed_minor("1.234", 4).unwrap(), 12_340_000);
        assert_invalid_amount("0.125", 4);
        assert_eq!(parse_signed_minor("0.1250", 4).unwrap(), 1_250);
    }

    #[test]
    fn a_four_digit_unit_keeps_all_four_decimals() {
        assert_eq!(parse_signed_minor("1,2345", 4).expect("CLF"), 12_345);
        assert_eq!(parse_signed_minor("40.000,5", 4).expect("CLF"), 400_005_000);
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

    /// The separator conventions the parser documents for two decimals: a
    /// grouping separator (none when empty) and a decimal mark.
    const CONVENTIONS: [(&str, char); 14] = [
        ("", '.'),
        ("", ','),
        (",", '.'),
        (".", ','),
        ("'", '.'),
        ("'", ','),
        ("\u{2019}", '.'),
        ("\u{2019}", ','),
        (" ", ','),
        (" ", '.'),
        ("\u{a0}", ','),
        ("\u{202f}", ','),
        ("\u{202f}", '.'),
        ("\u{2009}", ','),
    ];

    /// The grouping separators that are never a decimal mark: an apostrophe,
    /// and whitespace between digits.
    const GROUPING_ONLY: [&str; 5] = ["'", "\u{2019}", " ", "\u{a0}", "\u{202f}"];

    /// `whole` with `group` after every third digit counted from the left,
    /// which is a thousands grouping only when the digits divide by three.
    fn grouped_from_the_left(whole: u64, group: &str) -> String {
        let digits = whole.to_string();
        let groups: Vec<&str> = digits
            .as_bytes()
            .chunks(3)
            .map(|digits| std::str::from_utf8(digits).unwrap())
            .collect();

        groups.join(group)
    }

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
            raw in "[0-9.,()€$ '\u{2019}\u{a0}\u{202f}A-Za-z\u{2212}-]{0,24}",
            exponent in 0_u8..=4,
        ) {
            let _ = parse_signed_minor(&raw, exponent);
        }

        // Digits, the separators and nothing else: whatever parses is the
        // digits of the cell read as one number, with at most `exponent`
        // zeros of padding after them. No separator adds, drops or reorders
        // a digit, whichever of them the parser took for the decimal mark.
        #[test]
        fn a_separator_never_changes_the_digits_of_an_amount(
            raw in "[0-9]{1,4}([.,' \u{202f}][0-9]{1,4}){0,3}",
            exponent in 0_u8..=4,
        ) {
            if let Ok(minor) = parse_signed_minor(&raw, exponent) {
                let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
                let digits: u128 = digits.parse().unwrap();
                let minor = u128::try_from(minor).unwrap();

                prop_assert!(
                    (0..=u32::from(exponent)).any(|zeros| digits * 10_u128.pow(zeros) == minor),
                    "{} read as {}",
                    raw,
                    minor
                );
            }
        }

        // Three capitals are a code: the book's, or the cell is refused,
        // wherever the code stands.
        #[test]
        fn a_code_in_capitals_is_the_books_or_the_cell_is_refused(
            minor in 1_i64..=99_999_999,
            code in "[A-Z]{3}",
            code_first in any::<bool>(),
        ) {
            let book: CurrencyCode = "EUR".parse().unwrap();
            let amount = written(minor, "", '.');
            let cell = if code_first {
                format!("{code} {amount}")
            } else {
                format!("{amount} {code}")
            };

            let expected = if code == "EUR" {
                Ok(minor)
            } else {
                Err(CsvError::InvalidAmount(cell.clone()))
            };
            prop_assert_eq!(parse_book_amount(&cell, book), expected, "{}", cell);
        }

        // Three letters with a lowercase one among them are a word, and a
        // word never changes the amount: the cell reads as it does without
        // it, in the book's currency, whatever the book.
        #[test]
        fn three_letters_not_all_in_capitals_never_change_the_amount(
            minor in (i64::MIN + 1)..=i64::MAX,
            word in "[A-Za-z]{3}".prop_filter(
                "a lowercase letter",
                |word| word.bytes().any(|byte| byte.is_ascii_lowercase()),
            ),
            book in "[A-Z]{3}",
            (group, decimal) in proptest::sample::select(CONVENTIONS.to_vec()),
            word_first in any::<bool>(),
        ) {
            let book: CurrencyCode = book.parse().unwrap();
            let unsigned = written(minor, group, decimal);
            let amount = if minor < 0 { format!("-{unsigned}") } else { unsigned };
            let cell = if word_first {
                format!("{word} {amount}")
            } else {
                format!("{amount} {word}")
            };

            let without_the_word = parse_book_amount(&amount, book);
            // Refused cells carry the cell as written, so only what parses
            // is compared.
            if let Ok(parsed) = without_the_word {
                prop_assert_eq!(parse_book_amount(&cell, book), Ok(parsed), "{}", cell);
            } else {
                prop_assert!(parse_book_amount(&cell, book).is_err(), "{}", cell);
            }
        }

        #[test]
        fn a_lowercase_suffix_never_changes_a_two_decimal_amount(
            minor in (i64::MIN + 1)..=i64::MAX,
            word in "[a-z]{3}",
        ) {
            let book: CurrencyCode = "EUR".parse().unwrap();
            let unsigned = written(minor, "", '.');
            let amount = if minor < 0 { format!("-{unsigned}") } else { unsigned };

            let cell = format!("{amount} {word}");
            prop_assert_eq!(parse_book_amount(&cell, book), Ok(minor), "{}", cell);
        }

        #[test]
        fn a_grouping_counted_from_the_wrong_end_is_rejected(
            // Four, five, seven, eight, ten or eleven digits: more than one
            // group, and a number of digits that does not divide by three.
            whole in prop_oneof![
                1_000_u64..=99_999,
                1_000_000_u64..=99_999_999,
                1_000_000_000_u64..=99_999_999_999,
            ],
            cents in 0_u8..100,
        ) {
            for group in GROUPING_ONLY {
                let cell = format!("{},{cents:02}", grouped_from_the_left(whole, group));
                prop_assert!(
                    matches!(parse_signed_minor(&cell, 2), Err(CsvError::InvalidAmount(_))),
                    "{}",
                    cell
                );
            }
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
