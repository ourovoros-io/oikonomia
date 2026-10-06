//! Small parsing helpers shared across repositories.

use time::Date;
use time::Month;
use uuid::Uuid;

use crate::error::{Error, Result, ValidationError};

/// Parse a UUID string.
///
/// # Errors
///
/// Returns [`Error::Validation`] on invalid UUID text.
pub fn parse_uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|_| {
        Error::Validation(ValidationError::Internal {
            detail: format!("invalid id: {s}"),
        })
    })
}

/// The error for text that is not a real calendar date.
fn invalid_date(text: &str) -> Error {
    Error::Validation(ValidationError::InvalidDate {
        value: text.to_owned(),
    })
}

/// Format a calendar date as `YYYY-MM-DD`.
#[must_use]
pub fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

/// Parse `YYYY-MM-DD`.
///
/// # Errors
///
/// Returns [`Error::Validation`] when the string is not a valid date.
pub fn parse_date(s: &str) -> Result<Date> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(invalid_date(s));
    }

    let year: i32 = parts[0].parse().map_err(|_| invalid_date(s))?;
    let month_num: u8 = parts[1].parse().map_err(|_| invalid_date(s))?;
    let day: u8 = parts[2].parse().map_err(|_| invalid_date(s))?;

    let month = Month::try_from(month_num).map_err(|_| invalid_date(s))?;

    Date::from_calendar_date(year, month, day).map_err(|_| invalid_date(s))
}

/// ISO `YYYY-MM-DD` (de)serialization for `time::Date` fields crossing IPC.
///
/// The `time` crate's derive-default serializes `Date` as `[year, ordinal]`,
/// which the frontend cannot render.
pub mod serde_date {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::Date;

    /// Serialize a date as `YYYY-MM-DD`.
    ///
    /// # Errors
    ///
    /// Serializer errors only.
    pub fn serialize<S: Serializer>(date: &Date, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::format_date(*date))
    }

    /// Deserialize a `YYYY-MM-DD` date.
    ///
    /// # Errors
    ///
    /// Invalid date text.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Date, D::Error> {
        let s = String::deserialize(deserializer)?;
        super::parse_date(&s).map_err(serde::de::Error::custom)
    }
}

/// Calendar date of the current UTC instant.
///
/// Recurring due flags use this so `next_date <= today` is consistent across
/// the crate.
#[must_use]
pub fn utc_today() -> Date {
    time::OffsetDateTime::now_utc().date()
}

/// Current UTC timestamp as RFC3339-ish SQL text.
#[must_use]
pub fn now_utc_string() -> String {
    // time crate without clock feature — use std for wall clock.
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    // Enough for ordering; not used for accounting dates.
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Dated {
        #[serde(with = "super::serde_date")]
        date: time::Date,
    }

    #[test]
    fn serde_date_round_trips_as_iso_string() {
        let date = parse_date("2026-08-10").unwrap();
        let json = serde_json::to_string(&Dated { date }).unwrap();
        assert_eq!(json, r#"{"date":"2026-08-10"}"#);

        let back: Result<Dated> = serde_json::from_str(&json).map_err(|e| {
            Error::Validation(ValidationError::Internal {
                detail: e.to_string(),
            })
        });
        assert_eq!(back.map(|d| d.date), Ok(date));
    }
}
