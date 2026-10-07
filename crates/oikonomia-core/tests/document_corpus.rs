//! Offline document golden harness.
//!
//! Loads `testdata/documents/MANIFEST.toml`, runs the invoice reader (and the
//! public analyze path for PDF / JPEG), and compares locked fields to golden
//! JSON. No network. Missing `private/` files are skipped; the OCR models
//! are checked in, so the two OCR tests fail when they are missing.

#![expect(clippy::expect_used, reason = "corpus tests fail loudly by design")]

use std::fs;
use std::path::{Path, PathBuf};

use oikonomia_core::documents::{
    AnalyzeContext, DocumentSuggestion, EntryKindSuggestion, NewDocument, OcrModelPaths,
    analyze_document_bytes, analyzer_status, parse_invoice_text,
};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::prefs::Locale;
use oikonomia_core::ui_text::UiTextCode;
use oikonomia_core::util::format_date;
use serde::{Deserialize, Serialize};

const CORPUS_REL: &str = "testdata/documents";

/// Slim golden: a field is asserted only when present.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Golden {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount_minor: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entry_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<EntryKindSuggestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    merchant: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    merchant_aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bill_unpaid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

#[derive(Debug, Clone)]
struct ManifestEntry {
    id: String,
    path: String,
    golden: String,
    sector: String,
    brand: String,
    split: String,
    rights: String,
    parser: String,
    mime: Option<String>,
}

struct Manifest {
    schema_version: u32,
    documents: Vec<ManifestEntry>,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS_REL)
}

fn load_manifest() -> Manifest {
    let path = corpus_root().join("MANIFEST.toml");
    let src = fs::read_to_string(&path).expect("MANIFEST.toml");
    parse_manifest(&src)
}

/// Minimal TOML subset: `schema_version` plus `[[documents]]` string/int keys.
///
/// Avoids a new crate dependency. The on-disk format is still TOML.
fn parse_manifest(src: &str) -> Manifest {
    let mut schema_version = 0_u32;
    let mut documents = Vec::new();
    let mut current: Option<PartialEntry> = None;

    for (index, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[documents]]" {
            if let Some(partial) = current.take() {
                documents.push(partial.finish());
            }
            current = Some(PartialEntry::default());
            continue;
        }
        let (key, value) = split_toml_assignment(line, index);
        match current.as_mut() {
            None => {
                if key == "schema_version" {
                    schema_version = value.parse().expect("schema_version integer");
                }
            }
            Some(entry) => entry.set(key, value, index),
        }
    }
    if let Some(partial) = current {
        documents.push(partial.finish());
    }

    assert_eq!(schema_version, 1, "unsupported MANIFEST schema_version");
    Manifest {
        schema_version,
        documents,
    }
}

#[derive(Default)]
struct PartialEntry {
    id: Option<String>,
    path: Option<String>,
    golden: Option<String>,
    sector: Option<String>,
    brand: Option<String>,
    split: Option<String>,
    rights: Option<String>,
    parser: Option<String>,
    mime: Option<String>,
}

impl PartialEntry {
    fn set(&mut self, key: &str, value: String, index: usize) {
        match key {
            "id" => self.id = Some(value),
            "path" => self.path = Some(value),
            "golden" => self.golden = Some(value),
            "sector" => self.sector = Some(value),
            "brand" => self.brand = Some(value),
            "split" => self.split = Some(value),
            "rights" => self.rights = Some(value),
            "parser" => self.parser = Some(value),
            "mime" => self.mime = Some(value),
            other => unreachable!("line {}: unknown MANIFEST key {other}", index + 1),
        }
    }

    fn finish(self) -> ManifestEntry {
        let path = self.path.expect("documents entry missing path");
        let parser = self.parser.unwrap_or_else(|| default_parser(&path));
        ManifestEntry {
            id: self.id.expect("documents entry missing id"),
            golden: self.golden.expect("documents entry missing golden"),
            sector: self.sector.expect("documents entry missing sector"),
            brand: self.brand.expect("documents entry missing brand"),
            split: self.split.expect("documents entry missing split"),
            rights: self.rights.expect("documents entry missing rights"),
            path,
            parser,
            mime: self.mime,
        }
    }
}

fn default_parser(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => "analyze_bytes".into(),
        "jpg" | "jpeg" | "png" => "ocr_smoke".into(),
        _ => "invoice_text".into(),
    }
}

fn split_toml_assignment(line: &str, index: usize) -> (&str, String) {
    assert!(
        line.contains('='),
        "line {}: expected key = value",
        index + 1
    );
    let (key, raw) = line.split_once('=').expect("key = value");
    let key = key.trim();
    let raw = raw.trim();
    let value = if let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner.to_owned()
    } else {
        raw.to_owned()
    };
    (key, value)
}

fn load_golden(path: &Path) -> Golden {
    let src = fs::read_to_string(path).expect("golden JSON");
    serde_json::from_str(&src).expect("golden JSON parse")
}

fn suggest_for(entry: &ManifestEntry, bytes: &[u8]) -> DocumentSuggestion {
    match entry.parser.as_str() {
        "invoice_text" => {
            let text = String::from_utf8_lossy(bytes);
            parse_invoice_text(&text, Locale::En)
        }
        "analyze_bytes" => {
            let mime = entry.mime.as_deref().unwrap_or("text/plain");
            let name = Path::new(&entry.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("document");
            analyze_document_bytes(
                &NewDocument {
                    filename: name,
                    mime_type: mime,
                    data: bytes,
                },
                &AnalyzeContext {
                    template: ChartTemplate::Blank,
                    accounts: &[],
                    default_currency: "EUR".parse().expect("EUR is a currency code"),
                    locale: Locale::En,
                },
                None,
            )
        }
        other => unreachable!("id {}: unknown parser {other}", entry.id),
    }
}

fn merchant_matches(got: Option<&str>, golden: &Golden) -> bool {
    let Some(got) = got else {
        return golden.merchant.is_none() && golden.merchant_aliases.is_empty();
    };
    if let Some(exact) = golden.merchant.as_deref()
        && got == exact
    {
        return true;
    }
    if golden
        .merchant_aliases
        .iter()
        .any(|alias| got.contains(alias))
    {
        return true;
    }
    golden.merchant.is_none() && golden.merchant_aliases.is_empty()
}

fn assert_against_golden(id: &str, got: &DocumentSuggestion, golden: &Golden) {
    if let Some(amount) = golden.amount_minor {
        assert_eq!(
            got.amount_minor,
            Some(amount),
            "{id}: amount_minor (AFM/IBAN must not win)"
        );
    }
    if let Some(date) = golden.entry_date.as_deref() {
        assert_eq!(
            got.entry_date.map(format_date).as_deref(),
            Some(date),
            "{id}: entry_date"
        );
    }
    if let Some(kind) = golden.kind {
        assert_eq!(got.kind, kind, "{id}: kind");
    }
    if let Some(unpaid) = golden.bill_unpaid {
        assert_eq!(got.bill_unpaid, unpaid, "{id}: bill_unpaid");
    }
    if golden.merchant.is_some() || !golden.merchant_aliases.is_empty() {
        assert!(
            merchant_matches(got.merchant.as_deref(), golden),
            "{id}: merchant got={:?} exact={:?} aliases={:?}",
            got.merchant,
            golden.merchant,
            golden.merchant_aliases
        );
    }
    if let Some(needle) = golden.description.as_deref() {
        assert!(
            got.description
                .as_deref()
                .is_some_and(|d| d == needle || d.contains(needle)),
            "{id}: description got={:?}",
            got.description
        );
    }
    if let Some(reference) = golden.reference.as_deref() {
        assert_eq!(got.reference.as_deref(), Some(reference), "{id}: reference");
    }
}

fn is_private_row(entry: &ManifestEntry) -> bool {
    entry.split == "private"
        || entry.rights == "private"
        || entry.path.starts_with("private/")
        || entry.golden.starts_with("private/")
}

fn write_golden(path: &Path, suggestion: &DocumentSuggestion) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("golden dir");
    }
    let golden = Golden {
        amount_minor: suggestion.amount_minor,
        entry_date: suggestion.entry_date.map(format_date),
        kind: Some(suggestion.kind),
        merchant: suggestion.merchant.clone(),
        merchant_aliases: Vec::new(),
        bill_unpaid: Some(suggestion.bill_unpaid),
        description: suggestion.description.clone(),
        reference: suggestion.reference.clone(),
    };
    let json = serde_json::to_string_pretty(&golden).expect("serialize golden");
    fs::write(path, format!("{json}\n")).expect("write golden");
}

fn public_tree_has_no_private_rights(manifest: &Manifest) {
    for entry in &manifest.documents {
        if is_private_row(entry) {
            continue;
        }
        assert_eq!(
            entry.rights, "synthetic",
            "{}: public corpus rows must be rights=synthetic",
            entry.id
        );
        assert!(
            !entry.path.starts_with("private/"),
            "{}: public path must not be under private/",
            entry.id
        );
    }
}

/// Where the OCR models are checked in, relative to the workspace root.
const BUNDLED_OCR_REL: &str = "apps/desktop/src-tauri/resources/ocr";

/// The directory holding the OCR models: `OIKONOMIA_OCR_MODELS` when it
/// names one that has them, else the copy checked into the repository.
///
/// The models are ordinary tracked files (no Git LFS, no download step), so
/// a checkout without them is broken and the OCR tests fail instead of
/// passing without reading anything.
fn bundled_ocr_dir() -> PathBuf {
    // The files are checked directly: the analyzer status is true for any
    // directory once another test has loaded the engine.
    if let Some(dir) = std::env::var_os("OIKONOMIA_OCR_MODELS") {
        let path = PathBuf::from(dir);
        if OcrModelPaths::from_dir(&path).available() {
            return path;
        }
    }

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bundled = workspace.join(BUNDLED_OCR_REL);
    assert!(
        OcrModelPaths::from_dir(&bundled).available(),
        "the OCR models text-detection.rten and text-recognition.rten must be in {} \
         (or in the directory OIKONOMIA_OCR_MODELS names)",
        bundled.display()
    );
    bundled
}

#[test]
fn manifest_schema_and_public_rights() {
    let manifest = load_manifest();
    assert_eq!(manifest.schema_version, 1);
    assert!(
        !manifest.documents.is_empty(),
        "MANIFEST.toml must list fixtures"
    );
    public_tree_has_no_private_rights(&manifest);
    let _ = (&manifest.documents[0].sector, &manifest.documents[0].brand);
}

#[test]
fn golden_corpus_matches_parser() {
    let manifest = load_manifest();
    let root = corpus_root();
    let mut compared = 0_usize;

    for entry in &manifest.documents {
        if entry.parser == "ocr_smoke" {
            continue;
        }
        let input_path = root.join(&entry.path);
        if !input_path.is_file() {
            assert!(
                is_private_row(entry),
                "id {}: missing fixture {} (only private rows may be absent)",
                entry.id,
                entry.path
            );
            continue;
        }

        let bytes = fs::read(&input_path).expect("fixture bytes");
        let suggestion = suggest_for(entry, &bytes);
        let golden = load_golden(&root.join(&entry.golden));
        assert_against_golden(&entry.id, &suggestion, &golden);
        compared += 1;
    }

    assert!(
        compared >= 12,
        "expected every public text and PDF fixture, compared={compared}"
    );
}

#[test]
fn text_mime_analyze_path_matches_invoice_reader() {
    let root = corpus_root();
    let text = fs::read(root.join("synthetic/text/dei_electricity_current.txt")).expect("text");
    let via_parse = parse_invoice_text(&String::from_utf8_lossy(&text), Locale::En);
    let via_analyze = analyze_document_bytes(
        &NewDocument {
            filename: "dei_electricity_current.txt",
            mime_type: "text/plain",
            data: &text,
        },
        &AnalyzeContext {
            template: ChartTemplate::Blank,
            accounts: &[],
            default_currency: "EUR".parse().expect("EUR is a currency code"),
            locale: Locale::En,
        },
        None,
    );

    assert_eq!(via_analyze.amount_minor, via_parse.amount_minor);
    assert_eq!(via_analyze.entry_date, via_parse.entry_date);
    assert_eq!(via_analyze.kind, via_parse.kind);
    assert_eq!(via_analyze.merchant, via_parse.merchant);
    assert_eq!(via_analyze.bill_unpaid, via_parse.bill_unpaid);

    let transfer = fs::read(root.join("synthetic/text/greek_bank_embasma.txt")).expect("transfer");
    let transfer_parse = parse_invoice_text(&String::from_utf8_lossy(&transfer), Locale::En);
    let transfer_analyze = analyze_document_bytes(
        &NewDocument {
            filename: "greek_bank_embasma.txt",
            mime_type: "text/plain",
            data: &transfer,
        },
        &AnalyzeContext {
            template: ChartTemplate::Blank,
            accounts: &[],
            default_currency: "EUR".parse().expect("EUR is a currency code"),
            locale: Locale::En,
        },
        None,
    );
    assert_eq!(transfer_analyze.amount_minor, transfer_parse.amount_minor);
    assert_eq!(transfer_analyze.entry_date, transfer_parse.entry_date);
    assert_eq!(transfer_analyze.kind, transfer_parse.kind);
    assert_eq!(transfer_analyze.merchant, transfer_parse.merchant);
    assert_eq!(transfer_analyze.reference, transfer_parse.reference);
}

/// Writes a starter golden for every manifest row that has none.
///
/// A generator, not a check: it writes into `testdata/`, so it never runs
/// with the suite. Run it by hand after adding a fixture, then review every
/// field of the new file before committing it:
///
/// ```text
/// cargo test -p oikonomia-core --test document_corpus write_missing_goldens -- --ignored
/// ```
#[test]
#[ignore = "generator: writes golden files into testdata; run by hand"]
fn write_missing_goldens() {
    let manifest = load_manifest();
    let root = corpus_root();
    for entry in &manifest.documents {
        if entry.parser == "ocr_smoke" {
            continue;
        }
        let input_path = root.join(&entry.path);
        if !input_path.is_file() {
            continue;
        }
        let golden_path = root.join(&entry.golden);
        if golden_path.is_file() {
            continue;
        }
        let bytes = fs::read(&input_path).expect("fixture bytes");
        let suggestion = suggest_for(entry, &bytes);
        write_golden(&golden_path, &suggestion);
    }
}

#[test]
fn a_jpeg_is_read_through_ocr() {
    let root = corpus_root();
    let jpeg_path = root.join("synthetic/image/english_total.jpg");
    assert!(jpeg_path.is_file(), "synthetic JPEG must be checked in");
    let bytes = fs::read(&jpeg_path).expect("the corpus JPEG is checked in");

    let model_dir = bundled_ocr_dir();

    let suggestion = analyze_document_bytes(
        &NewDocument {
            filename: "english_total.jpg",
            mime_type: "image/jpeg",
            data: &bytes,
        },
        &AnalyzeContext {
            template: ChartTemplate::Blank,
            accounts: &[],
            default_currency: "EUR".parse().expect("EUR is a currency code"),
            locale: Locale::En,
        },
        Some(model_dir.as_path()),
    );

    let golden = load_golden(&root.join("golden/english_total_jpeg.json"));
    assert_against_golden("english-total-jpeg", &suggestion, &golden);

    assert!(
        analyzer_status(Some(Path::new("no-such-model-directory"))).ocr_available(),
        "a loaded engine answers for any directory"
    );
}

/// A one-page PDF whose only content is an image stream marked as a JPEG
/// and holding `image_bytes`.
fn scanned_pdf(image_bytes: &[u8]) -> Vec<u8> {
    use pdf_extract::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let image_id = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Image", "Filter" => "DCTDecode" },
        image_bytes.to_vec(),
    ));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
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

fn analyze_pdf(pdf: &[u8], model_dir: &Path) -> DocumentSuggestion {
    analyze_document_bytes(
        &NewDocument {
            filename: "scan.pdf",
            mime_type: "application/pdf",
            data: pdf,
        },
        &AnalyzeContext {
            template: ChartTemplate::Blank,
            accounts: &[],
            default_currency: "EUR".parse().expect("EUR is a currency code"),
            locale: Locale::En,
        },
        Some(model_dir),
    )
}

/// Runs in this file because it loads the real models: the engine is one
/// per process, and once loaded it hides a missing model directory from the
/// unit tests that check for one.
#[test]
fn a_scanned_pdf_is_read_through_its_image_or_says_why_not() {
    let model_dir = bundled_ocr_dir();
    let root = corpus_root();
    let jpeg = fs::read(root.join("synthetic/image/english_total.jpg"))
        .expect("the corpus JPEG is checked in");

    let read = analyze_pdf(&scanned_pdf(&jpeg), model_dir.as_path());
    let golden = load_golden(&root.join("golden/english_total_jpeg.json"));
    assert_against_golden("english-total-jpeg-in-pdf", &read, &golden);
    assert_eq!(
        read.notes.first().map(|note| note.code),
        Some(UiTextCode::OcrPdfImage)
    );

    let unreadable = analyze_pdf(&scanned_pdf(b"not a jpeg"), model_dir.as_path());
    assert_eq!(
        unreadable
            .notes
            .iter()
            .map(|note| note.code)
            .collect::<Vec<_>>(),
        [UiTextCode::OcrFailed]
    );
}

/// The accounts a suggestion points at, as chart codes, and the entry kind.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SuggestedCodes {
    kind: EntryKindSuggestion,
    category: Option<String>,
    wallet: Option<String>,
    payable: Option<String>,
}

const LOCALES: [Locale; 4] = [Locale::En, Locale::El, Locale::Fr, Locale::De];

const SEEDED_CHARTS: [ChartTemplate; 2] = [ChartTemplate::Personal, ChartTemplate::Company];

/// Documents the corpus does not hold, each reaching generated wording the
/// corpus documents do not: an unnamed gas supplier, an unnamed electricity
/// supplier, an invoice known only by its number, and a bare invoice.
const EXTRA_DOCUMENTS: [(&str, &str); 4] = [
    (
        "extra-unnamed-gas",
        "Λογαριασμός\nΠρομήθεια φυσικού αερίου\nΣΥΝΟΛΟ 45,90 EUR\n12/03/2025",
    ),
    (
        "extra-unnamed-electricity",
        "Power Business\nΣΥΝΟΛΟ 120,00 EUR\n12/03/2025",
    ),
    (
        "extra-invoice-number",
        "Acme Software Ltd\nInvoice No: INV-2025-0042\nTOTAL 99,00 EUR\n2025-03-12",
    ),
    ("extra-bare-invoice", "Τιμολόγιο\nΣΥΝΟΛΟ 10,00 EUR"),
];

/// Every document the language-independence test runs: the corpus (text and
/// PDF, not the JPEG that needs OCR models) plus the extra documents.
fn language_test_documents() -> Vec<(String, String, Vec<u8>)> {
    let root = corpus_root();
    let mut documents = Vec::new();

    for entry in &load_manifest().documents {
        if entry.parser == "ocr_smoke" {
            continue;
        }

        let path = root.join(&entry.path);
        if !path.is_file() {
            continue;
        }

        let mime = entry.mime.clone().unwrap_or_else(|| "text/plain".into());
        documents.push((
            entry.id.clone(),
            mime,
            fs::read(&path).expect("fixture bytes"),
        ));
    }

    for (id, text) in EXTRA_DOCUMENTS {
        documents.push((id.to_owned(), "text/plain".into(), text.as_bytes().to_vec()));
    }

    documents
}

/// The accounts of a freshly seeded book on `template`.
fn seeded_accounts(template: ChartTemplate) -> Vec<oikonomia_core::domain::Account> {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let mut vault = oikonomia_core::vault::Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");

    let conn = vault.connection().expect("connection");
    let entity = oikonomia_core::ledger::create_entity(
        conn,
        &oikonomia_core::ledger::CreateEntity {
            name: "Language".into(),
            base_currency: "EUR".into(),
            chart_template: template,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("entity");

    oikonomia_core::ledger::list_accounts(conn, entity.id).expect("accounts")
}

fn suggested_codes(
    template: ChartTemplate,
    accounts: &[oikonomia_core::domain::Account],
    locale: Locale,
    mime: &str,
    bytes: &[u8],
) -> SuggestedCodes {
    let suggestion = analyze_document_bytes(
        &NewDocument {
            filename: "document",
            mime_type: mime,
            data: bytes,
        },
        &AnalyzeContext {
            template,
            accounts,
            default_currency: "EUR".parse().expect("EUR is a currency code"),
            locale,
        },
        None,
    );

    let code_of = |id: Option<oikonomia_core::domain::AccountId>| -> Option<String> {
        let id = id?;
        accounts
            .iter()
            .find(|account| account.id == id)
            .map(|account| account.code.clone())
    };

    SuggestedCodes {
        kind: suggestion.kind,
        category: code_of(suggestion.category_account_id),
        wallet: code_of(suggestion.wallet_account_id),
        payable: code_of(suggestion.payable_account_id),
    }
}

#[test]
fn suggested_accounts_are_the_same_in_every_language() {
    let documents = language_test_documents();

    for template in SEEDED_CHARTS {
        let accounts = seeded_accounts(template);

        for (id, mime, bytes) in &documents {
            let english = suggested_codes(template, &accounts, Locale::En, mime, bytes);

            for locale in LOCALES {
                assert_eq!(
                    suggested_codes(template, &accounts, locale, mime, bytes),
                    english,
                    "{id} on {template:?} in {locale:?} differs from English"
                );
            }
        }
    }
}

/// The suggested category code of each document in English, as the app chose
/// it before text was localized: personal chart, then company chart. Pinned so
/// a change to the matching that moves an English user's suggestion fails.
const ENGLISH_CATEGORY_PINS: [(&str, &str, &str); 19] = [
    ("greek-sales-invoice", "4900", "4000"),
    ("dei-settlement", "5300", "5900"),
    ("cosmote-pay-via", "5350", "5900"),
    ("volton-myon-gas", "5300", "5900"),
    ("nova-telecom", "5350", "5900"),
    ("zenith-electricity", "5300", "5900"),
    ("ngs-gas-bill", "5300", "5900"),
    ("zenith-supplier-vs-grid", "5300", "5900"),
    ("dei-electricity-current", "5300", "5900"),
    ("volton-myon-gas-current", "5300", "5900"),
    ("cosmote-telecom-current", "5350", "5900"),
    ("eydap-water-current", "5300", "5900"),
    ("greek-bank-embasma", "5900", "5900"),
    ("dei-electricity-holdout", "5300", "5900"),
    ("english-total-pdf", "5350", "5900"),
    ("extra-unnamed-gas", "5300", "5900"),
    ("extra-unnamed-electricity", "5300", "5900"),
    ("extra-invoice-number", "5900", "5300"),
    ("extra-bare-invoice", "5350", "5900"),
];

#[test]
fn english_categories_are_the_ones_chosen_before_localization() {
    let documents = language_test_documents();

    assert_eq!(documents.len(), ENGLISH_CATEGORY_PINS.len());

    let personal_accounts = seeded_accounts(ChartTemplate::Personal);
    let company_accounts = seeded_accounts(ChartTemplate::Company);

    for (id, personal, company) in ENGLISH_CATEGORY_PINS {
        let (_, mime, bytes) = documents
            .iter()
            .find(|(document_id, _, _)| document_id == id)
            .expect(id);

        for (template, accounts, expected) in [
            (ChartTemplate::Personal, &personal_accounts, personal),
            (ChartTemplate::Company, &company_accounts, company),
        ] {
            let codes = suggested_codes(template, accounts, Locale::En, mime, bytes);

            assert_eq!(
                codes.category.as_deref(),
                Some(expected),
                "{id} on {template:?}"
            );
        }
    }
}
