//! Text for the user that Rust decides on and the UI words.
//!
//! Business rules decide which note applies and with what values. They never
//! write the sentence: they return a [`UiText`], a stable `snake_case` code plus
//! named string parameters, and the UI turns it into the user's language. A
//! parameter is a plain value (a currency code, an ISO date, an integer count
//! of minor units), never a formatted number or a piece of a sentence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Every note the document analyzer can attach to a suggestion.
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
}

impl UiTextCode {
    /// Every code, in the order the analyzer can meet them.
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
    /// Text with no values.
    #[must_use]
    pub fn new(code: UiTextCode) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    /// Add one named value, replacing an earlier value of the same name.
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

    /// The wire spelling of a value that serializes as one string.
    fn wire_spelling<T: Serialize>(value: &T) -> String {
        match serde_json::to_value(value) {
            Ok(serde_json::Value::String(spelling)) => spelling,
            other => format!("not a string: {other:?}"),
        }
    }

    /// The shared fixture the web side reads. Rust and the UI map must agree.
    #[derive(Deserialize)]
    struct Fixture {
        notes: Vec<String>,
        hints: Vec<String>,
        #[serde(rename = "syntheticLines")]
        synthetic_lines: Vec<String>,
    }

    #[expect(
        clippy::expect_used,
        reason = "a malformed fixture must fail the test loudly"
    )]
    fn fixture() -> Fixture {
        serde_json::from_str(include_str!("../../../web/src/lib/uiTextCodes.json"))
            .expect("uiTextCodes.json parses")
    }

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

    /// How many variants [`UiTextCode`] has. Kept by hand, next to the index
    /// below: it is the number the indices must reach.
    const CODE_COUNT: usize = 22;

    /// The position of a variant, from an exhaustive `match` with no wildcard
    /// arm. Adding a variant stops compiling here until it is given the next
    /// index.
    fn code_index(value: UiTextCode) -> usize {
        match value {
            UiTextCode::NoTextExtracted => 0,
            UiTextCode::AmountAssumesTwoDecimals => 1,
            UiTextCode::DatedFromDocument => 2,
            UiTextCode::AddPayableAccount => 3,
            UiTextCode::PdfOverBudget => 4,
            UiTextCode::ParsedFromDocumentText => 5,
            UiTextCode::OcrPathMissing => 6,
            UiTextCode::OcrModelsMissing => 7,
            UiTextCode::OcrRead => 8,
            UiTextCode::OcrLittleText => 9,
            UiTextCode::OcrFailed => 10,
            UiTextCode::OcrPdfImage => 11,
            UiTextCode::InvoiceParsed => 12,
            UiTextCode::InvoiceNoTotal => 13,
            UiTextCode::InvoiceIncome => 14,
            UiTextCode::InvoiceUtility => 15,
            UiTextCode::InvoiceUnpaid => 16,
            UiTextCode::InvoiceVatExempt => 17,
            UiTextCode::TransferDetected => 18,
            UiTextCode::TransferNoAmount => 19,
            UiTextCode::TransferFee => 20,
            UiTextCode::TransferFeeUnstated => 21,
        }
    }

    /// Fails when a variant is missing from `ALL`, repeated, or out of
    /// order: the indices of the listed variants must be exactly `0..CODE_COUNT`. It
    /// does not check that `CODE_COUNT` was raised for a new variant.
    #[test]
    fn all_lists_exactly_the_variants_of_the_enum() {
        let indices: Vec<usize> = UiTextCode::ALL.iter().copied().map(code_index).collect();

        assert_eq!(indices, (0..CODE_COUNT).collect::<Vec<_>>());
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
