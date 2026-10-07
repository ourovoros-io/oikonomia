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

use self::Keyword::{Prefix, Unit, Word};

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
#[expect(
    clippy::too_many_arguments,
    reason = "the document's name, type and bytes are separate arguments; tracked for the API pass"
)]
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
            id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at,
            analysis_json
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
    .map_err(|err| document_insert_error(&err, name))?;

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

/// Maps a failed insert into `documents` to the error the caller sees.
///
/// The table's one `UNIQUE` constraint is `(entity_id, filename)`, so a
/// unique violation means the name is taken. Any other failure, a primary
/// key clash included, is a database error.
fn document_insert_error(err: &rusqlite::Error, name: &str) -> Error {
    let unique_violation = matches!(
        err,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    );

    if unique_violation {
        Error::Validation(ValidationError::NameTaken {
            name: name.to_owned(),
        })
    } else {
        Error::Io(err.to_string())
    }
}

/// Validate and store a document linked to an existing entry (no OCR —
/// analysis belongs to the drop-zone flow).
///
/// # Errors
///
/// [`Error::NotFound`] for a missing entry, [`Error::Validation`] for an
/// entry in a different book, an invalid file, or a duplicate filename.
#[expect(
    clippy::too_many_arguments,
    reason = "the document's name, type and bytes are separate arguments; tracked for the API pass"
)]
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
#[expect(
    clippy::too_many_arguments,
    reason = "the document's name, type and bytes are separate arguments; tracked for the API pass"
)]
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
/// [`Error::NotFound`] when no document has this id; DB errors otherwise.
pub fn save_analysis_json(conn: &Connection, id: DocumentId, json: &str) -> Result<()> {
    let updated = conn
        .execute(
            "UPDATE documents SET analysis_json = ?1 WHERE id = ?2",
            rusqlite::params![json, id.0.to_string()],
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    if updated == 0 {
        return Err(Error::NotFound("document".into()));
    }
    Ok(())
}

/// The book's active accounts, for matching a document to an account.
///
/// An archived account is left out, so a suggestion never points at one.
///
/// # Errors
///
/// DB errors.
pub fn suggest_accounts_for_entity(conn: &Connection, entity_id: EntityId) -> Result<Vec<Account>> {
    let mut accounts = list_accounts(conn, entity_id)?;
    accounts.retain(|account| account.is_active);
    Ok(accounts)
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

/// Whether `filename` ends in the extension `ext`, in any letter case.
pub(super) fn has_extension(filename: &str, ext: &str) -> bool {
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
pub(super) fn match_expense_account(
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
pub(super) fn match_income_account(
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

/// How a keyword must sit in the text to count as a match.
///
/// Matching is on word boundaries, never on bare substrings: a letter or digit
/// (Unicode-aware, so Greek and accented letters count) directly next to the
/// keyword on a checked side stops it matching. That is what keeps "tax" out
/// of "taxi" and "syntax". Inflected forms that a whole-word keyword would
/// miss are either listed as their own keywords or marked [`Keyword::Prefix`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keyword {
    /// The keyword is a whole word: no letter or digit on either side.
    Word(&'static str),
    /// The keyword is the stem of a word (`consult` for "consulting"): it must
    /// start a word, and any letters may follow.
    Prefix(&'static str),
    /// A unit that is written glued to a number (`kwh` in "150kwh"): no letter
    /// before it, so digits are fine, and no letter or digit after it.
    Unit(&'static str),
}

impl Keyword {
    /// The text the keyword looks for.
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::Word(text) | Self::Prefix(text) | Self::Unit(text) => text,
        }
    }

    /// Whether this keyword occurs in `lowercased_text` under its rule.
    ///
    /// The caller lowercases the text; keywords are written in lowercase.
    #[expect(
        clippy::string_slice,
        reason = "`match_indices` yields the offset of a match of `needle`, \
                  so both ends of the match are character boundaries"
    )]
    pub(crate) fn occurs_in(self, lowercased_text: &str) -> bool {
        let needle = self.text();

        for (start, _) in lowercased_text.match_indices(needle) {
            let before = lowercased_text[..start].chars().next_back();
            let after = lowercased_text[start + needle.len()..].chars().next();

            let starts_a_word = match self {
                Self::Word(_) | Self::Prefix(_) => !before.is_some_and(char::is_alphanumeric),
                Self::Unit(_) => !before.is_some_and(char::is_alphabetic),
            };
            let ends_a_word = match self {
                Self::Prefix(_) => true,
                Self::Word(_) | Self::Unit(_) => !after.is_some_and(char::is_alphanumeric),
            };

            if starts_a_word && ends_a_word {
                return true;
            }
        }

        false
    }
}

/// Words in a document's text that point at a topic, in the order topics are tried.
type TopicKeywords = [(DocumentTopic, &'static [Keyword])];

/// Expense topics in the order they are tried; the first topic whose keywords
/// occur in the hint, and that the chart has an account for, wins.
///
/// The order is: Utilities, Transport, Housing, Subscription, Food, Software,
/// Health, Bills, Tax. Three constraints fix it:
///
/// - Transport before Housing: a car rental, car hire or "rent a car" is
///   travel, but it contains "rental" or "rent", which are Housing words.
/// - Bills after every specific topic except Tax: Bills is the generic topic
///   for a bill, invoice or receipt that names nothing more specific, so a
///   "doctor bill", "clinic invoice", "taxi receipt" or "rent invoice" goes to
///   the topic it is about.
/// - Bills before Tax: "Tax invoice 42" and "VAT invoice" are ordinary
///   invoices that mention tax, not tax payments.
///
/// A [`Keyword::Prefix`] is used for a stem that real names build on
/// (Cloudflare, healthcare, Foodpanda, fuels); a plural made redundant by a
/// prefix is not listed. Compounds that hide the stem mid-word (iCloud, efood,
/// seafood, polyclinic, refuel) are listed as whole words.
const EXPENSE_KEYWORDS: &TopicKeywords = &[
    (
        DocumentTopic::Utilities,
        &[
            Prefix("utilit"),
            Prefix("electric"),
            Word("water"),
            Word("gas"),
            Word("power"),
            Word("dei"),
            Prefix("ρεύμα"),
            Unit("kwh"),
            Word("zenith"),
            Word("zeniθ"),
            Prefix("εκκαθαριστ"),
            Prefix("ηλεκτρ"),
            Prefix("αέριο"),
            Prefix("αεριο"),
            Word("ngs"),
            Word("φ.α"),
            Prefix("έναντι"),
        ],
    ),
    (
        DocumentTopic::Transport,
        &[
            Prefix("fuel"),
            Word("refuel"),
            Word("uber"),
            // A word, not a prefix: "taxidermy" is not travel.
            Word("taxi"),
            Word("taxis"),
            Word("taxibeat"),
            Prefix("transport"),
            Word("parking"),
            Word("car rental"),
            Word("car rentals"),
            Word("car hire"),
            Word("rent a car"),
        ],
    ),
    (
        DocumentTopic::Housing,
        &[
            Word("rent"),
            Word("rents"),
            Word("renting"),
            Word("rented"),
            Word("rental"),
            Word("rentals"),
            Word("mortgage"),
            Word("mortgages"),
            Word("housing"),
        ],
    ),
    (
        DocumentTopic::Subscription,
        &[
            Word("netflix"),
            Word("spotify"),
            Word("subscription"),
            Word("subscriptions"),
            Word("saas"),
        ],
    ),
    (
        DocumentTopic::Food,
        &[
            Prefix("food"),
            Word("efood"),
            Word("seafood"),
            Word("grocery"),
            Word("groceries"),
            Word("supermarket"),
            Word("supermarkets"),
            Word("restaurant"),
            Word("restaurants"),
        ],
    ),
    (
        DocumentTopic::Software,
        &[
            Word("software"),
            Word("github"),
            Word("aws"),
            Prefix("cloud"),
            Word("icloud"),
            Word("security"),
            Prefix("program"),
        ],
    ),
    (
        DocumentTopic::Health,
        &[
            Prefix("pharma"),
            Word("doctor"),
            Word("doctors"),
            Prefix("health"),
            Prefix("clinic"),
            Word("polyclinic"),
        ],
    ),
    (
        DocumentTopic::Bills,
        &[
            Word("bill"),
            Word("bills"),
            Word("billing"),
            Word("billed"),
            Word("invoice"),
            Word("invoices"),
            Word("invoiced"),
            Word("invoicing"),
            Word("receipt"),
            Word("receipts"),
        ],
    ),
    (
        DocumentTopic::Tax,
        &[
            Word("tax"),
            Word("taxes"),
            Word("taxation"),
            Word("vat"),
            Word("irs"),
        ],
    ),
];

const INCOME_KEYWORDS: &TopicKeywords = &[
    (
        DocumentTopic::Sales,
        &[
            Word("sales"),
            Word("service"),
            Word("services"),
            Word("security"),
            Word("advise"),
            Word("advised"),
            Word("advises"),
            Prefix("consult"),
            Prefix("παροχ"),
            Prefix("τιμολ"),
        ],
    ),
    (
        DocumentTopic::Freelance,
        &[Prefix("freelance"), Word("project"), Word("projects")],
    ),
    (
        DocumentTopic::Salary,
        &[
            Word("salary"),
            Word("salaries"),
            Word("payroll"),
            Word("wage"),
            Word("wages"),
        ],
    ),
];

#[expect(
    clippy::too_many_arguments,
    reason = "the keyword table and its catch-all are passed separately; tracked for the API pass"
)]
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
        if !words.iter().any(|word| word.occurs_in(&hints)) {
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

    /// The error of inserting `(id, filename)` twice into a table with the
    /// constraints of `documents`, differing only where `second` differs.
    fn clash_error(first: (&str, &str), second: (&str, &str)) -> Option<rusqlite::Error> {
        let conn = Connection::open_in_memory().ok()?;
        conn.execute_batch(
            "CREATE TABLE documents (
                id TEXT PRIMARY KEY NOT NULL,
                entity_id TEXT NOT NULL,
                filename TEXT NOT NULL,
                UNIQUE (entity_id, filename)
            );",
        )
        .ok()?;

        let insert = "INSERT INTO documents (id, entity_id, filename) VALUES (?1, 'book', ?2)";
        conn.execute(insert, [first.0, first.1]).ok()?;
        conn.execute(insert, [second.0, second.1]).err()
    }

    #[test]
    fn a_filename_clash_on_insert_is_a_taken_name() {
        let err = clash_error(("id-1", "bill.pdf"), ("id-2", "bill.pdf"));
        let err = err.expect("the second insert must fail");

        assert_eq!(
            document_insert_error(&err, "bill.pdf"),
            Error::Validation(ValidationError::NameTaken {
                name: "bill.pdf".into()
            })
        );
    }

    #[test]
    fn a_primary_key_clash_on_insert_is_not_a_taken_name() {
        let err = clash_error(("id-1", "bill.pdf"), ("id-1", "other.pdf"));
        let err = err.expect("the second insert must fail");

        assert!(
            matches!(document_insert_error(&err, "other.pdf"), Error::Io(_)),
            "{err}"
        );
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
        // The company chart has no utilities or bills account.
        assert_eq!(
            expense_code(template, &accounts, "dei electricity").as_deref(),
            Some("5900")
        );
        // It does have Rent.
        assert_eq!(
            expense_code(template, &accounts, "rent for march").as_deref(),
            Some("5200")
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

    /// The same for the company chart. Only housing (Rent), transport (Travel),
    /// software, tax and the catch-all have a company account; every other topic
    /// falls to the catch-all.
    const COMPANY_EXPENSE_PINS: &[(&str, &str)] = &[
        ("electric", "5900"),
        ("invoice", "5900"),
        ("rent", "5200"),
        ("netflix", "5900"),
        ("grocery", "5900"),
        ("parking", "5600"),
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
    fn a_taxi_document_is_transport_and_not_taxes() {
        let personal = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let company = seeded_chart_for_tests(ChartTemplate::Company, false);

        for hints in ["taxi", "Taxi receipt", "uber taxi fare"] {
            assert_eq!(
                expense_code(ChartTemplate::Personal, &personal, hints).as_deref(),
                Some("5200"),
                "personal chart, {hints:?}",
            );
            assert_eq!(
                expense_code(ChartTemplate::Company, &company, hints).as_deref(),
                Some("5600"),
                "company chart, {hints:?}",
            );
        }
    }

    #[test]
    fn tax_documents_still_resolve_to_taxes_and_syntax_matches_nothing() {
        let personal = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let company = seeded_chart_for_tests(ChartTemplate::Company, false);

        for hints in [
            "tax",
            "income tax",
            "VAT/tax return",
            "property tax",
            "tax:",
        ] {
            for (template, accounts) in [
                (ChartTemplate::Personal, &personal),
                (ChartTemplate::Company, &company),
            ] {
                assert_eq!(
                    expense_code(template, accounts, hints).as_deref(),
                    Some("5700"),
                    "{template:?}, {hints:?}",
                );
            }
        }

        // "syntax" names no topic, so the catch-all takes it.
        assert_eq!(
            expense_code(ChartTemplate::Personal, &personal, "syntax").as_deref(),
            Some("5900")
        );
        assert_eq!(
            expense_code(ChartTemplate::Company, &company, "syntax").as_deref(),
            Some("5900")
        );
    }

    #[test]
    fn the_company_chart_files_rent_and_transport_under_its_own_accounts() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Company, false);
        let template = ChartTemplate::Company;

        for hints in ["rent for march", "office lease rent", "mortgage", "housing"] {
            assert_eq!(
                expense_code(template, &accounts, hints).as_deref(),
                Some("5200"),
                "{hints:?}",
            );
        }
        for hints in ["fuel", "parking", "taxi", "train transport", "uber ride"] {
            assert_eq!(
                expense_code(template, &accounts, hints).as_deref(),
                Some("5600"),
                "{hints:?}",
            );
        }
    }

    /// Merchant and title hints with the expense code each chart must give:
    /// (hint, personal code, company code). Personal: Housing 5000, Food 5100,
    /// Transport 5200, Utilities 5300, Bills 5350, Health 5400, Subscription
    /// 5500, Tax 5700, Other 5900. Company: Rent 5200, Software 5300, Travel
    /// 5600, Tax 5700, Other 5900.
    const EXPENSE_HINT_CODES: &[(&str, &str, &str)] = &[
        // Stems that real names build on.
        ("Cloudflare", "5900", "5300"),
        ("iCloud", "5900", "5300"),
        ("Cloud storage", "5900", "5300"),
        ("healthcare", "5400", "5900"),
        ("clinical", "5400", "5900"),
        ("clinics", "5400", "5900"),
        ("polyclinic", "5400", "5900"),
        ("Foodpanda", "5100", "5900"),
        ("foods", "5100", "5900"),
        ("efood", "5100", "5900"),
        ("seafood", "5100", "5900"),
        ("fuels", "5200", "5600"),
        ("fueling", "5200", "5600"),
        ("transporter", "5200", "5600"),
        ("transportation", "5200", "5600"),
        ("refuel", "5200", "5600"),
        ("taxis", "5200", "5600"),
        ("Taxibeat", "5200", "5600"),
        ("billing", "5350", "5900"),
        ("billed", "5350", "5900"),
        ("invoiced", "5350", "5900"),
        ("invoicing", "5350", "5900"),
        ("renting", "5000", "5200"),
        ("rented", "5000", "5200"),
        // Car hire is travel, not rent.
        ("car rental", "5200", "5600"),
        ("car rentals", "5200", "5600"),
        ("Car rental invoice", "5200", "5600"),
        ("car hire", "5200", "5600"),
        ("rent a car", "5200", "5600"),
        ("Rent a Car — Invoice 7", "5200", "5600"),
        ("rent", "5000", "5200"),
        ("rental", "5000", "5200"),
        ("apartment rent bill", "5000", "5200"),
        ("Invoice 42 — Rent", "5000", "5200"),
        // Bills is the generic topic: specific topics win over it...
        ("pharmacy receipt", "5400", "5900"),
        ("doctor bill", "5400", "5900"),
        ("clinic invoice", "5400", "5900"),
        // ...but it wins over Tax: these are ordinary invoices.
        ("Tax invoice 42", "5350", "5700"),
        ("VAT invoice", "5350", "5700"),
        // Pinned to what they resolved to before the change.
        ("Electricity bill", "5300", "5900"),
        ("Gas bill", "5300", "5900"),
        ("Water bill", "5300", "5900"),
        ("Telecom bill", "5350", "5900"),
        ("ACME — Invoice 42", "5350", "5900"),
        ("Netflix — Invoice 42", "5500", "5900"),
        ("Restaurant Plaka — Invoice 12", "5100", "5900"),
        ("Taxi receipt", "5200", "5600"),
        ("Fuel receipt", "5200", "5600"),
        // Words that only contain a keyword name no topic.
        ("syntax", "5900", "5900"),
        ("taxidermy", "5900", "5900"),
        ("waterfall", "5900", "5900"),
        ("savings", "5900", "5900"),
        ("Holdings", "5900", "5900"),
        ("renovation", "5900", "5900"),
        ("private", "5900", "5900"),
        ("parent", "5900", "5900"),
        ("current account", "5900", "5900"),
        ("Laurent", "5900", "5900"),
        ("Huber GmbH", "5900", "5900"),
        ("laws", "5900", "5900"),
    ];

    #[test]
    fn merchant_names_compounds_and_car_hire_resolve_on_both_charts() {
        let personal = seeded_chart_for_tests(ChartTemplate::Personal, false);
        let company = seeded_chart_for_tests(ChartTemplate::Company, false);

        for (hints, personal_code, company_code) in EXPENSE_HINT_CODES {
            assert_eq!(
                expense_code(ChartTemplate::Personal, &personal, hints).as_deref(),
                Some(*personal_code),
                "personal chart, {hints:?}",
            );
            assert_eq!(
                expense_code(ChartTemplate::Company, &company, hints).as_deref(),
                Some(*company_code),
                "company chart, {hints:?}",
            );
        }
    }

    /// The first topic of `table` whose keywords occur in `text`.
    fn first_topic(table: &TopicKeywords, text: &str) -> Option<DocumentTopic> {
        let lowercased = text.to_lowercase();

        table
            .iter()
            .find(|(_, keywords)| {
                keywords
                    .iter()
                    .any(|keyword| keyword.occurs_in(&lowercased))
            })
            .map(|(topic, _)| *topic)
    }

    #[test]
    fn every_keyword_as_a_whole_word_resolves_to_its_topic() {
        for table in [EXPENSE_KEYWORDS, INCOME_KEYWORDS] {
            for (topic, keywords) in table {
                for keyword in *keywords {
                    let text = keyword.text();

                    for sentence in [
                        text.to_string(),
                        format!("paid the {text} today"),
                        format!("Ref: {text}, due"),
                        format!("a/{text}/b"),
                    ] {
                        assert_eq!(
                            first_topic(table, &sentence),
                            Some(*topic),
                            "{keyword:?} in {sentence:?}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_keyword_matches_inside_a_longer_unrelated_word() {
        for table in [EXPENSE_KEYWORDS, INCOME_KEYWORDS] {
            for (_, keywords) in table {
                for keyword in *keywords {
                    let text = keyword.text();

                    match keyword {
                        Keyword::Word(_) => {
                            for sentence in [
                                format!("{text}xyz"),
                                format!("xyz{text}"),
                                format!("xyz{text}xyz"),
                                format!("{text}7"),
                                format!("7{text}"),
                            ] {
                                assert!(
                                    !keyword.occurs_in(&sentence),
                                    "{keyword:?} must not match in {sentence:?}",
                                );
                            }
                        }
                        Keyword::Prefix(_) => {
                            // A stem matches the start of a longer word, never its middle.
                            assert!(keyword.occurs_in(&format!("{text}xyz")), "{keyword:?}");
                            assert!(keyword.occurs_in(&format!("a {text}xyz b")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("xyz{text}")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("7{text}")), "{keyword:?}");
                        }
                        Keyword::Unit(_) => {
                            // A unit may follow a number, but not a letter.
                            assert!(keyword.occurs_in(&format!("150{text}")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("xyz{text}")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("{text}xyz")), "{keyword:?}");
                            assert!(!keyword.occurs_in(&format!("{text}7")), "{keyword:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn words_that_only_contain_a_keyword_name_no_topic() {
        for text in ["syntax", "taxidermy", "waterloo", "parent", "mortgagee x"] {
            assert_eq!(first_topic(EXPENSE_KEYWORDS, text), None, "{text:?}");
        }
        assert_eq!(
            first_topic(EXPENSE_KEYWORDS, "taxi"),
            Some(DocumentTopic::Transport)
        );
        assert_eq!(
            first_topic(EXPENSE_KEYWORDS, "150kWh"),
            Some(DocumentTopic::Utilities)
        );
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
