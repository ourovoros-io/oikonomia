//! Persist document blobs inside the encrypted vault database.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{Account, AccountId, AccountType, EntityId, JournalEntryId};
use crate::error::{Error, Result};
use crate::ledger::list_accounts;
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
    /// Linked entry if posted.
    pub entry_id: Option<JournalEntryId>,
    /// Original filename.
    pub filename: String,
    /// MIME type.
    pub mime_type: String,
    /// Byte length.
    pub size_bytes: i64,
}

/// Max upload size (8 MiB) — keeps vault lean and model latency reasonable.
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

/// Store a document blob (already protected by SQLCipher).
///
/// # Errors
///
/// Validation (size/mime) or DB errors.
pub fn save_document(
    conn: &Connection,
    entity_id: EntityId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> Result<DocumentMeta> {
    if data.is_empty() {
        return Err(Error::Validation("empty file".into()));
    }
    if data.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Validation(format!(
            "file too large (max {} MB)",
            MAX_DOCUMENT_BYTES / (1024 * 1024)
        )));
    }

    let name = filename.trim();
    if name.is_empty() {
        return Err(Error::Validation("filename is required".into()));
    }

    let mime = resolve_mime(mime_type, name);
    if !is_allowed_mime(&mime) {
        return Err(Error::Validation(
            "unsupported file type — use PDF, PNG, JPEG, WebP, or plain text".into(),
        ));
    }

    let id = DocumentId::new();
    let created = now_utc_string();

    conn.execute(
        "
        INSERT INTO documents (
            id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at, analysis_json
        ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, NULL)
        ",
        rusqlite::params![
            id.0.to_string(),
            entity_id.0.to_string(),
            name,
            mime,
            data.len() as i64,
            data,
            created,
        ],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id: None,
        filename: name.to_owned(),
        mime_type: mime,
        size_bytes: data.len() as i64,
    })
}

/// Attach a document to a posted journal entry.
///
/// # Errors
///
/// Not found or DB error.
pub fn link_document_to_entry(
    conn: &Connection,
    document_id: DocumentId,
    entry_id: JournalEntryId,
) -> Result<()> {
    let n = conn
        .execute(
            "UPDATE documents SET entry_id = ?1 WHERE id = ?2",
            rusqlite::params![entry_id.0.to_string(), document_id.0.to_string()],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if n == 0 {
        return Err(Error::NotFound("document".into()));
    }
    Ok(())
}

/// Load raw bytes for analysis (used when re-running extraction).
///
/// # Errors
///
/// Not found or DB error.
#[allow(dead_code)]
pub fn load_document_bytes(conn: &Connection, id: DocumentId) -> Result<(DocumentMeta, Vec<u8>)> {
    conn.query_row(
        "
        SELECT id, entity_id, entry_id, filename, mime_type, size_bytes, data
        FROM documents WHERE id = ?1
        ",
        [id.0.to_string()],
        |row| {
            let id = DocumentId(parse_uuid(&row.get::<_, String>(0)?).map_err(|e| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )))
            })?);
            let entity_id = EntityId(parse_uuid(&row.get::<_, String>(1)?).map_err(|e| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )))
            })?);
            let entry_s: Option<String> = row.get(2)?;
            let entry_id = match entry_s {
                Some(s) => Some(JournalEntryId(parse_uuid(&s).map_err(|e| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    )))
                })?)),
                None => None,
            };
            let meta = DocumentMeta {
                id,
                entity_id,
                entry_id,
                filename: row.get(3)?,
                mime_type: row.get(4)?,
                size_bytes: row.get(5)?,
            };
            let data: Vec<u8> = row.get(6)?;
            Ok((meta, data))
        },
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound("document".into()),
        other => Error::Io(other.to_string()),
    })
}

/// Save analysis JSON snapshot on the document row.
pub fn save_analysis_json(conn: &Connection, id: DocumentId, json: &str) -> Result<()> {
    conn.execute(
        "UPDATE documents SET analysis_json = ?1 WHERE id = ?2",
        rusqlite::params![json, id.0.to_string()],
    )
    .map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

/// Active accounts useful for auto-matching.
pub fn suggest_accounts_for_entity(conn: &Connection, entity_id: EntityId) -> Result<Vec<Account>> {
    list_accounts(conn, entity_id)
}

fn is_allowed_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/pdf"
            | "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/webp"
            | "text/plain"
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

    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".pdf") {
        return "application/pdf".into();
    }
    if lower.ends_with(".png") {
        return "image/png".into();
    }
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        return "image/jpeg".into();
    }
    if lower.ends_with(".webp") {
        return "image/webp".into();
    }
    if lower.ends_with(".txt") {
        return "text/plain".into();
    }

    mime
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
    ("subscription", &["netflix", "spotify", "subscription", "saas"]),
    ("food", &["food", "grocery", "supermarket", "restaurant"]),
    ("transport", &["fuel", "uber", "taxi", "transport", "parking"]),
    ("software", &["software", "github", "aws", "cloud", "security", "program"]),
    ("health", &["pharma", "doctor", "health", "clinic"]),
    ("tax", &["tax", "vat", "irs"]),
];

const INCOME_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "sales",
        &["sales", "service", "security", "advise", "consult", "παροχ", "τιμολ"],
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
        if words.iter().any(|w| hints.contains(*w)) {
            if let Some(acc) = pool.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.contains(account_hint)
                    || (*account_hint == "sales"
                        && (n.contains("sales") || n.contains("service") || n.contains("other")))
            }) {
                return Some(acc.id);
            }
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
