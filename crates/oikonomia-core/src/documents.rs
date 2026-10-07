//! Document intake: keeping a bill, receipt or statement in the vault, and
//! reading a draft journal entry out of it.
//!
//! Everything runs on the device. Text comes from the file itself or from OCR
//! models that ship with the application (about 12 MB); no cloud service and
//! no network is involved.
//!
//! # The pipeline
//!
//! [`analyze_document_bytes`] takes the bytes of one file and returns a
//! [`DocumentSuggestion`] for the user to review. It never fails on a file it
//! cannot read: the suggestion then holds a note that says why. The stages,
//! in order, and the file under `documents/` that owns each:
//!
//! 1. **Kind** (`file.rs`). The declared MIME type, the file extension and,
//!    when those two disagree about a PDF, the first bytes sort the file:
//!    image, plain text, PDF, or nothing readable.
//! 2. **PDF budget** (`pdf_load.rs`, `pdf_budget.rs`, `pdf_nesting.rs`). A
//!    PDF is parsed once. A file over 8 MiB, over 50 pages, or whose streams
//!    decode to over 32 MiB is not read at all. Nor, checked last, is one
//!    whose forms or page tree would make pdf-extract recurse without end:
//!    that overflows the stack, which no panic handler catches.
//! 3. **Whole-document text** (`analyze.rs`). pdf-extract reads the text
//!    layer of the whole PDF.
//! 4. **Per-page text** (`analyze.rs`). If that fails, each page is read on
//!    its own and the pages that can be read are joined.
//! 5. **Repair and retry** (`pdf_repair.rs`, `analyze.rs`). If there is still
//!    no text, stale cross-reference offsets are rewritten in a copy, and
//!    stages 2 to 4 run once more on the copy.
//! 6. **Embedded-JPEG fallback** (`analyze.rs`). A PDF with fewer than 8
//!    characters of text is taken as a scan: up to two JPEG images embedded
//!    in it go to OCR.
//! 7. **OCR** (`ocr.rs`). Image files, and those embedded JPEGs: decoding
//!    limits, resampling, contrast, recognition.
//! 8. **Invoice reader** (`invoice.rs`, `brands.rs`). The text becomes an
//!    amount, a date, a reference, a merchant, a description and an entry
//!    kind.
//! 9. **Account matching** (`account_match.rs`, `analyze.rs`). Keywords in the
//!    merchant and description choose a topic, and the topic an account of
//!    the book. The wallet and payable accounts are the book's defaults.
//! 10. **Suggestion** (`analyze.rs`). The notes are put in order, and an
//!     amount is withheld when the book's currency does not have two
//!     decimals.
//!
//! Plain text skips stages 2 to 7 and goes straight to the invoice reader.
//!
//! # Storage
//!
//! The other half of this module keeps documents: `store.rs` writes the bytes
//! into the encrypted vault database, linked to a journal entry, and reads
//! them back. Analysis does not depend on storage; the desktop shell analyzes
//! a dropped file first and stores it when the user posts the entry.

mod account_match;
mod analyze;
mod brands;
mod file;
mod invoice;
mod keyword;
mod ocr;
mod pdf_budget;
mod pdf_load;
mod pdf_nesting;
mod pdf_repair;
mod store;

pub use analyze::{
    AnalyzeContext, AnalyzeSource, AnalyzerHint, AnalyzerStatus, DocumentSuggestion,
    EntryKindSuggestion, analyze_document_bytes, analyzer_status, parse_invoice_text,
};
pub use file::{MAX_DOCUMENT_BYTES, NewDocument, ReadDocument, read_validated_file};
pub use ocr::OcrModelPaths;
pub use store::{
    DocumentId, DocumentMeta, attach_document, delete_document, get_document, list_documents,
    post_simple_entry_with_document, save_analysis_json, suggest_accounts_for_entity,
};
