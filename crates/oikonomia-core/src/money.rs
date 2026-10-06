//! Non-negative amounts of money in integer minor units.
//!
//! # Representation
//!
//! An amount is an `i64` count of the currency's smallest unit (cents for
//! EUR). Floating point is never used for money anywhere in the crate: a
//! binary fraction cannot hold most decimal amounts exactly, and a ledger
//! must balance to the unit.
//!
//! [`Money`] carries no currency. Every amount in one book is in that book's
//! base currency ([`Entity::base_currency`](crate::domain::Entity)), and how
//! many minor units make one major unit is decided where amounts are read
//! from or shown to the user, not here.
//!
//! # Invariants
//!
//! - A [`Money`] is never negative. [`Money::from_minor`] and the
//!   `Deserialize` impl are the only ways to make one from a number, and both
//!   refuse a negative count. Which side of an entry an amount sits on is
//!   said by the debit and credit fields of a
//!   [`JournalLine`](crate::domain::JournalLine), not by a sign.
//! - Arithmetic never wraps or saturates. [`Money::checked_add`] and
//!   [`Money::checked_sub`] return an error instead, because a wrapped total
//!   would be reported as a correct one.
//!
//! Balances and report totals can be negative, so they are plain `i64` minor
//! units rather than [`Money`]; see [`crate::ledger`] for how those are kept
//! from overflowing.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// A non-negative amount in the smallest unit of a currency.
///
/// The amount is at least zero and at most `i64::MAX`. Ordering and equality
/// compare the amounts. It serializes as `{"amount_minor": <integer>}`, and
/// deserializing a negative integer fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Money {
    /// The count of minor units; never negative.
    amount_minor: i64,
}

impl Money {
    /// The amount of no money at all.
    pub const ZERO: Self = Self { amount_minor: 0 };

    /// Returns the amount of `amount_minor` minor units.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::{Error, Money};
    ///
    /// let price = Money::from_minor(1_250)?;
    /// assert_eq!(price.amount_minor(), 1_250);
    ///
    /// assert_eq!(Money::from_minor(-1), Err(Error::NegativeMoney));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::NegativeMoney`] when `amount_minor` is negative.
    pub fn from_minor(amount_minor: i64) -> Result<Self> {
        if amount_minor < 0 {
            return Err(Error::NegativeMoney);
        }

        Ok(Self { amount_minor })
    }

    /// Returns the count of minor units, which is never negative.
    #[must_use]
    pub const fn amount_minor(self) -> i64 {
        self.amount_minor
    }

    /// Returns the sum of the two amounts.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::{Error, Money};
    ///
    /// let rent = Money::from_minor(80_000)?;
    /// let power = Money::from_minor(4_550)?;
    /// assert_eq!(rent.checked_add(power)?.amount_minor(), 84_550);
    ///
    /// let largest = Money::from_minor(i64::MAX)?;
    /// let one = Money::from_minor(1)?;
    /// assert_eq!(largest.checked_add(one), Err(Error::MoneyOverflow));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::MoneyOverflow`] when the sum is greater than
    /// `i64::MAX`.
    pub fn checked_add(self, other: Self) -> Result<Self> {
        self.amount_minor
            .checked_add(other.amount_minor)
            .map(|amount_minor| Self { amount_minor })
            .ok_or(Error::MoneyOverflow)
    }

    /// Returns `self` less `other`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NegativeMoney`] when `other` is greater than `self`.
    pub fn checked_sub(self, other: Self) -> Result<Self> {
        // Both amounts lie in `0..=i64::MAX`, so the difference lies in
        // `-i64::MAX..=i64::MAX` and always fits: `checked_sub` cannot return
        // `None` here, and the only failure left is a negative difference.
        self.amount_minor
            .checked_sub(other.amount_minor)
            .filter(|difference| *difference >= 0)
            .map(|amount_minor| Self { amount_minor })
            .ok_or(Error::NegativeMoney)
    }
}

impl<'de> Deserialize<'de> for Money {
    /// Reads `{"amount_minor": <integer>}` through [`Money::from_minor`], so
    /// a negative amount is an error instead of a value that breaks the
    /// type's invariant.
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        /// The serialized shape of [`Money`], before the sign is checked.
        #[derive(Deserialize)]
        struct Raw {
            /// The count of minor units as written, possibly negative.
            amount_minor: i64,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::from_minor(raw.amount_minor).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_minor() {
        assert_eq!(Money::from_minor(-1), Err(Error::NegativeMoney));
        assert_eq!(Money::from_minor(i64::MIN), Err(Error::NegativeMoney));
    }

    #[test]
    fn add_and_sub() {
        let hundred = Money::from_minor(100).unwrap();
        let forty = Money::from_minor(40).unwrap();

        assert_eq!(hundred.checked_add(forty).map(Money::amount_minor), Ok(140));
        assert_eq!(hundred.checked_sub(forty).map(Money::amount_minor), Ok(60));
        assert_eq!(forty.checked_sub(hundred), Err(Error::NegativeMoney));
    }

    #[test]
    fn a_sum_past_the_largest_amount_is_an_overflow() {
        let largest = Money::from_minor(i64::MAX).unwrap();
        let one = Money::from_minor(1).unwrap();

        assert_eq!(largest.checked_add(one), Err(Error::MoneyOverflow));
        assert_eq!(one.checked_add(largest), Err(Error::MoneyOverflow));
        assert_eq!(largest.checked_add(largest), Err(Error::MoneyOverflow));
    }

    #[test]
    fn a_sum_of_exactly_the_largest_amount_is_not_an_overflow() {
        let largest = Money::from_minor(i64::MAX).unwrap();
        let below = Money::from_minor(i64::MAX - 1).unwrap();
        let one = Money::from_minor(1).unwrap();

        assert_eq!(below.checked_add(one), Ok(largest));
        assert_eq!(largest.checked_add(Money::ZERO), Ok(largest));
    }

    #[test]
    fn subtraction_at_the_ends_of_the_range_is_exact_or_negative() {
        let largest = Money::from_minor(i64::MAX).unwrap();

        assert_eq!(largest.checked_sub(largest), Ok(Money::ZERO));
        assert_eq!(largest.checked_sub(Money::ZERO), Ok(largest));
        assert_eq!(
            Money::ZERO.checked_sub(largest),
            Err(Error::NegativeMoney),
            "0 - i64::MAX fits in i64, so this is a sign error, not an overflow"
        );
    }

    #[test]
    fn deserialize_rejects_negative() {
        let result = serde_json::from_str::<Money>(r#"{"amount_minor":-1}"#);
        assert!(result.is_err(), "negative Money must not deserialize");
    }

    #[test]
    fn deserialize_accepts_zero_and_positive() {
        assert_eq!(
            serde_json::from_str::<Money>(r#"{"amount_minor":0}"#).ok(),
            Some(Money::ZERO)
        );
        assert_eq!(
            serde_json::from_str::<Money>(r#"{"amount_minor":50}"#).ok(),
            Money::from_minor(50).ok()
        );
    }

    #[test]
    fn serializes_as_an_object_with_the_minor_units() {
        let amount = Money::from_minor(50).unwrap();

        assert_eq!(
            serde_json::to_value(amount).ok(),
            Some(serde_json::json!({ "amount_minor": 50 }))
        );
    }
}
