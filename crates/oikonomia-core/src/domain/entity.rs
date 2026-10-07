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

use crate::csv::currency_minor_exponent;
use crate::domain::currency::CurrencyCode;
use crate::domain::define_id;
use serde::{Deserialize, Serialize, Serializer};
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
///
/// # Wire form
///
/// The JSON form has one member the struct does not: `base_currency_decimals`,
/// the value of [`Entity::base_currency_decimals`], written after
/// `base_currency`. The UI converts between minor units and a displayed or
/// typed amount with that number and no other, so its own currency data
/// cannot disagree with the amounts core stores.
///
/// The member is computed on every write and ignored on read. It is not a
/// field, so an entity cannot hold a number of decimals that is not its
/// currency's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

impl Entity {
    /// Returns the number of decimals of the minor unit every amount in these
    /// books is counted in: 2 for `EUR`, 0 for `JPY`, 3 for `KWD`.
    ///
    /// It is [`currency_minor_exponent`] of [`Entity::base_currency`], and it
    /// is sent to the UI with the entity.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::domain::Entity;
    ///
    /// let book: Entity = serde_json::from_str(
    ///     r#"{"id":"11111111-1111-4111-8111-111111111111","name":"Tokyo",
    ///         "base_currency":"JPY","fiscal_year_start_month":4,
    ///         "chart_template":"personal"}"#,
    /// )?;
    ///
    /// assert_eq!(book.base_currency_decimals(), 0);
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    #[must_use]
    pub fn base_currency_decimals(&self) -> u8 {
        currency_minor_exponent(self.base_currency)
    }
}

impl Serialize for Entity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let json = EntityJson {
            id: &self.id,
            name: &self.name,
            base_currency: self.base_currency,
            base_currency_decimals: self.base_currency_decimals(),
            fiscal_year_start_month: self.fiscal_year_start_month,
            chart_template: self.chart_template,
        };

        json.serialize(serializer)
    }
}

/// The JSON form of an [`Entity`]: its fields, and the number of decimals of
/// its currency.
#[derive(Serialize)]
#[serde(rename = "Entity")]
struct EntityJson<'entity> {
    /// [`Entity::id`].
    id: &'entity EntityId,
    /// [`Entity::name`].
    name: &'entity str,
    /// [`Entity::base_currency`].
    base_currency: CurrencyCode,
    /// [`Entity::base_currency_decimals`].
    base_currency_decimals: u8,
    /// [`Entity::fiscal_year_start_month`], as its number.
    #[serde(with = "crate::util::serde_month")]
    fiscal_year_start_month: Month,
    /// [`Entity::chart_template`].
    chart_template: ChartTemplate,
}

#[cfg(test)]
mod tests {
    use super::{ChartTemplate, Entity};

    /// The JSON of a book in `currency`, as core writes it.
    fn written_book(currency: &str) -> serde_json::Value {
        let book: Entity = serde_json::from_value(serde_json::json!({
            "id": "11111111-1111-4111-8111-111111111111",
            "name": "Home",
            "base_currency": currency,
            "fiscal_year_start_month": 1,
            "chart_template": "personal",
        }))
        .unwrap();

        serde_json::to_value(book).unwrap()
    }

    #[test]
    fn the_json_of_an_entity_carries_the_decimals_of_its_currency() {
        for (currency, decimals) in [("EUR", 2), ("JPY", 0), ("KWD", 3)] {
            assert_eq!(
                written_book(currency)["base_currency_decimals"],
                serde_json::json!(decimals),
                "{currency}"
            );
        }
    }

    #[test]
    fn the_decimals_an_entity_is_read_with_are_ignored() {
        let book: Entity = serde_json::from_value(serde_json::json!({
            "id": "11111111-1111-4111-8111-111111111111",
            "name": "Home",
            "base_currency": "JPY",
            "base_currency_decimals": 2,
            "fiscal_year_start_month": 1,
            "chart_template": "personal",
        }))
        .unwrap();

        assert_eq!(book.base_currency_decimals(), 0);
        assert_eq!(
            serde_json::to_value(book).unwrap()["base_currency_decimals"],
            0
        );
    }

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
