//! The invoice reader: from the text of a bill, receipt or invoice to the
//! fields of a draft entry.
//!
//! The reader is a set of rules over lines of text, tuned for Greek and other
//! European tax documents. It uses no network and no model, and it does not
//! know the book: accounts are chosen later, by the analyzer.
//! [`read_invoice_text`] is the entry point, and an [`InvoiceReading`] what
//! it returns: the fields in no language, for the analyzer to word.
//!
//! # Steps
//!
//! 1. The text is normalized ([`normalize`]).
//! 2. A folded copy is made ([`folded`]): lowercase, Greek accents removed.
//!    Every label and marker constant in this module is written in that
//!    form, and a test checks that each one is.
//! 3. A Greek bank transfer receipt (`έμβασμα`) is recognized by its wording
//!    and read by rules of its own ([`parse_bank_transfer`]): the labelled
//!    principal is the amount, never the fee and never a clock time on the
//!    value-date line.
//! 4. For any other document each field is found on its own: total, date,
//!    reference, merchant, description and kind.
//! 5. The reader's notes are added. The confidence is computed from what
//!    was found ([`InvoiceReading::confidence`]).
//!
//! # Normalizing
//!
//! [`normalize`] makes four changes, in this order:
//!
//! 1. `\r` becomes a line break, and no-break, narrow no-break and thin
//!    spaces become plain spaces.
//! 2. A standalone `EUR`, in any letter case, becomes `€`.
//! 3. The pieces of a number that extraction split with a space are joined:
//!    `72, 53` becomes `72,53` and `1 234,56` becomes `1234,56`
//!    ([`collapse_spaced_decimals`]).
//! 4. Lines are trimmed and empty lines dropped.
//!
//! This runs before any masking or reading because everything after it works
//! one line at a time and takes a number to be one unbroken run of digits,
//! `,` and `.`. A number left in two pieces would be read as two amounts.
//! The currency step comes before the joining step so that in `EUR1 234,56`
//! the amount starts at a symbol and not at the tail of a word: a digit that
//! follows a letter is taken as the end of a code, and never starts a
//! space-grouped number.
//!
//! The joining step blanks dates and clock times on a private copy of the
//! line first, so the year of `13/08/2026` is never joined to an amount that
//! follows it.
//!
//! # Reading an amount
//!
//! On each line dates (`13/08/2026`, `2026-08-13`), clock times (`7:00`) and
//! percentages (`24%`) are blanked first ([`money_amounts_on_line`]). What is
//! left is cut into money tokens.
//!
//! A money token is a run of digits, `,` and `.`. Trailing separators are
//! sentence punctuation and are dropped first. Then:
//!
//! | Separators in the token | Reading | Example |
//! |-------------------------|---------|---------|
//! | none, up to five digits | whole euros | `50` is 50,00 |
//! | both `,` and `.` | the later one is the decimal mark | `1.234,56`, `1,234.56` |
//! | one kind, once, before 1 or 2 digits | decimal mark | `45,9`, `45.90` |
//! | one kind, once, before 3 digits | thousands | `1.234`, `1,234` |
//! | one kind, several times | thousands | `1.234.567` |
//! | one kind, once, before 4 or more digits | not money | `1,2345` |
//!
//! When both kinds appear, the one that is not the decimal mark groups
//! thousands. Thousands groups must be well formed: one to three leading
//! digits that do not start with zero, then groups of exactly three. So
//! `0,085` and `0.971` (a unit price, a conversion factor) are not money, and
//! neither is `1.2.3`. A fraction has one or two digits, and the whole part
//! at most eight.
//!
//! Digits-only tokens with a leading zero, of more than five digits, or in
//! 1900..=2100 are date fragments, identifiers and years, not money.
//!
//! Amounts are in minor units of a two-decimal currency throughout. The
//! analyzer withholds the amount for a book whose currency is not one.
//!
//! ## The plausibility band
//!
//! An amount counts only between 0,50 and 10 000 000,00
//! ([`PLAUSIBLE_MONEY_MINOR`]). Smaller numbers on a bill are taken as unit
//! prices and rates, and larger ones as identifiers that got past the token
//! rules.
//!
//! ## Years
//!
//! A token of digits only that reads as 1900 to 2100 is taken as the year of
//! a date the masks did not recognize ([`YEAR_LIKE_EUROS`]). A date itself
//! has a year of 1990 to 2100 ([`DOCUMENT_YEARS`]); a two-digit year is
//! counted from 2000.
//!
//! # Choosing the total
//!
//! [`find_total_amount`] tries four stages in order and returns the first
//! amount one of them yields. Every stage skips noise lines
//! ([`is_noise_amount_line`]): deposits, rate, volume, area and energy-mix
//! lines, and lines with a percentage that do not name a total.
//!
//! 1. **Labelled total** ([`find_labeled_total`]). The first line, top to
//!    bottom, that carries a label naming the amount to pay
//!    ([`TOTAL_LABELS`]) and has an amount on it or on the line below. It
//!    prefers what the document itself calls the total, and of several
//!    amounts beside the label the largest.
//! 2. **Utility vote** ([`find_utility_payment_total`]), for utility bills
//!    only. Every amount collects points from each line it is printed on.
//!    It prefers an amount with cents, on a short line with a `€`, beside a
//!    payment label, printed more than once.
//! 3. **Totals row**. The first line with the word `σύνολα` or `totals`
//!    that has a plausible amount. It prefers the largest amount on that
//!    row, since the row lists net, VAT and gross.
//! 4. **Weighted fallback**. Every plausible amount on every line that is
//!    not an identifier line (IBAN, tax number, MARK). It prefers a line
//!    with a value word (`αξία`, `value`, `total`) and a `€`, then an amount
//!    with cents, and of equals the largest.
//!
//! The corpus exercises the stages unevenly. Stage 2 decides the fixture
//! `zenith_electricity.txt`, and stage 4 decides `english_total.pdf` and
//! `english_total.jpg`. No test fails when stage 3 is removed.
//!
//! # The reference
//!
//! [`find_invoice_reference`] takes the first of these that the text has:
//!
//! 1. a supply or meter code beside its label;
//! 2. an `NGS` supply code anywhere;
//! 3. an RF payment code;
//! 4. a MARK number, on the MARK line or the one below;
//! 5. a number of 6 to 20 digits on a line that names an invoice or
//!    reference number;
//! 6. the longest number of 10 to 20 digits anywhere.
//!
//! The first four are codes with a shape or a label of their own. The last
//! two are bare numbers, and the last of all is a guess, so they come after.
//! Why a supply code comes before a payment code, and both before a MARK,
//! is not recorded; the `reference_*` unit tests pin each step.
//!
//! # Known tradeoffs
//!
//! The rules misread some inputs, on purpose or for lack of a better rule.
//! Each of these is pinned by a test in `documented_tradeoffs` or named
//! below:
//!
//! - **A lone digit before a three-digit group reads as thousands.**
//!   `5 120,50` is spelled like `1 234,56`, so a quantity column directly
//!   before a three-digit price reads as one amount, 5 120,50. A labelled
//!   total on the document still decides
//!   (`a_lone_digit_before_three_digits_reads_as_thousands` in
//!   `amounts_and_dates`).
//! - **A space before the separator is not joined.** `72, 53` is joined;
//!   `72 ,53` is not, and reads as 72,00.
//! - **Labels are matched as substrings.** `subtotal` contains `total`, so
//!   [`names_a_total`] holds for a subtotal line: such a line is not skipped
//!   for carrying a percentage, and it gets the value-word weight in the
//!   fallback. For the same reason `Subtotal amount` carries the label
//!   `total amount` and can decide stage 1.
//! - **An identifier marker inside a word hides its line from the
//!   fallback.** `mark` (the MARK number) is in `supermarket`, so an amount
//!   on a line with that word is found only by a labelled total.
//! - **Leading zeros in a whole part are accepted beside a decimal mark.**
//!   `01,50` reads as 1,50, although a bare `08` is not money and `01.234`
//!   is not a thousands amount.
//! - **A whole amount of 1900 to 2100 euros needs decimals.** `2026` is a
//!   year; `2026,00` is an amount.
//! - **Any zero percentage counts as a zero VAT rate.** The VAT-exempt note
//!   is added for `0%` anywhere in the text, beside a VAT label or not.
//! - **Dates are day first.** `03/04/2026` is 3 April. A month-first date
//!   is misread, or refused when its "month" is over 12.
//!
//! [`DOCUMENT_YEARS`]: dates::DOCUMENT_YEARS
//! [`PLAUSIBLE_MONEY_MINOR`]: money::PLAUSIBLE_MONEY_MINOR
//! [`TOTAL_LABELS`]: total::TOTAL_LABELS
//! [`YEAR_LIKE_EUROS`]: money::YEAR_LIKE_EUROS
//! [`collapse_spaced_decimals`]: normalization::collapse_spaced_decimals
//! [`find_labeled_total`]: total::find_labeled_total
//! [`find_utility_payment_total`]: total::find_utility_payment_total
//! [`is_noise_amount_line`]: total::is_noise_amount_line
//! [`money_amounts_on_line`]: money::money_amounts_on_line
//! [`names_a_total`]: total::names_a_total

use time::Date;

use crate::documents::invoice::dates::find_best_date;
use crate::documents::invoice::kind::{DocumentClass, classify_kind, is_utility_bill};
use crate::documents::invoice::merchant::{Description, Merchant, find_description, find_merchant};
use crate::documents::invoice::normalization::{contains_any, normalize};
use crate::documents::invoice::reference::find_invoice_reference;
use crate::documents::invoice::total::find_total_amount;
use crate::documents::invoice::transfer::{is_bank_transfer_receipt, parse_bank_transfer};
use crate::prefs::Locale;
use crate::ui_text::{UiText, UiTextCode};

mod dates;
mod kind;
mod merchant;
mod money;
mod normalization;
mod reference;
mod total;
mod transfer;

pub(super) use crate::documents::invoice::normalization::folded;

/// What the invoice reader found in a document.
///
/// Nothing in it is worded. The reader knows neither the book nor the
/// language of the application: a name or a line taken from the document is
/// held as written, and a generated name or title is held as which one it is.
/// [`merchant_in`](Self::merchant_in) and
/// [`description_in`](Self::description_in) word them, and the analyzer turns
/// the reading into the suggestion the UI receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InvoiceReading {
    /// The total, in minor units of a two-decimal currency (cents).
    pub amount_minor: Option<i64>,
    /// The date to post the entry on.
    pub entry_date: Option<Date>,
    /// The document's reference, as written.
    pub reference: Option<String>,
    /// Who the document is from, or on a sales invoice who it is to.
    pub merchant: Option<Merchant>,
    /// What the suggested description says.
    pub description: Option<Description>,
    /// The entry kind the document suggests, and whether it is still to be
    /// paid.
    pub class: DocumentClass,
    /// The fee a bank transfer receipt shows, in 2-decimal minor units (cents).
    ///
    /// It is data, not a note: the note needs the book's currency, which the
    /// reader does not know, so the analyzer builds it in one place.
    pub transfer_fee_minor: Option<i64>,
    /// The reader's own notes, in the order the user reads them. None of
    /// them mentions a transfer fee.
    pub notes: Vec<UiText>,
}

impl InvoiceReading {
    /// The merchant's name, with a generic supplier worded in `locale`.
    pub(crate) fn merchant_in(&self, locale: Locale) -> Option<&str> {
        self.merchant
            .as_ref()
            .map(|merchant| merchant.in_locale(locale))
    }

    /// The description, with a generated title worded in `locale`.
    pub(crate) fn description_in(&self, locale: Locale) -> Option<String> {
        self.description.as_ref()?.in_locale(
            locale,
            self.merchant_in(locale),
            self.reference.as_deref(),
        )
    }

    /// The words the suggested category is chosen from: the merchant and the
    /// description, always worded in English.
    ///
    /// The category must not depend on the language of the application, so
    /// it is matched against this text and never against the worded fields
    /// of the suggestion. Words taken from the document itself are the same
    /// in both.
    pub(crate) fn category_hint(&self) -> String {
        format!(
            "{} {}",
            self.merchant_in(Locale::En).unwrap_or(""),
            self.description_in(Locale::En).as_deref().unwrap_or("")
        )
    }

    /// A rough confidence between 0 and 1: the sum of the [`confidence`]
    /// parts for what was found, capped at the ceiling.
    pub(crate) fn confidence(&self) -> f32 {
        let mut score = confidence::BASE;
        if self.amount_minor.is_some() {
            score += confidence::AMOUNT;
        }
        if self.entry_date.is_some() {
            score += confidence::DATE;
        }
        if self.reference.is_some() {
            score += confidence::REFERENCE;
        }
        if !matches!(self.class, DocumentClass::Expense) {
            score += confidence::CLASSIFIED_KIND;
        }
        score.min(confidence::CEILING)
    }
}

/// Reads extracted document text.
///
/// The text is normalized and folded here, once. A bank transfer receipt is
/// handed to [`parse_bank_transfer`]: its amount is the capital debit
/// (`Ποσό Χρέωσης Κεφαλαίου` or `Ποσό:`), not the fee and not an `hh:mm`
/// time, and it stays an expense. Any other document has each field found on
/// its own.
pub(crate) fn read_invoice_text(text: &str) -> InvoiceReading {
    let normalized = normalize(text);
    let folded_full = folded(&normalized);
    if is_bank_transfer_receipt(&folded_full) {
        return parse_bank_transfer(&normalized);
    }

    let amount_minor = find_total_amount(&normalized, &folded_full);
    let reference = find_invoice_reference(&normalized);
    let merchant = find_merchant(&normalized, &folded_full);
    let description = find_description(
        &normalized,
        &folded_full,
        merchant.is_some(),
        reference.is_some(),
    );
    let class = classify_kind(&folded_full);

    InvoiceReading {
        amount_minor,
        entry_date: find_best_date(&normalized),
        reference,
        merchant,
        description,
        class,
        transfer_fee_minor: None,
        notes: build_notes(amount_minor, class, &folded_full),
    }
}

/// Wording of a document that states it carries no VAT ("without VAT"). A
/// rate of zero says the same; [`states_a_zero_rate`] finds that.
const VAT_EXEMPT_MARKERS: &[&str] = &["χωρις φπα"];

/// The parts of the confidence that [`InvoiceReading::confidence`] adds up.
///
/// The figure is a rough guide for the user, not a probability. An amount
/// weighs most, then a date, then a reference. The parts sum to 0.9, under
/// [`CEILING`](confidence::CEILING), so the ceiling does not bind; it states
/// that a heuristic reading is never reported as certain.
///
/// Why each part has the value it has is not recorded, and no test checks
/// the figure.
mod confidence {
    /// A document that was read at all.
    pub(super) const BASE: f32 = 0.2;
    /// An amount was found.
    pub(super) const AMOUNT: f32 = 0.4;
    /// A date was found.
    pub(super) const DATE: f32 = 0.15;
    /// A reference was found.
    pub(super) const REFERENCE: f32 = 0.1;
    /// The document was classified as income or as a bill.
    pub(super) const CLASSIFIED_KIND: f32 = 0.05;
    /// The most a heuristic reading claims.
    pub(super) const CEILING: f32 = 0.95;
}

/// The reader's notes for a document that is not a transfer receipt, in the
/// order the user reads them: parsed, no total found, income, utility bill,
/// unpaid, VAT-exempt. Each but the first is added only when it applies.
fn build_notes(amount: Option<i64>, class: DocumentClass, folded_text: &str) -> Vec<UiText> {
    let mut notes = vec![UiText::new(UiTextCode::InvoiceParsed)];

    if amount.is_none() {
        notes.push(UiText::new(UiTextCode::InvoiceNoTotal));
    }
    if matches!(class, DocumentClass::Income { .. }) {
        notes.push(UiText::new(UiTextCode::InvoiceIncome));
    }
    if is_utility_bill(folded_text) {
        notes.push(UiText::new(UiTextCode::InvoiceUtility));
    }
    if class.is_unpaid() {
        notes.push(UiText::new(UiTextCode::InvoiceUnpaid));
    }
    if contains_any(folded_text, VAT_EXEMPT_MARKERS) || states_a_zero_rate(folded_text) {
        notes.push(UiText::new(UiTextCode::InvoiceVatExempt));
    }

    notes
}

/// Whether `text` holds a percentage whose number is zero: `0%`, `0,0%`,
/// `0.00%`.
///
/// The number is every digit, `,` and `.` directly before the `%` sign, so
/// the `0%` that ends `10%`, `20%` or `10,0%` is part of a larger number and
/// does not count. A space between the number and the sign is not skipped:
/// `0 %` is not read as a rate here.
///
/// Any percentage counts, not only one beside a VAT label: a discount of
/// `0%` is taken as a zero rate too.
fn states_a_zero_rate(text: &str) -> bool {
    let is_number_char = |c: &char| c.is_ascii_digit() || matches!(c, ',' | '.');

    // Every piece but the last is the text before one `%` sign.
    let mut before_each_sign = text.split('%');
    before_each_sign.next_back();

    before_each_sign.any(|before| {
        let mut digits = before
            .chars()
            .rev()
            .take_while(is_number_char)
            .filter(char::is_ascii_digit)
            .peekable();

        digits.peek().is_some() && digits.all(|digit| digit == '0')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::analyze::{EntryKindSuggestion, parse_invoice_text};
    use crate::documents::invoice::dates::{DATE_LABELS, VALUE_DATE_LABELS};
    use crate::documents::invoice::kind::{
        CUSTOMER_BLOCK_LABEL, INVOICE_WORD_GREEK, INVOICE_WORDS, POWER_BUSINESS_TARIFF,
        PURCHASE_MARKERS, SALES_INVOICE_WORDS, UNPAID_MARKERS, UTILITY_MARKERS,
        UTILITY_UNPAID_MARKERS,
    };
    use crate::documents::invoice::merchant::{
        CUSTOMER_BLOCK_STARTS, DESCRIPTION_HEADERS, GAS_SUPPLY_MARKERS, ISSUER_NAME_LABEL,
        ISSUER_NAME_STEM, NAME_LABEL, QUANTITY_HEADERS, TAX_ID_OR_ADDRESS_LABELS,
    };
    use crate::documents::invoice::reference::{MARK_LABELS, REFERENCE_LABELS, SUPPLY_CODE_LABELS};
    use crate::documents::invoice::total::{
        CONSUMPTION_LABEL, DEPOSIT_LABELS, IBAN_WORD, IDENTIFIER_LINE_MARKERS, PAYMENT_LABELS,
        RATE_LINE_MARKERS, TOTAL_LABELS, TOTAL_WORD, TOTALS_ROW_LABELS, VALUE_WORDS,
    };
    use crate::documents::invoice::transfer::{
        BANK_WORDS, BENEFICIARY_LABEL, BENEFICIARY_NAME_LABELS, EXECUTION_DATE_LABELS,
        TRANSFER_AMOUNT_LABEL, TRANSFER_DATE_LABELS, TRANSFER_FEE_LABELS, TRANSFER_MARKERS,
        TRANSFER_PRINCIPAL_LABEL, TRANSFER_REFERENCE_LABEL,
    };
    use time::macros::date;

    /// Every label and marker constant of the reader. A constant added to
    /// the module has to be added here to be checked.
    const LABEL_SETS: &[(&str, &[&str])] = &[
        ("UTILITY_MARKERS", UTILITY_MARKERS),
        ("POWER_BUSINESS_TARIFF", &[POWER_BUSINESS_TARIFF]),
        ("CUSTOMER_BLOCK_LABEL", &[CUSTOMER_BLOCK_LABEL]),
        ("SALES_INVOICE_WORDS", &[SALES_INVOICE_WORDS]),
        ("IBAN_WORD", &[IBAN_WORD]),
        ("NAME_LABEL", &[NAME_LABEL]),
        ("INVOICE_WORDS", INVOICE_WORDS),
        ("UTILITY_UNPAID_MARKERS", UTILITY_UNPAID_MARKERS),
        ("PURCHASE_MARKERS", PURCHASE_MARKERS),
        ("UNPAID_MARKERS", UNPAID_MARKERS),
        ("TRANSFER_MARKERS", TRANSFER_MARKERS),
        ("TRANSFER_REFERENCE_LABEL", &[TRANSFER_REFERENCE_LABEL]),
        ("TRANSFER_PRINCIPAL_LABEL", &[TRANSFER_PRINCIPAL_LABEL]),
        ("TRANSFER_AMOUNT_LABEL", &[TRANSFER_AMOUNT_LABEL]),
        ("TRANSFER_FEE_LABELS", TRANSFER_FEE_LABELS),
        ("BENEFICIARY_LABEL", &[BENEFICIARY_LABEL]),
        ("BENEFICIARY_NAME_LABELS", &BENEFICIARY_NAME_LABELS),
        ("BANK_WORDS", BANK_WORDS),
        ("EXECUTION_DATE_LABELS", EXECUTION_DATE_LABELS),
        ("TRANSFER_DATE_LABELS", TRANSFER_DATE_LABELS),
        ("TOTALS_ROW_LABELS", TOTALS_ROW_LABELS),
        ("VALUE_WORDS", VALUE_WORDS),
        ("TOTAL_WORD", &[TOTAL_WORD]),
        ("IDENTIFIER_LINE_MARKERS", IDENTIFIER_LINE_MARKERS),
        ("TOTAL_LABELS", TOTAL_LABELS),
        ("PAYMENT_LABELS", PAYMENT_LABELS),
        ("DEPOSIT_LABELS", DEPOSIT_LABELS),
        ("RATE_LINE_MARKERS", RATE_LINE_MARKERS),
        ("CONSUMPTION_LABEL", &[CONSUMPTION_LABEL]),
        ("VALUE_DATE_LABELS", VALUE_DATE_LABELS),
        ("DATE_LABELS", DATE_LABELS),
        ("SUPPLY_CODE_LABELS", SUPPLY_CODE_LABELS),
        ("MARK_LABELS", MARK_LABELS),
        ("REFERENCE_LABELS", REFERENCE_LABELS),
        ("GAS_SUPPLY_MARKERS", GAS_SUPPLY_MARKERS),
        ("ISSUER_NAME_LABEL", &[ISSUER_NAME_LABEL]),
        ("ISSUER_NAME_STEM", &[ISSUER_NAME_STEM]),
        ("INVOICE_WORD_GREEK", &[INVOICE_WORD_GREEK]),
        ("CUSTOMER_BLOCK_STARTS", CUSTOMER_BLOCK_STARTS),
        ("TAX_ID_OR_ADDRESS_LABELS", TAX_ID_OR_ADDRESS_LABELS),
        ("DESCRIPTION_HEADERS", DESCRIPTION_HEADERS),
        ("QUANTITY_HEADERS", QUANTITY_HEADERS),
        ("VAT_EXEMPT_MARKERS", VAT_EXEMPT_MARKERS),
    ];

    #[test]
    fn every_label_and_marker_is_in_folded_form() {
        for (name, needles) in LABEL_SETS {
            for needle in *needles {
                assert_eq!(
                    folded(needle),
                    *needle,
                    "{name}: {needle:?} can never match folded text"
                );
            }
        }
    }

    /// Loads a corpus fixture, so the unit tests read the same documents as
    /// the golden test in `tests/document_corpus.rs`.
    fn corpus_text(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/documents")
            .join(relative);
        std::fs::read_to_string(&path).expect("corpus fixture")
    }

    #[test]
    fn a_greek_sales_invoice_reads_as_unpaid_income_from_its_customer() {
        let suggestion = parse_invoice_text(
            &corpus_text("synthetic/text/greek_sales_invoice.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(suggestion.amount_minor, Some(186_000), "expected €1860.00");
        assert_eq!(suggestion.kind, EntryKindSuggestion::Income);
        assert_eq!(suggestion.entry_date, Some(date!(2026 - 06 - 25)));
        assert_eq!(suggestion.reference.as_deref(), Some("900000000000001"));
        assert!(
            suggestion
                .merchant
                .as_deref()
                .is_some_and(|merchant| merchant.contains("ACME CONSULTING")),
            "merchant={:?}",
            suggestion.merchant
        );
        assert!(
            suggestion.bill_unpaid,
            "Επί πιστώσει should mark unpaid/credit"
        );
    }

    #[test]
    fn a_plain_invoice_without_a_total_asks_for_the_amount() {
        let suggestion = parse_invoice_text("Thank you for your visit", crate::prefs::Locale::En);

        assert_eq!(
            suggestion.notes,
            [
                UiText::new(UiTextCode::InvoiceParsed),
                UiText::new(UiTextCode::InvoiceNoTotal),
            ]
        );
    }

    #[test]
    fn a_utility_bill_is_noted_as_one() {
        let suggestion = parse_invoice_text(
            &corpus_text("synthetic/text/ngs_gas_bill.txt"),
            crate::prefs::Locale::En,
        );

        assert!(
            suggestion
                .notes
                .contains(&UiText::new(UiTextCode::InvoiceUtility)),
            "{:?}",
            suggestion.notes
        );
    }

    #[test]
    fn a_sales_invoice_is_noted_as_income() {
        let notes = build_notes(Some(1000), DocumentClass::Income { unpaid: false }, "");

        assert_eq!(
            notes,
            [
                UiText::new(UiTextCode::InvoiceParsed),
                UiText::new(UiTextCode::InvoiceIncome),
            ]
        );
    }

    #[test]
    fn credit_terms_and_zero_vat_each_add_their_note() {
        let notes = build_notes(
            Some(1000),
            DocumentClass::Bill { unpaid: true },
            &folded("Χωρίς ΦΠΑ"),
        );

        assert_eq!(
            notes,
            [
                UiText::new(UiTextCode::InvoiceParsed),
                UiText::new(UiTextCode::InvoiceUnpaid),
                UiText::new(UiTextCode::InvoiceVatExempt),
            ]
        );
    }

    /// Whether `text` gets the note that the document carries no VAT.
    fn is_noted_vat_exempt(text: &str) -> bool {
        build_notes(Some(1000), DocumentClass::Expense, &folded(text))
            .contains(&UiText::new(UiTextCode::InvoiceVatExempt))
    }

    #[test]
    fn a_zero_vat_rate_adds_the_vat_exempt_note() {
        for text in [
            "VAT 0%",
            "VAT 0% 0,00",
            "ΦΠΑ 0%",
            "Φ.Π.Α.0%",
            "VAT (0%)",
            "0% VAT",
            "ΦΠΑ 0,0%",
            "VAT 0.00%",
            "Χωρίς ΦΠΑ",
            "ΧΩΡΙΣ ΦΠΑ",
        ] {
            assert!(is_noted_vat_exempt(text), "{text:?}");
        }
    }

    #[test]
    fn a_rate_that_only_ends_in_zero_adds_no_vat_exempt_note() {
        for text in [
            "VAT 10%",
            "VAT 20%",
            "ΦΠΑ 10%",
            "ΦΠΑ 20% 24,00",
            "VAT 10,0%",
            "VAT 20.0%",
            "Discount 100%",
            "Λιγνίτης 30%",
            "VAT 24%",
            "no rate at all",
            "%",
            ",%",
        ] {
            assert!(!is_noted_vat_exempt(text), "{text:?}");
        }
    }

    #[test]
    fn the_category_hint_is_worded_in_english() {
        let reading = read_invoice_text(&corpus_text("synthetic/text/dei_settlement.txt"));

        assert_eq!(reading.category_hint(), "ΔΕΗ ΔΕΗ — Electricity bill");
        assert_eq!(
            reading.description_in(crate::prefs::Locale::El).as_deref(),
            Some("ΔΕΗ — Λογαριασμός ρεύματος"),
            "the description itself follows the language"
        );
    }

    #[test]
    fn a_generic_supplier_is_worded_only_when_asked_for_a_language() {
        let reading = read_invoice_text("Λογαριασμός\nΠρομήθεια φυσικού αερίου\nΣΥΝΟΛΟ 45,90 EUR");

        assert_eq!(reading.merchant, Some(Merchant::UnnamedGasSupplier));
        assert_eq!(
            reading.merchant_in(crate::prefs::Locale::En),
            Some("Natural gas")
        );
        assert_eq!(
            reading.merchant_in(crate::prefs::Locale::De),
            Some("Erdgas")
        );
        assert!(
            reading.category_hint().starts_with("Natural gas "),
            "{:?}",
            reading.category_hint()
        );
    }
}

#[cfg(test)]
mod amounts_and_dates {
    use super::*;
    use crate::documents::analyze::{DocumentSuggestion, EntryKindSuggestion, parse_invoice_text};
    use time::macros::date;

    /// Synthetic jumbled layout (the shape `pdf_extract` produces on a
    /// text-layer utility PDF). Placeholders only — not a live dump.
    #[test]
    fn a_jumbled_gas_bill_extract_reads_like_the_tidy_one() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ngs_gas_jumbled_extract.txt"
        ))
        .expect("fixture extract");
        let suggestion = parse_invoice_text(&text, crate::prefs::Locale::En);
        assert_eq!(
            suggestion.amount_minor,
            Some(7_253),
            "expected €72.53 not date/deposit noise, got {:?}",
            suggestion.amount_minor
        );
        assert_ne!(suggestion.kind, EntryKindSuggestion::Income);
        assert_eq!(suggestion.entry_date, Some(date!(2026 - 08 - 13)));
        assert!(
            suggestion.reference.as_deref().is_some_and(|reference| {
                reference.contains("NGS")
                    || reference.contains("SYN")
                    || reference.starts_with("RF")
            }),
            "reference={:?}",
            suggestion.reference
        );
        assert_eq!(
            suggestion.merchant.as_deref(),
            Some("Volton"),
            "MyON portal branding identifies the supplier"
        );
        assert_eq!(suggestion.description.as_deref(), Some("Volton — Gas bill"));
    }

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn all_caps_greek_markers_read_like_accented_ones() {
        let purchase = "ΤΙΜΟΛΟΓΙΟ ΑΓΟΡΑΣ\nΣΤΟΙΧΕΙΑ ΠΕΛΑΤΗ\nΕΠΩΝΥΜΙΑ: ACME LTD\nΠΛΗΡΩΤΕΟ 500,00";
        assert_eq!(read(purchase).kind, EntryKindSuggestion::Expense);

        let exempt = read("ΤΙΜΟΛΟΓΙΟ\nΧΩΡΙΣ ΦΠΑ\nΠΛΗΡΩΤΕΟ 500,00");
        assert!(
            exempt
                .notes
                .contains(&UiText::new(UiTextCode::InvoiceVatExempt)),
            "{:?}",
            exempt.notes
        );

        let described = read("ACME LTD\nΠΕΡΙΓΡΑΦΗ\nΣυμβουλευτικές υπηρεσίες\nΠΛΗΡΩΤΕΟ 500,00");
        assert_eq!(
            described.description.as_deref(),
            Some("Συμβουλευτικές υπηρεσίες")
        );

        let overdue = read("ΛΟΓΑΡΙΑΣΜΟΣ ΡΕΥΜΑΤΟΣ\nΛΗΞΙΠΡΟΘΕΣΜΟ ΥΠΟΛΟΙΠΟ\nΠΛΗΡΩΤΕΟ 80,00");
        assert!(overdue.bill_unpaid);
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;
    use crate::documents::analyze::parse_invoice_text;

    /// Words, amounts and dates an invoice reader looks for, in the scripts
    /// the app supports.
    const FRAGMENTS: [&str; 20] = [
        "Total",
        "ΣΥΝΟΛΟ",
        "Σύνολο",
        "Montant",
        "Betrag",
        "IBAN",
        "RF",
        "GR",
        "€",
        "EUR",
        "1.234,56",
        "1,234.56",
        "12/03/2026",
        "2026-03-12",
        ":",
        "：",
        "ΦΠΑ",
        "Rechnung",
        "N°",
        "\u{feff}",
    ];

    /// Lines built from [`FRAGMENTS`], which reach the parsing code that
    /// arbitrary text almost never does.
    fn invoice_like_text() -> impl Strategy<Value = String> {
        let fragment = prop::sample::select(FRAGMENTS.to_vec());
        let separator = prop::sample::select(vec!["", " ", "\n", "\t"]);

        prop::collection::vec((fragment, separator), 0..40).prop_map(|pieces| {
            pieces
                .into_iter()
                .flat_map(|(fragment, separator)| [fragment, separator])
                .collect()
        })
    }

    const LOCALES: [Locale; 4] = [Locale::En, Locale::El, Locale::Fr, Locale::De];

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn reading_any_text_returns_instead_of_panicking(text in any::<String>()) {
            for locale in LOCALES {
                let _ = parse_invoice_text(&text, locale);
            }
        }

        #[test]
        fn reading_invoice_like_text_returns_instead_of_panicking(text in invoice_like_text()) {
            for locale in LOCALES {
                let _ = parse_invoice_text(&text, locale);
            }
        }
    }
}
