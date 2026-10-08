//! Finds a currency marker in an amount cell that is not the book's.
//!
//! A statement in another currency reads as the book's own whenever its
//! amounts are plain numbers, and nothing in the digits can tell. A marker
//! next to some of them can: `3 990 HUF`, `£25.00`, `14 500 Ft`. The preview
//! reports the first one so the user sees that the file may be in another
//! currency before anything is posted.
//!
//! This only looks. Whether a marked cell is a valid amount is the amount
//! parser's decision, in [`crate::csv::amount`]: a cell marked with another
//! code in capitals is an invalid row there as well, and a cell with the
//! book's own marker is accepted.
//!
//! The markers it knows are three capital letters bounded by anything but
//! another letter, the currency signs the amount parser strips, and the short
//! local names `Ft`, `zł` and `Kč`. A word in lowercase is not a code, which
//! matches the amount parser: `25 lei` is a word there too.

use crate::domain::CurrencyCode;

/// Currency signs and the codes each is used for. A sign is another
/// currency's marker only when the book's code is not among them, since `$`
/// is the dollar of several countries.
const SIGNS: [(char, &[&str]); 7] = [
    ('€', &["EUR"]),
    (
        '$',
        &[
            "USD", "CAD", "AUD", "NZD", "SGD", "HKD", "MXN", "ARS", "CLP", "COP",
        ],
    ),
    ('£', &["GBP", "EGP"]),
    ('¥', &["JPY", "CNY"]),
    ('₹', &["INR"]),
    ('₺', &["TRY"]),
    ('₩', &["KRW"]),
];

/// Local names of a currency, matched whole and case-sensitively, with the
/// code each stands for.
const NAMES: [(&str, &str); 3] = [("Ft", "HUF"), ("zł", "PLN"), ("Kč", "CZK")];

/// Returns the first marker in `cells` that names a currency other than
/// `book`, as written in the cell.
pub(super) fn first_other_currency<'a>(
    cells: impl IntoIterator<Item = &'a str>,
    book: CurrencyCode,
) -> Option<String> {
    cells.into_iter().find_map(|cell| other_marker(cell, book))
}

/// The first marker of `cell` that is not the book's currency.
fn other_marker(cell: &str, book: CurrencyCode) -> Option<String> {
    let code_marker = capital_codes(cell).find(|code| *code != book.as_str());
    if let Some(code) = code_marker {
        return Some(code.to_owned());
    }
    let sign = SIGNS
        .iter()
        .find(|(sign, codes)| cell.contains(*sign) && !codes.contains(&book.as_str()));
    if let Some((sign, _)) = sign {
        return Some(sign.to_string());
    }
    NAMES
        .iter()
        .find(|(name, code)| *code != book.as_str() && words(cell).any(|word| word == *name))
        .map(|(name, _)| (*name).to_owned())
}

/// The runs of exactly three ASCII capitals in `cell` that no other letter
/// touches.
fn capital_codes(cell: &str) -> impl Iterator<Item = &str> {
    cell.split(|character: char| !character.is_alphabetic())
        .filter(|run| run.len() == 3 && run.bytes().all(|byte| byte.is_ascii_uppercase()))
}

/// The runs of letters in `cell`, split at digits, signs and spaces.
fn words(cell: &str) -> impl Iterator<Item = &str> {
    cell.split(|character: char| !character.is_alphabetic())
        .filter(|run| !run.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(code: &str) -> CurrencyCode {
        code.parse().expect("a currency code")
    }

    fn marker(cell: &str, code: &str) -> Option<String> {
        first_other_currency([cell], book(code))
    }

    #[test]
    fn another_code_in_capitals_is_a_marker_on_either_side_of_the_number() {
        assert_eq!(marker("-3 990 HUF", "EUR").as_deref(), Some("HUF"));
        assert_eq!(marker("USD 25.00", "EUR").as_deref(), Some("USD"));
    }

    #[test]
    fn the_books_own_marker_is_not_one() {
        assert_eq!(marker("25,00 EUR", "EUR"), None);
        assert_eq!(marker("€25,00", "EUR"), None);
        assert_eq!(marker("$25.00", "CAD"), None);
        assert_eq!(marker("14 500 Ft", "HUF"), None);
    }

    #[test]
    fn a_sign_or_local_name_of_another_currency_is_a_marker() {
        assert_eq!(marker("£25.00", "EUR").as_deref(), Some("£"));
        assert_eq!(marker("$25.00", "EUR").as_deref(), Some("$"));
        assert_eq!(marker("-14 500 Ft", "EUR").as_deref(), Some("Ft"));
        assert_eq!(marker("120 zł", "EUR").as_deref(), Some("zł"));
    }

    #[test]
    fn plain_numbers_and_ordinary_words_are_not_markers() {
        assert_eq!(marker("485.000,00", "EUR"), None);
        assert_eq!(marker("25 lei", "RON"), None);
        assert_eq!(marker("25 usd", "EUR"), None);
        assert_eq!(marker("", "EUR"), None);
    }

    #[test]
    fn a_run_of_capitals_longer_than_a_code_is_not_one() {
        assert_eq!(marker("25 EURO", "EUR"), None);
        assert_eq!(marker("A25B", "EUR"), None);
    }

    #[test]
    fn the_first_marked_cell_decides() {
        let cells = ["1,00", "2,00 GBP", "3,00 HUF"];

        assert_eq!(
            first_other_currency(cells, book("EUR")).as_deref(),
            Some("GBP")
        );
    }
}
