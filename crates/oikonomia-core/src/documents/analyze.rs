//! Extract draft entry fields from bill/receipt bytes.
//!
//! Fully offline pipeline (ships with the app):
//! 1. **PDF / plain text** → text extract
//! 2. **Images** → bundled neural OCR (`ocrs` models in app resources)
//! 3. **Invoice reader** → Greek/EU totals, MARK, kind (no cloud)

use std::path::Path;

// The PDF object model comes from pdf-extract's re-export, never from a
// lopdf dependency of our own: the documents parsed here are handed to
// pdf-extract, so both must be the same lopdf. With two declarations a
// version bump of one stops the build (the types no longer match).
use pdf_extract as lopdf;
use serde::{Deserialize, Serialize};

use super::invoice::read_invoice_text;
use super::ocr::{OcrModelPaths, ocr_available, ocr_image_bytes};
use super::store::{match_expense_account, match_income_account};
use crate::csv::currency_minor_exponent;
use crate::default_accounts::{default_account_for_role, seeded_account_for_role};
use crate::domain::{Account, AccountId, ChartTemplate};
use crate::error::{AccountRole, Result};
use crate::prefs::Locale;
use crate::ui_text::{UiText, UiTextCode};

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
    /// The document's date as `YYYY-MM-DD`, if one was found. Always a day
    /// the calendar has.
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
    /// What the UI should tell the user about this analysis, one coded note
    /// per sentence, in the order they are shown. The UI words them in the
    /// current language; see [`UiTextCode`].
    pub notes: Vec<UiText>,
}

/// What the analyzer status line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzerHint {
    /// The bundled OCR models are present: images and PDFs can be read.
    Ready,
    /// The OCR models are missing: text PDFs still work, images do not.
    ModelsMissing,
}

impl AnalyzerHint {
    /// Every hint. A test checks this list against `web/src/lib/uiTextCodes.json`.
    pub const ALL: &'static [Self] = &[Self::Ready, Self::ModelsMissing];
}

/// Status of the integrated analyzer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerStatus {
    /// Bundled OCR model files are present.
    pub ocr_available: bool,
    /// Always offline.
    pub offline: bool,
    /// Which status line the UI shows.
    pub hint: AnalyzerHint,
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
            AnalyzerHint::Ready
        } else {
            AnalyzerHint::ModelsMissing
        },
    }
}

/// The book a document is analyzed for, and the language to suggest in.
#[derive(Debug, Clone, Copy)]
pub struct AnalyzeContext<'a> {
    /// Chart template of the book, which decides the suggested accounts.
    pub template: ChartTemplate,
    /// The book's accounts the suggestion may point at.
    pub accounts: &'a [Account],
    /// The book's base currency code.
    pub default_currency: &'a str,
    /// Language of the suggested description and merchant.
    pub locale: Locale,
}

/// Analyze raw file bytes into a draft suggestion (fully offline).
///
/// The suggested description and merchant are written in `context.locale`; text taken
/// from the document itself stays as the document has it.
///
/// # Errors
///
/// Hard failures only (e.g. corrupt image after OCR path chosen).
pub fn analyze_document_bytes(
    filename: &str,
    mime_type: &str,
    data: &[u8],
    context: &AnalyzeContext<'_>,
    model_dir: Option<&std::path::Path>,
) -> Result<DocumentSuggestion> {
    let AnalyzeContext {
        template,
        accounts,
        default_currency,
        locale,
    } = *context;
    let mime = mime_type.to_ascii_lowercase();
    let is_image = mime.starts_with("image/");

    let extracted = read_document_text(filename, &mime, data, is_image, model_dir);
    let text = extracted.text;
    let mut source = extracted.source;
    let model_label = extracted.model_label;
    let source_note = extracted.source_note;

    let mut category_hint = String::new();
    let mut suggestion = if let Some(ref body) = text {
        if source == AnalyzeSource::None {
            source = AnalyzeSource::Heuristic;
        }
        let reading = read_invoice_text(body, locale);
        category_hint = reading.category_hint;
        let mut s = reading.suggestion;
        if let Some(fee_minor) = reading.transfer_fee_minor {
            s.notes.push(transfer_fee_note(fee_minor, default_currency));
        }
        if let Some(note) = source_note {
            s.notes.insert(0, note);
        }
        s
    } else {
        empty_suggestion(source_note.unwrap_or_else(|| UiText::new(UiTextCode::NoTextExtracted)))
    };

    let model = model_label.or_else(|| suggestion.model.clone());
    finalize_suggestion(
        &mut suggestion,
        template,
        accounts,
        &category_hint,
        source,
        model,
    );

    // The invoice reader emits 2-exponent minor units (cents). For currencies
    // with a different exponent the value would be silently wrong, so drop it.
    let two_decimals = currency_minor_exponent(default_currency) == 2;
    if !two_decimals && suggestion.amount_minor.is_some() {
        suggestion.amount_minor = None;
        suggestion.notes.push(
            UiText::new(UiTextCode::AmountAssumesTwoDecimals)
                .with_param("currency", default_currency.to_ascii_uppercase()),
        );
    }

    // Document dates (issue or due date) often fall outside the current month;
    // say so, or the entry seems to vanish from the dashboard after posting.
    if let Some(date) = suggestion.entry_date.as_deref() {
        let note = UiText::new(UiTextCode::DatedFromDocument).with_param("date", date);
        suggestion.notes.push(note);
    }

    Ok(suggestion)
}

/// The note for a transfer fee the receipt shows.
///
/// The reader reads the fee as cents. In a 2-decimal book it is sent as a
/// figure with the book's currency, which the UI formats. In any other book
/// the figure would be wrong, so the note states the fee exists without one.
fn transfer_fee_note(fee_minor: i64, currency: &str) -> UiText {
    if currency_minor_exponent(currency) == 2 {
        UiText::new(UiTextCode::TransferFee)
            .with_param("fee_minor", fee_minor.to_string())
            .with_param("currency", currency.to_ascii_uppercase())
    } else {
        UiText::new(UiTextCode::TransferFeeUnstated)
    }
}

fn finalize_suggestion(
    s: &mut DocumentSuggestion,
    template: ChartTemplate,
    accounts: &[Account],
    category_hint: &str,
    source: AnalyzeSource,
    model: Option<String>,
) {
    s.source = source;
    s.model = model;

    if s.category_account_id.is_none() {
        s.category_account_id = match s.kind {
            EntryKindSuggestion::Income => match_income_account(template, accounts, category_hint),
            EntryKindSuggestion::Expense | EntryKindSuggestion::Bill => {
                match_expense_account(template, accounts, category_hint)
            }
        };
    }
    if s.wallet_account_id.is_none() {
        s.wallet_account_id = default_account_for_role(template, accounts, AccountRole::Payment);
    }
    if s.payable_account_id.is_none() {
        s.payable_account_id =
            default_account_for_role(template, accounts, AccountRole::BillsPayable);
    }

    // A seeded book has a real payable account. When identity cannot find it
    // (deactivated or re-coded), any liability suggested above is only a
    // stand-in, and the user must still be told to add a payable account. A
    // blank book seeds none, so there a liability is the legitimate answer.
    let payable_is_missing = s.payable_account_id.is_none()
        || (template != ChartTemplate::Blank
            && seeded_account_for_role(template, accounts, AccountRole::BillsPayable).is_none());

    if s.kind == EntryKindSuggestion::Bill && s.bill_unpaid && payable_is_missing {
        s.notes.push(UiText::new(UiTextCode::AddPayableAccount));
    }
}

struct ExtractedText {
    text: Option<String>,
    source: AnalyzeSource,
    model_label: Option<String>,
    /// The note that says where the text came from, shown before the reader's.
    source_note: Option<UiText>,
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
    let mut source_note = None;

    let text = if is_image {
        ocr_plain_image(
            data,
            model_dir,
            &mut source,
            &mut model_label,
            &mut source_note,
        )
    } else {
        let pdf = mime.contains("pdf") || filename.to_ascii_lowercase().ends_with(".pdf");
        if pdf && !pdf_within_budget(data) {
            source_note = Some(UiText::new(UiTextCode::PdfOverBudget));
            None
        } else {
            let t = extract_text(filename, mime, data);
            if t.is_some() {
                source = AnalyzeSource::Heuristic;
                source_note = Some(UiText::new(UiTextCode::ParsedFromDocumentText));
            }
            if pdf && should_ocr_pdf_images(t.as_deref()) {
                ocr_pdf_embedded_images(
                    data,
                    model_dir,
                    &mut source,
                    &mut model_label,
                    &mut source_note,
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
        source_note,
    }
}

fn ocr_plain_image(
    data: &[u8],
    model_dir: Option<&Path>,
    source: &mut AnalyzeSource,
    model_label: &mut Option<String>,
    source_note: &mut Option<UiText>,
) -> Option<String> {
    let Some(dir) = model_dir else {
        *source_note = Some(UiText::new(UiTextCode::OcrPathMissing));
        return None;
    };
    let paths = OcrModelPaths::from_dir(dir);
    if !ocr_available(&paths) {
        *source_note = Some(UiText::new(UiTextCode::OcrModelsMissing));
        return None;
    }
    match ocr_image_bytes(&paths, data) {
        Ok(t) if !t.trim().is_empty() => {
            *source = AnalyzeSource::BundledOcr;
            *model_label = Some("ocrs-bundled".into());
            *source_note = Some(UiText::new(UiTextCode::OcrRead));
            Some(t)
        }
        Ok(_) => {
            *source_note = Some(UiText::new(UiTextCode::OcrLittleText));
            None
        }
        Err(e) => {
            // The cause can carry file or model detail; it belongs in the log,
            // not on the wire, and the user is told only that OCR failed.
            log::warn!("OCR failed on an image: {e}");
            *source_note = Some(UiText::new(UiTextCode::OcrFailed));
            None
        }
    }
}

fn empty_suggestion(note: UiText) -> DocumentSuggestion {
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
        notes: vec![note],
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
    source_note: &mut Option<UiText>,
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
            *source_note = Some(UiText::new(UiTextCode::OcrPdfImage));
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
    // Only when `DCTDecode` is the one filter are the stored bytes a JPEG file.
    // In a chain such as `[/FlateDecode /DCTDecode]` they are the outer
    // encoding of one.
    let jpeg = matches!(stream.filters().ok()?.as_slice(), [b"DCTDecode"]);
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
    use crate::default_accounts::{code_of_for_tests, seeded_chart_for_tests};
    use crate::domain::AccountType;
    use crate::test_macros::listed_variants;

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

    /// How an image is attached to the one-page test PDF.
    #[derive(Clone, Copy)]
    enum ImagePlacement {
        /// In the page's `/Resources /XObject` dictionary, by reference.
        PageXObject,
        /// The same, with resources and `XObject` dictionaries held by
        /// reference instead of inline.
        PageXObjectByReference,
        /// In the file but not named by any page.
        Unreferenced,
    }

    /// A one-page PDF holding `images`, each `(filter, bytes)`.
    #[expect(clippy::expect_used, reason = "test fails loudly by design")]
    fn pdf_with_images(placement: ImagePlacement, images: &[(lopdf::Object, &[u8])]) -> Vec<u8> {
        use lopdf::{Document, Object, Stream, dictionary};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();

        let mut xobjects = lopdf::Dictionary::new();
        for (index, (filter, bytes)) in images.iter().enumerate() {
            let image = Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Filter" => filter.clone(),
                },
                bytes.to_vec(),
            );
            let image_id = doc.add_object(image);
            xobjects.set(format!("Im{index}"), image_id);
        }

        let resources = match placement {
            ImagePlacement::PageXObject => dictionary! { "XObject" => xobjects },
            ImagePlacement::PageXObjectByReference => {
                let xobjects_id = doc.add_object(xobjects);
                dictionary! { "XObject" => xobjects_id }
            }
            ImagePlacement::Unreferenced => lopdf::Dictionary::new(),
        };
        let resources = match placement {
            ImagePlacement::PageXObjectByReference => Object::Reference(doc.add_object(resources)),
            ImagePlacement::PageXObject | ImagePlacement::Unreferenced => {
                Object::Dictionary(resources)
            }
        };

        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialize test pdf");
        bytes
    }

    fn dct() -> lopdf::Object {
        lopdf::Object::Name(b"DCTDecode".to_vec())
    }

    fn flate() -> lopdf::Object {
        lopdf::Object::Name(b"FlateDecode".to_vec())
    }

    #[test]
    fn jpeg_images_are_read_from_the_page_resources() {
        for placement in [
            ImagePlacement::PageXObject,
            ImagePlacement::PageXObjectByReference,
        ] {
            let pdf = pdf_with_images(placement, &[(dct(), b"jpeg-one")]);

            assert_eq!(extract_pdf_jpeg_images(&pdf), [b"jpeg-one".to_vec()]);
        }
    }

    #[test]
    fn a_filter_array_holding_only_the_jpeg_filter_counts() {
        let filters = lopdf::Object::Array(vec![dct()]);
        let pdf = pdf_with_images(ImagePlacement::PageXObject, &[(filters, b"jpeg-in-array")]);

        assert_eq!(extract_pdf_jpeg_images(&pdf), [b"jpeg-in-array".to_vec()]);
    }

    #[test]
    fn a_jpeg_wrapped_in_another_filter_is_not_taken_as_a_jpeg() {
        // The stored bytes of these streams are Flate or ASCII85 data, not a JPEG.
        let ascii = lopdf::Object::Name(b"ASCII85Decode".to_vec());
        let pdf = pdf_with_images(
            ImagePlacement::PageXObject,
            &[
                (lopdf::Object::Array(vec![flate(), dct()]), b"deflated-jpeg"),
                (lopdf::Object::Array(vec![ascii, dct()]), b"ascii-jpeg"),
                (lopdf::Object::Array(vec![]), b"no-filter"),
            ],
        );

        assert_eq!(extract_pdf_jpeg_images(&pdf), [] as [Vec<u8>; 0]);
    }

    #[test]
    fn images_that_are_not_jpeg_are_skipped() {
        let only_flate = lopdf::Object::Array(vec![flate()]);
        let pdf = pdf_with_images(
            ImagePlacement::PageXObject,
            &[
                (flate(), b"raw-pixels"),
                (only_flate, b"more-pixels"),
                (lopdf::Object::Integer(7), b"odd-filter"),
                (dct(), b"the-jpeg"),
            ],
        );

        assert_eq!(extract_pdf_jpeg_images(&pdf), [b"the-jpeg".to_vec()]);
    }

    #[test]
    fn at_most_two_page_images_are_taken() {
        let pdf = pdf_with_images(
            ImagePlacement::PageXObject,
            &[(dct(), b"a"), (dct(), b"b"), (dct(), b"c")],
        );

        assert_eq!(extract_pdf_jpeg_images(&pdf).len(), 2);
    }

    #[test]
    fn images_no_page_names_are_found_by_scanning_the_file() {
        let pdf = pdf_with_images(
            ImagePlacement::Unreferenced,
            &[
                (dct(), b"a"),
                (flate(), b"not-jpeg"),
                (dct(), b"b"),
                (dct(), b"c"),
            ],
        );

        let mut found = extract_pdf_jpeg_images(&pdf);
        found.sort();

        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|jpeg| jpeg.len() == 1));
    }

    #[test]
    fn an_oversized_jpeg_stream_is_refused() {
        let stream = lopdf::Stream::new(
            lopdf::dictionary! { "Subtype" => "Image", "Filter" => "DCTDecode" },
            vec![0; MAX_PDF_STREAM_BYTES + 1],
        );
        let doc = lopdf::Document::with_version("1.5");

        assert_eq!(jpeg_from_object(&doc, &lopdf::Object::Stream(stream)), None);
    }

    #[test]
    fn objects_that_are_not_image_streams_yield_no_jpeg() {
        let doc = lopdf::Document::with_version("1.5");
        let no_subtype = lopdf::Stream::new(
            lopdf::dictionary! { "Filter" => "DCTDecode" },
            b"x".to_vec(),
        );
        let form = lopdf::Stream::new(
            lopdf::dictionary! { "Subtype" => "Form", "Filter" => "DCTDecode" },
            b"x".to_vec(),
        );
        let no_filter =
            lopdf::Stream::new(lopdf::dictionary! { "Subtype" => "Image" }, b"x".to_vec());

        for object in [
            lopdf::Object::Integer(1),
            lopdf::Object::Reference((99, 0)),
            lopdf::Object::Stream(no_subtype),
            lopdf::Object::Stream(form),
            lopdf::Object::Stream(no_filter),
        ] {
            assert_eq!(jpeg_from_object(&doc, &object), None, "{object:?}");
        }
    }

    #[test]
    fn a_pdf_with_only_an_image_has_no_text_and_asks_for_ocr() {
        let pdf = pdf_with_images(ImagePlacement::PageXObject, &[(dct(), b"jpeg")]);

        let text = extract_text("scan.pdf", "application/pdf", &pdf);

        assert_eq!(text, None);
        assert!(should_ocr_pdf_images(text.as_deref()));
    }

    #[test]
    fn image_ocr_is_skipped_when_no_model_directory_is_given() {
        let pdf = pdf_with_images(ImagePlacement::PageXObject, &[(dct(), b"jpeg")]);
        let mut source = AnalyzeSource::Heuristic;
        let mut label = None;
        let mut note = None;

        let text = ocr_pdf_embedded_images(&pdf, None, &mut source, &mut label, &mut note);

        assert_eq!(text, None);
        assert_eq!(source, AnalyzeSource::Heuristic);
        assert_eq!(label, None);
        assert!(note.is_none());
    }

    /// A PDF with one page per entry of `pages`: `(text, readable)`. A
    /// readable page shows its text in a standard font; the other names a
    /// font that is not a font object, which makes pdf-extract fail on it.
    #[expect(clippy::expect_used, reason = "test fails loudly by design")]
    fn pdf_with_text_pages(pages: &[(&str, bool)]) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{Document, Object, Stream, dictionary};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids: Vec<Object> = Vec::new();

        for (text, readable) in pages {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![20.into(), 100.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let contents = doc.add_object(Stream::new(
                lopdf::Dictionary::new(),
                content.encode().expect("encode page content"),
            ));
            let font: Object = if *readable {
                doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                })
                .into()
            } else {
                7.into()
            };
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => contents,
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            });
            kids.push(page_id.into());
        }

        let count = i64::try_from(kids.len()).expect("page count");
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialize test pdf");
        bytes
    }

    #[test]
    fn page_by_page_extraction_joins_the_pages_in_order() {
        let pdf = pdf_with_text_pages(&[("Alpha", true), ("Omega", true)]);

        let text = pdf_text_per_page(&pdf).unwrap_or_default();

        let (alpha, omega) = (text.find("Alpha"), text.find("Omega"));
        assert!(
            alpha.is_some() && alpha < omega,
            "unexpected text: {text:?}"
        );
    }

    #[test]
    fn a_page_that_cannot_be_read_does_not_blank_the_rest() {
        let pdf = pdf_with_text_pages(&[("Readable", true), ("Broken", false)]);

        // The whole-document pass gives up on this file; the fallback reads
        // the page it can.
        assert_eq!(pdf_text_whole(&pdf), None);
        let text = pdf_text(&pdf).unwrap_or_default();

        assert!(text.contains("Readable"), "unexpected text: {text:?}");
        assert!(!text.contains("Broken"), "unexpected text: {text:?}");
    }

    #[test]
    fn a_pdf_with_no_readable_page_has_no_text() {
        let pdf = pdf_with_text_pages(&[("Broken", false)]);

        assert_eq!(pdf_text_per_page(&pdf), None);
        assert_eq!(pdf_text(&pdf), None);
    }

    #[test]
    fn page_by_page_extraction_reads_the_synthetic_invoice() {
        let pdf = include_bytes!("../../testdata/documents/synthetic/pdf/english_total.pdf");

        let text = pdf_text_per_page(pdf).unwrap_or_default();

        assert!(text.contains("45"), "unexpected text: {text:?}");
        assert_eq!(pdf_text_per_page(b"not a pdf"), None);
    }

    #[test]
    fn page_by_page_extraction_of_a_page_without_text_is_empty_not_missing() {
        let pdf = pdf_with_images(ImagePlacement::PageXObject, &[(dct(), b"jpeg")]);

        assert_eq!(
            pdf_text_per_page(&pdf).map(|text| text.trim().to_owned()),
            Some(String::new())
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
        let s = parse_invoice_text(text, crate::prefs::Locale::En);
        assert_eq!(s.amount_minor, Some(4590));
    }

    fn notes_of(suggestion: Result<DocumentSuggestion>) -> Vec<UiText> {
        suggestion.map_or_else(|_| Vec::new(), |s| s.notes)
    }

    fn codes_of(notes: &[UiText]) -> Vec<UiTextCode> {
        notes.iter().map(|note| note.code).collect()
    }

    #[test]
    fn non_two_exponent_currency_drops_amount() {
        let text = b"Invoice\nTOTAL 45,90\nThank you";
        let eur = analyze_document_bytes(
            "bill.txt",
            "text/plain",
            text,
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        );
        let jpy = analyze_document_bytes(
            "bill.txt",
            "text/plain",
            text,
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "JPY",
                locale: crate::prefs::Locale::En,
            },
            None,
        );

        let eur_notes = notes_of(eur.clone());
        assert_eq!(eur.map(|s| s.amount_minor), Ok(Some(4590)));
        assert!(
            !codes_of(&eur_notes).contains(&UiTextCode::AmountAssumesTwoDecimals),
            "a 2-decimal currency needs no warning"
        );

        let amount = jpy.as_ref().map_or(Some(-1), |s| s.amount_minor);
        assert_eq!(amount, None, "JPY amount must not be prefilled");

        let jpy_notes = notes_of(jpy);
        assert!(
            jpy_notes.contains(
                &UiText::new(UiTextCode::AmountAssumesTwoDecimals).with_param("currency", "JPY")
            ),
            "notes explain the skip: {jpy_notes:?}"
        );
    }

    /// Whether a 45,90 total survives analysis in a book of `currency`.
    fn keeps_the_amount_in(currency: &str) -> bool {
        let suggestion = analyze_document_bytes(
            "bill.txt",
            "text/plain",
            b"Invoice\nTOTAL 45,90\nThank you",
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: currency,
                locale: crate::prefs::Locale::En,
            },
            None,
        );

        suggestion.is_ok_and(|suggestion| suggestion.amount_minor == Some(4590))
    }

    #[test]
    fn the_shared_exponent_table_knows_the_currencies_the_analyzer_dropped() {
        use crate::csv::currency_minor_exponent;

        assert_eq!(currency_minor_exponent("ISK"), 0);
        assert_eq!(currency_minor_exponent("IQD"), 3);
        assert_eq!(currency_minor_exponent("LYD"), 3);
    }

    #[test]
    fn the_amount_is_kept_exactly_for_the_currencies_the_csv_table_gives_two_decimals() {
        let letters = || 'A'..='Z';
        let mut without_two_decimals = Vec::new();

        for code in letters()
            .flat_map(|a| letters().flat_map(move |b| letters().map(move |c| [a, b, c])))
            .map(String::from_iter)
        {
            let two_decimals = crate::csv::currency_minor_exponent(&code) == 2;

            assert_eq!(keeps_the_amount_in(&code), two_decimals, "{code}");
            if !two_decimals {
                without_two_decimals.push(code);
            }
        }

        assert_eq!(
            without_two_decimals,
            [
                "BHD", "CLP", "IQD", "ISK", "JOD", "JPY", "KRW", "KWD", "LYD", "OMR", "TND", "VND"
            ]
        );
        assert!(!keeps_the_amount_in("isk"), "codes are case-insensitive");
    }

    #[test]
    fn a_text_file_gets_the_source_note_then_the_reader_notes_then_the_date_note() {
        let text = b"Invoice\nDate 15/03/2026\nTOTAL 45,90 EUR\nThank you";
        let notes = notes_of(analyze_document_bytes(
            "bill.txt",
            "text/plain",
            text,
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        ));

        assert_eq!(
            codes_of(&notes),
            [
                UiTextCode::ParsedFromDocumentText,
                UiTextCode::InvoiceParsed,
                UiTextCode::DatedFromDocument,
            ]
        );
        assert_eq!(
            notes.last(),
            Some(&UiText::new(UiTextCode::DatedFromDocument).with_param("date", "2026-03-15"))
        );
    }

    #[test]
    fn an_unreadable_file_says_no_text_was_found() {
        let notes = notes_of(analyze_document_bytes(
            "scan.bin",
            "application/octet-stream",
            b"\x00\x01",
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        ));

        assert_eq!(notes, [UiText::new(UiTextCode::NoTextExtracted)]);
    }

    #[test]
    fn an_image_without_a_model_directory_says_the_path_is_missing() {
        let notes = notes_of(analyze_document_bytes(
            "scan.jpg",
            "image/jpeg",
            b"\xff\xd8",
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        ));

        assert_eq!(notes, [UiText::new(UiTextCode::OcrPathMissing)]);
    }

    #[test]
    fn an_image_with_an_empty_model_directory_says_the_models_are_missing() {
        let dir = tempfile::tempdir();
        let path = dir.as_ref().map(|dir| dir.path().to_path_buf());

        let notes = notes_of(analyze_document_bytes(
            "scan.jpg",
            "image/jpeg",
            b"\xff\xd8",
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            path.as_deref().ok(),
        ));

        assert_eq!(notes, [UiText::new(UiTextCode::OcrModelsMissing)]);
    }

    #[test]
    fn an_oversized_pdf_says_it_is_over_budget() {
        // Not a PDF structure, so the loader fails and the size check applies.
        let data = vec![0_u8; MAX_PDF_STREAM_BYTES + 1];
        let notes = notes_of(analyze_document_bytes(
            "big.pdf",
            "application/pdf",
            &data,
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        ));

        assert_eq!(notes, [UiText::new(UiTextCode::PdfOverBudget)]);
    }

    #[test]
    fn an_unpaid_bill_without_a_payable_account_asks_for_one() {
        let text = "Invoice\nTOTAL 45,90 EUR\nAmount due\nThank you";
        let notes = notes_of(analyze_document_bytes(
            "bill.txt",
            "text/plain",
            text.as_bytes(),
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        ));

        assert!(
            codes_of(&notes).contains(&UiTextCode::AddPayableAccount),
            "{notes:?}"
        );
    }

    #[test]
    fn an_unpaid_bill_finds_a_renamed_payable_account_and_asks_for_nothing() {
        let text = "Invoice\nTOTAL 45,90 EUR\nAmount due\nThank you";
        let accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);

        let suggestion = analyze_document_bytes(
            "bill.txt",
            "text/plain",
            text.as_bytes(),
            &AnalyzeContext {
                template: ChartTemplate::Personal,
                accounts: &accounts,
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        );
        assert!(suggestion.is_ok(), "analysis must succeed");
        let Ok(suggestion) = suggestion else { return };

        assert_eq!(
            code_of_for_tests(&accounts, suggestion.payable_account_id).as_deref(),
            Some("2050"),
        );
        assert!(
            !codes_of(&suggestion.notes).contains(&UiTextCode::AddPayableAccount),
            "{:?}",
            suggestion.notes
        );
        assert_eq!(
            code_of_for_tests(&accounts, suggestion.wallet_account_id).as_deref(),
            Some("1010"),
        );
    }

    const UNPAID_BILL: &str = "Invoice\nTOTAL 45,90 EUR\nAmount due\nThank you";

    fn analyze_unpaid_bill(
        template: ChartTemplate,
        accounts: &[Account],
    ) -> Option<DocumentSuggestion> {
        analyze_document_bytes(
            "bill.txt",
            "text/plain",
            UNPAID_BILL.as_bytes(),
            &AnalyzeContext {
                template,
                accounts,
                default_currency: "EUR",
                locale: crate::prefs::Locale::En,
            },
            None,
        )
        .ok()
    }

    fn deactivate_code(accounts: &mut [Account], code: &str) {
        for account in accounts {
            if account.code == code {
                account.is_active = false;
            }
        }
    }

    #[test]
    fn a_seeded_book_without_its_payable_still_suggests_a_liability_and_asks_for_one() {
        let cases = [
            (ChartTemplate::Personal, "2050", "2000"),
            (ChartTemplate::Company, "2000", "2100"),
        ];

        for (template, payable_code, fallback_code) in cases {
            let mut accounts = seeded_chart_for_tests(template, true);
            deactivate_code(&mut accounts, payable_code);

            let suggestion = analyze_unpaid_bill(template, &accounts);
            assert!(suggestion.is_some(), "{template:?}: analysis must succeed");
            let Some(suggestion) = suggestion else { return };

            assert_eq!(
                code_of_for_tests(&accounts, suggestion.payable_account_id).as_deref(),
                Some(fallback_code),
                "{template:?}: the by-type stand-in stays so the form is usable",
            );
            assert!(
                codes_of(&suggestion.notes).contains(&UiTextCode::AddPayableAccount),
                "{template:?}: {:?}",
                suggestion.notes
            );
        }
    }

    #[test]
    fn a_blank_book_with_a_liability_suggests_it_without_a_note() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);
        accounts.retain(|account| {
            account.account_type != AccountType::Liability || account.code == "2100"
        });

        let suggestion = analyze_unpaid_bill(ChartTemplate::Blank, &accounts);
        assert!(suggestion.is_some(), "analysis must succeed");
        let Some(suggestion) = suggestion else { return };

        assert_eq!(
            code_of_for_tests(&accounts, suggestion.payable_account_id).as_deref(),
            Some("2100"),
        );
        assert!(
            !codes_of(&suggestion.notes).contains(&UiTextCode::AddPayableAccount),
            "{:?}",
            suggestion.notes
        );
    }

    #[test]
    fn a_blank_book_without_a_liability_asks_for_a_payable_and_suggests_none() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);
        accounts.retain(|account| account.account_type != AccountType::Liability);

        let suggestion = analyze_unpaid_bill(ChartTemplate::Blank, &accounts);
        assert!(suggestion.is_some(), "analysis must succeed");
        let Some(suggestion) = suggestion else { return };

        assert_eq!(suggestion.payable_account_id, None);
        assert!(
            codes_of(&suggestion.notes).contains(&UiTextCode::AddPayableAccount),
            "{:?}",
            suggestion.notes
        );
    }

    /// A Greek bank transfer receipt with a 1,40 fee beside a 310,00 principal.
    const TRANSFER_RECEIPT: &str =
        include_str!("../../testdata/documents/synthetic/text/greek_bank_embasma.txt");

    fn analyze_receipt_in(currency: &str) -> Vec<UiText> {
        notes_of(analyze_document_bytes(
            "embasma.txt",
            "text/plain",
            TRANSFER_RECEIPT.as_bytes(),
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: currency,
                locale: crate::prefs::Locale::En,
            },
            None,
        ))
    }

    #[test]
    fn a_transfer_fee_in_a_two_decimal_book_carries_its_amount_and_the_book_currency() {
        let notes = analyze_receipt_in("EUR");

        assert!(
            notes.contains(
                &UiText::new(UiTextCode::TransferFee)
                    .with_param("fee_minor", "140")
                    .with_param("currency", "EUR")
            ),
            "{notes:?}"
        );
        assert!(
            !codes_of(&notes).contains(&UiTextCode::TransferFeeUnstated),
            "{notes:?}"
        );
    }

    #[test]
    fn a_transfer_fee_in_a_book_without_two_decimals_is_mentioned_without_a_figure() {
        let notes = analyze_receipt_in("JPY");

        assert!(
            notes.contains(&UiText::new(UiTextCode::TransferFeeUnstated)),
            "{notes:?}"
        );
        assert!(
            !codes_of(&notes).contains(&UiTextCode::TransferFee),
            "a cents figure must not be shown for JPY: {notes:?}"
        );
    }

    #[test]
    fn the_fee_note_keeps_its_place_between_the_reader_notes_and_the_amount_warning() {
        let notes = analyze_receipt_in("JPY");

        assert_eq!(
            codes_of(&notes),
            [
                UiTextCode::ParsedFromDocumentText,
                UiTextCode::InvoiceParsed,
                UiTextCode::TransferDetected,
                UiTextCode::TransferFeeUnstated,
                UiTextCode::AmountAssumesTwoDecimals,
                UiTextCode::DatedFromDocument,
            ]
        );
    }

    #[test]
    fn a_receipt_without_a_fee_gets_no_fee_note() {
        let notes = notes_of(analyze_document_bytes(
            "bill.txt",
            "text/plain",
            b"Invoice\nTOTAL 45,90\nThank you",
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "JPY",
                locale: crate::prefs::Locale::En,
            },
            None,
        ));
        let codes = codes_of(&notes);

        assert!(!codes.contains(&UiTextCode::TransferFee), "{codes:?}");
        assert!(
            !codes.contains(&UiTextCode::TransferFeeUnstated),
            "{codes:?}"
        );
    }

    #[test]
    fn the_hint_says_whether_the_models_are_present() {
        assert_eq!(analyzer_status(None).hint, AnalyzerHint::ModelsMissing);

        let dir = tempfile::tempdir();
        assert!(dir.is_ok(), "the temporary directory must be created");
        let Ok(dir) = dir else { return };
        assert_eq!(
            analyzer_status(Some(dir.path())).hint,
            AnalyzerHint::ModelsMissing
        );

        for file in ["text-detection.rten", "text-recognition.rten"] {
            assert!(std::fs::write(dir.path().join(file), b"x").is_ok());
        }
        assert_eq!(analyzer_status(Some(dir.path())).hint, AnalyzerHint::Ready);
    }

    listed_variants! {
        units listed_hints for AnalyzerHint {
            AnalyzerHint::Ready,
            AnalyzerHint::ModelsMissing,
        }
    }

    /// Fails unless `AnalyzerHint::ALL` is exactly the set of variants in the
    /// `listed_hints` list above, each once. The compiler checks that list
    /// against the enum with an exhaustive `match`, so a variant added to the
    /// enum but left out of the list does not compile. It does not check the
    /// order of `ALL`, nor that the UI has copy for a hint; the shared-fixture
    /// test in `ui_text` does that.
    #[test]
    fn all_lists_every_hint_variant() {
        let listed = listed_hints::variants();

        assert_eq!(
            AnalyzerHint::ALL.len(),
            listed_hints::COUNT,
            "AnalyzerHint::ALL and the listed variants differ in number"
        );
        for variant in listed {
            assert!(
                AnalyzerHint::ALL.contains(&variant),
                "{variant:?} is missing from AnalyzerHint::ALL"
            );
        }
        listed_hints::assert_every_position_once(
            AnalyzerHint::ALL
                .iter()
                .map(listed_hints::position)
                .collect(),
        );
    }

    #[test]
    fn the_status_serializes_the_hint_as_a_snake_case_code() {
        let json = serde_json::to_value(analyzer_status(None)).ok();

        assert_eq!(
            json,
            Some(serde_json::json!({
                "ocr_available": false,
                "offline": true,
                "hint": "models_missing",
            }))
        );
    }
}
