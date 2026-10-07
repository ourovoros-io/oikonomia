//! Loading a PDF for extraction: one parse, then the budget.
//!
//! [`BudgetedPdf`] lives in this module so that nothing outside it can build
//! one: its document is private, and [`load_pdf`] is the only constructor.
//! Every function that hands a document to pdf-extract takes a
//! [`BudgetedPdf`], so each of them works on a document that passed the
//! budget in [`pdf_budget`](crate::documents::pdf_budget), nesting check
//! included.
//!
//! [`contain_panics`] is the panic boundary of the PDF path. It lives here
//! because the load is the first thing that needs it; the analyzer wraps
//! every later call into lopdf and pdf-extract in it too.

// The same lopdf as pdf-extract uses; see `analyze`.
use pdf_extract as lopdf;

use crate::documents::file::MAX_DOCUMENT_BYTES;
use crate::documents::pdf_budget::within_budget;

/// A parsed PDF that is within the budget of
/// [`pdf_budget`](crate::documents::pdf_budget). Only [`load_pdf`] builds one.
///
/// The field is the document as lopdf parsed it, decrypted when it had an
/// empty user password.
pub(super) struct BudgetedPdf(lopdf::Document);

impl BudgetedPdf {
    /// The parsed document, for pdf-extract and image collection.
    pub(super) fn document(&self) -> &lopdf::Document {
        &self.0
    }
}

/// What loading a PDF produced.
pub(super) enum PdfLoad {
    /// The file parsed and is within the budget.
    Loaded(Box<BudgetedPdf>),
    /// The file, its page count, what its streams decode to or how deep
    /// pdf-extract would recurse into it is too large.
    OverBudget,
    /// lopdf could not parse the file, or it needs a password.
    Unreadable,
}

/// Runs `work`, turning a panic into `None`.
///
/// lopdf and pdf-extract index, `unwrap` and `expect` on file content, so a
/// malformed document can panic inside them; that makes the document
/// unreadable, not the app. The closures passed here only read borrowed
/// data and return owned values, so nothing is left half-updated when one
/// unwinds. Two failures abort instead of unwinding and are not caught: a
/// failed allocation and a stack overflow. The budget is what keeps
/// allocations small and recursion shallow; only a [`BudgetedPdf`] reaches
/// pdf-extract.
pub(super) fn contain_panics<T>(work: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok()
}

/// Parses `data` once and checks that one parsed document against the
/// budget; the check never parses the file again.
///
/// A file over the upload cap is over budget without being parsed. A file
/// encrypted with a password other than the empty one is unreadable, as is
/// one lopdf cannot parse or panics on. The parse, the decryption and the
/// budget all run inside [`contain_panics`].
pub(super) fn load_pdf(data: &[u8]) -> PdfLoad {
    // A stored document is never larger than the upload cap, and lopdf's
    // work and memory while parsing grow with the size of its input.
    if data.len() > MAX_DOCUMENT_BYTES {
        return PdfLoad::OverBudget;
    }

    let loaded = contain_panics(|| {
        let mut document = lopdf::Document::load_mem(data).ok()?;
        // Many PDFs are encrypted with an empty user password only to carry
        // permissions; pdf-extract's own entry points try it too.
        if document.is_encrypted() && document.decrypt("").is_err() {
            return None;
        }

        let fits = within_budget(&document);
        Some((document, fits))
    });

    match loaded.flatten() {
        Some((document, true)) => PdfLoad::Loaded(Box::new(BudgetedPdf(document))),
        Some((_, false)) => PdfLoad::OverBudget,
        None => PdfLoad::Unreadable,
    }
}
