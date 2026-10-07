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

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies one [`Entity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub Uuid);

impl EntityId {
    /// Returns a new random (version 4) id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EntityId {
    /// Returns a new random id, the same as [`EntityId::new`], not a fixed
    /// value.
    fn default() -> Self {
        Self::new()
    }
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

/// One set of books with a single base currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// The entity's own id.
    pub id: EntityId,
    /// The name shown to the user, such as "Personal" or "Acme Ltd".
    pub name: String,
    /// The currency of every amount in these books, as three capital
    /// letters such as `EUR`. It is meant to be an ISO 4217 code; only the
    /// shape is checked when the entity is created.
    pub base_currency: String,
    /// The month the fiscal year starts in, from 1 (January) to 12.
    pub fiscal_year_start_month: u8,
    /// The template the chart was seeded from, which also decides the
    /// seeded accounts that are defaults for each role.
    pub chart_template: ChartTemplate,
}
