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

impl TemplateAccount {
    #[must_use]
    const fn new(
        code: &'static str,
        name: &'static str,
        account_type: AccountType,
        is_system: bool,
        sort_order: i32,
    ) -> Self {
        Self {
            code,
            name,
            account_type,
            is_system,
            sort_order,
        }
    }
}

/// Accounts seeded when creating an entity.
#[must_use]
pub fn template_accounts(template: ChartTemplate) -> Vec<TemplateAccount> {
    match template {
        ChartTemplate::Blank => Vec::new(),
        ChartTemplate::Personal => personal_template_accounts(),
        ChartTemplate::Company => company_template_accounts(),
    }
}

/// Household chart: cash and cards, owner equity, salary, and living expenses.
fn personal_template_accounts() -> Vec<TemplateAccount> {
    vec![
        TemplateAccount::new("1000", "Cash", AccountType::Asset, false, 10),
        TemplateAccount::new("1010", "Checking", AccountType::Asset, false, 20),
        TemplateAccount::new("1020", "Savings", AccountType::Asset, false, 30),
        TemplateAccount::new("1100", "Investments", AccountType::Asset, false, 40),
        TemplateAccount::new("2000", "Credit Card", AccountType::Liability, false, 50),
        TemplateAccount::new("2050", "Bills Payable", AccountType::Liability, false, 55),
        TemplateAccount::new("2100", "Loans", AccountType::Liability, false, 60),
        // Opening Balances is system-protected so opening-balance posting has a
        // stable contra account that users cannot archive.
        TemplateAccount::new("3000", "Opening Balances", AccountType::Equity, true, 70),
        TemplateAccount::new("3100", "Owner Equity", AccountType::Equity, false, 80),
        TemplateAccount::new("4000", "Salary", AccountType::Income, false, 90),
        TemplateAccount::new("4100", "Freelance", AccountType::Income, false, 100),
        TemplateAccount::new("4200", "Interest", AccountType::Income, false, 110),
        TemplateAccount::new("4900", "Other Income", AccountType::Income, false, 120),
        TemplateAccount::new("5000", "Housing", AccountType::Expense, false, 130),
        TemplateAccount::new("5100", "Food", AccountType::Expense, false, 140),
        TemplateAccount::new("5200", "Transport", AccountType::Expense, false, 150),
        TemplateAccount::new("5300", "Utilities", AccountType::Expense, false, 160),
        TemplateAccount::new("5350", "Bills & services", AccountType::Expense, false, 165),
        TemplateAccount::new("5400", "Health", AccountType::Expense, false, 170),
        TemplateAccount::new("5500", "Subscriptions", AccountType::Expense, false, 180),
        TemplateAccount::new("5600", "Entertainment", AccountType::Expense, false, 190),
        TemplateAccount::new("5700", "Taxes", AccountType::Expense, false, 200),
        TemplateAccount::new("5900", "Other", AccountType::Expense, false, 210),
    ]
}

/// Company chart: AR/AP, capital, retained earnings, sales, and operating expenses.
fn company_template_accounts() -> Vec<TemplateAccount> {
    vec![
        TemplateAccount::new("1000", "Cash", AccountType::Asset, false, 10),
        TemplateAccount::new("1010", "Bank", AccountType::Asset, false, 20),
        TemplateAccount::new("1100", "Accounts Receivable", AccountType::Asset, false, 30),
        TemplateAccount::new("1500", "Equipment", AccountType::Asset, false, 40),
        TemplateAccount::new(
            "2000",
            "Accounts Payable",
            AccountType::Liability,
            false,
            50,
        ),
        TemplateAccount::new("2100", "Credit Card", AccountType::Liability, false, 60),
        TemplateAccount::new("2200", "Loans", AccountType::Liability, false, 70),
        TemplateAccount::new("2300", "Taxes Payable", AccountType::Liability, false, 80),
        TemplateAccount::new("3000", "Opening Balances", AccountType::Equity, true, 90),
        TemplateAccount::new("3100", "Owner Capital", AccountType::Equity, false, 100),
        TemplateAccount::new("3200", "Retained Earnings", AccountType::Equity, false, 110),
        TemplateAccount::new("4000", "Sales / Services", AccountType::Income, false, 120),
        TemplateAccount::new("4900", "Other Income", AccountType::Income, false, 130),
        TemplateAccount::new("5000", "COGS", AccountType::Expense, false, 140),
        TemplateAccount::new("5100", "Payroll", AccountType::Expense, false, 150),
        TemplateAccount::new("5200", "Rent", AccountType::Expense, false, 160),
        TemplateAccount::new("5300", "Software", AccountType::Expense, false, 170),
        TemplateAccount::new("5400", "Marketing", AccountType::Expense, false, 180),
        TemplateAccount::new(
            "5500",
            "Professional Fees",
            AccountType::Expense,
            false,
            190,
        ),
        TemplateAccount::new("5600", "Travel", AccountType::Expense, false, 200),
        TemplateAccount::new("5700", "Taxes", AccountType::Expense, false, 210),
        TemplateAccount::new("5900", "Other OpEx", AccountType::Expense, false, 220),
    ]
}
