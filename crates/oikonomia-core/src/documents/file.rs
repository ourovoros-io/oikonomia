//! A file offered as a document: what kind of file it is, and whether it may
//! be stored or read.
//!
//! Storage and analysis both start here. A file arrives with a name, a type
//! the webview declared for it, and its bytes ([`NewDocument`]); a file the
//! user dropped arrives as a path and is read by [`read_validated_file`].
//!
//! # The kind of a file
//!
//! [`DocumentKind::resolve`] decides the kind once, and every later step
//! matches on the result. Nothing after it looks at the declared type or the
//! name again. The rule:
//!
//! 1. A declared type the vault stores decides. `image/jpg` is read as
//!    `image/jpeg` and `application/x-pdf` as `application/pdf`.
//! 2. With any other declared type, the extension of the name decides: a
//!    file dropped into a desktop webview often arrives with an empty type.
//! 3. When the declared type and the extension name two different kinds and
//!    one of them is PDF, neither is trusted over the other. The bytes
//!    settle it: the file is a PDF exactly when [`PDF_MAGIC`] is within its
//!    first [`PDF_MAGIC_WINDOW`] bytes, and otherwise the other kind. So a
//!    file sent as `application/pdf` and named `notes.txt` is a PDF when it
//!    is one, whatever its name says, and a text file sent under a PDF type
//!    is still read as text.
//! 4. With neither a known type nor a known extension the file has no kind,
//!    and is refused.
//!
//! Two image kinds that disagree are not looked into: the declared one
//! stands, and the image decoder goes by the content anyway.
//!
//! # What may be stored
//!
//! [`NewDocument::validate`] refuses an empty file, one over
//! [`MAX_DOCUMENT_BYTES`], one with a blank name and one with no kind.
//! [`read_validated_file`] makes the size and type checks from the file's
//! metadata and name alone, before it reads a byte, so a stray drop of
//! gigabytes is never loaded into memory.

use std::path::Path;

use crate::error::{Error, NameField, Result, ValidationError};

/// Largest file that is stored or analyzed: 8 MiB.
///
/// The cap bounds what one document adds to the vault and what the analyzer
/// has to parse or run OCR on. The PDF budget and the PDF repair are sized
/// from it.
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

/// [`MAX_DOCUMENT_BYTES`] in whole megabytes, as shown to the user.
const MAX_DOCUMENT_MEGABYTES: u64 = 8;

// The two limits must never drift apart.
const _: () = assert!(MAX_DOCUMENT_BYTES as u64 == MAX_DOCUMENT_MEGABYTES * 1024 * 1024);

/// The bytes a PDF file starts with.
const PDF_MAGIC: &[u8] = b"%PDF-";

/// How far into a file [`PDF_MAGIC`] is looked for.
///
/// A PDF may carry other bytes before its header, and readers accept the
/// header anywhere in the first 1024 bytes (PDF Reference 1.7, appendix H,
/// the implementation note on section 3.4.1, "File Header").
const PDF_MAGIC_WINDOW: usize = 1024;

/// The kinds of file the vault stores and the analyzer reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    /// A PDF document.
    Pdf,
    /// A PNG image.
    Png,
    /// A JPEG image.
    Jpeg,
    /// A WebP image.
    Webp,
    /// Plain text.
    PlainText,
}

impl DocumentKind {
    /// The kind of a file, from the type declared for it, its name and its
    /// bytes, by the rule in the module documentation. `None` when the file
    /// is of no kind the vault stores.
    pub(crate) fn resolve(declared_mime: &str, filename: &str, data: &[u8]) -> Option<Self> {
        let declared = Self::from_mime(declared_mime);
        let named = Self::from_filename(filename);

        match (declared, named) {
            (Some(declared), Some(named)) if declared != named => {
                Some(Self::settle_pdf(declared, named, data))
            }
            (Some(kind), _) | (None, Some(kind)) => Some(kind),
            (None, None) => None,
        }
    }

    /// The kind the extension of `filename` names, in any letter case.
    pub(crate) fn from_filename(filename: &str) -> Option<Self> {
        let extension = Path::new(filename).extension()?.to_str()?;

        match extension.to_ascii_lowercase().as_str() {
            "pdf" => Some(Self::Pdf),
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            "txt" => Some(Self::PlainText),
            _ => None,
        }
    }

    /// The MIME type the kind is stored under.
    pub(crate) const fn mime(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::PlainText => "text/plain",
        }
    }

    /// The kind a declared MIME type names. The type is trimmed and compared
    /// in lowercase, without parameters: `text/plain; charset=utf-8` names
    /// none.
    fn from_mime(declared_mime: &str) -> Option<Self> {
        match declared_mime.trim().to_ascii_lowercase().as_str() {
            "application/pdf" | "application/x-pdf" => Some(Self::Pdf),
            "image/png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            "text/plain" => Some(Self::PlainText),
            _ => None,
        }
    }

    /// Chooses between a declared and a named kind that differ.
    ///
    /// When one of the two is PDF the bytes decide. Otherwise the declared
    /// kind stands.
    fn settle_pdf(declared: Self, named: Self, data: &[u8]) -> Self {
        let ((Self::Pdf, other) | (other, Self::Pdf)) = (declared, named) else {
            return declared;
        };

        if has_pdf_magic(data) {
            Self::Pdf
        } else {
            other
        }
    }
}

/// Whether [`PDF_MAGIC`] is within the first [`PDF_MAGIC_WINDOW`] bytes of
/// `data`.
fn has_pdf_magic(data: &[u8]) -> bool {
    let head = data.get(..PDF_MAGIC_WINDOW).unwrap_or(data);

    head.windows(PDF_MAGIC.len())
        .any(|window| window == PDF_MAGIC)
}

/// A file offered as a document: its name, the type declared for it, and its
/// bytes, as the caller received them.
///
/// Nothing is checked on construction. Storing validates the file, and
/// analysis reads whatever it can of it.
#[derive(Debug, Clone, Copy)]
pub struct NewDocument<'a> {
    /// The file's name. Only its extension and, once trimmed, the name it is
    /// stored under are taken from it.
    pub filename: &'a str,
    /// The MIME type the webview or the caller declares; empty when unknown.
    pub mime_type: &'a str,
    /// The file's bytes.
    pub data: &'a [u8],
}

impl<'a> NewDocument<'a> {
    /// Checks that the file may be stored.
    ///
    /// # Errors
    ///
    /// [`Error::Validation`] with, in this order,
    /// [`ValidationError::FileEmpty`] for no bytes,
    /// [`ValidationError::FileTooLarge`] over [`MAX_DOCUMENT_BYTES`],
    /// [`ValidationError::NameRequired`] for a blank name, and
    /// [`ValidationError::FileTypeUnsupported`] for a file that is not a PDF,
    /// a PNG, JPEG or WebP image, or plain text.
    pub fn validate(&self) -> Result<()> {
        self.checked().map(|_| ())
    }

    /// The file with its name trimmed and its kind resolved, once it has
    /// passed the checks of [`validate`](Self::validate).
    ///
    /// # Errors
    ///
    /// Those of [`validate`](Self::validate).
    pub(crate) fn checked(&self) -> Result<CheckedDocument<'a>> {
        let name = self.filename.trim();
        let kind = DocumentKind::resolve(self.mime_type, name, self.data);
        let kind = check_file(name, kind, self.data.len() as u64)?;

        Ok(CheckedDocument {
            name,
            kind,
            data: self.data,
        })
    }
}

/// A [`NewDocument`] that passed validation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedDocument<'a> {
    /// The trimmed file name. Not blank.
    pub name: &'a str,
    /// What kind of file it is.
    pub kind: DocumentKind,
    /// The file's bytes: at least one, at most [`MAX_DOCUMENT_BYTES`].
    pub data: &'a [u8],
}

/// A document read from disk: its name, the type its name gives it, and its
/// bytes.
#[derive(Debug, Clone)]
pub struct ReadDocument {
    /// The name the document is stored and typed under.
    pub filename: String,
    /// The MIME type resolved from the name.
    pub mime_type: String,
    /// The file's bytes.
    pub data: Vec<u8>,
}

impl ReadDocument {
    /// The document as the borrowed form that storing and analysis take.
    #[must_use]
    pub fn as_new(&self) -> NewDocument<'_> {
        NewDocument {
            filename: &self.filename,
            mime_type: &self.mime_type,
            data: &self.data,
        }
    }
}

/// Reads the file at `path` as a document named `filename`, refusing it
/// before it is read when it may not be stored.
///
/// The name is given apart from the path because a dropped link keeps its
/// own name and extension while the file it resolves to may be named
/// anything. The size and the type are checked from the file's metadata and
/// from `filename` alone; only a file that passes is read into memory. No
/// type is declared for a file on disk, so its extension decides its kind.
///
/// # Errors
///
/// - [`Error::Io`]: the file's metadata or its bytes cannot be read.
/// - [`Error::Validation`]: the file is empty, larger than
///   [`MAX_DOCUMENT_BYTES`], has a blank name, or has an extension the vault
///   does not store, as in [`NewDocument::validate`].
pub fn read_validated_file(path: &Path, filename: &str) -> Result<ReadDocument> {
    let metadata =
        std::fs::metadata(path).map_err(|err| Error::io("read document metadata", err))?;

    let name = filename.trim();
    let kind = check_file(name, DocumentKind::from_filename(name), metadata.len())?;

    let data = std::fs::read(path).map_err(|err| Error::io("read document", err))?;

    Ok(ReadDocument {
        filename: name.to_owned(),
        mime_type: kind.mime().to_owned(),
        data,
    })
}

/// Applies the storage rules to a file of `size_bytes` named `name`, whose
/// kind was resolved to `kind`, and returns the kind.
///
/// # Errors
///
/// The validation errors of [`NewDocument::validate`], in its order.
fn check_file(name: &str, kind: Option<DocumentKind>, size_bytes: u64) -> Result<DocumentKind> {
    if size_bytes == 0 {
        return Err(ValidationError::FileEmpty.into());
    }
    if size_bytes > MAX_DOCUMENT_BYTES as u64 {
        return Err(ValidationError::FileTooLarge {
            max_mb: MAX_DOCUMENT_MEGABYTES,
        }
        .into());
    }
    if name.is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::Filename,
        }
        .into());
    }

    kind.ok_or_else(|| ValidationError::FileTypeUnsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind of a file with these three properties.
    fn kind(declared_mime: &str, filename: &str, data: &[u8]) -> Option<DocumentKind> {
        DocumentKind::resolve(declared_mime, filename, data)
    }

    /// The validation error a file is refused with, if it is refused.
    fn refusal(filename: &str, mime_type: &str, data: &[u8]) -> Option<ValidationError> {
        let document = NewDocument {
            filename,
            mime_type,
            data,
        };

        match document.validate() {
            Err(Error::Validation(reason)) => Some(reason),
            _ => None,
        }
    }

    const PDF_BYTES: &[u8] = b"%PDF-1.7\n%%EOF";
    const TEXT_BYTES: &[u8] = b"TOTAL 45,90";

    #[test]
    fn a_stored_type_decides_and_its_aliases_name_the_same_kind() {
        assert_eq!(
            kind("application/pdf", "bill", b""),
            Some(DocumentKind::Pdf)
        );
        assert_eq!(
            kind("application/x-pdf", "bill", b""),
            Some(DocumentKind::Pdf)
        );
        assert_eq!(kind("image/jpg", "photo", b""), Some(DocumentKind::Jpeg));
        assert_eq!(kind("image/jpeg", "photo", b""), Some(DocumentKind::Jpeg));
        assert_eq!(kind(" IMAGE/PNG ", "shot", b""), Some(DocumentKind::Png));
        assert_eq!(kind("image/webp", "shot", b""), Some(DocumentKind::Webp));
        assert_eq!(
            kind("text/plain", "notes", b""),
            Some(DocumentKind::PlainText)
        );
    }

    #[test]
    fn without_a_stored_type_the_extension_decides() {
        for declared in ["", "application/octet-stream", "text/plain; charset=utf-8"] {
            assert_eq!(kind(declared, "bill.PDF", b""), Some(DocumentKind::Pdf));
            assert_eq!(kind(declared, "a.jpeg", b""), Some(DocumentKind::Jpeg));
            assert_eq!(kind(declared, "a.jpg", b""), Some(DocumentKind::Jpeg));
            assert_eq!(kind(declared, "a.png", b""), Some(DocumentKind::Png));
            assert_eq!(kind(declared, "a.webp", b""), Some(DocumentKind::Webp));
            assert_eq!(kind(declared, "a.txt", b""), Some(DocumentKind::PlainText));
        }
    }

    #[test]
    fn a_file_with_neither_a_stored_type_nor_a_known_extension_has_no_kind() {
        assert_eq!(kind("", "a.exe", PDF_BYTES), None);
        assert_eq!(kind("application/x-msdownload", "a.exe", b"MZ"), None);
        assert_eq!(kind("image/gif", "a.gif", b"GIF89a"), None);
        assert_eq!(kind("", "bill", PDF_BYTES), None);
    }

    #[test]
    fn a_pdf_type_on_a_file_named_as_text_is_settled_by_the_bytes() {
        // The declared type is right: the name does not override it.
        assert_eq!(
            kind("application/pdf", "x.txt", PDF_BYTES),
            Some(DocumentKind::Pdf)
        );
        // The declared type is wrong: the file is text, as it is named.
        assert_eq!(
            kind("application/pdf", "x.txt", TEXT_BYTES),
            Some(DocumentKind::PlainText)
        );
        assert_eq!(
            kind("application/x-pdf", "x.txt", PDF_BYTES),
            Some(DocumentKind::Pdf)
        );
    }

    #[test]
    fn a_file_named_as_a_pdf_under_another_type_is_settled_by_the_bytes() {
        assert_eq!(
            kind("text/plain", "x.pdf", PDF_BYTES),
            Some(DocumentKind::Pdf)
        );
        assert_eq!(
            kind("text/plain", "x.pdf", TEXT_BYTES),
            Some(DocumentKind::PlainText)
        );
        // An image named `scan.pdf` is still an image.
        assert_eq!(
            kind("image/jpeg", "scan.pdf", b"\xff\xd8\xff"),
            Some(DocumentKind::Jpeg)
        );
    }

    #[test]
    fn two_kinds_that_disagree_without_a_pdf_leave_the_declared_one() {
        assert_eq!(
            kind("image/png", "a.jpg", PDF_BYTES),
            Some(DocumentKind::Png)
        );
        assert_eq!(
            kind("text/plain", "a.png", b"\x89PNG"),
            Some(DocumentKind::PlainText)
        );
    }

    #[test]
    fn the_pdf_header_is_found_within_the_window_and_not_past_it() {
        let padded = |padding: usize| [vec![b' '; padding], PDF_MAGIC.to_vec()].concat();
        let last_start = PDF_MAGIC_WINDOW - PDF_MAGIC.len();

        assert!(has_pdf_magic(&padded(0)));
        assert!(has_pdf_magic(&padded(last_start)));
        assert!(!has_pdf_magic(&padded(last_start + 1)));
        assert!(!has_pdf_magic(b"%PDF"));
        assert!(!has_pdf_magic(b""));
    }

    #[test]
    fn every_kind_is_stored_under_a_type_that_names_it_again() {
        for kind in [
            DocumentKind::Pdf,
            DocumentKind::Png,
            DocumentKind::Jpeg,
            DocumentKind::Webp,
            DocumentKind::PlainText,
        ] {
            assert_eq!(DocumentKind::from_mime(kind.mime()), Some(kind));
        }
    }

    #[test]
    fn validation_refuses_by_size_then_name_then_type() {
        assert_eq!(refusal("a.pdf", "application/pdf", PDF_BYTES), None);
        assert_eq!(
            refusal("a.pdf", "application/pdf", b""),
            Some(ValidationError::FileEmpty)
        );
        assert_eq!(
            refusal("  ", "application/pdf", PDF_BYTES),
            Some(ValidationError::NameRequired {
                field: NameField::Filename
            })
        );
        assert_eq!(
            refusal("a.exe", "application/x-msdownload", b"MZ"),
            Some(ValidationError::FileTypeUnsupported)
        );
        // An empty file with a blank name and no type is first of all empty.
        assert_eq!(refusal(" ", "", b""), Some(ValidationError::FileEmpty));

        let oversize = vec![0_u8; MAX_DOCUMENT_BYTES + 1];
        assert_eq!(
            refusal("a.exe", "", &oversize),
            Some(ValidationError::FileTooLarge { max_mb: 8 })
        );
        let at_the_cap = vec![b' '; MAX_DOCUMENT_BYTES];
        assert_eq!(refusal("a.txt", "", &at_the_cap), None);
    }

    #[test]
    fn a_checked_document_carries_the_trimmed_name_and_the_resolved_kind() {
        let document = NewDocument {
            filename: "  scan.JPG ",
            mime_type: "",
            data: b"\xff\xd8",
        };

        let checked = document.checked().unwrap();

        assert_eq!(checked.name, "scan.JPG");
        assert_eq!(checked.kind, DocumentKind::Jpeg);
    }

    #[test]
    fn a_file_on_disk_is_read_under_the_name_it_was_given() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stored-under-another-name.bin");
        std::fs::write(&path, TEXT_BYTES).unwrap();

        let read = read_validated_file(&path, "bill.txt").unwrap();

        assert_eq!(read.filename, "bill.txt");
        assert_eq!(read.mime_type, "text/plain");
        assert_eq!(read.data, TEXT_BYTES);
        assert_eq!(read.as_new().filename, "bill.txt");
    }

    #[test]
    fn a_file_on_disk_is_refused_from_its_metadata_and_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");

        // A sparse file: over the cap by its length, with no data written.
        let oversize = std::fs::File::create(&path).unwrap();
        oversize.set_len(MAX_DOCUMENT_BYTES as u64 + 1).unwrap();
        drop(oversize);
        assert_eq!(
            read_validated_file(&path, "big.pdf").map(|read| read.data.len()),
            Err(Error::Validation(ValidationError::FileTooLarge {
                max_mb: 8
            }))
        );

        std::fs::write(&path, b"").unwrap();
        assert_eq!(
            read_validated_file(&path, "empty.pdf").map(|read| read.data.len()),
            Err(Error::Validation(ValidationError::FileEmpty))
        );

        std::fs::write(&path, b"MZ").unwrap();
        assert_eq!(
            read_validated_file(&path, "tool.exe").map(|read| read.data.len()),
            Err(Error::Validation(ValidationError::FileTypeUnsupported))
        );
    }

    #[test]
    fn a_file_that_is_not_there_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone.pdf");

        let read = read_validated_file(&missing, "gone.pdf").map(|read| read.data.len());

        assert!(matches!(read, Err(Error::Io { .. })), "{read:?}");
    }
}
