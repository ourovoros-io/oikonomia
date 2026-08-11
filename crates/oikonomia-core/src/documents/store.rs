//! Persist document blobs inside the encrypted vault database.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{Account, AccountId, AccountType, EntityId, JournalEntryId};
use crate::error::{Error, Result};
use crate::ledger::{
    PostSimpleEntry, PostedEntryView, get_entry, list_accounts, post_simple_entry_unchecked,
};
use crate::util::{now_utc_string, parse_uuid};

/// Document primary key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(pub Uuid);

impl DocumentId {
    /// New random id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DocumentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadata without raw bytes (for lists / IPC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    /// Id.
    pub id: DocumentId,
    /// Entity book.
    pub entity_id: EntityId,
    /// Linked entry (a document cannot exist without one).
    pub entry_id: JournalEntryId,
    /// Original filename.
    pub filename: String,
    /// MIME type.
    pub mime_type: String,
    /// Byte length.
    pub size_bytes: i64,
    /// Creation instant as the app-wide `unix:<seconds>` ordering key
    /// (see `now_utc_string`); not a display date — the UI formats it.
    pub created_at: String,
}

/// Max upload size (8 MiB) — keeps vault lean and model latency reasonable.
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

/// Validate a candidate document before its bytes are loaded or stored.
///
/// Shared by [`save_document`] and the drop-path command so oversized or
/// unsupported files are rejected from a `stat` alone, before any read.
///
/// # Errors
///
/// Returns [`Error::Validation`] for empty/oversized files, blank filenames,
/// or unsupported MIME types.
pub fn validate_document_file(filename: &str, mime: &str, size_bytes: u64) -> Result<()> {
    if size_bytes == 0 {
        return Err(Error::Validation("empty file".into()));
    }
    if size_bytes > MAX_DOCUMENT_BYTES as u64 {
        return Err(Error::Validation(format!(
            "file too large (max {} MB)",
            MAX_DOCUMENT_BYTES / (1024 * 1024)
        )));
    }
    if filename.trim().is_empty() {
        return Err(Error::Validation("filename is required".into()));
    }
    if !is_allowed_mime(mime) {
        return Err(Error::Validation(
            "unsupported file type — use PDF, PNG, JPEG, WebP, or plain text".into(),
        ));
    }
    Ok(())
}

/// Store a document blob linked to `entry_id` (already protected by `SQLCipher`).
///
/// # Errors
///
/// [`Error::Validation`] for invalid files or a duplicate filename in the
/// book; DB errors otherwise.
pub fn save_document(
    conn: &Connection,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> Result<DocumentMeta> {
    let name = filename.trim();
    let mime = resolve_mime(mime_type, name);
    validate_document_file(name, &mime, data.len() as u64)?;

    let clash: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM documents WHERE entity_id = ?1 AND filename = ?2",
            rusqlite::params![entity_id.0.to_string(), name],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if clash > 0 {
        return Err(Error::Validation(format!(
            "a document named {name} already exists in this book"
        )));
    }

    // Validation caps the size at 8 MiB, so the length always fits an i64.
    let size_bytes = i64::try_from(data.len()).unwrap_or(i64::MAX);

    let id = DocumentId::new();
    let created = now_utc_string();

    conn.execute(
        "
        INSERT INTO documents (
            id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at, analysis_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)
        ",
        rusqlite::params![
            id.0.to_string(),
            entity_id.0.to_string(),
            entry_id.0.to_string(),
            name,
            mime,
            size_bytes,
            data,
            created,
        ],
    )
    .map_err(|err| match err.sqlite_error_code() {
        // Backstop: the pre-check races nothing (single-writer vault), but a
        // constraint violation must still read as validation, not IO.
        Some(rusqlite::ErrorCode::ConstraintViolation) => Error::Validation(format!(
            "a document named {name} already exists in this book"
        )),
        _ => Error::Io(err.to_string()),
    })?;

    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id,
        filename: name.to_owned(),
        mime_type: mime,
        size_bytes,
        created_at: created,
    })
}

/// Validate and store a document linked to an existing entry (no OCR —
/// analysis belongs to the drop-zone flow).
///
/// # Errors
///
/// [`Error::NotFound`] for a missing entry, [`Error::Validation`] for an
/// entry in a different book, an invalid file, or a duplicate filename.
pub fn attach_document(
    conn: &Connection,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> Result<DocumentMeta> {
    let entry = get_entry(conn, entry_id)?;
    if entry.entry.entity_id != entity_id {
        return Err(Error::Validation(
            "entry belongs to a different book".into(),
        ));
    }

    save_document(conn, entity_id, entry_id, filename, mime_type, data)
}

/// Post a simple entry and store its document in one transaction.
///
/// A duplicate filename (or any other failure) rolls back the entry too —
/// the vault never holds a document without its entry or vice versa from
/// this path.
///
/// # Errors
///
/// All [`post_simple_entry`](crate::ledger::post_simple_entry) and
/// [`save_document`] errors.
pub fn post_simple_entry_with_document(
    conn: &Connection,
    input: &PostSimpleEntry,
    filename: &str,
    mime_type: &str,
    data: &[u8],
    analysis_json: Option<&str>,
) -> Result<(PostedEntryView, DocumentMeta)> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;

    let view = post_simple_entry_unchecked(&tx, input)?;
    let meta = save_document(
        &tx,
        input.entity_id,
        view.entry.id,
        filename,
        mime_type,
        data,
    )?;
    if let Some(json) = analysis_json {
        save_analysis_json(&tx, meta.id, json)?;
    }

    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok((view, meta))
}

type MetaColumns = (String, String, String, String, String, i64, String);

fn meta_from_columns(raw: MetaColumns) -> Result<DocumentMeta> {
    let (id_s, entity_s, entry_s, filename, mime_type, size_bytes, created_at) = raw;
    Ok(DocumentMeta {
        id: DocumentId(parse_uuid(&id_s)?),
        entity_id: EntityId(parse_uuid(&entity_s)?),
        entry_id: JournalEntryId(parse_uuid(&entry_s)?),
        filename,
        mime_type,
        size_bytes,
        created_at,
    })
}

/// All documents for an entity, newest first (metadata only — no blobs).
///
/// # Errors
///
/// DB errors.
pub fn list_documents(conn: &Connection, entity_id: EntityId) -> Result<Vec<DocumentMeta>> {
    // Order by created_at DESC, rowid DESC for deterministic insertion-recency order.
    // now_utc_string has 1-second granularity, so multiple documents saved in the same
    // second may tie on created_at; rowid tie-break ensures deterministic order.
    let mut stmt = conn
        .prepare(
            "
            SELECT id, entity_id, entry_id, filename, mime_type, size_bytes, created_at
            FROM documents
            WHERE entity_id = ?1
            ORDER BY created_at DESC, rowid DESC
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map([entity_id.0.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let raw = row.map_err(|err| Error::Io(err.to_string()))?;
        out.push(meta_from_columns(raw)?);
    }
    Ok(out)
}

/// One document's metadata plus raw bytes (for viewing/export).
///
/// # Errors
///
/// Not found or DB error.
pub fn get_document(conn: &Connection, id: DocumentId) -> Result<(DocumentMeta, Vec<u8>)> {
    let (raw, data) = conn
        .query_row(
            "
            SELECT id, entity_id, entry_id, filename, mime_type, size_bytes, created_at, data
            FROM documents
            WHERE id = ?1
            ",
            [id.0.to_string()],
            |row| {
                Ok((
                    (
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                    ),
                    row.get::<_, Vec<u8>>(7)?,
                ))
            },
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound("document".into()),
            other => Error::Io(other.to_string()),
        })?;

    Ok((meta_from_columns(raw)?, data))
}

/// Permanently remove a document blob. Linked entries are unaffected.
///
/// # Errors
///
/// Not found or DB error.
pub fn delete_document(conn: &Connection, id: DocumentId) -> Result<()> {
    let n = conn
        .execute("DELETE FROM documents WHERE id = ?1", [id.0.to_string()])
        .map_err(|err| Error::Io(err.to_string()))?;
    if n == 0 {
        return Err(Error::NotFound("document".into()));
    }
    Ok(())
}

/// Save analysis JSON snapshot on the document row.
///
/// # Errors
///
/// DB errors.
pub fn save_analysis_json(conn: &Connection, id: DocumentId, json: &str) -> Result<()> {
    conn.execute(
        "UPDATE documents SET analysis_json = ?1 WHERE id = ?2",
        rusqlite::params![json, id.0.to_string()],
    )
    .map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

/// Active accounts useful for auto-matching.
///
/// # Errors
///
/// DB errors.
pub fn suggest_accounts_for_entity(conn: &Connection, entity_id: EntityId) -> Result<Vec<Account>> {
    list_accounts(conn, entity_id)
}

fn is_allowed_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/pdf" | "image/png" | "image/jpeg" | "image/jpg" | "image/webp" | "text/plain"
    )
}

/// Infer MIME from the browser-provided type and/or filename extension.
///
/// Dropped files in desktop webviews often arrive with an empty `type`.
#[must_use]
pub fn resolve_mime(mime_type: &str, filename: &str) -> String {
    let mime = mime_type.trim().to_ascii_lowercase();
    if is_allowed_mime(&mime) {
        return if mime == "image/jpg" {
            "image/jpeg".into()
        } else {
            mime
        };
    }

    if has_extension(filename, "pdf") {
        return "application/pdf".into();
    }
    if has_extension(filename, "png") {
        return "image/png".into();
    }
    if has_extension(filename, "jpg") || has_extension(filename, "jpeg") {
        return "image/jpeg".into();
    }
    if has_extension(filename, "webp") {
        return "image/webp".into();
    }
    if has_extension(filename, "txt") {
        return "text/plain".into();
    }

    mime
}

fn has_extension(filename: &str, ext: &str) -> bool {
    std::path::Path::new(filename)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// Pick best expense account id from merchant/description hints.
#[must_use]
pub fn match_expense_account(accounts: &[Account], hints: &str) -> Option<AccountId> {
    match_account_of_type(accounts, AccountType::Expense, hints, EXPENSE_KEYWORDS)
}

/// Pick best income account (sales/services, freelance, other).
#[must_use]
pub fn match_income_account(accounts: &[Account], hints: &str) -> Option<AccountId> {
    match_account_of_type(accounts, AccountType::Income, hints, INCOME_KEYWORDS)
}

const EXPENSE_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "utilities",
        &[
            "utilit",
            "electric",
            "water",
            "gas",
            "power",
            "dei",
            "ρεύμα",
            "kwh",
            "zenith",
            "zeniθ",
            "εκκαθαριστ",
            "ηλεκτρ",
            "αέριο",
            "αεριο",
            "ngs",
            "φ.α",
            "έναντι",
        ],
    ),
    ("bills", &["bill", "invoice", "receipt"]),
    ("housing", &["rent", "mortgage", "housing"]),
    (
        "subscription",
        &["netflix", "spotify", "subscription", "saas"],
    ),
    ("food", &["food", "grocery", "supermarket", "restaurant"]),
    (
        "transport",
        &["fuel", "uber", "taxi", "transport", "parking"],
    ),
    (
        "software",
        &["software", "github", "aws", "cloud", "security", "program"],
    ),
    ("health", &["pharma", "doctor", "health", "clinic"]),
    ("tax", &["tax", "vat", "irs"]),
];

const INCOME_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "sales",
        &[
            "sales",
            "service",
            "security",
            "advise",
            "consult",
            "παροχ",
            "τιμολ",
        ],
    ),
    ("freelance", &["freelance", "project"]),
    ("salary", &["salary", "payroll", "wage"]),
];

fn match_account_of_type(
    accounts: &[Account],
    ty: AccountType,
    hints: &str,
    keywords: &[(&str, &[&str])],
) -> Option<AccountId> {
    let hints = hints.to_lowercase();
    let pool: Vec<_> = accounts
        .iter()
        .filter(|a| a.is_active && a.account_type == ty)
        .collect();

    for (account_hint, words) in keywords {
        if words.iter().any(|w| hints.contains(*w))
            && let Some(acc) = pool.iter().find(|a| {
                let name = a.name.to_lowercase();
                name.contains(account_hint)
                    || (*account_hint == "sales"
                        && (name.contains("sales")
                            || name.contains("service")
                            || name.contains("other")))
            })
        {
            return Some(acc.id);
        }
    }

    pool.iter()
        .find(|a| a.name.to_lowercase().contains("other"))
        .or_else(|| pool.first())
        .map(|a| a.id)
}

/// First active asset (bank/cash) for payments.
#[must_use]
pub fn match_wallet_account(accounts: &[Account]) -> Option<AccountId> {
    let assets: Vec<_> = accounts
        .iter()
        .filter(|a| a.is_active && a.account_type == AccountType::Asset)
        .collect();
    for hint in ["checking", "bank", "cash"] {
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().contains(hint))
        {
            return Some(a.id);
        }
    }
    assets.first().map(|a| a.id)
}

/// Bills payable / AP liability.
#[must_use]
pub fn match_payable_account(accounts: &[Account]) -> Option<AccountId> {
    let liab: Vec<_> = accounts
        .iter()
        .filter(|a| a.is_active && a.account_type == AccountType::Liability)
        .collect();
    for hint in ["bills payable", "accounts payable", "payable"] {
        if let Some(a) = liab
            .iter()
            .find(|a| a.name.to_ascii_lowercase().contains(hint))
        {
            return Some(a.id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_document_file_gates_size_name_and_mime() {
        assert!(validate_document_file("a.pdf", "application/pdf", 1_000).is_ok());
        assert!(validate_document_file("a.pdf", "application/pdf", 0).is_err());
        assert!(
            validate_document_file("a.pdf", "application/pdf", 20 * 1024 * 1024 * 1024).is_err(),
            "oversize must fail from metadata alone"
        );
        assert!(validate_document_file("  ", "application/pdf", 1_000).is_err());
        assert!(validate_document_file("a.exe", "application/x-msdownload", 1_000).is_err());
    }
}
