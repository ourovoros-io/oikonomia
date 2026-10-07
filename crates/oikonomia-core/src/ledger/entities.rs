//! Entities: the separate sets of books a vault holds.
//!
//! An entity is one set of books, for a person, a household or a company.
//! Every account, journal entry, document and recurring template belongs to
//! exactly one entity, and nothing is shared between two of them.
//!
//! # Creating
//!
//! [`create_entity`] inserts the entity and seeds its chart of accounts from
//! a template in one transaction, so a book never exists with half a chart.
//!
//! # Archiving and deleting
//!
//! [`delete_entity`] removes an entity and everything that belongs to it, in
//! one transaction and in an order the foreign keys allow.
//!
//! [`archive_entity`] deletes nothing; it only stamps `archived_at`. One rule
//! then holds for the entity: **it is readable everywhere and never
//! writable.**
//!
//! | Operation on an archived entity | Outcome |
//! |---------------------------------|---------|
//! | A read | answers as before |
//! | A write that takes the entity | [`Error::NotFound`] |
//! | [`update_entity`], [`archive_entity`] | [`Error::NotFound`] |
//! | [`unarchive_entity`] | makes it active again, unless its name is taken |
//! | [`delete_entity`] | deletes it |
//!
//! The reads are [`get_entity`], the reports, the cash flow series, the
//! journal export, and the lists of the entity's accounts, entries,
//! templates and documents. The writes that take the entity are posting an
//! entry, creating or updating a template, creating an account, importing
//! CSV rows and attaching a document; each calls
//! [`ensure_writable_entity`], which reports the entity as if it did not
//! exist.
//!
//! Every entry is inserted in one place, which makes the check, so it also
//! covers the operations that post on the caller's behalf: voiding or
//! correcting an entry, setting an opening balance, and posting from a
//! template.
//!
//! Two things follow from being archived without being a read or a write:
//! [`list_entities`] leaves the entity out, which is how the app hides it,
//! and its name no longer counts as taken. [`count_entities`] counts it, and
//! [`list_archived_entities`] lists it.
//!
//! # Un-archiving
//!
//! [`unarchive_entity`] clears `archived_at`, and the entity is an active one
//! again in every respect: writable, listed by [`list_entities`], its name
//! taken. Nothing else about it was changed by being archived, so nothing
//! else is restored.
//!
//! The name is the one thing that can stand in the way. While the entity was
//! archived its name was free, and another entity may have taken it. Two
//! active entities never share a name, so the un-archive is then refused
//! with [`ValidationError::NameTaken`] and the entity stays archived; the
//! other entity has to give the name up first. An archived entity cannot be
//! renamed out of the way, because a rename is a write.
//!
//! The two operations mirror each other: [`archive_entity`] reports an
//! entity that is already archived as [`Error::NotFound`], and
//! [`unarchive_entity`] reports one that is not archived the same way.
//!
//! The writes that name a record and no entity do not make the check:
//! updating or archiving an account, hiding an entry, deleting a template or
//! a document, and storing the analysis of a document.

use crate::coa::template_accounts;
use crate::db::{collect_rows, corrupt_column, read_column, stored_id};
use crate::domain::{Account, AccountId, ChartTemplate, CurrencyCode, Entity, EntityId};
use crate::error::{DatabaseContext, Error, NameField, Resource, Result, ValidationError};
use crate::prefs::Locale;
use crate::util::now_utc_string;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Month;

/// Input for [`create_entity`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntity {
    /// Name of the book. Surrounding whitespace is trimmed; it must not be
    /// empty, and no other entity that is not archived may have the same name
    /// in any letter case.
    pub name: String,
    /// ISO 4217 code of the currency every amount of the entity is in: three
    /// ASCII letters in any case, stored in capitals.
    pub base_currency: String,
    /// Template the chart of accounts is seeded from. `Blank` seeds none.
    pub chart_template: ChartTemplate,
    /// Month the fiscal year starts in, 1 (January) to 12. `None` is January.
    pub fiscal_year_start_month: Option<u8>,
}

/// Lists the entities that are not archived, ordered by name.
///
/// Names are ordered by their case fold (the `fold` SQL function every vault
/// connection registers), so capitals do not sort ahead of small letters in
/// any script; names that fold alike keep code point order.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored entity that does not parse.
/// - [`Error::Database`] on database errors.
pub fn list_entities(conn: &Connection) -> Result<Vec<Entity>> {
    query_entities(
        conn,
        "list entities",
        "
        SELECT id, name, base_currency, fiscal_year_start_month, chart_template
        FROM entities
        WHERE archived_at IS NULL
        ORDER BY fold(name), name
        ",
    )
}

/// Lists the archived entities, ordered by name as [`list_entities`] orders
/// the others.
///
/// Every entity is in exactly one of the two lists. An archived entity is
/// read-only; [`unarchive_entity`] moves it back to the other list.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored entity that does not parse.
/// - [`Error::Database`] on database errors.
pub fn list_archived_entities(conn: &Connection) -> Result<Vec<Entity>> {
    query_entities(
        conn,
        "list archived entities",
        "
        SELECT id, name, base_currency, fiscal_year_start_month, chart_template
        FROM entities
        WHERE archived_at IS NOT NULL
        ORDER BY fold(name), name
        ",
    )
}

/// Returns one entity, archived or not.
///
/// Reports use this as their existence check, so a report on an archived
/// entity still works.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entity.
/// - [`Error::VaultCorrupt`] for a stored entity whose id, chart template or
///   fiscal year start month does not parse.
/// - [`Error::Database`] on database errors.
pub fn get_entity(conn: &Connection, id: EntityId) -> Result<Entity> {
    conn.query_row(
        "
        SELECT id, name, base_currency, fiscal_year_start_month, chart_template
        FROM entities WHERE id = ?1
        ",
        [id.to_string()],
        |row| Ok(map_entity("read entity", row)),
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::Entity),
        other => Error::database("read entity", other),
    })?
}

/// Creates an entity and seeds its chart of accounts from the template,
/// atomically.
///
/// The seeded account names are written in `locale`, the language the app is
/// set to now. They are never rewritten if the language changes later. The
/// base currency is stored in capitals.
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for an empty name.
/// - [`ValidationError::CurrencyInvalid`] unless the base currency is three
///   ASCII letters.
/// - [`ValidationError::Internal`] for a fiscal year start month outside 1–12.
/// - [`ValidationError::NameTaken`] when an entity that is not archived has
///   the same name, compared without case.
/// - [`Error::VaultCorrupt`] when the entity does not parse on being read
///   back.
/// - [`Error::Database`] on database errors.
pub fn create_entity(conn: &Connection, input: &CreateEntity, locale: Locale) -> Result<Entity> {
    let tx = conn
        .unchecked_transaction()
        .database("begin entity creation")?;
    let entity = create_entity_in_tx(&tx, input, locale)?;
    tx.commit().database("commit entity creation")?;
    Ok(entity)
}

/// Counts the entities in the vault, archived ones included.
///
/// # Errors
///
/// [`Error::Database`] on database errors.
pub fn count_entities(conn: &Connection) -> Result<u64> {
    let count: i64 = conn
        .query_row("SELECT COUNT(1) FROM entities", [], |row| row.get(0))
        .database("count entities")?;
    u64::try_from(count).map_err(|_| Error::database("count entities", "the count is negative"))
}

/// Renames an entity.
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for an empty name.
/// - [`ValidationError::NameTaken`] when another entity that is not archived
///   has the same name, compared without case.
/// - [`Error::NotFound`] for an unknown or archived entity.
/// - [`Error::VaultCorrupt`] when the stored entity does not parse.
/// - [`Error::Database`] on database errors.
pub fn update_entity(conn: &Connection, id: EntityId, name: &str) -> Result<Entity> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::EntityName,
        }
        .into());
    }

    ensure_unique_name(conn, name, Some(id))?;

    // An archived entity is not renamed. It matches no row here and is
    // reported like an unknown one.
    let renamed = conn
        .execute(
            "UPDATE entities SET name = ?1 WHERE id = ?2 AND archived_at IS NULL",
            rusqlite::params![name, id.to_string()],
        )
        .database("rename entity")?;

    if renamed == 0 {
        return Err(Error::NotFound(Resource::Entity));
    }

    get_entity(conn, id)
}

/// Marks an entity as archived, deleting nothing.
///
/// The module documentation lists what an archived entity can still be used
/// for. [`unarchive_entity`] undoes it.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entity, or one that is already
///   archived.
/// - [`Error::Database`] on database errors.
pub fn archive_entity(conn: &Connection, id: EntityId) -> Result<()> {
    let archived = conn
        .execute(
            "UPDATE entities SET archived_at = ?1 WHERE id = ?2 AND archived_at IS NULL",
            rusqlite::params![now_utc_string(), id.to_string()],
        )
        .database("archive entity")?;

    if archived == 0 {
        return Err(Error::NotFound(Resource::Entity));
    }
    Ok(())
}

/// Makes an archived entity active again: writable, listed by
/// [`list_entities`], and its name taken.
///
/// The name check and the change run in one transaction.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entity, or one that is not archived.
///   [`archive_entity`] treats an entity that is already archived the same
///   way.
/// - [`ValidationError::NameTaken`] when an entity that is not archived now
///   has the same name, compared without case. The entity stays archived.
/// - [`Error::Database`] on database errors.
pub fn unarchive_entity(conn: &Connection, id: EntityId) -> Result<()> {
    let tx = conn
        .unchecked_transaction()
        .database("begin entity un-archiving")?;
    unarchive_entity_in_tx(&tx, id)?;
    tx.commit().database("commit entity un-archiving")?;
    Ok(())
}

/// Deletes an entity for good, with its accounts, journal entries, documents
/// and recurring templates, in one transaction.
///
/// An archived entity is deleted like any other.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entity.
/// - [`Error::Database`] on database errors. Nothing is deleted in that case.
pub fn delete_entity(conn: &Connection, id: EntityId) -> Result<()> {
    let tx = conn
        .unchecked_transaction()
        .database("begin entity deletion")?;
    delete_entity_in_tx(&tx, id)?;
    tx.commit().database("commit entity deletion")?;
    Ok(())
}

/// Checks that `id` names an entity that may be written to: one that exists
/// and is not archived.
///
/// Every write path that takes an entity calls this before it writes; the
/// module documentation lists them. An archived entity is reported exactly
/// like an unknown one, so a caller cannot tell the two apart.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown or archived entity.
/// - [`Error::Database`] on database errors.
pub(crate) fn ensure_writable_entity(conn: &Connection, id: EntityId) -> Result<()> {
    let writable: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entities WHERE id = ?1 AND archived_at IS NULL",
            [id.to_string()],
            |row| row.get(0),
        )
        .database("check entity is writable")?;

    if writable == 0 {
        return Err(Error::NotFound(Resource::Entity));
    }
    Ok(())
}

/// Runs `sql`, which selects the columns [`map_entity`] reads and takes no
/// parameters, and maps every row.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored entity that does not parse.
/// - [`Error::Database`] on database errors, under `operation`.
fn query_entities(conn: &Connection, operation: &'static str, sql: &str) -> Result<Vec<Entity>> {
    let mut stmt = conn.prepare(sql).database(operation)?;

    let rows = stmt
        .query_map([], |row| Ok(map_entity(operation, row)))
        .database(operation)?;

    collect_rows(operation, rows)
}

/// Checks that the archived entity's name is still free and clears its
/// `archived_at`.
///
/// The caller owns the transaction: the check and the change are separate
/// statements.
///
/// # Errors
///
/// Those of [`unarchive_entity`].
fn unarchive_entity_in_tx(conn: &Connection, id: EntityId) -> Result<()> {
    let entity_id = id.to_string();

    let name: String = conn
        .query_row(
            "SELECT name FROM entities WHERE id = ?1 AND archived_at IS NOT NULL",
            [&entity_id],
            |row| row.get(0),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::Entity),
            other => Error::database("read archived entity name", other),
        })?;

    // The entity itself is archived and so never counts against its own
    // name; no entity needs excluding.
    ensure_unique_name(conn, &name, None)?;

    // The row is there and archived: it was read above, in the caller's
    // transaction.
    conn.execute(
        "UPDATE entities SET archived_at = NULL WHERE id = ?1",
        [&entity_id],
    )
    .database("un-archive entity")?;

    Ok(())
}

/// Validates `input`, inserts the entity and seeds its chart.
///
/// The caller owns the transaction: the entity row and each seeded account
/// are separate statements.
///
/// # Errors
///
/// Those of [`create_entity`].
fn create_entity_in_tx(conn: &Connection, input: &CreateEntity, locale: Locale) -> Result<Entity> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::EntityName,
        }
        .into());
    }

    let currency: CurrencyCode = input.base_currency.parse()?;

    let month = match input.fiscal_year_start_month {
        None => Month::January,
        Some(number) => Month::try_from(number).map_err(|_| ValidationError::Internal {
            detail: "fiscal_year_start_month must be 1-12".into(),
        })?,
    };

    ensure_unique_name(conn, name, None)?;

    let id = EntityId::generate();

    conn.execute(
        "
        INSERT INTO entities (
            id, name, base_currency, fiscal_year_start_month, chart_template, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        rusqlite::params![
            id.to_string(),
            name,
            currency.as_str(),
            u8::from(month),
            input.chart_template.identifier(),
            now_utc_string(),
        ],
    )
    .database("insert entity")?;

    for template_account in template_accounts(input.chart_template, locale) {
        let account = Account {
            id: AccountId::generate(),
            entity_id: id,
            code: template_account.code.to_owned(),
            name: template_account.name.to_owned(),
            account_type: template_account.account_type,
            is_active: true,
            is_system: template_account.is_system,
            sort_order: template_account.sort_order,
        };
        insert_account_row(conn, &account)?;
    }

    get_entity(conn, id)
}

/// Deletes an entity and every row that belongs to it.
///
/// The caller owns the transaction. The statements run in the order the
/// foreign keys allow: each table is emptied before the one it references.
///
/// # Errors
///
/// Those of [`delete_entity`].
fn delete_entity_in_tx(conn: &Connection, id: EntityId) -> Result<()> {
    let entity_id = id.to_string();

    // No `archived_at` test: an archived entity is deleted like any other.
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entities WHERE id = ?1",
            [&entity_id],
            |row| row.get(0),
        )
        .database("check entity exists")?;
    if exists == 0 {
        return Err(Error::NotFound(Resource::Entity));
    }

    // A voided entry and its reversal reference each other, so neither could
    // be deleted first while the links stand.
    conn.execute(
        "
        UPDATE journal_entries
        SET voided_by_entry_id = NULL
        WHERE entity_id = ?1
        ",
        [&entity_id],
    )
    .database("clear void links of entity")?;

    conn.execute(
        "
        DELETE FROM journal_lines
        WHERE entry_id IN (SELECT id FROM journal_entries WHERE entity_id = ?1)
        ",
        [&entity_id],
    )
    .database("delete journal lines of entity")?;

    // Documents reference journal entries, so they go before the entries.
    conn.execute("DELETE FROM documents WHERE entity_id = ?1", [&entity_id])
        .database("delete documents of entity")?;

    // Templates reference accounts, so they go before the accounts.
    conn.execute(
        "DELETE FROM recurring_templates WHERE entity_id = ?1",
        [&entity_id],
    )
    .database("delete recurring templates of entity")?;

    conn.execute(
        "DELETE FROM journal_entries WHERE entity_id = ?1",
        [&entity_id],
    )
    .database("delete journal entries of entity")?;

    conn.execute("DELETE FROM accounts WHERE entity_id = ?1", [&entity_id])
        .database("delete accounts of entity")?;

    // The row is there: it was counted above, in the caller's transaction.
    conn.execute("DELETE FROM entities WHERE id = ?1", [&entity_id])
        .database("delete entity")?;

    Ok(())
}

/// Checks that no entity that is not archived has `name`, compared without
/// case. `exclude` names the entity being renamed, which may keep its name.
///
/// Case folds for every letter through the `fold` SQL function, which every
/// vault connection registers; `SQLite`'s `lower()` would fold ASCII only
/// (<https://www.sqlite.org/lang_corefunc.html#lower>).
///
/// # Errors
///
/// - [`ValidationError::NameTaken`] when the name is in use.
/// - [`Error::Database`] on database errors.
fn ensure_unique_name(conn: &Connection, name: &str, exclude: Option<EntityId>) -> Result<()> {
    let count: i64 = match exclude {
        Some(id) => conn
            .query_row(
                "
                SELECT COUNT(1) FROM entities
                WHERE archived_at IS NULL
                  AND fold(name) = fold(?1)
                  AND id != ?2
                ",
                rusqlite::params![name, id.to_string()],
                |row| row.get(0),
            )
            .database("check entity name is free")?,
        None => conn
            .query_row(
                "
                SELECT COUNT(1) FROM entities
                WHERE archived_at IS NULL
                  AND fold(name) = fold(?1)
                ",
                [name],
                |row| row.get(0),
            )
            .database("check entity name is free")?,
    };

    if count > 0 {
        return Err(ValidationError::NameTaken {
            name: name.to_owned(),
        }
        .into());
    }

    Ok(())
}

/// Inserts `account` as given, with no validation.
///
/// Only for the accounts a chart template seeds, whose codes and names the
/// crate wrote itself. An account from user input goes through
/// [`create_account`](crate::ledger::accounts::create_account). Like that
/// function it does not write the `parent_id` column, which holds NULL.
///
/// # Errors
///
/// [`Error::Database`] on database errors.
fn insert_account_row(conn: &Connection, account: &Account) -> Result<()> {
    conn.execute(
        "
        INSERT INTO accounts (
            id, entity_id, code, name, account_type,
            is_active, is_system, sort_order
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        rusqlite::params![
            account.id.to_string(),
            account.entity_id.to_string(),
            account.code,
            account.name,
            account.account_type.identifier(),
            i32::from(account.is_active),
            i32::from(account.is_system),
            account.sort_order,
        ],
    )
    .database("insert account")?;
    Ok(())
}

/// Maps a row selected as `id, name, base_currency, fiscal_year_start_month,
/// chart_template`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when the id, the base currency,
/// the chart template or the fiscal year start month does not parse, or a
/// column has the wrong storage class.
fn map_entity(operation: &'static str, row: &rusqlite::Row<'_>) -> Result<Entity> {
    let id = stored_id("entities.id", &read_column::<String>(operation, row, 0)?)?;
    let chart_template = parse_chart_template(&read_column::<String>(operation, row, 4)?)?;

    // Reports derive the fiscal year from this number, so one outside the
    // calendar is refused here instead of shifting every year boundary.
    let stored_month: i64 = read_column(operation, row, 3)?;
    let fiscal_year_start_month = u8::try_from(stored_month)
        .ok()
        .and_then(|number| Month::try_from(number).ok())
        .ok_or_else(|| {
            corrupt_column(
                "entities.fiscal_year_start_month",
                format_args!("not a month: {stored_month}"),
            )
        })?;

    // The code is a key into the table of decimal digits and is handed to
    // the UI's number formatter, so text that is not a code is refused here
    // instead of being formatted with a guessed number of decimals.
    let stored_currency: String = read_column(operation, row, 2)?;
    let base_currency = stored_currency.parse().map_err(|_| {
        corrupt_column(
            "entities.base_currency",
            format_args!("not a currency code: {stored_currency}"),
        )
    })?;

    Ok(Entity {
        id,
        name: read_column(operation, row, 1)?,
        base_currency,
        fiscal_year_start_month,
        chart_template,
    })
}

/// Parses the text [`ChartTemplate::identifier`] writes into
/// `entities.chart_template`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `entities.chart_template` when `stored` is
/// none of those strings.
fn parse_chart_template(stored: &str) -> Result<ChartTemplate> {
    match stored {
        "personal" => Ok(ChartTemplate::Personal),
        "company" => Ok(ChartTemplate::Company),
        "blank" => Ok(ChartTemplate::Blank),
        other => Err(corrupt_column(
            "entities.chart_template",
            format_args!("unknown chart template: {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{ChartTemplate, parse_chart_template};

    #[test]
    fn a_chart_template_is_read_back_from_the_text_it_is_stored_as() {
        for template in [
            ChartTemplate::Personal,
            ChartTemplate::Company,
            ChartTemplate::Blank,
        ] {
            assert_eq!(parse_chart_template(template.identifier()), Ok(template));
        }
        assert_eq!(
            parse_chart_template("Personal").map_err(|error| error.code()),
            Err("vault_corrupt")
        );
    }
}
