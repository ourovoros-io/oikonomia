//! Integer minor-unit money. Never use floating point for currency amounts.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Amount in the smallest currency unit (e.g. cents for EUR/USD).
///
/// Always non-negative at the type boundary for line amounts; signed
/// aggregates (balances) use plain `i64` where a sign is meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Money {
    amount_minor: i64,
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            amount_minor: i64,
        }

        let raw = Raw::deserialize(deserializer)?;
        Money::from_minor(raw.amount_minor).map_err(serde::de::Error::custom)
    }
}

impl Money {
    /// Zero amount.
    pub const ZERO: Self = Self { amount_minor: 0 };

    /// Create from minor units. Rejects negative values.
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

    /// Minor units as `i64`.
    #[must_use]
    pub const fn amount_minor(self) -> i64 {
        self.amount_minor
    }

    /// Checked addition.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MoneyOverflow`] if the sum does not fit in `i64`.
    pub fn checked_add(self, other: Self) -> Result<Self> {
        self.amount_minor
            .checked_add(other.amount_minor)
            .map(|amount_minor| Self { amount_minor })
            .ok_or(Error::MoneyOverflow)
    }

    /// Checked subtraction. Result must stay non-negative.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MoneyOverflow`] on underflow of the intermediate `i64`
    /// subtraction, or [`Error::NegativeMoney`] if the result would be negative.
    pub fn checked_sub(self, other: Self) -> Result<Self> {
        let amount_minor = self
            .amount_minor
            .checked_sub(other.amount_minor)
            .ok_or(Error::MoneyOverflow)?;

        if amount_minor < 0 {
            return Err(Error::NegativeMoney);
        }

        Ok(Self { amount_minor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_minor() {
        assert_eq!(Money::from_minor(-1), Err(Error::NegativeMoney));
    }

    #[test]
    fn add_and_sub() {
        let a = Money::from_minor(100);
        let b = Money::from_minor(40);
        assert!(a.is_ok());
        assert!(b.is_ok());

        let a = a.unwrap_or(Money::ZERO);
        let b = b.unwrap_or(Money::ZERO);

        assert_eq!(a.checked_add(b).map(Money::amount_minor), Ok(140));
        assert_eq!(a.checked_sub(b).map(Money::amount_minor), Ok(60));
        assert_eq!(b.checked_sub(a), Err(Error::NegativeMoney));
    }

    #[test]
    fn deserialize_rejects_negative() {
        let err = serde_json::from_str::<Money>(r#"{"amount_minor":-1}"#);
        assert!(err.is_err(), "negative Money must not deserialize");
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
}
