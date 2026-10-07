//! Arithmetic on calendar months, for the code that steps a date by whole
//! months: the schedule of a monthly template and the window a dashboard is
//! compared against.
//!
//! A month is counted from January of year zero, so that a step of any size
//! in either direction is one addition, and the year carries by division.
//! Nothing here knows about days: the callers place a day in the month they
//! get back, each by its own rule.

use time::{Date, Month};

/// Numbers the calendar months from year zero: January of year 0 is 0,
/// February is 1, and January of year 1 is 12.
///
/// The product cannot overflow: `year` is an `i32`, and twelve times its
/// largest size, plus eleven, is far inside `i64`.
fn month_index(year: i32, month: Month) -> i64 {
    i64::from(year) * 12 + i64::from(u8::from(month)) - 1
}

/// Returns how many calendar months `to`'s month lies after `from`'s:
/// 0 within one month, 1 from any day of January to any day of February of
/// the same year, and a negative number when `to` is in an earlier month.
pub(super) fn months_between(from: Date, to: Date) -> i64 {
    month_index(to.year(), to.month()) - month_index(from.year(), from.month())
}

/// Returns the year and month that lie `delta` calendar months after `month`
/// of `year`; a negative `delta` goes back.
///
/// `None` when the count of months overflows or the year does not fit in
/// `i32`. A year that fits may still be outside the calendar the `time`
/// crate holds; building a date from the result reports that.
pub(super) fn add_months(year: i32, month: Month, delta: i64) -> Option<(i32, Month)> {
    let months = month_index(year, month).checked_add(delta)?;

    let year = i32::try_from(months.div_euclid(12)).ok()?;
    let month_number = u8::try_from(months.rem_euclid(12) + 1).ok()?;
    let month = Month::try_from(month_number).ok()?;

    Some((year, month))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn adding_months_carries_into_the_year_in_both_directions() {
        assert_eq!(
            add_months(2026, Month::January, 1),
            Some((2026, Month::February))
        );
        assert_eq!(
            add_months(2026, Month::December, 1),
            Some((2027, Month::January))
        );
        assert_eq!(
            add_months(2026, Month::January, -1),
            Some((2025, Month::December))
        );
        assert_eq!(
            add_months(2026, Month::March, -15),
            Some((2024, Month::December))
        );
        assert_eq!(
            add_months(2026, Month::March, 0),
            Some((2026, Month::March))
        );
        assert_eq!(
            add_months(0, Month::January, -1),
            Some((-1, Month::December))
        );
    }

    #[test]
    fn adding_months_refuses_a_year_that_does_not_fit() {
        assert_eq!(add_months(i32::MAX, Month::December, 1), None);
        assert_eq!(add_months(i32::MIN, Month::January, -1), None);
        assert_eq!(add_months(2026, Month::January, i64::MAX), None);
        assert_eq!(
            add_months(i32::MAX, Month::November, 1),
            Some((i32::MAX, Month::December))
        );
    }

    #[test]
    fn months_between_counts_calendar_months_whatever_the_days() {
        assert_eq!(
            months_between(date!(2026 - 01 - 31), date!(2026 - 01 - 01)),
            0
        );
        assert_eq!(
            months_between(date!(2026 - 01 - 31), date!(2026 - 02 - 01)),
            1
        );
        assert_eq!(
            months_between(date!(2025 - 11 - 15), date!(2026 - 02 - 15)),
            3
        );
        assert_eq!(
            months_between(date!(2026 - 02 - 15), date!(2025 - 11 - 15)),
            -3
        );
    }

    #[test]
    fn adding_the_months_between_two_dates_gives_the_month_of_the_second() {
        let (from, to) = (date!(2019 - 08 - 20), date!(2026 - 03 - 05));

        assert_eq!(
            add_months(from.year(), from.month(), months_between(from, to)),
            Some((to.year(), to.month()))
        );
    }
}
