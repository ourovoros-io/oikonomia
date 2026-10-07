//! Entities: the separate sets of books one vault can hold.
//!
//! An [`Entity`] is a household, a company, or anything else whose money is
//! kept apart. Each has its own chart of accounts and its own journal, and
//! nothing crosses between two entities: an entry and every account on its
//! lines belong to the same one, which [`crate::ledger`] checks on every
//! post.
//!
//! An entity has exactly one currency, so no amount in the crate carries a
//! currency of its own and nothing is ever converted.
//!
//! The [`ChartTemplate`] chosen at creation is kept on the entity because it
//! is still needed afterwards: the seeded accounts that play a role by
//! default are found by the template's codes (see [`crate::coa`] and
//! [`crate::default_accounts`]).

use crate::domain::currency::CurrencyCode;
use crate::domain::define_id;
use serde::{Deserialize, Serialize};
use time::Month;

define_id! {
    /// Identifies one [`Entity`].
    EntityId
}

/// The starter chart of accounts an entity is created with.
///
/// [`crate::coa::template_accounts`] lists the accounts each one seeds.
/// Serialized in `snake_case`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartTemplate {
    /// A household chart: cash and bank accounts, cards and loans, salary,
    /// and living expenses.
    Personal,
    /// A small-company chart: receivables and payables, capital and retained
    /// earnings, sales, and operating expenses.
    Company,
    /// No seeded accounts; the user creates every account.
    Blank,
}

impl ChartTemplate {
    /// Returns the template as the UI and the vault write it: `personal`,
    /// `company` or `blank`.
    ///
    /// This is the text serde writes and the text stored in
    /// `entities.chart_template`, so it is part of the vault format.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::domain::ChartTemplate;
    ///
    /// assert_eq!(ChartTemplate::Company.identifier(), "company");
    /// ```
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Company => "company",
            Self::Blank => "blank",
        }
    }
}

/// One set of books with a single base currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// The entity's own id.
    pub id: EntityId,
    /// The name shown to the user, such as "Personal" or "Acme Ltd".
    pub name: String,
    /// The currency of every amount in these books, serialized as its three
    /// capital letters such as `EUR`.
    pub base_currency: CurrencyCode,
    /// The month the fiscal year starts in, serialized as its number from 1
    /// (January) to 12.
    #[serde(with = "crate::util::serde_month")]
    pub fiscal_year_start_month: Month,
    /// The template the chart was seeded from, which also decides the
    /// seeded accounts that are defaults for each role.
    pub chart_template: ChartTemplate,
}

#[cfg(test)]
mod tests {
    use super::ChartTemplate;

    #[test]
    fn the_identifier_of_a_chart_template_is_the_text_serde_writes() {
        for template in [
            ChartTemplate::Personal,
            ChartTemplate::Company,
            ChartTemplate::Blank,
        ] {
            assert_eq!(
                serde_json::to_value(template).unwrap(),
                serde_json::Value::from(template.identifier())
            );
        }
    }
}
