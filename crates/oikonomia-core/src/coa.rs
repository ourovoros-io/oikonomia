//! The starter charts of accounts, and which seeded account plays which part.
//!
//! "coa" is the chart of accounts. Creating an entity from a
//! [`ChartTemplate`] seeds a fixed list of accounts, and this module is that
//! list plus two maps over it.
//!
//! # What an account is made of
//!
//! A seeded account has a language-independent shape (code, type, system
//! flag, sort order) and a name. The shape is defined here; the name comes
//! from the table in [`crate::text`], looked up by code in the language the
//! book is created in. Keeping them apart means a translation can change a
//! name and nothing else, and [`template_accounts`] joins the two.
//!
//! # Identity is the code
//!
//! Once a book exists its account names are the user's: they can be renamed,
//! and a chart created in Greek never had the English names. So nothing here
//! or anywhere else in the crate finds an account by name. A seeded account
//! is recognised by its template code and its type, and the two maps are
//! keyed that way:
//!
//! - [`default_role_codes`] says which seeded accounts are the default for
//!   each [`AccountRole`] of a simple entry (the wallet, the category, the
//!   payable), best first. [`crate::default_accounts`] applies it to a real
//!   book and falls back to the account type when the codes are gone.
//! - [`document_topic_codes`] says which seeded accounts cover a
//!   [`DocumentTopic`] the document reader recognised.
//!
//! # Kept true by tests
//!
//! The tables are hand-written, so the tests at the bottom check what the
//! compiler cannot: every code a map names exists in its template with the
//! expected type, every seeded code has a name in every language, and the
//! name table has no code the template does not seed.

use crate::domain::{AccountType, ChartTemplate};
use crate::error::AccountRole;
use crate::prefs::Locale;
use crate::text::seeded_account_name;

/// One account a template seeds, before it is given an id and stored.
#[derive(Debug, Clone)]
pub struct TemplateAccount {
    /// The account code, which identifies the account within its template.
    pub code: &'static str,
    /// The name in the language the accounts were requested in.
    pub name: &'static str,
    /// The class of the account.
    pub account_type: AccountType,
    /// `true` for an account the application relies on, which cannot be
    /// archived (Opening Balances).
    pub is_system: bool,
    /// Position in the chart, ascending.
    pub sort_order: i32,
}

/// Returns the accounts `template` seeds, named in `locale`, in chart order.
///
/// Codes, types, flags and order are the same in every language; only the
/// names differ. A blank template seeds nothing. The caller writes the names
/// into the book at creation, and they are not translated again when the app
/// language changes.
///
/// # Examples
///
/// ```
/// use oikonomia_core::coa::template_accounts;
/// use oikonomia_core::domain::ChartTemplate;
/// use oikonomia_core::prefs::Locale;
///
/// let english = template_accounts(ChartTemplate::Personal, Locale::En);
/// let german = template_accounts(ChartTemplate::Personal, Locale::De);
///
/// assert_eq!((english[0].code, english[0].name), ("1000", "Cash"));
/// assert_eq!((german[0].code, german[0].name), ("1000", "Bargeld"));
/// assert!(template_accounts(ChartTemplate::Blank, Locale::En).is_empty());
/// ```
///
/// # Panics
///
/// In a build with debug assertions, panics if a seeded code has no name in
/// [`crate::text`]. The test
/// `every_seeded_code_is_named_and_every_named_code_is_seeded` keeps the two
/// tables in step, so this is a guard against a table edit, not a condition a
/// caller can cause. A release build uses the code as the name instead.
#[must_use]
pub fn template_accounts(template: ChartTemplate, locale: Locale) -> Vec<TemplateAccount> {
    let shapes = match template {
        ChartTemplate::Blank => Vec::new(),
        ChartTemplate::Personal => personal_template_shapes(),
        ChartTemplate::Company => company_template_shapes(),
    };

    shapes
        .into_iter()
        .map(|shape| {
            let name = seeded_account_name(template, shape.code, locale);

            // A name written into a book is never rewritten, so a missing
            // table entry must be caught before release rather than papered
            // over. The release fallback is the code: visible and harmless,
            // where an empty name would not be.
            debug_assert!(
                name.is_some(),
                "{template:?} account {} has no {locale:?} name in text.rs",
                shape.code,
            );

            TemplateAccount {
                code: shape.code,
                name: name.unwrap_or(shape.code),
                account_type: shape.account_type,
                is_system: shape.is_system,
                sort_order: shape.sort_order,
            }
        })
        .collect()
}

/// The language-independent part of one seeded account.
///
/// The name is not here: it comes from the [`crate::text`] table by code, so
/// a translation can never change a code, a type, a flag or the order.
struct AccountShape {
    /// The account code, unique within its template.
    code: &'static str,
    /// The class of the account.
    account_type: AccountType,
    /// Whether the account is protected from archiving.
    is_system: bool,
    /// Position in the chart, ascending.
    sort_order: i32,
}

impl AccountShape {
    /// Returns a shape with the given code, type, system flag and position.
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
/// Returns the shapes of the personal chart, in chart order: cash and cards,
/// owner equity, salary, and living expenses.
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

/// Returns the shapes of the company chart, in chart order: receivables and
/// payables, capital, retained earnings, sales, and operating expenses.
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

/// Returns the account type of the default account for `role`.
///
/// [`crate::default_accounts`] only ever proposes an active account of this
/// type for the role, whether it found it by template code or by type.
///
/// This is not the full set of types the role accepts. A payment, a transfer
/// source and a transfer destination default to an asset but may also be
/// filled with a liability, such as a credit card. The allowed types are
/// enforced where a simple entry is posted, in
/// [`crate::ledger::post_simple_entry`].
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

/// Returns the template account codes that play `role` by default, best
/// first.
///
/// This is the single place that says which seeded account is "the" wallet,
/// "the" payable and so on. It is keyed on the template code, which a rename
/// or a translation of the seeded names cannot change, and every code listed
/// here exists in that template with the type [`role_account_type`] gives (a
/// test enforces it). A blank chart seeds nothing, so its list is empty and
/// callers fall back to the account type.
///
/// # Examples
///
/// ```
/// use oikonomia_core::coa::default_role_codes;
/// use oikonomia_core::domain::ChartTemplate;
/// use oikonomia_core::error::AccountRole;
///
/// // Checking first, then Cash.
/// assert_eq!(
///     default_role_codes(ChartTemplate::Personal, AccountRole::Payment),
///     ["1010", "1000"]
/// );
/// assert!(default_role_codes(ChartTemplate::Blank, AccountRole::Payment).is_empty());
/// ```
#[must_use]
pub fn default_role_codes(template: ChartTemplate, role: AccountRole) -> &'static [&'static str] {
    match template {
        ChartTemplate::Blank => &[],
        ChartTemplate::Personal => personal_role_codes(role),
        ChartTemplate::Company => company_role_codes(role),
    }
}

/// Returns the personal chart's default codes for `role`, best first.
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

/// Returns the company chart's default codes for `role`, best first.
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
/// The document reader in [`crate::documents`] matches words in a document's
/// text to a topic, and [`document_topic_codes`] maps the topic to the seeded
/// accounts that cover it. The first ten topics are expenses and the last
/// four are income.
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

impl DocumentTopic {
    /// Every topic, in declaration order.
    ///
    /// A test checks the list against the enum, so a new topic cannot be left
    /// out of it.
    pub const ALL: &'static [Self] = &[
        Self::Utilities,
        Self::Bills,
        Self::Housing,
        Self::Subscription,
        Self::Food,
        Self::Transport,
        Self::Software,
        Self::Health,
        Self::Tax,
        Self::OtherExpense,
        Self::Sales,
        Self::Freelance,
        Self::Salary,
        Self::OtherIncome,
    ];
}

/// Returns the template account codes that cover `topic`, best first.
///
/// An empty list means the template seeds no account for the topic (a blank
/// template seeds none at all), so the reader moves on to the next topic it
/// recognised.
///
/// # Examples
///
/// ```
/// use oikonomia_core::coa::{DocumentTopic, document_topic_codes};
/// use oikonomia_core::domain::ChartTemplate;
///
/// assert_eq!(
///     document_topic_codes(ChartTemplate::Personal, DocumentTopic::Food),
///     ["5100"]
/// );
/// // The company chart has no food account.
/// assert!(document_topic_codes(ChartTemplate::Company, DocumentTopic::Food).is_empty());
/// ```
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

/// Returns the personal chart's codes for `topic`, best first.
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

/// Returns the company chart's codes for `topic`, best first.
fn company_topic_codes(topic: DocumentTopic) -> &'static [&'static str] {
    match topic {
        DocumentTopic::Utilities
        | DocumentTopic::Bills
        | DocumentTopic::Subscription
        | DocumentTopic::Food
        | DocumentTopic::Health
        | DocumentTopic::Freelance
        | DocumentTopic::Salary => &[],
        // Rent.
        DocumentTopic::Housing => &["5200"],
        // Travel.
        DocumentTopic::Transport => &["5600"],
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
    use oikonomia_test_support::listed_variants;

    /// Every chart template.
    const TEMPLATES: [ChartTemplate; 3] = [
        ChartTemplate::Blank,
        ChartTemplate::Personal,
        ChartTemplate::Company,
    ];

    listed_variants! {
        units listed_topics for DocumentTopic {
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
        }
    }

    /// Fails unless `DocumentTopic::ALL` is the variants in the
    /// `listed_topics` list above, in that order. The compiler checks that
    /// list against the enum with an exhaustive `match`, so a topic added to
    /// the enum but left out of the list does not compile.
    #[test]
    fn all_lists_every_topic_once_in_declaration_order() {
        assert_eq!(DocumentTopic::ALL, listed_topics::variants());
        listed_topics::assert_every_position_once(
            DocumentTopic::ALL
                .iter()
                .map(listed_topics::position)
                .collect(),
        );
    }

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

    /// Fails unless `template` seeds an account `code` of type `expected`;
    /// `what` names the role or topic being checked in the failure message.
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
            for &topic in DocumentTopic::ALL {
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
            for &locale in Locale::ALL {
                for account in template_accounts(template, locale) {
                    assert_ne!(account.name, account.code, "{template:?} {locale:?}");
                }
            }
        }
    }
}
