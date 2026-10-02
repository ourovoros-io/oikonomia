//! Persist document blobs inside the encrypted vault database.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::coa::{DocumentTopic, document_topic_codes};
use crate::default_accounts::{account_by_codes, first_of_type};
use crate::domain::{Account, AccountId, AccountType, ChartTemplate, EntityId, JournalEntryId};
use crate::error::{Error, Result, ValidationError};
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
    /// Linked journal entry description (for list decoration without a second query).
    pub entry_description: String,
}

/// Max upload size (8 MiB) — keeps vault lean and model latency reasonable.
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

/// [`MAX_DOCUMENT_BYTES`] in whole megabytes, as shown to the user.
const MAX_DOCUMENT_MEGABYTES: u64 = 8;

// The two limits must never drift apart.
const _: () = assert!(MAX_DOCUMENT_BYTES as u64 == MAX_DOCUMENT_MEGABYTES * 1024 * 1024);

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
        return Err(Error::Validation(ValidationError::FileEmpty));
    }
    if size_bytes > MAX_DOCUMENT_BYTES as u64 {
        return Err(Error::Validation(ValidationError::FileTooLarge {
            max_mb: MAX_DOCUMENT_MEGABYTES,
        }));
    }
    if filename.trim().is_empty() {
        return Err(Error::Validation(ValidationError::NameRequired {
            field: "filename",
        }));
    }
    if !is_allowed_mime(mime) {
        return Err(Error::Validation(ValidationError::FileTypeUnsupported));
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

    let entry = get_entry(conn, entry_id)?;
    if entry.entry.entity_id != entity_id {
        return Err(Error::Validation(ValidationError::WrongBook));
    }

    let clash: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM documents WHERE entity_id = ?1 AND filename = ?2",
            rusqlite::params![entity_id.0.to_string(), name],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if clash > 0 {
        return Err(Error::Validation(ValidationError::NameTaken {
            name: name.to_owned(),
        }));
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
    .map_err(|err| {
        let text = err.to_string();
        if matches!(
            err.sqlite_error_code(),
            Some(rusqlite::ErrorCode::ConstraintViolation)
        ) && text.contains("UNIQUE")
        {
            Error::Validation(ValidationError::NameTaken {
                name: name.to_owned(),
            })
        } else {
            Error::Io(text)
        }
    })?;

    let entry_description = entry.entry.description;

    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id,
        filename: name.to_owned(),
        mime_type: mime,
        size_bytes,
        created_at: created,
        entry_description,
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
        return Err(Error::Validation(ValidationError::WrongBook));
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

type MetaColumns = (String, String, String, String, String, i64, String, String);

fn meta_from_columns(raw: MetaColumns) -> Result<DocumentMeta> {
    let (id_s, entity_s, entry_s, filename, mime_type, size_bytes, created_at, entry_description) =
        raw;
    Ok(DocumentMeta {
        id: DocumentId(parse_uuid(&id_s)?),
        entity_id: EntityId(parse_uuid(&entity_s)?),
        entry_id: JournalEntryId(parse_uuid(&entry_s)?),
        filename,
        mime_type,
        size_bytes,
        created_at,
        entry_description,
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
            SELECT d.id, d.entity_id, d.entry_id, d.filename, d.mime_type, d.size_bytes,
                   d.created_at, je.description
            FROM documents d
            JOIN journal_entries je ON je.id = d.entry_id
            WHERE d.entity_id = ?1
            ORDER BY d.created_at DESC, d.rowid DESC
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
                row.get::<_, String>(7)?,
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
            SELECT d.id, d.entity_id, d.entry_id, d.filename, d.mime_type, d.size_bytes,
                   d.created_at, je.description, d.data
            FROM documents d
            JOIN journal_entries je ON je.id = d.entry_id
            WHERE d.id = ?1
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
                        row.get::<_, String>(7)?,
                    ),
                    row.get::<_, Vec<u8>>(8)?,
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

/// Pick the expense account a document most likely belongs to.
///
/// The merchant and description hints choose a topic; the topic is mapped to a
/// seeded account by template code, so the account's name never matters. With
/// no recognised topic the template's catch-all is used, then the first active
/// expense account.
#[must_use]
pub fn match_expense_account(
    template: ChartTemplate,
    accounts: &[Account],
    hints: &str,
) -> Option<AccountId> {
    match_account_of_type(
        template,
        accounts,
        AccountType::Expense,
        hints,
        EXPENSE_KEYWORDS,
        DocumentTopic::OtherExpense,
    )
}

/// Pick the income account a document most likely belongs to (sales, freelance,
/// salary), by the same rule as [`match_expense_account`].
#[must_use]
pub fn match_income_account(
    template: ChartTemplate,
    accounts: &[Account],
    hints: &str,
) -> Option<AccountId> {
    match_account_of_type(
        template,
        accounts,
        AccountType::Income,
        hints,
        INCOME_KEYWORDS,
        DocumentTopic::OtherIncome,
    )
}

/// Words in a document's text that point at a topic, in the order topics are tried.
type TopicKeywords = [(DocumentTopic, &'static [&'static str])];

const EXPENSE_KEYWORDS: &TopicKeywords = &[
    (
        DocumentTopic::Utilities,
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
    (DocumentTopic::Bills, &["bill", "invoice", "receipt"]),
    (DocumentTopic::Housing, &["rent", "mortgage", "housing"]),
    (
        DocumentTopic::Subscription,
        &["netflix", "spotify", "subscription", "saas"],
    ),
    (
        DocumentTopic::Food,
        &["food", "grocery", "supermarket", "restaurant"],
    ),
    (
        DocumentTopic::Transport,
        &["fuel", "uber", "taxi", "transport", "parking"],
    ),
    (
        DocumentTopic::Software,
        &["software", "github", "aws", "cloud", "security", "program"],
    ),
    (
        DocumentTopic::Health,
        &["pharma", "doctor", "health", "clinic"],
    ),
    (DocumentTopic::Tax, &["tax", "vat", "irs"]),
];

const INCOME_KEYWORDS: &TopicKeywords = &[
    (
        DocumentTopic::Sales,
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
    (DocumentTopic::Freelance, &["freelance", "project"]),
    (DocumentTopic::Salary, &["salary", "payroll", "wage"]),
];

fn match_account_of_type(
    template: ChartTemplate,
    accounts: &[Account],
    account_type: AccountType,
    hints: &str,
    keywords: &TopicKeywords,
    catch_all: DocumentTopic,
) -> Option<AccountId> {
    let hints = hints.to_lowercase();

    // The first topic the text points at that the chart has an account for.
    for (topic, words) in keywords {
        if !words.iter().any(|word| hints.contains(*word)) {
            continue;
        }

        let codes = document_topic_codes(template, *topic);
        if let Some(account) = account_by_codes(accounts, account_type, codes) {
            return Some(account.id);
        }
    }

    let codes = document_topic_codes(template, catch_all);

    account_by_codes(accounts, account_type, codes)
        .or_else(|| first_of_type(accounts, account_type))
        .map(|account| account.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_accounts::{code_of_for_tests, seeded_chart_for_tests};

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

    /// Code of the expense account suggested for `hints`.
    fn expense_code(template: ChartTemplate, accounts: &[Account], hints: &str) -> Option<String> {
        code_of_for_tests(accounts, match_expense_account(template, accounts, hints))
    }

    /// Code of the income account suggested for `hints`.
    fn income_code(template: ChartTemplate, accounts: &[Account], hints: &str) -> Option<String> {
        code_of_for_tests(accounts, match_income_account(template, accounts, hints))
    }

    /// Every hint the matcher is exercised with, for both charts.
    const EXPENSE_HINTS: &[&str] = &[
        "dei electricity",
        "monthly invoice",
        "rent for march",
        "netflix",
        "supermarket",
        "uber ride",
        "github cloud",
        "pharmacy",
        "vat payment",
        "something unrecognised",
        "",
    ];

    const INCOME_HINTS: &[&str] = &[
        "consulting services",
        "freelance project",
        "monthly salary",
        "something unrecognised",
        "",
    ];

    #[test]
    fn english_personal_chart_matches_as_before() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let template = ChartTemplate::Personal;

        assert_eq!(
            expense_code(template, &accounts, "dei electricity").as_deref(),
            Some("5300")
        );
        assert_eq!(
            expense_code(template, &accounts, "monthly invoice").as_deref(),
            Some("5350")
        );
        assert_eq!(
            expense_code(template, &accounts, "rent for march").as_deref(),
            Some("5000")
        );
        assert_eq!(
            expense_code(template, &accounts, "netflix").as_deref(),
            Some("5500")
        );
        assert_eq!(
            expense_code(template, &accounts, "supermarket").as_deref(),
            Some("5100")
        );
        assert_eq!(
            expense_code(template, &accounts, "uber ride").as_deref(),
            Some("5200")
        );
        assert_eq!(
            expense_code(template, &accounts, "pharmacy").as_deref(),
            Some("5400")
        );
        assert_eq!(
            expense_code(template, &accounts, "vat payment").as_deref(),
            Some("5700")
        );
        // No personal account covers software, so it falls to the catch-all.
        assert_eq!(
            expense_code(template, &accounts, "github cloud").as_deref(),
            Some("5900")
        );
        assert_eq!(
            expense_code(template, &accounts, "something unrecognised").as_deref(),
            Some("5900")
        );

        assert_eq!(
            income_code(template, &accounts, "consulting services").as_deref(),
            Some("4900")
        );
        assert_eq!(
            income_code(template, &accounts, "freelance project").as_deref(),
            Some("4100")
        );
        assert_eq!(
            income_code(template, &accounts, "monthly salary").as_deref(),
            Some("4000")
        );
        assert_eq!(
            income_code(template, &accounts, "something unrecognised").as_deref(),
            Some("4900")
        );
    }

    #[test]
    fn english_company_chart_matches_as_before() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Company, false);
        let template = ChartTemplate::Company;

        assert_eq!(
            expense_code(template, &accounts, "github cloud").as_deref(),
            Some("5300")
        );
        assert_eq!(
            expense_code(template, &accounts, "vat payment").as_deref(),
            Some("5700")
        );
        // The company chart has no utilities, bills or housing account.
        assert_eq!(
            expense_code(template, &accounts, "dei electricity").as_deref(),
            Some("5900")
        );
        assert_eq!(
            expense_code(template, &accounts, "rent for march").as_deref(),
            Some("5900")
        );

        assert_eq!(
            income_code(template, &accounts, "consulting services").as_deref(),
            Some("4000")
        );
        assert_eq!(
            income_code(template, &accounts, "monthly salary").as_deref(),
            Some("4900")
        );
    }

    /// One hint per expense topic, each naming no other topic's word, with the
    /// code the personal chart gives it. Software has no personal account, so
    /// it takes the catch-all.
    const PERSONAL_EXPENSE_PINS: &[(&str, &str)] = &[
        ("electric", "5300"),
        ("invoice", "5350"),
        ("rent", "5000"),
        ("netflix", "5500"),
        ("grocery", "5100"),
        ("parking", "5200"),
        ("github", "5900"),
        ("clinic", "5400"),
        ("vat", "5700"),
        ("something unrecognised", "5900"),
    ];

    /// The same for the company chart. Only software, tax and the catch-all
    /// have a company account; every other topic falls to the catch-all.
    const COMPANY_EXPENSE_PINS: &[(&str, &str)] = &[
        ("electric", "5900"),
        ("invoice", "5900"),
        ("rent", "5900"),
        ("netflix", "5900"),
        ("grocery", "5900"),
        ("parking", "5900"),
        ("github", "5300"),
        ("clinic", "5900"),
        ("vat", "5700"),
        ("something unrecognised", "5900"),
    ];

    const PERSONAL_INCOME_PINS: &[(&str, &str)] = &[
        ("consulting", "4900"),
        ("freelance", "4100"),
        ("payroll", "4000"),
        ("something unrecognised", "4900"),
    ];

    const COMPANY_INCOME_PINS: &[(&str, &str)] = &[
        ("consulting", "4000"),
        ("freelance", "4900"),
        ("payroll", "4900"),
        ("something unrecognised", "4900"),
    ];

    fn assert_pins(
        template: ChartTemplate,
        expense_pins: &[(&str, &str)],
        income_pins: &[(&str, &str)],
    ) {
        for rename in [false, true] {
            let accounts = seeded_chart_for_tests(template, rename);

            for (hints, code) in expense_pins {
                assert_eq!(
                    expense_code(template, &accounts, hints).as_deref(),
                    Some(*code),
                    "{template:?} expense for {hints:?} (renamed: {rename})",
                );
            }
            for (hints, code) in income_pins {
                assert_eq!(
                    income_code(template, &accounts, hints).as_deref(),
                    Some(*code),
                    "{template:?} income for {hints:?} (renamed: {rename})",
                );
            }
        }
    }

    #[test]
    fn every_topic_on_the_personal_chart_is_pinned_by_code() {
        assert_pins(
            ChartTemplate::Personal,
            PERSONAL_EXPENSE_PINS,
            PERSONAL_INCOME_PINS,
        );
    }

    #[test]
    fn every_topic_on_the_company_chart_is_pinned_by_code() {
        assert_pins(
            ChartTemplate::Company,
            COMPANY_EXPENSE_PINS,
            COMPANY_INCOME_PINS,
        );
    }

    #[test]
    fn renamed_charts_match_the_same_accounts_as_english_ones() {
        for template in [ChartTemplate::Personal, ChartTemplate::Company] {
            let english = seeded_chart_for_tests(template, false);
            let renamed = seeded_chart_for_tests(template, true);

            for hints in EXPENSE_HINTS {
                assert_eq!(
                    expense_code(template, &renamed, hints),
                    expense_code(template, &english, hints),
                    "{template:?} expense for {hints:?}",
                );
            }
            for hints in INCOME_HINTS {
                assert_eq!(
                    income_code(template, &renamed, hints),
                    income_code(template, &english, hints),
                    "{template:?} income for {hints:?}",
                );
            }
        }
    }

    #[test]
    fn a_deactivated_topic_account_falls_through_to_the_next_choice() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);
        for account in &mut accounts {
            if account.code == "5300" {
                account.is_active = false;
            }
        }

        // Utilities is gone, so the catch-all "Other" takes the document.
        assert_eq!(
            expense_code(ChartTemplate::Personal, &accounts, "dei electricity").as_deref(),
            Some("5900")
        );
    }

    /// Code of the active account of `account_type` with the lowest sort
    /// order, then code: what a book with no seeded codes must suggest.
    fn first_code_of_type(accounts: &[Account], account_type: AccountType) -> Option<String> {
        accounts
            .iter()
            .filter(|account| account.is_active && account.account_type == account_type)
            .min_by_key(|account| (account.sort_order, account.code.clone()))
            .map(|account| account.code.clone())
    }

    #[test]
    fn a_blank_book_suggests_the_first_account_of_the_type_whatever_it_is_called() {
        let template = ChartTemplate::Blank;
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);
        // A blank book's accounts are the user's own: none carries a seeded
        // code, so the words in their names must not steer the choice.
        for (index, account) in accounts.iter_mut().enumerate() {
            account.code = format!("U{index:03}");
        }

        let expected_expense = first_code_of_type(&accounts, AccountType::Expense);
        let expected_income = first_code_of_type(&accounts, AccountType::Income);
        assert!(expected_expense.is_some() && expected_income.is_some());

        assert_eq!(
            expense_code(template, &accounts, "dei electricity"),
            expected_expense
        );
        assert_eq!(
            income_code(template, &accounts, "monthly salary"),
            expected_income
        );
    }

    #[test]
    fn a_book_with_no_account_of_the_type_suggests_nothing() {
        let accounts: Vec<Account> = seeded_chart_for_tests(ChartTemplate::Personal, false)
            .into_iter()
            .filter(|account| account.account_type == AccountType::Asset)
            .collect();

        assert_eq!(
            match_expense_account(ChartTemplate::Personal, &accounts, "electricity"),
            None
        );
        assert_eq!(
            match_income_account(ChartTemplate::Personal, &accounts, "salary"),
            None
        );
    }
}
