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
//!   `total amount` and can decide stage 1. The MARK label alone is matched
//!   as a whole word ([`MARK_WORDS`]), because shop names contain it:
//!   `Supermarket`, `Marks & Spencer`, `ΣΟΥΠΕΡ ΜΑΡΚΕΤ`.
//! - **A MARK label glued to its number is not a label.** In
//!   `MARK400001234567890` the label is not a whole word, so the number is
//!   found only by the later reference steps: a labelled invoice number
//!   comes first, and the longest number after it.
//! - **Leading zeros in a whole part are accepted beside a decimal mark.**
//!   `01,50` reads as 1,50, although a bare `08` is not money and `01.234`
//!   is not a thousands amount.
//! - **A whole amount of 1900 to 2100 euros needs decimals.** `2026` is a
//!   year; `2026,00` is an amount.
//! - **Any zero percentage counts as a zero VAT rate.** The VAT-exempt note
//!   is added for `0%` anywhere in the text, beside a VAT label or not.
//! - **Dates are day first.** `03/04/2026` is 3 April. A month-first date
//!   is misread, or refused when its "month" is over 12.

use std::ops::RangeInclusive;

use crate::documents::analyze::{AnalyzeSource, DocumentSuggestion, EntryKindSuggestion};
use crate::documents::brands::{Service, classify_service, known_brand};
use crate::documents::store::Keyword;
use crate::prefs::Locale;
use crate::text::{
    BillKind, bank_transfer_description, bill_description, customer_invoice_description,
    electricity_supplier_merchant, invoice_reference_description, invoice_word,
    natural_gas_merchant,
};
use crate::ui_text::{UiText, UiTextCode};

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

/// Cleans up extracted text before anything reads it, by the four steps in
/// the module documentation ("Normalizing").
///
/// The result has no `\r`, no empty line, and no line with leading or
/// trailing whitespace.
fn normalize(text: &str) -> String {
    let text = text.replace('\r', "\n");
    let text = text.replace(['\u{00a0}', '\u{202f}', '\u{2009}'], " ");
    // Euro symbol variants — standalone tokens only, so EUROBANK stays intact.
    // Before the number joins, so "EUR1 234,56" starts its amount at a symbol
    // and not at the tail of a word.
    let text = replace_eur_token(&text);
    // OCR and PDF extraction split numbers with spaces: "72, 53", "1 234,56".
    let text = collapse_spaced_decimals(&text);

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replaces each standalone `EUR`, in any letter case, with `€`.
///
/// Standalone means no letter directly before or after. So `EUROBANK` and
/// `Europe` keep their letters, which would otherwise show up damaged in a
/// merchant or description, while `10EUR` and `EUR1` are replaced: a digit
/// is a boundary.
fn replace_eur_token(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut replaced = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let is_eur = index + 2 < chars.len()
            && chars[index].eq_ignore_ascii_case(&'e')
            && chars[index + 1].eq_ignore_ascii_case(&'u')
            && chars[index + 2].eq_ignore_ascii_case(&'r');
        let boundary_before = index == 0 || !chars[index - 1].is_alphabetic();
        let boundary_after = index + 3 >= chars.len() || !chars[index + 3].is_alphabetic();

        if is_eur && boundary_before && boundary_after {
            replaced.push('€');
            index += 3;
        } else {
            replaced.push(chars[index]);
            index += 1;
        }
    }
    replaced
}

/// Digits in a thousands group: `1.234.567`, `1 234 567`.
const THOUSANDS_GROUP_DIGITS: usize = 3;

/// Joins the pieces of a number that OCR or PDF extraction split with a space.
///
/// Two joins are made, each inside one line:
///
/// - a separator and the digits after it: `72, 53` becomes `72,53`. Once a
///   number has a separator, a later one joins across a space only after a
///   group of three digits (`1.234, 56`), so the full stop in
///   `45,90. 3 items` ends the amount;
/// - space-grouped thousands: `1 234,56` becomes `1234,56`.
///
/// A thousands join needs a prefix that can be one: one to three digits
/// that start their own token, with no `,` or `.` read yet, followed by one
/// space and exactly three digits. Dates and clock times are never a
/// prefix, and neither is the tail of a code such as `A-7` or `B7`.
///
/// A lone digit is a valid prefix, because `5 120,50` and `1 234,56` are
/// spelled the same way. A quantity column directly before a three-digit
/// price therefore reads as one amount; a labelled total on the document
/// still decides the suggested amount.
fn collapse_spaced_decimals(text: &str) -> String {
    text.split('\n')
        .map(collapse_spaced_decimals_on_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// [`collapse_spaced_decimals`] for one line.
///
/// Characters that are not part of a number are copied as they are, dates
/// and clock times included.
fn collapse_spaced_decimals_on_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    // Dates and clocks are blanked in this copy, so their digits never start
    // or extend a number; the output below copies them from `chars`.
    let numbers: Vec<char> = mask_time_tokens(&mask_date_tokens(line)).chars().collect();

    let mut joined = String::with_capacity(line.len());
    let mut index = 0;
    while let Some(&original) = chars.get(index) {
        if numbers.get(index).is_some_and(char::is_ascii_digit) {
            index = push_spaced_number(&numbers, index, &mut joined);
        } else {
            joined.push(original);
            index += 1;
        }
    }
    joined
}

/// Appends the number starting at `start` to `joined` with its inner spaces
/// removed, and returns the index after it.
fn push_spaced_number(numbers: &[char], start: usize, joined: &mut String) -> usize {
    let may_group_thousands = starts_own_token(numbers, start);
    let mut has_separator = false;
    // Digits since the start of the number, the last join or the last separator.
    let mut group_digits = 0_usize;
    let mut index = start;

    while let Some(&current) = numbers.get(index) {
        if current.is_ascii_digit() {
            joined.push(current);
            group_digits += 1;
            index += 1;
        } else if let Some(next_digit) = digit_after_separator(numbers, index)
            && (next_digit == index + 1 || !has_separator || group_digits == THOUSANDS_GROUP_DIGITS)
        {
            joined.push(current);
            has_separator = true;
            group_digits = 0;
            index = next_digit;
        } else if !has_separator
            && may_group_thousands
            && (1..=THOUSANDS_GROUP_DIGITS).contains(&group_digits)
            && is_thousands_group(numbers, index)
        {
            group_digits = 0;
            index += 1;
        } else {
            break;
        }
    }
    index
}

/// Whether the digits at `start` begin a token: nothing before them makes
/// them the tail of a date, a dotted number or a code.
fn starts_own_token(numbers: &[char], start: usize) -> bool {
    let Some(before) = start.checked_sub(1).and_then(|index| numbers.get(index)) else {
        return true;
    };
    !before.is_alphanumeric() && !matches!(before, '/' | '.' | '-' | ',' | ':')
}

/// Index of the digit that continues a number across the `,` or `.` at
/// `index`, with at most one space in between.
fn digit_after_separator(numbers: &[char], index: usize) -> Option<usize> {
    if !matches!(numbers.get(index), Some(',' | '.')) {
        return None;
    }

    let is_digit = |at: usize| numbers.get(at).is_some_and(char::is_ascii_digit);
    let space_then_digit = numbers
        .get(index + 1)
        .is_some_and(|next| next.is_whitespace())
        && is_digit(index + 2);

    if is_digit(index + 1) {
        Some(index + 1)
    } else {
        space_then_digit.then_some(index + 2)
    }
}

/// Whether `index` holds one space followed by exactly three digits.
fn is_thousands_group(numbers: &[char], index: usize) -> bool {
    let is_digit = |at: usize| numbers.get(at).is_some_and(char::is_ascii_digit);

    numbers
        .get(index)
        .is_some_and(|space| space.is_whitespace())
        && (1..=THOUSANDS_GROUP_DIGITS).all(|offset| is_digit(index + offset))
        && !is_digit(index + THOUSANDS_GROUP_DIGITS + 1)
}

/// Lowercases `text` and folds its Greek accents.
///
/// Every label and marker in this module and in `brands` is matched against
/// text in this form, and is itself written in it, so `Τελική`, `ΤΕΛΙΚΗ` and
/// `τελικη` all match the one needle `τελικη`.
pub(super) fn folded(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'ά' | 'ὰ' | 'ᾶ' | 'ἀ' | 'ἁ' | 'ᾳ' => 'α',
            'έ' | 'ὲ' | 'ἐ' | 'ἑ' => 'ε',
            'ή' | 'ὴ' | 'ῆ' | 'ἠ' | 'ἡ' | 'ῃ' => 'η',
            'ί' | 'ὶ' | 'ῖ' | 'ϊ' | 'ΐ' | 'ἰ' | 'ἱ' => 'ι',
            'ό' | 'ὸ' | 'ὀ' | 'ὁ' => 'ο',
            'ύ' | 'ὺ' | 'ῦ' | 'ϋ' | 'ΰ' | 'ὐ' | 'ὑ' => 'υ',
            'ώ' | 'ὼ' | 'ῶ' | 'ὠ' | 'ὡ' | 'ῳ' => 'ω',
            other => other,
        })
        .collect()
}

/// Whether folded text contains any of `needles`.
fn contains_any(folded_text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| folded_text.contains(needle))
}

/// Whether folded text contains any of `keywords`, each under its own
/// boundary rule ([`Keyword::occurs_in`]).
fn contains_any_keyword(folded_text: &str, keywords: &[Keyword]) -> bool {
    keywords
        .iter()
        .any(|keyword| keyword.occurs_in(folded_text))
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

/// The word that marks a line or a name as holding a bank account number.
const IBAN_WORD: &str = "iban";

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

/// The first date written on `line`, as `YYYY-MM-DD`.
///
/// A date is a whitespace-separated word that, with everything but digits
/// and `/ . -` trimmed from its ends, is a day-month-year date or an ISO
/// date. A day the calendar does not have is not a date.
fn first_date_on_line(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        let token =
            word.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '.' && c != '-');
        if let Some(iso) = parse_eu_date(token).or_else(|| parse_iso_date(token)) {
            return Some(iso);
        }
    }
    None
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

/// The label of a totals row: the largest amount on it is the total.
const TOTALS_ROW_LABELS: &[&str] = &["συνολα", "totals"];

/// Words that mark a line as holding a value, for the weighted fallback.
const VALUE_WORDS: &[&str] = &["αξια", "value", TOTAL_WORD];

/// The bare word the fallback favours and the percent rule exempts.
const TOTAL_WORD: &str = "total";

/// Markers of lines that hold identifiers, never an amount: the fallback
/// skips a line with one ([`is_identifier_line`]).
///
/// They are matched as substrings, which also finds a tax-number label
/// glued to its digits (`αφμ123456789`). The MARK label is in
/// [`MARK_WORDS`] instead.
const IDENTIFIER_LINE_MARKERS: &[&str] = &[IBAN_WORD, "α.φ.μ", "αφμ"];

/// Whether a folded line holds an identifier, an IBAN, a tax number or a
/// MARK number, and so no amount.
fn is_identifier_line(folded_line: &str) -> bool {
    contains_any(folded_line, IDENTIFIER_LINE_MARKERS)
        || contains_any_keyword(folded_line, MARK_WORDS)
}

/// Weights of the fallback, stage 4 of [`find_total_amount`].
///
/// A line's weight is [`LINE`](fallback_weight::LINE) plus what its content
/// adds; an identifier line weighs nothing and is skipped. Each plausible
/// amount on the line scores the line's weight, plus
/// [`CENTS`](fallback_weight::CENTS) when it is not whole euros. The highest
/// score wins, and of equal scores the larger amount.
///
/// What the sizes say: a `€` on the line (4) counts for more than a value
/// word (3), and either counts for more than cents (2). A line with both
/// (8) therefore outranks any line with one of them, whatever the cents.
///
/// `LINE` has to be above zero or a line with neither sign would be skipped
/// like an identifier line; three unit tests fail when it is zero. The other
/// three values are not derived from anything recorded, and no test fails
/// when any one of them is set to zero.
mod fallback_weight {
    /// Every line that may hold an amount.
    pub(super) const LINE: i32 = 1;
    /// A line with one of [`VALUE_WORDS`](super::VALUE_WORDS).
    pub(super) const VALUE_WORD: i32 = 3;
    /// A line with a euro mark.
    pub(super) const EURO_MARK: i32 = 4;
    /// An amount with cents.
    pub(super) const CENTS: i32 = 2;
}

/// The total of a document, in minor units, by the four stages in the module
/// documentation ("Choosing the total").
///
/// `text` is normalized and `folded_text` is its folded form. `None` when no
/// stage finds a plausible amount.
fn find_total_amount(text: &str, folded_text: &str) -> Option<i64> {
    // Stage 1. A label decides even in a jumbled extract, where a vote
    // among bare numbers can land on the day of a date.
    if let Some(total) = find_labeled_total(text) {
        return Some(total);
    }

    // Stage 2.
    if is_utility_bill(folded_text)
        && let Some(total) = find_utility_payment_total(text)
    {
        return Some(total);
    }

    // Stage 3.
    for line in text.lines() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }
        if contains_any(&folded_line, TOTALS_ROW_LABELS)
            && let Some(largest) = largest_plausible_amount(line)
        {
            return Some(largest);
        }
    }

    // Stage 4.
    let mut weighted: Vec<(i64, i32)> = Vec::new();
    for line in text.lines() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }
        let mut line_weight = fallback_weight::LINE;
        // `αξία` on invoices means line-value; `Ημερομηνία Αξίας` is a value
        // date and must not boost a clock (`7:00` → 700 minor).
        if !is_value_date_line(&folded_line) && contains_any(&folded_line, VALUE_WORDS) {
            line_weight += fallback_weight::VALUE_WORD;
        }
        if folded_line.contains('€') {
            line_weight += fallback_weight::EURO_MARK;
        }
        if is_identifier_line(&folded_line) {
            line_weight = 0;
        }
        if line_weight == 0 {
            continue;
        }
        for amount in money_amounts_on_line(line) {
            if is_plausible_money(amount) {
                let mut score = line_weight;
                if has_cents(amount) {
                    score += fallback_weight::CENTS;
                }
                weighted.push((amount, score));
            }
        }
    }
    weighted.sort_by(|left, right| right.1.cmp(&left.1).then(right.0.cmp(&left.0)));
    weighted.first().map(|(amount, _)| *amount)
}

/// Labels that name the amount to pay. A line with one decides the total.
///
/// They are matched as substrings of the folded line, so `subtotal amount`
/// carries the label `total amount`.
const TOTAL_LABELS: &[&str] = &[
    "συνολικο ποσο πληρωμης",
    "ποσο πληρωμης",
    "συνολο τρεχοντος λογαριασμου",
    "τρεχοντος λογαριασμου",
    "πληρωτεο",
    "payable",
    "amount due",
    "grand total",
    "total due",
    "amount payable",
    "amount to pay",
    "total to pay",
    "συνολ. αξια",
    "συνολικη αξια",
    "τελ. αξια",
    "τελικη αξια",
    "total amount",
    "invoice total",
    "net payable",
];

/// Labels of the payment line on a utility bill, for the vote among amounts.
const PAYMENT_LABELS: &[&str] = &["πληρωμ", "τρεχοντος", "payable", "amount due"];

/// Deposits and guarantees are not the bill total.
const DEPOSIT_LABELS: &[&str] = &["εγγυηση", "deposit"];

/// Markers of rate, volume, area and energy-mix lines, which hold numbers
/// that are not the payment total.
const RATE_LINE_MARKERS: &[&str] = &[
    "kwh",
    "gwh",
    "kva",
    "τ.μ",
    "τμ ",
    "τιμη ζωνης",
    "συντελεστ",
    "λιγνιτ",
    "υδροηλεκτ",
    "διασυνδεσ",
    "παραγωγ",
    "x0,",
    "x 0,",
    "x0.",
    "×",
];

/// The heading of a gas volume or calorific table when an `x` follows it.
const CONSUMPTION_LABEL: &str = "καταναλωση";

/// Labels of the value date of a bank transaction.
const VALUE_DATE_LABELS: &[&str] = &["ημερομηνια αξιας", "value date"];

/// Stage 1: the amount beside the first label that has one.
///
/// Lines are taken top to bottom. A line counts when it carries one of
/// [`TOTAL_LABELS`] and is not a noise line. Its largest plausible amount is
/// the total; when it has none, the largest on the next line is, unless
/// that line is a noise line. Extraction often puts a label and its value on
/// separate lines. A label with no amount in either place is passed over.
fn find_labeled_total(text: &str) -> Option<i64> {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }
        if !contains_any(&folded_line, TOTAL_LABELS) {
            continue;
        }
        for candidate in [*line, lines.get(index + 1).copied().unwrap_or("")] {
            if candidate.is_empty() {
                continue;
            }
            let folded_candidate = folded(candidate);
            if is_noise_amount_line(&folded_candidate) && candidate != *line {
                continue;
            }
            if let Some(largest) = largest_plausible_amount(candidate) {
                return Some(largest);
            }
        }
    }
    None
}

/// Weights and thresholds of the vote in [`find_utility_payment_total`].
///
/// Each line an amount is printed on adds one score to that amount: the sum
/// of the weights of the signs the line and the amount show. All weights are
/// on one scale, so their sizes say how much each sign counts:
///
/// - A payment label (25) is the largest single weight.
/// - A line that holds only the amount (18) and an amount with cents (16)
///   come next.
/// - A `€` counts 12 on its own. A date on the line (14) and a short line
///   (8) count only together with the `€`.
/// - A whole-euro amount takes part only on an amount-only line or beside a
///   payment label, and then starts 24 points behind an amount with cents
///   (16 against -8).
/// - The band of a typical bill (+6) and the penalty for a large figure
///   (-10) are small beside the rest. They separate amounts that are
///   otherwise close.
/// - An amount printed twice or more gains 10 per printing on top of its
///   line scores: the payment total is printed several times on a Greek
///   power bill.
///
/// The vote as a whole decides the corpus fixture `zenith_electricity.txt`.
/// Why each weight and threshold has the value it has is not recorded. No
/// test fails when any one weight is set to zero, and none pins a threshold.
mod utility_vote {
    use std::ops::RangeInclusive;

    /// Every occurrence of an amount.
    pub(super) const OCCURRENCE: i32 = 1;
    /// The line has a euro mark.
    pub(super) const EURO_MARK: i32 = 12;
    /// The amount has cents.
    pub(super) const CENTS: i32 = 16;
    /// The amount is whole euros.
    pub(super) const WHOLE_EUROS: i32 = -8;
    /// The line holds the amount and nothing else.
    pub(super) const AMOUNT_ONLY_LINE: i32 = 18;
    /// The line has a euro mark and a date.
    pub(super) const DATED_EURO_LINE: i32 = 14;
    /// The line has a euro mark and is short.
    pub(super) const SHORT_EURO_LINE: i32 = 8;
    /// The line has one of [`PAYMENT_LABELS`](super::PAYMENT_LABELS).
    pub(super) const PAYMENT_LABEL: i32 = 25;
    /// The amount is in [`TYPICAL_BILL_MINOR`].
    pub(super) const TYPICAL_BILL: i32 = 6;
    /// The amount is at least [`LARGE_AMOUNT_MINOR`].
    pub(super) const LARGE_AMOUNT: i32 = -10;
    /// Each occurrence of an amount printed [`MIN_REPEATS`] times or more.
    pub(super) const PER_REPEAT: i32 = 10;

    /// Most characters of a short line.
    pub(super) const SHORT_LINE_CHARS: usize = 40;
    /// The band of a typical monthly bill, in minor units.
    pub(super) const TYPICAL_BILL_MINOR: RangeInclusive<i64> = 1_000..=50_000;
    /// The amount, in minor units, from which a figure counts as large.
    pub(super) const LARGE_AMOUNT_MINOR: i64 = 100_000;
    /// Occurrences from which an amount counts as repeated.
    pub(super) const MIN_REPEATS: i32 = 2;
}

/// Stage 2, for utility bills: a vote among the amounts.
///
/// On an electricity, gas or water bill the payment total is usually on a
/// short line with a `€`, often printed several times, and not in a rate or
/// consumption table. Every line an amount is printed on gives that amount
/// points for such signs ([`utility_vote`]), and an amount printed more than
/// once gets a bonus per printing.
///
/// Noise lines and implausible amounts take no part. Neither does a
/// whole-euro amount, unless its line holds only the amount or carries a
/// payment label: a bare whole number on these bills is more often the day
/// of a date than a total.
///
/// Returns the amount with the highest total above zero, the larger one on
/// a tie, or `None`.
fn find_utility_payment_total(text: &str) -> Option<i64> {
    use std::collections::HashMap;

    let mut scores: HashMap<i64, i32> = HashMap::new();
    let mut occurrences: HashMap<i64, i32> = HashMap::new();

    for line in text.lines() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }

        let amounts = money_amounts_on_line(line);
        if amounts.is_empty() {
            continue;
        }

        let is_short = line.chars().count() <= utility_vote::SHORT_LINE_CHARS;
        let has_euro = line.contains('€');
        let has_date = line_has_date(line);
        let is_amount_only = is_amount_only_line(line);
        let has_payment_label = contains_any(&folded_line, PAYMENT_LABELS);

        for amount in amounts {
            if !is_plausible_money(amount) {
                continue;
            }
            let amount_has_cents = has_cents(amount);
            if !amount_has_cents && !is_amount_only && !has_payment_label {
                continue;
            }

            let mut score = utility_vote::OCCURRENCE;
            if has_euro {
                score += utility_vote::EURO_MARK;
            }
            if amount_has_cents {
                score += utility_vote::CENTS;
            } else {
                score += utility_vote::WHOLE_EUROS;
            }
            if is_amount_only {
                score += utility_vote::AMOUNT_ONLY_LINE;
            }
            if has_date && has_euro {
                score += utility_vote::DATED_EURO_LINE;
            }
            if is_short && has_euro {
                score += utility_vote::SHORT_EURO_LINE;
            }
            if has_payment_label {
                score += utility_vote::PAYMENT_LABEL;
            }
            if utility_vote::TYPICAL_BILL_MINOR.contains(&amount) {
                score += utility_vote::TYPICAL_BILL;
            }
            if amount >= utility_vote::LARGE_AMOUNT_MINOR {
                score += utility_vote::LARGE_AMOUNT;
            }

            *scores.entry(amount).or_insert(0) += score;
            *occurrences.entry(amount).or_insert(0) += 1;
        }
    }

    for (amount, count) in &occurrences {
        if *count >= utility_vote::MIN_REPEATS {
            *scores.entry(*amount).or_insert(0) += count * utility_vote::PER_REPEAT;
        }
    }

    scores
        .into_iter()
        .filter(|(_, score)| *score > 0)
        .max_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)))
        .map(|(amount, _)| amount)
}

/// Whether a folded line holds numbers that are not the total, so that every
/// stage of [`find_total_amount`] skips it:
///
/// - a deposit or guarantee ([`DEPOSIT_LABELS`]);
/// - a percentage, unless the line names a total ([`names_a_total`]): a
///   total is often printed with its VAT rate, as in "Total incl. VAT 24%";
/// - a rate, volume, area or energy-mix line ([`RATE_LINE_MARKERS`]);
/// - a consumption table row, which has [`CONSUMPTION_LABEL`] and an `x`.
fn is_noise_amount_line(folded_line: &str) -> bool {
    if contains_any(folded_line, DEPOSIT_LABELS) {
        return true;
    }
    // A percentage marks a rate line, unless the line names the total: a
    // total is often printed with its VAT rate ("Total incl. VAT 24%").
    if folded_line.contains('%') && !names_a_total(folded_line) {
        return true;
    }

    contains_any(folded_line, RATE_LINE_MARKERS)
        || (folded_line.contains(CONSUMPTION_LABEL) && folded_line.contains('x'))
}

/// Whether a folded line carries a total label: one of [`TOTAL_LABELS`] or
/// the bare word `total`, which the weighted fallback also favours.
///
/// The word is matched as a substring, so `subtotal` names a total too.
fn names_a_total(folded_line: &str) -> bool {
    folded_line.contains(TOTAL_WORD) || contains_any(folded_line, TOTAL_LABELS)
}

/// Whether a folded line is the value-date line of a bank transaction.
fn is_value_date_line(folded_line: &str) -> bool {
    contains_any(folded_line, VALUE_DATE_LABELS)
}

/// Whether `line` holds one amount and nothing else: with dates blanked,
/// only digits, `,`, `.`, `€` and whitespace, and exactly one money token.
///
/// `76,65` and `76,65 €` qualify; so does `13/08/2026 76,65 €`.
fn is_amount_only_line(line: &str) -> bool {
    let stripped = mask_date_tokens(line);
    let cleaned: String = stripped
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '€' && *c != '.')
        .collect();
    let digits_and_comma = !cleaned.is_empty()
        && cleaned
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.');
    digits_and_comma && money_amounts_on_line(line).len() == 1
}

/// Whether [`first_date_on_line`] finds a date on `line`.
fn line_has_date(line: &str) -> bool {
    first_date_on_line(line).is_some()
}

/// Characters in an ISO date, `YYYY-MM-DD`.
const ISO_DATE_CHARS: usize = 10;

/// Blanks out day-first and ISO date tokens, so the numbers of a date are
/// not read as euros.
///
/// Extraction often puts `13/08/2026 72,53 €` on one line, where `13`, `08`
/// and `2026` would each be an amount. Bank receipts also print unpadded
/// dates such as `27/8/2026`.
///
/// A token only has to be written like a date ([`DateShape`]): `31/02/2026`
/// is not a date, and its digits are still not money.
fn mask_date_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut masked = String::with_capacity(line.len());
    let mut index = 0;

    while let Some(&current) = chars.get(index) {
        if is_iso_date_at(&chars, index) {
            masked.push_str(&" ".repeat(ISO_DATE_CHARS));
            index += ISO_DATE_CHARS;
        } else if let Some(len) = eu_date_len_at(&chars, index) {
            masked.push_str(&" ".repeat(len));
            index += len;
        } else {
            masked.push(current);
            index += 1;
        }
    }
    masked
}

/// Whether the ten characters from `start` are written `dddd-dd-dd`.
///
/// Only the shape is tested: `2026-99-99` qualifies, and is blanked like a
/// date.
fn is_iso_date_at(chars: &[char], start: usize) -> bool {
    let Some([y1, y2, y3, y4, '-', m1, m2, '-', d1, d2]) = chars.get(start..start + ISO_DATE_CHARS)
    else {
        return false;
    };

    [y1, y2, y3, y4, m1, m2, d1, d2]
        .into_iter()
        .all(char::is_ascii_digit)
}

/// Length of the day-month-year token that starts at `start`, if one does.
///
/// The token is one or two digits, a separator (`/`, `.` or `-`), one or two
/// digits, the same separator, and two or four digits, with no digit
/// directly before or after it. Its numbers must fit a [`DateShape`]. So
/// `27/8/2026`, `13.08.26` and `31/02/2026` are tokens, and `45/90/2026` is
/// not.
fn eu_date_len_at(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && chars[start - 1].is_ascii_digit() {
        return None;
    }
    if start >= chars.len() || !chars[start].is_ascii_digit() {
        return None;
    }

    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() && end - start < 2 {
        end += 1;
    }
    if end == start || end >= chars.len() {
        return None;
    }
    let separator = chars[end];
    if separator != '/' && separator != '.' && separator != '-' {
        return None;
    }
    end += 1;

    let month_start = end;
    while end < chars.len() && chars[end].is_ascii_digit() && end - month_start < 2 {
        end += 1;
    }
    if end == month_start || end >= chars.len() || chars[end] != separator {
        return None;
    }
    end += 1;

    let year_start = end;
    while end < chars.len() && chars[end].is_ascii_digit() && end - year_start < 4 {
        end += 1;
    }
    let year_len = end - year_start;
    if year_len != 2 && year_len != 4 {
        return None;
    }
    if end < chars.len() && chars[end].is_ascii_digit() {
        return None;
    }

    let token: String = chars[start..end].iter().collect();
    eu_date_shape(&token).map(|_| end - start)
}

/// Blanks out clock times (`h:mm`, `hh:mm`, `hh:mm:ss`), so `7:00` is not
/// read as 7,00.
fn mask_time_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut masked = String::with_capacity(line.len());
    let mut index = 0;

    while let Some(&current) = chars.get(index) {
        if let Some(len) = time_len_at(&chars, index) {
            masked.push_str(&" ".repeat(len));
            index += len;
        } else {
            masked.push(current);
            index += 1;
        }
    }
    masked
}

/// Length of the `H:MM`, `HH:MM` or `HH:MM:SS` time starting at `start`, if
/// one starts there and is not part of a longer run of digits.
fn time_len_at(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && digit_at(chars, start - 1).is_some() {
        return None;
    }

    let colon = hour_colon_at(chars, start)?;
    let minutes = two_digits_at(chars, colon + 1)?;
    if minutes > 59 {
        return None;
    }

    let mut end = colon + 3;
    let has_seconds = chars.get(end) == Some(&':')
        && two_digits_at(chars, end + 1).is_some_and(|seconds| seconds <= 59);
    if has_seconds {
        end += 3;
    }

    if digit_at(chars, end).is_some() {
        return None;
    }
    Some(end - start)
}

/// Index of the colon that ends the hour starting at `start`: a two-digit
/// hour up to 23, or else a single digit.
fn hour_colon_at(chars: &[char], start: usize) -> Option<usize> {
    let first = digit_at(chars, start)?;

    let two_digit_hour = digit_at(chars, start + 1).is_some_and(|second| first * 10 + second <= 23);
    if two_digit_hour && chars.get(start + 2) == Some(&':') {
        return Some(start + 2);
    }

    (chars.get(start + 1) == Some(&':')).then_some(start + 1)
}

/// The decimal digit at `index`, or `None` past the end or on another
/// character.
fn digit_at(chars: &[char], index: usize) -> Option<u32> {
    chars.get(index)?.to_digit(10)
}

/// The two-digit number at `index`, or `None` when either character is not a
/// digit.
fn two_digits_at(chars: &[char], index: usize) -> Option<u32> {
    Some(digit_at(chars, index)? * 10 + digit_at(chars, index + 1)?)
}

/// Minor units in one euro. The reader always reads two decimals.
const MINOR_PER_EURO: i64 = 100;

/// The amounts taken as money: 0,50 to 10 000 000,00, in minor units.
///
/// Every stage that picks an amount ignores a number outside this band. Its
/// purpose is to keep out numbers that are not currency amounts, at the cost
/// of not reading a real total or fee under 0,50.
///
/// The reasons for these two bounds in particular are not recorded. Tests
/// pin 1,40 and 1 234 567,89 as inside and 0,40 as outside; none reaches the
/// upper bound.
const PLAUSIBLE_MONEY_MINOR: RangeInclusive<i64> = 50..=1_000_000_000;

/// Whether `minor` is not a whole number of euros.
const fn has_cents(minor: i64) -> bool {
    minor % MINOR_PER_EURO != 0
}

/// Whether `minor` is in [`PLAUSIBLE_MONEY_MINOR`].
fn is_plausible_money(minor: i64) -> bool {
    PLAUSIBLE_MONEY_MINOR.contains(&minor)
}

/// The largest plausible amount written on `line`, or `None` when it has
/// none. An empty line has none.
fn largest_plausible_amount(line: &str) -> Option<i64> {
    money_amounts_on_line(line)
        .into_iter()
        .filter(|amount| is_plausible_money(*amount))
        .max()
}

/// Blanks out percentages (`24%`, `13,5 %`) so a rate is never an amount.
fn mask_percent_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut masked = String::with_capacity(line.len());
    let mut index = 0;

    while let Some(&current) = chars.get(index) {
        if let Some(len) = percent_len_at(&chars, index) {
            masked.push_str(&" ".repeat(len));
            index += len;
        } else {
            masked.push(current);
            index += 1;
        }
    }
    masked
}

/// Length of the percentage that starts at `index`: a number, any spaces
/// after it, and the `%` sign.
fn percent_len_at(chars: &[char], index: usize) -> Option<usize> {
    let is_number_char = |c: &char| c.is_ascii_digit() || matches!(c, ',' | '.');

    // Digits right after a separator are not masked here even when no number
    // precedes it ("Φ.Π.Α.24%"): the money tokenizer reads them as ".24",
    // which is not an amount.
    let continues_a_number = index
        .checked_sub(1)
        .and_then(|before| chars.get(before))
        .is_some_and(is_number_char);
    if continues_a_number || !chars.get(index)?.is_ascii_digit() {
        return None;
    }

    let number_len = chars
        .get(index..)?
        .iter()
        .take_while(|c| is_number_char(c))
        .count();
    let spaces = chars
        .get(index + number_len..)?
        .iter()
        .take_while(|c| **c == ' ')
        .count();

    let percent_at = index + number_len + spaces;
    (chars.get(percent_at) == Some(&'%')).then_some(percent_at + 1 - index)
}

/// Every money amount written on `line`, in minor units, in the order
/// written.
///
/// Dates, clock times and percentages are blanked first. What is left is cut
/// into runs of digits, `,` and `.`, and each run is read by
/// [`parse_money_token`]; a run that is not money is dropped. The amounts
/// are not tested for plausibility here.
fn money_amounts_on_line(line: &str) -> Vec<i64> {
    let line = mask_percent_tokens(&mask_time_tokens(&mask_date_tokens(line)));
    let mut amounts = Vec::new();
    let mut token = String::new();
    for character in line.chars() {
        if character.is_ascii_digit() || character == '.' || character == ',' {
            token.push(character);
        } else {
            if let Some(amount) = parse_money_token(&token) {
                amounts.push(amount);
            }
            token.clear();
        }
    }
    if let Some(amount) = parse_money_token(&token) {
        amounts.push(amount);
    }
    amounts
}

/// Most characters of a money token.
///
/// The longest token the other rules accept is `12.345.678,90`, with 13.
/// Why the limit is 14 and not 13 is not recorded; no test tells them apart.
const MAX_MONEY_TOKEN_CHARS: usize = 14;

/// Most digits of a token without separators that is read as whole euros.
///
/// A longer run of digits is an identifier (a tax number, a MARK number, a
/// piece of an IBAN), so a whole amount of 100 000 euros or more is read
/// only when it is written with a separator.
/// `a_money_token_is_read_by_the_separator_table` pins both sides: `99999`
/// is money and `123456` is not.
const MAX_BARE_EURO_DIGITS: usize = 5;

/// Whole-euro figures that are taken as years, not money.
///
/// A date that the masks did not recognize leaves its year behind as a token
/// of its own. The price is that a total of exactly 1900 to 2100 euros is
/// read only when it is written with decimals (`2026,00`).
/// `a_whole_amount_that_looks_like_a_year_needs_decimals` pins both ends.
const YEAR_LIKE_EUROS: RangeInclusive<i64> = 1_900..=2_100;

/// Most digits of the whole part of a token with separators, after the
/// thousands marks are removed. Eight digits reach 99 999 999,99, which is
/// already above [`PLAUSIBLE_MONEY_MINOR`]; the table test pins that nine
/// are refused (`123456789,00`).
const MAX_WHOLE_PART_DIGITS: usize = 8;

/// Most digits of a fraction. Three or more after a lone separator make a
/// thousands group or no amount at all, per the module table.
const MAX_CENT_DIGITS: usize = 2;

/// Reads one run of digits, `,` and `.` as minor units, by the table in the
/// module documentation ("Reading an amount").
///
/// Trailing separators are dropped first. `None` when the token is empty,
/// longer than [`MAX_MONEY_TOKEN_CHARS`], or not money by the table.
fn parse_money_token(token: &str) -> Option<i64> {
    // The tokenizer keeps `.` and `,`, so an amount that ends a sentence or a
    // list item arrives with that punctuation attached.
    let token = token.trim().trim_end_matches(['.', ',']);
    if token.is_empty() || token.len() > MAX_MONEY_TOKEN_CHARS {
        return None;
    }

    if token.contains([',', '.']) {
        separated_to_minor(token)
    } else {
        whole_euros_to_minor(token)
    }
}

/// Reads a token of digits only as whole euros.
///
/// `None` for more than [`MAX_BARE_EURO_DIGITS`] digits, for a leading zero
/// on more than one digit (`08` is a piece of a date), and for a figure in
/// [`YEAR_LIKE_EUROS`].
fn whole_euros_to_minor(digits: &str) -> Option<i64> {
    if digits.len() > MAX_BARE_EURO_DIGITS {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }

    let whole: i64 = digits.parse().ok()?;
    if YEAR_LIKE_EUROS.contains(&whole) {
        return None;
    }
    whole.checked_mul(MINOR_PER_EURO)
}

/// The role each separator plays in a money token, per the module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeparatorRoles {
    /// The decimal mark, when the token has a fraction.
    decimal: Option<char>,
    /// The mark between thousands groups.
    grouping: char,
}

/// Decides which separator of `token` is the decimal mark.
///
/// Returns `None` when no reading fits: one kind of separator whose last
/// occurrence is followed by four or more digits, or which appears several
/// times with one or two digits after the last; and a token with no
/// separator at all.
fn separator_roles(token: &str) -> Option<SeparatorRoles> {
    let lone = |mark: char, other: char| {
        let (_, tail) = token.rsplit_once(mark)?;
        let only_one = token.matches(mark).count() == 1;

        match tail.len() {
            1..=MAX_CENT_DIGITS if only_one => Some(SeparatorRoles {
                decimal: Some(mark),
                grouping: other,
            }),
            THOUSANDS_GROUP_DIGITS => Some(SeparatorRoles {
                decimal: None,
                grouping: mark,
            }),
            _ => None,
        }
    };

    match (token.rfind(','), token.rfind('.')) {
        (Some(comma), Some(dot)) if comma > dot => Some(SeparatorRoles {
            decimal: Some(','),
            grouping: '.',
        }),
        (Some(_), Some(_)) => Some(SeparatorRoles {
            decimal: Some('.'),
            grouping: ',',
        }),
        (Some(_), None) => lone(',', '.'),
        (None, Some(_)) => lone('.', ','),
        (None, None) => None,
    }
}

/// Reads a token that holds `,` or `.` as minor units, per the module table.
fn separated_to_minor(token: &str) -> Option<i64> {
    let roles = separator_roles(token)?;

    let (whole, cents) = match roles.decimal {
        Some(mark) => {
            let (whole, cents) = token.rsplit_once(mark)?;
            (whole, Some(cents))
        }
        None => (token, None),
    };

    let whole = if whole.contains(roles.grouping) {
        join_thousands_groups(whole, roles.grouping)?
    } else {
        whole.to_owned()
    };
    decimal_to_minor(&whole, cents)
}

/// Joins `1.234.567` into `1234567` when the groups are well formed: one to
/// three leading digits that do not start with zero, then groups of three.
///
/// The zero rule keeps a three-decimal fraction such as a unit price of
/// `0,085` from being read as thousands.
fn join_thousands_groups(whole: &str, grouping: char) -> Option<String> {
    let mut groups = whole.split(grouping);

    let first = groups.next()?;
    if !(1..=THOUSANDS_GROUP_DIGITS).contains(&first.len()) || first.starts_with('0') {
        return None;
    }

    let mut joined = first.to_owned();
    for group in groups {
        if group.len() != THOUSANDS_GROUP_DIGITS {
            return None;
        }
        joined.push_str(group);
    }
    Some(joined)
}

/// `whole` euros and an optional fraction of one or two digits, as minor
/// units. A one-digit fraction is tenths: `45,9` is 45,90.
///
/// `whole` has no separators left. `None` when it is empty, longer than
/// [`MAX_WHOLE_PART_DIGITS`] or not all digits, or when the fraction is not
/// one or two digits. Leading zeros in `whole` are accepted: `01,50` is
/// 1,50.
fn decimal_to_minor(whole: &str, cents: Option<&str>) -> Option<i64> {
    let all_digits = |digits: &str| digits.bytes().all(|byte| byte.is_ascii_digit());

    if whole.is_empty() || whole.len() > MAX_WHOLE_PART_DIGITS || !all_digits(whole) {
        return None;
    }

    let cents = match cents {
        Some(cents) if !(1..=MAX_CENT_DIGITS).contains(&cents.len()) || !all_digits(cents) => {
            return None;
        }
        Some(cents) => format!("{cents:0<MAX_CENT_DIGITS$}").parse::<i64>().ok()?,
        None => 0,
    };
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(MINOR_PER_EURO)?
        .checked_add(cents)
}

/// The date to suggest for a document, as `YYYY-MM-DD`. In order of
/// preference:
///
/// 1. the first date on a line that also has a `€`: on a utility payment
///    slip that is the due date printed beside the amount to pay;
/// 2. the first date on a line with a date label ([`DATE_LABELS`]);
/// 3. the first date anywhere.
///
/// `None` when the text has no date.
fn find_best_date(text: &str) -> Option<String> {
    for line in text.lines() {
        if line.contains('€')
            && let Some(iso) = first_date_on_line(line)
        {
            return Some(iso);
        }
    }

    for line in text.lines() {
        if contains_any(&folded(line), DATE_LABELS)
            && let Some(iso) = first_date_on_line(line)
        {
            return Some(iso);
        }
    }

    text.lines().find_map(first_date_on_line)
}

/// Labels of a date line: "date", "issued", "expires", "due". Stems, so they
/// match every inflection.
const DATE_LABELS: &[&str] = &["ημερομην", "date", "εκδοσ", "ληξ", "due"];

/// The years a document date may have.
///
/// A day-first token with a year outside this range is not a date, so it is
/// not blanked either and its numbers can be read as money. An ISO-shaped
/// token is blanked by its shape whatever its year, and is then not a date.
///
/// The reason for these two years is not recorded, and no test pins either.
const DOCUMENT_YEARS: RangeInclusive<i32> = 1990..=2100;

/// What a two-digit year is counted from: `26` is 2026.
const TWO_DIGIT_YEAR_CENTURY: i32 = 2000;

/// The numbers of a token written like a date: a year in 1990..=2100, a
/// month in 1..=12 and a day in 1..=31.
///
/// The day need not exist in that month. The shape alone decides that the
/// digits are not money; only [`DateShape::to_iso`] decides that they are a
/// date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DateShape {
    /// The year, in [`DOCUMENT_YEARS`].
    year: i32,
    /// The month, 1 to 12.
    month: u8,
    /// The day, 1 to 31, whatever the month.
    day: u8,
}

impl DateShape {
    /// The shape, or `None` when a number is out of its range.
    fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let in_range =
            DOCUMENT_YEARS.contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day);

        in_range.then_some(Self { year, month, day })
    }

    /// The date as `YYYY-MM-DD`, or `None` when the calendar has no such day
    /// (31 February, 29 February outside a leap year).
    fn to_iso(self) -> Option<String> {
        let month = time::Month::try_from(self.month).ok()?;
        let date = time::Date::from_calendar_date(self.year, month, self.day).ok()?;

        Some(format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            u8::from(date.month()),
            date.day()
        ))
    }
}

/// The numbers of a `YYYY-MM-DD` token, or `None` when it is not three
/// numbers joined by `-` or they do not fit a [`DateShape`].
fn iso_date_shape(token: &str) -> Option<DateShape> {
    let mut parts = token.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    DateShape::new(year, month, day)
}

/// The numbers of a day-month-year token, or `None` when it is not three
/// numbers joined by one of `/`, `.` and `-`, or they do not fit a
/// [`DateShape`].
///
/// The order is always day, month, year; a month-first date is misread or
/// refused. A year under 100 is counted from [`TWO_DIGIT_YEAR_CENTURY`].
fn eu_date_shape(token: &str) -> Option<DateShape> {
    let separator = ['/', '.', '-']
        .into_iter()
        .find(|separator| token.contains(*separator))?;

    let mut parts = token.split(separator);
    let day = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    let year = if year < 100 {
        year + TWO_DIGIT_YEAR_CENTURY
    } else {
        year
    };
    DateShape::new(year, month, day)
}

/// A `YYYY-MM-DD` token as that same date, or `None` when it is not a day
/// the calendar has.
fn parse_iso_date(token: &str) -> Option<String> {
    iso_date_shape(token)?.to_iso()
}

/// A day-month-year token as `YYYY-MM-DD`, or `None` when it is not a day
/// the calendar has.
fn parse_eu_date(token: &str) -> Option<String> {
    eu_date_shape(token)?.to_iso()
}

/// The document's reference, by the first of these that yields one: a
/// labelled supply code, a bare NGS supply code, an RF payment code, a MARK
/// number, a number next to an invoice label, the longest long number.
fn find_invoice_reference(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();

    labelled_supply_code(&lines)
        .or_else(|| bare_ngs_supply_code(&lines))
        .or_else(|| lines.iter().find_map(|line| rf_payment_code(line)))
        .or_else(|| mark_number(&lines))
        .or_else(|| labelled_reference_number(&lines))
        .or_else(|| longest_reference_number(&lines))
}

/// Labels of a supply or meter code. `ηκασπ` is the Greek abbreviation for a
/// gas delivery point code.
const SUPPLY_CODE_LABELS: &[&str] = &["κωδικος παροχης", "supply", "ηκασπ"];

/// Lengths of an alphanumeric supply code.
///
/// The codes in the tests have 10 and 12 characters. The reasons for 8 and
/// 24 are not recorded, and no test pins either.
const SUPPLY_CODE_CHARS: RangeInclusive<usize> = 8..=24;

/// Fewest characters of an RF payment code, the `RF` included.
///
/// `reference_finds_an_rf_payment_code_as_a_token` pins that nine are too
/// few. The reason for 10 is not recorded.
const MIN_RF_CODE_CHARS: usize = 10;

/// The supply or meter code beside the first supply label that has one
/// (`NGS000000001`), on the label's line or the next: extraction often puts
/// the value on the line after its label.
fn labelled_supply_code(lines: &[&str]) -> Option<String> {
    for (index, line) in lines.iter().enumerate() {
        if !contains_any(&folded(line), SUPPLY_CODE_LABELS) {
            continue;
        }

        let next = lines.get(index + 1).copied().unwrap_or("");
        if let Some(code) = alnum_supply_code(line).or_else(|| alnum_supply_code(next)) {
            return Some(code);
        }
    }
    None
}

/// The first supply code anywhere that starts with `NGS`, labelled or not.
fn bare_ngs_supply_code(lines: &[&str]) -> Option<String> {
    lines
        .iter()
        .filter_map(|line| alnum_supply_code(line))
        .find(|code| code.to_ascii_uppercase().starts_with("NGS"))
}

/// The RF payment code of a Greek utility bill on `line`, in upper case: a
/// word of its own, or the run that starts at the first `RF` when
/// extraction glued the code to its neighbours.
fn rf_payment_code(line: &str) -> Option<String> {
    let token = line
        .split_whitespace()
        .map(str::to_ascii_uppercase)
        .find(|token| is_rf_payment_code(token));
    if token.is_some() {
        return token;
    }

    let upper = line.to_ascii_uppercase();
    let start = upper.find("RF")?;
    let glued: String = upper
        .get(start..)?
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    is_rf_payment_code(&glued).then_some(glued)
}

/// Whether uppercased `upper` is an RF payment code: `RF` then digits only,
/// at least [`MIN_RF_CODE_CHARS`] characters in all.
fn is_rf_payment_code(upper: &str) -> bool {
    upper.len() >= MIN_RF_CODE_CHARS && is_rf_then_digits(upper)
}

/// Whether uppercased `upper` is `RF` followed by digits only, whatever its
/// length. `RF` alone counts: nothing follows it that is not a digit.
fn is_rf_then_digits(upper: &str) -> bool {
    upper.starts_with("RF") && upper.chars().skip(2).all(|c| c.is_ascii_digit())
}

/// The label of a MARK number, the registration number the Greek tax
/// authority gives an invoice, in Greek and in Latin letters.
///
/// Whole words, since shop names contain both: `Supermarket`,
/// `Marks & Spencer`, `ΣΟΥΠΕΡ ΜΑΡΚΕΤ`.
const MARK_WORDS: &[Keyword] = &[Keyword::Word("μαρκ"), Keyword::Word("mark")];

/// Labels of the serial number (`α.α`), which heads the same table row as
/// the MARK. Matched as substrings, which also finds one glued to its
/// digits.
const MARK_ROW_LABELS: &[&str] = &["α.α", "αα "];

/// Whether a folded line is a MARK line: it has the MARK label or a serial
/// number label.
fn is_mark_line(folded_line: &str) -> bool {
    contains_any_keyword(folded_line, MARK_WORDS) || contains_any(folded_line, MARK_ROW_LABELS)
}

/// The MARK number of a Greek invoice: a run of digits on the first MARK or
/// `Α.Α.` line that has one on it or on the line below.
fn mark_number(lines: &[&str]) -> Option<String> {
    for (index, line) in lines.iter().enumerate() {
        if !is_mark_line(&folded(line)) {
            continue;
        }

        let next = lines.get(index + 1).copied().unwrap_or("");
        if let Some(number) = long_digit_token(line).or_else(|| long_digit_token(next)) {
            return Some(number);
        }
    }
    None
}

/// Labels of an invoice or reference number.
const REFERENCE_LABELS: &[&str] = &["invoice", "αρ. παραστατ", "αριθμος", "number", "ref"];

/// The run of digits on the first line that names an invoice or reference
/// number and has one.
fn labelled_reference_number(lines: &[&str]) -> Option<String> {
    lines.iter().find_map(|line| {
        if contains_any(&folded(line), REFERENCE_LABELS) {
            long_digit_token(line)
        } else {
            None
        }
    })
}

/// Fewest digits of a reference that no label names.
///
/// `reference_falls_back_to_the_longest_run_of_ten_or_more_digits` pins that
/// nine are too few. The reason for 10 is not recorded.
const MIN_UNLABELLED_REFERENCE_DIGITS: usize = 10;

/// The longest run of digits anywhere that can be an unlabelled reference:
/// [`MIN_UNLABELLED_REFERENCE_DIGITS`] to 20 digits. The first one wins a
/// tie. This is the last resort of [`find_invoice_reference`].
fn longest_reference_number(lines: &[&str]) -> Option<String> {
    let mut longest: Option<String> = None;
    for number in lines.iter().filter_map(|line| long_digit_token(line)) {
        if number.len() >= MIN_UNLABELLED_REFERENCE_DIGITS
            && longest
                .as_ref()
                .is_none_or(|longest| number.len() > longest.len())
        {
            longest = Some(number);
        }
    }
    longest
}

/// Lengths of a run of digits that can be a reference. A MARK number has 15.
///
/// Both ends are pinned: five digits are too few
/// (`reference_reads_a_number_next_to_an_invoice_label`) and twenty-one too
/// many (`reference_falls_back_to_the_longest_run_of_ten_or_more_digits`).
/// The reasons for 6 and 20 are not recorded.
const REFERENCE_DIGITS: RangeInclusive<usize> = 6..=20;

/// The longest run of digits on `line` whose length is in
/// [`REFERENCE_DIGITS`]; the last such run on a tie.
///
/// A run longer than the range is passed over whole; no part of it is
/// taken.
fn long_digit_token(line: &str) -> Option<String> {
    let mut longest: Option<String> = None;
    let mut run = String::new();

    let end_run = |run: &mut String, longest: &mut Option<String>| {
        if REFERENCE_DIGITS.contains(&run.len())
            && longest
                .as_ref()
                .is_none_or(|longest| run.len() >= longest.len())
        {
            *longest = Some(run.clone());
        }
        run.clear();
    };

    for character in line.chars() {
        if character.is_ascii_digit() {
            run.push(character);
        } else {
            end_run(&mut run, &mut longest);
        }
    }
    end_run(&mut run, &mut longest);
    longest
}

/// An alphanumeric supply or delivery-point code on `line`, such as
/// `NGS000000001`, as written.
///
/// First the whitespace-separated words are tried, with the punctuation
/// around each trimmed. Then every run of ASCII letters and digits in the
/// line is, which finds a code that extraction glued to other text with
/// punctuation (`text;NGS000000042;more`). A code has
/// [`SUPPLY_CODE_CHARS`] characters with at least one letter and one digit,
/// and is not an RF payment code.
fn alnum_supply_code(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        let token = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if SUPPLY_CODE_CHARS.contains(&token.len())
            && token.chars().any(|c| c.is_ascii_alphabetic())
            && token.chars().any(|c| c.is_ascii_digit())
            && token.chars().all(|c| c.is_ascii_alphanumeric())
        {
            // An RF payment code has this shape too, and has a step of its own.
            if is_rf_then_digits(&token.to_ascii_uppercase()) {
                continue;
            }
            return Some(token.to_owned());
        }
    }
    let mut run = String::new();
    for character in line.chars() {
        if character.is_ascii_alphanumeric() {
            run.push(character);
        } else {
            if let Some(code) = supply_code_from_run(&run) {
                return Some(code);
            }
            run.clear();
        }
    }
    supply_code_from_run(&run)
}

/// `run` as a supply code, when a run of ASCII letters and digits has the
/// shape of one (see [`alnum_supply_code`]).
fn supply_code_from_run(run: &str) -> Option<String> {
    if SUPPLY_CODE_CHARS.contains(&run.len())
        && run.chars().any(|c| c.is_ascii_alphabetic())
        && run.chars().any(|c| c.is_ascii_digit())
    {
        if is_rf_then_digits(&run.to_ascii_uppercase()) {
            return None;
        }
        Some(run.to_owned())
    } else {
        None
    }
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

    fn reference(text: &str) -> Option<String> {
        find_invoice_reference(text)
    }

    fn time_len(text: &str, at: usize) -> Option<usize> {
        let chars: Vec<char> = text.chars().collect();
        time_len_at(&chars, at)
    }

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
        ("MARK_ROW_LABELS", MARK_ROW_LABELS),
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

    /// Every keyword constant of the reader, checked like [`LABEL_SETS`].
    const KEYWORD_SETS: &[(&str, &[Keyword])] = &[("MARK_WORDS", MARK_WORDS)];

    #[test]
    fn every_label_and_marker_is_in_folded_form() {
        let keywords = KEYWORD_SETS.iter().map(|(name, keywords)| {
            let texts: Vec<&str> = keywords.iter().map(|keyword| keyword.text()).collect();
            (*name, texts)
        });
        let labels = LABEL_SETS
            .iter()
            .map(|(name, needles)| (*name, needles.to_vec()));

        for (name, needles) in labels.chain(keywords) {
            for needle in needles {
                assert_eq!(
                    folded(needle),
                    needle,
                    "{name}: {needle:?} can never match folded text"
                );
            }
        }
    }

    #[test]
    fn folding_lowercases_and_drops_greek_accents() {
        assert_eq!(folded("Τελική Αξία"), "τελικη αξια");
        assert_eq!(folded("ΤΕΛΙΚΗ ΑΞΙΑ"), "τελικη αξια");
        assert_eq!(folded("Ϊ ΰ Ώ"), "ι υ ω");
        assert_eq!(folded("Total 24%"), "total 24%");
    }

    #[test]
    fn reference_prefers_a_labelled_supply_code_on_the_same_or_next_line() {
        assert_eq!(
            reference("Supply number: NGS000000001\nInvoice 1234567890"),
            Some("NGS000000001".into())
        );
        assert_eq!(
            reference("Κωδικός παροχής\nAB12345678\nRF12345678901234"),
            Some("AB12345678".into())
        );
        assert_eq!(
            reference("ΚΩΔΙΚΟΣ ΠΑΡΟΧΗΣ: 9 XY99887766 1"),
            Some("XY99887766".into())
        );
    }

    #[test]
    fn reference_finds_an_unlabelled_ngs_code_anywhere() {
        assert_eq!(
            reference("Total 45,90\nsome text;NGS000000042;more\nInvoice 1234567890"),
            Some("NGS000000042".into())
        );
        // Other alphanumeric codes need a label; this one falls through to the number.
        assert_eq!(
            reference("ZZ12345678 somewhere\nInvoice 1234567890"),
            Some("1234567890".into())
        );
    }

    #[test]
    fn reference_finds_an_rf_payment_code_as_a_token() {
        assert_eq!(
            reference("Pay with rf12345678901234 today\nInvoice 777777"),
            Some("RF12345678901234".into())
        );
        // Too short to be a payment code.
        assert_eq!(reference("RF1234567"), None);
        // Letters after RF: not a payment code.
        assert_eq!(reference("RFID-reader 12"), None);
    }

    #[test]
    fn reference_finds_an_rf_payment_code_glued_to_ascii_text() {
        assert_eq!(
            reference("code:RF12345678901234/end"),
            Some("RF12345678901234".into())
        );
    }

    #[test]
    fn reference_finds_an_rf_payment_code_glued_to_greek_text() {
        // The byte offset of "RF" is larger than its character offset here;
        // treating one as the other skipped into the middle of the code.
        assert_eq!(
            reference("Κωδικός πληρωμής:RF12345678901234"),
            Some("RF12345678901234".into())
        );
    }

    #[test]
    fn reference_reads_a_mark_number_on_the_same_or_next_line() {
        assert_eq!(
            reference("ΜΑΡΚ: 400001234567890\nInvoice 123456"),
            Some("400001234567890".into())
        );
        assert_eq!(
            reference("M.AR.K (mark)\n400009876543210\nInvoice 123456"),
            Some("400009876543210".into())
        );
        // A MARK label with no number nearby falls through to the labelled invoice.
        assert_eq!(
            reference("mark\nno digits here\nInvoice 123456"),
            Some("123456".into())
        );
    }

    #[test]
    fn reference_reads_a_mark_label_in_either_script_and_any_case() {
        for label in ["MARK", "Mark", "ΜΑΡΚ", "Μαρκ", "ΜΑΡΚ:"] {
            assert_eq!(
                reference(&format!("{label} 400001234567890\nInvoice 123456")),
                Some("400001234567890".into()),
                "{label}"
            );
        }
    }

    #[test]
    fn reference_does_not_take_a_shop_name_for_a_mark_label() {
        // The phone number below the name is not a MARK number.
        assert_eq!(
            reference("ΣΟΥΠΕΡ ΜΑΡΚΕΤ ΚΡΗΤΙΚΟΣ\nΤΗΛ 2101234567\nΑρ. παραστατικού 123456"),
            Some("123456".into())
        );
        assert_eq!(
            reference("Supermarket Athens\nTel 2101234567\nInvoice 123456"),
            Some("123456".into())
        );
    }

    #[test]
    fn reference_reads_a_number_next_to_an_invoice_label() {
        for label in ["Invoice", "Αρ. παραστατικού", "Αριθμός", "Number", "Ref"]
        {
            assert_eq!(
                reference(&format!("{label}: 654321")),
                Some("654321".into()),
                "{label}"
            );
        }
        // Five digits is too short to be a reference.
        assert_eq!(reference("Invoice 12345"), None);
    }

    #[test]
    fn reference_falls_back_to_the_longest_run_of_ten_or_more_digits() {
        assert_eq!(
            reference("a 1234567890\nb 123456789012\nc 12345678901"),
            Some("123456789012".into())
        );
        // Unlabelled runs under ten digits are not references.
        assert_eq!(reference("a 123456789"), None);
        // Runs over twenty digits are not tokens at all.
        assert_eq!(reference("a 123456789012345678901"), None);
        assert_eq!(reference(""), None);
    }

    #[test]
    fn a_clock_time_is_measured_only_where_one_starts() {
        let cases = [
            ("9:05", 0, Some(4)),
            ("09:05", 0, Some(5)),
            ("23:59", 0, Some(5)),
            ("23:59:59", 0, Some(8)),
            ("at 7:30 pm", 3, Some(4)),
            // Seconds out of range: the time ends after the minutes.
            ("12:30:61", 0, Some(5)),
            // Not a valid hour or minute.
            ("24:00", 0, None),
            ("12:60", 0, None),
            ("12:5", 0, None),
            ("12:", 0, None),
            ("12", 0, None),
            ("x", 0, None),
            // A digit before or after makes it part of a longer number.
            ("112:30", 1, None),
            ("12:301", 0, None),
            ("12:30:451", 0, None),
            // Past the end.
            ("12:30", 5, None),
        ];
        for (text, at, want) in cases {
            assert_eq!(time_len(text, at), want, "{text:?} at {at}");
        }
    }

    #[test]
    fn a_money_token_is_read_by_the_separator_table() {
        let cases = [
            ("45,90", Some(4_590)),
            ("45.90", Some(4_590)),
            ("45,9", Some(4_590)),
            ("1.234,56", Some(123_456)),
            ("1,234.56", Some(123_456)),
            ("1.234.567", Some(123_456_700)),
            ("1,234,567", Some(123_456_700)),
            ("1,234", Some(123_400)),
            ("50", Some(5_000)),
            ("99999", Some(9_999_900)),
            // One separator before three digits groups thousands, dot or comma.
            ("1.234", Some(123_400)),
            ("999.999", Some(99_999_900)),
            // Unless the whole part is zero or starts with one: a fraction.
            ("0,085", None),
            ("0.971", None),
            ("01,234", None),
            // Thousands groups hold exactly three digits after the first.
            ("1.2.3", None),
            ("1234.567", None),
            ("1,23,456", None),
            // More than two decimals is not money.
            ("1,2345", None),
            // Dates, years and identifiers are not money.
            ("08", None),
            ("2026", None),
            ("123456", None),
            ("123456789,00", None),
            ("12,345.678", None),
            ("", None),
            (",50", None),
            // A trailing comma or dot is sentence punctuation.
            ("5,", Some(500)),
            ("5.", Some(500)),
            ("45,90.", Some(4_590)),
            ("1.234,56,", Some(123_456)),
            (".", None),
            ("1.2.3,4.5", None),
            ("123456789012345", None),
        ];
        for (token, want) in cases {
            assert_eq!(parse_money_token(token), want, "{token:?}");
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
    fn eur_token_replacement_keeps_words() {
        assert_eq!(replace_eur_token("TOTAL 10 EUR"), "TOTAL 10 €");
        assert_eq!(replace_eur_token("10eur"), "10€");
        assert_eq!(replace_eur_token("EUROBANK EUROPE"), "EUROBANK EUROPE");
        assert_eq!(replace_eur_token("EUR"), "€");
    }

    #[test]
    fn a_bare_total_word_is_read_by_the_weighted_fallback() {
        let text = "Invoice\nSubtotal 10,00\nTOTAL 45,90 EUR\nThank you";

        let suggestion = parse_invoice_text(text, crate::prefs::Locale::En);

        assert_eq!(suggestion.amount_minor, Some(4590));
    }

    #[test]
    fn a_tax_number_is_not_money() {
        assert_eq!(parse_money_token("000000000"), None);
        assert_eq!(parse_money_token("900000000000001"), None);
        assert_eq!(parse_money_token("1860,00"), Some(186_000));
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
            suggestion.description
                .as_deref()
                .is_some_and(|description| description.starts_with("ACME CONSULTING LTD — Invoice")),
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
    fn an_electricity_bill_total_is_found_by_the_utility_vote() {
        let suggestion = parse_invoice_text(
            &corpus_text("synthetic/text/zenith_electricity.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(
            suggestion.amount_minor,
            Some(7_665),
            "expected €76.65, got {:?}",
            suggestion.amount_minor
        );
        assert!(
            matches!(
                suggestion.kind,
                EntryKindSuggestion::Bill | EntryKindSuggestion::Expense
            ),
            "kind={:?}",
            suggestion.kind
        );
        assert_ne!(suggestion.kind, EntryKindSuggestion::Income);
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-08-18"));
        assert!(
            suggestion
                .merchant
                .as_deref()
                .is_some_and(|merchant| merchant.to_lowercase().contains("zeni")),
            "merchant={:?}",
            suggestion.merchant
        );
        assert!(
            suggestion
                .reference
                .as_deref()
                .is_some_and(|reference| reference.starts_with("RF")),
            "reference={:?}",
            suggestion.reference
        );
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
                default_currency: "EUR",
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
    fn a_gas_bill_reads_its_payment_total_not_its_deposit() {
        let suggestion = parse_invoice_text(
            &corpus_text("synthetic/text/ngs_gas_bill.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(
            suggestion.amount_minor,
            Some(7_253),
            "expected €72.53 (not deposit 60 or subtotal 50.50), got {:?}",
            suggestion.amount_minor
        );
        assert_ne!(suggestion.kind, EntryKindSuggestion::Income);
        assert!(
            matches!(
                suggestion.kind,
                EntryKindSuggestion::Bill | EntryKindSuggestion::Expense
            ),
            "kind={:?}",
            suggestion.kind
        );
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-08-13"));
        assert!(
            suggestion.merchant.as_deref().is_some_and(|merchant| {
                let merchant = merchant.to_lowercase();
                merchant.contains("gas") || merchant.contains("ngs")
            }),
            "merchant={:?}",
            suggestion.merchant
        );
        assert!(
            suggestion.reference.as_deref().is_some_and(|reference| {
                reference.contains("NGS")
                    || reference.contains("SYN")
                    || reference.starts_with("RF")
            }),
            "reference={:?}",
            suggestion.reference
        );
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

    #[test]
    fn date_tokens_are_not_money() {
        assert!(money_amounts_on_line("13/08/2026 72,53 €").contains(&7_253));
        assert!(!money_amounts_on_line("13/08/2026 72,53 €").contains(&1_300));
        assert!(!money_amounts_on_line("26/05/2026 30/06/2026").contains(&2_600));
        assert!(!money_amounts_on_line("27/8/2026 310,00").contains(&2_700));
        assert!(!money_amounts_on_line("27/8/2026 310,00").contains(&800));
        assert!(money_amounts_on_line("27/8/2026 310,00").contains(&31_000));
        assert_eq!(parse_money_token("08"), None);
        assert_eq!(parse_money_token("2026"), None);
    }

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn a_shop_name_that_contains_mark_does_not_hide_its_amount_from_the_fallback() {
        assert_eq!(read("Supermarket 12,50 €").amount_minor, Some(1_250));
        assert_eq!(read("Marks & Spencer 8,40 €").amount_minor, Some(840));
        assert_eq!(read("Supermarkt 12,50 €").amount_minor, Some(1_250));
        assert_eq!(read("ΣΟΥΠΕΡ ΜΑΡΚΕΤ 12,50 €").amount_minor, Some(1_250));
        // The change given back is not the amount paid.
        assert_eq!(
            read("Supermarket purchase 12,50 €\nChange 7,50").amount_minor,
            Some(1_250)
        );
        // A labelled total is still found before the fallback runs.
        assert_eq!(
            read("Supermarket\nAmount due 12,50 €").amount_minor,
            Some(1_250)
        );
    }

    #[test]
    fn a_mark_line_is_still_skipped_by_the_fallback() {
        for label in ["MARK", "ΜΑΡΚ"] {
            assert_eq!(
                read(&format!("Coffee 3,20 €\n{label} 400001234567890 45,00 €")).amount_minor,
                Some(320),
                "{label}"
            );
        }
    }

    #[test]
    fn a_date_before_an_amount_is_not_its_thousands_prefix() {
        let padded = read("13/08/2026 172,53 €");
        assert_eq!(padded.amount_minor, Some(17_253));
        assert_eq!(padded.entry_date.as_deref(), Some("2026-08-13"));

        let unpadded = read("27/8/2026 310,00");
        assert_eq!(unpadded.amount_minor, Some(31_000));
        assert_eq!(unpadded.entry_date.as_deref(), Some("2026-08-27"));

        // A two-digit year ends in a group short enough to be a prefix.
        let short_year = read("13/08/26 172,53 €");
        assert_eq!(short_year.amount_minor, Some(17_253));
        assert_eq!(short_year.entry_date.as_deref(), Some("2026-08-13"));
    }

    #[test]
    fn space_grouped_thousands_are_one_amount() {
        assert_eq!(read("TOTAL 1 234,56").amount_minor, Some(123_456));
        assert_eq!(read("TOTAL 12 345,00 €").amount_minor, Some(1_234_500));
        assert_eq!(read("TOTAL 1 234 567,89").amount_minor, Some(123_456_789));
    }

    #[test]
    fn a_number_that_cannot_be_a_thousands_prefix_stays_separate() {
        // More than three digits are not a thousands prefix.
        assert_eq!(read("Order 123456 500,00").amount_minor, Some(50_000));
        // A finished decimal amount is not one either.
        assert_eq!(read("Amount due 45,90 120,00").amount_minor, Some(12_000));
        // Nor is the tail of a code or of a dotted number.
        assert_eq!(read("Ref A-7 120,50").amount_minor, Some(12_050));
        assert_eq!(read("Item B7 120,50").amount_minor, Some(12_050));
        // A number on the line above is a different number.
        assert_eq!(read("Quantity 5\n120,50").amount_minor, Some(12_050));
    }

    #[test]
    fn a_lone_digit_before_three_digits_reads_as_thousands() {
        // "5 120,50" is spelled exactly like "1 234,56", so a quantity column
        // next to a three-digit price reads as one amount. A labelled total
        // on the document still decides.
        assert_eq!(read("Qty 5 120,50").amount_minor, Some(512_050));
        assert_eq!(
            read("Qty 5 120,50\nAmount due: 602,50").amount_minor,
            Some(60_250)
        );
    }

    #[test]
    fn sentence_punctuation_after_an_amount_is_not_part_of_it() {
        assert_eq!(read("TOTAL 45,90.").amount_minor, Some(4_590));
        assert_eq!(read("Amount due: 45,90.").amount_minor, Some(4_590));
        assert_eq!(
            read("Amount due: 45.90, thank you").amount_minor,
            Some(4_590)
        );
        assert_eq!(read("Amount due: 45.").amount_minor, Some(4_500));
        assert_eq!(read("Amount due: 45,").amount_minor, Some(4_500));
        // A number after the full stop belongs to the next sentence.
        assert_eq!(read("TOTAL 45,90. 3 items").amount_minor, Some(4_590));
        assert_eq!(
            read("Amount due: 1.234,56. 2 pages").amount_minor,
            Some(123_456)
        );
        // A spaced decimal after a thousands group is still one amount.
        assert_eq!(read("Amount due: 1.234, 56").amount_minor, Some(123_456));
        assert_eq!(read("Amount due: 72, 53").amount_minor, Some(7_253));
    }

    #[test]
    fn one_separator_before_three_digits_groups_thousands_for_comma_and_dot() {
        assert_eq!(read("Amount due 1,234").amount_minor, Some(123_400));
        assert_eq!(read("Amount due 1.234").amount_minor, Some(123_400));
        assert_eq!(read("Amount due 12.345 €").amount_minor, Some(1_234_500));
        assert_eq!(read("Amount due 2,500").amount_minor, Some(250_000));
    }

    #[test]
    fn a_three_decimal_fraction_is_not_a_thousands_amount() {
        assert_eq!(read("Amount due 0,085").amount_minor, None);
        assert_eq!(read("Amount due 0.971").amount_minor, None);
        assert_eq!(read("Amount due 01,234").amount_minor, None);
        assert_eq!(
            read("Unit price 0,085\nAmount due 12,40").amount_minor,
            Some(1_240)
        );
    }

    #[test]
    fn thousands_groups_must_be_well_formed() {
        assert_eq!(read("Amount due 1.2.3").amount_minor, None);
        assert_eq!(read("Amount due 1234.567,00").amount_minor, None);
        assert_eq!(
            read("Amount due 1.234.567,00").amount_minor,
            Some(123_456_700)
        );
    }

    #[test]
    fn a_day_the_month_does_not_have_is_not_a_date() {
        for impossible in ["31/02/2026", "29/02/2026", "31.04.2026", "2026-02-31"] {
            let suggestion = read(&format!("Invoice\nDate {impossible}\nTOTAL 45,90"));

            assert_eq!(suggestion.entry_date, None, "{impossible}");
            assert_eq!(suggestion.amount_minor, Some(4_590), "{impossible}");
        }

        assert_eq!(
            read("Date 29/02/2024").entry_date.as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(
            read("Date 2024-02-29").entry_date.as_deref(),
            Some("2024-02-29")
        );
    }

    #[test]
    fn the_digits_of_an_impossible_date_are_still_not_money() {
        assert_eq!(read("Amount due 31/02/2026").amount_minor, None);
        assert_eq!(read("Amount due 2026-02-31").amount_minor, None);
    }

    #[test]
    fn a_total_line_that_states_a_vat_rate_is_still_the_total() {
        assert_eq!(
            read("TOTAL (incl. 24% VAT) 45,90").amount_minor,
            Some(4_590)
        );
        assert_eq!(
            read("Total incl. VAT 24%: 124,00\nSubtotal 100,00").amount_minor,
            Some(12_400)
        );
        assert_eq!(
            read("Amount due (VAT 24 %) 124,00\nNet 100,00").amount_minor,
            Some(12_400)
        );
        // A rate glued to an abbreviation is still a rate.
        assert_eq!(read("Total 5,00 (Φ.Π.Α.24%)").amount_minor, Some(500));
        // The rate itself is never the amount.
        assert_eq!(read("Amount due incl. 24% VAT").amount_minor, None);
        assert_eq!(read("Total 13,5% VAT").amount_minor, None);
    }

    #[test]
    fn a_rate_line_without_a_total_label_is_still_skipped() {
        assert_eq!(
            read("VAT 24% 24,00\nAmount due 124,00").amount_minor,
            Some(12_400)
        );
        assert_eq!(read("Discount 10% 5,00").amount_minor, None);
    }

    #[test]
    fn an_all_caps_total_label_reads_like_the_accented_one() {
        for label in ["Τελική Αξία", "ΤΕΛΙΚΗ ΑΞΙΑ", "ΤΕΛ. ΑΞΙΑ", "ΣΥΝΟΛ. ΑΞΙΑ"]
        {
            assert_eq!(
                read(&format!("{label} 124,00\nΚαθαρή 100,00 €")).amount_minor,
                Some(12_400),
                "{label}"
            );
        }
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

    #[test]
    fn clock_tokens_are_not_money() {
        assert!(!money_amounts_on_line("Ημερομηνία Αξίας 28/8/2026 7:00 μ.μ.").contains(&700));
        assert_eq!(money_amounts_on_line("7:00"), [] as [i64; 0]);
        assert_eq!(money_amounts_on_line("19:30"), [] as [i64; 0]);
    }

    #[test]
    #[ignore = "needs tests/fixtures/local_gas_bill.pdf, a private bill that is not in the tree"]
    fn a_private_gas_bill_pdf_reads_its_payment_total() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/local_gas_bill.pdf"
        );
        let bytes = std::fs::read(path).expect("the private fixture must be present");
        let text = pdf_extract::extract_text_from_mem(&bytes).expect("pdf text");
        let suggestion = parse_invoice_text(&text, crate::prefs::Locale::En);
        assert_eq!(
            suggestion.amount_minor,
            Some(7_253),
            "local pdf_extract amount={:?}\ntext excerpt:\n{}",
            suggestion.amount_minor,
            text.chars().take(800).collect::<String>()
        );
    }
}

#[cfg(test)]
mod documented_tradeoffs {
    use super::*;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
    }

    #[test]
    fn a_space_before_the_separator_is_not_joined() {
        assert_eq!(read("Amount due: 72, 53").amount_minor, Some(7_253));
        assert_eq!(read("Amount due: 72 ,53").amount_minor, Some(7_200));
    }

    #[test]
    fn a_subtotal_line_names_a_total() {
        assert!(names_a_total(&folded("Subtotal 100,00")));
        // So its percentage does not make it a rate line.
        assert!(!is_noise_amount_line(&folded("Subtotal (VAT 24%) 100,00")));
        assert!(is_noise_amount_line(&folded("Net (VAT 24%) 100,00")));
    }

    #[test]
    fn a_label_inside_a_longer_word_still_decides_the_total() {
        assert_eq!(
            read("Subtotal amount: 100,00\nBalance 124,00 €").amount_minor,
            Some(10_000)
        );
    }

    #[test]
    fn a_mark_label_glued_to_its_number_is_not_a_label() {
        assert_eq!(
            read("MARK400001234567890").reference.as_deref(),
            Some("400001234567890"),
            "the longest number still finds it"
        );
        assert_eq!(
            read("MARK400001234567890\nInvoice 123456")
                .reference
                .as_deref(),
            Some("123456"),
            "a labelled invoice number comes first"
        );
    }

    #[test]
    fn leading_zeros_in_a_whole_part_are_accepted_beside_a_decimal_mark() {
        assert_eq!(parse_money_token("01,50"), Some(150));
        assert_eq!(parse_money_token("007,00"), Some(700));
        assert_eq!(parse_money_token("08"), None);
        assert_eq!(parse_money_token("01.234"), None);
    }

    #[test]
    fn a_whole_amount_that_looks_like_a_year_needs_decimals() {
        assert_eq!(parse_money_token("2026"), None);
        assert_eq!(parse_money_token("1900"), None);
        assert_eq!(parse_money_token("2100"), None);
        assert_eq!(parse_money_token("2026,00"), Some(202_600));
        assert_eq!(parse_money_token("1899"), Some(189_900));
        assert_eq!(parse_money_token("2101"), Some(210_100));
    }

    #[test]
    fn dates_are_read_day_first() {
        assert_eq!(
            read("Date 03/04/2026").entry_date.as_deref(),
            Some("2026-04-03")
        );
        // Month first, with a day over 12: not a date.
        assert_eq!(read("Date 04/13/2026").entry_date, None);
    }

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
