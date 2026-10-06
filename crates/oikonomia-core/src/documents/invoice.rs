//! Offline structured parsing for invoices / bills / receipts.
//!
//! Tuned for European (incl. Greek) tax documents: labeled totals, MARK/AFM
//! rejection, and sales-invoice vs expense detection. Greek bank transfer
//! receipts (`έμβασμα`) are a separate class: the labeled principal is the
//! amount, never a clock on `Ημερομηνία Αξίας` or the transfer fee. No network.
//!
//! # Reading an amount
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

use super::analyze::{DocumentSuggestion, EntryKindSuggestion};
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

/// Parse extracted document text into a draft suggestion.
///
/// Bank transfer receipts stay [`EntryKindSuggestion::Expense`]. The
/// posted amount is the capital debit (`Ποσό Χρέωσης Κεφαλαίου` / `Ποσό:`),
/// not the fee and not a `hh:mm` time.
///
/// The returned notes never mention a transfer fee, even when the receipt
/// shows one: that note needs the book's currency, so
/// [`analyze_document_bytes`](super::analyze_document_bytes) adds it. Callers
/// that need the fee use `read_invoice_text`.
#[must_use]
pub fn parse_invoice_text(text: &str, locale: Locale) -> DocumentSuggestion {
    read_invoice_text(text, locale).suggestion
}

/// Parse extracted document text, keeping the detected transfer fee as data.
///
/// The suggestion is worded in `locale`; the category hint never is.
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

/// Read every field, wording the generated text in `locale`.
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
        source: super::analyze::AnalyzeSource::Heuristic,
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

fn normalize(text: &str) -> String {
    // Common OCR / PDF quirks before line-oriented parsing.
    let mut t = text.replace('\r', "\n");
    // No-break space, narrow no-break space, thin space
    t = t.replace(['\u{00a0}', '\u{202f}', '\u{2009}'], " ");
    // Euro symbol variants — standalone tokens only, so EUROBANK stays intact.
    // Before the number joins, so "EUR1 234,56" starts its amount at a symbol
    // and not at the tail of a word.
    t = replace_eur_token(&t);
    // OCR and PDF extraction split numbers with spaces: "72, 53", "1 234,56".
    t = collapse_spaced_decimals(&t);

    t.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace a standalone `EUR`/`eur` token with `€`.
///
/// A blanket `str::replace` corrupted words containing the trigram
/// (EUROBANK → €OBANK), which then leaked into merchant/description fields.
fn replace_eur_token(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let is_eur = i + 2 < chars.len()
            && chars[i].eq_ignore_ascii_case(&'e')
            && chars[i + 1].eq_ignore_ascii_case(&'u')
            && chars[i + 2].eq_ignore_ascii_case(&'r');
        let boundary_before = i == 0 || !chars[i - 1].is_alphabetic();
        let boundary_after = i + 3 >= chars.len() || !chars[i + 3].is_alphabetic();

        if is_eur && boundary_before && boundary_after {
            out.push('€');
            i += 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Joins the pieces of a number that OCR or PDF extraction split with a space.
///
/// Two joins are made, each inside one line:
///
/// - a separator and the digits after it: `72, 53` becomes `72,53`;
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

fn collapse_spaced_decimals_on_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    // Dates and clocks are blanked in this copy, so their digits never start
    // or extend a number; the output below copies them from `chars`.
    let numbers: Vec<char> = mask_time_tokens(&mask_date_tokens(line)).chars().collect();

    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    while let Some(&original) = chars.get(index) {
        if numbers.get(index).is_some_and(char::is_ascii_digit) {
            index = push_spaced_number(&numbers, index, &mut out);
        } else {
            out.push(original);
            index += 1;
        }
    }
    out
}

/// Appends the number starting at `start` to `out` with its inner spaces
/// removed, and returns the index after it.
fn push_spaced_number(numbers: &[char], start: usize, out: &mut String) -> usize {
    let mut may_group_thousands = starts_own_token(numbers, start);
    let mut group_digits = 0_usize;
    let mut index = start;

    while let Some(&current) = numbers.get(index) {
        if current.is_ascii_digit() {
            out.push(current);
            group_digits += 1;
            index += 1;
        } else if let Some(next_digit) = digit_after_separator(numbers, index) {
            out.push(current);
            may_group_thousands = false;
            index = next_digit;
        } else if may_group_thousands
            && (1..=3).contains(&group_digits)
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
        && (1..=3).all(|offset| is_digit(index + offset))
        && !is_digit(index + 4)
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
    "power business",
];

/// The heading of the counterparty block on a sales invoice.
const CUSTOMER_BLOCK_LABEL: &str = "στοιχεια πελατη";

const INVOICE_WORDS: &[&str] = &[INVOICE_WORD_GREEK, "invoice"];

const UTILITY_UNPAID_MARKERS: &[&str] = &["ληξιπροθεσμ", "ανεξοφλητ", "amount due"];

const PURCHASE_MARKERS: &[&str] = &["τιμολογιο αγορ", "purchase invoice", "supplier"];

const UNPAID_MARKERS: &[&str] = &[
    "επι πιστωσει",
    "amount due",
    "unpaid",
    "outstanding",
    "please pay",
];

const TRANSFER_MARKERS: &[&str] = &[
    "εμβασμα",
    "μεταφορα σε αλλη τραπεζα",
    TRANSFER_REFERENCE_LABEL,
];

const TRANSFER_REFERENCE_LABEL: &str = "κωδικος συναλλαγης";

fn is_utility_bill(folded_text: &str) -> bool {
    contains_any(folded_text, UTILITY_MARKERS)
}

/// Sales invoice issued by the book's owner: the counterparty block is
/// labeled "Στοιχεία Πελάτη".
fn is_sales_invoice(folded_text: &str) -> bool {
    folded_text.contains(CUSTOMER_BLOCK_LABEL) && contains_any(folded_text, INVOICE_WORDS)
}

fn classify_kind(folded_text: &str) -> (EntryKindSuggestion, bool) {
    // Utility / electricity / water / gas settlement bills are expenses/bills for the customer.
    if is_utility_bill(folded_text) {
        let unpaid = contains_any(folded_text, UTILITY_UNPAID_MARKERS);
        return (EntryKindSuggestion::Bill, unpaid);
    }

    // "Σταθερό Τιμολόγιο" is a tariff name, not a sales invoice.
    let sales = is_sales_invoice(folded_text) || folded_text.contains("sales invoice");
    let purchase = contains_any(folded_text, PURCHASE_MARKERS);
    let unpaid = contains_any(folded_text, UNPAID_MARKERS);

    if sales && !purchase {
        return (EntryKindSuggestion::Income, unpaid);
    }

    // A recognized biller with a known service (telecom etc.) is a bill to
    // pay even without the utility markers above.
    if let Some((_, Some(_))) = super::brands::known_brand(folded_text) {
        return (EntryKindSuggestion::Bill, unpaid);
    }

    if unpaid {
        (EntryKindSuggestion::Bill, true)
    } else {
        (EntryKindSuggestion::Expense, false)
    }
}

fn is_bank_transfer_receipt(folded_text: &str) -> bool {
    contains_any(folded_text, TRANSFER_MARKERS)
}

/// Expense fill for a Greek bank `έμβασμα` / other-bank transfer receipt.
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
        source: super::analyze::AnalyzeSource::Heuristic,
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

fn find_transfer_principal(text: &str) -> Option<i64> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if is_transfer_fee_line(&folded_line) {
            continue;
        }
        if folded_line.contains(TRANSFER_PRINCIPAL_LABEL)
            && let Some(amount) = amount_on_line_or_next(&lines, i)
        {
            return Some(amount);
        }
    }
    for (i, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if is_transfer_fee_line(&folded_line) {
            continue;
        }
        if is_transfer_amount_label(&folded_line)
            && let Some(amount) = amount_on_line_or_next(&lines, i)
        {
            return Some(amount);
        }
    }
    None
}

fn amount_on_line_or_next(lines: &[&str], index: usize) -> Option<i64> {
    for candidate in [lines[index], lines.get(index + 1).copied().unwrap_or("")] {
        if candidate.is_empty() {
            continue;
        }
        if let Some(amount) = money_amounts_on_line(candidate)
            .into_iter()
            .filter(|value| is_plausible_money(*value) && *value > 0)
            .max()
        {
            return Some(amount);
        }
    }
    None
}

/// The label of the capital debit on a transfer receipt.
const TRANSFER_PRINCIPAL_LABEL: &str = "ποσο χρεωσης κεφαλαιου";

/// The plain "amount" label a receipt uses when it has no capital line.
const TRANSFER_AMOUNT_LABEL: &str = "ποσο";

const TRANSFER_FEE_LABELS: &[&str] = &["προμηθεια", "εξοδων", "εξοδα"];

const BENEFICIARY_LABEL: &str = "δικαιουχου";

/// The two halves of the `Ονοματεπώνυμο / Επωνυμία` beneficiary label.
const BENEFICIARY_NAME_LABELS: [&str; 2] = ["ονοματεπωνυμο", ISSUER_NAME_LABEL];

const BANK_WORDS: &[&str] = &["τραπεζα", "bank"];

const EXECUTION_DATE_LABELS: &[&str] = &["εκτελεσ", "execution"];

const TRANSFER_DATE_LABELS: &[&str] = &["ημερομην", "date", "συναλλαγ"];

fn is_transfer_fee_line(folded_line: &str) -> bool {
    contains_any(folded_line, TRANSFER_FEE_LABELS)
}

fn is_transfer_amount_label(folded_line: &str) -> bool {
    folded_line.trim() == TRANSFER_AMOUNT_LABEL
        || (folded_line.contains(TRANSFER_AMOUNT_LABEL)
            && folded_line.contains(':')
            && !is_transfer_fee_line(folded_line))
}

fn find_transfer_fee(text: &str) -> Option<i64> {
    for line in text.lines() {
        let folded_line = folded(line);
        if !is_transfer_fee_line(&folded_line) {
            continue;
        }
        if let Some(amount) = money_amounts_on_line(line)
            .into_iter()
            .filter(|value| is_plausible_money(*value) && *value > 0)
            .min()
        {
            return Some(amount);
        }
    }
    None
}

fn find_transfer_payee(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
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
        if let Some(next) = lines.get(i + 1)
            && is_plausible_payee(next)
        {
            return Some(next.trim().to_owned());
        }
    }
    None
}

fn is_plausible_payee(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.chars().count() < 3 || !trimmed.chars().any(char::is_alphabetic) {
        return false;
    }
    let folded_name = folded(trimmed);
    if is_bank_counterparty(&folded_name) || folded_name.contains("iban") {
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

fn is_bank_counterparty(folded_name: &str) -> bool {
    contains_any(folded_name, BANK_WORDS)
}

fn find_transfer_reference(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if !folded_line.contains(TRANSFER_REFERENCE_LABEL) {
            continue;
        }
        for candidate in [*line, lines.get(i + 1).copied().unwrap_or("")] {
            if let Some(code) = transfer_code_token(candidate) {
                return Some(code);
            }
        }
    }
    None
}

fn transfer_code_token(line: &str) -> Option<String> {
    for tok in line.split_whitespace() {
        let token = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if is_transfer_code(token) {
            return Some(token.to_ascii_uppercase());
        }
    }
    None
}

fn is_transfer_code(token: &str) -> bool {
    if token.len() < 10 || token.len() > 24 {
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
    if upper.starts_with("RF") && upper.chars().skip(2).all(|c| c.is_ascii_digit()) {
        return false;
    }
    // IBAN-shaped: two letters then only digits.
    if upper.len() >= 15
        && upper.chars().take(2).all(|c| c.is_ascii_alphabetic())
        && upper.chars().skip(2).all(|c| c.is_ascii_digit())
    {
        return false;
    }
    true
}

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

fn first_date_on_line(line: &str) -> Option<String> {
    for token in line.split_whitespace() {
        let trimmed =
            token.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '.' && c != '-');
        if let Some(iso) = parse_eu_date(trimmed).or_else(|| parse_iso_date(trimmed)) {
            return Some(iso);
        }
    }
    None
}

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

/// Lines that hold identifiers, never an amount.
const IDENTIFIER_LINE_MARKERS: &[&str] = &["iban", "α.φ.μ", "αφμ", "mark"];

fn find_total_amount(text: &str, folded_text: &str) -> Option<i64> {
    // 1) Strong labeled totals always win — even when PDF extract is jumbled.
    //    Frequency scoring before them could pick the day of a date as euros.
    if let Some(v) = find_labeled_total(text) {
        return Some(v);
    }

    // 2) Utility bills: vote among € amounts (with date tokens masked).
    if is_utility_bill(folded_text)
        && let Some(v) = find_utility_payment_total(text)
    {
        return Some(v);
    }

    // 3) Line containing "σύνολα" / "totals" — take the largest plausible amount
    for line in text.lines() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }
        if contains_any(&folded_line, TOTALS_ROW_LABELS)
            && let Some(v) = money_amounts_on_line(line)
                .into_iter()
                .filter(|a| is_plausible_money(*a) && *a > 0)
                .max()
        {
            return Some(v);
        }
    }

    // 4) Weighted fallback (skip rate / mix / area lines)
    let mut weighted: Vec<(i64, i32)> = Vec::new();
    for line in text.lines() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }
        let mut w = 1;
        // `αξία` on invoices means line-value; `Ημερομηνία Αξίας` is a value
        // date and must not boost a clock (`7:00` → 700 minor).
        if !is_value_date_line(&folded_line) && contains_any(&folded_line, VALUE_WORDS) {
            w += 3;
        }
        if folded_line.contains('€') {
            w += 4;
        }
        if contains_any(&folded_line, IDENTIFIER_LINE_MARKERS) {
            w = 0;
        }
        if w == 0 {
            continue;
        }
        for a in money_amounts_on_line(line) {
            if is_plausible_money(a) {
                // Prefer amounts with cents over bare whole euros (less ID/date-like).
                let mut score = w;
                if a % 100 != 0 {
                    score += 2;
                }
                weighted.push((a, score));
            }
        }
    }
    weighted.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    weighted.first().map(|(v, _)| *v)
}

/// Labels that name the amount to pay. A line with one decides the total.
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

const VALUE_DATE_LABELS: &[&str] = &["ημερομηνια αξιας", "value date"];

fn find_labeled_total(text: &str) -> Option<i64> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }
        if !contains_any(&folded_line, TOTAL_LABELS) {
            continue;
        }
        // Same line first, then next line (labels and values often split in PDF extract).
        for candidate in [*line, lines.get(i + 1).copied().unwrap_or("")] {
            if candidate.is_empty() {
                continue;
            }
            let folded_candidate = folded(candidate);
            if is_noise_amount_line(&folded_candidate) && candidate != *line {
                continue;
            }
            if let Some(v) = money_amounts_on_line(candidate)
                .into_iter()
                .filter(|a| is_plausible_money(*a) && *a > 0)
                .max()
            {
                return Some(v);
            }
        }
    }
    None
}

/// Electricity/gas/water bills: payment total is usually a short line with `€`,
/// often repeated, and not on kWh/GWh/rate tables.
fn find_utility_payment_total(text: &str) -> Option<i64> {
    use std::collections::HashMap;

    let mut score: HashMap<i64, i32> = HashMap::new();
    let mut freq: HashMap<i64, i32> = HashMap::new();

    for line in text.lines() {
        let folded_line = folded(line);
        if is_noise_amount_line(&folded_line) {
            continue;
        }

        let amounts = money_amounts_on_line(line);
        if amounts.is_empty() {
            continue;
        }

        let short = line.chars().count() <= 40;
        let has_euro = line.contains('€');
        let has_date = line_has_date(line);
        let mostly_amount = is_amount_only_line(line);
        let pay_label = contains_any(&folded_line, PAYMENT_LABELS);

        for a in amounts {
            if !is_plausible_money(a) {
                continue;
            }
            // Bare whole-euro integers (e.g. day-of-month 26 → €26) are almost never
            // the printed payment total on Greek utilities — those show cents.
            let has_cents = a % 100 != 0;
            if !has_cents && !mostly_amount && !pay_label {
                continue;
            }

            let mut s = 1;
            if has_euro {
                s += 12;
            }
            if has_cents {
                s += 16;
            } else {
                s -= 8;
            }
            if mostly_amount {
                s += 18;
            }
            if has_date && has_euro {
                s += 14;
            }
            if short && has_euro {
                s += 8;
            }
            if pay_label {
                s += 25;
            }
            // Typical monthly utility total band
            if (1_000..=50_000).contains(&a) {
                s += 6;
            }
            // Zone prices / large one-offs are often noise
            if a >= 100_000 {
                s -= 10;
            }

            *score.entry(a).or_insert(0) += s;
            *freq.entry(a).or_insert(0) += 1;
        }
    }

    // Frequency bonus (payment total is printed several times on Greek power bills)
    for (a, f) in &freq {
        if *f >= 2 {
            *score.entry(*a).or_insert(0) += f * 10;
        }
    }

    score
        .into_iter()
        .filter(|(_, s)| *s > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(a, _)| a)
}

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
fn names_a_total(folded_line: &str) -> bool {
    folded_line.contains(TOTAL_WORD) || contains_any(folded_line, TOTAL_LABELS)
}

fn is_value_date_line(folded_line: &str) -> bool {
    contains_any(folded_line, VALUE_DATE_LABELS)
}

fn is_amount_only_line(line: &str) -> bool {
    // Ignore date tokens when deciding if the line is "just an amount".
    let stripped = mask_date_tokens(line);
    let cleaned: String = stripped
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '€' && *c != '.')
        .collect();
    // e.g. "76,65" or "76,65€"
    let digits_and_comma = !cleaned.is_empty()
        && cleaned
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.');
    digits_and_comma && money_amounts_on_line(line).len() == 1
}

fn line_has_date(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let t =
            token.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '.' && c != '-');
        parse_eu_date(t).or_else(|| parse_iso_date(t)).is_some()
    })
}

/// Blank out EU/ISO date tokens so day/month numbers are not parsed as euros.
///
/// Real PDF extracts often put `13/08/2026 72,53 €` on one line — without this,
/// `13`, `08`, and `2026` become €13 / €8 / €2026 candidates. Bank receipts
/// also print unpadded `27/8/2026`, which must not become €27 / €8.
///
/// A token only has to be written like a date ([`DateShape`]): `31/02/2026`
/// is not a date, and its digits are still not money.
fn mask_date_tokens(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if is_iso_date_at(&chars, i) {
            out.push_str("          ");
            i += 10;
            continue;
        }
        if let Some(len) = eu_date_len_at(&chars, i) {
            for _ in 0..len {
                out.push(' ');
            }
            i += len;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn is_iso_date_at(chars: &[char], i: usize) -> bool {
    i + 9 < chars.len()
        && chars[i].is_ascii_digit()
        && chars[i + 1].is_ascii_digit()
        && chars[i + 2].is_ascii_digit()
        && chars[i + 3].is_ascii_digit()
        && chars[i + 4] == '-'
        && chars[i + 5].is_ascii_digit()
        && chars[i + 6].is_ascii_digit()
        && chars[i + 7] == '-'
        && chars[i + 8].is_ascii_digit()
        && chars[i + 9].is_ascii_digit()
}

/// Length of a `d/m/yyyy` (or `dd.mm.yy`, …) token starting at `i`.
fn eu_date_len_at(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_ascii_digit() {
        return None;
    }
    if i >= chars.len() || !chars[i].is_ascii_digit() {
        return None;
    }

    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() && j - i < 2 {
        j += 1;
    }
    if j == i || j >= chars.len() {
        return None;
    }
    let sep = chars[j];
    if sep != '/' && sep != '.' && sep != '-' {
        return None;
    }
    j += 1;

    let month_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() && j - month_start < 2 {
        j += 1;
    }
    if j == month_start || j >= chars.len() || chars[j] != sep {
        return None;
    }
    j += 1;

    let year_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() && j - year_start < 4 {
        j += 1;
    }
    let year_len = j - year_start;
    if year_len != 2 && year_len != 4 {
        return None;
    }
    if j < chars.len() && chars[j].is_ascii_digit() {
        return None;
    }

    let token: String = chars[i..j].iter().collect();
    eu_date_shape(&token).map(|_| j - i)
}

/// Blank out `h:mm` / `hh:mm` clocks so `7:00` is not parsed as €7.00.
fn mask_time_tokens(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if let Some(len) = time_len_at(&chars, i) {
            for _ in 0..len {
                out.push(' ');
            }
            i += len;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Length of the `H:MM`, `HH:MM` or `HH:MM:SS` time starting at `i`, if one
/// starts there and is not part of a longer run of digits.
fn time_len_at(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && digit_at(chars, i - 1).is_some() {
        return None;
    }

    let colon = hour_colon_at(chars, i)?;
    let minutes = two_digits_at(chars, colon + 1)?;
    if minutes > 59 {
        return None;
    }

    let mut end = colon + 3;
    let has_seconds =
        chars.get(end) == Some(&':') && two_digits_at(chars, end + 1).is_some_and(|s| s <= 59);
    if has_seconds {
        end += 3;
    }

    if digit_at(chars, end).is_some() {
        return None;
    }
    Some(end - i)
}

/// Index of the colon that ends the hour starting at `i`: a two-digit hour
/// up to 23, or else a single digit.
fn hour_colon_at(chars: &[char], i: usize) -> Option<usize> {
    let first = digit_at(chars, i)?;

    let two_digit_hour = digit_at(chars, i + 1).is_some_and(|second| first * 10 + second <= 23);
    if two_digit_hour && chars.get(i + 2) == Some(&':') {
        return Some(i + 2);
    }

    (chars.get(i + 1) == Some(&':')).then_some(i + 1)
}

fn digit_at(chars: &[char], i: usize) -> Option<u32> {
    chars.get(i)?.to_digit(10)
}

fn two_digits_at(chars: &[char], i: usize) -> Option<u32> {
    Some(digit_at(chars, i)? * 10 + digit_at(chars, i + 1)?)
}

/// Accept amounts that look like currency, not AFM / invoice IDs / ZIPs.
fn is_plausible_money(minor: i64) -> bool {
    // 0.50 EUR .. 10_000_000.00 EUR
    (50..=1_000_000_000).contains(&minor)
}

/// Blanks out percentages (`24%`, `13,5 %`) so a rate is never an amount.
fn mask_percent_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;

    while let Some(&current) = chars.get(index) {
        if let Some(len) = percent_len_at(&chars, index) {
            out.push_str(&" ".repeat(len));
            index += len;
        } else {
            out.push(current);
            index += 1;
        }
    }
    out
}

/// Length of the percentage that starts at `index`: a number, any spaces
/// after it, and the `%` sign.
fn percent_len_at(chars: &[char], index: usize) -> Option<usize> {
    let is_number_char = |c: &char| c.is_ascii_digit() || matches!(c, ',' | '.');

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

fn money_amounts_on_line(line: &str) -> Vec<i64> {
    let line = mask_percent_tokens(&mask_time_tokens(&mask_date_tokens(line)));
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in line.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == ',' {
            buf.push(ch);
        } else {
            if let Some(v) = parse_money_token(&buf) {
                out.push(v);
            }
            buf.clear();
        }
    }
    if let Some(v) = parse_money_token(&buf) {
        out.push(v);
    }
    out
}

fn parse_money_token(s: &str) -> Option<i64> {
    // The tokenizer keeps `.` and `,`, so an amount that ends a sentence or a
    // list item arrives with that punctuation attached.
    let s = s.trim().trim_end_matches(['.', ',']);
    if s.is_empty() || s.len() > 14 {
        return None;
    }

    if s.contains([',', '.']) {
        separated_to_minor(s)
    } else {
        whole_euros_to_minor(s)
    }
}

/// A token of digits only, as whole euros. Long digit strings are IDs (AFM,
/// MARK, IBAN fragments), not money.
fn whole_euros_to_minor(digits: &str) -> Option<i64> {
    // Cap at 5 digits so long IDs never become money.
    if digits.len() > 5 {
        return None;
    }
    // Leading-zero numbers (08, 06) are date fragments, not euro amounts.
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }

    let whole: i64 = digits.parse().ok()?;
    // 4-digit years leak from unmasked date fragments — not money.
    if (1_900..=2_100).contains(&whole) {
        return None;
    }
    whole.checked_mul(100)
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
/// Returns `None` for one separator followed by four or more digits, which
/// is neither a fraction of a cent nor a thousands group.
fn separator_roles(token: &str) -> Option<SeparatorRoles> {
    let lone = |mark: char, other: char| {
        let (_, tail) = token.rsplit_once(mark)?;
        let only_one = token.matches(mark).count() == 1;

        match tail.len() {
            1 | 2 if only_one => Some(SeparatorRoles {
                decimal: Some(mark),
                grouping: other,
            }),
            3 => Some(SeparatorRoles {
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
    if !(1..=3).contains(&first.len()) || first.starts_with('0') {
        return None;
    }

    let mut joined = first.to_owned();
    for group in groups {
        if group.len() != 3 {
            return None;
        }
        joined.push_str(group);
    }
    Some(joined)
}

/// `whole` euros and an optional one or two digits of cents, as minor units.
fn decimal_to_minor(whole: &str, cents: Option<&str>) -> Option<i64> {
    let all_digits = |digits: &str| digits.bytes().all(|byte| byte.is_ascii_digit());

    // Whole part too long → ID
    if whole.is_empty() || whole.len() > 8 || !all_digits(whole) {
        return None;
    }

    let cents = match cents {
        Some(cents) if !(1..=2).contains(&cents.len()) || !all_digits(cents) => return None,
        Some(cents) => format!("{cents:0<2}").parse::<i64>().ok()?,
        None => 0,
    };
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(cents)
}

fn find_best_date(text: &str) -> Option<String> {
    // Prefer due/payment date on the same line as a € amount (utility payment slips).
    for line in text.lines() {
        if line.contains('€')
            && line_has_date(line)
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

/// Labels of an issue, due or generic date line.
const DATE_LABELS: &[&str] = &["ημερομην", "date", "εκδοσ", "ληξ", "due"];

/// The numbers of a token written like a date: a year in 1990..=2100, a
/// month in 1..=12 and a day in 1..=31.
///
/// The day need not exist in that month. The shape alone decides that the
/// digits are not money; only [`DateShape::to_iso`] decides that they are a
/// date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DateShape {
    year: i32,
    month: u8,
    day: u8,
}

impl DateShape {
    fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let in_range =
            (1990..=2100).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day);

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

fn iso_date_shape(s: &str) -> Option<DateShape> {
    let mut parts = s.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    DateShape::new(year, month, day)
}

fn eu_date_shape(s: &str) -> Option<DateShape> {
    let sep = ['/', '.', '-'].into_iter().find(|sep| s.contains(*sep))?;

    let mut parts = s.split(sep);
    let day = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    let year = if year < 100 { year + 2000 } else { year };
    DateShape::new(year, month, day)
}

fn parse_iso_date(s: &str) -> Option<String> {
    iso_date_shape(s)?.to_iso()
}

fn parse_eu_date(s: &str) -> Option<String> {
    eu_date_shape(s)?.to_iso()
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

const SUPPLY_CODE_LABELS: &[&str] = &["κωδικος παροχης", "supply", "ηκασπ"];

/// Supply / meter code next to its label (e.g. NGS000000001). PDF extraction
/// often puts the value on the line after the label.
fn labelled_supply_code(lines: &[&str]) -> Option<String> {
    for (i, line) in lines.iter().enumerate() {
        if !contains_any(&folded(line), SUPPLY_CODE_LABELS) {
            continue;
        }

        let next = lines.get(i + 1).copied().unwrap_or("");
        if let Some(code) = alnum_supply_code(line).or_else(|| alnum_supply_code(next)) {
            return Some(code);
        }
    }
    None
}

/// An unlabelled NGS supply code anywhere in the body.
fn bare_ngs_supply_code(lines: &[&str]) -> Option<String> {
    lines
        .iter()
        .filter_map(|line| alnum_supply_code(line))
        .find(|code| code.to_ascii_uppercase().starts_with("NGS"))
}

/// RF payment code on Greek utility bills: a token of its own, or glued to
/// neighbouring text by PDF extraction.
fn rf_payment_code(line: &str) -> Option<String> {
    let token = line
        .split_whitespace()
        .map(str::to_ascii_uppercase)
        .find(|token| is_rf_payment_code(token));
    if token.is_some() {
        return token;
    }

    // ASCII uppercasing keeps byte offsets, so the index is valid in `upper`.
    let upper = line.to_ascii_uppercase();
    let start = upper.find("RF")?;
    let glued: String = upper[start..]
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    is_rf_payment_code(&glued).then_some(glued)
}

fn is_rf_payment_code(upper: &str) -> bool {
    upper.len() >= 10
        && upper.starts_with("RF")
        && upper.chars().skip(2).all(|c| c.is_ascii_digit())
}

const MARK_LABELS: &[&str] = &["μαρκ", "mark", "α.α", "αα "];

/// MARK number on Greek invoices: a long digit string on the MARK / Α.Α.
/// line or the one after it.
fn mark_number(lines: &[&str]) -> Option<String> {
    for (i, line) in lines.iter().enumerate() {
        if !contains_any(&folded(line), MARK_LABELS) {
            continue;
        }

        let next = lines.get(i + 1).copied().unwrap_or("");
        if let Some(number) = long_digit_token(line).or_else(|| long_digit_token(next)) {
            return Some(number);
        }
    }
    None
}

const REFERENCE_LABELS: &[&str] = &["invoice", "αρ. παραστατ", "αριθμος", "number", "ref"];

/// A long number on a line that names it as an invoice or reference number.
fn labelled_reference_number(lines: &[&str]) -> Option<String> {
    lines.iter().find_map(|line| {
        if contains_any(&folded(line), REFERENCE_LABELS) {
            long_digit_token(line)
        } else {
            None
        }
    })
}

/// Fallback: the longest digit run that looks like an invoice id (10–20
/// digits); the first one wins a tie.
fn longest_reference_number(lines: &[&str]) -> Option<String> {
    let mut best: Option<String> = None;
    for number in lines.iter().filter_map(|line| long_digit_token(line)) {
        if number.len() >= 10 && best.as_ref().is_none_or(|b| number.len() > b.len()) {
            best = Some(number);
        }
    }
    best
}

fn long_digit_token(line: &str) -> Option<String> {
    let mut best: Option<String> = None;
    let mut buf = String::new();

    let take = |buf: &mut String, best: &mut Option<String>| {
        if buf.len() >= 6 && buf.len() <= 20 && best.as_ref().is_none_or(|b| buf.len() >= b.len()) {
            *best = Some(buf.clone());
        }
        buf.clear();
    };

    for ch in line.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else {
            take(&mut buf, &mut best);
        }
    }
    take(&mut buf, &mut best);
    best
}

/// Alphanumeric supply / point-of-delivery codes (e.g. `NGS000000001`).
fn alnum_supply_code(line: &str) -> Option<String> {
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if t.len() >= 8
            && t.len() <= 24
            && t.chars().any(|c| c.is_ascii_alphabetic())
            && t.chars().any(|c| c.is_ascii_digit())
            && t.chars().all(|c| c.is_ascii_alphanumeric())
        {
            // Skip pure RF payment refs (handled separately) and obvious words.
            let up = t.to_ascii_uppercase();
            if up.starts_with("RF") && up.chars().skip(2).all(|c| c.is_ascii_digit()) {
                continue;
            }
            return Some(t.to_owned());
        }
    }
    // Also scan without whitespace (jumbled extract: "NGS000000001" alone is fine).
    let mut buf = String::new();
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch);
        } else {
            if let Some(c) = take_supply_buf(&buf) {
                return Some(c);
            }
            buf.clear();
        }
    }
    take_supply_buf(&buf)
}

fn take_supply_buf(buf: &str) -> Option<String> {
    if buf.len() >= 8
        && buf.len() <= 24
        && buf.chars().any(|c| c.is_ascii_alphabetic())
        && buf.chars().any(|c| c.is_ascii_digit())
    {
        let up = buf.to_ascii_uppercase();
        if up.starts_with("RF") && up.chars().skip(2).all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(buf.to_owned())
    } else {
        None
    }
}

fn find_merchant(text: &str, folded_text: &str, locale: Locale) -> Option<String> {
    // Outgoing sales invoice: the counterparty is the customer. This runs
    // before brand recognition because the issuer's payment footer often
    // names a bank ("PIRAEUS BANK, IBAN …") that must not win.
    if is_sales_invoice(folded_text)
        && let Some(customer) = sales_invoice_customer(text)
    {
        return Some(customer);
    }

    // Known billers: brand tokens that survive text extraction.
    if let Some((brand, _)) = super::brands::known_brand(folded_text) {
        return Some(brand.to_owned());
    }

    // Unrecognized utility supplier: generic but honest labels.
    if is_utility_bill(folded_text) {
        if contains_any(folded_text, GAS_SUPPLY_MARKERS) {
            return Some(natural_gas_merchant(locale).into());
        }
        if folded_text.contains("power business") {
            return Some(electricity_supplier_merchant(locale).into());
        }
    }

    // Issuer: first Επωνυμία value
    for line in text.lines() {
        if folded(line).contains(ISSUER_NAME_LABEL) {
            if let Some(name) = value_after_colon(line)
                && name.chars().count() >= 3
            {
                return Some(name);
            }
            // same line after spaces
            let cleaned = line
                .split_whitespace()
                .skip_while(|word| *word == ":" || folded(word).contains(ISSUER_NAME_STEM))
                .collect::<Vec<_>>()
                .join(" ");
            if cleaned.chars().count() >= 3 {
                return Some(cleaned);
            }
        }
    }

    text.lines()
        .map(str::trim)
        .find(|line| {
            line.chars().count() >= 5
                && line.chars().count() <= 80
                && line.chars().any(char::is_alphabetic)
                && !folded(line).contains(INVOICE_WORD_GREEK)
        })
        .map(ToOwned::to_owned)
}

/// Markers of a natural gas bill whose supplier is not a known brand.
const GAS_SUPPLY_MARKERS: &[&str] = &[
    "φυσικου αεριου",
    "φυσικο αεριο",
    "gas simple",
    "προμηθεια φ.α",
];

/// The label of a legal name, on an issuer or a customer.
const ISSUER_NAME_LABEL: &str = "επωνυμια";

/// What every form of that label starts with, to skip the label word itself.
const ISSUER_NAME_STEM: &str = "επων";

const INVOICE_WORD_GREEK: &str = "τιμολογιο";

/// Headings that open the customer block of a sales invoice.
const CUSTOMER_BLOCK_STARTS: &[&str] = &[CUSTOMER_BLOCK_LABEL, "customer"];

/// Labels of the lines in a customer block that are not the customer's name.
const TAX_ID_OR_ADDRESS_LABELS: &[&str] = &["α.φ.μ", "αφμ", "διευθυν"];

const DESCRIPTION_HEADERS: &[&str] = &["περιγραφη", "description"];

/// Column headings that follow the description heading in a table header.
const QUANTITY_HEADERS: &[&str] = &["ποσοτητα", "quantity"];

/// Markers of a document that states it carries no VAT.
const VAT_EXEMPT_MARKERS: &[&str] = &["χωρις φπα", "0%"];

/// Customer name from the "Στοιχεία Πελάτη" block of a sales invoice.
fn sales_invoice_customer(text: &str) -> Option<String> {
    let mut after_client = false;

    for line in text.lines() {
        let folded_line = folded(line);

        if contains_any(&folded_line, CUSTOMER_BLOCK_STARTS) {
            after_client = true;
            continue;
        }
        if !after_client {
            continue;
        }

        if (folded_line.contains(ISSUER_NAME_LABEL) || folded_line.starts_with("name"))
            && let Some(name) = value_after_colon(line)
            && name.chars().count() >= 3
        {
            return Some(name);
        }

        // Next substantial non-label line
        if !contains_any(&folded_line, TAX_ID_OR_ADDRESS_LABELS)
            && line.chars().count() >= 5
            && line.chars().any(char::is_alphabetic)
            && !folded_line.ends_with(':')
        {
            return Some(line.trim().to_owned());
        }
    }

    None
}

fn value_after_colon(line: &str) -> Option<String> {
    let (idx, ch) = line.char_indices().find(|(_, c)| *c == ':' || *c == '：')?;
    let v = line[idx + ch.len_utf8()..].trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_owned())
    }
}

fn find_description(
    text: &str,
    folded_text: &str,
    merchant: Option<&str>,
    reference: Option<&str>,
    locale: Locale,
) -> Option<String> {
    // Outgoing sales invoice: customer-first title.
    if is_sales_invoice(folded_text)
        && let Some(m) = merchant
    {
        return Some(customer_invoice_description(locale, m, reference));
    }

    // Recognized biller or utility bill: company-first title with the
    // service decided by weighted keyword scoring.
    let brand_service = super::brands::known_brand(folded_text).and_then(|(_, service)| service);

    if is_utility_bill(folded_text) || brand_service.is_some() {
        let service = brand_service.or_else(|| super::brands::classify_service(folded_text));
        let kind = service.map_or(BillKind::Utility, super::brands::Service::bill_kind);

        return Some(bill_description(locale, kind, merchant));
    }

    // Line-item description under Περιγραφή
    let mut after_header = false;
    for line in text.lines() {
        if contains_any(&folded(line), DESCRIPTION_HEADERS) {
            after_header = true;
            continue;
        }
        if after_header {
            // skip table noise / numbers-only
            let alpha: String = line
                .chars()
                .filter(|c| c.is_alphabetic() || c.is_whitespace())
                .collect();
            let alpha = alpha.trim();
            if alpha.chars().count() >= 4 && !contains_any(&folded(alpha), QUANTITY_HEADERS) {
                return Some(alpha.to_owned());
            }
        }
    }

    if let Some(r) = reference {
        return Some(invoice_reference_description(locale, r, merchant));
    }
    if folded_text.contains(INVOICE_WORD_GREEK) {
        return Some(invoice_word(locale).into());
    }
    merchant.map(ToOwned::to_owned)
}

fn score_confidence(
    amount: Option<i64>,
    date: Option<&String>,
    reference: Option<&String>,
    kind: EntryKindSuggestion,
) -> f32 {
    let mut c = 0.2_f32;
    if amount.is_some() {
        c += 0.4;
    }
    if date.is_some() {
        c += 0.15;
    }
    if reference.is_some() {
        c += 0.1;
    }
    if matches!(
        kind,
        EntryKindSuggestion::Income | EntryKindSuggestion::Bill
    ) {
        c += 0.05;
    }
    c.min(0.95)
}

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
    if contains_any(folded_text, VAT_EXEMPT_MARKERS) {
        notes.push(UiText::new(UiTextCode::InvoiceVatExempt));
    }

    notes
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
        ("CUSTOMER_BLOCK_LABEL", &[CUSTOMER_BLOCK_LABEL]),
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
    fn time_token_lengths() {
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
    fn money_tokens_in_both_decimal_conventions() {
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

    /// Load a week-1 corpus fixture so unit tests share the public golden tree.
    fn corpus_text(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/documents")
            .join(relative);
        #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
        {
            std::fs::read_to_string(&path).expect("corpus fixture")
        }
    }

    #[test]
    fn greek_service_invoice_total_and_kind() {
        let s = parse_invoice_text(
            &corpus_text("synthetic/text/greek_sales_invoice.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(s.amount_minor, Some(186_000), "expected €1860.00");
        assert_eq!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.entry_date.as_deref(), Some("2026-06-25"));
        assert_eq!(s.reference.as_deref(), Some("900000000000001"));
        assert!(
            s.merchant
                .as_deref()
                .is_some_and(|m| m.contains("ACME CONSULTING")),
            "merchant={:?}",
            s.merchant
        );
        assert!(s.bill_unpaid, "Επί πιστώσει should mark unpaid/credit");
    }

    #[test]
    fn received_service_invoice_is_not_income() {
        let text = "\
Τιμολόγιο Παροχής Υπηρεσιών
Επωνυμία: ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ
Α.Φ.Μ.: 000000000
Πληρωτέο (€): 200,00
";
        let s = parse_invoice_text(text, crate::prefs::Locale::En);
        assert_ne!(s.kind, EntryKindSuggestion::Income, "kind={:?}", s.kind);
    }

    #[test]
    fn value_after_fullwidth_colon_does_not_panic() {
        assert_eq!(value_after_colon("Name：ACME LTD"), Some("ACME LTD".into()));
        assert_eq!(value_after_colon("Name: ACME LTD"), Some("ACME LTD".into()));
    }

    #[test]
    fn settlement_bill_is_not_automatically_unpaid() {
        let s = parse_invoice_text(
            &corpus_text("synthetic/text/dei_settlement.txt"),
            crate::prefs::Locale::En,
        );
        assert!(
            !s.bill_unpaid,
            "εμπρόθεσμο/εκκαθαριστικό/εξόφληση μέσω must not force unpaid"
        );
    }

    #[test]
    fn cosmote_pay_via_is_not_unpaid() {
        let s = parse_invoice_text(
            &corpus_text("synthetic/text/cosmote_pay_via.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(s.kind, EntryKindSuggestion::Bill);
        assert!(
            !s.bill_unpaid,
            "known-brand εξόφληση μέσω must not force unpaid: {s:?}"
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
    fn rejects_afm_as_money() {
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
        let s = parse_invoice_text(
            &corpus_text("synthetic/text/zenith_supplier_vs_grid.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(s.merchant.as_deref(), Some("ZeniΘ"));
        assert_eq!(s.description.as_deref(), Some("ZeniΘ — Electricity bill"));
    }

    #[test]
    fn sales_invoice_titles_carry_the_customer() {
        let s = parse_invoice_text(
            "Επωνυμία ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ\n\
             Τιμολόγιο Παροχής / Ενδοκοινοτική Παροχή Υπηρεσιών\n\
             900000000000001 Επί πιστώσειB 51 25/06/2026\n\
             Στοιχεία Πελάτη\nΑ.Φ.Μ.: 000000000\nΕπωνυμία: ACME CONSULTING LTD\n\
             Πληρωτέο (€): 1860,00",
            crate::prefs::Locale::En,
        );
        assert_eq!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.merchant.as_deref(), Some("ACME CONSULTING LTD"));
        assert!(
            s.description
                .as_deref()
                .is_some_and(|d| d.starts_with("ACME CONSULTING LTD — Invoice")),
            "description={:?}",
            s.description
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
    fn zenith_electricity_bill_total() {
        let s = parse_invoice_text(
            &corpus_text("synthetic/text/zenith_electricity.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(
            s.amount_minor,
            Some(7_665),
            "expected €76.65, got {:?}",
            s.amount_minor
        );
        assert!(
            matches!(
                s.kind,
                EntryKindSuggestion::Bill | EntryKindSuggestion::Expense
            ),
            "kind={:?}",
            s.kind
        );
        assert_ne!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.entry_date.as_deref(), Some("2026-08-18"));
        assert!(
            s.merchant
                .as_deref()
                .is_some_and(|m| m.to_lowercase().contains("zeni")),
            "merchant={:?}",
            s.merchant
        );
        assert!(
            s.reference.as_deref().is_some_and(|r| r.starts_with("RF")),
            "reference={:?}",
            s.reference
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
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

    #[test]
    fn ngs_gas_bill_payment_total() {
        let s = parse_invoice_text(
            &corpus_text("synthetic/text/ngs_gas_bill.txt"),
            crate::prefs::Locale::En,
        );
        assert_eq!(
            s.amount_minor,
            Some(7_253),
            "expected €72.53 (not deposit 60 or subtotal 50.50), got {:?}",
            s.amount_minor
        );
        assert_ne!(s.kind, EntryKindSuggestion::Income);
        assert!(
            matches!(
                s.kind,
                EntryKindSuggestion::Bill | EntryKindSuggestion::Expense
            ),
            "kind={:?}",
            s.kind
        );
        assert_eq!(s.entry_date.as_deref(), Some("2026-08-13"));
        assert!(
            s.merchant.as_deref().is_some_and(
                |m| m.to_lowercase().contains("gas") || m.to_lowercase().contains("ngs")
            ),
            "merchant={:?}",
            s.merchant
        );
        assert!(
            s.reference
                .as_deref()
                .is_some_and(|r| { r.contains("NGS") || r.contains("SYN") || r.starts_with("RF") }),
            "reference={:?}",
            s.reference
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
mod jumbled_extract {
    use super::*;

    /// Synthetic jumbled layout (the shape `pdf_extract` produces on a
    /// text-layer utility PDF). Placeholders only — not a live dump.
    #[test]
    #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
    fn parse_jumbled_ngs_extract_fixture() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ngs_gas_jumbled_extract.txt"
        ))
        .expect("fixture extract");
        let s = parse_invoice_text(&text, crate::prefs::Locale::En);
        assert_eq!(
            s.amount_minor,
            Some(7_253),
            "expected €72.53 not date/deposit noise, got {:?}",
            s.amount_minor
        );
        assert_ne!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.entry_date.as_deref(), Some("2026-08-13"));
        assert!(
            s.reference
                .as_deref()
                .is_some_and(|r| { r.contains("NGS") || r.contains("SYN") || r.starts_with("RF") }),
            "reference={:?}",
            s.reference
        );
        assert_eq!(
            s.merchant.as_deref(),
            Some("Volton"),
            "MyON portal branding identifies the supplier"
        );
        assert_eq!(s.description.as_deref(), Some("Volton — Gas bill"));
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
    #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
    fn parse_local_gas_pdf_bytes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/local_gas_bill.pdf"
        );
        let bytes = std::fs::read(path).expect("the private fixture must be present");
        let text = pdf_extract::extract_text_from_mem(&bytes).expect("pdf text");
        let s = parse_invoice_text(&text, crate::prefs::Locale::En);
        assert_eq!(
            s.amount_minor,
            Some(7_253),
            "local pdf_extract amount={:?}\ntext excerpt:\n{}",
            s.amount_minor,
            text.chars().take(800).collect::<String>()
        );
    }
}
