//! The chart of accounts: creating, reading, changing and archiving accounts.
//!
//! An account belongs to one entity and has a code that is unique within it;
//! the schema's `UNIQUE (entity_id, code)` enforces that, and a write that
//! violates it is reported as [`ValidationError::AccountCodeTaken`]. The
//! account's type is fixed when it is created. [`update_account`] cannot
//! change it, because the type decides the sign of every balance the account
//! has ever been reported with.
//!
//! An account is never deleted on its own, only with its whole entity, so no
//! journal line can lose its account. [`archive_account`] clears `is_active`
//! instead: the account stays in the
//! chart and in every report, and posting a new entry to it is refused by the
//! journal. System accounts, which the chart template marks, cannot be
//! archived.
//!
//! Each function writes with a single statement, so none opens a transaction.

use crate::db::{collect_rows, read_column, stored_id};
use crate::domain::{Account, AccountId, AccountType, EntityId};
use crate::error::{DatabaseContext, Error, NameField, Resource, Result, ValidationError};
use crate::ledger::balance::parse_account_type;
use crate::ledger::entities::ensure_writable_entity;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Input for [`create_account`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccount {
    /// Entity whose chart the account joins. It must exist and not be archived.
    pub entity_id: EntityId,
    /// Code the chart lists the account under, unique within the entity.
    /// Surrounding whitespace is trimmed; an empty code is refused.
    pub code: String,
    /// Name shown to the user. Surrounding whitespace is trimmed; an empty
    /// name is refused.
    pub name: String,
    /// Type of the account. It cannot be changed after creation.
    pub account_type: AccountType,
    /// Position in the chart, lowest first; accounts that share a position are
    /// ordered by code. `None` stores 500.
    pub sort_order: Option<i32>,
}

/// Input for [`update_account`]. Every field replaces the stored value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAccount {
    /// Account to change.
    pub id: AccountId,
    /// Code to store, unique within the account's entity. Surrounding
    /// whitespace is trimmed; an empty code is refused.
    pub code: String,
    /// Name to store. Surrounding whitespace is trimmed; an empty name is
    /// refused.
    pub name: String,
    /// Whether new entries may post to the account. `false` archives it and
    /// `true` brings an archived account back.
    pub is_active: bool,
    /// Position in the chart, lowest first.
    pub sort_order: i32,
}

/// Lists every account of an entity, archived ones included, by sort order
/// and then by code.
///
/// An entity that does not exist has no accounts and gives an empty list.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored account whose id, entity or type
///   does not parse. The whole list fails: an account left out would be
///   missing from every report built on it.
/// - [`Error::Database`] on database errors.
pub fn list_accounts(conn: &Connection, entity_id: EntityId) -> Result<Vec<Account>> {
    let mut stmt = conn
        .prepare(
            "
            SELECT id, entity_id, code, name, account_type,
                   is_active, is_system, sort_order
            FROM accounts
            WHERE entity_id = ?1
            ORDER BY sort_order, code
            ",
        )
        .database("list accounts")?;

    let rows = stmt
        .query_map([entity_id.to_string()], |row| {
            Ok(map_account("list accounts", row))
        })
        .database("list accounts")?;

    collect_rows("list accounts", rows)
}

/// Returns one account, archived or not.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown account.
/// - [`Error::VaultCorrupt`] for a stored account whose id, entity or type
///   does not parse.
/// - [`Error::Database`] on database errors.
pub fn get_account(conn: &Connection, id: AccountId) -> Result<Account> {
    conn.query_row(
        "
        SELECT id, entity_id, code, name, account_type,
               is_active, is_system, sort_order
        FROM accounts WHERE id = ?1
        ",
        [id.to_string()],
        |row| Ok(map_account("read account", row)),
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::Account),
        other => Error::database("read account", other),
    })?
}

/// Adds an account to an entity's chart.
///
/// The account is active and is not a system account. The `parent_id` column
/// is not written, so it holds NULL.
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for an empty code or name.
/// - [`Error::NotFound`] for an unknown or archived entity.
/// - [`ValidationError::AccountCodeTaken`] when the entity already has an
///   account with this code.
/// - [`Error::VaultCorrupt`] when the account does not parse on being read
///   back.
/// - [`Error::Database`] on database errors.
pub fn create_account(conn: &Connection, input: &CreateAccount) -> Result<Account> {
    let code = input.code.trim();
    let name = input.name.trim();
    if code.is_empty() || name.is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::AccountCodeAndName,
        }
        .into());
    }

    ensure_writable_entity(conn, input.entity_id)?;

    let id = AccountId::generate();
    let sort_order = input.sort_order.unwrap_or(DEFAULT_SORT_ORDER);

    conn.execute(
        "
        INSERT INTO accounts (
            id, entity_id, code, name, account_type,
            is_active, is_system, sort_order
        ) VALUES (?1, ?2, ?3, ?4, ?5, 1, 0, ?6)
        ",
        rusqlite::params![
            id.to_string(),
            input.entity_id.to_string(),
            code,
            name,
            input.account_type.identifier(),
            sort_order,
        ],
    )
    .map_err(|err| account_write_error(&err))?;

    get_account(conn, id)
}

/// Replaces the code, name, active flag and sort order of an account.
///
/// The type, the entity and the system flag are not part of the input and
/// stay as they are.
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for an empty code or name.
/// - [`Error::NotFound`] for an unknown account.
/// - [`ValidationError::SystemAccountProtected`] when deactivating a system
///   account.
/// - [`ValidationError::AccountCodeTaken`] when the entity already has
///   another account with this code.
/// - [`Error::VaultCorrupt`] when the stored account does not parse.
/// - [`Error::Database`] on database errors.
pub fn update_account(conn: &Connection, input: &UpdateAccount) -> Result<Account> {
    let code = input.code.trim();
    let name = input.name.trim();
    if code.is_empty() || name.is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::AccountCodeAndName,
        }
        .into());
    }

    let account = get_account(conn, input.id)?;
    if account.is_system && !input.is_active {
        return Err(ValidationError::SystemAccountProtected.into());
    }

    let updated = conn
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
                input.id.to_string(),
            ],
        )
        .map_err(|err| account_write_error(&err))?;

    if updated == 0 {
        return Err(Error::NotFound(Resource::Account));
    }

    get_account(conn, input.id)
}

/// Archives an account: clears its active flag and keeps the account and its
/// entries.
///
/// Archiving an account that is already archived succeeds and changes
/// nothing.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown account.
/// - [`ValidationError::SystemAccountProtected`] for a system account.
/// - [`Error::VaultCorrupt`] when the stored account does not parse.
/// - [`Error::Database`] on database errors.
pub fn archive_account(conn: &Connection, id: AccountId) -> Result<()> {
    let account = get_account(conn, id)?;
    if account.is_system {
        return Err(ValidationError::SystemAccountProtected.into());
    }

    conn.execute(
        "UPDATE accounts SET is_active = 0 WHERE id = ?1",
        [id.to_string()],
    )
    .database("archive account")?;
    Ok(())
}

/// Sort order of an account created without one.
///
/// The chart templates give their accounts lower numbers, so an account
/// created without a position is listed after the seeded ones, by code among
/// the others created the same way.
const DEFAULT_SORT_ORDER: i32 = 500;

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
            ValidationError::AccountCodeTaken.into()
        }
        other => Error::database("write account", other),
    }
}

/// Maps a row selected as `id, entity_id, code, name, account_type,
/// is_active, is_system, sort_order`.
///
/// The table's `parent_id` column is not selected, so whatever a vault holds
/// there has no effect on the account that is read.
///
/// A sort order outside `i32` reads as 0: it only orders the chart, and
/// refusing the row for it would take the account out of every report.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when the id, the entity or the
/// type does not parse, or a column has the wrong storage class. No
/// half-read account is returned.
fn map_account(operation: &'static str, row: &rusqlite::Row<'_>) -> Result<Account> {
    let id = stored_id("accounts.id", &read_column::<String>(operation, row, 0)?)?;
    let entity_id = stored_id(
        "accounts.entity_id",
        &read_column::<String>(operation, row, 1)?,
    )?;
    let account_type = parse_account_type(&read_column::<String>(operation, row, 4)?)?;

    Ok(Account {
        id,
        entity_id,
        code: read_column(operation, row, 2)?,
        name: read_column(operation, row, 3)?,
        account_type,
        is_active: read_column::<i64>(operation, row, 5)? != 0,
        is_system: read_column::<i64>(operation, row, 6)? != 0,
        sort_order: i32::try_from(read_column::<i64>(operation, row, 7)?).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::VaultCorruption;

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

    /// Writes an archived system account `1000 Cash` of [`ENTITY`] with the
    /// given id, stored type and sort order, valid or not, and with `parent`
    /// in the reserved `parent_id` column.
    fn insert(conn: &Connection, id: &str, account_type: &str, parent: Option<&str>, sort: i64) {
        conn.execute(
            "INSERT INTO accounts (id, entity_id, code, name, account_type, parent_id,
                                   is_active, is_system, sort_order)
             VALUES (?1, ?2, '1000', 'Cash', ?3, ?4, 0, 1, ?5)",
            rusqlite::params![id, ENTITY, account_type, parent, sort],
        )
        .expect("insert row");
    }

    /// The id of the one entity these tests use.
    fn entity() -> EntityId {
        ENTITY.parse().expect("entity id")
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

    /// Input for an asset account `Cash` under `code`, with no sort order.
    fn new_account(code: &str) -> CreateAccount {
        CreateAccount {
            entity_id: entity(),
            code: code.into(),
            name: "Cash".into(),
            account_type: AccountType::Asset,
            sort_order: None,
        }
    }

    /// Input that changes only the code of `account`, leaving it active.
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
        assert!(
            matches!(created, Err(Error::Database { .. })),
            "{created:?}"
        );

        let updated = update_account(&conn, &renumbered(&existing, "3000"));
        assert!(
            matches!(updated, Err(Error::Database { .. })),
            "{updated:?}"
        );
    }

    #[test]
    fn a_stored_row_maps_to_an_account_field_by_field() {
        let conn = accounts_table();
        insert(&conn, ACCOUNT, "asset", Some(PARENT), 7);

        let accounts = list_accounts(&conn, entity()).expect("list");

        let [account] = accounts.as_slice() else {
            unreachable!("one row was inserted, got {accounts:?}");
        };
        assert_eq!(account.id.to_string(), ACCOUNT);
        assert_eq!(account.entity_id, entity());
        assert_eq!(account.code, "1000");
        assert_eq!(account.name, "Cash");
        assert_eq!(account.account_type, AccountType::Asset);
        assert!(!account.is_active);
        assert!(account.is_system);
        assert_eq!(account.sort_order, 7);
    }

    #[test]
    fn an_out_of_range_sort_order_is_tolerated() {
        let conn = accounts_table();
        insert(&conn, ACCOUNT, "expense", None, i64::MAX);

        let accounts = list_accounts(&conn, entity()).expect("list");

        assert_eq!(accounts[0].sort_order, 0);
    }

    /// The column is reserved: a vault may hold a parent there, an id or
    /// anything else, and the account reads as it would without one.
    #[test]
    fn a_stored_parent_is_ignored_when_an_account_is_read() {
        let without_parent = accounts_table();
        insert(&without_parent, ACCOUNT, "asset", None, 7);
        let expected = list_accounts(&without_parent, entity()).expect("list");

        for parent in [PARENT, "not-a-uuid"] {
            let conn = accounts_table();
            insert(&conn, ACCOUNT, "asset", Some(parent), 7);

            assert_eq!(
                list_accounts(&conn, entity()).as_ref(),
                Ok(&expected),
                "{parent}"
            );
            let account_id = ACCOUNT.parse().expect("account id");
            assert_eq!(get_account(&conn, account_id).as_ref(), Ok(&expected[0]));
        }
    }

    #[test]
    fn a_created_account_stores_no_parent() {
        let conn = chart_with_unique_codes();
        let account = create_account(&conn, &new_account("1000")).expect("create");

        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_id FROM accounts WHERE id = ?1",
                [account.id.to_string()],
                |row| row.get(0),
            )
            .expect("read the column");

        assert_eq!(parent, None);
    }

    #[test]
    fn a_corrupt_row_fails_the_query_instead_of_yielding_a_wrong_account() {
        // Each row, and the first column that is wrong in it.
        let corrupt_rows = [
            ("not-a-uuid", "asset", None, "accounts.id"),
            (ACCOUNT, "treasure", None, "accounts.account_type"),
            ("not-a-uuid", "treasure", Some("not-a-uuid"), "accounts.id"),
        ];

        for (id, account_type, parent, column) in corrupt_rows {
            let conn = accounts_table();
            insert(&conn, id, account_type, parent, 0);

            let err = list_accounts(&conn, entity()).expect_err("a corrupt row is refused");

            assert!(
                matches!(
                    &err,
                    Error::VaultCorrupt(VaultCorruption::Column { column: named, .. })
                        if named == column
                ),
                "row ({id}, {account_type}, {parent:?}): {err:?}"
            );
        }
    }
}
