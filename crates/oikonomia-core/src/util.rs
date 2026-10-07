//! Dates, ids and timestamps in the text forms the database and the UI use.
//!
//! `SQLite` has no date or id column type, and the web UI exchanges JSON, so
//! three kinds of value cross those boundaries as text. This module is the
//! one place that says what that text looks like.
//!
//! # Calendar dates
//!
//! An accounting date is a [`time::Date`] in memory and `YYYY-MM-DD` in the
//! database and over IPC. [`format_date`] writes that form and [`parse_date`]
//! reads it; [`serde_date`] applies the pair to a struct field.
//!
//! The form is fixed-width on purpose. Queries compare and sort date columns
//! as text, and text order equals date order only when every date has the
//! same shape, so [`parse_date`] accepts exactly the ten-character form and
//! nothing looser. The two functions are inverses for years 0000 to 9999.
//! [`format_date`] writes an earlier year with a leading minus sign, which
//! [`parse_date`] refuses, so such a date cannot be stored.
//!
//! # Today
//!
//! [`utc_today`] is the calendar date in UTC, not in the user's time zone;
//! its documentation says what that costs.
//!
//! # Timestamps
//!
//! [`now_utc_string`] gives the text stored in the `created_at`, `posted_at`
//! and `archived_at` columns. It is not an accounting date, and core never
//! parses it back.
//!
//! # Ids
//!
//! Ids are UUIDs stored as their hyphenated text; [`parse_uuid`] reads one.

use crate::error::{Error, Result, ValidationError};
use time::{Date, Month};
use uuid::Uuid;

/// Parses a UUID from its text form.
///
/// Every form [`Uuid::parse_str`] reads is accepted.
///
/// # Errors
///
/// Returns [`Error::Validation`] with [`ValidationError::Internal`] when
/// `text` is not a UUID. It is `Internal` because ids are produced by the
/// application, never typed by the user, so a bad one is a caller bug.
pub fn parse_uuid(text: &str) -> Result<Uuid> {
    Uuid::parse_str(text).map_err(|_| {
        ValidationError::Internal {
            detail: format!("invalid id: {text}"),
        }
        .into()
    })
}

/// Returns `date` as `YYYY-MM-DD`, zero-padded.
///
/// The text is ten characters for years 0000 to 9999. An earlier year is
/// written with a leading minus sign (`-001-12-31`), a form [`parse_date`]
/// rejects.
///
/// # Examples
///
/// ```
/// use oikonomia_core::util::format_date;
/// use time::{Date, Month};
///
/// let date = Date::from_calendar_date(2026, Month::March, 7)?;
/// assert_eq!(format_date(date), "2026-03-07");
/// # Ok::<(), time::error::ComponentRange>(())
/// ```
#[must_use]
pub fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

/// Parses a calendar date written exactly as `YYYY-MM-DD`.
///
/// The text must be ten bytes: four ASCII digits, `-`, two ASCII digits, `-`,
/// two ASCII digits, naming a day that exists. Nothing else is accepted: no
/// sign, no surrounding space, no shorter or longer part (`2026-8-1`), no
/// other digits. That makes this the inverse of [`format_date`] for years
/// 0000 to 9999, and guarantees that accepted texts compare in date order.
///
/// # Examples
///
/// ```
/// use oikonomia_core::util::{format_date, parse_date};
///
/// let date = parse_date("2026-08-10")?;
/// assert_eq!(format_date(date), "2026-08-10");
///
/// assert!(parse_date("2026-8-10").is_err());
/// assert!(parse_date("2026-02-30").is_err());
/// # Ok::<(), oikonomia_core::Error>(())
/// ```
///
/// # Errors
///
/// Returns [`Error::Validation`] with [`ValidationError::InvalidDate`], which
/// carries `text` unchanged, when `text` does not have that form or names a
/// day that does not exist.
pub fn parse_date(text: &str) -> Result<Date> {
    calendar_date(text).ok_or_else(|| invalid_date(text))
}

/// Returns the date `text` names when it is exactly `YYYY-MM-DD`.
fn calendar_date(text: &str) -> Option<Date> {
    // A fixed-length byte pattern: any multi-byte character changes the
    // length or puts a non-digit byte where a digit is required.
    let &[y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1] = text.as_bytes() else {
        return None;
    };

    let year = decimal_value(&[y0, y1, y2, y3])?;
    let month = decimal_value(&[m0, m1])?;
    let day = decimal_value(&[d0, d1])?;

    let month = Month::try_from(u8::try_from(month).ok()?).ok()?;
    let day = u8::try_from(day).ok()?;

    Date::from_calendar_date(i32::from(year), month, day).ok()
}

/// Returns the number that `digits` spell in base ten, or `None` when a byte
/// is not an ASCII digit.
///
/// Callers pass at most four digits, so the value is at most 9999 and the
/// arithmetic stays inside `u16`.
fn decimal_value(digits: &[u8]) -> Option<u16> {
    digits.iter().try_fold(0_u16, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u16::from(byte - b'0'))
    })
}

/// Returns the error for `text` that is not a `YYYY-MM-DD` calendar date.
fn invalid_date(text: &str) -> Error {
    ValidationError::InvalidDate {
        value: text.to_owned(),
    }
    .into()
}

/// Serializes a [`time::Date`] field as a `YYYY-MM-DD` string, for use with
/// `#[serde(with = "crate::util::serde_date")]`.
///
/// Without this, the `time` crate as built here (no `serde-human-readable`
/// feature) serializes a `Date` as the pair `[year, ordinal]`, which the web
/// UI cannot use.
///
/// # Examples
///
/// ```
/// use oikonomia_core::util::serde_date;
/// use serde::{Deserialize, Serialize};
/// use time::{Date, Month};
///
/// #[derive(Debug, PartialEq, Serialize, Deserialize)]
/// struct Due {
///     #[serde(with = "serde_date")]
///     on: Date,
/// }
///
/// let due = Due { on: Date::from_calendar_date(2026, Month::August, 10)? };
/// let json = serde_json::to_string(&due)?;
/// assert_eq!(json, r#"{"on":"2026-08-10"}"#);
/// assert_eq!(serde_json::from_str::<Due>(&json)?, due);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub mod serde_date {
    use crate::util::{format_date, parse_date};
    use serde::{Deserialize, Deserializer, Serializer};
    use time::Date;

    /// Serializes `date` as the string [`format_date`] gives.
    ///
    /// # Errors
    ///
    /// Returns the serializer's own error when it cannot write a string.
    pub fn serialize<S: Serializer>(date: &Date, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format_date(*date))
    }

    /// Deserializes a string through [`parse_date`].
    ///
    /// # Errors
    ///
    /// Returns the deserializer's error when the value is not a string, and
    /// a custom error with the message of
    /// [`ValidationError::InvalidDate`](crate::error::ValidationError::InvalidDate)
    /// when the string is not a `YYYY-MM-DD` date.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Date, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_date(&text).map_err(serde::de::Error::custom)
    }
}

/// Returns the calendar date of the current instant in UTC.
///
/// Recurring templates are due when their next date is on or before this
/// date, and the desktop shell passes it as "today" when it works out the
/// window of the cash-flow series, so core and the shell agree on the day.
///
/// That day is UTC's, not the user's. Ahead of UTC the local date changes
/// first: at UTC+3, from local midnight until 03:00 this still returns
/// yesterday's date, so a template due on the new local day is not yet
/// reported as due. Behind UTC the opposite happens: at UTC-5 this returns
/// tomorrow's date from 19:00 local time, and a template due tomorrow is
/// reported as due that evening.
#[must_use]
pub fn utc_today() -> Date {
    time::OffsetDateTime::now_utc().date()
}

/// Returns the current instant as `unix:` followed by whole seconds since the
/// Unix epoch, for example `unix:1791244800`.
///
/// The text is stored in the `created_at`, `posted_at` and `archived_at`
/// columns, and queries order rows by `created_at`; it is never an
/// accounting date. Comparing two such texts agrees with time order only
/// while both have the same number of digits: ten digits cover 2001-09-09 to
/// 2286-11-20 (`10^9` to `10^10 - 1` seconds). A clock set before 1970 gives
/// a negative count (`unix:-5`), which sorts before every ten-digit value but
/// not in time order among other negative ones.
///
/// The resolution is one second, so the text does not order two rows written
/// within the same second.
#[must_use]
pub fn now_utc_string() -> String {
    let seconds = time::OffsetDateTime::now_utc().unix_timestamp();

    format!("unix:{seconds}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    /// A struct with one date field that goes through [`serde_date`].
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Dated {
        /// The date under test.
        #[serde(with = "crate::util::serde_date")]
        date: time::Date,
    }

    #[test]
    fn serde_date_round_trips_as_iso_string() {
        let date = parse_date("2026-08-10").unwrap();
        let json = serde_json::to_string(&Dated { date }).unwrap();
        assert_eq!(json, r#"{"date":"2026-08-10"}"#);

        let back: Result<Dated> = serde_json::from_str(&json).map_err(|err| {
            Error::Validation(ValidationError::Internal {
                detail: err.to_string(),
            })
        });
        assert_eq!(back.map(|dated| dated.date), Ok(date));
    }

    #[test]
    fn only_the_ten_character_form_parses() {
        let rejected = [
            "",
            "2026-08",
            "2026-08-10-01",
            // Short or long parts: the widths are fixed.
            "2026-8-1",
            "2026-8-10",
            "2026-08-1",
            "26-08-10",
            "999-01-01",
            "02026-08-10",
            "2026-008-10",
            "2026-08-010",
            // Signs and spaces that integer parsing would take.
            "+2026-08-10",
            "2026-+8-10",
            "2026-08-+1",
            "-2026-08-10",
            " 2026-08-10",
            "2026-08-10 ",
            "2026-08-10\n",
            "+026-08-10",
            "2026-+8-01",
            // Other separators and digits.
            "2026/08/10",
            "2026.08.10",
            "20260810",
            "2026-08-1O",
            "２０２６-08-10",
            "2026-08-१०",
            "2026-08-10T00:00:00",
            // The right shape, but no such day.
            "2026-00-10",
            "2026-13-01",
            "2026-08-00",
            "2026-08-32",
            "2026-02-29",
            "2026-04-31",
            "1900-02-29",
        ];

        for text in rejected {
            assert_eq!(
                parse_date(text),
                Err(Error::Validation(ValidationError::InvalidDate {
                    value: text.to_owned()
                })),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_ten_character_form_parses_to_its_calendar_date() {
        let accepted = [
            ("2026-08-10", (2026, Month::August, 10)),
            ("2024-02-29", (2024, Month::February, 29)),
            ("2000-02-29", (2000, Month::February, 29)),
            ("0000-01-01", (0, Month::January, 1)),
            ("0999-12-31", (999, Month::December, 31)),
            ("9999-12-31", (9999, Month::December, 31)),
        ];

        for (text, (year, month, day)) in accepted {
            assert_eq!(
                parse_date(text),
                Ok(Date::from_calendar_date(year, month, day).unwrap()),
                "{text}"
            );
        }
    }

    #[test]
    fn the_timestamp_text_is_the_clock_in_whole_unix_seconds() {
        let before = time::OffsetDateTime::now_utc().unix_timestamp();
        let text = now_utc_string();
        let after = time::OffsetDateTime::now_utc().unix_timestamp();

        let seconds: i64 = text.strip_prefix("unix:").unwrap().parse().unwrap();
        assert!((before..=after).contains(&seconds), "{text}");
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// The first day of year zero, the earliest date [`parse_date`] reads.
    const YEAR_ZERO: Date = time::macros::date!(0000 - 01 - 01);

    /// Every date from `first` to the last one the `time` crate can hold.
    fn dates_from(first: Date) -> impl Strategy<Value = Date> {
        (first.to_julian_day()..=Date::MAX.to_julian_day())
            .prop_map(|day| Date::from_julian_day(day).unwrap())
    }

    // The smallest case of the ignored property below. Which side should
    // change is undecided: today the rejection also keeps such a date out of
    // the ledger, where dates are compared as text and a leading minus sign
    // would sort wrongly.
    #[test]
    fn a_date_before_year_zero_is_formatted_but_not_parsed() {
        let last_day_before_year_zero = YEAR_ZERO.previous_day().unwrap();
        let text = format_date(last_day_before_year_zero);

        assert_eq!(text, "-001-12-31");
        assert_eq!(
            parse_date(&text),
            Err(Error::Validation(ValidationError::InvalidDate {
                value: text
            }))
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn parsing_any_text_returns_instead_of_panicking(text in any::<String>()) {
            let _ = parse_date(&text);
        }

        #[test]
        fn parsing_date_shaped_text_returns_instead_of_panicking(text in "[-+0-9 ]{0,14}") {
            let _ = parse_date(&text);
        }

        #[test]
        fn a_formatted_date_from_year_zero_on_parses_back(date in dates_from(YEAR_ZERO)) {
            prop_assert_eq!(parse_date(&format_date(date)), Ok(date));
        }

        #[test]
        #[ignore = "format_date writes a year before zero with a leading minus sign, \
                    which parse_date rejects"]
        fn a_formatted_date_of_any_year_parses_back(date in dates_from(Date::MIN)) {
            prop_assert_eq!(parse_date(&format_date(date)), Ok(date));
        }
    }
}
