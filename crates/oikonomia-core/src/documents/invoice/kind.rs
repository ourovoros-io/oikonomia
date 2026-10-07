//! What kind of document the text is: a utility bill, a sales invoice the
//! book's owner issued, or anything else, and whether it is still unpaid.
//!
//! [`classify_kind`] turns that into a [`DocumentClass`]. The two
//! tests it rests on, [`is_utility_bill`] and [`is_sales_invoice`], are also
//! what the total, the merchant and the description branch on, so the three
//! always agree about what the document is.

use crate::documents::analyze::EntryKindSuggestion;
use crate::documents::brands::known_brand;
use crate::documents::keyword::Keyword::{Prefix, Unit, Word};
use crate::documents::keyword::{Keyword, contains_any};

/// Strong utility markers only. Loose ones such as `ηλεκτρ` also match a
/// software company's line of business (`ΗΛΕΚΤΡΟΝΙΚΩΝ ΣΥΣΤΗΜΑΤΩΝ`).
pub(super) const UTILITY_MARKERS: &[Keyword] = &[
    Unit("kwh"),
    Prefix("ρευμα"),
    Prefix("εκκαθαριστικ"),
    Word("δεδδηε"),
    Unit("ηκασπ"),
    Word("φυσικου αεριου"),
    Prefix("φυσικο αεριο"),
    Word("προμηθεια φ.α"),
    Word("χρεωση προμηθειας φ.α"),
    Word("gas simple"),
    Word("myon"),
    Word("κωδικος παροχης"),
    Prefix("υδρευσ"),
    POWER_BUSINESS_TARIFF,
];

/// A business electricity tariff, printed on bills whose supplier is not
/// named in the text.
pub(super) const POWER_BUSINESS_TARIFF: Keyword = Word("power business");

/// The heading of the counterparty block on a sales invoice.
pub(super) const CUSTOMER_BLOCK_LABEL: Keyword = Prefix("στοιχεια πελατη");

/// The English heading of an invoice the book's owner issued.
pub(super) const SALES_INVOICE_WORDS: Keyword = Word("sales invoice");

/// The word "invoice", in Greek and in English.
pub(super) const INVOICE_WORDS: &[Keyword] = &[INVOICE_WORD_GREEK, Prefix("invoice")];

/// Wording that marks a utility bill as unpaid: overdue (`ληξιπρόθεσμ-`),
/// unpaid (`ανεξόφλητ-`), amount due.
///
/// Wording that every settlement bill prints, such as "pay by" and "pay
/// through", is not here; `settlement_bill_is_not_automatically_unpaid` pins
/// that such a bill is not marked unpaid.
pub(super) const UTILITY_UNPAID_MARKERS: &[Keyword] = &[
    Prefix("ληξιπροθεσμ"),
    Prefix("ανεξοφλητ"),
    Word("amount due"),
];

/// Wording of an invoice the book's owner received, which overrides the
/// signs of a sales invoice.
pub(super) const PURCHASE_MARKERS: &[Keyword] = &[
    Prefix("τιμολογιο αγορ"),
    Word("purchase invoice"),
    Prefix("supplier"),
];

/// Wording that marks a document other than a utility bill as unpaid. `επι
/// πιστωσει` is "on credit", the payment method of an invoice not yet paid;
/// it is a stem because extraction glues it to the series code that follows
/// (`Επί πιστώσειB 51`).
pub(super) const UNPAID_MARKERS: &[Keyword] = &[
    Prefix("επι πιστωσει"),
    Word("amount due"),
    Word("unpaid"),
    Word("outstanding"),
    Word("please pay"),
];

/// Whether folded text carries one of [`UTILITY_MARKERS`].
pub(super) fn is_utility_bill(folded_text: &str) -> bool {
    contains_any(folded_text, UTILITY_MARKERS)
}

/// Whether folded text is a sales invoice issued by the book's owner: it has
/// the customer block heading "Στοιχεία Πελάτη" and the word "invoice".
pub(super) fn is_sales_invoice(folded_text: &str) -> bool {
    CUSTOMER_BLOCK_LABEL.occurs_in(folded_text) && contains_any(folded_text, INVOICE_WORDS)
}

/// What a document is for the books: the entry kind it suggests and, where
/// that can be said, whether it is still to be paid.
///
/// An expense is never unpaid: an unpaid purchase is a [`Bill`](Self::Bill).
/// Income can be: a sales invoice issued on credit is money the customer
/// still owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentClass {
    /// Money spent, with nothing left to pay.
    Expense,
    /// Money received or to be received, from a sales invoice.
    Income {
        /// The invoice is on credit terms or shows an amount due.
        unpaid: bool,
    },
    /// A bill to pay.
    Bill {
        /// The bill is overdue, on credit terms or shows an amount due.
        unpaid: bool,
    },
}

impl DocumentClass {
    /// The entry kind the suggestion carries.
    pub(crate) const fn kind(self) -> EntryKindSuggestion {
        match self {
            Self::Expense => EntryKindSuggestion::Expense,
            Self::Income { .. } => EntryKindSuggestion::Income,
            Self::Bill { .. } => EntryKindSuggestion::Bill,
        }
    }

    /// Whether the document is still to be paid.
    pub(crate) const fn is_unpaid(self) -> bool {
        match self {
            Self::Expense => false,
            Self::Income { unpaid } | Self::Bill { unpaid } => unpaid,
        }
    }
}

/// Classifies a document from its folded text. The first rule that applies
/// wins:
///
/// 1. A utility bill is a `Bill`, unpaid when it carries one of
///    [`UTILITY_UNPAID_MARKERS`].
/// 2. A sales invoice (see [`is_sales_invoice`], or the words "sales
///    invoice") without a purchase marker is `Income`.
/// 3. A known biller whose brand implies a service is a `Bill`.
/// 4. Anything else with an unpaid marker is an unpaid `Bill`.
/// 5. The rest is an `Expense`.
///
/// In rules 2 and 3 the document is unpaid when it carries one of
/// [`UNPAID_MARKERS`].
pub(super) fn classify_kind(folded_text: &str) -> DocumentClass {
    if is_utility_bill(folded_text) {
        let unpaid = contains_any(folded_text, UTILITY_UNPAID_MARKERS);
        return DocumentClass::Bill { unpaid };
    }

    // "Σταθερό Τιμολόγιο" is a tariff name, not a sales invoice.
    let sales = is_sales_invoice(folded_text) || SALES_INVOICE_WORDS.occurs_in(folded_text);
    let purchase = contains_any(folded_text, PURCHASE_MARKERS);
    let unpaid = contains_any(folded_text, UNPAID_MARKERS);

    if sales && !purchase {
        return DocumentClass::Income { unpaid };
    }

    // A recognized biller with a known service (telecom etc.) is a bill to
    // pay even without the utility markers above.
    let billed_service = known_brand(folded_text).and_then(|brand| brand.service);
    if billed_service.is_some() || unpaid {
        return DocumentClass::Bill { unpaid };
    }

    DocumentClass::Expense
}

/// "Invoice" in Greek. Also part of tariff names ("Σταθερό Τιμολόγιο"), so
/// on its own it does not make a document an invoice.
pub(super) const INVOICE_WORD_GREEK: Keyword = Prefix("τιμολογιο");

#[cfg(test)]
mod tests {
    use super::*;
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
    fn received_service_invoice_is_not_income() {
        let text = "\
Τιμολόγιο Παροχής Υπηρεσιών
Επωνυμία: ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ
Α.Φ.Μ.: 000000000
Πληρωτέο (€): 200,00
";
        let suggestion = parse_invoice_text(text, crate::prefs::Locale::En);
        assert_ne!(
            suggestion.kind,
            EntryKindSuggestion::Income,
            "kind={:?}",
            suggestion.kind
        );
    }

    #[test]
    fn settlement_bill_is_not_automatically_unpaid() {
        let suggestion = parse_invoice_text(
            &corpus_text("synthetic/text/dei_settlement.txt"),
            crate::prefs::Locale::En,
        );
        assert!(
            !suggestion.bill_unpaid,
            "εμπρόθεσμο/εκκαθαριστικό/εξόφληση μέσω must not force unpaid"
        );
    }

    #[test]
    fn cosmote_pay_via_is_not_unpaid() {
        let suggestion = parse_invoice_text(
            &corpus_text("synthetic/text/cosmote_pay_via.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(suggestion.kind, EntryKindSuggestion::Bill);
        assert!(
            !suggestion.bill_unpaid,
            "known-brand εξόφληση μέσω must not force unpaid: {suggestion:?}"
        );
    }
}
