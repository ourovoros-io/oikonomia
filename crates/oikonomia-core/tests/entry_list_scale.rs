//! Listing entries binds the same few variables however many entries match,
//! so a book with more entries than `SQLite` accepts bound variables lists.
//!
//! The real limit is 32766. Listing that many entries takes minutes today for
//! an unrelated reason (the voided flag scans `journal_entries` once per
//! entry), so the test lowers the limit on its own connection instead and
//! lists a handful of entries.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntry, SimpleEntryKind, create_entity, list_accounts,
    list_entries, post_simple_entry,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use rusqlite::limits::Limit;
use tempfile::TempDir;

/// More entries than [`LOWERED_VARIABLE_LIMIT`], and more than the five
/// variables a listing binds.
const ENTRY_COUNT: usize = 12;

/// Fewer variables than entries, and at least the five a listing binds.
const LOWERED_VARIABLE_LIMIT: i32 = 8;

fn setup() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn account(conn: &Connection, entity_id: EntityId, code: &str) -> AccountId {
    list_accounts(conn, entity_id)
        .expect("accounts")
        .iter()
        .find(|account| account.code == code)
        .map(|account| account.id)
        .expect(code)
}

/// A book with [`ENTRY_COUNT`] expenses, each with a memo-free pair of lines.
fn book_with_entries(conn: &Connection) -> (EntityId, AccountId) {
    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "Busy".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("entity");
    let food = account(conn, entity.id, "5100");
    let checking = account(conn, entity.id, "1010");

    for number in 1..=ENTRY_COUNT {
        post_simple_entry(
            conn,
            &PostSimpleEntry {
                entity_id: entity.id,
                kind: SimpleEntryKind::Expense,
                bill_status: None,
                entry_date: "2026-03-15".into(),
                description: format!("lunch {number}"),
                reference: None,
                amount_minor: 100,
                category_account_id: Some(food),
                wallet_account_id: Some(checking),
                payable_account_id: None,
                from_account_id: None,
                to_account_id: None,
            },
        )
        .expect("post");
    }
    (entity.id, food)
}

#[test]
fn the_bundled_sqlite_accepts_32766_bound_variables() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");

    assert_eq!(conn.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER), Ok(32_766));
}

#[test]
fn a_listing_of_more_entries_than_the_variable_limit_returns_every_entry_with_its_lines() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let (entity_id, food) = book_with_entries(conn);
    conn.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, LOWERED_VARIABLE_LIMIT)
        .expect("lower the limit");

    let every_filter_bound = EntryFilter {
        text: Some("LUNCH".into()),
        date_from: Some("2026-03-01".into()),
        date_to: Some("2026-03-31".into()),
        account_id: Some(food),
    };
    for filter in [EntryFilter::default(), every_filter_bound] {
        let listed = list_entries(conn, entity_id, &filter).expect("list");

        assert_eq!(listed.len(), ENTRY_COUNT, "{filter:?}");
        let incomplete = listed.iter().filter(|view| view.lines.len() != 2).count();
        assert_eq!(incomplete, 0, "every entry comes back with both its lines");
    }
}
