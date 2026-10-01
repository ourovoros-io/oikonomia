//! Extract draft entry fields from bill/receipt bytes.
//!
//! Fully offline pipeline (ships with the app):
//! 1. **PDF / plain text** → text extract
//! 2. **Images** → bundled neural OCR (`ocrs` models in app resources)
//! 3. **Invoice reader** → Greek/EU totals, MARK, kind (no cloud)

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::invoice::parse_invoice_text;
use super::ocr::{OcrModelPaths, ocr_available, ocr_image_bytes};
use super::store::{
    match_expense_account, match_income_account, match_payable_account, match_wallet_account,
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
    filename: &str,
    mime_type: &str,
    data: &[u8],
    accounts: &[Account],
    default_currency: &str,
    model_dir: Option<&std::path::Path>,
) -> Result<DocumentSuggestion> {
    let mime = mime_type.to_ascii_lowercase();
    let is_image = mime.starts_with("image/");

    let extracted = read_document_text(filename, &mime, data, is_image, model_dir);
    let text = extracted.text;
    let mut source = extracted.source;
    let model_label = extracted.model_label;
    let notes_prefix = extracted.notes_prefix;

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
    finalize_suggestion(&mut suggestion, accounts, source, model);

    // The invoice reader emits 2-exponent minor units (cents). For currencies
    // with a different exponent the value would be silently wrong, so drop it.
    if currency_exponent(default_currency) != 2 && suggestion.amount_minor.is_some() {
        suggestion.amount_minor = None;
        suggestion.notes = format!(
            "{} Amount detection assumes 2-decimal currencies; enter the {default_currency} amount manually.",
            suggestion.notes
        );
    }

    // Document dates (issue or due date) often fall outside the current month;
    // say so, or the entry seems to vanish from the dashboard after posting.
    if let Some(date) = suggestion.entry_date.as_deref() {
        suggestion.notes = format!(
            "{} Entry will be dated {date} (from the document) and counts toward that month — adjust the date if you want it in a different period.",
            suggestion.notes
        );
    }

    Ok(suggestion)
}

/// ISO 4217 minor-unit exponent for the currencies the app offers.
fn currency_exponent(code: &str) -> u32 {
    match code.to_ascii_uppercase().as_str() {
        "JPY" | "KRW" | "VND" | "CLP" | "ISK" => 0,
        "BHD" | "KWD" | "OMR" | "TND" | "JOD" | "IQD" | "LYD" => 3,
        _ => 2,
    }
}

fn finalize_suggestion(
    s: &mut DocumentSuggestion,
    accounts: &[Account],
    source: AnalyzeSource,
    model: Option<String>,
) {
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

struct ExtractedText {
    text: Option<String>,
    source: AnalyzeSource,
    model_label: Option<String>,
    notes_prefix: String,
}

fn read_document_text(
    filename: &str,
    mime: &str,
    data: &[u8],
    is_image: bool,
    model_dir: Option<&Path>,
) -> ExtractedText {
    let mut source = AnalyzeSource::None;
    let mut model_label = None;
    let mut notes_prefix = String::new();

    let text = if is_image {
        ocr_plain_image(
            data,
            model_dir,
            &mut source,
            &mut model_label,
            &mut notes_prefix,
        )
    } else {
        let pdf = mime.contains("pdf") || filename.to_ascii_lowercase().ends_with(".pdf");
        if pdf && !pdf_within_budget(data) {
            notes_prefix =
                "PDF exceeds the page or stream budget; enter the fields manually.".into();
            None
        } else {
            let t = extract_text(filename, mime, data);
            if t.is_some() {
                source = AnalyzeSource::Heuristic;
                notes_prefix =
                    "Parsed from document text on-device (no network). Review before saving."
                        .into();
            }
            if pdf && should_ocr_pdf_images(t.as_deref()) {
                ocr_pdf_embedded_images(
                    data,
                    model_dir,
                    &mut source,
                    &mut model_label,
                    &mut notes_prefix,
                )
                .or(t)
            } else {
                t
            }
        }
    };

    ExtractedText {
        text,
        source,
        model_label,
        notes_prefix,
    }
}

fn ocr_plain_image(
    data: &[u8],
    model_dir: Option<&Path>,
    source: &mut AnalyzeSource,
    model_label: &mut Option<String>,
    notes_prefix: &mut String,
) -> Option<String> {
    let Some(dir) = model_dir else {
        *notes_prefix = "OCR model path not configured.".into();
        return None;
    };
    let paths = OcrModelPaths::from_dir(dir);
    if !ocr_available(&paths) {
        *notes_prefix =
            "Bundled OCR models not found. Use a text PDF or enter fields manually.".into();
        return None;
    }
    match ocr_image_bytes(&paths, data) {
        Ok(t) if !t.trim().is_empty() => {
            *source = AnalyzeSource::BundledOcr;
            *model_label = Some("ocrs-bundled".into());
            *notes_prefix = "Read with built-in offline OCR. Review before saving.".into();
            Some(t)
        }
        Ok(_) => {
            *notes_prefix =
                "OCR ran but found little text — fill the form manually if needed.".into();
            None
        }
        Err(e) => {
            *notes_prefix = format!("OCR error: {e}. You can still enter the fields.");
            None
        }
    }
}

fn empty_suggestion(notes: &str) -> DocumentSuggestion {
    DocumentSuggestion {
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

const MIN_PDF_TEXT_CHARS: usize = 8;
const MAX_PDF_PAGES: usize = 50;
const MAX_PDF_STREAM_BYTES: usize = 32 * 1024 * 1024;

fn should_ocr_pdf_images(text: Option<&str>) -> bool {
    text.is_none_or(|t| t.chars().count() < MIN_PDF_TEXT_CHARS)
}

fn ocr_pdf_embedded_images(
    data: &[u8],
    model_dir: Option<&Path>,
    source: &mut AnalyzeSource,
    model_label: &mut Option<String>,
    notes_prefix: &mut String,
) -> Option<String> {
    let dir = model_dir?;
    let paths = OcrModelPaths::from_dir(dir);
    if !ocr_available(&paths) {
        return None;
    }
    for jpeg in extract_pdf_jpeg_images(data).into_iter().take(2) {
        let Ok(text) = ocr_image_bytes(&paths, &jpeg) else {
            continue;
        };
        if text.chars().count() > 8 {
            *source = AnalyzeSource::BundledOcr;
            *model_label = Some("ocrs-bundled".into());
            *notes_prefix =
                "Read embedded PDF image with built-in OCR. Review before saving.".into();
            return Some(text);
        }
    }
    None
}

/// JPEG (`DCTDecode`) image streams only — no new PDF rasterizer.
/// Page `/XObject` images are preferred so a logo in the catalog is not first.
fn extract_pdf_jpeg_images(data: &[u8]) -> Vec<Vec<u8>> {
    let Ok(doc) = lopdf::Document::load_mem(data) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for page_id in doc.get_pages().values().copied().take(MAX_PDF_PAGES) {
        collect_jpegs_from_page(&doc, page_id, &mut out);
        if out.len() >= 2 {
            return out;
        }
    }
    if !out.is_empty() {
        return out;
    }
    for object in doc.objects.values() {
        if let Some(jpeg) = jpeg_from_object(&doc, object) {
            out.push(jpeg);
            if out.len() >= 2 {
                break;
            }
        }
    }
    out
}

fn collect_jpegs_from_page(
    doc: &lopdf::Document,
    page_id: lopdf::ObjectId,
    out: &mut Vec<Vec<u8>>,
) {
    let Ok(page) = doc.get_dictionary(page_id) else {
        return;
    };
    let Some(resources) = dict_ref_or_inline(doc, page.get(b"Resources").ok()) else {
        return;
    };
    let Some(xobjects) = dict_ref_or_inline(doc, resources.get(b"XObject").ok()) else {
        return;
    };
    for (_name, object) in xobjects {
        if let Some(jpeg) = jpeg_from_object(doc, object) {
            out.push(jpeg);
            if out.len() >= 2 {
                return;
            }
        }
    }
}

fn dict_ref_or_inline<'a>(
    doc: &'a lopdf::Document,
    object: Option<&'a lopdf::Object>,
) -> Option<&'a lopdf::Dictionary> {
    match object? {
        lopdf::Object::Dictionary(dict) => Some(dict),
        lopdf::Object::Reference(id) => doc.get_dictionary(*id).ok(),
        _ => None,
    }
}

fn jpeg_from_object(doc: &lopdf::Document, object: &lopdf::Object) -> Option<Vec<u8>> {
    let stream = match object {
        lopdf::Object::Stream(stream) => stream,
        lopdf::Object::Reference(id) => match doc.objects.get(id) {
            Some(lopdf::Object::Stream(stream)) => stream,
            _ => return None,
        },
        _ => return None,
    };
    let subtype = stream.dict.get(b"Subtype").ok()?;
    let is_image = matches!(subtype, lopdf::Object::Name(name) if name == b"Image");
    if !is_image {
        return None;
    }
    let filter = stream.dict.get(b"Filter").ok()?;
    let jpeg = match filter {
        lopdf::Object::Name(name) if name == b"DCTDecode" => true,
        lopdf::Object::Array(arr) => arr
            .iter()
            .any(|item| matches!(item, lopdf::Object::Name(name) if name == b"DCTDecode")),
        _ => false,
    };
    if !jpeg || stream.content.len() > MAX_PDF_STREAM_BYTES {
        return None;
    }
    Some(stream.content.clone())
}

fn pdf_within_budget(data: &[u8]) -> bool {
    let Ok(doc) = lopdf::Document::load_mem(data) else {
        return data.len() <= MAX_PDF_STREAM_BYTES;
    };
    if doc.get_pages().len() > MAX_PDF_PAGES {
        return false;
    }
    let mut total = 0usize;
    for object in doc.objects.values() {
        if let lopdf::Object::Stream(stream) = object {
            total = total.saturating_add(stream.content.len());
            if total > MAX_PDF_STREAM_BYTES {
                return false;
            }
        }
    }
    true
}

fn extract_text(filename: &str, mime: &str, data: &[u8]) -> Option<String> {
    if mime == "text/plain" || filename.to_ascii_lowercase().ends_with(".txt") {
        return Some(String::from_utf8_lossy(data).into_owned());
    }
    if mime == "application/pdf" || filename.to_ascii_lowercase().ends_with(".pdf") {
        return pdf_text(data).and_then(|t| {
            let t = t.trim().to_owned();
            if t.is_empty() { None } else { Some(t) }
        });
    }
    None
}

/// PDF text extraction hardened for real-world statements and invoices.
///
/// Two failure modes show up in the wild, especially with bank statements:
/// stale xref offsets left behind by stamping/signing tools (lopdf refuses
/// to load), and malformed font or resource objects that make pdf-extract
/// panic mid-page. We repair the former and contain the latter, falling
/// back to page-by-page extraction so one bad page cannot blank the rest.
fn pdf_text(data: &[u8]) -> Option<String> {
    if !pdf_within_budget(data) {
        return None;
    }
    if let Some(text) = pdf_text_whole(data).or_else(|| pdf_text_per_page(data)) {
        return Some(text);
    }

    let repaired = super::pdf_repair::repair_xref_offsets(data)?;

    pdf_text_whole(&repaired).or_else(|| pdf_text_per_page(&repaired))
}

/// Whole-document pass. pdf-extract calls `expect` on odd font objects, so
/// panics are contained here and treated as "no text".
fn pdf_text_whole(data: &[u8]) -> Option<String> {
    std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(data).ok())
        .ok()
        .flatten()
}

/// Page-by-page pass: pages whose resources make pdf-extract error or panic
/// are skipped, and the surviving pages' text is joined.
fn pdf_text_per_page(data: &[u8]) -> Option<String> {
    let doc = lopdf::Document::load_mem(data).ok()?;
    if doc.is_encrypted() {
        return None;
    }

    let page_numbers: Vec<u32> = doc
        .get_pages()
        .keys()
        .copied()
        .take(MAX_PDF_PAGES)
        .collect();

    let mut chunks: Vec<String> = Vec::new();
    for page in page_numbers {
        let extracted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut text = String::new();
            let mut output = pdf_extract::PlainTextOutput::new(&mut text);

            pdf_extract::output_doc_page(&doc, &mut output, page)
                .ok()
                .map(|()| text)
        }));

        if let Ok(Some(text)) = extracted {
            chunks.push(text);
        }
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::invoice::parse_invoice_text;
    use super::*;

    #[test]
    fn extract_pdf_jpeg_images_ignores_non_pdf() {
        assert_eq!(
            extract_pdf_jpeg_images(b"not a pdf"),
            [] as [std::vec::Vec<u8>; 0]
        );
        assert_eq!(
            extract_pdf_jpeg_images(b"%PDF-1.4\ntrailer\n%%EOF"),
            [] as [std::vec::Vec<u8>; 0]
        );
    }

    #[test]
    fn short_or_missing_pdf_text_triggers_image_ocr() {
        assert!(should_ocr_pdf_images(None));
        assert!(should_ocr_pdf_images(Some("abc")));
        assert!(should_ocr_pdf_images(Some("1234567")));
        assert!(!should_ocr_pdf_images(Some("12345678")));
    }

    #[test]
    fn tiny_non_pdf_is_within_budget() {
        assert!(pdf_within_budget(b"not a pdf"));
    }

    #[test]
    fn english_total_line() {
        let text = "Invoice\nSubtotal 10,00\nTOTAL 45,90 EUR\nThank you";
        let s = parse_invoice_text(text);
        assert_eq!(s.amount_minor, Some(4590));
    }

    #[test]
    fn non_two_exponent_currency_drops_amount() {
        let text = b"Invoice\nTOTAL 45,90\nThank you";
        let eur = analyze_document_bytes("bill.txt", "text/plain", text, &[], "EUR", None);
        let jpy = analyze_document_bytes("bill.txt", "text/plain", text, &[], "JPY", None);

        assert_eq!(eur.map(|s| s.amount_minor), Ok(Some(4590)));

        let (amount, notes) = jpy.map_or((Some(-1), String::new()), |s| (s.amount_minor, s.notes));
        assert_eq!(amount, None, "JPY amount must not be prefilled");
        assert!(notes.contains("JPY"), "notes explain the skip: {notes}");
    }
}
