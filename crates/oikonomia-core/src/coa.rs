//! Chart-of-accounts starter templates.

use crate::domain::{AccountType, ChartTemplate};
use crate::error::AccountRole;

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

/// The account type a role must be filled with.
///
/// The default for a role is always an active account of this type, whether it
/// came from a template code or from the by-type fallback.
#[must_use]
pub const fn role_account_type(role: AccountRole) -> AccountType {
    match role {
        AccountRole::Category | AccountRole::BillCategory => AccountType::Expense,
        AccountRole::Income => AccountType::Income,
        AccountRole::Payment
        | AccountRole::Deposit
        | AccountRole::TransferSource
        | AccountRole::TransferDestination => AccountType::Asset,
        AccountRole::BillsPayable => AccountType::Liability,
    }
}

/// Template account codes that play `role` by default, best first.
///
/// This is the single place that says which seeded account is "the" wallet,
/// "the" payable and so on. It is keyed on the template code, which a rename
/// or a translation of the seeded names cannot change, and every code listed
/// here must exist in that template (a test enforces it). A blank chart seeds
/// nothing, so it names nothing and relies on the by-type fallback.
#[must_use]
pub fn default_role_codes(template: ChartTemplate, role: AccountRole) -> &'static [&'static str] {
    match template {
        ChartTemplate::Blank => &[],
        ChartTemplate::Personal => personal_role_codes(role),
        ChartTemplate::Company => company_role_codes(role),
    }
}

fn personal_role_codes(role: AccountRole) -> &'static [&'static str] {
    match role {
        // Food, then Utilities, Bills & services, Other.
        AccountRole::Category => &["5100", "5300", "5350", "5900"],
        // Checking, then Cash.
        AccountRole::Payment | AccountRole::Deposit => &["1010", "1000"],
        // Salary, then Freelance.
        AccountRole::Income => &["4000", "4100"],
        // Utilities, Bills & services, Housing, Subscriptions.
        AccountRole::BillCategory => &["5300", "5350", "5000", "5500"],
        AccountRole::BillsPayable => &["2050"],
        AccountRole::TransferSource => &["1010"],
        // Savings, then Cash.
        AccountRole::TransferDestination => &["1020", "1000"],
    }
}

fn company_role_codes(role: AccountRole) -> &'static [&'static str] {
    match role {
        // Other OpEx.
        AccountRole::Category => &["5900"],
        // Bank, then Cash.
        AccountRole::Payment | AccountRole::Deposit => &["1010", "1000"],
        // Sales / Services.
        AccountRole::Income => &["4000"],
        // Rent.
        AccountRole::BillCategory => &["5200"],
        // Accounts Payable.
        AccountRole::BillsPayable => &["2000"],
        AccountRole::TransferSource => &["1010"],
        AccountRole::TransferDestination => &["1000"],
    }
}

/// What a scanned document is about, as far as choosing a category goes.
///
/// The document reader matches words in the merchant text to a topic; this
/// maps the topic to the seeded accounts that cover it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentTopic {
    /// Electricity, water, gas.
    Utilities,
    /// Invoices and bills with no better category.
    Bills,
    /// Rent and mortgage.
    Housing,
    /// Streaming and software subscriptions.
    Subscription,
    /// Groceries and restaurants.
    Food,
    /// Fuel, taxis, parking.
    Transport,
    /// Software and cloud services.
    Software,
    /// Pharmacy and doctors.
    Health,
    /// Taxes and VAT.
    Tax,
    /// Expense catch-all when no topic fits.
    OtherExpense,
    /// Sales and services income.
    Sales,
    /// Freelance and project income.
    Freelance,
    /// Salary income.
    Salary,
    /// Income catch-all when no topic fits.
    OtherIncome,
}

/// Template account codes that cover a document topic, best first.
///
/// An empty list means the template has no account for the topic, so the
/// reader moves on to the next topic it recognised.
#[must_use]
pub fn document_topic_codes(
    template: ChartTemplate,
    topic: DocumentTopic,
) -> &'static [&'static str] {
    match template {
        ChartTemplate::Blank => &[],
        ChartTemplate::Personal => personal_topic_codes(topic),
        ChartTemplate::Company => company_topic_codes(topic),
    }
}

fn personal_topic_codes(topic: DocumentTopic) -> &'static [&'static str] {
    match topic {
        DocumentTopic::Utilities => &["5300"],
        DocumentTopic::Bills => &["5350"],
        DocumentTopic::Housing => &["5000"],
        DocumentTopic::Subscription => &["5500"],
        DocumentTopic::Food => &["5100"],
        DocumentTopic::Transport => &["5200"],
        DocumentTopic::Software => &[],
        DocumentTopic::Health => &["5400"],
        DocumentTopic::Tax => &["5700"],
        DocumentTopic::OtherExpense => &["5900"],
        DocumentTopic::Freelance => &["4100"],
        DocumentTopic::Salary => &["4000"],
        // Personal has no sales account; Other Income is the nearest.
        DocumentTopic::Sales | DocumentTopic::OtherIncome => &["4900"],
    }
}

fn company_topic_codes(topic: DocumentTopic) -> &'static [&'static str] {
    match topic {
        DocumentTopic::Utilities
        | DocumentTopic::Bills
        | DocumentTopic::Housing
        | DocumentTopic::Subscription
        | DocumentTopic::Food
        | DocumentTopic::Transport
        | DocumentTopic::Health
        | DocumentTopic::Freelance
        | DocumentTopic::Salary => &[],
        DocumentTopic::Software => &["5300"],
        DocumentTopic::Tax => &["5700"],
        DocumentTopic::OtherExpense => &["5900"],
        DocumentTopic::Sales => &["4000", "4900"],
        DocumentTopic::OtherIncome => &["4900"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATES: [ChartTemplate; 3] = [
        ChartTemplate::Blank,
        ChartTemplate::Personal,
        ChartTemplate::Company,
    ];

    const TOPICS: [DocumentTopic; 14] = [
        DocumentTopic::Utilities,
        DocumentTopic::Bills,
        DocumentTopic::Housing,
        DocumentTopic::Subscription,
        DocumentTopic::Food,
        DocumentTopic::Transport,
        DocumentTopic::Software,
        DocumentTopic::Health,
        DocumentTopic::Tax,
        DocumentTopic::OtherExpense,
        DocumentTopic::Sales,
        DocumentTopic::Freelance,
        DocumentTopic::Salary,
        DocumentTopic::OtherIncome,
    ];

    /// The type a topic's accounts must have.
    fn topic_type(topic: DocumentTopic) -> AccountType {
        match topic {
            DocumentTopic::Sales
            | DocumentTopic::Freelance
            | DocumentTopic::Salary
            | DocumentTopic::OtherIncome => AccountType::Income,
            DocumentTopic::Utilities
            | DocumentTopic::Bills
            | DocumentTopic::Housing
            | DocumentTopic::Subscription
            | DocumentTopic::Food
            | DocumentTopic::Transport
            | DocumentTopic::Software
            | DocumentTopic::Health
            | DocumentTopic::Tax
            | DocumentTopic::OtherExpense => AccountType::Expense,
        }
    }

    fn assert_code_has_type(
        template: ChartTemplate,
        code: &str,
        expected: AccountType,
        what: &str,
    ) {
        let seeded = template_accounts(template);
        let found = seeded.iter().find(|account| account.code == code);

        assert_eq!(
            found.map(|account| account.account_type),
            Some(expected),
            "{what}: code {code} must exist in {template:?} with the role's type",
        );
    }

    #[test]
    fn every_role_code_exists_in_its_template_with_the_roles_type() {
        for template in TEMPLATES {
            for role in AccountRole::ALL {
                for code in default_role_codes(template, *role) {
                    assert_code_has_type(
                        template,
                        code,
                        role_account_type(*role),
                        &role.to_string(),
                    );
                }
            }
        }
    }

    #[test]
    fn every_topic_code_exists_in_its_template_with_the_topics_type() {
        for template in TEMPLATES {
            for topic in TOPICS {
                for code in document_topic_codes(template, topic) {
                    assert_code_has_type(template, code, topic_type(topic), &format!("{topic:?}"));
                }
            }
        }
    }

    #[test]
    fn the_category_and_bill_category_roles_pin_their_fallback_order() {
        // Personal category: Food, Utilities, Bills & services, Other.
        assert_eq!(
            default_role_codes(ChartTemplate::Personal, AccountRole::Category),
            &["5100", "5300", "5350", "5900"],
        );
        // Personal bill category: Utilities, Bills & services, Housing,
        // Subscriptions.
        assert_eq!(
            default_role_codes(ChartTemplate::Personal, AccountRole::BillCategory),
            &["5300", "5350", "5000", "5500"],
        );
        // The company chart has a single choice for each.
        assert_eq!(
            default_role_codes(ChartTemplate::Company, AccountRole::Category),
            &["5900"],
        );
        assert_eq!(
            default_role_codes(ChartTemplate::Company, AccountRole::BillCategory),
            &["5200"],
        );
    }

    #[test]
    fn a_blank_template_names_no_codes() {
        for role in AccountRole::ALL {
            assert_eq!(
                default_role_codes(ChartTemplate::Blank, *role),
                &[] as &[&str]
            );
        }
    }

    #[test]
    fn seeded_templates_name_a_code_for_every_role() {
        for template in [ChartTemplate::Personal, ChartTemplate::Company] {
            for role in AccountRole::ALL {
                assert_ne!(
                    default_role_codes(template, *role),
                    &[] as &[&str],
                    "{template:?} has no code for {role}",
                );
            }
        }
    }
}
