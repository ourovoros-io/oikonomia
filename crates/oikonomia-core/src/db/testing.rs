//! Test-only helpers: a database on the current schema, and the plan
//! `SQLite` chooses for a query on it.
//!
//! A query plan is not behaviour a caller can see through the public API,
//! but an index that a query stops using is a regression nothing else
//! reports: the result stays right and only the time grows with the square
//! of the book. The unit tests beside the ledger's queries therefore pin the
//! index each one relies on, through [`query_plan`].

use crate::db::{migrate, register_fold};
use rusqlite::{Connection, ToSql};

/// Returns an in-memory database migrated to the current schema, with the
/// `fold` function registered as on a vault connection.
///
/// # Panics
///
/// Panics if the database cannot be opened or migrated.
pub(crate) fn migrated_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database opens");
    register_fold(&conn).expect("the fold function registers");
    conn.execute_batch(
        "
        CREATE TABLE vault_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            schema_version INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );
        INSERT INTO vault_meta (id, schema_version, created_at)
        VALUES (1, 1, datetime('now'));
        ",
    )
    .expect("the metadata table is created");
    migrate(&conn).expect("a new database migrates");

    conn
}

/// Returns the plan of `sql` as `EXPLAIN QUERY PLAN` reports it, one step per
/// line.
///
/// The wording of a step is `SQLite`'s and changes between versions, so a
/// test looks only for the name of the index it expects.
///
/// # Panics
///
/// Panics if `sql` does not prepare with `params` bound.
pub(crate) fn query_plan(conn: &Connection, sql: &str, params: &[&dyn ToSql]) -> String {
    let mut statement = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("the query prepares");
    let steps = statement
        .query_map(params, |row| row.get::<_, String>(3))
        .expect("the plan is read");

    steps
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("every step of the plan is text")
        .join("\n")
}
