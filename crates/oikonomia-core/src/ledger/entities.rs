//! Entity (book) CRUD.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use std::path::Path;

use crate::coa::template_accounts;
use crate::domain::{Account, AccountId, ChartTemplate, Entity, EntityId};
use crate::error::{Error, Result};
use crate::ledger::balance::account_type_str;
use crate::license::LicenseVerifier;
use crate::util::{now_utc_string, parse_uuid};

/// Input for creating a new entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntity {
    /// Display name.
    pub name: String,
    /// ISO 4217 currency code.
    pub base_currency: String,
    /// Template for starter accounts.
    pub chart_template: ChartTemplate,
    /// Fiscal year start month 1–12 (default 1).
    pub fiscal_year_start_month: Option<u8>,
}

/// List non-archived entities ordered by name.
///
/// # Errors
///
/// Returns DB errors.
pub fn list_entities(conn: &Connection) -> Result<Vec<Entity>> {
    let mut stmt = conn
        .prepare(
            "
            SELECT id, name, base_currency, fiscal_year_start_month, chart_template
            FROM entities
            WHERE archived_at IS NULL
            ORDER BY name COLLATE NOCASE
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map([], map_entity)
        .map_err(|err| Error::Io(err.to_string()))?;

    collect_rows(rows)
}

/// Fetch one entity by id.
///
/// # Errors
///
/// Not found or DB error.
pub fn get_entity(conn: &Connection, id: EntityId) -> Result<Entity> {
    conn.query_row(
        "
        SELECT id, name, base_currency, fiscal_year_start_month, chart_template
        FROM entities WHERE id = ?1
        ",
        [id.0.to_string()],
        map_entity,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound("entity".into()),
        other => Error::Io(other.to_string()),
    })
}

/// Create entity and seed chart of accounts from template, atomically.
///
/// # Errors
///
/// Validation or DB errors.
pub fn create_entity(conn: &Connection, input: &CreateEntity) -> Result<Entity> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let entity = create_entity_in_tx(&tx, input)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(entity)
}

/// Number of entities in the vault (including archived).
///
/// # Errors
///
/// DB errors.
pub fn count_entities(conn: &Connection) -> Result<u64> {
    let n: i64 = conn
        .query_row("SELECT COUNT(1) FROM entities", [], |row| row.get(0))
        .map_err(|err| Error::Io(err.to_string()))?;
    u64::try_from(n).map_err(|_| Error::Io("entity count overflow".into()))
}

/// [`create_entity`] after the license write gate and one-entity unlicensed cap.
///
/// # Errors
///
/// [`Error::LicenseExpired`], [`Error::LicenseEntityLimit`], or
/// [`create_entity`] errors.
pub fn create_entity_allowed(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    conn: &Connection,
    input: &CreateEntity,
) -> Result<Entity> {
    let count = count_entities(conn)?;
    crate::license::require_entity_create_allowed(data_dir, verifier, count)?;
    create_entity(conn, input)
}

fn create_entity_in_tx(conn: &Connection, input: &CreateEntity) -> Result<Entity> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(Error::Validation("entity name is required".into()));
    }

    let currency = input.base_currency.trim().to_uppercase();
    if currency.len() != 3 {
        return Err(Error::Validation(
            "base_currency must be a 3-letter ISO code".into(),
        ));
    }

    let month = input.fiscal_year_start_month.unwrap_or(1);
    if !(1..=12).contains(&month) {
        return Err(Error::Validation(
            "fiscal_year_start_month must be 1–12".into(),
        ));
    }

    ensure_unique_name(conn, name, None)?;

    let id = EntityId::new();
    let created = now_utc_string();
    let template_s = chart_template_str(input.chart_template);

    conn.execute(
        "
        INSERT INTO entities (id, name, base_currency, fiscal_year_start_month, chart_template, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        rusqlite::params![
            id.0.to_string(),
            name,
            currency,
            month,
            template_s,
            created,
        ],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    for tmpl in template_accounts(input.chart_template) {
        let account = Account {
            id: AccountId::new(),
            entity_id: id,
            code: tmpl.code.to_owned(),
            name: tmpl.name.to_owned(),
            account_type: tmpl.account_type,
            parent_id: None,
            is_active: true,
            is_system: tmpl.is_system,
            sort_order: tmpl.sort_order,
        };
        insert_account_row(conn, &account)?;
    }

    get_entity(conn, id)
}

/// Rename an entity.
///
/// # Errors
///
/// Not found, validation, or DB error.
pub fn update_entity(conn: &Connection, id: EntityId, name: &str) -> Result<Entity> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Validation("entity name is required".into()));
    }

    ensure_unique_name(conn, name, Some(id))?;

    let n = conn
        .execute(
            "UPDATE entities SET name = ?1 WHERE id = ?2 AND archived_at IS NULL",
            rusqlite::params![name, id.0.to_string()],
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    if n == 0 {
        return Err(Error::NotFound("entity".into()));
    }

    get_entity(conn, id)
}

/// Soft-archive an entity (kept for compatibility; prefer [`delete_entity`]).
///
/// # Errors
///
/// Not found or DB error.
pub fn archive_entity(conn: &Connection, id: EntityId) -> Result<()> {
    let n = conn
        .execute(
            "UPDATE entities SET archived_at = ?1 WHERE id = ?2 AND archived_at IS NULL",
            rusqlite::params![now_utc_string(), id.0.to_string()],
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    if n == 0 {
        return Err(Error::NotFound("entity".into()));
    }
    Ok(())
}

/// Permanently delete an entity and all of its accounts, documents, and
/// journal data in one transaction.
///
/// # Errors
///
/// Not found or DB error.
pub fn delete_entity(conn: &Connection, id: EntityId) -> Result<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    delete_entity_in_tx(&tx, id)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

fn delete_entity_in_tx(conn: &Connection, id: EntityId) -> Result<()> {
    let id_s = id.0.to_string();

    // Ensure it exists and is not already gone.
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entities WHERE id = ?1",
            [&id_s],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if exists == 0 {
        return Err(Error::NotFound("entity".into()));
    }

    // Break self-FK on voided_by, then cascade lines → entries → accounts → entity.
    conn.execute(
        "
        UPDATE journal_entries
        SET voided_by_entry_id = NULL
        WHERE entity_id = ?1
        ",
        [&id_s],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    conn.execute(
        "
        DELETE FROM journal_lines
        WHERE entry_id IN (SELECT id FROM journal_entries WHERE entity_id = ?1)
        ",
        [&id_s],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    // Documents reference journal_entries (entry_id FK), so they must go first.
    conn.execute("DELETE FROM documents WHERE entity_id = ?1", [&id_s])
        .map_err(|err| Error::Io(err.to_string()))?;

    conn.execute("DELETE FROM journal_entries WHERE entity_id = ?1", [&id_s])
        .map_err(|err| Error::Io(err.to_string()))?;

    conn.execute("DELETE FROM accounts WHERE entity_id = ?1", [&id_s])
        .map_err(|err| Error::Io(err.to_string()))?;

    let n = conn
        .execute("DELETE FROM entities WHERE id = ?1", [&id_s])
        .map_err(|err| Error::Io(err.to_string()))?;

    if n == 0 {
        return Err(Error::NotFound("entity".into()));
    }

    Ok(())
}

/// Case-insensitive unique name among non-archived entities.
fn ensure_unique_name(conn: &Connection, name: &str, exclude: Option<EntityId>) -> Result<()> {
    let count: i64 = match exclude {
        Some(id) => conn
            .query_row(
                "
                SELECT COUNT(1) FROM entities
                WHERE archived_at IS NULL
                  AND lower(name) = lower(?1)
                  AND id != ?2
                ",
                rusqlite::params![name, id.0.to_string()],
                |row| row.get(0),
            )
            .map_err(|err| Error::Io(err.to_string()))?,
        None => conn
            .query_row(
                "
                SELECT COUNT(1) FROM entities
                WHERE archived_at IS NULL
                  AND lower(name) = lower(?1)
                ",
                [name],
                |row| row.get(0),
            )
            .map_err(|err| Error::Io(err.to_string()))?,
    };

    if count > 0 {
        return Err(Error::Validation(format!(
            "an entity named \"{name}\" already exists"
        )));
    }

    Ok(())
}

fn insert_account_row(conn: &Connection, account: &Account) -> Result<()> {
    conn.execute(
        "
        INSERT INTO accounts (
            id, entity_id, code, name, account_type, parent_id,
            is_active, is_system, sort_order
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        rusqlite::params![
            account.id.0.to_string(),
            account.entity_id.0.to_string(),
            account.code,
            account.name,
            account_type_str(account.account_type),
            account.parent_id.map(|p| p.0.to_string()),
            i32::from(account.is_active),
            i32::from(account.is_system),
            account.sort_order,
        ],
    )
    .map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

fn map_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entity> {
    let id_s: String = row.get(0)?;
    let id = parse_uuid(&id_s).map_err(|e| row_err(0, &e))?;
    let template_s: String = row.get(4)?;
    let template = parse_chart_template(&template_s).map_err(|e| row_err(4, &e))?;
    let month_raw: i64 = row.get(3)?;
    let month = u8::try_from(month_raw)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, month_raw))?;

    Ok(Entity {
        id: EntityId(id),
        name: row.get(1)?,
        base_currency: row.get(2)?,
        fiscal_year_start_month: month,
        chart_template: template,
    })
}

fn row_err(col: usize, err: &Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        col,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            err.to_string(),
        )),
    )
}

fn chart_template_str(t: ChartTemplate) -> &'static str {
    match t {
        ChartTemplate::Personal => "personal",
        ChartTemplate::Company => "company",
        ChartTemplate::Blank => "blank",
    }
}

fn parse_chart_template(s: &str) -> Result<ChartTemplate> {
    match s {
        "personal" => Ok(ChartTemplate::Personal),
        "company" => Ok(ChartTemplate::Company),
        "blank" => Ok(ChartTemplate::Blank),
        other => Err(Error::VaultCorrupt(format!(
            "unknown chart template: {other}"
        ))),
    }
}

fn collect_rows<T, E>(rows: impl Iterator<Item = std::result::Result<T, E>>) -> Result<Vec<T>>
where
    E: std::fmt::Display,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|err| Error::Io(err.to_string()))?);
    }
    Ok(out)
}
