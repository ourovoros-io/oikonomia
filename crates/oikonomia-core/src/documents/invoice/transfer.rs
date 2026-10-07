//! Greek bank transfer receipts (`έμβασμα`), read by rules of their own.
//!
//! A receipt lists the principal, a fee, an execution date and a value date
//! with a clock time, so the general rules would be free to pick the fee or
//! the time as the amount and the value date as the date.
//! [`parse_bank_transfer`] reads each field from its own label instead: the
//! principal from the capital debit line, the date from the execution line,
//! the payee from the beneficiary line and the reference from the
//! transaction code. The fee is returned as data beside the reading.
//!
//! [`is_bank_transfer_receipt`] decides, from the wording of the whole
//! text, whether a document is read this way.

use std::ops::RangeInclusive;

use time::Date;

use crate::documents::invoice::InvoiceReading;
use crate::documents::invoice::dates::{first_date_on_line, is_value_date_line};
use crate::documents::invoice::kind::DocumentClass;
use crate::documents::invoice::merchant::{
    Description, ISSUER_NAME_LABEL, MIN_NAME_CHARS, Merchant, value_after_colon,
};
use crate::documents::invoice::money::{
    is_plausible_money, largest_plausible_amount, money_amounts_on_line,
};
use crate::documents::invoice::reference::is_rf_then_digits;
use crate::documents::invoice::total::IBAN_WORD;
use crate::documents::keyword::Keyword::{Fragment, Prefix, Word};
use crate::documents::keyword::{Keyword, contains_any, folded};
use crate::ui_text::{UiText, UiTextCode};

/// Wording that makes a document a bank transfer receipt: "έμβασμα"
/// (remittance), "transfer to another bank", or a transaction-code label.
pub(super) const TRANSFER_MARKERS: &[Keyword] = &[
    Prefix("εμβασμα"),
    Word("μεταφορα σε αλλη τραπεζα"),
    TRANSFER_REFERENCE_LABEL,
];

/// The label of the transaction code on a transfer receipt.
pub(super) const TRANSFER_REFERENCE_LABEL: Keyword = Word("κωδικος συναλλαγης");

/// Whether folded text carries one of [`TRANSFER_MARKERS`].
pub(super) fn is_bank_transfer_receipt(folded_text: &str) -> bool {
    contains_any(folded_text, TRANSFER_MARKERS)
}

/// Reads a Greek bank transfer receipt (`έμβασμα`, or a transfer to another
/// bank) from normalized text.
///
/// The kind is always `Expense` and never unpaid. The description is the
/// generated transfer title, which names the payee. The fee is returned as
/// data, not in the notes.
pub(super) fn parse_bank_transfer(text: &str) -> InvoiceReading {
    let amount_minor = find_transfer_principal(text);

    InvoiceReading {
        amount_minor,
        entry_date: find_transfer_date(text),
        reference: find_transfer_reference(text),
        merchant: find_transfer_payee(text).map(Merchant::Named),
        description: Some(Description::BankTransfer),
        class: DocumentClass::Expense,
        transfer_fee_minor: find_transfer_fee(text),
        notes: build_transfer_notes(amount_minor),
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
        if TRANSFER_PRINCIPAL_LABEL.occurs_in(&folded_line)
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
pub(super) const TRANSFER_PRINCIPAL_LABEL: Keyword = Word("ποσο χρεωσης κεφαλαιου");

/// The plain "amount" label a receipt uses when it has no capital line.
pub(super) const TRANSFER_AMOUNT_LABEL: Keyword = Word("ποσο");

/// Labels of the fee and charges lines of a transfer receipt.
pub(super) const TRANSFER_FEE_LABELS: &[Keyword] =
    &[Prefix("προμηθεια"), Word("εξοδων"), Word("εξοδα")];

/// "Of the beneficiary", as in "name of the beneficiary".
pub(super) const BENEFICIARY_LABEL: Keyword = Word("δικαιουχου");

/// The two halves of the `Ονοματεπώνυμο / Επωνυμία` beneficiary label.
pub(super) const BENEFICIARY_NAME_LABELS: [Keyword; 2] = [Word("ονοματεπωνυμο"), ISSUER_NAME_LABEL];

/// Words that make a name a bank's, not the payee's.
///
/// `bank` is a fragment because banks write it into their names: Eurobank,
/// Optima bank, Piraeusbank.
pub(super) const BANK_WORDS: &[Keyword] = &[Prefix("τραπεζα"), Fragment("bank")];

/// Labels of the date a transfer was executed, which is the date to post.
pub(super) const EXECUTION_DATE_LABELS: &[Keyword] = &[Prefix("εκτελεσ"), Word("execution")];

/// Labels of a date line on a transfer receipt, used when no execution date
/// is labelled.
pub(super) const TRANSFER_DATE_LABELS: &[Keyword] =
    &[Prefix("ημερομην"), Word("date"), Prefix("συναλλαγ")];

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
/// alone on the line, or the word `ποσο` anywhere on a line that has a colon.
///
/// Fee lines also say `ποσο`; the caller has skipped them before it asks.
/// `Ποσοστό:` (percentage) is another word and does not count.
fn is_transfer_amount_label(folded_line: &str) -> bool {
    let alone = folded_line.trim() == TRANSFER_AMOUNT_LABEL.text();

    TRANSFER_AMOUNT_LABEL.occurs_in(folded_line) && (alone || folded_line.contains(':'))
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
        let beneficiary = BENEFICIARY_LABEL.occurs_in(&folded_line)
            || BENEFICIARY_NAME_LABELS
                .iter()
                .all(|label| label.occurs_in(&folded_line));
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
    if is_bank_counterparty(&folded_name) || IBAN_WORD.occurs_in(&folded_name) {
        return false;
    }

    // A line that only repeats the label is not a name.
    let mut stripped = folded_name.replace(['/', ':', '：'], " ");
    for label in BENEFICIARY_NAME_LABELS
        .into_iter()
        .chain([BENEFICIARY_LABEL])
    {
        stripped = stripped.replace(label.text(), " ");
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
        if !TRANSFER_REFERENCE_LABEL.occurs_in(&folded_line) {
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

/// The date of a transfer.
///
/// Value-date lines are skipped throughout: the value date can be a day or
/// more after the transfer. Among the other lines, in order of preference:
/// the first date on a line with an execution label, the first on a line
/// with any date label, the first anywhere.
fn find_transfer_date(text: &str) -> Option<Date> {
    let mut labeled: Option<Date> = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::analyze::{EntryKindSuggestion, parse_invoice_text};
    use crate::documents::invoice::read_invoice_text;
    use time::macros::date;

    /// Loads a corpus fixture, so the unit tests read the same documents as
    /// the golden test in `tests/document_corpus.rs`.
    fn corpus_text(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/documents")
            .join(relative);
        std::fs::read_to_string(&path).expect("corpus fixture")
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
        assert_eq!(suggestion.entry_date, Some(date!(2026 - 08 - 27)));
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
            read_invoice_text(&text).transfer_fee_minor,
            Some(140),
            "the fee is returned as integer minor units"
        );
        assert_ne!(suggestion.amount_minor, Some(140));
        assert_ne!(suggestion.amount_minor, Some(700));

        let analyzed = crate::documents::analyze_document_bytes(
            &crate::documents::NewDocument {
                filename: "greek_bank_embasma.txt",
                mime_type: "text/plain",
                data: text.as_bytes(),
            },
            &crate::documents::AnalyzeContext {
                template: crate::domain::ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::El,
            },
            None,
        );
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
        assert_eq!(suggestion.entry_date, Some(date!(2026 - 08 - 27)));
        assert_eq!(suggestion.merchant.as_deref(), Some("HELIOS TRADING IKE"));
        assert_eq!(suggestion.reference.as_deref(), Some("F000TO0000000001"));
        assert_eq!(suggestion.kind, EntryKindSuggestion::Expense);
    }

    #[test]
    fn a_percentage_label_is_not_the_amount_label() {
        let amount = |text: &str| parse_invoice_text(text, crate::prefs::Locale::En).amount_minor;

        // `Ποσοστό` (percentage) starts with `Ποσό` (amount) and is another word.
        assert_eq!(amount("Έμβασμα\nΠοσοστό: 310,00"), None);
        assert_eq!(
            amount("Έμβασμα\nΠοσοστό: 5,00\nΠοσό: 310,00\nΠρομήθεια: 1,40"),
            Some(31_000)
        );
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
}

#[cfg(test)]
mod documented_tradeoffs {
    use crate::documents::invoice::read_invoice_text;

    #[test]
    fn a_fee_under_the_plausibility_band_is_not_reported() {
        let receipt = "Έμβασμα\nΠοσό Χρέωσης Κεφαλαίου 310,00\nΠρομήθεια 0,40";

        let reading = read_invoice_text(receipt);

        assert_eq!(reading.amount_minor, Some(31_000));
        assert_eq!(reading.transfer_fee_minor, None);
    }
}
