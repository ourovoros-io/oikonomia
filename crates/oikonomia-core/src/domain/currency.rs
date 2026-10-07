//! Currency codes: the three capital letters that name an entity's currency.
//!
//! A [`CurrencyCode`] has the shape of an ISO 4217 code, three ASCII capital
//! letters, and nothing checks that the letters name a currency that exists.
//! The shape is what the rest of the crate relies on: the code is a key into
//! the table of decimal digits
//! ([`currency_minor_exponent`](crate::csv::currency_minor_exponent)), where
//! an unknown code reads as two decimals, and the web UI hands it to
//! `Intl.NumberFormat`, which refuses anything that is not three letters.
//!
//! # One parser
//!
//! [`FromStr`] is the only way to make a code. It ignores surrounding
//! whitespace and the case of the letters, so `" eur "` reads as `EUR`, and
//! the value it returns always holds capitals. The code is therefore compared
//! and stored in one form, and no caller has to trim or change case.
//!
//! # Wire form
//!
//! A code serializes as the plain string, `"EUR"`. Deserializing goes through
//! the same parser.

use crate::error::{Error, Result, ValidationError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// The currency of an entity's books: three ASCII capital letters, such as
/// `EUR`.
///
/// It is meant to be an ISO 4217 code; only the shape is checked. Parsing
/// ignores surrounding whitespace and the case of the letters, and the value
/// always holds capitals, so a code is compared and stored in one form.
///
/// # Examples
///
/// ```
/// use oikonomia_core::domain::CurrencyCode;
///
/// let euro: CurrencyCode = " eur ".parse()?;
/// assert_eq!(euro.as_str(), "EUR");
/// assert_eq!(euro.to_string(), "EUR");
/// assert_eq!(serde_json::to_string(&euro)?, r#""EUR""#);
///
/// let refused = "12$".parse::<CurrencyCode>().map_err(|error| error.code());
/// assert_eq!(refused, Err("currency_invalid"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CurrencyCode {
    /// The three letters, each an ASCII capital.
    letters: [u8; 3],
}

impl CurrencyCode {
    /// Returns the code as its three capital letters.
    #[must_use]
    #[expect(
        clippy::expect_used,
        clippy::missing_panics_doc,
        reason = "the only constructor stores three ASCII letters, which are valid UTF-8"
    )]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.letters).expect("three ASCII letters are valid UTF-8")
    }
}

impl fmt::Display for CurrencyCode {
    /// Writes the three capital letters.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CurrencyCode {
    type Err = Error;

    /// Parses a code from three ASCII letters of either case, ignoring
    /// surrounding whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] with [`ValidationError::CurrencyInvalid`]
    /// when the trimmed text is not exactly three ASCII letters.
    fn from_str(text: &str) -> Result<Self> {
        // Matching three bytes that are each an ASCII letter makes the byte
        // count a character count too, so "12$" and "€" are both refused.
        let &[first, second, third] = text.trim().as_bytes() else {
            return Err(ValidationError::CurrencyInvalid.into());
        };
        let letters = [first, second, third];
        if !letters.iter().all(u8::is_ascii_alphabetic) {
            return Err(ValidationError::CurrencyInvalid.into());
        }

        Ok(Self {
            letters: letters.map(|letter| letter.to_ascii_uppercase()),
        })
    }
}

impl TryFrom<&str> for CurrencyCode {
    type Error = Error;

    /// Parses a code as [`FromStr`] does.
    ///
    /// # Errors
    ///
    /// Those of [`CurrencyCode::from_str`].
    fn try_from(text: &str) -> Result<Self> {
        text.parse()
    }
}

impl Serialize for CurrencyCode {
    /// Serializes the code as the string of its three letters.
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CurrencyCode {
    /// Deserializes a string through [`CurrencyCode::from_str`].
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_letters_of_either_case_parse_to_capitals() {
        for text in ["EUR", "eur", "Eur", " eur ", "\tEUR\n"] {
            assert_eq!(
                text.parse::<CurrencyCode>().map(|code| code.to_string()),
                Ok("EUR".to_owned()),
                "{text:?}"
            );
        }
    }

    #[test]
    fn anything_but_three_ascii_letters_is_refused() {
        for text in ["", "EU", "EURO", "12$", "€", "E U", "EU1", "ΕΥΡ", "É1"] {
            assert_eq!(
                text.parse::<CurrencyCode>(),
                Err(Error::Validation(ValidationError::CurrencyInvalid)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_code_serializes_as_the_plain_string_and_reads_back() {
        let code: CurrencyCode = "JPY".parse().unwrap();
        let json = serde_json::to_string(&code).unwrap();

        assert_eq!(json, r#""JPY""#);
        assert_eq!(serde_json::from_str::<CurrencyCode>(&json).unwrap(), code);
        assert!(serde_json::from_str::<CurrencyCode>(r#""yen!""#).is_err());
        assert!(serde_json::from_str::<CurrencyCode>("392").is_err());
    }

    #[test]
    fn try_from_agrees_with_from_str() {
        assert_eq!(CurrencyCode::try_from("usd"), "USD".parse());
        assert_eq!(CurrencyCode::try_from("us"), "us".parse());
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn three_letters_parse_to_their_capitals_and_back(
            letters in "[A-Za-z]{3}",
            before in "[ \t]{0,2}",
            after in "[ \t\n]{0,2}",
        ) {
            let code: CurrencyCode = format!("{before}{letters}{after}").parse().unwrap();

            prop_assert_eq!(code.as_str(), letters.to_ascii_uppercase());
            prop_assert_eq!(code.as_str().parse::<CurrencyCode>(), Ok(code));
            prop_assert_eq!(code.to_string().parse::<CurrencyCode>(), Ok(code));
        }

        #[test]
        fn text_parses_exactly_when_it_trims_to_three_ascii_letters(text in any::<String>()) {
            let trimmed = text.trim();
            let is_a_code =
                trimmed.len() == 3 && trimmed.bytes().all(|byte| byte.is_ascii_alphabetic());

            prop_assert_eq!(text.parse::<CurrencyCode>().is_ok(), is_a_code);
        }

        #[test]
        fn short_ascii_text_parses_exactly_when_it_is_three_letters(text in "[A-Za-z0-9 $]{0,5}") {
            let trimmed = text.trim();
            let is_a_code =
                trimmed.len() == 3 && trimmed.bytes().all(|byte| byte.is_ascii_alphabetic());

            prop_assert_eq!(text.parse::<CurrencyCode>().is_ok(), is_a_code);
        }
    }
}
