//! Extract draft entry fields from bill/receipt bytes.
//!
//! Fully offline pipeline (ships with the app):
//! 1. **PDF / plain text** → text extract
//! 2. **Images** → bundled neural OCR (`ocrs` models in app resources)
//! 3. **Invoice reader** → Greek/EU totals, MARK, kind (no cloud)

use serde::{Deserialize, Serialize};

use super::invoice::parse_invoice_text;
use super::ocr::{OcrModelPaths, ocr_available, ocr_image_bytes};
use super::store::{
    DocumentId, match_expense_account, match_income_account, match_payable_account,
    match_wallet_account,
};
use crate::domain::{Account, AccountId};
use crate::error::Result;

/// Suggested high-level entry kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKindSuggestion {
    /// Money spent / bill category.
    Expense,
    /// Money received.
    Income,
    /// Bill (may be unpaid).
    Bill,
}

/// Where the suggestion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzeSource {
    /// Bundled on-device OCR + invoice reader.
    BundledOcr,
    /// Text extracted from PDF/plain file + invoice reader.
    Heuristic,
    /// Nothing usable extracted.
    None,
}

/// Draft fields for the UI to review before posting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSuggestion {
    /// Stored document id (encrypted vault).
    pub document_id: DocumentId,
    /// How we analyzed.
    pub source: AnalyzeSource,
    /// Model label for UI (e.g. "ocrs-bundled").
    pub model: Option<String>,
    /// Suggested kind.
    pub kind: EntryKindSuggestion,
    /// Amount in minor units (entity currency assumed).
    pub amount_minor: Option<i64>,
    /// ISO date if found.
    pub entry_date: Option<String>,
    /// Description / merchant line.
    pub description: Option<String>,
    /// Invoice / reference number.
    pub reference: Option<String>,
    /// Merchant / biller name.
    pub merchant: Option<String>,
    /// Whether bill appears unpaid.
    pub bill_unpaid: bool,
    /// Suggested expense/income account.
    pub category_account_id: Option<AccountId>,
    /// Suggested bank/cash/card.
    pub wallet_account_id: Option<AccountId>,
    /// Suggested bills payable.
    pub payable_account_id: Option<AccountId>,
    /// 0.0–1.0 rough confidence.
    pub confidence: f32,
    /// Human notes for the UI.
    pub notes: String,
}

/// Status of the integrated analyzer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerStatus {
    /// Bundled OCR model files are present.
    pub ocr_available: bool,
    /// Always offline.
    pub offline: bool,
    /// Short user-facing status.
    pub hint: String,
}

/// Describe capability given model directory.
#[must_use]
pub fn analyzer_status(model_dir: Option<&std::path::Path>) -> AnalyzerStatus {
    let paths = model_dir.map(OcrModelPaths::from_dir);
    let ok = paths.as_ref().is_some_and(ocr_available);
    AnalyzerStatus {
        ocr_available: ok,
        offline: true,
        hint: if ok {
            "Built-in offline invoice reader + OCR — nothing leaves this device.".into()
        } else {
            "OCR models missing from the app bundle. Text PDFs still use the offline invoice reader.".into()
        },
    }
}

/// Analyze raw file bytes into a draft suggestion (fully offline).
///
/// # Errors
///
/// Hard failures only (e.g. corrupt image after OCR path chosen).
pub fn analyze_document_bytes(
    document_id: DocumentId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
    accounts: &[Account],
    default_currency: &str,
    model_dir: Option<&std::path::Path>,
) -> Result<DocumentSuggestion> {
    let _ = default_currency;
    let mime = mime_type.to_ascii_lowercase();
    let is_image = mime.starts_with("image/");

    let mut source = AnalyzeSource::None;
    let mut model_label: Option<String> = None;
    let mut notes_prefix = String::new();

    let text = if is_image {
        if let Some(dir) = model_dir {
            let paths = OcrModelPaths::from_dir(dir);
            if ocr_available(&paths) {
                match ocr_image_bytes(&paths, data) {
                    Ok(t) if !t.trim().is_empty() => {
                        source = AnalyzeSource::BundledOcr;
                        model_label = Some("ocrs-bundled".into());
                        notes_prefix =
                            "Read with built-in offline OCR. Review before saving.".into();
                        Some(t)
                    }
                    Ok(_) => {
                        notes_prefix =
                            "OCR ran but found little text — fill the form manually if needed."
                                .into();
                        None
                    }
                    Err(e) => {
                        notes_prefix = format!("OCR error: {e}. You can still enter the fields.");
                        None
                    }
                }
            } else {
                notes_prefix =
                    "Bundled OCR models not found. Use a text PDF or enter fields manually.".into();
                None
            }
        } else {
            notes_prefix = "OCR model path not configured.".into();
            None
        }
    } else {
        let t = extract_text(filename, &mime, data);
        if t.is_some() {
            source = AnalyzeSource::Heuristic;
            notes_prefix =
                "Parsed from document text on-device (no network). Review before saving.".into();
        }
        t
    };

    let mut suggestion = if let Some(ref body) = text {
        if source == AnalyzeSource::None {
            source = AnalyzeSource::Heuristic;
        }
        let mut s = parse_invoice_text(body);
        if !notes_prefix.is_empty() {
            s.notes = format!("{} {}", notes_prefix, s.notes);
        }
        s
    } else {
        empty_suggestion(if notes_prefix.is_empty() {
            "Could not extract text from this file."
        } else {
            &notes_prefix
        })
    };

    let model = model_label.or_else(|| suggestion.model.clone());
    finalize_suggestion(&mut suggestion, document_id, accounts, source, model);
    Ok(suggestion)
}

fn finalize_suggestion(
    s: &mut DocumentSuggestion,
    document_id: DocumentId,
    accounts: &[Account],
    source: AnalyzeSource,
    model: Option<String>,
) {
    s.document_id = document_id;
    s.source = source;
    s.model = model;

    let hint = format!(
        "{} {}",
        s.merchant.as_deref().unwrap_or(""),
        s.description.as_deref().unwrap_or("")
    );

    if s.category_account_id.is_none() {
        s.category_account_id = match s.kind {
            EntryKindSuggestion::Income => match_income_account(accounts, &hint),
            EntryKindSuggestion::Expense | EntryKindSuggestion::Bill => {
                match_expense_account(accounts, &hint)
            }
        };
    }
    if s.wallet_account_id.is_none() {
        s.wallet_account_id = match_wallet_account(accounts);
    }
    if s.payable_account_id.is_none() {
        s.payable_account_id = match_payable_account(accounts);
    }

    if s.kind == EntryKindSuggestion::Bill && s.bill_unpaid && s.payable_account_id.is_none() {
        s.notes = format!(
            "{} Add a Bills Payable liability account to track unpaid bills.",
            s.notes
        );
    }
}

fn empty_suggestion(notes: &str) -> DocumentSuggestion {
    DocumentSuggestion {
        document_id: DocumentId(uuid::Uuid::nil()),
        source: AnalyzeSource::None,
        model: None,
        kind: EntryKindSuggestion::Expense,
        amount_minor: None,
        entry_date: None,
        description: None,
        reference: None,
        merchant: None,
        bill_unpaid: false,
        category_account_id: None,
        wallet_account_id: None,
        payable_account_id: None,
        confidence: 0.0,
        notes: notes.to_owned(),
    }
}

fn extract_text(filename: &str, mime: &str, data: &[u8]) -> Option<String> {
    if mime == "text/plain" || filename.to_ascii_lowercase().ends_with(".txt") {
        return Some(String::from_utf8_lossy(data).into_owned());
    }
    if mime == "application/pdf" || filename.to_ascii_lowercase().ends_with(".pdf") {
        return pdf_extract::extract_text_from_mem(data).ok().and_then(|t| {
            let t = t.trim().to_owned();
            if t.is_empty() { None } else { Some(t) }
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::invoice::parse_invoice_text;

    #[test]
    fn english_total_line() {
        let text = "Invoice\nSubtotal 10,00\nTOTAL 45,90 EUR\nThank you";
        let s = parse_invoice_text(text);
        assert_eq!(s.amount_minor, Some(4590));
    }
}
