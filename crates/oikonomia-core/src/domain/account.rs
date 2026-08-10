//! Chart of accounts.

use super::entity::EntityId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable account identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub Uuid);

impl AccountId {
    /// Generate a new random account id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self::new()
    }
}

/// High-level account classification for double-entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    /// Resources owned (cash, bank, AR).
    Asset,
    /// Obligations (AP, loans, credit cards).
    Liability,
    /// Owner interest (capital, retained earnings, opening balances).
    Equity,
    /// Revenue accounts.
    Income,
    /// Cost accounts.
    Expense,
}

impl AccountType {
    /// Whether this account type is debit-normal (assets and expenses).
    ///
    /// Assets and expenses use debits minus credits; liabilities, equity, and
    /// income use credits minus debits.
    #[must_use]
    pub const fn is_debit_normal(self) -> bool {
        matches!(self, Self::Asset | Self::Expense)
    }
}

/// A ledger account within an entity's chart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Primary key.
    pub id: AccountId,
    /// Owning entity.
    pub entity_id: EntityId,
    /// Human-readable code (e.g. "1000").
    pub code: String,
    /// Display name.
    pub name: String,
    /// Classification.
    pub account_type: AccountType,
    /// Optional parent for hierarchical charts.
    pub parent_id: Option<AccountId>,
    /// Soft-active flag.
    pub is_active: bool,
    /// System accounts (e.g. Opening Balances) have delete restrictions.
    pub is_system: bool,
    /// Display order within the chart.
    pub sort_order: i32,
}
