//! Chart-of-accounts starter templates.

use crate::domain::{AccountType, ChartTemplate};
use crate::error::AccountRole;
use crate::prefs::Locale;
use crate::text::seeded_account_name;

/// One account row from a template (before IDs are assigned).
#[derive(Debug, Clone)]
pub struct TemplateAccount {
    /// Account code.
    pub code: &'static str,
    /// Display name, in the language the template was seeded in.
    pub name: &'static str,
    /// Classification.
    pub account_type: AccountType,
    /// System-protected (e.g. Opening Balances).
    pub is_system: bool,
    /// Sort order.
    pub sort_order: i32,
}

/// The language-independent shape of one seeded account.
///
/// The name is not here: it comes from the [`crate::text`] table by code, so
/// a translation can never change a code, a type, a flag or the order.
struct AccountShape {
    code: &'static str,
    account_type: AccountType,
    is_system: bool,
    sort_order: i32,
}

impl AccountShape {
    const fn new(
        code: &'static str,
        account_type: AccountType,
        is_system: bool,
        sort_order: i32,
    ) -> Self {
        Self {
            code,
            account_type,
            is_system,
            sort_order,
        }
    }
}

/// Accounts seeded when creating an entity, named in `locale`.
///
/// Codes, types, flags and order are the same in every language; only the
/// names differ. The names are written into the book once, at creation.
#[must_use]
pub fn template_accounts(template: ChartTemplate, locale: Locale) -> Vec<TemplateAccount> {
    let shapes = match template {
        ChartTemplate::Blank => Vec::new(),
        ChartTemplate::Personal => personal_template_shapes(),
        ChartTemplate::Company => company_template_shapes(),
    };

    shapes
        .into_iter()
        .map(|shape| TemplateAccount {
            code: shape.code,
            // A shape without a table entry is a bug a test catches (every
            // seeded code must be named in every language); the code is a
            // visible, harmless stand-in rather than an empty name.
            name: seeded_account_name(template, shape.code, locale).unwrap_or(shape.code),
            account_type: shape.account_type,
            is_system: shape.is_system,
            sort_order: shape.sort_order,
        })
        .collect()
}

/// Household chart: cash and cards, owner equity, salary, and living expenses.
fn personal_template_shapes() -> Vec<AccountShape> {
    vec![
        AccountShape::new("1000", AccountType::Asset, false, 10),
        AccountShape::new("1010", AccountType::Asset, false, 20),
        AccountShape::new("1020", AccountType::Asset, false, 30),
        AccountShape::new("1100", AccountType::Asset, false, 40),
        AccountShape::new("2000", AccountType::Liability, false, 50),
        AccountShape::new("2050", AccountType::Liability, false, 55),
        AccountShape::new("2100", AccountType::Liability, false, 60),
        // Opening Balances is system-protected so opening-balance posting has a
        // stable contra account that users cannot archive.
        AccountShape::new("3000", AccountType::Equity, true, 70),
        AccountShape::new("3100", AccountType::Equity, false, 80),
        AccountShape::new("4000", AccountType::Income, false, 90),
        AccountShape::new("4100", AccountType::Income, false, 100),
        AccountShape::new("4200", AccountType::Income, false, 110),
        AccountShape::new("4900", AccountType::Income, false, 120),
        AccountShape::new("5000", AccountType::Expense, false, 130),
        AccountShape::new("5100", AccountType::Expense, false, 140),
        AccountShape::new("5200", AccountType::Expense, false, 150),
        AccountShape::new("5300", AccountType::Expense, false, 160),
        AccountShape::new("5350", AccountType::Expense, false, 165),
        AccountShape::new("5400", AccountType::Expense, false, 170),
        AccountShape::new("5500", AccountType::Expense, false, 180),
        AccountShape::new("5600", AccountType::Expense, false, 190),
        AccountShape::new("5700", AccountType::Expense, false, 200),
        AccountShape::new("5900", AccountType::Expense, false, 210),
    ]
}

/// Company chart: AR/AP, capital, retained earnings, sales, and operating expenses.
fn company_template_shapes() -> Vec<AccountShape> {
    vec![
        AccountShape::new("1000", AccountType::Asset, false, 10),
        AccountShape::new("1010", AccountType::Asset, false, 20),
        AccountShape::new("1100", AccountType::Asset, false, 30),
        AccountShape::new("1500", AccountType::Asset, false, 40),
        AccountShape::new("2000", AccountType::Liability, false, 50),
        AccountShape::new("2100", AccountType::Liability, false, 60),
        AccountShape::new("2200", AccountType::Liability, false, 70),
        AccountShape::new("2300", AccountType::Liability, false, 80),
        AccountShape::new("3000", AccountType::Equity, true, 90),
        AccountShape::new("3100", AccountType::Equity, false, 100),
        AccountShape::new("3200", AccountType::Equity, false, 110),
        AccountShape::new("4000", AccountType::Income, false, 120),
        AccountShape::new("4900", AccountType::Income, false, 130),
        AccountShape::new("5000", AccountType::Expense, false, 140),
        AccountShape::new("5100", AccountType::Expense, false, 150),
        AccountShape::new("5200", AccountType::Expense, false, 160),
        AccountShape::new("5300", AccountType::Expense, false, 170),
        AccountShape::new("5400", AccountType::Expense, false, 180),
        AccountShape::new("5500", AccountType::Expense, false, 190),
        AccountShape::new("5600", AccountType::Expense, false, 200),
        AccountShape::new("5700", AccountType::Expense, false, 210),
        AccountShape::new("5900", AccountType::Expense, false, 220),
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
        let seeded = template_accounts(template, Locale::En);
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

    #[test]
    fn every_seeded_code_is_named_and_every_named_code_is_seeded() {
        for template in [ChartTemplate::Personal, ChartTemplate::Company] {
            let seeded: Vec<&str> = template_accounts(template, Locale::En)
                .iter()
                .map(|account| account.code)
                .collect();

            assert_eq!(crate::text::seeded_account_codes(template), seeded);
        }
    }

    #[test]
    fn no_seeded_name_falls_back_to_its_code() {
        for template in [ChartTemplate::Personal, ChartTemplate::Company] {
            for locale in [Locale::En, Locale::El, Locale::Fr, Locale::De] {
                for account in template_accounts(template, locale) {
                    assert_ne!(account.name, account.code, "{template:?} {locale:?}");
                }
            }
        }
    }
}
