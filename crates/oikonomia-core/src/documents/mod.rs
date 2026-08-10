//! Scanned bills/receipts: encrypted storage + **bundled offline OCR**.
//!
//! No cloud APIs. No Ollama. Models ship with the application (~12 MB).

mod analyze;
mod invoice;
mod ocr;
mod store;

pub use analyze::{
    AnalyzeSource, AnalyzerStatus, DocumentSuggestion, EntryKindSuggestion, analyze_document_bytes,
    analyzer_status,
};
pub use ocr::OcrModelPaths;
pub use store::{
    DocumentId, DocumentMeta, link_document_to_entry, resolve_mime, save_analysis_json,
    save_document, suggest_accounts_for_entity,
};
