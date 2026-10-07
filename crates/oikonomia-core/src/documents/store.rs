//! Document storage in the vault, and the keyword matcher that picks an
//! account for a document.
//!
//! The two halves share this file and nothing else.
//!
//! # Storage
//!
//! A document is a row of the `documents` table inside the `SQLCipher`
//! database, so its bytes are encrypted at rest with everything else and no
//! file sits on disk beside the vault. A document belongs to one book
//! (`entity_id`), is linked to one journal entry (`entry_id`), and has a
//! filename that is unique within its book.
//!
//! [`validate_document_file`] decides what may be stored from the name, type
//! and size alone, before any byte is read. [`save_document`],
//! [`attach_document`] and [`post_simple_entry_with_document`] write,
//! [`list_documents`] and [`get_document`] read, and [`delete_document`]
//! removes. Both readers go through [`map_document_meta`]: a row the
//! application wrote and cannot read back is reported as a corrupt vault, not
//! as a mistake of the caller.
//!
//! # Account matcher
//!
//! [`match_expense_account`] and [`match_income_account`] choose the account a
//! document is filed under. Their input is a hint: the merchant and
//! description the invoice reader produced, worded in English whatever the
//! language of the application.
//!
//! 1. The hint is lowercased and tested against a keyword table
//!    ([`EXPENSE_KEYWORDS`] or [`INCOME_KEYWORDS`]), topic by topic, in table
//!    order. A keyword matches on word boundaries only ([`Keyword`]).
//! 2. A topic that matches is turned into chart codes by the book's template
//!    ([`document_topic_codes`]). The first active account of the right type
//!    that carries one of those codes is the answer.
//! 3. A topic the chart has no account for is skipped, and the next topic
//!    that matches is tried.
//! 4. When no topic is left, the template's catch-all topic is used, and
//!    after that the first active account of the type.
//!
//! Accounts are found by code and type, never by name, so a renamed or
//! translated account is matched the same. A blank book has no template
//! codes, so it always gets the first active account of the type.
//!
//! Table order decides between topics whenever a hint names two of them.
//! [`EXPENSE_KEYWORDS`] lists the constraints on that order.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::coa::{DocumentTopic, document_topic_codes};
use crate::db::{read_column, stored_id};
use crate::default_accounts::{account_by_codes, first_of_type};
use crate::documents::store::Keyword::{Prefix, Unit, Word};
use crate::domain::define_id;
use crate::domain::{Account, AccountId, AccountType, ChartTemplate, EntityId, JournalEntryId};
use crate::error::{DatabaseContext, Error, NameField, Resource, Result, ValidationError};
use crate::ledger::{
    PostSimpleEntry, PostedEntryView, get_entry, list_accounts, post_simple_entry_unchecked,
};
use crate::util::now_utc_string;

define_id! {
    /// The primary key of a stored document.
    DocumentId
}

/// A stored document without its bytes, for lists and for the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    /// The document's id.
    pub id: DocumentId,
    /// The book the document belongs to.
    pub entity_id: EntityId,
    /// The journal entry the document is linked to. Every document has one.
    pub entry_id: JournalEntryId,
    /// The filename as given at upload, trimmed. Unique within the book.
    pub filename: String,
    /// The MIME type as [`resolve_mime`] resolved it at upload.
    pub mime_type: String,
    /// Length of the stored bytes.
    pub size_bytes: i64,
    /// Creation instant as the app-wide `unix:<seconds>` ordering key
    /// (see `now_utc_string`); not a display date — the UI formats it.
    pub created_at: String,
    /// Description of the linked journal entry, read in the same query so a
    /// list can show it without a second one.
    pub entry_description: String,
}

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

/// Validates a candidate document before its bytes are loaded or stored.
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
        return Err(ValidationError::FileEmpty.into());
    }
    if size_bytes > MAX_DOCUMENT_BYTES as u64 {
        return Err(ValidationError::FileTooLarge {
            max_mb: MAX_DOCUMENT_MEGABYTES,
        }
        .into());
    }
    if filename.trim().is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::Filename,
        }
        .into());
    }
    if !is_allowed_mime(mime) {
        return Err(ValidationError::FileTypeUnsupported.into());
    }
    Ok(())
}

/// Stores a document linked to `entry_id`.
///
/// The filename is trimmed and the MIME type resolved with [`resolve_mime`]
/// before either is checked or stored.
///
/// # Errors
///
/// - [`Error::Validation`]: the file is empty, larger than
///   [`MAX_DOCUMENT_BYTES`], has a blank name or an unsupported type
///   ([`validate_document_file`]); the entry belongs to another book
///   ([`ValidationError::WrongBook`]); or the book already has a document of
///   that name ([`ValidationError::NameTaken`]).
/// - [`Error::NotFound`]: no entry has `entry_id`.
/// - [`Error::VaultCorrupt`]: the entry's stored row cannot be read back.
/// - [`Error::Database`]: any other database failure.
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
        return Err(ValidationError::WrongBook.into());
    }

    let clash: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM documents WHERE entity_id = ?1 AND filename = ?2",
            rusqlite::params![entity_id.to_string(), name],
            |row| row.get(0),
        )
        .database("check document name is free")?;
    if clash > 0 {
        return Err(ValidationError::NameTaken {
            name: name.to_owned(),
        }
        .into());
    }

    // Validation caps the size at `MAX_DOCUMENT_BYTES`, so the length always
    // fits an i64.
    let size_bytes = i64::try_from(data.len()).unwrap_or(i64::MAX);

    let id = DocumentId::generate();
    let created = now_utc_string();

    conn.execute(
        "
        INSERT INTO documents (
            id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at,
            analysis_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)
        ",
        rusqlite::params![
            id.to_string(),
            entity_id.to_string(),
            entry_id.to_string(),
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
        ValidationError::NameTaken {
            name: name.to_owned(),
        }
        .into()
    } else {
        Error::database("insert document", err)
    }
}

/// Validates and stores a document linked to an existing entry.
///
/// No analysis runs here: reading a document belongs to the drop-zone flow.
///
/// # Errors
///
/// The errors of [`save_document`]. The entry is looked up first, so a
/// missing entry ([`Error::NotFound`]) or one in another book
/// ([`ValidationError::WrongBook`]) is reported before the file is checked.
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
        return Err(ValidationError::WrongBook.into());
    }

    save_document(conn, entity_id, entry_id, filename, mime_type, data)
}

/// Posts a simple entry and stores its document in one transaction.
///
/// Any failure rolls the entry back with the document, a duplicate filename
/// included, so this path never leaves a document without its entry or an
/// entry without its document.
///
/// # Errors
///
/// - Every error of [`post_simple_entry`](crate::ledger::post_simple_entry).
/// - Every error of [`save_document`].
/// - [`Error::NotFound`] from [`save_analysis_json`], if the row written a
///   moment earlier in the same transaction cannot be found.
/// - [`Error::Database`] when the transaction cannot be opened or committed.
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
    let transaction = conn
        .unchecked_transaction()
        .database("begin entry post with document")?;

    let view = post_simple_entry_unchecked(&transaction, input)?;
    let meta = save_document(
        &transaction,
        input.entity_id,
        view.entry.id,
        filename,
        mime_type,
        data,
    )?;
    if let Some(json) = analysis_json {
        save_analysis_json(&transaction, meta.id, json)?;
    }

    transaction
        .commit()
        .database("commit entry post with document")?;
    Ok((view, meta))
}

/// The columns every metadata query selects, in the order
/// [`map_document_meta`] reads them. `d` is `documents` and `je` the
/// `journal_entries` row it is joined to.
const DOCUMENT_META_COLUMNS: &str = "d.id, d.entity_id, d.entry_id, d.filename, d.mime_type, \
                                     d.size_bytes, d.created_at, je.description";

/// Reads the [`DOCUMENT_META_COLUMNS`] of `row`, which start at column 0.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] when a stored id is not a UUID or a column holds
/// a value of the wrong type: the application wrote the row, so the damage
/// is in the vault and not in what the caller passed. [`Error::Database`] for any
/// other driver failure.
fn map_document_meta(row: &rusqlite::Row<'_>) -> Result<DocumentMeta> {
    let id = stored_id("documents.id", &read_column::<String>(row, 0)?)?;
    let entity_id = stored_id("documents.entity_id", &read_column::<String>(row, 1)?)?;
    let entry_id = stored_id("documents.entry_id", &read_column::<String>(row, 2)?)?;

    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id,
        filename: read_column(row, 3)?,
        mime_type: read_column(row, 4)?,
        size_bytes: read_column(row, 5)?,
        created_at: read_column(row, 6)?,
        entry_description: read_column(row, 7)?,
    })
}

/// Lists a book's documents, newest first, without their bytes.
///
/// Documents created within the same second (the resolution of the stored
/// creation time) are ordered by insertion, latest first.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`]: a stored row cannot be read back.
/// - [`Error::Database`]: any other database failure.
pub fn list_documents(conn: &Connection, entity_id: EntityId) -> Result<Vec<DocumentMeta>> {
    // Order by created_at DESC, rowid DESC for deterministic insertion-recency order.
    // now_utc_string has 1-second granularity, so multiple documents saved in the same
    // second may tie on created_at; rowid tie-break ensures deterministic order.
    let mut statement = conn
        .prepare(&format!(
            "
            SELECT {DOCUMENT_META_COLUMNS}
            FROM documents d
            JOIN journal_entries je ON je.id = d.entry_id
            WHERE d.entity_id = ?1
            ORDER BY d.created_at DESC, d.rowid DESC
            "
        ))
        .database("list documents")?;

    let rows = statement
        .query_map([entity_id.to_string()], |row| Ok(map_document_meta(row)))
        .database("list documents")?;

    let mut documents = Vec::new();
    for row in rows {
        documents.push(row.database("list documents")??);
    }
    Ok(documents)
}

/// Returns one document's metadata and its bytes.
///
/// # Errors
///
/// - [`Error::NotFound`]: no document has this id.
/// - [`Error::VaultCorrupt`]: the stored row cannot be read back.
/// - [`Error::Database`]: any other database failure.
pub fn get_document(conn: &Connection, id: DocumentId) -> Result<(DocumentMeta, Vec<u8>)> {
    // The blob follows the eight metadata columns.
    const DATA_COLUMN: usize = 8;

    conn.query_row(
        &format!(
            "
            SELECT {DOCUMENT_META_COLUMNS}, d.data
            FROM documents d
            JOIN journal_entries je ON je.id = d.entry_id
            WHERE d.id = ?1
            "
        ),
        [id.to_string()],
        |row| {
            Ok(map_document_meta(row)
                .and_then(|meta| Ok((meta, read_column::<Vec<u8>>(row, DATA_COLUMN)?))))
        },
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::Document),
        other => Error::database("read document", other),
    })?
}

/// Permanently removes a document. The entry it was linked to is untouched.
///
/// # Errors
///
/// - [`Error::NotFound`]: no document has this id.
/// - [`Error::Database`]: the database failed.
pub fn delete_document(conn: &Connection, id: DocumentId) -> Result<()> {
    let deleted = conn
        .execute("DELETE FROM documents WHERE id = ?1", [id.to_string()])
        .database("delete document")?;
    if deleted == 0 {
        return Err(Error::NotFound(Resource::Document));
    }
    Ok(())
}

/// Stores the analysis a document was posted with, as JSON, on its row.
///
/// The text is stored as given; it is not parsed or checked here.
///
/// # Errors
///
/// - [`Error::NotFound`]: no document has this id.
/// - [`Error::Database`]: the database failed.
pub fn save_analysis_json(conn: &Connection, id: DocumentId, json: &str) -> Result<()> {
    let updated = conn
        .execute(
            "UPDATE documents SET analysis_json = ?1 WHERE id = ?2",
            rusqlite::params![json, id.to_string()],
        )
        .database("save document analysis")?;

    if updated == 0 {
        return Err(Error::NotFound(Resource::Document));
    }
    Ok(())
}

/// The book's active accounts, for matching a document to an account.
///
/// An archived account is left out, so a suggestion never points at one.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`]: a stored account row cannot be read back.
/// - [`Error::Database`]: any other database failure.
pub fn suggest_accounts_for_entity(conn: &Connection, entity_id: EntityId) -> Result<Vec<Account>> {
    let mut accounts = list_accounts(conn, entity_id)?;
    accounts.retain(|account| account.is_active);
    Ok(accounts)
}

/// Whether `mime` is a type the vault stores: PDF, PNG, JPEG (also spelled
/// `image/jpg`), WebP or plain text.
///
/// The comparison is exact, so the caller passes a lowercased type without
/// parameters such as `; charset=utf-8`.
fn is_allowed_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/pdf" | "image/png" | "image/jpeg" | "image/jpg" | "image/webp" | "text/plain"
    )
}

/// The MIME type to store for a file, from the type the webview reports and
/// the filename.
///
/// A reported type the vault stores is kept, with `image/jpg` rewritten to
/// `image/jpeg`. Otherwise the extension decides: a file dropped into a
/// desktop webview often arrives with an empty type. When neither is
/// recognised the reported type is returned trimmed and lowercased, and
/// [`validate_document_file`] then rejects it.
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

/// Whether `filename` ends in `extension` (given without the dot), in any
/// letter case.
pub(super) fn has_extension(filename: &str, extension: &str) -> bool {
    std::path::Path::new(filename)
        .extension()
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

/// Picks the expense account a document most likely belongs to.
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

/// Picks the income account a document most likely belongs to (sales, freelance,
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

/// A keyword table: topics in the order they are tried, each with the words
/// that point at it.
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

/// Income topics in the order they are tried, by the rule of
/// [`EXPENSE_KEYWORDS`]: Sales, Freelance, Salary.
///
/// No hint in the tests names two income topics, so no test depends on this
/// order.
///
/// `παροχ` and `τιμολ` are the stems of "Παροχή Υπηρεσιών" (provision of
/// services) and "Τιμολόγιο" (invoice), the heading of a Greek sales invoice.
///
/// `security` and `advise` are the two words of the line item on the sample
/// sales invoice in the corpus (`greek_sales_invoice.txt`: "security
/// advise"). They describe that one issuer's service, not sales in general.
/// The sample itself does not need them: the hint of a sales invoice is its
/// customer and a generated title, and there `consult` in the customer's name
/// selects Sales. They can match only when the description of an income
/// document is a line item.
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

/// Picks the account of `account_type` that `hints` point at, by the four
/// steps in the module documentation.
///
/// `hints` may be in any letter case. Returns `None` only when the book has
/// no active account of the type.
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
    use crate::error::VaultCorruption;

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
            matches!(
                document_insert_error(&err, "other.pdf"),
                Error::Database { .. }
            ),
            "{err}"
        );
    }

    const STORED_DOCUMENT_ID: &str = "11111111-1111-4111-8111-111111111111";
    const STORED_ENTITY_ID: &str = "22222222-2222-4222-8222-222222222222";

    /// A database holding one document whose `entry_id` is not an id, and
    /// the journal entry that text joins to.
    fn database_with_a_damaged_document() -> Connection {
        let conn = Connection::open_in_memory().expect("an in-memory database must open");
        conn.execute_batch(
            "CREATE TABLE journal_entries (id TEXT PRIMARY KEY NOT NULL, description TEXT NOT NULL);
             CREATE TABLE documents (
                id TEXT PRIMARY KEY NOT NULL,
                entity_id TEXT NOT NULL,
                entry_id TEXT NOT NULL,
                filename TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                data BLOB NOT NULL,
                created_at TEXT NOT NULL
             );
             INSERT INTO journal_entries (id, description) VALUES ('damaged', 'Rent');",
        )
        .expect("the tables must be created");
        conn.execute(
            "INSERT INTO documents
                (id, entity_id, entry_id, filename, mime_type, size_bytes, data, created_at)
             VALUES (?1, ?2, 'damaged', 'bill.pdf', 'application/pdf', 1, x'00', 'unix:1')",
            [STORED_DOCUMENT_ID, STORED_ENTITY_ID],
        )
        .expect("the document row must be inserted");
        conn
    }

    #[test]
    fn a_stored_id_that_does_not_parse_is_a_corrupt_vault_not_a_caller_mistake() {
        let conn = database_with_a_damaged_document();
        let damaged = Error::VaultCorrupt(VaultCorruption::Column {
            column: "documents.entry_id".into(),
            detail: "not an id: damaged".into(),
        });

        let entity_id = STORED_ENTITY_ID.parse::<EntityId>().unwrap();
        assert_eq!(
            list_documents(&conn, entity_id).map(|documents| documents.len()),
            Err(damaged.clone())
        );

        let id = STORED_DOCUMENT_ID.parse::<DocumentId>().unwrap();
        assert_eq!(
            get_document(&conn, id).map(|(meta, _)| meta.filename),
            Err(damaged)
        );
    }

    #[test]
    fn a_stored_size_of_the_wrong_type_is_a_corrupt_vault() {
        let conn = database_with_a_damaged_document();
        conn.execute(
            "UPDATE documents SET entry_id = ?1, size_bytes = 'large'",
            [STORED_ENTITY_ID],
        )
        .unwrap();
        conn.execute("UPDATE journal_entries SET id = ?1", [STORED_ENTITY_ID])
            .unwrap();

        let id = STORED_DOCUMENT_ID.parse::<DocumentId>().unwrap();
        let read = get_document(&conn, id).map(|(meta, _)| meta.filename);

        assert!(
            matches!(
                &read,
                Err(Error::VaultCorrupt(VaultCorruption::Column { column, .. }))
                    if column == "size_bytes"
            ),
            "{read:?}"
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
    fn the_personal_chart_files_each_topic_under_its_own_account() {
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
    fn the_company_chart_sends_a_topic_it_has_no_account_for_to_the_catch_all() {
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
    fn tax_documents_resolve_to_taxes_and_syntax_matches_nothing() {
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
        // Titles in the form the invoice reader generates.
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
