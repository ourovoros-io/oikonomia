//! Text that core writes into the user's books, in every supported language.
//!
//! This is the one exception to the rule that core hands the UI codes and
//! never sentences (see [`crate::ui_text`]). Account names seeded by a chart
//! template, the descriptions core generates for opening balances and voids,
//! and the suggestions the document reader offers all end up stored in the
//! book, where they are data like anything the user typed. They are written
//! in the language the app is set to at that moment and are not rewritten
//! when the language changes later.
//!
//! # Structure
//!
//! The module is data keyed by [`Locale`]. Every piece of wording is a
//! private `Localized` value holding one string per language, and the only
//! way to read one is a `match` on [`Locale`] with no wildcard arm. Adding a
//! language therefore does not compile until every piece of wording here has
//! it.
//!
//! Callers never see the table. They call a function per kind of text
//! ([`seeded_account_name`], [`void_description`], [`bill_description`], ...)
//! with the locale, and get the finished string.
//!
//! # Conventions for stored wording
//!
//! - The text is exported to CSV and printed in the expense PDF. The embedded
//!   Inter font covers U+202F, but the Helvetica fallback used if the embed
//!   fails cannot encode it, so French wording here uses an ordinary space,
//!   never a narrow no-break space. The wording also keeps to the plain
//!   apostrophe and has no U+00A0; a test checks all three.
//! - No wording supplied here starts with `=`, `+`, `-` or `@`, the
//!   characters a spreadsheet reads as the start of a formula. That covers a
//!   generated text only when core's wording comes first. [`bill_description`]
//!   and [`customer_invoice_description`] lead with a merchant or customer
//!   name read from a document, so their result can start with anything.
//!   [`crate::csv::export_journal_csv`] guards every free-text cell it writes
//!   for that reason.

use crate::domain::ChartTemplate;
use crate::prefs::Locale;

/// One piece of wording in every supported language.
#[derive(Debug, Clone, Copy)]
struct Localized {
    /// The English wording.
    en: &'static str,
    /// The Greek wording.
    el: &'static str,
    /// The French wording.
    fr: &'static str,
    /// The German wording.
    de: &'static str,
}

impl Localized {
    /// Returns the wording given in English, Greek, French and German, in
    /// that order.
    const fn new(en: &'static str, el: &'static str, fr: &'static str, de: &'static str) -> Self {
        Self { en, el, fr, de }
    }

    /// Returns the wording in `locale`.
    const fn in_locale(self, locale: Locale) -> &'static str {
        match locale {
            Locale::En => self.en,
            Locale::El => self.el,
            Locale::Fr => self.fr,
            Locale::De => self.de,
        }
    }
}

/// Names of the accounts the personal chart seeds, by account code.
const PERSONAL_ACCOUNT_NAMES: &[(&str, Localized)] = &[
    (
        "1000",
        Localized::new("Cash", "Μετρητά", "Espèces", "Bargeld"),
    ),
    (
        "1010",
        Localized::new(
            "Checking",
            "Λογαριασμός όψεως",
            "Compte courant",
            "Girokonto",
        ),
    ),
    (
        "1020",
        Localized::new("Savings", "Αποταμίευση", "Épargne", "Sparkonto"),
    ),
    (
        "1100",
        Localized::new("Investments", "Επενδύσεις", "Placements", "Geldanlagen"),
    ),
    (
        "2000",
        Localized::new(
            "Credit Card",
            "Πιστωτική κάρτα",
            "Carte de crédit",
            "Kreditkarte",
        ),
    ),
    (
        "2050",
        Localized::new(
            "Bills Payable",
            "Πληρωτέοι λογαριασμοί",
            "Factures à payer",
            "Offene Rechnungen",
        ),
    ),
    (
        "2100",
        Localized::new("Loans", "Δάνεια", "Emprunts", "Kredite"),
    ),
    (
        "3000",
        Localized::new(
            "Opening Balances",
            "Υπόλοιπα έναρξης",
            "Soldes d'ouverture",
            "Anfangssalden",
        ),
    ),
    (
        "3100",
        Localized::new(
            "Owner Equity",
            "Ίδια κεφάλαια",
            "Capitaux propres",
            "Eigenkapital",
        ),
    ),
    (
        "4000",
        Localized::new("Salary", "Μισθός", "Salaire", "Gehalt"),
    ),
    (
        "4100",
        Localized::new(
            "Freelance",
            "Ελεύθερο επάγγελμα",
            "Activité indépendante",
            "Freiberufliche Tätigkeit",
        ),
    ),
    (
        "4200",
        Localized::new("Interest", "Τόκοι", "Intérêts", "Zinsen"),
    ),
    (
        "4900",
        Localized::new(
            "Other Income",
            "Λοιπά έσοδα",
            "Autres recettes",
            "Sonstige Einnahmen",
        ),
    ),
    (
        "5000",
        Localized::new("Housing", "Στέγαση", "Logement", "Wohnen"),
    ),
    (
        "5100",
        Localized::new("Food", "Διατροφή", "Alimentation", "Lebensmittel"),
    ),
    (
        "5200",
        Localized::new("Transport", "Μετακινήσεις", "Transports", "Mobilität"),
    ),
    (
        "5300",
        Localized::new(
            "Utilities",
            "Λογαριασμοί κοινής ωφέλειας",
            "Charges",
            "Nebenkosten",
        ),
    ),
    (
        "5350",
        Localized::new(
            "Bills & services",
            "Λογαριασμοί και υπηρεσίες",
            "Factures et services",
            "Rechnungen und Dienstleistungen",
        ),
    ),
    (
        "5400",
        Localized::new("Health", "Υγεία", "Santé", "Gesundheit"),
    ),
    (
        "5500",
        Localized::new("Subscriptions", "Συνδρομές", "Abonnements", "Abonnements"),
    ),
    (
        "5600",
        Localized::new("Entertainment", "Ψυχαγωγία", "Loisirs", "Freizeit"),
    ),
    (
        "5700",
        Localized::new("Taxes", "Φόροι", "Impôts", "Steuern"),
    ),
    (
        "5900",
        Localized::new("Other", "Λοιπά", "Autres", "Sonstiges"),
    ),
];

/// Names of the accounts the company chart seeds, by account code.
///
/// Some accounts repeat the personal chart, which the tables do not enforce:
/// Cash (1000), Opening Balances (3000) and Taxes (5700) have the same code
/// and the same wording in every language. Credit Card and Loans have the
/// same wording under other codes (2100 and 2200 here, 2000 and 2100 there).
/// Other Income (4900) matches in English and Greek only; the French and
/// German differ.
const COMPANY_ACCOUNT_NAMES: &[(&str, Localized)] = &[
    (
        "1000",
        Localized::new("Cash", "Μετρητά", "Espèces", "Bargeld"),
    ),
    ("1010", Localized::new("Bank", "Τράπεζα", "Banque", "Bank")),
    (
        "1100",
        Localized::new("Accounts Receivable", "Πελάτες", "Clients", "Forderungen"),
    ),
    (
        "1500",
        Localized::new("Equipment", "Εξοπλισμός", "Matériel", "Betriebsausstattung"),
    ),
    (
        "2000",
        Localized::new(
            "Accounts Payable",
            "Προμηθευτές",
            "Fournisseurs",
            "Lieferantenverbindlichkeiten",
        ),
    ),
    (
        "2100",
        Localized::new(
            "Credit Card",
            "Πιστωτική κάρτα",
            "Carte de crédit",
            "Kreditkarte",
        ),
    ),
    (
        "2200",
        Localized::new("Loans", "Δάνεια", "Emprunts", "Kredite"),
    ),
    (
        "2300",
        Localized::new(
            "Taxes Payable",
            "Φόροι πληρωτέοι",
            "Dettes fiscales",
            "Steuerschulden",
        ),
    ),
    (
        "3000",
        Localized::new(
            "Opening Balances",
            "Υπόλοιπα έναρξης",
            "Soldes d'ouverture",
            "Anfangssalden",
        ),
    ),
    (
        "3100",
        Localized::new(
            "Owner Capital",
            "Κεφάλαιο ιδιοκτήτη",
            "Capital",
            "Kapitaleinlagen",
        ),
    ),
    (
        "3200",
        Localized::new(
            "Retained Earnings",
            "Αποτελέσματα εις νέο",
            "Report à nouveau",
            "Gewinnvortrag",
        ),
    ),
    (
        "4000",
        Localized::new(
            "Sales / Services",
            "Πωλήσεις / Υπηρεσίες",
            "Ventes / Prestations",
            "Umsätze / Leistungen",
        ),
    ),
    (
        "4900",
        Localized::new(
            "Other Income",
            "Λοιπά έσοδα",
            "Autres produits",
            "Sonstige Erträge",
        ),
    ),
    (
        "5000",
        Localized::new(
            "COGS",
            "Κόστος πωληθέντων",
            "Coût des ventes",
            "Wareneinsatz",
        ),
    ),
    (
        "5100",
        Localized::new("Payroll", "Μισθοδοσία", "Salaires", "Personalkosten"),
    ),
    ("5200", Localized::new("Rent", "Ενοίκιο", "Loyer", "Miete")),
    (
        "5300",
        Localized::new("Software", "Λογισμικό", "Logiciels", "Software"),
    ),
    (
        "5400",
        Localized::new("Marketing", "Διαφήμιση και προβολή", "Publicité", "Werbung"),
    ),
    (
        "5500",
        Localized::new(
            "Professional Fees",
            "Αμοιβές τρίτων",
            "Honoraires",
            "Rechts- und Beratungskosten",
        ),
    ),
    (
        "5600",
        Localized::new("Travel", "Ταξίδια", "Déplacements", "Reisekosten"),
    ),
    (
        "5700",
        Localized::new("Taxes", "Φόροι", "Impôts", "Steuern"),
    ),
    (
        "5900",
        Localized::new(
            "Other OpEx",
            "Λοιπά λειτουργικά έξοδα",
            "Autres charges d'exploitation",
            "Sonstige Betriebskosten",
        ),
    ),
];

/// Returns the name of the seeded account `code` in `template`, in `locale`.
///
/// The result is `None` when the template seeds no account with that code; a
/// blank chart seeds none.
///
/// # Examples
///
/// ```
/// use oikonomia_core::domain::ChartTemplate;
/// use oikonomia_core::prefs::Locale;
/// use oikonomia_core::text::seeded_account_name;
///
/// assert_eq!(
///     seeded_account_name(ChartTemplate::Personal, "1010", Locale::De),
///     Some("Girokonto")
/// );
/// assert_eq!(seeded_account_name(ChartTemplate::Blank, "1010", Locale::De), None);
/// ```
#[must_use]
pub fn seeded_account_name(
    template: ChartTemplate,
    code: &str,
    locale: Locale,
) -> Option<&'static str> {
    let names = match template {
        ChartTemplate::Blank => return None,
        ChartTemplate::Personal => PERSONAL_ACCOUNT_NAMES,
        ChartTemplate::Company => COMPANY_ACCOUNT_NAMES,
    };

    names
        .iter()
        .find(|(seeded_code, _)| *seeded_code == code)
        .map(|(_, name)| name.in_locale(locale))
}

/// Returns every account code with a name in the table for `template`, in
/// table order.
#[cfg(test)]
#[must_use]
pub(crate) fn seeded_account_codes(template: ChartTemplate) -> Vec<&'static str> {
    let names = match template {
        ChartTemplate::Blank => return Vec::new(),
        ChartTemplate::Personal => PERSONAL_ACCOUNT_NAMES,
        ChartTemplate::Company => COMPANY_ACCOUNT_NAMES,
    };

    names.iter().map(|(code, _)| *code).collect()
}

/// The words that open an opening-balance description.
const OPENING_BALANCE: Localized = Localized::new(
    "Opening balance",
    "Υπόλοιπο έναρξης",
    "Solde d'ouverture",
    "Anfangssaldo",
);

/// Returns the description of the entry that sets the opening balance of the
/// account named `account_name`: the wording, a dash, then the name.
#[must_use]
pub fn opening_balance_description(locale: Locale, account_name: &str) -> String {
    format!("{} — {account_name}", OPENING_BALANCE.in_locale(locale))
}

/// The word that opens a void description, with its separator: French puts a
/// space before the colon. The space is an ordinary one, never U+202F.
const VOID_PREFIX: Localized = Localized::new("VOID:", "ΑΚΥΡΩΣΗ:", "ANNULATION :", "STORNO:");

/// The memo on each line of a reversing entry.
const VOID_MEMO: Localized = Localized::new("Void", "Ακύρωση", "Annulation", "Storno");

/// Returns the description of the reversing entry that voids the entry
/// described by `original_description`: the void prefix, then that text.
///
/// # Examples
///
/// ```
/// use oikonomia_core::prefs::Locale;
/// use oikonomia_core::text::void_description;
///
/// assert_eq!(void_description(Locale::En, "Groceries"), "VOID: Groceries");
/// assert_eq!(void_description(Locale::Fr, "Courses"), "ANNULATION : Courses");
/// ```
#[must_use]
pub fn void_description(locale: Locale, original_description: &str) -> String {
    format!("{} {original_description}", VOID_PREFIX.in_locale(locale))
}

/// Returns the memo put on every line of a reversing entry.
#[must_use]
pub const fn void_memo(locale: Locale) -> &'static str {
    VOID_MEMO.in_locale(locale)
}

/// What a recognised bill is for, as far as its suggested title goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillKind {
    /// An electricity bill.
    Electricity,
    /// A gas bill.
    Gas,
    /// A phone or internet bill.
    Telecom,
    /// A water bill.
    Water,
    /// A utility bill whose service could not be told apart.
    Utility,
}

impl BillKind {
    /// Returns the title a bill of this kind is suggested under.
    const fn title(self) -> Localized {
        match self {
            Self::Electricity => Localized::new(
                "Electricity bill",
                "Λογαριασμός ρεύματος",
                "Facture d'électricité",
                "Stromrechnung",
            ),
            Self::Gas => Localized::new(
                "Gas bill",
                "Λογαριασμός φυσικού αερίου",
                "Facture de gaz",
                "Gasrechnung",
            ),
            Self::Telecom => Localized::new(
                "Telecom bill",
                "Λογαριασμός τηλεπικοινωνιών",
                "Facture télécom",
                "Telefon- und Internetrechnung",
            ),
            Self::Water => Localized::new(
                "Water bill",
                "Λογαριασμός νερού",
                "Facture d'eau",
                "Wasserrechnung",
            ),
            Self::Utility => Localized::new(
                "Utility bill",
                "Λογαριασμός κοινής ωφέλειας",
                "Facture d'énergie",
                "Versorgerrechnung",
            ),
        }
    }
}

/// Returns the suggested description of a utility bill: the title of `kind`,
/// preceded by `merchant` and a dash when the biller is known.
#[must_use]
pub fn bill_description(locale: Locale, kind: BillKind, merchant: Option<&str>) -> String {
    let title = kind.title().in_locale(locale);

    match merchant {
        Some(merchant) => format!("{merchant} — {title}"),
        None => title.to_owned(),
    }
}

/// The word for an invoice.
const INVOICE: Localized = Localized::new("Invoice", "Τιμολόγιο", "Facture", "Rechnung");

/// Returns the suggested description of an outgoing invoice: `customer`, a
/// dash, the word for invoice, then `reference` when there is one.
#[must_use]
pub fn customer_invoice_description(
    locale: Locale,
    customer: &str,
    reference: Option<&str>,
) -> String {
    let word = INVOICE.in_locale(locale);

    match reference {
        Some(reference) => format!("{customer} — {word} {reference}"),
        None => format!("{customer} — {word}"),
    }
}

/// Returns the suggested description of an invoice known by its number: the
/// word for invoice and `reference`, then a dash and `merchant` when there is
/// one.
#[must_use]
pub fn invoice_reference_description(
    locale: Locale,
    reference: &str,
    merchant: Option<&str>,
) -> String {
    let word = INVOICE.in_locale(locale);

    match merchant {
        Some(merchant) => format!("{word} {reference} — {merchant}"),
        None => format!("{word} {reference}"),
    }
}

/// Returns the suggested description of an invoice with neither number nor
/// merchant: the bare word for invoice.
#[must_use]
pub const fn invoice_word(locale: Locale) -> &'static str {
    INVOICE.in_locale(locale)
}

/// The words for a bank transfer.
const BANK_TRANSFER: Localized = Localized::new(
    "Bank transfer",
    "Έμβασμα",
    "Virement bancaire",
    "Überweisung",
);

/// Returns the suggested description of a bank transfer receipt: the words
/// for a bank transfer, then a dash and `payee` when there is one.
#[must_use]
pub fn bank_transfer_description(locale: Locale, payee: Option<&str>) -> String {
    let word = BANK_TRANSFER.in_locale(locale);

    match payee {
        Some(payee) => format!("{word} — {payee}"),
        None => word.to_owned(),
    }
}

/// The stand-in merchant name for an unrecognised gas supplier.
const NATURAL_GAS_SUPPLIER: Localized =
    Localized::new("Natural gas", "Φυσικό αέριο", "Gaz naturel", "Erdgas");

/// The stand-in merchant name for an unrecognised electricity supplier.
const ELECTRICITY_SUPPLIER: Localized = Localized::new(
    "Electricity supplier",
    "Πάροχος ηλεκτρικής ενέργειας",
    "Fournisseur d'électricité",
    "Stromversorger",
);

/// Returns the suggested merchant for a gas bill whose issuer is not
/// recognised.
#[must_use]
pub const fn natural_gas_merchant(locale: Locale) -> &'static str {
    NATURAL_GAS_SUPPLIER.in_locale(locale)
}

/// Returns the suggested merchant for an electricity bill whose issuer is not
/// recognised.
#[must_use]
pub const fn electricity_supplier_merchant(locale: Locale) -> &'static str {
    ELECTRICITY_SUPPLIER.in_locale(locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bill kind.
    const BILL_KINDS: [BillKind; 5] = [
        BillKind::Electricity,
        BillKind::Gas,
        BillKind::Telecom,
        BillKind::Water,
        BillKind::Utility,
    ];

    /// Every generated form for `locale`, in the order the expectations below
    /// list them: customer invoice with and without a reference, invoice by
    /// reference with and without a merchant, the bare invoice word, the bank
    /// transfer with and without a payee, then for each bill kind the title
    /// with a merchant and alone.
    fn forms(locale: Locale) -> Vec<String> {
        let mut forms = vec![
            customer_invoice_description(locale, "ACME", Some("42")),
            customer_invoice_description(locale, "ACME", None),
            invoice_reference_description(locale, "42", Some("ACME")),
            invoice_reference_description(locale, "42", None),
            invoice_word(locale).to_owned(),
            bank_transfer_description(locale, Some("ACME")),
            bank_transfer_description(locale, None),
        ];

        for kind in BILL_KINDS {
            forms.push(bill_description(locale, kind, Some("ACME")));
            forms.push(bill_description(locale, kind, None));
        }

        forms
    }

    #[test]
    fn english_forms_are_exact() {
        assert_eq!(
            forms(Locale::En),
            [
                "ACME — Invoice 42",
                "ACME — Invoice",
                "Invoice 42 — ACME",
                "Invoice 42",
                "Invoice",
                "Bank transfer — ACME",
                "Bank transfer",
                "ACME — Electricity bill",
                "Electricity bill",
                "ACME — Gas bill",
                "Gas bill",
                "ACME — Telecom bill",
                "Telecom bill",
                "ACME — Water bill",
                "Water bill",
                "ACME — Utility bill",
                "Utility bill",
            ]
        );
    }

    #[test]
    fn greek_forms_are_exact() {
        assert_eq!(
            forms(Locale::El),
            [
                "ACME — Τιμολόγιο 42",
                "ACME — Τιμολόγιο",
                "Τιμολόγιο 42 — ACME",
                "Τιμολόγιο 42",
                "Τιμολόγιο",
                "Έμβασμα — ACME",
                "Έμβασμα",
                "ACME — Λογαριασμός ρεύματος",
                "Λογαριασμός ρεύματος",
                "ACME — Λογαριασμός φυσικού αερίου",
                "Λογαριασμός φυσικού αερίου",
                "ACME — Λογαριασμός τηλεπικοινωνιών",
                "Λογαριασμός τηλεπικοινωνιών",
                "ACME — Λογαριασμός νερού",
                "Λογαριασμός νερού",
                "ACME — Λογαριασμός κοινής ωφέλειας",
                "Λογαριασμός κοινής ωφέλειας",
            ]
        );
    }

    #[test]
    fn french_forms_are_exact() {
        assert_eq!(
            forms(Locale::Fr),
            [
                "ACME — Facture 42",
                "ACME — Facture",
                "Facture 42 — ACME",
                "Facture 42",
                "Facture",
                "Virement bancaire — ACME",
                "Virement bancaire",
                "ACME — Facture d'électricité",
                "Facture d'électricité",
                "ACME — Facture de gaz",
                "Facture de gaz",
                "ACME — Facture télécom",
                "Facture télécom",
                "ACME — Facture d'eau",
                "Facture d'eau",
                "ACME — Facture d'énergie",
                "Facture d'énergie",
            ]
        );
    }

    #[test]
    fn german_forms_are_exact() {
        assert_eq!(
            forms(Locale::De),
            [
                "ACME — Rechnung 42",
                "ACME — Rechnung",
                "Rechnung 42 — ACME",
                "Rechnung 42",
                "Rechnung",
                "Überweisung — ACME",
                "Überweisung",
                "ACME — Stromrechnung",
                "Stromrechnung",
                "ACME — Gasrechnung",
                "Gasrechnung",
                "ACME — Telefon- und Internetrechnung",
                "Telefon- und Internetrechnung",
                "ACME — Wasserrechnung",
                "Wasserrechnung",
                "ACME — Versorgerrechnung",
                "Versorgerrechnung",
            ]
        );
    }

    #[test]
    fn void_descriptions_and_memos_are_exact() {
        let expected = [
            (Locale::En, "VOID: Groceries", "Void"),
            (Locale::El, "ΑΚΥΡΩΣΗ: Groceries", "Ακύρωση"),
            (Locale::Fr, "ANNULATION : Groceries", "Annulation"),
            (Locale::De, "STORNO: Groceries", "Storno"),
        ];

        for (locale, description, memo) in expected {
            assert_eq!(void_description(locale, "Groceries"), description);
            assert_eq!(void_memo(locale), memo);
        }
    }

    #[test]
    fn suggested_merchants_are_exact() {
        let expected = [
            (Locale::En, "Natural gas", "Electricity supplier"),
            (Locale::El, "Φυσικό αέριο", "Πάροχος ηλεκτρικής ενέργειας"),
            (Locale::Fr, "Gaz naturel", "Fournisseur d'électricité"),
            (Locale::De, "Erdgas", "Stromversorger"),
        ];

        for (locale, gas, electricity) in expected {
            assert_eq!(natural_gas_merchant(locale), gas);
            assert_eq!(electricity_supplier_merchant(locale), electricity);
        }
    }

    #[test]
    fn generated_text_is_safe_for_spreadsheets_and_the_pdf_font() {
        for &locale in Locale::ALL {
            let mut generated = forms(locale);
            generated.push(opening_balance_description(locale, "Cash"));
            generated.push(void_description(locale, "Groceries"));
            generated.push(void_memo(locale).to_owned());
            generated.push(natural_gas_merchant(locale).to_owned());
            generated.push(electricity_supplier_merchant(locale).to_owned());

            for names in [PERSONAL_ACCOUNT_NAMES, COMPANY_ACCOUNT_NAMES] {
                generated.extend(
                    names
                        .iter()
                        .map(|(_, name)| name.in_locale(locale).to_owned()),
                );
            }

            for text in &generated {
                assert!(
                    !text.starts_with(['=', '+', '-', '@']),
                    "{locale:?}: {text} could be read as a formula"
                );
                assert!(
                    !text.contains(['\u{202f}', '\u{a0}', '\u{2019}']),
                    "{locale:?}: {text} has a no-break space or typographic apostrophe"
                );
            }
        }
    }
}
