//! Document storage in the vault.
//!
//! A document is a row of the `documents` table inside the `SQLCipher`
//! database, so its bytes are encrypted at rest with everything else and no
//! file sits on disk beside the vault. A document belongs to one book
//! (`entity_id`), is linked to one journal entry (`entry_id`), and has a
//! filename that is unique within its book.
//!
//! What may be stored is decided before anything is written, by
//! [`NewDocument::checked`]. [`attach_document`] and
//! [`post_simple_entry_with_document`] write, both through
//! [`insert_document`]; [`list_documents`] and [`get_document`] read, and
//! [`delete_document`] removes. Both readers go through
//! [`map_document_meta`]: a row the application wrote and cannot read back is
//! reported as a corrupt vault, not as a mistake of the caller.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::{read_column, stored_id};
use crate::documents::file::{CheckedDocument, NewDocument};
use crate::domain::{Account, EntityId, JournalEntry, JournalEntryId, define_id};
use crate::error::{DatabaseContext, Error, Resource, Result, ValidationError};
use crate::ledger::{
    PostSimpleEntry, PostedEntryView, ensure_writable_entity, get_entry, list_accounts,
    post_simple_entry_unchecked,
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
    /// The MIME type of the kind the file was resolved to at upload.
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

/// Validates and stores a document linked to an existing entry of the book
/// `entity_id`.
///
/// The file's name is trimmed and its kind resolved before either is checked
/// or stored. No analysis runs here: reading a document belongs to the
/// drop-zone flow.
///
/// # Errors
///
/// The entry is looked up first, so a missing entry or one in another book
/// is reported before the file is checked.
///
/// - [`Error::NotFound`]: no entry has `entry_id`, or the book is archived
///   and takes no more documents.
/// - [`Error::Validation`]: the entry belongs to another book
///   ([`ValidationError::WrongBook`]); the file is empty, larger than
///   [`MAX_DOCUMENT_BYTES`](crate::documents::MAX_DOCUMENT_BYTES), has a
///   blank name or an unsupported type ([`NewDocument::validate`]); or the
///   book already has a document of that name
///   ([`ValidationError::NameTaken`]).
/// - [`Error::VaultCorrupt`]: the entry's stored row cannot be read back.
/// - [`Error::Database`]: any other database failure.
pub fn attach_document(
    conn: &Connection,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    document: &NewDocument<'_>,
) -> Result<DocumentMeta> {
    let entry = get_entry(conn, entry_id)?.entry;
    if entry.entity_id != entity_id {
        return Err(ValidationError::WrongBook.into());
    }
    ensure_writable_entity(conn, entity_id)?;

    insert_document(conn, &entry, &document.checked()?)
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
/// - [`Error::Validation`] for a file that may not be stored
///   ([`NewDocument::validate`]) or whose name the book already uses
///   ([`ValidationError::NameTaken`]).
/// - [`Error::NotFound`] from [`save_analysis_json`], if the row written a
///   moment earlier in the same transaction cannot be found.
/// - [`Error::Database`] when the transaction cannot be opened or committed,
///   or the document cannot be written.
pub fn post_simple_entry_with_document(
    conn: &Connection,
    input: &PostSimpleEntry,
    document: &NewDocument<'_>,
    analysis_json: Option<&str>,
) -> Result<(PostedEntryView, DocumentMeta)> {
    let transaction = conn
        .unchecked_transaction()
        .database("begin entry post with document")?;

    let view = post_simple_entry_unchecked(&transaction, input)?;
    let meta = insert_document(&transaction, &view.entry, &document.checked()?)?;
    if let Some(json) = analysis_json {
        save_analysis_json(&transaction, meta.id, json)?;
    }

    transaction
        .commit()
        .database("commit entry post with document")?;
    Ok((view, meta))
}

/// Stores `document` linked to `entry`, which the caller has read from the
/// same connection.
///
/// # Errors
///
/// - [`Error::Validation`] with [`ValidationError::NameTaken`]: the entry's
///   book already has a document of that name.
/// - [`Error::Database`]: any other database failure.
fn insert_document(
    conn: &Connection,
    entry: &JournalEntry,
    document: &CheckedDocument<'_>,
) -> Result<DocumentMeta> {
    let CheckedDocument { name, kind, data } = *document;
    let entity_id = entry.entity_id;

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

    // A checked document is at most `MAX_DOCUMENT_BYTES` long, so the length
    // always fits an i64.
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
            entry.id.to_string(),
            name,
            kind.mime(),
            size_bytes,
            data,
            created,
        ],
    )
    .map_err(|err| document_insert_error(&err, name))?;

    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id: entry.id,
        filename: name.to_owned(),
        mime_type: kind.mime().to_owned(),
        size_bytes,
        created_at: created,
        entry_description: entry.description.clone(),
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
fn map_document_meta(operation: &'static str, row: &rusqlite::Row<'_>) -> Result<DocumentMeta> {
    let id = stored_id("documents.id", &read_column::<String>(operation, row, 0)?)?;
    let entity_id = stored_id(
        "documents.entity_id",
        &read_column::<String>(operation, row, 1)?,
    )?;
    let entry_id = stored_id(
        "documents.entry_id",
        &read_column::<String>(operation, row, 2)?,
    )?;

    Ok(DocumentMeta {
        id,
        entity_id,
        entry_id,
        filename: read_column(operation, row, 3)?,
        mime_type: read_column(operation, row, 4)?,
        size_bytes: read_column(operation, row, 5)?,
        created_at: read_column(operation, row, 6)?,
        entry_description: read_column(operation, row, 7)?,
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
        .query_map([entity_id.to_string()], |row| {
            Ok(map_document_meta("list documents", row))
        })
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
            Ok(map_document_meta("read document", row).and_then(|meta| {
                Ok((
                    meta,
                    read_column::<Vec<u8>>("read document", row, DATA_COLUMN)?,
                ))
            }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::VaultCorruption;

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
}
