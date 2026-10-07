//! Choosing the total of a document among the amounts written on it.
//!
//! [`find_total_amount`] runs the four stages described in the
//! [module above](crate::documents::invoice#choosing-the-total): a labelled
//! total, the vote among the amounts of a utility bill, a totals row, and a
//! weighted fallback over every remaining amount. The weights of the vote
//! and of the fallback are the constants of [`utility_vote`] and
//! [`fallback_weight`].
//!
//! Every stage skips noise lines ([`is_noise_amount_line`]) and reads
//! amounts through [`crate::documents::invoice::money`], so the token rules
//! and the plausibility band are the same in all four.

use crate::documents::invoice::dates::{is_value_date_line, line_has_date, mask_date_tokens};
use crate::documents::invoice::kind::is_utility_bill;
use crate::documents::invoice::money::{
    has_cents, is_plausible_money, largest_plausible_amount, money_amounts_on_line,
};
use crate::documents::invoice::normalization::{contains_any, folded};

/// The word that marks a line or a name as holding a bank account number.
pub(super) const IBAN_WORD: &str = "iban";

/// The label of a totals row: the largest amount on it is the total.
pub(super) const TOTALS_ROW_LABELS: &[&str] = &["συνολα", "totals"];

/// Words that mark a line as holding a value, for the weighted fallback.
pub(super) const VALUE_WORDS: &[&str] = &["αξια", "value", TOTAL_WORD];

/// The bare word the fallback favours and the percent rule exempts.
pub(super) const TOTAL_WORD: &str = "total";

/// Markers of lines that hold identifiers, never an amount: the fallback
/// skips a line with one.
///
/// They are matched as substrings, so `mark` also matches `supermarket`.
pub(super) const IDENTIFIER_LINE_MARKERS: &[&str] = &[IBAN_WORD, "α.φ.μ", "αφμ", "mark"];

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
pub(super) fn find_total_amount(text: &str, folded_text: &str) -> Option<i64> {
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
        if contains_any(&folded_line, IDENTIFIER_LINE_MARKERS) {
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
pub(super) const TOTAL_LABELS: &[&str] = &[
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
pub(super) const PAYMENT_LABELS: &[&str] = &["πληρωμ", "τρεχοντος", "payable", "amount due"];

/// Deposits and guarantees are not the bill total.
pub(super) const DEPOSIT_LABELS: &[&str] = &["εγγυηση", "deposit"];

/// Markers of rate, volume, area and energy-mix lines, which hold numbers
/// that are not the payment total.
pub(super) const RATE_LINE_MARKERS: &[&str] = &[
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
pub(super) const CONSUMPTION_LABEL: &str = "καταναλωση";

/// Stage 1: the amount beside the first label that has one.
///
/// Lines are taken top to bottom. A line counts when it carries one of
/// [`TOTAL_LABELS`] and is not a noise line. Its largest plausible amount is
/// the total; when it has none, the largest on the next line is, unless
/// that line is a noise line. Extraction often puts a label and its value on
/// separate lines. A label with no amount in either place is passed over.
pub(super) fn find_labeled_total(text: &str) -> Option<i64> {
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
pub(super) fn find_utility_payment_total(text: &str) -> Option<i64> {
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
pub(super) fn is_noise_amount_line(folded_line: &str) -> bool {
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
pub(super) fn names_a_total(folded_line: &str) -> bool {
    folded_line.contains(TOTAL_WORD) || contains_any(folded_line, TOTAL_LABELS)
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

#[cfg(test)]
mod tests {
    use crate::documents::analyze::EntryKindSuggestion;
    use crate::documents::analyze::parse_invoice_text;
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
    fn a_bare_total_word_is_read_by_the_weighted_fallback() {
        let text = "Invoice\nSubtotal 10,00\nTOTAL 45,90 EUR\nThank you";

        let suggestion = parse_invoice_text(text, crate::prefs::Locale::En);

        assert_eq!(suggestion.amount_minor, Some(4590));
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
        assert_eq!(suggestion.entry_date, Some(date!(2026 - 08 - 18)));
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
        assert_eq!(suggestion.entry_date, Some(date!(2026 - 08 - 13)));
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
}

#[cfg(test)]
mod amounts_and_dates {
    use crate::documents::analyze::DocumentSuggestion;
    use crate::documents::analyze::parse_invoice_text;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
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
    use crate::documents::analyze::DocumentSuggestion;
    use crate::documents::analyze::parse_invoice_text;

    fn read(text: &str) -> DocumentSuggestion {
        parse_invoice_text(text, crate::prefs::Locale::En)
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
    fn an_identifier_marker_inside_a_word_hides_the_line_from_the_fallback() {
        assert_eq!(read("Supermarket 12,50 €").amount_minor, None);
        assert_eq!(read("Grocery 12,50 €").amount_minor, Some(1_250));
        // A labelled total is found before the fallback runs.
        assert_eq!(
            read("Supermarket\nAmount due 12,50 €").amount_minor,
            Some(1_250)
        );
    }
}
