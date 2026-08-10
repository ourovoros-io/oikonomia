//! Chart of accounts CRUD.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{Account, AccountId, AccountType, EntityId};
use crate::error::{Error, Result};
use crate::ledger::balance::{account_type_str, parse_account_type};
use crate::util::parse_uuid;

/// Create account input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccount {
    /// Owning entity.
    pub entity_id: EntityId,
    /// Code (unique per entity).
    pub code: String,
    /// Display name.
    pub name: String,
    /// Classification.
    pub account_type: AccountType,
    /// Sort order (optional).
    pub sort_order: Option<i32>,
}

/// Update account input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAccount {
    /// Account id.
    pub id: AccountId,
    /// New code.
    pub code: String,
    /// New name.
    pub name: String,
    /// Active flag.
    pub is_active: bool,
    /// Sort order.
    pub sort_order: i32,
}

/// List accounts for an entity (active and inactive), ordered by sort then code.
///
/// # Errors
///
/// DB errors.
pub fn list_accounts(conn: &Connection, entity_id: EntityId) -> Result<Vec<Account>> {
    let mut stmt = conn
        .prepare(
            "
            SELECT id, entity_id, code, name, account_type, parent_id,
                   is_active, is_system, sort_order
            FROM accounts
            WHERE entity_id = ?1
            ORDER BY sort_order, code
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map([entity_id.0.to_string()], map_account)
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|err| Error::Io(err.to_string()))?);
    }
    Ok(out)
}

/// Get one account.
///
/// # Errors
///
/// Not found or DB error.
pub fn get_account(conn: &Connection, id: AccountId) -> Result<Account> {
    conn.query_row(
        "
        SELECT id, entity_id, code, name, account_type, parent_id,
               is_active, is_system, sort_order
        FROM accounts WHERE id = ?1
        ",
        [id.0.to_string()],
        map_account,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound("account".into()),
        other => Error::Io(other.to_string()),
    })
}

/// Create a user account.
///
/// # Errors
///
/// Validation or DB error.
pub fn create_account(conn: &Connection, input: &CreateAccount) -> Result<Account> {
    let code = input.code.trim();
    let name = input.name.trim();
    if code.is_empty() || name.is_empty() {
        return Err(Error::Validation("code and name are required".into()));
    }

    // Ensure entity exists.
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entities WHERE id = ?1 AND archived_at IS NULL",
            [input.entity_id.0.to_string()],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if exists == 0 {
        return Err(Error::NotFound("entity".into()));
    }

    let id = AccountId::new();
    let sort = input.sort_order.unwrap_or(500);

    conn.execute(
        "
        INSERT INTO accounts (
            id, entity_id, code, name, account_type, parent_id,
            is_active, is_system, sort_order
        ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, 1, 0, ?6)
        ",
        rusqlite::params![
            id.0.to_string(),
            input.entity_id.0.to_string(),
            code,
            name,
            account_type_str(input.account_type),
            sort,
        ],
    )
    .map_err(|err| {
        if err.to_string().contains("UNIQUE") {
            Error::Validation("account code already exists for this entity".into())
        } else {
            Error::Io(err.to_string())
        }
    })?;

    get_account(conn, id)
}

/// Update account fields (not type / system flag).
///
/// # Errors
///
/// Not found, validation, or DB error.
pub fn update_account(conn: &Connection, input: &UpdateAccount) -> Result<Account> {
    let code = input.code.trim();
    let name = input.name.trim();
    if code.is_empty() || name.is_empty() {
        return Err(Error::Validation("code and name are required".into()));
    }

    let n = conn
        .execute(
            "
            UPDATE accounts
            SET code = ?1, name = ?2, is_active = ?3, sort_order = ?4
            WHERE id = ?5
            ",
            rusqlite::params![
                code,
                name,
                i32::from(input.is_active),
                input.sort_order,
                input.id.0.to_string(),
            ],
        )
        .map_err(|err| {
            if err.to_string().contains("UNIQUE") {
                Error::Validation("account code already exists for this entity".into())
            } else {
                Error::Io(err.to_string())
            }
        })?;

    if n == 0 {
        return Err(Error::NotFound("account".into()));
    }

    get_account(conn, input.id)
}

/// Soft-deactivate an account (cannot deactivate system accounts that are protected).
///
/// # Errors
///
/// Not found or DB error.
pub fn archive_account(conn: &Connection, id: AccountId) -> Result<()> {
    let account = get_account(conn, id)?;
    if account.is_system {
        return Err(Error::Validation(
            "system accounts cannot be archived".into(),
        ));
    }

    conn.execute(
        "UPDATE accounts SET is_active = 0 WHERE id = ?1",
        [id.0.to_string()],
    )
    .map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let id = parse_uuid_col(row, 0)?;
    let entity_id = parse_uuid_col(row, 1)?;
    let type_s: String = row.get(4)?;
    let account_type = parse_account_type(&type_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    let parent: Option<String> = row.get(5)?;
    let parent_id = match parent {
        Some(s) => Some(AccountId(parse_uuid(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?)),
        None => None,
    };

    Ok(Account {
        id: AccountId(id),
        entity_id: EntityId(entity_id),
        code: row.get(2)?,
        name: row.get(3)?,
        account_type,
        parent_id,
        is_active: row.get::<_, i64>(6)? != 0,
        is_system: row.get::<_, i64>(7)? != 0,
        sort_order: i32::try_from(row.get::<_, i64>(8)?).unwrap_or(0),
    })
}

fn parse_uuid_col(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<uuid::Uuid> {
    let s: String = row.get(idx)?;
    parse_uuid(&s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })
}
