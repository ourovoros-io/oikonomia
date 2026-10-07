//! Setup shared by the integration tests: a vault, a book, and a posting.
//!
//! Cargo builds every file directly under `tests/` as its own test binary, so
//! the shared code lives in `common/mod.rs`, which is not built on its own;
//! each test file pulls it in with `mod common;`.

#![expect(
    clippy::expect_used,
    reason = "setup fails loudly, and these helpers run outside `#[test]` functions, \
              where the test allowance of clippy.toml does not reach"
)]

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, PostJournal, PostJournalLine, PostJournalRequest, PostSimpleEntry,
    PostSimpleEntryRequest, PostedEntryView, SimpleEntryAccounts, create_entity, list_accounts,
    post_entry, post_simple_entry,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::util::parse_date;
use oikonomia_core::vault::Vault;
use oikonomia_core::{Error, Money};
use rusqlite::Connection;
use tempfile::TempDir;
use time::Date;

/// The master password of every vault [`vault`] creates.
pub(crate) const PASSWORD: &str = "correct horse battery staple";

// Every test binary compiles this module and calls only some of it, so each
// binary would report the rest as dead code. An `expect(dead_code)` cannot
// stand in: it is unfulfilled in a binary that calls everything. Naming each
// item once in an unnamed constant, which the compiler always treats as used,
// keeps the lint on and quiet. A helper added here must be named below too.
const _: () = {
    let _ = PASSWORD;
    let _ = (vault, book, account);
    let _ = (two_line, post_two_line, simple_expense);
    let _ = (
        date,
        strict::<PostJournalRequest, PostJournal>,
        post_simple_request,
    );
};

/// Parses a `YYYY-MM-DD` literal of a test.
///
/// # Panics
///
/// Panics if `text` is not such a date.
pub(crate) fn date(text: &str) -> Date {
    parse_date(text).expect("a test writes its dates as YYYY-MM-DD")
}

/// Converts a request in its wire form into the strict input the ledger
/// takes, as the desktop shell does before it calls the ledger.
///
/// A test that expects the conversion itself to fail calls `try_from` on the
/// strict type instead.
///
/// # Panics
///
/// Panics if the conversion refuses the request.
pub(crate) fn strict<W, T>(wire: W) -> T
where
    T: TryFrom<W, Error = Error>,
{
    T::try_from(wire).expect("the request converts into its strict form")
}

/// Creates and unlocks a vault in a new temporary directory.
///
/// The directory is deleted when the returned guard is dropped, so the caller
/// keeps it for as long as it uses the vault.
pub(crate) fn vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init(PASSWORD).expect("init");

    (dir, vault)
}

/// Creates a book called `name` on the chart of `template`.
///
/// The book is in euros, its fiscal year starts in January, and its seeded
/// account names are English.
pub(crate) fn book(conn: &Connection, name: &str, template: ChartTemplate) -> EntityId {
    let input = CreateEntity {
        name: name.into(),
        base_currency: "EUR".into(),
        chart_template: template,
        fiscal_year_start_month: Some(1),
    };

    create_entity(conn, &input, Locale::En).expect("entity").id
}

/// Returns the account of the book that has `code`.
///
/// # Panics
///
/// Panics if the book has no account with that code.
pub(crate) fn account(conn: &Connection, entity_id: EntityId, code: &str) -> AccountId {
    list_accounts(conn, entity_id)
        .expect("accounts")
        .iter()
        .find(|account| account.code == code)
        .map(|account| account.id)
        .expect(code)
}

/// Builds a balanced entry of `minor` with one debit line and one credit line.
///
/// `sides` holds the code of the debited account and then the code of the
/// credited one. The entry has no reference and its lines have no memo.
pub(crate) fn two_line(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    sides: (&str, &str),
    minor: i64,
) -> PostJournal {
    let (debit_code, credit_code) = sides;
    let amount = Money::from_minor(minor).expect("a test posts an amount that is not negative");

    PostJournal {
        entity_id,
        entry_date: self::date(date),
        description: format!("{debit_code} from {credit_code}: {minor} on {date}"),
        reference: None,
        lines: vec![
            PostJournalLine::debit(account(conn, entity_id, debit_code), amount),
            PostJournalLine::credit(account(conn, entity_id, credit_code), amount),
        ],
    }
}

/// Posts the entry [`two_line`] builds from the same arguments.
pub(crate) fn post_two_line(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    sides: (&str, &str),
    minor: i64,
) -> PostedEntryView {
    post_entry(conn, &two_line(conn, entity_id, date, sides, minor)).expect("post")
}

/// Posts a simple entry from the request the UI sends, converting it first as
/// the desktop shell does.
///
/// # Errors
///
/// Returns what the conversion or the post refuses the request for.
pub(crate) fn post_simple_request(
    conn: &Connection,
    request: &PostSimpleEntryRequest,
) -> Result<PostedEntryView, Error> {
    let entry = PostSimpleEntry::try_from(request.clone())?;
    post_simple_entry(conn, &entry)
}

/// Builds a paid expense of `minor` in `category`, paid from `wallet`.
///
/// The description is "groceries" and there is no reference.
pub(crate) fn simple_expense(
    entity_id: EntityId,
    category: AccountId,
    wallet: AccountId,
    date: &str,
    minor: i64,
) -> PostSimpleEntry {
    PostSimpleEntry {
        entity_id,
        accounts: SimpleEntryAccounts::Expense { category, wallet },
        entry_date: self::date(date),
        description: "groceries".into(),
        reference: None,
        amount_minor: minor,
    }
}
