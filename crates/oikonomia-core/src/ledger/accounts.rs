//! Chart of accounts CRUD.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::{read_column, stored_uuid};
use crate::domain::{Account, AccountId, AccountType, EntityId};
use crate::error::{Error, Result, ValidationError};
use crate::ledger::balance::{account_type_str, parse_account_type};

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
        .query_map([entity_id.0.to_string()], |row| Ok(map_account(row)))
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|err| Error::Io(err.to_string()))??);
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
        |row| Ok(map_account(row)),
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound("account".into()),
        other => Error::Io(other.to_string()),
    })?
}

/// Creates a user account.
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for an empty code or name.
/// - [`Error::NotFound`] for an unknown or archived entity.
/// - [`ValidationError::AccountCodeTaken`] when the entity already has an
///   account with this code.
/// - [`Error::Io`] on database errors.
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
    .map_err(|err| account_write_error(&err))?;

    get_account(conn, id)
}

/// Classifies a failed write to `accounts`.
///
/// The only unique constraint a caller can violate is `(entity_id, code)`, so
/// that failure means the code is taken. It is recognised by `SQLite`'s
/// extended result code, never by the wording of its message.
fn account_write_error(err: &rusqlite::Error) -> Error {
    match err {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            Error::Validation(ValidationError::AccountCodeTaken)
        }
        other => Error::Io(other.to_string()),
    }
}

/// Updates account fields (not type / system flag).
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for an empty code or name.
/// - [`Error::NotFound`] for an unknown account.
/// - [`ValidationError::SystemAccountProtected`] when deactivating a system
///   account.
/// - [`ValidationError::AccountCodeTaken`] when the entity already has
///   another account with this code.
/// - [`Error::Io`] on database errors.
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
        .map_err(|err| account_write_error(&err))?;

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

/// Maps a row selected as `id, entity_id, code, name, account_type, parent_id,
/// is_active, is_system, sort_order`.
///
/// A stored id or type that does not parse fails the query naming the column
/// instead of returning a half-read account.
fn map_account(row: &rusqlite::Row<'_>) -> Result<Account> {
    let id = stored_uuid("accounts.id", &read_column::<String>(row, 0)?)?;
    let entity_id = stored_uuid("accounts.entity_id", &read_column::<String>(row, 1)?)?;
    let account_type = parse_account_type(&read_column::<String>(row, 4)?)?;

    let parent_id = read_column::<Option<String>>(row, 5)?
        .map(|text| stored_uuid("accounts.parent_id", &text).map(AccountId))
        .transpose()?;

    Ok(Account {
        id: AccountId(id),
        entity_id: EntityId(entity_id),
        code: read_column(row, 2)?,
        name: read_column(row, 3)?,
        account_type,
        parent_id,
        is_active: read_column::<i64>(row, 6)? != 0,
        is_system: read_column::<i64>(row, 7)? != 0,
        sort_order: i32::try_from(read_column::<i64>(row, 8)?).unwrap_or(0),
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::*;
    use crate::util::parse_uuid;

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

    /// One entity and an accounts table with the schema's unique code per entity.
    fn chart_with_unique_codes() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory database");
        conn.execute_batch(&format!(
            "CREATE TABLE entities (id TEXT PRIMARY KEY NOT NULL, archived_at TEXT);
             INSERT INTO entities (id) VALUES ('{ENTITY}');
             CREATE TABLE accounts (
                id TEXT PRIMARY KEY NOT NULL,
                entity_id TEXT NOT NULL,
                code TEXT NOT NULL,
                name TEXT NOT NULL,
                account_type TEXT NOT NULL,
                parent_id TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                is_system INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                UNIQUE (entity_id, code)
            );"
        ))
        .expect("create tables");
        conn
    }

    fn new_account(code: &str) -> CreateAccount {
        CreateAccount {
            entity_id: entity(),
            code: code.into(),
            name: "Cash".into(),
            account_type: AccountType::Asset,
            sort_order: None,
        }
    }

    fn renumbered(account: &Account, code: &str) -> UpdateAccount {
        UpdateAccount {
            id: account.id,
            code: code.into(),
            name: account.name.clone(),
            is_active: true,
            sort_order: account.sort_order,
        }
    }

    #[test]
    fn a_code_already_used_in_the_book_is_reported_as_taken() {
        let conn = chart_with_unique_codes();
        create_account(&conn, &new_account("1000")).expect("first");
        let second = create_account(&conn, &new_account("1001")).expect("second");
        let taken = Err(Error::Validation(ValidationError::AccountCodeTaken));

        assert_eq!(
            create_account(&conn, &new_account("1000")).map(|account| account.id),
            taken
        );
        assert_eq!(
            update_account(&conn, &renumbered(&second, "1000")).map(|account| account.id),
            taken
        );
    }

    /// The failure is classified by `SQLite`'s result code. Its message is
    /// English prose that only happens to say UNIQUE for that code today.
    #[test]
    fn another_failure_whose_message_says_unique_is_not_a_taken_code() {
        let conn = chart_with_unique_codes();
        let existing = create_account(&conn, &new_account("1000")).expect("first");
        conn.execute_batch(
            "CREATE TRIGGER refuse_insert BEFORE INSERT ON accounts
             BEGIN SELECT RAISE(ABORT, 'UNIQUE visitors only'); END;
             CREATE TRIGGER refuse_update BEFORE UPDATE ON accounts
             BEGIN SELECT RAISE(ABORT, 'UNIQUE visitors only'); END;",
        )
        .expect("create triggers");

        let created = create_account(&conn, &new_account("2000"));
        assert!(matches!(created, Err(Error::Io(_))), "{created:?}");

        let updated = update_account(&conn, &renumbered(&existing, "3000"));
        assert!(matches!(updated, Err(Error::Io(_))), "{updated:?}");
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
        // Each row, and the first column that is wrong in it.
        let corrupt_rows = [
            ("not-a-uuid", "asset", None, "accounts.id"),
            (ACCOUNT, "treasure", None, "accounts.account_type"),
            (ACCOUNT, "asset", Some("not-a-uuid"), "accounts.parent_id"),
            ("not-a-uuid", "treasure", Some("not-a-uuid"), "accounts.id"),
        ];

        for (id, account_type, parent, column) in corrupt_rows {
            let conn = accounts_table();
            insert(&conn, id, account_type, parent, 0);

            let err = list_accounts(&conn, entity()).expect_err("a corrupt row is refused");

            assert!(
                matches!(&err, Error::VaultCorrupt(detail) if detail.starts_with(column)),
                "row ({id}, {account_type}, {parent:?}): {err:?}"
            );
        }
    }
}
