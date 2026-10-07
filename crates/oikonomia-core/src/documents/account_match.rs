//! The keyword matcher that picks the account a document is filed under.
//!
//! [`match_expense_account`] and [`match_income_account`] are the two entry
//! points. Their input is a hint: the merchant and description the invoice
//! reader produced, worded in English whatever the language of the
//! application.
//!
//! 1. The hint is lowercased and tested against a keyword table
//!    ([`EXPENSE_KEYWORDS`] or [`INCOME_KEYWORDS`]), topic by topic, in table
//!    order. A keyword matches on word boundaries only ([`Keyword`]).
//! 2. A topic that matches is turned into chart codes by the book's template
//!    ([`document_topic_codes`]). The first active account of the right type
//!    that carries one of those codes is the answer.
//! 3. A topic the chart has no account for is skipped, and the next topic
//!    that matches is tried.
//! 4. When no topic is left, the template's catch-all topic is used, and
//!    after that the first active account of the type.
//!
//! Accounts are found by code and type, never by name, so a renamed or
//! translated account is matched the same. A blank book has no template
//! codes, so it always gets the first active account of the type.
//!
//! Table order decides between topics whenever a hint names two of them.
//! [`EXPENSE_KEYWORDS`] lists the constraints on that order.

use crate::coa::{DocumentTopic, document_topic_codes};
use crate::default_accounts::{account_by_codes, first_of_type};
use crate::documents::keyword::Keyword;
use crate::documents::keyword::Keyword::{Prefix, Unit, Word};
use crate::domain::{Account, AccountId, AccountType, ChartTemplate};

/// Picks the expense account a document most likely belongs to.
///
/// The merchant and description hints choose a topic; the topic is mapped to a
/// seeded account by template code, so the account's name never matters. With
/// no recognised topic the template's catch-all is used, then the first active
/// expense account.
#[must_use]
pub(super) fn match_expense_account(
    template: ChartTemplate,
    accounts: &[Account],
    hints: &str,
) -> Option<AccountId> {
    match_account_of_type(template, accounts, hints, &EXPENSE_TOPICS)
}

/// Picks the income account a document most likely belongs to (sales, freelance,
/// salary), by the same rule as [`match_expense_account`].
#[must_use]
pub(super) fn match_income_account(
    template: ChartTemplate,
    accounts: &[Account],
    hints: &str,
) -> Option<AccountId> {
    match_account_of_type(template, accounts, hints, &INCOME_TOPICS)
}

/// A keyword table: topics in the order they are tried, each with the words
/// that point at it.
type TopicKeywords = [(DocumentTopic, &'static [Keyword])];

/// Expense topics in the order they are tried; the first topic whose keywords
/// occur in the hint, and that the chart has an account for, wins.
///
/// The order is: Utilities, Transport, Housing, Subscription, Food, Software,
/// Health, Bills, Tax. Three constraints fix it:
///
/// - Transport before Housing: a car rental, car hire or "rent a car" is
///   travel, but it contains "rental" or "rent", which are Housing words.
/// - Bills after every specific topic except Tax: Bills is the generic topic
///   for a bill, invoice or receipt that names nothing more specific, so a
///   "doctor bill", "clinic invoice", "taxi receipt" or "rent invoice" goes to
///   the topic it is about.
/// - Bills before Tax: "Tax invoice 42" and "VAT invoice" are ordinary
///   invoices that mention tax, not tax payments.
///
/// A [`Keyword::Prefix`] is used for a stem that real names build on
/// (Cloudflare, healthcare, Foodpanda, fuels); a plural made redundant by a
/// prefix is not listed. Compounds that hide the stem mid-word (iCloud, efood,
/// seafood, polyclinic, refuel) are listed as whole words.
const EXPENSE_KEYWORDS: &TopicKeywords = &[
    (
        DocumentTopic::Utilities,
        &[
            Prefix("utilit"),
            Prefix("electric"),
            Word("water"),
            Word("gas"),
            Word("power"),
            Word("dei"),
            Prefix("ρεύμα"),
            Unit("kwh"),
            Word("zenith"),
            Word("zeniθ"),
            Prefix("εκκαθαριστ"),
            Prefix("ηλεκτρ"),
            Prefix("αέριο"),
            Prefix("αεριο"),
            Word("ngs"),
            Word("φ.α"),
            Prefix("έναντι"),
        ],
    ),
    (
        DocumentTopic::Transport,
        &[
            Prefix("fuel"),
            Word("refuel"),
            Word("uber"),
            // A word, not a prefix: "taxidermy" is not travel.
            Word("taxi"),
            Word("taxis"),
            Word("taxibeat"),
            Prefix("transport"),
            Word("parking"),
            Word("car rental"),
            Word("car rentals"),
            Word("car hire"),
            Word("rent a car"),
        ],
    ),
    (
        DocumentTopic::Housing,
        &[
            Word("rent"),
            Word("rents"),
            Word("renting"),
            Word("rented"),
            Word("rental"),
            Word("rentals"),
            Word("mortgage"),
            Word("mortgages"),
            Word("housing"),
        ],
    ),
    (
        DocumentTopic::Subscription,
        &[
            Word("netflix"),
            Word("spotify"),
            Word("subscription"),
            Word("subscriptions"),
            Word("saas"),
        ],
    ),
    (
        DocumentTopic::Food,
        &[
            Prefix("food"),
            Word("efood"),
            Word("seafood"),
            Word("grocery"),
            Word("groceries"),
            Word("supermarket"),
            Word("supermarkets"),
            Word("restaurant"),
            Word("restaurants"),
        ],
    ),
    (
        DocumentTopic::Software,
        &[
            Word("software"),
            Word("github"),
            Word("aws"),
            Prefix("cloud"),
            Word("icloud"),
            Word("security"),
            Prefix("program"),
        ],
    ),
    (
        DocumentTopic::Health,
        &[
            Prefix("pharma"),
            Word("doctor"),
            Word("doctors"),
            Prefix("health"),
            Prefix("clinic"),
            Word("polyclinic"),
        ],
    ),
    (
        DocumentTopic::Bills,
        &[
            Word("bill"),
            Word("bills"),
            Word("billing"),
            Word("billed"),
            Word("invoice"),
            Word("invoices"),
            Word("invoiced"),
            Word("invoicing"),
            Word("receipt"),
            Word("receipts"),
        ],
    ),
    (
        DocumentTopic::Tax,
        &[
            Word("tax"),
            Word("taxes"),
            Word("taxation"),
            Word("vat"),
            Word("irs"),
        ],
    ),
];

/// Income topics in the order they are tried, by the rule of
/// [`EXPENSE_KEYWORDS`]: Sales, Freelance, Salary.
///
/// No hint in the tests names two income topics, so no test depends on this
/// order.
///
/// `παροχ` and `τιμολ` are the stems of "Παροχή Υπηρεσιών" (provision of
/// services) and "Τιμολόγιο" (invoice), the heading of a Greek sales invoice.
///
/// `security` and `advise` are the two words of the line item on the sample
/// sales invoice in the corpus (`greek_sales_invoice.txt`: "security
/// advise"). They describe that one issuer's service, not sales in general.
/// The sample itself does not need them: the hint of a sales invoice is its
/// customer and a generated title, and there `consult` in the customer's name
/// selects Sales. They can match only when the description of an income
/// document is a line item.
const INCOME_KEYWORDS: &TopicKeywords = &[
    (
        DocumentTopic::Sales,
        &[
            Word("sales"),
            Word("service"),
            Word("services"),
            Word("security"),
            Word("advise"),
            Word("advised"),
            Word("advises"),
            Prefix("consult"),
            Prefix("παροχ"),
            Prefix("τιμολ"),
        ],
    ),
    (
        DocumentTopic::Freelance,
        &[Prefix("freelance"), Word("project"), Word("projects")],
    ),
    (
        DocumentTopic::Salary,
        &[
            Word("salary"),
            Word("salaries"),
            Word("payroll"),
            Word("wage"),
            Word("wages"),
        ],
    ),
];

/// One side of the matcher: the accounts it suggests among, the keywords
/// that choose a topic, and the topic for a hint that names none.
struct TopicTable {
    /// The type of account the table suggests.
    account_type: AccountType,
    /// The topics in the order they are tried, with their keywords.
    keywords: &'static TopicKeywords,
    /// The topic of a document no keyword places.
    catch_all: DocumentTopic,
}

/// The matcher for money spent.
const EXPENSE_TOPICS: TopicTable = TopicTable {
    account_type: AccountType::Expense,
    keywords: EXPENSE_KEYWORDS,
    catch_all: DocumentTopic::OtherExpense,
};

/// The matcher for money received.
const INCOME_TOPICS: TopicTable = TopicTable {
    account_type: AccountType::Income,
    keywords: INCOME_KEYWORDS,
    catch_all: DocumentTopic::OtherIncome,
};

/// Picks the account of the table's type that `hints` point at, by the four
/// steps in the module documentation.
///
/// `hints` may be in any letter case. Returns `None` only when the book has
/// no active account of the type.
fn match_account_of_type(
    template: ChartTemplate,
    accounts: &[Account],
    hints: &str,
    table: &TopicTable,
) -> Option<AccountId> {
    let hints = hints.to_lowercase();
    let account_type = table.account_type;

    // The first topic the text points at that the chart has an account for.
    for (topic, words) in table.keywords {
        if !words.iter().any(|word| word.occurs_in(&hints)) {
            continue;
        }

        let codes = document_topic_codes(template, *topic);
        if let Some(account) = account_by_codes(accounts, account_type, codes) {
            return Some(account.id);
        }
    }

    let codes = document_topic_codes(template, table.catch_all);

    account_by_codes(accounts, account_type, codes)
        .or_else(|| first_of_type(accounts, account_type))
        .map(|account| account.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_accounts::{code_of_for_tests, seeded_chart_for_tests};

    /// Code of the expense account suggested for `hints`.
    fn expense_code(template: ChartTemplate, accounts: &[Account], hints: &str) -> Option<String> {
        code_of_for_tests(accounts, match_expense_account(template, accounts, hints))
    }

    /// Code of the income account suggested for `hints`.
    fn income_code(template: ChartTemplate, accounts: &[Account], hints: &str) -> Option<String> {
        code_of_for_tests(accounts, match_income_account(template, accounts, hints))
    }

    /// Every hint the matcher is exercised with, for both charts.
    const EXPENSE_HINTS: &[&str] = &[
        "dei electricity",
        "monthly invoice",
        "rent for march",
        "netflix",
        "supermarket",
        "uber ride",
        "github cloud",
        "pharmacy",
        "vat payment",
        "something unrecognised",
        "",
    ];

    const INCOME_HINTS: &[&str] = &[
        "consulting services",
        "freelance project",
        "monthly salary",
        "something unrecognised",
        "",
    ];

    #[test]
    fn the_personal_chart_files_each_topic_under_its_own_account() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let template = ChartTemplate::Personal;

        assert_eq!(
            expense_code(template, &accounts, "dei electricity").as_deref(),
            Some("5300")
        );
        assert_eq!(
            expense_code(template, &accounts, "monthly invoice").as_deref(),
            Some("5350")
        );
        assert_eq!(
            expense_code(template, &accounts, "rent for march").as_deref(),
            Some("5000")
        );
        assert_eq!(
            expense_code(template, &accounts, "netflix").as_deref(),
            Some("5500")
        );
        assert_eq!(
            expense_code(template, &accounts, "supermarket").as_deref(),
            Some("5100")
        );
        assert_eq!(
            expense_code(template, &accounts, "uber ride").as_deref(),
            Some("5200")
        );
        assert_eq!(
            expense_code(template, &accounts, "pharmacy").as_deref(),
            Some("5400")
        );
        assert_eq!(
            expense_code(template, &accounts, "vat payment").as_deref(),
            Some("5700")
        );
        // No personal account covers software, so it falls to the catch-all.
        assert_eq!(
            expense_code(template, &accounts, "github cloud").as_deref(),
            Some("5900")
        );
        assert_eq!(
            expense_code(template, &accounts, "something unrecognised").as_deref(),
            Some("5900")
        );

        assert_eq!(
            income_code(template, &accounts, "consulting services").as_deref(),
            Some("4900")
        );
        assert_eq!(
            income_code(template, &accounts, "freelance project").as_deref(),
            Some("4100")
        );
        assert_eq!(
            income_code(template, &accounts, "monthly salary").as_deref(),
            Some("4000")
        );
        assert_eq!(
            income_code(template, &accounts, "something unrecognised").as_deref(),
            Some("4900")
        );
    }

    #[test]
    fn the_company_chart_sends_a_topic_it_has_no_account_for_to_the_catch_all() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Company, false);
        let template = ChartTemplate::Company;

        assert_eq!(
            expense_code(template, &accounts, "github cloud").as_deref(),
            Some("5300")
        );
        assert_eq!(
            expense_code(template, &accounts, "vat payment").as_deref(),
            Some("5700")
        );
        // The company chart has no utilities or bills account.
        assert_eq!(
            expense_code(template, &accounts, "dei electricity").as_deref(),
            Some("5900")
        );
        // It does have Rent.
        assert_eq!(
            expense_code(template, &accounts, "rent for march").as_deref(),
            Some("5200")
        );

        assert_eq!(
            income_code(template, &accounts, "consulting services").as_deref(),
            Some("4000")
        );
        assert_eq!(
            income_code(template, &accounts, "monthly salary").as_deref(),
            Some("4900")
        );
    }

    /// One hint per expense topic, each naming no other topic's word, with the
    /// code the personal chart gives it. Software has no personal account, so
    /// it takes the catch-all.
    const PERSONAL_EXPENSE_PINS: &[(&str, &str)] = &[
        ("electric", "5300"),
        ("invoice", "5350"),
        ("rent", "5000"),
        ("netflix", "5500"),
        ("grocery", "5100"),
        ("parking", "5200"),
        ("github", "5900"),
        ("clinic", "5400"),
        ("vat", "5700"),
        ("something unrecognised", "5900"),
    ];

    /// The same for the company chart. Only housing (Rent), transport (Travel),
    /// software, tax and the catch-all have a company account; every other topic
    /// falls to the catch-all.
    const COMPANY_EXPENSE_PINS: &[(&str, &str)] = &[
        ("electric", "5900"),
        ("invoice", "5900"),
        ("rent", "5200"),
        ("netflix", "5900"),
        ("grocery", "5900"),
        ("parking", "5600"),
        ("github", "5300"),
        ("clinic", "5900"),
        ("vat", "5700"),
        ("something unrecognised", "5900"),
    ];

    const PERSONAL_INCOME_PINS: &[(&str, &str)] = &[
        ("consulting", "4900"),
        ("freelance", "4100"),
        ("payroll", "4000"),
        ("something unrecognised", "4900"),
    ];

    const COMPANY_INCOME_PINS: &[(&str, &str)] = &[
        ("consulting", "4000"),
        ("freelance", "4900"),
        ("payroll", "4900"),
        ("something unrecognised", "4900"),
    ];

    fn assert_pins(
        template: ChartTemplate,
        expense_pins: &[(&str, &str)],
        income_pins: &[(&str, &str)],
    ) {
        for rename in [false, true] {
            let accounts = seeded_chart_for_tests(template, rename);

            for (hints, code) in expense_pins {
                assert_eq!(
                    expense_code(template, &accounts, hints).as_deref(),
                    Some(*code),
                    "{template:?} expense for {hints:?} (renamed: {rename})",
                );
            }
            for (hints, code) in income_pins {
                assert_eq!(
                    income_code(template, &accounts, hints).as_deref(),
                    Some(*code),
                    "{template:?} income for {hints:?} (renamed: {rename})",
                );
            }
        }
    }

    #[test]
    fn a_taxi_document_is_transport_and_not_taxes() {
        let personal = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let company = seeded_chart_for_tests(ChartTemplate::Company, false);

        for hints in ["taxi", "Taxi receipt", "uber taxi fare"] {
            assert_eq!(
                expense_code(ChartTemplate::Personal, &personal, hints).as_deref(),
                Some("5200"),
                "personal chart, {hints:?}",
            );
            assert_eq!(
                expense_code(ChartTemplate::Company, &company, hints).as_deref(),
                Some("5600"),
                "company chart, {hints:?}",
            );
        }
    }

    #[test]
    fn tax_documents_resolve_to_taxes_and_syntax_matches_nothing() {
        let personal = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let company = seeded_chart_for_tests(ChartTemplate::Company, false);

        for hints in [
            "tax",
            "income tax",
            "VAT/tax return",
            "property tax",
            "tax:",
        ] {
            for (template, accounts) in [
                (ChartTemplate::Personal, &personal),
                (ChartTemplate::Company, &company),
            ] {
                assert_eq!(
                    expense_code(template, accounts, hints).as_deref(),
                    Some("5700"),
                    "{template:?}, {hints:?}",
                );
            }
        }

        // "syntax" names no topic, so the catch-all takes it.
        assert_eq!(
            expense_code(ChartTemplate::Personal, &personal, "syntax").as_deref(),
            Some("5900")
        );
        assert_eq!(
            expense_code(ChartTemplate::Company, &company, "syntax").as_deref(),
            Some("5900")
        );
    }

    #[test]
    fn the_company_chart_files_rent_and_transport_under_its_own_accounts() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Company, false);
        let template = ChartTemplate::Company;

        for hints in ["rent for march", "office lease rent", "mortgage", "housing"] {
            assert_eq!(
                expense_code(template, &accounts, hints).as_deref(),
                Some("5200"),
                "{hints:?}",
            );
        }
        for hints in ["fuel", "parking", "taxi", "train transport", "uber ride"] {
            assert_eq!(
                expense_code(template, &accounts, hints).as_deref(),
                Some("5600"),
                "{hints:?}",
            );
        }
    }

    /// Merchant and title hints with the expense code each chart must give:
    /// (hint, personal code, company code). Personal: Housing 5000, Food 5100,
    /// Transport 5200, Utilities 5300, Bills 5350, Health 5400, Subscription
    /// 5500, Tax 5700, Other 5900. Company: Rent 5200, Software 5300, Travel
    /// 5600, Tax 5700, Other 5900.
    const EXPENSE_HINT_CODES: &[(&str, &str, &str)] = &[
        // Stems that real names build on.
        ("Cloudflare", "5900", "5300"),
        ("iCloud", "5900", "5300"),
        ("Cloud storage", "5900", "5300"),
        ("healthcare", "5400", "5900"),
        ("clinical", "5400", "5900"),
        ("clinics", "5400", "5900"),
        ("polyclinic", "5400", "5900"),
        ("Foodpanda", "5100", "5900"),
        ("foods", "5100", "5900"),
        ("efood", "5100", "5900"),
        ("seafood", "5100", "5900"),
        ("fuels", "5200", "5600"),
        ("fueling", "5200", "5600"),
        ("transporter", "5200", "5600"),
        ("transportation", "5200", "5600"),
        ("refuel", "5200", "5600"),
        ("taxis", "5200", "5600"),
        ("Taxibeat", "5200", "5600"),
        ("billing", "5350", "5900"),
        ("billed", "5350", "5900"),
        ("invoiced", "5350", "5900"),
        ("invoicing", "5350", "5900"),
        ("renting", "5000", "5200"),
        ("rented", "5000", "5200"),
        // Car hire is travel, not rent.
        ("car rental", "5200", "5600"),
        ("car rentals", "5200", "5600"),
        ("Car rental invoice", "5200", "5600"),
        ("car hire", "5200", "5600"),
        ("rent a car", "5200", "5600"),
        ("Rent a Car — Invoice 7", "5200", "5600"),
        ("rent", "5000", "5200"),
        ("rental", "5000", "5200"),
        ("apartment rent bill", "5000", "5200"),
        ("Invoice 42 — Rent", "5000", "5200"),
        // Bills is the generic topic: specific topics win over it...
        ("pharmacy receipt", "5400", "5900"),
        ("doctor bill", "5400", "5900"),
        ("clinic invoice", "5400", "5900"),
        // ...but it wins over Tax: these are ordinary invoices.
        ("Tax invoice 42", "5350", "5700"),
        ("VAT invoice", "5350", "5700"),
        // Titles in the form the invoice reader generates.
        ("Electricity bill", "5300", "5900"),
        ("Gas bill", "5300", "5900"),
        ("Water bill", "5300", "5900"),
        ("Telecom bill", "5350", "5900"),
        ("ACME — Invoice 42", "5350", "5900"),
        ("Netflix — Invoice 42", "5500", "5900"),
        ("Restaurant Plaka — Invoice 12", "5100", "5900"),
        ("Taxi receipt", "5200", "5600"),
        ("Fuel receipt", "5200", "5600"),
        // Words that only contain a keyword name no topic.
        ("syntax", "5900", "5900"),
        ("taxidermy", "5900", "5900"),
        ("waterfall", "5900", "5900"),
        ("savings", "5900", "5900"),
        ("Holdings", "5900", "5900"),
        ("renovation", "5900", "5900"),
        ("private", "5900", "5900"),
        ("parent", "5900", "5900"),
        ("current account", "5900", "5900"),
        ("Laurent", "5900", "5900"),
        ("Huber GmbH", "5900", "5900"),
        ("laws", "5900", "5900"),
    ];

    #[test]
    fn merchant_names_compounds_and_car_hire_resolve_on_both_charts() {
        let personal = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let company = seeded_chart_for_tests(ChartTemplate::Company, false);

        for (hints, personal_code, company_code) in EXPENSE_HINT_CODES {
            assert_eq!(
                expense_code(ChartTemplate::Personal, &personal, hints).as_deref(),
                Some(*personal_code),
                "personal chart, {hints:?}",
            );
            assert_eq!(
                expense_code(ChartTemplate::Company, &company, hints).as_deref(),
                Some(*company_code),
                "company chart, {hints:?}",
            );
        }
    }

    /// The first topic of `table` whose keywords occur in `text`.
    fn first_topic(table: &TopicKeywords, text: &str) -> Option<DocumentTopic> {
        let lowercased = text.to_lowercase();

        table
            .iter()
            .find(|(_, keywords)| {
                keywords
                    .iter()
                    .any(|keyword| keyword.occurs_in(&lowercased))
            })
            .map(|(topic, _)| *topic)
    }

    #[test]
    fn every_keyword_as_a_whole_word_resolves_to_its_topic() {
        for table in [EXPENSE_KEYWORDS, INCOME_KEYWORDS] {
            for (topic, keywords) in table {
                for keyword in *keywords {
                    let text = keyword.text();

                    for sentence in [
                        text.to_string(),
                        format!("paid the {text} today"),
                        format!("Ref: {text}, due"),
                        format!("a/{text}/b"),
                    ] {
                        assert_eq!(
                            first_topic(table, &sentence),
                            Some(*topic),
                            "{keyword:?} in {sentence:?}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_keyword_matches_inside_a_longer_unrelated_word() {
        for table in [EXPENSE_KEYWORDS, INCOME_KEYWORDS] {
            for (_, keywords) in table {
                for keyword in *keywords {
                    let text = keyword.text();

                    match keyword {
                        Keyword::Word(_) => {
                            for sentence in [
                                format!("{text}xyz"),
                                format!("xyz{text}"),
                                format!("xyz{text}xyz"),
                                format!("{text}7"),
                                format!("7{text}"),
                            ] {
                                assert!(
                                    !keyword.occurs_in(&sentence),
                                    "{keyword:?} must not match in {sentence:?}",
                                );
                            }
                        }
                        Keyword::Prefix(_) => {
                            // A stem matches the start of a longer word, never its middle.
                            assert!(keyword.occurs_in(&format!("{text}xyz")), "{keyword:?}");
                            assert!(keyword.occurs_in(&format!("a {text}xyz b")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("xyz{text}")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("7{text}")), "{keyword:?}");
                        }
                        Keyword::Fragment(_) => {
                            // A fragment would match inside any word.
                            assert_eq!(None, Some(keyword), "a topic keyword is never a fragment");
                        }
                        Keyword::Unit(_) => {
                            // A unit may follow a number, but not a letter.
                            assert!(keyword.occurs_in(&format!("150{text}")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("xyz{text}")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("{text}xyz")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("{text}7")), "{keyword:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn words_that_only_contain_a_keyword_name_no_topic() {
        for text in ["syntax", "taxidermy", "waterloo", "parent", "mortgagee x"] {
            assert_eq!(first_topic(EXPENSE_KEYWORDS, text), None, "{text:?}");
        }
        assert_eq!(
            first_topic(EXPENSE_KEYWORDS, "taxi"),
            Some(DocumentTopic::Transport)
        );
        assert_eq!(
            first_topic(EXPENSE_KEYWORDS, "150kWh"),
            Some(DocumentTopic::Utilities)
        );
    }

    #[test]
    fn every_topic_on_the_personal_chart_is_pinned_by_code() {
        assert_pins(
            ChartTemplate::Personal,
            PERSONAL_EXPENSE_PINS,
            PERSONAL_INCOME_PINS,
        );
    }

    #[test]
    fn every_topic_on_the_company_chart_is_pinned_by_code() {
        assert_pins(
            ChartTemplate::Company,
            COMPANY_EXPENSE_PINS,
            COMPANY_INCOME_PINS,
        );
    }

    #[test]
    fn renamed_charts_match_the_same_accounts_as_english_ones() {
        for template in [ChartTemplate::Personal, ChartTemplate::Company] {
            let english = seeded_chart_for_tests(template, false);
            let renamed = seeded_chart_for_tests(template, true);

            for hints in EXPENSE_HINTS {
                assert_eq!(
                    expense_code(template, &renamed, hints),
                    expense_code(template, &english, hints),
                    "{template:?} expense for {hints:?}",
                );
            }
            for hints in INCOME_HINTS {
                assert_eq!(
                    income_code(template, &renamed, hints),
                    income_code(template, &english, hints),
                    "{template:?} income for {hints:?}",
                );
            }
        }
    }

    #[test]
    fn a_deactivated_topic_account_falls_through_to_the_next_choice() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);
        for account in &mut accounts {
            if account.code == "5300" {
                account.is_active = false;
            }
        }

        // Utilities is gone, so the catch-all "Other" takes the document.
        assert_eq!(
            expense_code(ChartTemplate::Personal, &accounts, "dei electricity").as_deref(),
            Some("5900")
        );
    }

    /// Code of the active account of `account_type` with the lowest sort
    /// order, then code: what a book with no seeded codes must suggest.
    fn first_code_of_type(accounts: &[Account], account_type: AccountType) -> Option<String> {
        accounts
            .iter()
            .filter(|account| account.is_active && account.account_type == account_type)
            .min_by_key(|account| (account.sort_order, account.code.clone()))
            .map(|account| account.code.clone())
    }

    #[test]
    fn a_blank_book_suggests_the_first_account_of_the_type_whatever_it_is_called() {
        let template = ChartTemplate::Blank;
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);
        // A blank book's accounts are the user's own: none carries a seeded
        // code, so the words in their names must not steer the choice.
        for (index, account) in accounts.iter_mut().enumerate() {
            account.code = format!("U{index:03}");
        }

        let expected_expense = first_code_of_type(&accounts, AccountType::Expense);
        let expected_income = first_code_of_type(&accounts, AccountType::Income);
        assert!(expected_expense.is_some() && expected_income.is_some());

        assert_eq!(
            expense_code(template, &accounts, "dei electricity"),
            expected_expense
        );
        assert_eq!(
            income_code(template, &accounts, "monthly salary"),
            expected_income
        );
    }

    #[test]
    fn a_book_with_no_account_of_the_type_suggests_nothing() {
        let accounts: Vec<Account> = seeded_chart_for_tests(ChartTemplate::Personal, false)
            .into_iter()
            .filter(|account| account.account_type == AccountType::Asset)
            .collect();

        assert_eq!(
            match_expense_account(ChartTemplate::Personal, &accounts, "electricity"),
            None
        );
        assert_eq!(
            match_income_account(ChartTemplate::Personal, &accounts, "salary"),
            None
        );
    }
}
