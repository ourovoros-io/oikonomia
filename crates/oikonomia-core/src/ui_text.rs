//! Text for the user that Rust decides on and the UI words.
//!
//! Business rules decide which note applies and with what values. They never
//! write the sentence: they return a [`UiText`], a stable `snake_case` code plus
//! named string parameters, and the UI turns it into the user's language. A
//! parameter is a plain value (a currency code, an ISO date, an integer count
//! of minor units), never a formatted number or a piece of a sentence.
//!
//! This is the channel for text that is not a failure: a note on a document
//! suggestion, or the reason one row of a CSV import cannot be used. A failed
//! operation reports through [`crate::error`], which
//! uses the same idea of a code with parameters. Wording that is stored in
//! the book is the one case where core writes the words itself, in
//! [`crate::text`].
//!
//! The codes are shared with the web sources by hand, so the tests below
//! check [`UiTextCode::ALL`] against the enum and against the fixture the UI
//! reads its code list from.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Every coded text Rust sends: the notes the document analyzer attaches to a
/// suggestion, and the reasons a CSV import row cannot be used.
///
/// The `snake_case` spelling is the wire code. The web catalog maps each code
/// to copy in `web/src/lib/uiText.ts`, and a test pins both lists against
/// `web/src/lib/uiTextCodes.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiTextCode {
    /// No text could be read from the file.
    NoTextExtracted,
    /// Amount detection assumes 2-decimal currencies. Parameter: `currency`.
    AmountAssumesTwoDecimals,
    /// The entry takes the document's date. Parameter: `date` (ISO).
    DatedFromDocument,
    /// An unpaid bill has no payable account to book to.
    AddPayableAccount,
    /// The PDF is over the page or stream budget.
    PdfOverBudget,
    /// The text came from the document itself, on this device.
    ParsedFromDocumentText,
    /// The OCR model directory is not configured.
    OcrPathMissing,
    /// The bundled OCR models are not on disk.
    OcrModelsMissing,
    /// An image was read with the bundled OCR.
    OcrRead,
    /// OCR ran and found little text.
    OcrLittleText,
    /// OCR failed. The cause is logged, not sent.
    OcrFailed,
    /// An image embedded in a PDF was read with the bundled OCR.
    OcrPdfImage,
    /// The invoice reader parsed the text.
    InvoiceParsed,
    /// No total could be detected with confidence.
    InvoiceNoTotal,
    /// The document is a sales or service invoice.
    InvoiceIncome,
    /// The document is a utility bill.
    InvoiceUtility,
    /// The document is on credit terms or shows an amount due.
    InvoiceUnpaid,
    /// VAT appears zero or exempt.
    InvoiceVatExempt,
    /// The document is a bank transfer receipt.
    TransferDetected,
    /// The transferred amount could not be detected with confidence.
    TransferNoAmount,
    /// The receipt shows a transfer fee. Parameters: `fee_minor`, `currency`.
    TransferFee,
    /// The receipt shows a transfer fee that cannot be given as a figure,
    /// because the book's currency does not use two decimals.
    TransferFeeUnstated,
    /// A CSV row's date is not a date. Parameter: `value`, the cell as written.
    CsvInvalidDate,
    /// A CSV row's amount is not an amount. Parameter: `value`, the cell as written.
    CsvInvalidAmount,
    /// A CSV row's type cell is not a type the import knows. Parameter: `value`,
    /// the cell as written.
    CsvInvalidType,
    /// A CSV row's date cell is empty or only whitespace.
    CsvMissingDate,
    /// A CSV row's amount cell is empty or only whitespace.
    CsvMissingAmount,
    /// A CSV row's amount is zero.
    CsvZeroAmount,
    /// A CSV row's amount is too large to hold.
    CsvAmountOverflow,
    /// A CSV row could not be read at all. The cause is logged, not sent.
    CsvUnreadableRow,
}

impl UiTextCode {
    /// Every code, in declaration order: the notes of the document analyzer,
    /// then the reasons a CSV import row cannot be used.
    ///
    /// A test checks this list against `web/src/lib/uiTextCodes.json`.
    pub const ALL: &'static [Self] = &[
        Self::NoTextExtracted,
        Self::AmountAssumesTwoDecimals,
        Self::DatedFromDocument,
        Self::AddPayableAccount,
        Self::PdfOverBudget,
        Self::ParsedFromDocumentText,
        Self::OcrPathMissing,
        Self::OcrModelsMissing,
        Self::OcrRead,
        Self::OcrLittleText,
        Self::OcrFailed,
        Self::OcrPdfImage,
        Self::InvoiceParsed,
        Self::InvoiceNoTotal,
        Self::InvoiceIncome,
        Self::InvoiceUtility,
        Self::InvoiceUnpaid,
        Self::InvoiceVatExempt,
        Self::TransferDetected,
        Self::TransferNoAmount,
        Self::TransferFee,
        Self::TransferFeeUnstated,
        Self::CsvInvalidDate,
        Self::CsvInvalidAmount,
        Self::CsvInvalidType,
        Self::CsvMissingDate,
        Self::CsvMissingAmount,
        Self::CsvZeroAmount,
        Self::CsvAmountOverflow,
        Self::CsvUnreadableRow,
    ];
}

/// One piece of user-facing text as a code and its values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiText {
    /// Which text this is.
    pub code: UiTextCode,
    /// Named values the text fills in. Empty when the text has none.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

impl UiText {
    /// Returns the text `code` with no values.
    #[must_use]
    pub fn new(code: UiTextCode) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    /// Returns the text with one more named value, which replaces an earlier
    /// value of the same name.
    #[must_use]
    pub fn with_param(mut self, name: &str, value: impl Into<String>) -> Self {
        self.params.insert(name.to_owned(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::documents::AnalyzerHint;
    use crate::ledger::SyntheticLine;
    use oikonomia_test_support::listed_variants;

    /// Returns the wire spelling of a value that serializes as one string.
    fn wire_spelling<T: Serialize>(value: &T) -> String {
        match serde_json::to_value(value) {
            Ok(serde_json::Value::String(spelling)) => spelling,
            other => format!("not a string: {other:?}"),
        }
    }

    /// The shared fixture the web side reads. Rust and the UI map must agree.
    #[derive(Deserialize)]
    struct Fixture {
        /// The codes of [`UiTextCode`].
        notes: Vec<String>,
        /// The codes of [`AnalyzerHint`].
        hints: Vec<String>,
        /// The codes of [`SyntheticLine`].
        #[serde(rename = "syntheticLines")]
        synthetic_lines: Vec<String>,
    }

    /// Returns the fixture, read from the web sources at compile time.
    fn fixture() -> Fixture {
        serde_json::from_str(include_str!("../../../web/src/lib/uiTextCodes.json"))
            .expect("uiTextCodes.json parses")
    }

    /// Fails unless `fixture` has no duplicate and holds the same codes as
    /// `listed`; `label` names the list in the failure message.
    fn assert_same_set(label: &str, fixture: &[String], listed: &[String]) {
        let from_fixture: BTreeSet<&String> = fixture.iter().collect();
        let from_rust: BTreeSet<&String> = listed.iter().collect();

        assert_eq!(
            from_fixture.len(),
            fixture.len(),
            "duplicate {label} in uiTextCodes.json"
        );
        assert_eq!(
            from_fixture, from_rust,
            "uiTextCodes.json and Rust differ for {label}"
        );
    }

    #[test]
    fn the_shared_fixture_lists_exactly_the_codes_rust_can_emit() {
        let fixture = fixture();

        assert_same_set(
            "notes",
            &fixture.notes,
            &UiTextCode::ALL
                .iter()
                .map(wire_spelling)
                .collect::<Vec<_>>(),
        );
        assert_same_set(
            "hints",
            &fixture.hints,
            &AnalyzerHint::ALL
                .iter()
                .map(wire_spelling)
                .collect::<Vec<_>>(),
        );
        assert_same_set(
            "syntheticLines",
            &fixture.synthetic_lines,
            &SyntheticLine::ALL
                .iter()
                .map(wire_spelling)
                .collect::<Vec<_>>(),
        );
    }

    listed_variants! {
        units listed_codes for UiTextCode {
            UiTextCode::NoTextExtracted,
            UiTextCode::AmountAssumesTwoDecimals,
            UiTextCode::DatedFromDocument,
            UiTextCode::AddPayableAccount,
            UiTextCode::PdfOverBudget,
            UiTextCode::ParsedFromDocumentText,
            UiTextCode::OcrPathMissing,
            UiTextCode::OcrModelsMissing,
            UiTextCode::OcrRead,
            UiTextCode::OcrLittleText,
            UiTextCode::OcrFailed,
            UiTextCode::OcrPdfImage,
            UiTextCode::InvoiceParsed,
            UiTextCode::InvoiceNoTotal,
            UiTextCode::InvoiceIncome,
            UiTextCode::InvoiceUtility,
            UiTextCode::InvoiceUnpaid,
            UiTextCode::InvoiceVatExempt,
            UiTextCode::TransferDetected,
            UiTextCode::TransferNoAmount,
            UiTextCode::TransferFee,
            UiTextCode::TransferFeeUnstated,
            UiTextCode::CsvInvalidDate,
            UiTextCode::CsvInvalidAmount,
            UiTextCode::CsvInvalidType,
            UiTextCode::CsvMissingDate,
            UiTextCode::CsvMissingAmount,
            UiTextCode::CsvZeroAmount,
            UiTextCode::CsvAmountOverflow,
            UiTextCode::CsvUnreadableRow,
        }
    }

    /// Fails unless `UiTextCode::ALL` is exactly the set of variants in the
    /// `listed_codes` list above, each once. The compiler checks that list
    /// against the enum with an exhaustive `match`, so a variant added to the
    /// enum but left out of the list does not compile. It does not check the
    /// order of `ALL`, nor that the UI has copy for a code;
    /// `the_shared_fixture_lists_exactly_the_codes_rust_can_emit` does that.
    #[test]
    fn all_lists_exactly_the_variants_of_the_enum() {
        let listed = listed_codes::variants();

        assert_eq!(
            UiTextCode::ALL.len(),
            listed_codes::COUNT,
            "UiTextCode::ALL and the listed variants differ in number"
        );
        for variant in listed {
            assert!(
                UiTextCode::ALL.contains(&variant),
                "{variant:?} is missing from UiTextCode::ALL"
            );
        }
        listed_codes::assert_every_position_once(
            UiTextCode::ALL.iter().map(listed_codes::position).collect(),
        );
    }

    #[test]
    fn all_lists_every_code_once() {
        let spellings: BTreeSet<String> = UiTextCode::ALL.iter().map(wire_spelling).collect();

        assert_eq!(
            spellings.len(),
            UiTextCode::ALL.len(),
            "duplicate code in ALL"
        );
        assert_eq!(
            wire_spelling(&UiTextCode::AmountAssumesTwoDecimals),
            "amount_assumes_two_decimals"
        );
    }

    #[test]
    fn text_serializes_as_code_and_params() {
        let text = UiText::new(UiTextCode::TransferFee)
            .with_param("fee_minor", "140")
            .with_param("currency", "EUR");

        assert_eq!(
            serde_json::to_value(&text).ok(),
            Some(serde_json::json!({
                "code": "transfer_fee",
                "params": { "currency": "EUR", "fee_minor": "140" },
            }))
        );
    }
}
