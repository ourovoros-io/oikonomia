//! Chart-of-accounts starter templates.

use crate::domain::{AccountType, ChartTemplate};

/// One account row from a template (before IDs are assigned).
#[derive(Debug, Clone)]
pub struct TemplateAccount {
    /// Account code.
    pub code: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Classification.
    pub account_type: AccountType,
    /// System-protected (e.g. Opening Balances).
    pub is_system: bool,
    /// Sort order.
    pub sort_order: i32,
}

/// Accounts seeded when creating an entity.
#[must_use]
pub fn template_accounts(template: ChartTemplate) -> Vec<TemplateAccount> {
    match template {
        ChartTemplate::Blank => Vec::new(),
        ChartTemplate::Personal => personal(),
        ChartTemplate::Company => company(),
    }
}

fn personal() -> Vec<TemplateAccount> {
    vec![
        ta("1000", "Cash", AccountType::Asset, false, 10),
        ta("1010", "Checking", AccountType::Asset, false, 20),
        ta("1020", "Savings", AccountType::Asset, false, 30),
        ta("1100", "Investments", AccountType::Asset, false, 40),
        ta("2000", "Credit Card", AccountType::Liability, false, 50),
        ta("2050", "Bills Payable", AccountType::Liability, false, 55),
        ta("2100", "Loans", AccountType::Liability, false, 60),
        ta("3000", "Opening Balances", AccountType::Equity, true, 70),
        ta("3100", "Owner Equity", AccountType::Equity, false, 80),
        ta("4000", "Salary", AccountType::Income, false, 90),
        ta("4100", "Freelance", AccountType::Income, false, 100),
        ta("4200", "Interest", AccountType::Income, false, 110),
        ta("4900", "Other Income", AccountType::Income, false, 120),
        ta("5000", "Housing", AccountType::Expense, false, 130),
        ta("5100", "Food", AccountType::Expense, false, 140),
        ta("5200", "Transport", AccountType::Expense, false, 150),
        ta("5300", "Utilities", AccountType::Expense, false, 160),
        ta("5350", "Bills & services", AccountType::Expense, false, 165),
        ta("5400", "Health", AccountType::Expense, false, 170),
        ta("5500", "Subscriptions", AccountType::Expense, false, 180),
        ta("5600", "Entertainment", AccountType::Expense, false, 190),
        ta("5700", "Taxes", AccountType::Expense, false, 200),
        ta("5900", "Other", AccountType::Expense, false, 210),
    ]
}

fn company() -> Vec<TemplateAccount> {
    vec![
        ta("1000", "Cash", AccountType::Asset, false, 10),
        ta("1010", "Bank", AccountType::Asset, false, 20),
        ta("1100", "Accounts Receivable", AccountType::Asset, false, 30),
        ta("1500", "Equipment", AccountType::Asset, false, 40),
        ta(
            "2000",
            "Accounts Payable",
            AccountType::Liability,
            false,
            50,
        ),
        ta("2100", "Credit Card", AccountType::Liability, false, 60),
        ta("2200", "Loans", AccountType::Liability, false, 70),
        ta("2300", "Taxes Payable", AccountType::Liability, false, 80),
        ta("3000", "Opening Balances", AccountType::Equity, true, 90),
        ta("3100", "Owner Capital", AccountType::Equity, false, 100),
        ta("3200", "Retained Earnings", AccountType::Equity, false, 110),
        ta("4000", "Sales / Services", AccountType::Income, false, 120),
        ta("4900", "Other Income", AccountType::Income, false, 130),
        ta("5000", "COGS", AccountType::Expense, false, 140),
        ta("5100", "Payroll", AccountType::Expense, false, 150),
        ta("5200", "Rent", AccountType::Expense, false, 160),
        ta("5300", "Software", AccountType::Expense, false, 170),
        ta("5400", "Marketing", AccountType::Expense, false, 180),
        ta(
            "5500",
            "Professional Fees",
            AccountType::Expense,
            false,
            190,
        ),
        ta("5600", "Travel", AccountType::Expense, false, 200),
        ta("5700", "Taxes", AccountType::Expense, false, 210),
        ta("5900", "Other OpEx", AccountType::Expense, false, 220),
    ]
}

fn ta(
    code: &'static str,
    name: &'static str,
    account_type: AccountType,
    is_system: bool,
    sort_order: i32,
) -> TemplateAccount {
    TemplateAccount {
        code,
        name,
        account_type,
        is_system,
        sort_order,
    }
}
