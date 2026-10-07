//! The counterparty of a document and the description to suggest for it.
//!
//! [`find_merchant`] names who the document is from, or on a sales invoice
//! who it is to. [`find_description`] words the entry: a generated title for
//! a bill, a sales invoice or a referenced invoice, and otherwise the first
//! line of the document's item table.
//!
//! Neither is worded here. A name taken from the document is returned as
//! written; a generated name or title is returned as which one it is
//! ([`Merchant`], [`Description`]), and worded later in the language the
//! caller asks for, by [`crate::text`].

use std::ops::RangeInclusive;

use crate::documents::brands::{Service, classify_service, known_brand};
use crate::documents::invoice::kind::{
    CUSTOMER_BLOCK_LABEL, INVOICE_WORD_GREEK, POWER_BUSINESS_TARIFF, is_sales_invoice,
    is_utility_bill,
};
use crate::documents::invoice::normalization::contains_any;
use crate::documents::keyword::folded;
use crate::prefs::Locale;
use crate::text::{
    BillKind, bank_transfer_description, bill_description, customer_invoice_description,
    electricity_supplier_merchant, invoice_reference_description, invoice_word,
    natural_gas_merchant,
};

/// Fewest characters of a payee, issuer or customer name.
///
/// The reason for 3 is not recorded, and no test pins it.
pub(super) const MIN_NAME_CHARS: usize = 3;

/// The counterparty of a document: a name the document or the brand table
/// gives, or a generic supplier that has to be worded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Merchant {
    /// A name taken from the document or from the brand table. It is the
    /// same in every language.
    Named(String),
    /// The supplier of a natural gas bill that names no known brand.
    UnnamedGasSupplier,
    /// The supplier of an electricity bill that names no known brand.
    UnnamedElectricitySupplier,
}

impl Merchant {
    /// The name to show, worded in `locale` when it is a generic supplier.
    pub(crate) fn in_locale(&self, locale: Locale) -> &str {
        match self {
            Self::Named(name) => name,
            Self::UnnamedGasSupplier => natural_gas_merchant(locale),
            Self::UnnamedElectricitySupplier => electricity_supplier_merchant(locale),
        }
    }
}

/// What the suggested description of a document says.
///
/// Only [`LineItem`](Self::LineItem) holds text of the document. The others
/// name a title that [`Description::in_locale`] generates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Description {
    /// A sales invoice: the customer, the word "invoice" and the reference.
    CustomerInvoice,
    /// A bill for a service, with the merchant in front.
    Bill(BillKind),
    /// The first line of the document's item table, as written.
    LineItem(String),
    /// The word "invoice", the reference and the merchant.
    InvoiceReference,
    /// The word "invoice" alone.
    InvoiceWord,
    /// The merchant's name alone.
    MerchantName,
    /// A bank transfer, with the payee.
    BankTransfer,
}

impl Description {
    /// The description in `locale`, built around the `merchant` and
    /// `reference` of the same reading.
    ///
    /// `merchant` is already worded. `None` when the description needs a
    /// merchant or a reference and the reading has none; the reader does not
    /// produce such a pair.
    pub(crate) fn in_locale(
        &self,
        locale: Locale,
        merchant: Option<&str>,
        reference: Option<&str>,
    ) -> Option<String> {
        let description = match self {
            Self::CustomerInvoice => customer_invoice_description(locale, merchant?, reference),
            Self::Bill(kind) => bill_description(locale, *kind, merchant),
            Self::LineItem(words) => words.clone(),
            Self::InvoiceReference => invoice_reference_description(locale, reference?, merchant),
            Self::InvoiceWord => invoice_word(locale).to_owned(),
            Self::MerchantName => merchant?.to_owned(),
            Self::BankTransfer => bank_transfer_description(locale, merchant),
        };

        Some(description)
    }
}

/// The counterparty of a document. The first of these that yields a name:
///
/// 1. on a sales invoice, the customer ([`sales_invoice_customer`]). This
///    comes before brand recognition because the issuer's payment footer
///    often names a bank ("PIRAEUS BANK, IBAN ...") that must not win;
/// 2. a known biller ([`known_brand`]);
/// 3. on a utility bill of no known brand, a generic supplier: natural gas,
///    or electricity for a "Power Business" tariff;
/// 4. the first `Επωνυμία` (legal name) line: what follows its colon, or
///    else the line without the label words at its start. When the label is
///    neither followed by a colon nor at the start, the whole line is
///    returned, label included;
/// 5. the first line of [`MERCHANT_LINE_CHARS`] characters that has a letter
///    and does not hold the Greek word for "invoice".
///
/// `text` is normalized and `folded_text` is its folded form.
pub(super) fn find_merchant(text: &str, folded_text: &str) -> Option<Merchant> {
    if is_sales_invoice(folded_text)
        && let Some(customer) = sales_invoice_customer(text)
    {
        return Some(Merchant::Named(customer));
    }

    if let Some(brand) = known_brand(folded_text) {
        return Some(Merchant::Named(brand.name.to_owned()));
    }

    if is_utility_bill(folded_text) {
        if contains_any(folded_text, GAS_SUPPLY_MARKERS) {
            return Some(Merchant::UnnamedGasSupplier);
        }
        if folded_text.contains(POWER_BUSINESS_TARIFF) {
            return Some(Merchant::UnnamedElectricitySupplier);
        }
    }

    for line in text.lines() {
        if folded(line).contains(ISSUER_NAME_LABEL) {
            if let Some(name) = value_after_colon(line)
                && name.chars().count() >= MIN_NAME_CHARS
            {
                return Some(Merchant::Named(name));
            }
            // No colon: the name is what follows the label word.
            let cleaned = line
                .split_whitespace()
                .skip_while(|word| *word == ":" || folded(word).contains(ISSUER_NAME_STEM))
                .collect::<Vec<_>>()
                .join(" ");
            if cleaned.chars().count() >= MIN_NAME_CHARS {
                return Some(Merchant::Named(cleaned));
            }
        }
    }

    text.lines()
        .map(str::trim)
        .find(|line| {
            MERCHANT_LINE_CHARS.contains(&line.chars().count())
                && line.chars().any(char::is_alphabetic)
                && !folded(line).contains(INVOICE_WORD_GREEK)
        })
        .map(|line| Merchant::Named(line.to_owned()))
}

/// Lengths of a line that can stand in for the merchant's name.
///
/// The reasons for 5 and 80 are not recorded, and no test pins either.
const MERCHANT_LINE_CHARS: RangeInclusive<usize> = 5..=80;

/// Fewest characters of an unlabelled line taken as the customer's name.
///
/// The reason for 5 is not recorded, and no test pins it.
const MIN_CUSTOMER_LINE_CHARS: usize = 5;

/// Markers of a natural gas bill whose supplier is not a known brand.
pub(super) const GAS_SUPPLY_MARKERS: &[&str] = &[
    "φυσικου αεριου",
    "φυσικο αεριο",
    "gas simple",
    "προμηθεια φ.α",
];

/// The label of a legal name, on an issuer or a customer.
pub(super) const ISSUER_NAME_LABEL: &str = "επωνυμια";

/// What every inflection of [`ISSUER_NAME_LABEL`] starts with, to skip the
/// label word itself.
pub(super) const ISSUER_NAME_STEM: &str = "επων";

/// The English label of a name line in a customer block, at the line's
/// start.
pub(super) const NAME_LABEL: &str = "name";

/// Headings that open the customer block of a sales invoice.
pub(super) const CUSTOMER_BLOCK_STARTS: &[&str] = &[CUSTOMER_BLOCK_LABEL, "customer"];

/// Labels of the lines in a customer block that are not the customer's name.
pub(super) const TAX_ID_OR_ADDRESS_LABELS: &[&str] = &["α.φ.μ", "αφμ", "διευθυν"];

/// Column headings of the description column of a line-item table.
pub(super) const DESCRIPTION_HEADERS: &[&str] = &["περιγραφη", "description"];

/// Column headings that follow the description heading in a table header.
pub(super) const QUANTITY_HEADERS: &[&str] = &["ποσοτητα", "quantity"];

/// The customer's name from the customer block of a sales invoice.
///
/// After the block heading, the first line decides that is either a name
/// line (`Επωνυμία` or `Name`) with at least [`MIN_NAME_CHARS`] characters
/// after its colon, or a line that is no tax-id or address label, has a
/// letter, has at least [`MIN_CUSTOMER_LINE_CHARS`] characters and does not
/// end in a colon. `None` when the text has no customer block or the block
/// has no such line.
fn sales_invoice_customer(text: &str) -> Option<String> {
    let mut in_customer_block = false;

    for line in text.lines() {
        let folded_line = folded(line);

        if contains_any(&folded_line, CUSTOMER_BLOCK_STARTS) {
            in_customer_block = true;
            continue;
        }
        if !in_customer_block {
            continue;
        }

        if (folded_line.contains(ISSUER_NAME_LABEL) || folded_line.starts_with(NAME_LABEL))
            && let Some(name) = value_after_colon(line)
            && name.chars().count() >= MIN_NAME_CHARS
        {
            return Some(name);
        }

        if !contains_any(&folded_line, TAX_ID_OR_ADDRESS_LABELS)
            && line.chars().count() >= MIN_CUSTOMER_LINE_CHARS
            && line.chars().any(char::is_alphabetic)
            && !folded_line.ends_with(':')
        {
            return Some(line.trim().to_owned());
        }
    }

    None
}

/// The text after the first colon of `line`, trimmed. The full-width colon
/// `：` counts as one. `None` when there is no colon or nothing after it.
pub(super) fn value_after_colon(line: &str) -> Option<String> {
    let (_, value) = line.split_once([':', '：'])?;
    let value = value.trim();

    (!value.is_empty()).then(|| value.to_owned())
}

/// Fewest letters and spaces of a line-item description.
///
/// The reason for 4 is not recorded, and no test pins it.
const MIN_DESCRIPTION_CHARS: usize = 4;

/// The description to suggest. The first of these that applies:
///
/// 1. sales invoice with a customer: the customer invoice title;
/// 2. utility bill, or known biller whose brand implies a service: the bill
///    title for the service. The brand's service wins; otherwise
///    [`classify_service`] decides, and a bill whose service cannot be told
///    is a plain utility bill;
/// 3. the first line under a description heading that has at least
///    [`MIN_DESCRIPTION_CHARS`] letters and spaces once everything else is
///    removed, and is not the quantity heading of the same table row;
/// 4. with a reference: the invoice reference title;
/// 5. with the Greek word for "invoice" in the text: that word;
/// 6. with a merchant: the merchant's name.
///
/// `has_merchant` and `has_reference` say what the same reading found.
pub(super) fn find_description(
    text: &str,
    folded_text: &str,
    has_merchant: bool,
    has_reference: bool,
) -> Option<Description> {
    if is_sales_invoice(folded_text) && has_merchant {
        return Some(Description::CustomerInvoice);
    }

    let brand_service = known_brand(folded_text).and_then(|brand| brand.service);

    if is_utility_bill(folded_text) || brand_service.is_some() {
        let service = brand_service.or_else(|| classify_service(folded_text));
        let kind = service.map_or(BillKind::Utility, Service::bill_kind);

        return Some(Description::Bill(kind));
    }

    let mut after_header = false;
    for line in text.lines() {
        if contains_any(&folded(line), DESCRIPTION_HEADERS) {
            after_header = true;
            continue;
        }
        if after_header {
            let words: String = line
                .chars()
                .filter(|c| c.is_alphabetic() || c.is_whitespace())
                .collect();
            let words = words.trim();
            if words.chars().count() >= MIN_DESCRIPTION_CHARS
                && !contains_any(&folded(words), QUANTITY_HEADERS)
            {
                return Some(Description::LineItem(words.to_owned()));
            }
        }
    }

    if has_reference {
        return Some(Description::InvoiceReference);
    }
    if folded_text.contains(INVOICE_WORD_GREEK) {
        return Some(Description::InvoiceWord);
    }
    has_merchant.then_some(Description::MerchantName)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::analyze::EntryKindSuggestion;
    use crate::documents::analyze::parse_invoice_text;

    /// Loads a corpus fixture, so the unit tests read the same documents as
    /// the golden test in `tests/document_corpus.rs`.
    fn corpus_text(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/documents")
            .join(relative);
        std::fs::read_to_string(&path).expect("corpus fixture")
    }

    #[test]
    fn value_after_fullwidth_colon_does_not_panic() {
        assert_eq!(value_after_colon("Name：ACME LTD"), Some("ACME LTD".into()));
        assert_eq!(value_after_colon("Name: ACME LTD"), Some("ACME LTD".into()));
    }

    #[test]
    fn utility_titles_are_company_first() {
        // Gas bill whose issuer only appears via the MyON portal branding.
        let gas = parse_invoice_text(
            &corpus_text("synthetic/text/volton_myon_gas.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(gas.merchant.as_deref(), Some("Volton"));
        assert_eq!(gas.description.as_deref(), Some("Volton — Gas bill"));

        // Telecom bill: brand implies the service without utility markers.
        let telecom = parse_invoice_text(
            &corpus_text("synthetic/text/nova_telecom.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(telecom.merchant.as_deref(), Some("Nova"));
        assert_eq!(telecom.description.as_deref(), Some("Nova — Telecom bill"));
        assert_eq!(telecom.kind, EntryKindSuggestion::Bill);
    }

    #[test]
    fn electricity_supplier_beats_grid_operator_and_energy_mix() {
        // Every Greek electricity bill mentions ΔΕΔΔΗΕ (grid operator) and a
        // national energy-mix table that includes natural gas; neither may
        // decide the title.
        let suggestion = parse_invoice_text(
            &corpus_text("synthetic/text/zenith_supplier_vs_grid.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(suggestion.merchant.as_deref(), Some("ZeniΘ"));
        assert_eq!(
            suggestion.description.as_deref(),
            Some("ZeniΘ — Electricity bill")
        );
    }

    #[test]
    fn sales_invoice_titles_carry_the_customer() {
        let suggestion = parse_invoice_text(
            "Επωνυμία ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ\n\
             Τιμολόγιο Παροχής / Ενδοκοινοτική Παροχή Υπηρεσιών\n\
             900000000000001 Επί πιστώσειB 51 25/06/2026\n\
             Στοιχεία Πελάτη\nΑ.Φ.Μ.: 000000000\nΕπωνυμία: ACME CONSULTING LTD\n\
             Πληρωτέο (€): 1860,00",
            crate::prefs::Locale::En,
        );
        assert_eq!(suggestion.kind, EntryKindSuggestion::Income);
        assert_eq!(suggestion.merchant.as_deref(), Some("ACME CONSULTING LTD"));
        assert!(
            suggestion
                .description
                .as_deref()
                .is_some_and(|text| text.starts_with("ACME CONSULTING LTD — Invoice")),
            "description={:?}",
            suggestion.description
        );

        // The issuer's payment footer must not hijack the merchant.
        let with_bank = parse_invoice_text(
            "Τιμολόγιο Παροχής Υπηρεσιών\nΣτοιχεία Πελάτη\nΕπωνυμία: ACME CONSULTING LTD\n\
             Πληρωτέο (€): 500,00\nPIRAEUS BANK, GREECE, IBAN: GR0000000000000000000000000",
            crate::prefs::Locale::En,
        );
        assert_eq!(with_bank.merchant.as_deref(), Some("ACME CONSULTING LTD"));
    }
}
