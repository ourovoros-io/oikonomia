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
//! [`archive_entity`] deletes nothing; it only stamps `archived_at`. The
//! operations do not treat an archived entity alike:
//!
//! - [`list_entities`] leaves it out, and its name no longer counts as taken.
//! - [`update_entity`], [`archive_entity`] and
//!   [`create_account`](crate::ledger::create_account) report it as
//!   [`Error::NotFound`].
//! - [`get_entity`], [`count_entities`] and [`delete_entity`] treat it like any
//!   other entity. So do the reports and the cash flow series, which look the
//!   entity up through [`get_entity`], and the recurring templates, which
//!   only check that the row exists.
//! - Posting an entry checks the entry's accounts and never the entity, so it
//!   is accepted as well.

use crate::coa::template_accounts;
use crate::db::{collect_rows, corrupt_column, read_column, stored_id};
use crate::domain::{Account, AccountId, ChartTemplate, CurrencyCode, Entity, EntityId};
use crate::error::{DatabaseContext, Error, NameField, Resource, Result, ValidationError};
use crate::ledger::balance::account_type_str;
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
    let mut stmt = conn
        .prepare(
            "
            SELECT id, name, base_currency, fiscal_year_start_month, chart_template
            FROM entities
            WHERE archived_at IS NULL
            ORDER BY fold(name), name
            ",
        )
        .database("list entities")?;

    let rows = stmt
        .query_map([], |row| Ok(map_entity(row)))
        .database("list entities")?;

    collect_rows("list entities", rows)
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
        |row| Ok(map_entity(row)),
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
/// for. There is no function that undoes it.
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
            chart_template_str(input.chart_template),
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
            parent_id: None,
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

    let deleted = conn
        .execute("DELETE FROM entities WHERE id = ?1", [&entity_id])
        .database("delete entity")?;

    if deleted == 0 {
        return Err(Error::NotFound(Resource::Entity));
    }

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
/// [`create_account`](crate::ledger::accounts::create_account).
///
/// # Errors
///
/// [`Error::Database`] on database errors.
fn insert_account_row(conn: &Connection, account: &Account) -> Result<()> {
    conn.execute(
        "
        INSERT INTO accounts (
            id, entity_id, code, name, account_type, parent_id,
            is_active, is_system, sort_order
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        rusqlite::params![
            account.id.to_string(),
            account.entity_id.to_string(),
            account.code,
            account.name,
            account_type_str(account.account_type),
            account.parent_id.map(|parent| parent.to_string()),
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
fn map_entity(row: &rusqlite::Row<'_>) -> Result<Entity> {
    let id = stored_id("entities.id", &read_column::<String>(row, 0)?)?;
    let chart_template = parse_chart_template(&read_column::<String>(row, 4)?)?;

    // Reports derive the fiscal year from this number, so one outside the
    // calendar is refused here instead of shifting every year boundary.
    let stored_month: i64 = read_column(row, 3)?;
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
    let stored_currency: String = read_column(row, 2)?;
    let base_currency = stored_currency.parse().map_err(|_| {
        corrupt_column(
            "entities.base_currency",
            format_args!("not a currency code: {stored_currency}"),
        )
    })?;

    Ok(Entity {
        id,
        name: read_column(row, 1)?,
        base_currency,
        fiscal_year_start_month,
        chart_template,
    })
}

/// Returns the text `template` is stored as in `entities.chart_template`.
///
/// The strings are part of the vault format; [`parse_chart_template`] reads
/// them back.
fn chart_template_str(template: ChartTemplate) -> &'static str {
    match template {
        ChartTemplate::Personal => "personal",
        ChartTemplate::Company => "company",
        ChartTemplate::Blank => "blank",
    }
}

/// Parses the text [`chart_template_str`] writes.
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
