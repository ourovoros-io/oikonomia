//! Chart of accounts CRUD.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{Account, AccountId, AccountType, EntityId};
use crate::error::{Error, Result, ValidationError};
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
        return Err(Error::Validation(ValidationError::NameRequired {
            field: "code and name",
        }));
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
            Error::Validation(ValidationError::AccountCodeTaken)
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
        return Err(Error::Validation(ValidationError::NameRequired {
            field: "code and name",
        }));
    }

    let account = get_account(conn, input.id)?;
    if account.is_system && !input.is_active {
        return Err(Error::Validation(ValidationError::SystemAccountProtected));
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
                Error::Validation(ValidationError::AccountCodeTaken)
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
        return Err(Error::Validation(ValidationError::SystemAccountProtected));
    }

    conn.execute(
        "UPDATE accounts SET is_active = 0 WHERE id = ?1",
        [id.0.to_string()],
    )
    .map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let type_text: String = row.get(4)?;
    let account_type = parse_account_type(&type_text).map_err(|err| invalid_text(4, &err))?;

    let parent: Option<String> = row.get(5)?;
    let parent_id = parent
        .map(|text| parse_uuid(&text).map(AccountId))
        .transpose()
        .map_err(|err| invalid_text(5, &err))?;

    Ok(Account {
        id: AccountId(parse_uuid_col(row, 0)?),
        entity_id: EntityId(parse_uuid_col(row, 1)?),
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
    let text: String = row.get(idx)?;
    parse_uuid(&text).map_err(|err| invalid_text(idx, &err))
}

/// A stored text value that does not parse: the row is corrupt, and the
/// query fails naming the column instead of returning a half-read account.
fn invalid_text(column: usize, err: &Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            err.to_string(),
        )),
    )
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::*;

    const ENTITY: &str = "11111111-1111-4111-8111-111111111111";
    const ACCOUNT: &str = "22222222-2222-4222-8222-222222222222";
    const PARENT: &str = "33333333-3333-4333-8333-333333333333";

    /// The accounts table as the schema declares it, without the foreign
    /// keys, so a row can be written in any state a damaged file could hold.
    fn accounts_table() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory database");
        conn.execute_batch(
            "CREATE TABLE accounts (
                id TEXT PRIMARY KEY NOT NULL,
                entity_id TEXT NOT NULL,
                code TEXT NOT NULL,
                name TEXT NOT NULL,
                account_type TEXT NOT NULL,
                parent_id TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                is_system INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0
            );",
        )
        .expect("create table");
        conn
    }

    fn insert(conn: &Connection, id: &str, account_type: &str, parent: Option<&str>, sort: i64) {
        conn.execute(
            "INSERT INTO accounts (id, entity_id, code, name, account_type, parent_id,
                                   is_active, is_system, sort_order)
             VALUES (?1, ?2, '1000', 'Cash', ?3, ?4, 0, 1, ?5)",
            rusqlite::params![id, ENTITY, account_type, parent, sort],
        )
        .expect("insert row");
    }

    fn entity() -> EntityId {
        EntityId(parse_uuid(ENTITY).expect("entity id"))
    }

    #[test]
    fn a_stored_row_maps_to_an_account_field_by_field() {
        let conn = accounts_table();
        insert(&conn, ACCOUNT, "asset", Some(PARENT), 7);

        let accounts = list_accounts(&conn, entity()).expect("list");

        let [account] = accounts.as_slice() else {
            unreachable!("one row was inserted, got {accounts:?}");
        };
        assert_eq!(account.id.0.to_string(), ACCOUNT);
        assert_eq!(account.entity_id, entity());
        assert_eq!(account.code, "1000");
        assert_eq!(account.name, "Cash");
        assert_eq!(account.account_type, AccountType::Asset);
        assert_eq!(
            account.parent_id.map(|id| id.0.to_string()).as_deref(),
            Some(PARENT)
        );
        assert!(!account.is_active);
        assert!(account.is_system);
        assert_eq!(account.sort_order, 7);
    }

    #[test]
    fn a_missing_parent_and_an_out_of_range_sort_order_are_tolerated() {
        let conn = accounts_table();
        insert(&conn, ACCOUNT, "expense", None, i64::MAX);

        let accounts = list_accounts(&conn, entity()).expect("list");

        assert_eq!(accounts[0].parent_id, None);
        assert_eq!(accounts[0].sort_order, 0);
    }

    #[test]
    fn a_corrupt_row_fails_the_query_instead_of_yielding_a_wrong_account() {
        let corrupt_rows = [
            ("not-a-uuid", "asset", None),
            (ACCOUNT, "treasure", None),
            (ACCOUNT, "asset", Some("not-a-uuid")),
        ];

        for (id, account_type, parent) in corrupt_rows {
            let conn = accounts_table();
            insert(&conn, id, account_type, parent, 0);

            let result = list_accounts(&conn, entity());

            assert!(
                result.is_err(),
                "row ({id}, {account_type}, {parent:?}) was accepted: {result:?}"
            );
        }
    }
}
