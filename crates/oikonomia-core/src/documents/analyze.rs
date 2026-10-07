//! From the bytes of a file to a draft entry: text extraction, then the
//! invoice reader, then the book's accounts.
//!
//! [`analyze_document_bytes`] is the entry point. It returns a suggestion for
//! every input: a file that cannot be read yields an empty suggestion with a
//! note that says why.
//!
//! # Getting the text
//!
//! [`read_document_text`] resolves the kind of the file once
//! ([`DocumentKind::resolve`]) and reads it by that kind:
//!
//! 1. an image goes to OCR ([`ocr_image`]);
//! 2. plain text is taken as it is, decoded as UTF-8 with invalid bytes
//!    replaced;
//! 3. a PDF goes through [`read_pdf_text`], described below.
//!
//! A file of no kind has no text. The declared type and the name are not
//! looked at again after the kind is resolved; `file.rs` states the rule,
//! including what happens when the two disagree.
//!
//! # Reading a PDF
//!
//! 1. **Budget.** [`load_pdf`] refuses a file over the upload cap before
//!    parsing it, parses it once with lopdf, and then checks, in this order,
//!    the page count, the decoded size of every stream
//!    ([`pdf_budget`](crate::documents::pdf_budget)), and that the form
//!    `XObject`s and the page tree do not make pdf-extract recurse without
//!    end ([`pdf_nesting`](crate::documents::pdf_nesting)). Over budget ends
//!    the analysis with the "over budget" note; nothing of the file is
//!    read.
//! 2. **Whole document.** [`pdf_text_whole`] asks pdf-extract for the text of
//!    the whole file.
//! 3. **Page by page.** When that returns an error or panics,
//!    [`pdf_text_per_page`] reads each page on its own and joins the pages
//!    that can be read. One bad page then costs its own text and no more.
//! 4. **Repair and retry.** When there is still no text, because lopdf could
//!    not parse the file or no page could be read,
//!    [`repair_xref_offsets`] rewrites stale cross-reference offsets in a
//!    copy, and steps 1 to 3 run once more on the copy, the whole budget
//!    included. A file that reads as empty, with no error, is not repaired.
//! 5. **Embedded JPEGs.** A PDF whose text is missing or shorter than
//!    [`MIN_PDF_TEXT_CHARS`] is taken as a scan. Up to
//!    [`MAX_PDF_OCR_IMAGES`] JPEG images embedded in it are read with OCR, in
//!    order, until one yields text. Only streams whose single filter is
//!    `DCTDecode` are taken: their stored bytes are a JPEG file as they are,
//!    so no PDF rasterizer is needed. A scan stored any other way is not
//!    read.
//!
//! Text read from an image wins over a text layer too short to count, and a
//! short text layer stands when no image can be read
//! ([`pdf_text_or_image_text`]).
//!
//! # The panic boundary
//!
//! lopdf and pdf-extract index and unwrap on file content, so a malformed
//! PDF can panic inside them. [`contain_panics`] turns such a panic into "no
//! result" at five places: loading with decryption and the whole budget
//! check, the whole-document pass, listing the pages, each single page, and
//! the search for embedded JPEGs.
//!
//! Outside the boundary are the xref repair and the invoice reader, which
//! are this crate's own code and are property-tested not to panic on
//! arbitrary input, and image decoding, which reports a bad image as an
//! error. OCR inference has a boundary of its own in
//! [`ocr`](crate::documents::ocr).
//!
//! What the boundary cannot catch, and what stands in its place:
//!
//! - A failed allocation aborts the process; it does not unwind. The budget
//!   is what keeps allocations small.
//! - A stack overflow aborts the process too. pdf-extract recurses through
//!   nested forms and up `Parent` links with no limit, so the nesting check
//!   refuses a document that would take it too deep, and only a document
//!   that passed it ([`BudgetedPdf`]) is handed to pdf-extract. The first
//!   load and the repaired copy both go through the check.
//! - lopdf decompresses object streams and cross-reference streams while it
//!   loads a file, inside the boundary but before the budget check can run,
//!   and with no limit of its own. Neither the budget nor the nesting check
//!   covers it: the only bound on that step is the upload cap on the file.
//!   [`pdf_budget`](crate::documents::pdf_budget) says the same.
//!
//! # From text to suggestion
//!
//! The invoice reader ([`read_invoice_text`]) finds the amount, date,
//! reference, merchant, description and kind, in no language.
//! [`suggest_accounts`] chooses the book's accounts for them, and
//! [`suggestion_from_reading`] words the reading in the language of the
//! application and builds the suggestion, once. The notes end up in this
//! order: where the text came from, the reader's own notes, the transfer
//! fee, the request to add a payable account, the warning that the amount
//! was withheld because the book's currency does not have two decimals, and
//! the document's date.

use std::path::Path;

// The PDF object model comes from pdf-extract's re-export, never from a
// lopdf dependency of our own: the documents parsed here are handed to
// pdf-extract, so both must be the same lopdf. With two declarations a
// version bump of one stops the build (the types no longer match).
use pdf_extract as lopdf;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::csv::currency_minor_exponent;
use crate::default_accounts::{default_account_for_role, seeded_account_for_role};
use crate::documents::account_match::{match_expense_account, match_income_account};
use crate::documents::file::{DocumentKind, NewDocument};
use crate::documents::invoice::{InvoiceReading, read_invoice_text};
use crate::documents::ocr::{OcrModelPaths, ocr_available, ocr_image_bytes};
use crate::documents::pdf_load::{BudgetedPdf, PdfLoad, contain_panics, load_pdf};
use crate::documents::pdf_repair::repair_xref_offsets;
use crate::domain::{Account, AccountId, ChartTemplate, CurrencyCode};
use crate::error::AccountRole;
use crate::prefs::Locale;
use crate::ui_text::{UiText, UiTextCode};
use crate::util::format_date;

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
    /// The document's date, if one was found. Written as `YYYY-MM-DD`.
    #[serde(with = "optional_date")]
    pub entry_date: Option<Date>,
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

/// Writes an optional [`Date`] as a `YYYY-MM-DD` string or `null`, the form
/// the web UI reads.
///
/// [`serde_date`](crate::util::serde_date) does the same for a date that is
/// always there.
mod optional_date {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::Date;

    use crate::util::{format_date, parse_date};

    /// Serializes `date` as the string [`format_date`] gives, or as `null`.
    ///
    /// # Errors
    ///
    /// Returns the serializer's own error when it cannot write the value.
    #[expect(
        clippy::ref_option,
        clippy::trivially_copy_pass_by_ref,
        reason = "serde calls a `with` serializer with a reference to the field"
    )]
    pub(super) fn serialize<S: Serializer>(
        date: &Option<Date>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match date {
            Some(date) => serializer.serialize_some(&format_date(*date)),
            None => serializer.serialize_none(),
        }
    }

    /// Deserializes `null` or a string read through [`parse_date`].
    ///
    /// # Errors
    ///
    /// Returns the deserializer's error when the value is neither `null`
    /// nor a string, and a custom error when the string is not a
    /// `YYYY-MM-DD` date.
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Date>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| parse_date(&text).map_err(serde::de::Error::custom))
            .transpose()
    }
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
///
/// The status is one fact: whether OCR can run. On the wire it is three
/// fields, `ocr_available`, `offline` and `hint`, which the UI reads; they
/// are all written from that one fact, so they cannot disagree, and a JSON
/// object in which they do is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "AnalyzerStatusWire", try_from = "AnalyzerStatusWire")]
pub struct AnalyzerStatus {
    /// Whether an engine is loaded or the model files are present.
    ocr_available: bool,
}

impl AnalyzerStatus {
    /// Whether the bundled OCR can run: images and scanned PDFs can be read.
    #[must_use]
    pub const fn ocr_available(self) -> bool {
        self.ocr_available
    }

    /// Which status line the UI shows.
    #[must_use]
    pub const fn hint(self) -> AnalyzerHint {
        if self.ocr_available {
            AnalyzerHint::Ready
        } else {
            AnalyzerHint::ModelsMissing
        }
    }
}

/// The JSON form of an [`AnalyzerStatus`].
#[derive(Debug, Serialize, Deserialize)]
struct AnalyzerStatusWire {
    /// Bundled OCR model files are present.
    ocr_available: bool,
    /// Always `true`: the analyzer never uses the network.
    offline: bool,
    /// Which status line the UI shows.
    hint: AnalyzerHint,
}

impl From<AnalyzerStatus> for AnalyzerStatusWire {
    fn from(status: AnalyzerStatus) -> Self {
        Self {
            ocr_available: status.ocr_available(),
            offline: true,
            hint: status.hint(),
        }
    }
}

impl TryFrom<AnalyzerStatusWire> for AnalyzerStatus {
    type Error = &'static str;

    /// Reads the status back from its wire form.
    ///
    /// # Errors
    ///
    /// Refuses an object whose three fields do not state the same fact.
    fn try_from(wire: AnalyzerStatusWire) -> std::result::Result<Self, Self::Error> {
        let status = Self {
            ocr_available: wire.ocr_available,
        };

        if wire.offline && wire.hint == status.hint() {
            Ok(status)
        } else {
            Err("the analyzer status fields contradict each other")
        }
    }
}

/// What the analyzer can read with the models in `model_dir`.
///
/// OCR counts as available when both model files exist there, or when the
/// process has already loaded an engine. With no directory it is
/// unavailable. The call does not load the models and does not wait for a
/// running OCR.
#[must_use]
pub fn analyzer_status(model_dir: Option<&std::path::Path>) -> AnalyzerStatus {
    let paths = model_dir.map(OcrModelPaths::from_dir);

    AnalyzerStatus {
        ocr_available: paths.as_ref().is_some_and(ocr_available),
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
    pub default_currency: CurrencyCode,
    /// Language of the suggested description and merchant.
    pub locale: Locale,
}

/// Analyzes the bytes of one file into a draft suggestion, fully offline.
///
/// The kind of `document` decides how it is read; a file of no kind the
/// vault stores is not read. `model_dir` is where the OCR models are, and
/// without it images and scanned PDFs are not read.
///
/// The suggested description and merchant are written in `context.locale`;
/// text taken from the document itself stays as the document has it.
///
/// There is no error to return: a file that cannot be read, decoded or
/// parsed yields an empty suggestion whose note says why.
#[must_use]
pub fn analyze_document_bytes(
    document: &NewDocument<'_>,
    context: &AnalyzeContext<'_>,
    model_dir: Option<&std::path::Path>,
) -> DocumentSuggestion {
    let AnalyzeContext {
        default_currency,
        locale,
        ..
    } = *context;

    let mut suggestion = match read_document_text(document, model_dir) {
        ExtractedText::Read { text, origin } => {
            let reading = read_invoice_text(&text);
            let kind = reading.class.kind();
            let accounts = suggest_accounts(context, kind, &reading.category_hint());
            let fee_minor = reading.transfer_fee_minor;

            let mut suggestion = suggestion_from_reading(reading, origin, locale, accounts);
            suggestion.notes.insert(0, UiText::new(origin.note()));
            if let Some(fee_minor) = fee_minor {
                suggestion
                    .notes
                    .push(transfer_fee_note(fee_minor, default_currency));
            }
            suggestion
        }
        ExtractedText::Unread(reason) => {
            let accounts = suggest_accounts(context, EntryKindSuggestion::Expense, "");

            unread_suggestion(UiText::new(reason), accounts)
        }
    };

    let unpaid_bill = suggestion.kind == EntryKindSuggestion::Bill && suggestion.bill_unpaid;
    if unpaid_bill && lacks_payable_account(context, suggestion.payable_account_id) {
        suggestion
            .notes
            .push(UiText::new(UiTextCode::AddPayableAccount));
    }

    // The invoice reader emits 2-exponent minor units (cents). For currencies
    // with a different exponent the value would be silently wrong, so drop it.
    let two_decimals = currency_minor_exponent(default_currency) == 2;
    if !two_decimals && suggestion.amount_minor.is_some() {
        suggestion.amount_minor = None;
        suggestion.notes.push(
            UiText::new(UiTextCode::AmountAssumesTwoDecimals)
                .with_param("currency", default_currency.as_str()),
        );
    }

    // Document dates (issue or due date) often fall outside the current month;
    // say so, or the entry seems to vanish from the dashboard after posting.
    if let Some(date) = suggestion.entry_date {
        let note = UiText::new(UiTextCode::DatedFromDocument).with_param("date", format_date(date));
        suggestion.notes.push(note);
    }

    suggestion
}

/// The note for a transfer fee the receipt shows.
///
/// The reader reads the fee as cents. In a 2-decimal book it is sent as a
/// figure with the book's currency, which the UI formats. In any other book
/// the figure would be wrong, so the note states the fee exists without one.
fn transfer_fee_note(fee_minor: i64, currency: CurrencyCode) -> UiText {
    if currency_minor_exponent(currency) == 2 {
        UiText::new(UiTextCode::TransferFee)
            .with_param("fee_minor", fee_minor.to_string())
            .with_param("currency", currency.as_str())
    } else {
        UiText::new(UiTextCode::TransferFeeUnstated)
    }
}

/// The accounts of the book that a suggestion points at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SuggestedAccounts {
    /// The expense or income account the document is filed under.
    category: Option<AccountId>,
    /// The bank, cash or card account that pays or receives.
    wallet: Option<AccountId>,
    /// The account an unpaid bill is owed on.
    payable: Option<AccountId>,
}

/// Chooses the accounts for a document of `kind` in the book of `context`.
///
/// The category comes from the keyword matcher on `category_hint`, by entry
/// kind. The wallet and the payable account are the book's defaults for
/// those roles.
fn suggest_accounts(
    context: &AnalyzeContext<'_>,
    kind: EntryKindSuggestion,
    category_hint: &str,
) -> SuggestedAccounts {
    let AnalyzeContext {
        template, accounts, ..
    } = *context;

    let category = match kind {
        EntryKindSuggestion::Income => match_income_account(template, accounts, category_hint),
        EntryKindSuggestion::Expense | EntryKindSuggestion::Bill => {
            match_expense_account(template, accounts, category_hint)
        }
    };

    SuggestedAccounts {
        category,
        wallet: default_account_for_role(template, accounts, AccountRole::Payment),
        payable: default_account_for_role(template, accounts, AccountRole::BillsPayable),
    }
}

/// Whether the user still has to add a payable account, given the one that
/// was suggested.
///
/// A seeded book has a real payable account. When identity cannot find it
/// (deactivated or re-coded), any liability suggested is only a stand-in,
/// and the user must still be told to add a payable account. A blank book
/// seeds none, so there a liability is the legitimate answer.
fn lacks_payable_account(context: &AnalyzeContext<'_>, payable: Option<AccountId>) -> bool {
    let AnalyzeContext {
        template, accounts, ..
    } = *context;

    payable.is_none()
        || (template != ChartTemplate::Blank
            && seeded_account_for_role(template, accounts, AccountRole::BillsPayable).is_none())
}

/// Reads extracted document text into a draft suggestion, worded in
/// `locale`.
///
/// The accounts of the suggestion are left empty: no book is given. The
/// source and model are those of a document's own text.
///
/// A bank transfer receipt stays [`EntryKindSuggestion::Expense`]. Its
/// amount is the capital debit (`Ποσό Χρέωσης Κεφαλαίου` or `Ποσό:`), not the
/// fee and not an `hh:mm` time.
///
/// The returned notes are the reader's own. They never mention a transfer
/// fee, even when the receipt shows one: that note needs the book's
/// currency, so [`analyze_document_bytes`] adds it.
#[must_use]
pub fn parse_invoice_text(text: &str, locale: Locale) -> DocumentSuggestion {
    suggestion_from_reading(
        read_invoice_text(text),
        TextOrigin::DocumentText,
        locale,
        SuggestedAccounts::default(),
    )
}

/// The suggestion for a reading: its fields worded in `locale`, with where
/// the text came from and the accounts chosen for it.
///
/// The notes are the reader's own, in its order.
fn suggestion_from_reading(
    reading: InvoiceReading,
    origin: TextOrigin,
    locale: Locale,
    accounts: SuggestedAccounts,
) -> DocumentSuggestion {
    let description = reading.description_in(locale);
    let merchant = reading.merchant_in(locale).map(str::to_owned);
    let confidence = reading.confidence();

    DocumentSuggestion {
        source: origin.source(),
        model: Some(origin.model_label().to_owned()),
        kind: reading.class.kind(),
        amount_minor: reading.amount_minor,
        entry_date: reading.entry_date,
        description,
        reference: reading.reference,
        merchant,
        bill_unpaid: reading.class.is_unpaid(),
        category_account_id: accounts.category,
        wallet_account_id: accounts.wallet,
        payable_account_id: accounts.payable,
        confidence,
        notes: reading.notes,
    }
}

/// The model label of a suggestion read from a document's own text by the
/// invoice reader.
const READER_MODEL_LABEL: &str = "invoice-parser-v1";

/// The model label of a suggestion whose text came from the bundled OCR.
const OCR_MODEL_LABEL: &str = "ocrs-bundled";

/// Where the text of a document came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextOrigin {
    /// The file's own text: a plain text file or the text layer of a PDF.
    DocumentText,
    /// OCR of an image file.
    ImageOcr,
    /// OCR of an image embedded in a PDF.
    PdfImageOcr,
}

impl TextOrigin {
    /// What the suggestion reports as its source: the bundled OCR, or the
    /// document's own text.
    const fn source(self) -> AnalyzeSource {
        match self {
            Self::DocumentText => AnalyzeSource::Heuristic,
            Self::ImageOcr | Self::PdfImageOcr => AnalyzeSource::BundledOcr,
        }
    }

    /// The label the suggestion reports as its model: the invoice reader's
    /// for a document's own text, the OCR's when OCR produced the text.
    const fn model_label(self) -> &'static str {
        match self {
            Self::DocumentText => READER_MODEL_LABEL,
            Self::ImageOcr | Self::PdfImageOcr => OCR_MODEL_LABEL,
        }
    }

    /// The note that says where the text came from, shown before the reader's.
    const fn note(self) -> UiTextCode {
        match self {
            Self::DocumentText => UiTextCode::ParsedFromDocumentText,
            Self::ImageOcr => UiTextCode::OcrRead,
            Self::PdfImageOcr => UiTextCode::OcrPdfImage,
        }
    }
}

/// The text read from a file, or the reason there is none.
#[derive(Debug, PartialEq, Eq)]
enum ExtractedText {
    /// Text, and where it came from. The text is not empty for OCR and for a
    /// PDF; a plain text file is passed on even when it is empty.
    Read { text: String, origin: TextOrigin },
    /// No text. The code is the note that tells the user why.
    Unread(UiTextCode),
}

/// Extracts the text of a file, choosing how by its kind.
///
/// An image is read with OCR, plain text is decoded as UTF-8 with invalid
/// bytes replaced, and a PDF is read as one. A file of no kind is unread,
/// with the note that no text was found.
fn read_document_text(document: &NewDocument<'_>, model_dir: Option<&Path>) -> ExtractedText {
    let NewDocument {
        filename,
        mime_type,
        data,
    } = *document;

    match DocumentKind::resolve(mime_type, filename, data) {
        Some(DocumentKind::Png | DocumentKind::Jpeg | DocumentKind::Webp) => {
            ocr_image(data, model_dir).into_extracted(TextOrigin::ImageOcr)
        }
        Some(DocumentKind::PlainText) => ExtractedText::Read {
            text: String::from_utf8_lossy(data).into_owned(),
            origin: TextOrigin::DocumentText,
        },
        Some(DocumentKind::Pdf) => read_pdf_text(data, model_dir),
        None => ExtractedText::Unread(UiTextCode::NoTextExtracted),
    }
}

/// How reading one image with the bundled OCR went.
#[derive(Debug, PartialEq, Eq)]
enum OcrOutcome {
    /// OCR found text.
    Read(String),
    /// OCR ran and found nothing to speak of.
    LittleText,
    /// No model directory is configured.
    PathMissing,
    /// The model files are not in the directory.
    ModelsMissing,
    /// The image could not be decoded or the engine failed. The cause is
    /// logged where it happened.
    Failed,
}

impl OcrOutcome {
    /// The outcome as the text of a document read from `origin`.
    fn into_extracted(self, origin: TextOrigin) -> ExtractedText {
        match self {
            Self::Read(text) => ExtractedText::Read { text, origin },
            Self::LittleText => ExtractedText::Unread(UiTextCode::OcrLittleText),
            Self::PathMissing => ExtractedText::Unread(UiTextCode::OcrPathMissing),
            Self::ModelsMissing => ExtractedText::Unread(UiTextCode::OcrModelsMissing),
            Self::Failed => ExtractedText::Unread(UiTextCode::OcrFailed),
        }
    }
}

/// Reads one image with the bundled OCR and says how it went.
///
/// A reading that is empty or only whitespace is
/// [`OcrOutcome::LittleText`]. An error is logged here with its cause and
/// returned as [`OcrOutcome::Failed`] without it.
fn ocr_image(data: &[u8], model_dir: Option<&Path>) -> OcrOutcome {
    let Some(dir) = model_dir else {
        return OcrOutcome::PathMissing;
    };
    let paths = OcrModelPaths::from_dir(dir);
    if !ocr_available(&paths) {
        return OcrOutcome::ModelsMissing;
    }

    match ocr_image_bytes(&paths, data) {
        Ok(text) if !text.trim().is_empty() => OcrOutcome::Read(text),
        Ok(_) => OcrOutcome::LittleText,
        Err(err) => {
            // The cause can carry file or model detail; it belongs in the log,
            // not on the wire, and the user is told only that OCR failed.
            log::warn!("OCR failed on an image: {err}");
            OcrOutcome::Failed
        }
    }
}

/// The suggestion for a file no text was read from: no field, `note` as the
/// only note, and the accounts an expense with no hint gets.
///
/// The kind is `Expense` and the confidence zero.
fn unread_suggestion(note: UiText, accounts: SuggestedAccounts) -> DocumentSuggestion {
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
        category_account_id: accounts.category,
        wallet_account_id: accounts.wallet,
        payable_account_id: accounts.payable,
        confidence: 0.0,
        notes: vec![note],
    }
}

/// Fewest characters, once trimmed, of text that counts as the text of a
/// PDF. One rule, applied by [`is_enough_pdf_text`] in two places:
///
/// - a text layer with fewer is not a text layer: the PDF is taken to be a
///   scan and its embedded images are read with OCR
///   ([`should_ocr_pdf_images`]);
/// - an OCR reading of an embedded image with fewer is not a reading, and
///   the next image is tried ([`pdf_image_outcome`]).
///
/// The unit tests `short_or_missing_pdf_text_triggers_image_ocr` and
/// `an_image_reading_counts_from_the_same_length_as_a_text_layer` pin the
/// boundary on both sides. The reason for 8 in particular is not recorded.
const MIN_PDF_TEXT_CHARS: usize = 8;

/// Whether `text` has at least [`MIN_PDF_TEXT_CHARS`] characters, not
/// counting whitespace at its ends.
fn is_enough_pdf_text(text: &str) -> bool {
    text.trim().chars().count() >= MIN_PDF_TEXT_CHARS
}

/// Most embedded JPEG images of one PDF that are tried with OCR. Each try
/// runs the OCR models once, so this bounds how long a scanned PDF takes.
///
/// The reason for 2 in particular is not recorded;
/// `at_most_two_page_images_are_taken` pins it.
const MAX_PDF_OCR_IMAGES: usize = 2;

/// Whether a PDF with this text layer is taken as a scan: no text, or not
/// enough of it ([`is_enough_pdf_text`]).
fn should_ocr_pdf_images(text: Option<&str>) -> bool {
    !text.is_some_and(is_enough_pdf_text)
}

/// Reads a PDF: its text layer, or failing that its embedded images.
fn read_pdf_text(data: &[u8], model_dir: Option<&Path>) -> ExtractedText {
    let Ok(pdf) = parse_pdf(data) else {
        return ExtractedText::Unread(UiTextCode::PdfOverBudget);
    };

    let text = pdf
        .text
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());

    // A PDF with a text layer is not a scan: its images are left alone.
    let jpegs = match &pdf.document {
        Some(document) if should_ocr_pdf_images(text.as_deref()) => {
            extract_pdf_jpeg_images(document)
        }
        _ => Vec::new(),
    };
    pdf_text_or_image_text(text, &jpegs, model_dir)
}

/// Chooses between the text layer of a PDF and the OCR of its images.
///
/// Text read from an image wins. Otherwise whatever text layer there is
/// stands, however short. With neither, the reason OCR gave nothing is
/// reported the way it is for an image file; a PDF with no image to read
/// simply has no text.
fn pdf_text_or_image_text(
    text: Option<String>,
    jpegs: &[Vec<u8>],
    model_dir: Option<&Path>,
) -> ExtractedText {
    match (ocr_pdf_images(jpegs, model_dir), text) {
        (Some(OcrOutcome::Read(text)), _) => ExtractedText::Read {
            text,
            origin: TextOrigin::PdfImageOcr,
        },
        (_, Some(text)) => ExtractedText::Read {
            text,
            origin: TextOrigin::DocumentText,
        },
        (Some(failure), None) => failure.into_extracted(TextOrigin::PdfImageOcr),
        (None, None) => ExtractedText::Unread(UiTextCode::NoTextExtracted),
    }
}

/// Reads the embedded images of a scanned PDF in turn, up to
/// [`MAX_PDF_OCR_IMAGES`] of them, until one has at least
/// [`MIN_PDF_TEXT_CHARS`] characters of text.
///
/// Returns `None` for a PDF without images. When no image yields text, the
/// outcome of the first one is returned.
fn ocr_pdf_images(jpegs: &[Vec<u8>], model_dir: Option<&Path>) -> Option<OcrOutcome> {
    let mut first_failure = None;

    for jpeg in jpegs.iter().take(MAX_PDF_OCR_IMAGES) {
        match pdf_image_outcome(ocr_image(jpeg, model_dir)) {
            OcrOutcome::Read(text) => return Some(OcrOutcome::Read(text)),
            failure => first_failure.get_or_insert(failure),
        };
    }
    first_failure
}

/// The outcome of reading one embedded image, with a reading that is not
/// enough text ([`is_enough_pdf_text`]) turned into
/// [`OcrOutcome::LittleText`].
fn pdf_image_outcome(outcome: OcrOutcome) -> OcrOutcome {
    match outcome {
        OcrOutcome::Read(text) if is_enough_pdf_text(&text) => OcrOutcome::Read(text),
        OcrOutcome::Read(_) => OcrOutcome::LittleText,
        other => other,
    }
}

/// A PDF after loading: the document if lopdf could parse it, and its text
/// layer if it has one.
struct ParsedPdf {
    /// The parsed file, or `None` when lopdf could not read it even after
    /// repair.
    document: Option<Box<BudgetedPdf>>,
    /// The text pdf-extract produced, untrimmed, or `None` when it produced
    /// none for any page.
    text: Option<String>,
}

/// The PDF is over the budget; nothing of it is read.
struct PdfOverBudget;

/// Loads a PDF and extracts its text, hardened for real-world statements
/// and invoices.
///
/// Two failure modes show up in the wild, especially with bank statements:
/// stale xref offsets left behind by stamping/signing tools (lopdf refuses
/// to load the file or reads the wrong objects), and malformed font or
/// resource objects that make pdf-extract panic mid-page. A file that yields
/// no text is repaired and loaded once more for the former; panics are
/// contained and pages read one by one for the latter.
fn parse_pdf(data: &[u8]) -> std::result::Result<ParsedPdf, PdfOverBudget> {
    let mut document = match load_pdf(data) {
        PdfLoad::Loaded(document) => Some(document),
        PdfLoad::OverBudget => return Err(PdfOverBudget),
        PdfLoad::Unreadable => None,
    };
    let mut text = document.as_deref().and_then(pdf_text);

    if text.is_none()
        && let Some(repaired) = repair_xref_offsets(data)
    {
        match load_pdf(&repaired) {
            PdfLoad::Loaded(repaired_document) => {
                text = pdf_text(&repaired_document);
                document = Some(repaired_document);
            }
            PdfLoad::OverBudget => return Err(PdfOverBudget),
            PdfLoad::Unreadable => {}
        }
    }

    Ok(ParsedPdf { document, text })
}

/// The text of every page, or of the pages that can be read when one page
/// makes the whole-document pass fail.
fn pdf_text(pdf: &BudgetedPdf) -> Option<String> {
    pdf_text_whole(pdf).or_else(|| pdf_text_per_page(pdf))
}

/// Whole-document pass: the text of every page in one call to pdf-extract.
///
/// `None` when pdf-extract returns an error or panics. `Some` of an empty
/// string when it succeeds on a document without text.
fn pdf_text_whole(pdf: &BudgetedPdf) -> Option<String> {
    contain_panics(|| {
        let mut text = String::new();
        let mut output = pdf_extract::PlainTextOutput::new(&mut text);

        pdf_extract::output_doc(pdf.document(), &mut output)
            .ok()
            .map(|()| text)
    })
    .flatten()
}

/// Page-by-page pass: pages whose resources make pdf-extract error or panic
/// are skipped, and the text of the others is joined with newlines, in page
/// order.
///
/// `None` when the pages cannot be listed or no page can be read.
fn pdf_text_per_page(pdf: &BudgetedPdf) -> Option<String> {
    let page_numbers =
        contain_panics(|| pdf.document().get_pages().into_keys().collect::<Vec<u32>>())?;

    let chunks: Vec<String> = page_numbers
        .into_iter()
        .filter_map(|page| {
            contain_panics(|| {
                let mut text = String::new();
                let mut output = pdf_extract::PlainTextOutput::new(&mut text);

                pdf_extract::output_doc_page(pdf.document(), &mut output, page)
                    .ok()
                    .map(|()| text)
            })
            .flatten()
        })
        .collect();

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n"))
    }
}

/// The first JPEG images of a PDF, at most [`MAX_PDF_OCR_IMAGES`].
///
/// Images that a page names in its `/Resources /XObject` dictionary are
/// taken first, in page order, so a logo that only the file's object table
/// holds does not come before the scan. When no page names a JPEG, every
/// object of the file is searched. Returns no image when lopdf panics.
fn extract_pdf_jpeg_images(pdf: &BudgetedPdf) -> Vec<Vec<u8>> {
    contain_panics(|| {
        let document = pdf.document();
        let mut jpegs = Vec::new();

        for page_id in document.get_pages().into_values() {
            collect_jpegs_from_page(document, page_id, &mut jpegs);
            if jpegs.len() >= MAX_PDF_OCR_IMAGES {
                return jpegs;
            }
        }
        if !jpegs.is_empty() {
            return jpegs;
        }

        document
            .objects
            .values()
            .filter_map(|object| jpeg_from_object(document, object))
            .take(MAX_PDF_OCR_IMAGES)
            .collect()
    })
    .unwrap_or_default()
}

/// Appends the JPEG images that the page's `/Resources /XObject` dictionary
/// names to `jpegs`, stopping at [`MAX_PDF_OCR_IMAGES`] in total.
///
/// A page without resources or without image objects adds nothing.
fn collect_jpegs_from_page(
    document: &lopdf::Document,
    page_id: lopdf::ObjectId,
    jpegs: &mut Vec<Vec<u8>>,
) {
    let Ok(page) = document.get_dictionary(page_id) else {
        return;
    };
    let Some(resources) = dictionary_inline_or_referenced(document, page.get(b"Resources").ok())
    else {
        return;
    };
    let Some(xobjects) = dictionary_inline_or_referenced(document, resources.get(b"XObject").ok())
    else {
        return;
    };
    for (_name, object) in xobjects {
        if let Some(jpeg) = jpeg_from_object(document, object) {
            jpegs.push(jpeg);
            if jpegs.len() >= MAX_PDF_OCR_IMAGES {
                return;
            }
        }
    }
}

/// The dictionary that `object` is, or that it refers to.
///
/// PDF writers store `/Resources` and `/XObject` either way. `None` when the
/// object is missing, is neither, or refers to something that is not a
/// dictionary.
fn dictionary_inline_or_referenced<'document>(
    document: &'document lopdf::Document,
    object: Option<&'document lopdf::Object>,
) -> Option<&'document lopdf::Dictionary> {
    match object? {
        lopdf::Object::Dictionary(dictionary) => Some(dictionary),
        lopdf::Object::Reference(id) => document.get_dictionary(*id).ok(),
        _ => None,
    }
}

/// The stored bytes of an image stream that is a JPEG file: an object, or a
/// reference to one, that is a stream with `/Subtype /Image` and `DCTDecode`
/// as its only filter.
///
/// The bytes are a slice of the uploaded file, so no larger than the upload
/// cap. `None` for anything else.
fn jpeg_from_object(document: &lopdf::Document, object: &lopdf::Object) -> Option<Vec<u8>> {
    let stream = match object {
        lopdf::Object::Stream(stream) => stream,
        lopdf::Object::Reference(id) => match document.objects.get(id) {
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
    let is_jpeg = matches!(stream.filters().ok()?.as_slice(), [b"DCTDecode"]);
    is_jpeg.then(|| stream.content.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_accounts::{code_of_for_tests, seeded_chart_for_tests};
    use crate::documents::MAX_DOCUMENT_BYTES;
    use crate::documents::pdf_budget::MAX_PDF_DECODED_BYTES;
    use crate::domain::AccountType;
    use oikonomia_test_support::listed_variants;

    /// The parsed form of a test PDF that must load within budget.
    #[expect(clippy::panic, reason = "test fails loudly by design")]
    fn loaded(bytes: &[u8]) -> Box<BudgetedPdf> {
        match load_pdf(bytes) {
            PdfLoad::Loaded(pdf) => pdf,
            PdfLoad::OverBudget => panic!("the test PDF is over budget"),
            PdfLoad::Unreadable => panic!("the test PDF does not parse"),
        }
    }

    #[test]
    fn a_file_that_is_not_a_pdf_is_unreadable_not_over_budget() {
        for data in [&b"not a pdf"[..], b"%PDF-1.4\ntrailer\n%%EOF"] {
            assert!(matches!(load_pdf(data), PdfLoad::Unreadable));
            assert_eq!(
                read_pdf_text(data, None),
                ExtractedText::Unread(UiTextCode::NoTextExtracted)
            );
        }
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

    /// The filter name of a JPEG image stream.
    fn dct() -> lopdf::Object {
        lopdf::Object::Name(b"DCTDecode".to_vec())
    }

    /// The filter name of a deflate-compressed stream.
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

            assert_eq!(
                extract_pdf_jpeg_images(&loaded(&pdf)),
                [b"jpeg-one".to_vec()]
            );
        }
    }

    #[test]
    fn a_filter_array_holding_only_the_jpeg_filter_counts() {
        let filters = lopdf::Object::Array(vec![dct()]);
        let pdf = pdf_with_images(ImagePlacement::PageXObject, &[(filters, b"jpeg-in-array")]);

        assert_eq!(
            extract_pdf_jpeg_images(&loaded(&pdf)),
            [b"jpeg-in-array".to_vec()]
        );
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

        assert_eq!(extract_pdf_jpeg_images(&loaded(&pdf)), [] as [Vec<u8>; 0]);
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

        assert_eq!(
            extract_pdf_jpeg_images(&loaded(&pdf)),
            [b"the-jpeg".to_vec()]
        );
    }

    #[test]
    fn at_most_two_page_images_are_taken() {
        let pdf = pdf_with_images(
            ImagePlacement::PageXObject,
            &[(dct(), b"a"), (dct(), b"b"), (dct(), b"c")],
        );

        assert_eq!(extract_pdf_jpeg_images(&loaded(&pdf)).len(), 2);
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

        let mut found = extract_pdf_jpeg_images(&loaded(&pdf));
        found.sort();

        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|jpeg| jpeg.len() == 1));
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
    fn a_pdf_with_only_an_image_has_no_text_layer() {
        let pdf = pdf_with_images(ImagePlacement::PageXObject, &[(dct(), b"jpeg")]);

        let text = pdf_text(&loaded(&pdf)).map(|text| text.trim().to_owned());

        assert_eq!(text, Some(String::new()));
    }

    #[test]
    fn a_pdf_without_images_has_nothing_to_read_with_ocr() {
        assert_eq!(ocr_pdf_images(&[], None), None);
    }

    #[test]
    fn pdf_images_are_not_read_without_a_model_directory() {
        let jpegs = [b"jpeg".to_vec(), b"jpeg".to_vec()];

        assert_eq!(ocr_pdf_images(&jpegs, None), Some(OcrOutcome::PathMissing));
    }

    /// A PDF with one page per entry of `pages`: `(text, readable)`. A
    /// readable page shows its text in a standard font; the other names a
    /// font that is not a font object, which makes pdf-extract fail on it.
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

        let text = pdf_text_per_page(&loaded(&pdf)).unwrap();

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
        assert_eq!(pdf_text_whole(&loaded(&pdf)), None);
        let text = pdf_text(&loaded(&pdf)).unwrap();

        assert!(text.contains("Readable"), "unexpected text: {text:?}");
        assert!(!text.contains("Broken"), "unexpected text: {text:?}");
    }

    #[test]
    fn a_pdf_with_no_readable_page_has_no_text() {
        let pdf = pdf_with_text_pages(&[("Broken", false)]);

        assert_eq!(pdf_text_per_page(&loaded(&pdf)), None);
        assert_eq!(pdf_text(&loaded(&pdf)), None);
    }

    #[test]
    fn page_by_page_extraction_reads_the_synthetic_invoice() {
        let pdf = include_bytes!("../../testdata/documents/synthetic/pdf/english_total.pdf");

        let text = pdf_text_per_page(&loaded(pdf)).unwrap();

        assert!(text.contains("45"), "unexpected text: {text:?}");
    }

    #[test]
    fn page_by_page_extraction_of_a_page_without_text_is_empty_not_missing() {
        let pdf = pdf_with_images(ImagePlacement::PageXObject, &[(dct(), b"jpeg")]);

        assert_eq!(
            pdf_text_per_page(&loaded(&pdf)).map(|text| text.trim().to_owned()),
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
    fn an_image_reading_counts_from_the_same_length_as_a_text_layer() {
        let read = |text: &str| pdf_image_outcome(OcrOutcome::Read(text.into()));

        assert_eq!(read("1234567"), OcrOutcome::LittleText);
        assert_eq!(read("12345678"), OcrOutcome::Read("12345678".into()));
        assert_eq!(read("  1234567 \n"), OcrOutcome::LittleText);
        // The two rules meet at one length: what is too short to be a text
        // layer is too short to be a reading, and the reverse.
        for text in ["", "1234567", "12345678", "123456789"] {
            assert_eq!(
                should_ocr_pdf_images(Some(text)),
                read(text) == OcrOutcome::LittleText,
                "{text:?}"
            );
        }
        // Outcomes that are not readings pass through.
        assert_eq!(pdf_image_outcome(OcrOutcome::Failed), OcrOutcome::Failed);
        assert_eq!(
            pdf_image_outcome(OcrOutcome::ModelsMissing),
            OcrOutcome::ModelsMissing
        );
    }

    /// The notes of an analysis.
    fn notes_of(suggestion: DocumentSuggestion) -> Vec<UiText> {
        suggestion.notes
    }

    /// The codes of `notes`, without their parameters.
    fn codes_of(notes: &[UiText]) -> Vec<UiTextCode> {
        notes.iter().map(|note| note.code).collect()
    }

    #[test]
    fn non_two_exponent_currency_drops_amount() {
        let text = b"Invoice\nTOTAL 45,90\nThank you";
        let eur = analyze_document_bytes(
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: text,
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            None,
        );
        let jpy = analyze_document_bytes(
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: text,
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "JPY".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            None,
        );

        assert_eq!(eur.amount_minor, Some(4590));
        let eur_notes = notes_of(eur);
        assert!(
            !codes_of(&eur_notes).contains(&UiTextCode::AmountAssumesTwoDecimals),
            "a 2-decimal currency needs no warning"
        );

        assert_eq!(jpy.amount_minor, None, "JPY amount must not be prefilled");

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
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: b"Invoice\nTOTAL 45,90\nThank you",
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: currency.parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            None,
        );

        suggestion.amount_minor == Some(4590)
    }

    #[test]
    fn the_shared_exponent_table_knows_the_currencies_the_analyzer_dropped() {
        use crate::csv::currency_minor_exponent;

        assert_eq!(currency_minor_exponent("ISK".parse().unwrap()), 0);
        assert_eq!(currency_minor_exponent("IQD".parse().unwrap()), 0);
        assert_eq!(currency_minor_exponent("LYD".parse().unwrap()), 3);
    }

    #[test]
    fn the_amount_is_kept_exactly_for_the_currencies_the_csv_table_gives_two_decimals() {
        let letters = || 'A'..='Z';
        let mut without_two_decimals = Vec::new();

        for code in letters()
            .flat_map(|a| letters().flat_map(move |b| letters().map(move |c| [a, b, c])))
            .map(String::from_iter)
        {
            let two_decimals = crate::csv::currency_minor_exponent(code.parse().unwrap()) == 2;

            assert_eq!(keeps_the_amount_in(&code), two_decimals, "{code}");
            if !two_decimals {
                without_two_decimals.push(code);
            }
        }

        // The exact list is pinned next to the table in `csv::amount`.
        assert_eq!(without_two_decimals.len(), 51);
        assert!(without_two_decimals.iter().any(|code| code == "IQD"));
        assert!(!keeps_the_amount_in("isk"), "codes are case-insensitive");
    }

    #[test]
    fn a_text_file_gets_the_source_note_then_the_reader_notes_then_the_date_note() {
        let text = b"Invoice\nDate 15/03/2026\nTOTAL 45,90 EUR\nThank you";
        let notes = notes_of(analyze_document_bytes(
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: text,
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
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
            &NewDocument {
                filename: "scan.bin",
                mime_type: "application/octet-stream",
                data: b"\x00\x01",
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            None,
        ));

        assert_eq!(notes, [UiText::new(UiTextCode::NoTextExtracted)]);
    }

    /// Where the text of a file with these three properties comes from, or
    /// the note that says why it has none.
    fn text_origin(
        mime_type: &str,
        filename: &str,
        data: &[u8],
    ) -> std::result::Result<TextOrigin, UiTextCode> {
        let document = NewDocument {
            filename,
            mime_type,
            data,
        };

        match read_document_text(&document, None) {
            ExtractedText::Read { origin, .. } => Ok(origin),
            ExtractedText::Unread(reason) => Err(reason),
        }
    }

    #[test]
    fn a_pdf_sent_under_the_x_pdf_type_is_read_as_a_pdf() {
        let pdf = pdf_with_text_pages(&[("Paid in full", true)]);

        assert_eq!(
            text_origin("application/x-pdf", "document", &pdf),
            Ok(TextOrigin::DocumentText)
        );
        // Not a PDF inside: read as one all the same, and found unreadable.
        assert_eq!(
            text_origin("application/x-pdf", "document", b"TOTAL 45,90"),
            Err(UiTextCode::NoTextExtracted)
        );
    }

    #[test]
    fn a_pdf_named_as_a_text_file_is_read_as_a_pdf_and_not_as_its_raw_bytes() {
        let pdf = pdf_with_text_pages(&[("Paid in full", true)]);
        let document = NewDocument {
            filename: "x.txt",
            mime_type: "application/pdf",
            data: &pdf,
        };

        let read = read_document_text(&document, None);

        assert!(
            matches!(&read, ExtractedText::Read { text, .. } if text.contains("Paid in full")),
            "{read:?}"
        );
        assert!(
            matches!(&read, ExtractedText::Read { text, .. } if !text.contains("%PDF")),
            "the PDF source must not be read as text: {read:?}"
        );
    }

    #[test]
    fn text_sent_under_the_pdf_type_and_named_as_text_is_read_as_text() {
        let document = NewDocument {
            filename: "x.txt",
            mime_type: "application/pdf",
            data: b"TOTAL 45,90",
        };

        assert_eq!(
            read_document_text(&document, None),
            ExtractedText::Read {
                text: "TOTAL 45,90".into(),
                origin: TextOrigin::DocumentText
            }
        );
    }

    #[test]
    fn an_image_type_the_vault_does_not_store_is_not_read() {
        assert_eq!(
            text_origin("image/gif", "anim.gif", b"GIF89a"),
            Err(UiTextCode::NoTextExtracted)
        );
        assert_eq!(
            text_origin("image/jpeg", "scan.pdf", b"\xff\xd8"),
            Err(UiTextCode::OcrPathMissing),
            "an image named like a PDF goes to OCR"
        );
    }

    #[test]
    fn an_image_without_a_model_directory_says_the_path_is_missing() {
        let notes = notes_of(analyze_document_bytes(
            &NewDocument {
                filename: "scan.jpg",
                mime_type: "image/jpeg",
                data: b"\xff\xd8",
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
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
            &NewDocument {
                filename: "scan.jpg",
                mime_type: "image/jpeg",
                data: b"\xff\xd8",
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            path.as_deref().ok(),
        ));

        assert_eq!(notes, [UiText::new(UiTextCode::OcrModelsMissing)]);
    }

    /// The fixtures make pdf-extract recurse until the stack overflows,
    /// which aborts the process; the budget refuses both before extraction.
    #[test]
    fn a_pdf_that_nests_without_end_is_over_budget_and_never_extracted() {
        for pdf in [
            &include_bytes!("../../testdata/hostile/xobject_self_loop.pdf")[..],
            include_bytes!("../../testdata/hostile/page_parent_loop.pdf"),
        ] {
            assert!(matches!(load_pdf(pdf), PdfLoad::OverBudget));
            assert_eq!(
                read_pdf_text(pdf, None),
                ExtractedText::Unread(UiTextCode::PdfOverBudget)
            );
        }
    }

    /// Stack of the thread the desktop app extracts on. It runs analysis in
    /// `tauri::async_runtime::spawn_blocking`, and Tauri's Tokio runtime
    /// keeps Tokio's default stack for its blocking threads.
    const EXTRACTION_THREAD_STACK: usize = 2 * 1024 * 1024;

    #[test]
    fn forms_nested_to_the_depth_limit_read_on_a_quarter_of_the_extraction_stack() {
        use crate::documents::pdf_nesting::MAX_FORM_DEPTH;
        use crate::documents::pdf_nesting::tests::{LEAF_TEXT, chain, pdf_with_forms};

        // The deepest nesting the budget lets through, run through the whole
        // read path on a thread with a quarter of the app's stack. In a
        // debug build, whose frames are larger than release ones. A stack
        // overflow here aborts the test run rather than failing one test.
        let pdf = pdf_with_forms(&[0], &chain(MAX_FORM_DEPTH));
        assert!(matches!(load_pdf(&pdf), PdfLoad::Loaded(_)));

        let read = std::thread::Builder::new()
            .stack_size(EXTRACTION_THREAD_STACK / 4)
            .spawn(move || read_pdf_text(&pdf, None))
            .expect("spawn the extraction thread")
            .join()
            .expect("extraction does not panic");

        assert!(
            matches!(&read, ExtractedText::Read { text, .. } if text.contains(LEAF_TEXT)),
            "{read:?}"
        );
    }

    #[test]
    fn an_oversized_pdf_says_it_is_over_budget() {
        // Larger than any stored document: refused before it is parsed.
        let data = vec![0_u8; MAX_DOCUMENT_BYTES + 1];
        let notes = notes_of(analyze_document_bytes(
            &NewDocument {
                filename: "big.pdf",
                mime_type: "application/pdf",
                data: &data,
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            None,
        ));

        assert_eq!(notes, [UiText::new(UiTextCode::PdfOverBudget)]);
    }

    /// A one-page PDF whose page content is `len` spaces, Flate-compressed.
    fn pdf_with_inflating_content(len: usize) -> Vec<u8> {
        use lopdf::{Document, Object, Stream, dictionary};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();

        let mut content = Stream::new(lopdf::Dictionary::new(), vec![b' '; len]);
        content.compress().expect("compress page content");
        let contents = doc.add_object(content);

        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => contents,
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

    /// The notes of analyzing `pdf` with no model directory.
    fn analyze_pdf_notes(pdf: &[u8]) -> Vec<UiText> {
        analyze_pdf_notes_with_models(pdf, None)
    }

    /// The notes of analyzing `pdf` in a blank EUR book, in English.
    fn analyze_pdf_notes_with_models(pdf: &[u8], model_dir: Option<&Path>) -> Vec<UiText> {
        notes_of(analyze_document_bytes(
            &NewDocument {
                filename: "document.pdf",
                mime_type: "application/pdf",
                data: pdf,
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            model_dir,
        ))
    }

    #[test]
    fn a_scanned_pdf_without_a_model_directory_says_the_path_is_missing() {
        let scan = pdf_with_images(ImagePlacement::PageXObject, &[(dct(), b"jpeg")]);

        assert_eq!(
            analyze_pdf_notes(&scan),
            [UiText::new(UiTextCode::OcrPathMissing)]
        );
    }

    #[test]
    fn a_scanned_pdf_with_an_empty_model_directory_says_the_models_are_missing() {
        let scan = pdf_with_images(ImagePlacement::PageXObject, &[(dct(), b"jpeg")]);
        let dir = tempfile::tempdir();
        let dir = dir.expect("the temporary directory must be created");

        assert_eq!(
            analyze_pdf_notes_with_models(&scan, Some(dir.path())),
            [UiText::new(UiTextCode::OcrModelsMissing)]
        );
    }

    #[test]
    fn a_pdf_with_neither_text_nor_images_says_no_text_was_found() {
        let empty = pdf_with_images(ImagePlacement::PageXObject, &[]);

        assert_eq!(
            analyze_pdf_notes(&empty),
            [UiText::new(UiTextCode::NoTextExtracted)]
        );
    }

    #[test]
    fn the_text_layer_of_a_pdf_is_kept_when_its_images_cannot_be_read() {
        let pdf = pdf_with_text_pages(&[("Paid", true)]);
        let jpegs = [b"jpeg".to_vec()];

        assert_eq!(
            pdf_text_or_image_text(Some("Paid".into()), &jpegs, None),
            ExtractedText::Read {
                text: "Paid".into(),
                origin: TextOrigin::DocumentText
            }
        );
        assert_eq!(
            codes_of(&analyze_pdf_notes(&pdf)).first(),
            Some(&UiTextCode::ParsedFromDocumentText)
        );
    }

    #[test]
    fn a_small_pdf_that_inflates_past_the_budget_says_it_is_over_budget() {
        let pdf = pdf_with_inflating_content(MAX_PDF_DECODED_BYTES + 1);

        assert!(pdf.len() < 1024 * 1024, "the file is {} bytes", pdf.len());
        assert_eq!(
            analyze_pdf_notes(&pdf),
            [UiText::new(UiTextCode::PdfOverBudget)]
        );
    }

    #[test]
    fn a_pdf_that_inflates_to_less_than_the_budget_is_read() {
        let pdf = pdf_with_inflating_content(1024 * 1024);

        assert!(matches!(load_pdf(&pdf), PdfLoad::Loaded(_)));
        assert_eq!(
            analyze_pdf_notes(&pdf),
            [UiText::new(UiTextCode::NoTextExtracted)],
            "a page of spaces is within budget and has no text"
        );
    }

    #[test]
    fn an_unpaid_bill_without_a_payable_account_asks_for_one() {
        let text = "Invoice\nTOTAL 45,90 EUR\nAmount due\nThank you";
        let notes = notes_of(analyze_document_bytes(
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: text.as_bytes(),
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "EUR".parse().unwrap(),
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
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: text.as_bytes(),
            },
            &AnalyzeContext {
                template: ChartTemplate::Personal,
                accounts: &accounts,
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            None,
        );

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

    /// A bill with a total and the words that mark it unpaid.
    const UNPAID_BILL: &str = "Invoice\nTOTAL 45,90 EUR\nAmount due\nThank you";

    /// The suggestion for [`UNPAID_BILL`] in a book with these accounts.
    fn analyze_unpaid_bill(template: ChartTemplate, accounts: &[Account]) -> DocumentSuggestion {
        analyze_document_bytes(
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: UNPAID_BILL.as_bytes(),
            },
            &AnalyzeContext {
                template,
                accounts,
                default_currency: "EUR".parse().unwrap(),
                locale: crate::prefs::Locale::En,
            },
            None,
        )
    }

    /// Archives the account with this chart code.
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

    /// The notes for [`TRANSFER_RECEIPT`] in a book of `currency`.
    fn analyze_receipt_in(currency: &str) -> Vec<UiText> {
        notes_of(analyze_document_bytes(
            &NewDocument {
                filename: "embasma.txt",
                mime_type: "text/plain",
                data: TRANSFER_RECEIPT.as_bytes(),
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: currency.parse().unwrap(),
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
            &NewDocument {
                filename: "bill.txt",
                mime_type: "text/plain",
                data: b"Invoice\nTOTAL 45,90\nThank you",
            },
            &AnalyzeContext {
                template: ChartTemplate::Blank,
                accounts: &[],
                default_currency: "JPY".parse().unwrap(),
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
        assert_eq!(analyzer_status(None).hint(), AnalyzerHint::ModelsMissing);

        let dir = tempfile::tempdir();
        let dir = dir.expect("the temporary directory must be created");
        assert_eq!(
            analyzer_status(Some(dir.path())).hint(),
            AnalyzerHint::ModelsMissing
        );

        for file in ["text-detection.rten", "text-recognition.rten"] {
            assert!(std::fs::write(dir.path().join(file), b"x").is_ok());
        }
        assert_eq!(
            analyzer_status(Some(dir.path())).hint(),
            AnalyzerHint::Ready
        );
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
