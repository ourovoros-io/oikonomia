//! Accounts and the five account types.
//!
//! An [`Account`] is one line of an entity's chart: Cash, Rent, Salary. Its
//! [`AccountType`] is the only thing double entry needs to know about it,
//! because the type fixes the account's normal side:
//!
//! | Type | Normal side | A debit | A credit |
//! |------|-------------|---------|----------|
//! | Asset, Expense | debit | raises the balance | lowers it |
//! | Liability, Equity, Income | credit | lowers the balance | raises it |
//!
//! [`AccountType::is_debit_normal`] is that table, and
//! [`crate::ledger::normal_balance`] uses it to turn summed debits and
//! credits into a balance that is positive on the account's normal side.
//!
//! An account is never chosen by its name anywhere in the crate. The name is
//! free text the user can change, so the code and the type identify an
//! account (see [`crate::default_accounts`]).
//!
//! An account is archived, not deleted: [`Account::is_active`] goes to
//! `false` and the account's entries stay in the book. Accounts are removed
//! only together with their whole entity.
//!
//! The chart is flat: an account has no parent. The `accounts` table has a
//! `parent_id` column that is reserved and not part of an [`Account`]; the
//! module docs of `db/schema.rs` say what it holds.

use crate::domain::define_id;
use crate::domain::entity::EntityId;
use serde::{Deserialize, Serialize};

define_id! {
    /// Identifies one [`Account`].
    AccountId
}

/// The five classes of account in double-entry bookkeeping.
///
/// Serialized in `snake_case` (`"asset"`, `"liability"`, ...), which is also
/// how the type is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    /// Something the entity owns: cash, a bank account, money owed to it.
    Asset,
    /// Something the entity owes: a loan, a credit card, an unpaid bill.
    Liability,
    /// The owner's stake: capital, retained earnings, opening balances.
    Equity,
    /// Money earned: salary, sales, interest.
    Income,
    /// Money spent: rent, food, fees.
    Expense,
}

impl AccountType {
    /// Returns the type as the UI and the vault write it: `asset`,
    /// `liability`, `equity`, `income` or `expense`.
    ///
    /// This is the text serde writes and the text stored in
    /// `accounts.account_type`, so it is part of the vault format: changing
    /// one of the five would make every existing vault unreadable.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::domain::AccountType;
    ///
    /// assert_eq!(AccountType::Liability.identifier(), "liability");
    /// ```
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Liability => "liability",
            Self::Equity => "equity",
            Self::Income => "income",
            Self::Expense => "expense",
        }
    }

    /// Returns `true` for the types whose balance a debit raises: assets and
    /// expenses.
    ///
    /// For those the balance is debits minus credits. For liabilities, equity
    /// and income it is credits minus debits.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::domain::AccountType;
    ///
    /// assert!(AccountType::Asset.is_debit_normal());
    /// assert!(AccountType::Expense.is_debit_normal());
    /// assert!(!AccountType::Income.is_debit_normal());
    /// ```
    #[must_use]
    pub const fn is_debit_normal(self) -> bool {
        matches!(self, Self::Asset | Self::Expense)
    }
}

/// One account in an entity's chart of accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// The account's own id.
    pub id: AccountId,
    /// The entity whose chart this account is in.
    pub entity_id: EntityId,
    /// The short code the user sees beside the name, such as `1000`. For a
    /// seeded account it is the template code that identifies its role.
    pub code: String,
    /// The name shown to the user; free text, never used to pick an account.
    pub name: String,
    /// The class of the account, which fixes its normal side.
    pub account_type: AccountType,
    /// `false` once the account is archived. An archived account keeps its
    /// entries and is refused for new ones, except the reversing entry that
    /// voids one of them.
    pub is_active: bool,
    /// `true` for an account the application relies on, such as Opening
    /// Balances; a system account cannot be archived.
    pub is_system: bool,
    /// Position in the chart, ascending; ties are ordered by code.
    pub sort_order: i32,
}

#[cfg(test)]
mod tests {
    use super::AccountType;

    #[test]
    fn the_identifier_of_an_account_type_is_the_text_serde_writes() {
        for account_type in [
            AccountType::Asset,
            AccountType::Liability,
            AccountType::Equity,
            AccountType::Income,
            AccountType::Expense,
        ] {
            assert_eq!(
                serde_json::to_value(account_type).unwrap(),
                serde_json::Value::from(account_type.identifier())
            );
        }
    }
}
