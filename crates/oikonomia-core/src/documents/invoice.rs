//! The invoice reader: from the text of a bill, receipt or invoice to the
//! fields of a draft entry.
//!
//! The reader is a set of rules over lines of text, tuned for Greek and other
//! European tax documents. It uses no network and no model, and it does not
//! know the book: accounts are chosen later, by the analyzer.
//! [`read_invoice_text`] is the entry point.
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
//! 5. A confidence and the reader's notes are added.
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
//! [`names_a_total`]: total::names_a_total

use std::ops::RangeInclusive;

use crate::documents::analyze::{AnalyzeSource, DocumentSuggestion, EntryKindSuggestion};
use crate::documents::brands::{Service, classify_service, known_brand};
use crate::documents::invoice::dates::{find_best_date, first_date_on_line, is_value_date_line};
use crate::documents::invoice::money::{
    is_plausible_money, largest_plausible_amount, money_amounts_on_line,
};
use crate::documents::invoice::normalization::{contains_any, normalize};
use crate::documents::invoice::reference::{find_invoice_reference, is_rf_then_digits};
use crate::documents::invoice::total::{IBAN_WORD, find_total_amount};
use crate::prefs::Locale;
use crate::text::{
    BillKind, bank_transfer_description, bill_description, customer_invoice_description,
    electricity_supplier_merchant, invoice_reference_description, invoice_word,
    natural_gas_merchant,
};
use crate::ui_text::{UiText, UiTextCode};

mod dates;
mod money;
mod normalization;
mod reference;
mod total;

pub(super) use crate::documents::invoice::normalization::folded;

/// What the invoice reader found in a document.
pub(crate) struct InvoiceReading {
    /// The draft fields and the reader's own notes.
    pub suggestion: DocumentSuggestion,
    /// The fee a bank transfer receipt shows, in 2-decimal minor units (cents).
    ///
    /// It is data, not a note: the note needs the book's currency, which the
    /// reader does not know, so the analyzer builds it in one place.
    pub transfer_fee_minor: Option<i64>,
    /// The words the suggested category is chosen from: the merchant and
    /// description, always worded in English.
    ///
    /// The suggestion's own merchant and description are written in the app's
    /// language, and the category must not depend on that language, so the
    /// category is matched against this text instead. Words taken from the
    /// document itself are the same in it as in the suggestion.
    pub category_hint: String,
}

/// Reads extracted document text into a draft suggestion.
///
/// The accounts of the suggestion are left empty: the reader knows no book.
/// Inside the crate, `read_invoice_text` returns the same suggestion with
/// the transfer fee and the category hint beside it.
///
/// A bank transfer receipt stays [`EntryKindSuggestion::Expense`]. Its
/// amount is the capital debit (`Ποσό Χρέωσης Κεφαλαίου` or `Ποσό:`), not the
/// fee and not an `hh:mm` time.
///
/// The returned notes never mention a transfer fee, even when the receipt
/// shows one: that note needs the book's currency, so
/// [`analyze_document_bytes`](crate::documents::analyze_document_bytes) adds
/// it.
#[must_use]
pub fn parse_invoice_text(text: &str, locale: Locale) -> DocumentSuggestion {
    read_invoice_text(text, locale).suggestion
}

/// Reads extracted document text, keeping the transfer fee and the category
/// hint as data.
///
/// The suggestion is worded in `locale`; the category hint never is. For a
/// locale other than English the fields are read a second time in English to
/// get the hint, so the text is parsed twice.
pub(crate) fn read_invoice_text(text: &str, locale: Locale) -> InvoiceReading {
    let mut reading = read_fields(text, locale);

    if locale != Locale::En {
        reading.category_hint = read_fields(text, Locale::En).category_hint;
    }

    reading
}

/// The merchant and description of a suggestion, joined for keyword matching.
fn category_hint_of(suggestion: &DocumentSuggestion) -> String {
    format!(
        "{} {}",
        suggestion.merchant.as_deref().unwrap_or(""),
        suggestion.description.as_deref().unwrap_or("")
    )
}

/// Reads every field of `text`, wording the generated merchant and
/// description in `locale`.
///
/// The text is normalized and folded here, once. A bank transfer receipt is
/// handed to [`parse_bank_transfer`]; any other document has each field found
/// on its own.
fn read_fields(text: &str, locale: Locale) -> InvoiceReading {
    let normalized = normalize(text);
    let folded_full = folded(&normalized);
    if is_bank_transfer_receipt(&folded_full) {
        return parse_bank_transfer(&normalized, locale);
    }

    let amount_minor = find_total_amount(&normalized, &folded_full);
    let entry_date = find_best_date(&normalized);
    let reference = find_invoice_reference(&normalized);
    let merchant = find_merchant(&normalized, &folded_full, locale);
    let description = find_description(
        &normalized,
        &folded_full,
        merchant.as_deref(),
        reference.as_deref(),
        locale,
    );
    let (kind, bill_unpaid) = classify_kind(&folded_full);

    let confidence = score_confidence(amount_minor, entry_date.as_ref(), reference.as_ref(), kind);

    let suggestion = DocumentSuggestion {
        source: AnalyzeSource::Heuristic,
        model: Some("invoice-parser-v1".into()),
        kind,
        amount_minor,
        entry_date,
        description,
        reference,
        merchant,
        bill_unpaid,
        category_account_id: None,
        wallet_account_id: None,
        payable_account_id: None,
        confidence,
        notes: build_notes(amount_minor, kind, bill_unpaid, &folded_full),
    };

    InvoiceReading {
        category_hint: category_hint_of(&suggestion),
        suggestion,
        transfer_fee_minor: None,
    }
}

/// Strong utility markers only. Loose ones such as `ηλεκτρ` also match a
/// software company's line of business (`ΗΛΕΚΤΡΟΝΙΚΩΝ ΣΥΣΤΗΜΑΤΩΝ`).
const UTILITY_MARKERS: &[&str] = &[
    "kwh",
    "ρευμα",
    "εκκαθαριστικ",
    "δεδδηε",
    "ηκασπ",
    "φυσικου αεριου",
    "φυσικο αεριο",
    "προμηθεια φ.α",
    "χρεωση προμηθειας φ.α",
    "gas simple",
    "myon",
    "κωδικος παροχης",
    "υδρευσ",
    POWER_BUSINESS_TARIFF,
];

/// A business electricity tariff, printed on bills whose supplier is not
/// named in the text.
const POWER_BUSINESS_TARIFF: &str = "power business";

/// The heading of the counterparty block on a sales invoice.
const CUSTOMER_BLOCK_LABEL: &str = "στοιχεια πελατη";

/// The English heading of an invoice the book's owner issued.
const SALES_INVOICE_WORDS: &str = "sales invoice";

/// The word "invoice", in Greek and in English.
const INVOICE_WORDS: &[&str] = &[INVOICE_WORD_GREEK, "invoice"];

/// Wording that marks a utility bill as unpaid: overdue (`ληξιπρόθεσμ-`),
/// unpaid (`ανεξόφλητ-`), amount due.
///
/// Wording that every settlement bill prints, such as "pay by" and "pay
/// through", is not here; `settlement_bill_is_not_automatically_unpaid` pins
/// that such a bill is not marked unpaid.
const UTILITY_UNPAID_MARKERS: &[&str] = &["ληξιπροθεσμ", "ανεξοφλητ", "amount due"];

/// Wording of an invoice the book's owner received, which overrides the
/// signs of a sales invoice.
const PURCHASE_MARKERS: &[&str] = &["τιμολογιο αγορ", "purchase invoice", "supplier"];

/// Wording that marks a document other than a utility bill as unpaid. `επι
/// πιστωσει` is "on credit", the payment method of an invoice not yet paid.
const UNPAID_MARKERS: &[&str] = &[
    "επι πιστωσει",
    "amount due",
    "unpaid",
    "outstanding",
    "please pay",
];

/// Wording that makes a document a bank transfer receipt: "έμβασμα"
/// (remittance), "transfer to another bank", or a transaction-code label.
const TRANSFER_MARKERS: &[&str] = &[
    "εμβασμα",
    "μεταφορα σε αλλη τραπεζα",
    TRANSFER_REFERENCE_LABEL,
];

/// The label of the transaction code on a transfer receipt.
const TRANSFER_REFERENCE_LABEL: &str = "κωδικος συναλλαγης";

/// Whether folded text carries one of [`UTILITY_MARKERS`].
fn is_utility_bill(folded_text: &str) -> bool {
    contains_any(folded_text, UTILITY_MARKERS)
}

/// Whether folded text is a sales invoice issued by the book's owner: it has
/// the customer block heading "Στοιχεία Πελάτη" and the word "invoice".
fn is_sales_invoice(folded_text: &str) -> bool {
    folded_text.contains(CUSTOMER_BLOCK_LABEL) && contains_any(folded_text, INVOICE_WORDS)
}

/// Decides the entry kind and whether the document is unpaid, from folded
/// text. The first rule that applies wins:
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
fn classify_kind(folded_text: &str) -> (EntryKindSuggestion, bool) {
    if is_utility_bill(folded_text) {
        let unpaid = contains_any(folded_text, UTILITY_UNPAID_MARKERS);
        return (EntryKindSuggestion::Bill, unpaid);
    }

    // "Σταθερό Τιμολόγιο" is a tariff name, not a sales invoice.
    let sales = is_sales_invoice(folded_text) || folded_text.contains(SALES_INVOICE_WORDS);
    let purchase = contains_any(folded_text, PURCHASE_MARKERS);
    let unpaid = contains_any(folded_text, UNPAID_MARKERS);

    if sales && !purchase {
        return (EntryKindSuggestion::Income, unpaid);
    }

    // A recognized biller with a known service (telecom etc.) is a bill to
    // pay even without the utility markers above.
    if let Some((_, Some(_))) = known_brand(folded_text) {
        return (EntryKindSuggestion::Bill, unpaid);
    }

    if unpaid {
        (EntryKindSuggestion::Bill, true)
    } else {
        (EntryKindSuggestion::Expense, false)
    }
}

/// Whether folded text carries one of [`TRANSFER_MARKERS`].
fn is_bank_transfer_receipt(folded_text: &str) -> bool {
    contains_any(folded_text, TRANSFER_MARKERS)
}

/// Reads a Greek bank transfer receipt (`έμβασμα`, or a transfer to another
/// bank) from normalized text.
///
/// The kind is always `Expense` and never unpaid. The description is
/// generated from the payee. The fee is returned beside the suggestion, not
/// in its notes.
fn parse_bank_transfer(text: &str, locale: Locale) -> InvoiceReading {
    let amount_minor = find_transfer_principal(text);
    let entry_date = find_transfer_date(text);
    let reference = find_transfer_reference(text);
    let merchant = find_transfer_payee(text);
    let description = Some(bank_transfer_description(locale, merchant.as_deref()));
    let fee_minor = find_transfer_fee(text);
    let kind = EntryKindSuggestion::Expense;
    let confidence = score_confidence(amount_minor, entry_date.as_ref(), reference.as_ref(), kind);

    let suggestion = DocumentSuggestion {
        source: AnalyzeSource::Heuristic,
        model: Some("invoice-parser-v1".into()),
        kind,
        amount_minor,
        entry_date,
        description,
        reference,
        merchant,
        bill_unpaid: false,
        category_account_id: None,
        wallet_account_id: None,
        payable_account_id: None,
        confidence,
        notes: build_transfer_notes(amount_minor),
    };

    InvoiceReading {
        category_hint: category_hint_of(&suggestion),
        suggestion,
        transfer_fee_minor: fee_minor,
    }
}

/// The amount that was transferred, without the fee.
///
/// Two passes over the lines, both skipping fee lines: first for the capital
/// debit label ([`TRANSFER_PRINCIPAL_LABEL`]), then for a plain amount label
/// ([`is_transfer_amount_label`]). The first label that has an amount on its
/// line or the next decides. `None` when no label has one.
fn find_transfer_principal(text: &str) -> Option<i64> {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if is_transfer_fee_line(&folded_line) {
            continue;
        }
        if folded_line.contains(TRANSFER_PRINCIPAL_LABEL)
            && let Some(amount) = amount_on_line_or_next(&lines, index)
        {
            return Some(amount);
        }
    }
    for (index, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if is_transfer_fee_line(&folded_line) {
            continue;
        }
        if is_transfer_amount_label(&folded_line)
            && let Some(amount) = amount_on_line_or_next(&lines, index)
        {
            return Some(amount);
        }
    }
    None
}

/// The largest plausible amount on line `index`, or, when that line has
/// none, on the line after it.
///
/// `index` must be a valid index into `lines`. The next line is taken
/// whatever it is labelled.
fn amount_on_line_or_next(lines: &[&str], index: usize) -> Option<i64> {
    let next = lines.get(index + 1).copied().unwrap_or("");

    largest_plausible_amount(lines[index]).or_else(|| largest_plausible_amount(next))
}

/// The label of the capital debit on a transfer receipt.
const TRANSFER_PRINCIPAL_LABEL: &str = "ποσο χρεωσης κεφαλαιου";

/// The plain "amount" label a receipt uses when it has no capital line.
const TRANSFER_AMOUNT_LABEL: &str = "ποσο";

/// Labels of the fee and charges lines of a transfer receipt.
const TRANSFER_FEE_LABELS: &[&str] = &["προμηθεια", "εξοδων", "εξοδα"];

/// "Of the beneficiary", as in "name of the beneficiary".
const BENEFICIARY_LABEL: &str = "δικαιουχου";

/// The two halves of the `Ονοματεπώνυμο / Επωνυμία` beneficiary label.
const BENEFICIARY_NAME_LABELS: [&str; 2] = ["ονοματεπωνυμο", ISSUER_NAME_LABEL];

/// Words that make a name a bank's, not the payee's.
const BANK_WORDS: &[&str] = &["τραπεζα", "bank"];

/// Labels of the date a transfer was executed, which is the date to post.
const EXECUTION_DATE_LABELS: &[&str] = &["εκτελεσ", "execution"];

/// Labels of a date line on a transfer receipt, used when no execution date
/// is labelled.
const TRANSFER_DATE_LABELS: &[&str] = &["ημερομην", "date", "συναλλαγ"];

/// Fewest characters of a payee, issuer or customer name.
///
/// The reason for 3 is not recorded, and no test pins it.
const MIN_NAME_CHARS: usize = 3;

/// Lengths of a transfer transaction code.
///
/// The code on the corpus receipt has 16 characters. The reasons for 10 and
/// 24 are not recorded, and no test pins either.
const TRANSFER_CODE_CHARS: RangeInclusive<usize> = 10..=24;

/// Length of the shortest IBAN (Norway's). No test pins it.
const MIN_IBAN_CHARS: usize = 15;

/// Whether a folded line is a fee or charges line.
fn is_transfer_fee_line(folded_line: &str) -> bool {
    contains_any(folded_line, TRANSFER_FEE_LABELS)
}

/// Whether a folded line carries the plain amount label: the word `ποσο`
/// alone on the line, or `ποσο` anywhere on a line that has a colon.
///
/// Fee lines also say `ποσο`; the caller has skipped them before it asks.
/// The label is matched as a substring, so a line such as `Ποσοστό:`
/// (percentage) also counts.
fn is_transfer_amount_label(folded_line: &str) -> bool {
    folded_line.trim() == TRANSFER_AMOUNT_LABEL
        || (folded_line.contains(TRANSFER_AMOUNT_LABEL) && folded_line.contains(':'))
}

/// The fee of a transfer: the smallest plausible amount on the first fee
/// line that has one. `None` when the receipt shows no fee, or one under the
/// plausibility band.
fn find_transfer_fee(text: &str) -> Option<i64> {
    for line in text.lines() {
        let folded_line = folded(line);
        if !is_transfer_fee_line(&folded_line) {
            continue;
        }
        if let Some(amount) = money_amounts_on_line(line)
            .into_iter()
            .filter(|amount| is_plausible_money(*amount))
            .min()
        {
            return Some(amount);
        }
    }
    None
}

/// The payee of a transfer: the value on the first beneficiary line that
/// has a plausible name after its colon or on the line below it.
///
/// A beneficiary line holds [`BENEFICIARY_LABEL`] or both
/// [`BENEFICIARY_NAME_LABELS`].
fn find_transfer_payee(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        let beneficiary = folded_line.contains(BENEFICIARY_LABEL)
            || BENEFICIARY_NAME_LABELS
                .iter()
                .all(|label| folded_line.contains(label));
        if !beneficiary {
            continue;
        }
        if let Some(name) = value_after_colon(line)
            && is_plausible_payee(&name)
        {
            return Some(name);
        }
        if let Some(next) = lines.get(index + 1)
            && is_plausible_payee(next)
        {
            return Some(next.trim().to_owned());
        }
    }
    None
}

/// Whether `name` can be a payee: at least [`MIN_NAME_CHARS`] characters,
/// with a letter, naming no bank and no IBAN, and holding more than the
/// beneficiary labels themselves.
fn is_plausible_payee(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.chars().count() < MIN_NAME_CHARS || !trimmed.chars().any(char::is_alphabetic) {
        return false;
    }
    let folded_name = folded(trimmed);
    if is_bank_counterparty(&folded_name) || folded_name.contains(IBAN_WORD) {
        return false;
    }

    // A line that only repeats the label is not a name.
    let mut stripped = folded_name.replace(['/', ':', '：'], " ");
    for label in BENEFICIARY_NAME_LABELS
        .into_iter()
        .chain([BENEFICIARY_LABEL])
    {
        stripped = stripped.replace(label, " ");
    }
    stripped.chars().any(char::is_alphabetic)
}

/// Whether a folded name holds one of [`BANK_WORDS`].
fn is_bank_counterparty(folded_name: &str) -> bool {
    contains_any(folded_name, BANK_WORDS)
}

/// The transaction code of a transfer: the first code on the first line that
/// carries [`TRANSFER_REFERENCE_LABEL`], or on the line below it. `None`
/// when no labelled line has a code near it.
fn find_transfer_reference(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if !folded_line.contains(TRANSFER_REFERENCE_LABEL) {
            continue;
        }
        for candidate in [*line, lines.get(index + 1).copied().unwrap_or("")] {
            if let Some(code) = transfer_code_token(candidate) {
                return Some(code);
            }
        }
    }
    None
}

/// The first word of `line` that is a transfer code once the punctuation
/// around it is trimmed, in upper case.
fn transfer_code_token(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        let token = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if is_transfer_code(token) {
            return Some(token.to_ascii_uppercase());
        }
    }
    None
}

/// Whether `token` has the shape of a transaction code:
/// [`TRANSFER_CODE_CHARS`] ASCII letters and digits with at least one of
/// each.
///
/// Two shapes that fit are something else and are refused: an RF payment
/// code (`RF` then digits), and an IBAN (two letters then digits, at least
/// [`MIN_IBAN_CHARS`] long).
fn is_transfer_code(token: &str) -> bool {
    if !TRANSFER_CODE_CHARS.contains(&token.len()) {
        return false;
    }
    if !token.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if !token.chars().any(|c| c.is_ascii_alphabetic()) || !token.chars().any(|c| c.is_ascii_digit())
    {
        return false;
    }
    let upper = token.to_ascii_uppercase();
    if is_rf_then_digits(&upper) {
        return false;
    }
    // IBAN-shaped: two letters then only digits.
    if upper.len() >= MIN_IBAN_CHARS
        && upper.chars().take(2).all(|c| c.is_ascii_alphabetic())
        && upper.chars().skip(2).all(|c| c.is_ascii_digit())
    {
        return false;
    }
    true
}

/// The date of a transfer, as `YYYY-MM-DD`.
///
/// Value-date lines are skipped throughout: the value date can be a day or
/// more after the transfer. Among the other lines, in order of preference:
/// the first date on a line with an execution label, the first on a line
/// with any date label, the first anywhere.
fn find_transfer_date(text: &str) -> Option<String> {
    let mut labeled: Option<String> = None;
    for line in text.lines() {
        if is_value_date_line(&folded(line)) {
            continue;
        }
        let Some(date) = first_date_on_line(line) else {
            continue;
        };
        let folded_line = folded(line);
        if contains_any(&folded_line, EXECUTION_DATE_LABELS) {
            return Some(date);
        }
        if labeled.is_none() && contains_any(&folded_line, TRANSFER_DATE_LABELS) {
            labeled = Some(date);
        }
    }
    labeled.or_else(|| {
        text.lines().find_map(|line| {
            if is_value_date_line(&folded(line)) {
                None
            } else {
                first_date_on_line(line)
            }
        })
    })
}

/// The reader's notes for a transfer receipt: that it was parsed, that it is
/// a transfer, and, when no principal was found, that the amount is missing.
fn build_transfer_notes(amount: Option<i64>) -> Vec<UiText> {
    let mut notes = vec![
        UiText::new(UiTextCode::InvoiceParsed),
        UiText::new(UiTextCode::TransferDetected),
    ];

    if amount.is_none() {
        notes.push(UiText::new(UiTextCode::TransferNoAmount));
    }

    notes
}

/// The counterparty of a document. The first of these that yields a name:
///
/// 1. on a sales invoice, the customer ([`sales_invoice_customer`]). This
///    comes before brand recognition because the issuer's payment footer
///    often names a bank ("PIRAEUS BANK, IBAN ...") that must not win;
/// 2. a known biller ([`known_brand`]);
/// 3. on a utility bill of no known brand, a generic supplier name in
///    `locale`: natural gas, or electricity for a "Power Business" tariff;
/// 4. the first `Επωνυμία` (legal name) line: what follows its colon, or
///    else the line without the label words at its start. When the label is
///    neither followed by a colon nor at the start, the whole line is
///    returned, label included;
/// 5. the first line of [`MERCHANT_LINE_CHARS`] characters that has a letter
///    and does not hold the Greek word for "invoice".
///
/// `text` is normalized and `folded_text` is its folded form.
fn find_merchant(text: &str, folded_text: &str, locale: Locale) -> Option<String> {
    if is_sales_invoice(folded_text)
        && let Some(customer) = sales_invoice_customer(text)
    {
        return Some(customer);
    }

    if let Some((brand, _)) = known_brand(folded_text) {
        return Some(brand.to_owned());
    }

    if is_utility_bill(folded_text) {
        if contains_any(folded_text, GAS_SUPPLY_MARKERS) {
            return Some(natural_gas_merchant(locale).into());
        }
        if folded_text.contains(POWER_BUSINESS_TARIFF) {
            return Some(electricity_supplier_merchant(locale).into());
        }
    }

    for line in text.lines() {
        if folded(line).contains(ISSUER_NAME_LABEL) {
            if let Some(name) = value_after_colon(line)
                && name.chars().count() >= MIN_NAME_CHARS
            {
                return Some(name);
            }
            // No colon: the name is what follows the label word.
            let cleaned = line
                .split_whitespace()
                .skip_while(|word| *word == ":" || folded(word).contains(ISSUER_NAME_STEM))
                .collect::<Vec<_>>()
                .join(" ");
            if cleaned.chars().count() >= MIN_NAME_CHARS {
                return Some(cleaned);
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
        .map(ToOwned::to_owned)
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
const GAS_SUPPLY_MARKERS: &[&str] = &[
    "φυσικου αεριου",
    "φυσικο αεριο",
    "gas simple",
    "προμηθεια φ.α",
];

/// The label of a legal name, on an issuer or a customer.
const ISSUER_NAME_LABEL: &str = "επωνυμια";

/// What every inflection of [`ISSUER_NAME_LABEL`] starts with, to skip the
/// label word itself.
const ISSUER_NAME_STEM: &str = "επων";

/// "Invoice" in Greek. Also part of tariff names ("Σταθερό Τιμολόγιο"), so
/// on its own it does not make a document an invoice.
const INVOICE_WORD_GREEK: &str = "τιμολογιο";

/// The English label of a name line in a customer block, at the line's
/// start.
const NAME_LABEL: &str = "name";

/// Headings that open the customer block of a sales invoice.
const CUSTOMER_BLOCK_STARTS: &[&str] = &[CUSTOMER_BLOCK_LABEL, "customer"];

/// Labels of the lines in a customer block that are not the customer's name.
const TAX_ID_OR_ADDRESS_LABELS: &[&str] = &["α.φ.μ", "αφμ", "διευθυν"];

/// Column headings of the description column of a line-item table.
const DESCRIPTION_HEADERS: &[&str] = &["περιγραφη", "description"];

/// Column headings that follow the description heading in a table header.
const QUANTITY_HEADERS: &[&str] = &["ποσοτητα", "quantity"];

/// Wording of a document that states it carries no VAT ("without VAT"). A
/// rate of zero says the same; [`states_a_zero_rate`] finds that.
const VAT_EXEMPT_MARKERS: &[&str] = &["χωρις φπα"];

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
fn value_after_colon(line: &str) -> Option<String> {
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
/// 1. sales invoice with a customer: a generated "customer, invoice,
///    reference" title;
/// 2. utility bill, or known biller whose brand implies a service: a
///    generated bill title for the service. The brand's service wins;
///    otherwise [`classify_service`] decides, and a bill whose service
///    cannot be told is a plain utility bill;
/// 3. the first line under a description heading that has at least
///    [`MIN_DESCRIPTION_CHARS`] letters and spaces once everything else is
///    removed, and is not the quantity heading of the same table row;
/// 4. with a reference: a generated "invoice reference" title;
/// 5. with the Greek word for "invoice" in the text: that word in `locale`;
/// 6. the merchant.
///
/// Generated titles are worded in `locale` by [`crate::text`].
fn find_description(
    text: &str,
    folded_text: &str,
    merchant: Option<&str>,
    reference: Option<&str>,
    locale: Locale,
) -> Option<String> {
    if is_sales_invoice(folded_text)
        && let Some(customer) = merchant
    {
        return Some(customer_invoice_description(locale, customer, reference));
    }

    let brand_service = known_brand(folded_text).and_then(|(_, service)| service);

    if is_utility_bill(folded_text) || brand_service.is_some() {
        let service = brand_service.or_else(|| classify_service(folded_text));
        let kind = service.map_or(BillKind::Utility, Service::bill_kind);

        return Some(bill_description(locale, kind, merchant));
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
                return Some(words.to_owned());
            }
        }
    }

    if let Some(reference) = reference {
        return Some(invoice_reference_description(locale, reference, merchant));
    }
    if folded_text.contains(INVOICE_WORD_GREEK) {
        return Some(invoice_word(locale).into());
    }
    merchant.map(ToOwned::to_owned)
}

/// The parts of the confidence that [`score_confidence`] adds up.
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

/// A rough confidence between 0 and 1 for a reading: the sum of the
/// [`confidence`] parts for what was found, capped at the ceiling.
fn score_confidence(
    amount: Option<i64>,
    date: Option<&String>,
    reference: Option<&String>,
    kind: EntryKindSuggestion,
) -> f32 {
    let mut score = confidence::BASE;
    if amount.is_some() {
        score += confidence::AMOUNT;
    }
    if date.is_some() {
        score += confidence::DATE;
    }
    if reference.is_some() {
        score += confidence::REFERENCE;
    }
    if matches!(
        kind,
        EntryKindSuggestion::Income | EntryKindSuggestion::Bill
    ) {
        score += confidence::CLASSIFIED_KIND;
    }
    score.min(confidence::CEILING)
}

/// The reader's notes for a document that is not a transfer receipt, in the
/// order the user reads them: parsed, no total found, income, utility bill,
/// unpaid, VAT-exempt. Each but the first is added only when it applies.
fn build_notes(
    amount: Option<i64>,
    kind: EntryKindSuggestion,
    unpaid: bool,
    folded_text: &str,
) -> Vec<UiText> {
    let mut notes = vec![UiText::new(UiTextCode::InvoiceParsed)];

    if amount.is_none() {
        notes.push(UiText::new(UiTextCode::InvoiceNoTotal));
    }
    if matches!(kind, EntryKindSuggestion::Income) {
        notes.push(UiText::new(UiTextCode::InvoiceIncome));
    }
    if is_utility_bill(folded_text) {
        notes.push(UiText::new(UiTextCode::InvoiceUtility));
    }
    if unpaid {
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
    use crate::documents::invoice::dates::{DATE_LABELS, VALUE_DATE_LABELS};
    use crate::documents::invoice::reference::{MARK_LABELS, REFERENCE_LABELS, SUPPLY_CODE_LABELS};
    use crate::documents::invoice::total::{
        CONSUMPTION_LABEL, DEPOSIT_LABELS, IDENTIFIER_LINE_MARKERS, PAYMENT_LABELS,
        RATE_LINE_MARKERS, TOTAL_LABELS, TOTAL_WORD, TOTALS_ROW_LABELS, VALUE_WORDS,
    };

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
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-06-25"));
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
    fn value_after_fullwidth_colon_does_not_panic() {
        assert_eq!(value_after_colon("Name：ACME LTD"), Some("ACME LTD".into()));
        assert_eq!(value_after_colon("Name: ACME LTD"), Some("ACME LTD".into()));
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

    #[test]
    fn greek_bank_transfer_receipt_principal_not_fee_or_clock() {
        let text = corpus_text("synthetic/text/greek_bank_embasma.txt");
        let suggestion = parse_invoice_text(&text, crate::prefs::Locale::El);
        assert_eq!(
            suggestion.amount_minor,
            Some(31_000),
            "expected €310,00 not fee 1,40 or clock 7:00, got {:?}",
            suggestion.amount_minor
        );
        assert_eq!(suggestion.kind, EntryKindSuggestion::Expense);
        assert!(!suggestion.bill_unpaid);
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-08-27"));
        assert_eq!(suggestion.merchant.as_deref(), Some("HELIOS TRADING IKE"));
        assert_eq!(suggestion.reference.as_deref(), Some("F000TO0000000001"));
        assert_eq!(
            suggestion.description.as_deref(),
            Some("Έμβασμα — HELIOS TRADING IKE")
        );
        assert_eq!(
            suggestion.notes,
            [
                UiText::new(UiTextCode::InvoiceParsed),
                UiText::new(UiTextCode::TransferDetected),
            ],
            "the fee note needs the book currency, so the analyzer adds it"
        );
        assert_eq!(
            read_invoice_text(&text, crate::prefs::Locale::En).transfer_fee_minor,
            Some(140),
            "the fee is returned as integer minor units"
        );
        assert_ne!(suggestion.amount_minor, Some(140));
        assert_ne!(suggestion.amount_minor, Some(700));

        let via_analyze = crate::documents::analyze_document_bytes(
            "greek_bank_embasma.txt",
            "text/plain",
            text.as_bytes(),
            &crate::documents::AnalyzeContext {
                template: crate::domain::ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::El,
            },
            None,
        );
        let analyzed = via_analyze.expect("analyze text/plain");
        assert_eq!(analyzed.amount_minor, suggestion.amount_minor);
        assert_eq!(analyzed.entry_date, suggestion.entry_date);
        assert_eq!(analyzed.kind, suggestion.kind);
        assert_eq!(analyzed.merchant, suggestion.merchant);
        assert_eq!(analyzed.reference, suggestion.reference);
        assert_eq!(analyzed.description, suggestion.description);
    }

    #[test]
    fn unaccented_bank_transfer_markers_are_detected() {
        let text = "\
Εμβασμα
Μεταφορα σε αλλη τραπεζα
Ημερομηνια Εκτελεσης: 27/8/2026
Ημερομηνια Αξιας: 28/8/2026 7:00 μ.μ.
Ποσο Χρεωσης Κεφαλαιου 310,00
Προμηθεια 1,40
Ονοματεπωνυμο / Επωνυμια Δικαιουχου: HELIOS TRADING IKE
Κωδικος Συναλλαγης: F000TO0000000001
";
        let suggestion = parse_invoice_text(text, crate::prefs::Locale::En);
        assert_eq!(suggestion.amount_minor, Some(31_000));
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-08-27"));
        assert_eq!(suggestion.merchant.as_deref(), Some("HELIOS TRADING IKE"));
        assert_eq!(suggestion.reference.as_deref(), Some("F000TO0000000001"));
        assert_eq!(suggestion.kind, EntryKindSuggestion::Expense);
    }

    #[test]
    fn a_receipt_without_a_capital_line_takes_the_plain_amount_label() {
        let amount = |text: &str| parse_invoice_text(text, crate::prefs::Locale::En).amount_minor;

        // The label with a colon, the amount beside it.
        assert_eq!(
            amount("Έμβασμα\nΠοσό: 310,00\nΠρομήθεια: 1,40"),
            Some(31_000)
        );
        // The bare label, the amount on the line below.
        assert_eq!(
            amount("Έμβασμα\nΠοσό\n310,00\nΠρομήθεια 1,40"),
            Some(31_000)
        );
        // A fee line that also says "amount" is not the principal.
        assert_eq!(
            amount("Έμβασμα\nΠοσό προμήθειας: 1,40\nΠοσό: 310,00"),
            Some(31_000)
        );
        assert_eq!(amount("Έμβασμα\nΠοσό προμήθειας: 1,40"), None);
        // Without a colon the word inside a sentence is no label.
        assert_eq!(amount("Έμβασμα\nΤο ποσό των 310,00 μεταφέρθηκε"), None);
    }

    #[test]
    fn a_transfer_without_a_principal_asks_for_the_amount() {
        let suggestion = parse_invoice_text(
            "Εμβασμα\nΜεταφορά σε άλλη τράπεζα\n",
            crate::prefs::Locale::En,
        );

        assert_eq!(
            suggestion.notes,
            [
                UiText::new(UiTextCode::InvoiceParsed),
                UiText::new(UiTextCode::TransferDetected),
                UiText::new(UiTextCode::TransferNoAmount),
            ]
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
        let notes = build_notes(Some(1000), EntryKindSuggestion::Income, false, "");

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
            EntryKindSuggestion::Bill,
            true,
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
        build_notes(
            Some(1000),
            EntryKindSuggestion::Expense,
            false,
            &folded(text),
        )
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
    fn the_category_hint_is_worded_in_english_whatever_the_language() {
        let text = corpus_text("synthetic/text/dei_settlement.txt");
        let english = read_invoice_text(&text, crate::prefs::Locale::En);

        for locale in [
            crate::prefs::Locale::El,
            crate::prefs::Locale::Fr,
            crate::prefs::Locale::De,
        ] {
            let reading = read_invoice_text(&text, locale);

            assert_eq!(reading.category_hint, english.category_hint, "{locale:?}");
        }

        assert!(
            english.category_hint.contains("Electricity bill"),
            "{:?}",
            english.category_hint
        );
    }
}

#[cfg(test)]
mod amounts_and_dates {
    use super::*;

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
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-08-13"));
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
mod documented_tradeoffs {
    use super::*;

    #[test]
    fn a_fee_under_the_plausibility_band_is_not_reported() {
        let receipt = "Έμβασμα\nΠοσό Χρέωσης Κεφαλαίου 310,00\nΠρομήθεια 0,40";

        let reading = read_invoice_text(receipt, crate::prefs::Locale::En);

        assert_eq!(reading.suggestion.amount_minor, Some(31_000));
        assert_eq!(reading.transfer_fee_minor, None);
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

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
