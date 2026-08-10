//! Small parsing helpers shared across repositories.

use time::Date;
use time::Month;
use uuid::Uuid;

use crate::error::{Error, Result};

/// Parse a UUID string.
///
/// # Errors
///
/// Returns [`Error::Validation`] on invalid UUID text.
pub fn parse_uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|_| Error::Validation(format!("invalid id: {s}")))
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
        return Err(Error::Validation(format!("invalid date: {s}")));
    }

    let year: i32 = parts[0]
        .parse()
        .map_err(|_| Error::Validation(format!("invalid date: {s}")))?;
    let month_num: u8 = parts[1]
        .parse()
        .map_err(|_| Error::Validation(format!("invalid date: {s}")))?;
    let day: u8 = parts[2]
        .parse()
        .map_err(|_| Error::Validation(format!("invalid date: {s}")))?;

    let month =
        Month::try_from(month_num).map_err(|_| Error::Validation(format!("invalid date: {s}")))?;

    Date::from_calendar_date(year, month, day)
        .map_err(|_| Error::Validation(format!("invalid date: {s}")))
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
