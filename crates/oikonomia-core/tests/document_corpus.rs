//! Offline document golden harness (week 1).
//!
//! Loads `testdata/documents/MANIFEST.toml`, runs the invoice reader (and the
//! public analyze path for PDF / JPEG), and compares locked fields to golden
//! JSON. No network. Missing `private/` files are skipped.

#![expect(clippy::expect_used, reason = "corpus tests fail loudly by design")]

use std::fs;
use std::path::{Path, PathBuf};

use oikonomia_core::documents::{
    AnalyzeContext, DocumentSuggestion, EntryKindSuggestion, analyze_document_bytes,
    analyzer_status, parse_invoice_text,
};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::prefs::Locale;
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
                name,
                mime,
                bytes,
                &AnalyzeContext {
                    template: ChartTemplate::Blank,
                    accounts: &[],
                    default_currency: "EUR",
                    locale: Locale::En,
                },
                None,
            )
            .expect("analyze")
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
        assert_eq!(got.entry_date.as_deref(), Some(date), "{id}: entry_date");
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

fn dump_enabled() -> bool {
    std::env::var_os("DUMP_DOCUMENT_GOLDENS").is_some()
}

fn write_golden(path: &Path, suggestion: &DocumentSuggestion) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("golden dir");
    }
    let golden = Golden {
        amount_minor: suggestion.amount_minor,
        entry_date: suggestion.entry_date.clone(),
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

fn bundled_ocr_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OIKONOMIA_OCR_MODELS") {
        let path = PathBuf::from(dir);
        if analyzer_status(Some(path.as_path())).ocr_available {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let candidate = workspace.join("apps/desktop/src-tauri/resources/ocr");
    analyzer_status(Some(candidate.as_path()))
        .ocr_available
        .then_some(candidate)
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
        let golden_path = root.join(&entry.golden);

        if dump_enabled() && !golden_path.is_file() {
            write_golden(&golden_path, &suggestion);
            continue;
        }

        let golden = load_golden(&golden_path);
        assert_against_golden(&entry.id, &suggestion, &golden);
        compared += 1;
    }

    assert!(
        compared >= 12,
        "expected the week-1 public text/PDF fixtures, compared={compared}"
    );
}

#[test]
fn text_mime_analyze_path_matches_invoice_reader() {
    let root = corpus_root();
    let text = fs::read(root.join("synthetic/text/dei_electricity_current.txt")).expect("text");
    let via_parse = parse_invoice_text(&String::from_utf8_lossy(&text), Locale::En);
    let via_analyze = analyze_document_bytes(
        "dei_electricity_current.txt",
        "text/plain",
        &text,
        &AnalyzeContext {
            template: ChartTemplate::Blank,
            accounts: &[],
            default_currency: "EUR",
            locale: Locale::En,
        },
        None,
    )
    .expect("analyze text/plain");

    assert_eq!(via_analyze.amount_minor, via_parse.amount_minor);
    assert_eq!(via_analyze.entry_date, via_parse.entry_date);
    assert_eq!(via_analyze.kind, via_parse.kind);
    assert_eq!(via_analyze.merchant, via_parse.merchant);
    assert_eq!(via_analyze.bill_unpaid, via_parse.bill_unpaid);

    let transfer = fs::read(root.join("synthetic/text/greek_bank_embasma.txt")).expect("transfer");
    let transfer_parse = parse_invoice_text(&String::from_utf8_lossy(&transfer), Locale::En);
    let transfer_analyze = analyze_document_bytes(
        "greek_bank_embasma.txt",
        "text/plain",
        &transfer,
        &AnalyzeContext {
            template: ChartTemplate::Blank,
            accounts: &[],
            default_currency: "EUR",
            locale: Locale::En,
        },
        None,
    )
    .expect("analyze transfer text/plain");
    assert_eq!(transfer_analyze.amount_minor, transfer_parse.amount_minor);
    assert_eq!(transfer_analyze.entry_date, transfer_parse.entry_date);
    assert_eq!(transfer_analyze.kind, transfer_parse.kind);
    assert_eq!(transfer_analyze.merchant, transfer_parse.merchant);
    assert_eq!(transfer_analyze.reference, transfer_parse.reference);
}

#[test]
fn write_missing_goldens() {
    if !dump_enabled() {
        return;
    }
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
fn jpeg_ocr_smoke() {
    let root = corpus_root();
    let jpeg_path = root.join("synthetic/image/english_total.jpg");
    assert!(jpeg_path.is_file(), "synthetic JPEG must be checked in");
    let bytes = fs::read(&jpeg_path).expect("jpeg");

    let Some(model_dir) = bundled_ocr_dir() else {
        let status = analyzer_status(None);
        assert!(!status.ocr_available);
        assert!(status.offline);
        return;
    };

    let suggestion = analyze_document_bytes(
        "english_total.jpg",
        "image/jpeg",
        &bytes,
        &AnalyzeContext {
            template: ChartTemplate::Blank,
            accounts: &[],
            default_currency: "EUR",
            locale: Locale::En,
        },
        Some(model_dir.as_path()),
    )
    .expect("analyze jpeg");

    let golden_path = root.join("golden/english_total_jpeg.json");
    if golden_path.is_file() {
        let golden = load_golden(&golden_path);
        // OCR is best-effort: lock fields only when the golden exists and OCR
        // produced a comparable amount. Missing amount is not a CI failure.
        if suggestion.amount_minor.is_some() {
            assert_against_golden("english-total-jpeg", &suggestion, &golden);
        }
    }
}
