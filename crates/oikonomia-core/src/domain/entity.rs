//! Multi-entity books (personal, company, …).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for an accounting entity (book).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub Uuid);

impl EntityId {
    /// Generate a new random entity id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::new()
    }
}

/// Starter chart-of-accounts template applied when creating an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartTemplate {
    /// Personal cash tracking chart.
    Personal,
    /// Small company / sole-trader chart.
    Company,
    /// Empty chart; user defines accounts.
    Blank,
}

/// An independent set of books with one base currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// Primary key.
    pub id: EntityId,
    /// Display name (e.g. "Personal", "Acme Ltd").
    pub name: String,
    /// ISO 4217 currency code (e.g. "EUR").
    pub base_currency: String,
    /// Month when the fiscal year starts (1–12).
    pub fiscal_year_start_month: u8,
    /// Template used at creation (informational).
    pub chart_template: ChartTemplate,
}
