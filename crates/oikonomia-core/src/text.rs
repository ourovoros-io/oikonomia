//! Text that core writes into the user's books, in every supported language.
//!
//! Account names seeded by a chart template, the descriptions core generates
//! for opening balances and voids, and the suggestions the document reader
//! offers are all stored once the user saves them. They are written in the
//! language the app is set to at that moment and are never rewritten when the
//! language changes later, so this module is pure data keyed by [`Locale`]:
//! no lookup has a wildcard arm, and adding a locale fails to compile until
//! its wording is supplied here.
//!
//! Conventions for stored wording:
//!
//! - The text is data. It is exported to CSV and printed in the expense PDF.
//!   The embedded Inter font covers U+202F and the typographic apostrophe, but
//!   the Helvetica fallback used if the embed fails cannot encode them, so
//!   stored text avoids them: French uses ordinary spaces and the plain
//!   apostrophe, never a narrow no-break space.
//! - A generated description never starts with `=`, `+`, `-` or `@`, so it is
//!   not mistaken for a formula by a spreadsheet.

use crate::domain::ChartTemplate;
use crate::prefs::Locale;

/// One piece of wording in every supported language.
#[derive(Debug, Clone, Copy)]
pub struct Localized {
    en: &'static str,
    el: &'static str,
    fr: &'static str,
    de: &'static str,
}

impl Localized {
    #[must_use]
    const fn new(en: &'static str, el: &'static str, fr: &'static str, de: &'static str) -> Self {
        Self { en, el, fr, de }
    }

    /// The wording in `locale`.
    #[must_use]
    pub const fn in_locale(self, locale: Locale) -> &'static str {
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
/// Cash, Opening Balances, Other Income and Taxes share their wording with the
/// personal chart (the language catalogs only ever translated them there).
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

/// Name of the seeded account `code` in `template`, in `locale`.
///
/// `None` when the template seeds no such account (a blank chart seeds none).
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

/// Every account code with a name in the table for `template`, in table order.
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

const OPENING_BALANCE: Localized = Localized::new(
    "Opening balance",
    "Υπόλοιπο έναρξης",
    "Solde d'ouverture",
    "Anfangssaldo",
);

/// Description of the entry that sets an account's opening balance.
#[must_use]
pub fn opening_balance_description(locale: Locale, account_name: &str) -> String {
    format!("{} — {account_name}", OPENING_BALANCE.in_locale(locale))
}

/// The word that opens a void description, with its separator: French puts a
/// space before the colon. The space is an ordinary one, never U+202F.
const VOID_PREFIX: Localized = Localized::new("VOID:", "ΑΚΥΡΩΣΗ:", "ANNULATION :", "STORNO:");

const VOID_MEMO: Localized = Localized::new("Void", "Ακύρωση", "Annulation", "Storno");

/// Description of the reversing entry that voids `original_description`.
#[must_use]
pub fn void_description(locale: Locale, original_description: &str) -> String {
    format!("{} {original_description}", VOID_PREFIX.in_locale(locale))
}

/// Memo on every line of a reversing entry.
#[must_use]
pub const fn void_memo(locale: Locale) -> &'static str {
    VOID_MEMO.in_locale(locale)
}

/// What a recognised bill is for, as far as its suggested title goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillKind {
    /// Electricity.
    Electricity,
    /// Gas.
    Gas,
    /// Phone and internet.
    Telecom,
    /// Water.
    Water,
    /// A utility bill whose service could not be told apart.
    Utility,
}

impl BillKind {
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

/// Suggested description of a utility bill, led by the biller when known.
#[must_use]
pub fn bill_description(locale: Locale, kind: BillKind, merchant: Option<&str>) -> String {
    let title = kind.title().in_locale(locale);

    match merchant {
        Some(merchant) => format!("{merchant} — {title}"),
        None => title.to_owned(),
    }
}

const INVOICE: Localized = Localized::new("Invoice", "Τιμολόγιο", "Facture", "Rechnung");

/// Suggested description of an outgoing invoice: the customer first.
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

/// Suggested description of an invoice known by its number, naming the
/// merchant when there is one.
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

/// Suggested description of an invoice with neither number nor merchant.
#[must_use]
pub const fn invoice_word(locale: Locale) -> &'static str {
    INVOICE.in_locale(locale)
}

const BANK_TRANSFER: Localized = Localized::new(
    "Bank transfer",
    "Έμβασμα",
    "Virement bancaire",
    "Überweisung",
);

/// Suggested description of a bank transfer receipt, naming the payee as the
/// document spells it.
#[must_use]
pub fn bank_transfer_description(locale: Locale, payee: Option<&str>) -> String {
    let word = BANK_TRANSFER.in_locale(locale);

    match payee {
        Some(payee) => format!("{word} — {payee}"),
        None => word.to_owned(),
    }
}

const NATURAL_GAS_SUPPLIER: Localized =
    Localized::new("Natural gas", "Φυσικό αέριο", "Gaz naturel", "Erdgas");

const ELECTRICITY_SUPPLIER: Localized = Localized::new(
    "Electricity supplier",
    "Πάροχος ηλεκτρικής ενέργειας",
    "Fournisseur d'électricité",
    "Stromversorger",
);

/// Suggested merchant for a gas bill whose issuer is not recognised.
#[must_use]
pub const fn natural_gas_merchant(locale: Locale) -> &'static str {
    NATURAL_GAS_SUPPLIER.in_locale(locale)
}

/// Suggested merchant for an electricity bill whose issuer is not recognised.
#[must_use]
pub const fn electricity_supplier_merchant(locale: Locale) -> &'static str {
    ELECTRICITY_SUPPLIER.in_locale(locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALES: [Locale; 4] = [Locale::En, Locale::El, Locale::Fr, Locale::De];

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
    fn english_forms_are_the_ones_written_before_localization() {
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
        for locale in LOCALES {
            let mut generated = forms(locale);
            generated.push(opening_balance_description(locale, "Cash"));
            generated.push(void_description(locale, "Groceries"));
            generated.push(void_memo(locale).to_owned());
            generated.push(natural_gas_merchant(locale).to_owned());
            generated.push(electricity_supplier_merchant(locale).to_owned());

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
