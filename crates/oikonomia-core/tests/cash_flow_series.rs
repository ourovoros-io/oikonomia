//! Cash-flow series: the numbers the light draws. They must agree with the
//! dashboard to the minor unit, cover the window exactly, and ignore voids.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::error::Error;
use oikonomia_core::error::ValidationError;
use oikonomia_core::ledger::{
    CashFlowGranularity, CashFlowSeries, CreateEntity, CreateJournalLine, PostJournal,
    activity_window, cash_flow_series, create_entity, dashboard_summary, list_accounts, post_entry,
    set_entry_hidden, void_entry,
};
use oikonomia_core::prefs::Locale;
use oikonomia_core::util::parse_date;
use oikonomia_core::vault::Vault;
use rusqlite::Connection;
use tempfile::TempDir;
use time::Date;

fn setup() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init");
    (dir, vault)
}

fn book(conn: &Connection) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: "Probe".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
        Locale::En,
    )
    .expect("entity")
    .id
}

fn account(conn: &Connection, entity_id: EntityId, code: &str) -> AccountId {
    list_accounts(conn, entity_id)
        .expect("accounts")
        .iter()
        .find(|a| a.code == code)
        .map(|a| a.id)
        .expect(code)
}

/// Posts `minor` debited to `debit_code` and credited to `credit_code`.
fn post(
    conn: &Connection,
    entity_id: EntityId,
    date: &str,
    debit_code: &str,
    credit_code: &str,
    minor: i64,
) -> JournalEntryId {
    post_entry(
        conn,
        &PostJournal {
            entity_id,
            entry_date: date.into(),
            description: format!("{debit_code} from {credit_code}: {minor} on {date}"),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: account(conn, entity_id, debit_code),
                    debit_minor: minor,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: account(conn, entity_id, credit_code),
                    debit_minor: 0,
                    credit_minor: minor,
                    memo: None,
                },
            ],
        },
    )
    .expect("post")
    .entry
    .id
}

/// Food paid from checking.
fn expense(conn: &Connection, entity_id: EntityId, date: &str, minor: i64) -> JournalEntryId {
    post(conn, entity_id, date, "5100", "1010", minor)
}

/// Salary into checking.
fn income(conn: &Connection, entity_id: EntityId, date: &str, minor: i64) -> JournalEntryId {
    post(conn, entity_id, date, "1010", "4000", minor)
}

fn date(s: &str) -> Date {
    parse_date(s).expect("date")
}

fn series(conn: &Connection, entity_id: EntityId, from: &str, to: &str) -> CashFlowSeries {
    cash_flow_series(conn, entity_id, from, to).expect("series")
}

fn assert_contiguous(s: &CashFlowSeries) {
    let first = s.buckets.first().expect("at least one bucket");
    let last = s.buckets.last().expect("at least one bucket");
    assert_eq!(first.start, s.from, "the first bucket starts on from");
    assert_eq!(last.end, s.to, "the last bucket ends on to");
    for pair in s.buckets.windows(2) {
        assert_eq!(
            pair[0].end.next_day(),
            Some(pair[1].start),
            "buckets are contiguous: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }
    for bucket in &s.buckets {
        assert!(
            bucket.start <= bucket.end,
            "bucket runs forward: {bucket:?}"
        );
    }
}

#[test]
fn totals_match_the_dashboard_for_the_same_window() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    income(conn, e, "2026-08-03", 100_000);
    expense(conn, e, "2026-08-05", 2_500);
    post(conn, e, "2026-08-06", "1020", "1010", 30_000); // transfer
    post(conn, e, "2026-08-07", "5600", "2000", 4_000); // card expense
    let voided = expense(conn, e, "2026-08-08", 9_999);
    void_entry(conn, voided, Locale::En).expect("void");
    post(conn, e, "2026-08-13", "5300", "2050", 7_253); // unpaid bill
    expense(conn, e, "2026-08-20", 1_000);
    income(conn, e, "2026-02-14", 3_000);
    expense(conn, e, "2026-11-30", 450);

    for (from, to) in [
        ("2026-08-01", "2026-08-31"),
        ("2026-08-05", "2026-08-13"),
        ("2026-07-01", "2026-09-30"),
        ("2026-01-01", "2026-12-31"),
        ("2025-06-01", "2027-05-31"),
    ] {
        let s = series(conn, e, from, to);
        let dash = dashboard_summary(conn, e, from, to, to).expect("dashboard");
        let last = s.buckets.last().expect("bucket");
        assert_eq!(
            last.cumulative_income_minor, dash.income,
            "income {from}..{to}"
        );
        assert_eq!(
            last.cumulative_expenses_minor, dash.expenses,
            "expenses {from}..{to}"
        );
        assert_eq!(
            s.total_income_minor, dash.income,
            "total income {from}..{to}"
        );
        assert_eq!(
            s.total_expenses_minor, dash.expenses,
            "total expenses {from}..{to}"
        );
        assert_eq!(s.net_minor, dash.net_income, "net {from}..{to}");
        assert_eq!(
            s.buckets.iter().map(|b| b.income_minor).sum::<i64>(),
            dash.income
        );
        assert_eq!(
            s.buckets.iter().map(|b| b.expenses_minor).sum::<i64>(),
            dash.expenses
        );
    }

    assert_eq!(
        series(conn, e, "2026-08-01", "2026-08-31").total_expenses_minor,
        14_753,
        "groceries 2500 + card 4000 + bill 7253 + 1000 (the void is gone)"
    );
}

#[test]
fn a_month_is_bucketed_per_day_and_covers_the_window_exactly() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    income(conn, e, "2026-08-03", 100_000);
    expense(conn, e, "2026-08-05", 2_500);

    let s = series(conn, e, "2026-08-01", "2026-08-31");
    assert_eq!(s.granularity, CashFlowGranularity::Day);
    assert_eq!(s.buckets.len(), 31);
    assert_contiguous(&s);
    assert!(
        s.buckets.iter().all(|b| b.start == b.end),
        "one day per bucket"
    );

    assert_eq!(
        s.buckets[1].cumulative_income_minor, 0,
        "nothing before the salary"
    );
    let third = &s.buckets[2];
    assert_eq!(
        (
            third.start,
            third.income_minor,
            third.cumulative_income_minor
        ),
        (date("2026-08-03"), 100_000, 100_000)
    );
    let fifth = &s.buckets[4];
    assert_eq!(
        (
            fifth.expenses_minor,
            fifth.cumulative_expenses_minor,
            fifth.cumulative_income_minor
        ),
        (2_500, 2_500, 100_000)
    );
}

#[test]
fn ninety_two_days_is_daily_and_ninety_three_is_monthly() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);

    let quarter = series(conn, e, "2026-07-01", "2026-09-30");
    assert_eq!(quarter.granularity, CashFlowGranularity::Day);
    assert_eq!(quarter.buckets.len(), 92);
    assert_contiguous(&quarter);

    let longer = series(conn, e, "2026-07-01", "2026-10-01");
    assert_eq!(longer.granularity, CashFlowGranularity::Month);
    let ranges: Vec<(Date, Date)> = longer.buckets.iter().map(|b| (b.start, b.end)).collect();
    assert_eq!(
        ranges,
        vec![
            (date("2026-07-01"), date("2026-07-31")),
            (date("2026-08-01"), date("2026-08-31")),
            (date("2026-09-01"), date("2026-09-30")),
            (date("2026-10-01"), date("2026-10-01")),
        ]
    );
}

#[test]
fn monthly_buckets_are_clipped_to_the_window() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);

    let s = series(conn, e, "2026-01-15", "2026-12-10");
    assert_eq!(s.granularity, CashFlowGranularity::Month);
    assert_eq!(s.buckets.len(), 12);
    assert_contiguous(&s);
    assert_eq!(
        (s.buckets[0].start, s.buckets[0].end),
        (date("2026-01-15"), date("2026-01-31"))
    );
    assert_eq!(
        (s.buckets[1].start, s.buckets[1].end),
        (date("2026-02-01"), date("2026-02-28"))
    );
    assert_eq!(
        (s.buckets[11].start, s.buckets[11].end),
        (date("2026-12-01"), date("2026-12-10"))
    );
}

#[test]
fn an_entry_on_a_bucket_boundary_lands_in_exactly_one_bucket() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    expense(conn, e, "2026-01-31", 1_000);
    expense(conn, e, "2026-02-01", 2_000);
    income(conn, e, "2026-01-01", 500);
    income(conn, e, "2026-12-31", 700);

    let year = series(conn, e, "2026-01-01", "2026-12-31");
    assert_eq!(
        (
            year.buckets[0].expenses_minor,
            year.buckets[1].expenses_minor
        ),
        (1_000, 2_000)
    );
    assert_eq!(
        year.buckets[0].income_minor, 500,
        "the window's first day counts"
    );
    assert_eq!(
        year.buckets[11].income_minor, 700,
        "the window's last day counts"
    );
    assert_eq!(year.total_expenses_minor, 3_000);

    let edge = series(conn, e, "2026-01-31", "2026-02-01");
    assert_eq!(
        edge.buckets
            .iter()
            .map(|b| b.expenses_minor)
            .collect::<Vec<_>>(),
        vec![1_000, 2_000]
    );
}

#[test]
fn an_empty_window_returns_zero_valued_buckets() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);

    let s = series(conn, e, "2026-08-01", "2026-08-07");
    assert_eq!(s.buckets.len(), 7);
    assert!(s.buckets.iter().all(|b| {
        b.income_minor == 0
            && b.expenses_minor == 0
            && b.cumulative_income_minor == 0
            && b.cumulative_expenses_minor == 0
    }));
    assert_eq!(
        (s.total_income_minor, s.total_expenses_minor, s.net_minor),
        (0, 0, 0)
    );
}

#[test]
fn inverted_and_malformed_windows_are_validation_errors() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);

    assert!(matches!(
        cash_flow_series(conn, e, "2026-08-31", "2026-08-01"),
        Err(Error::Validation(ValidationError::DateRangeInverted))
    ));
    assert!(matches!(
        cash_flow_series(conn, e, "2026-13-01", "2026-12-31"),
        Err(Error::Validation(ValidationError::InvalidDate { .. }))
    ));
}

#[test]
fn an_unknown_book_is_not_found() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");

    assert_eq!(
        cash_flow_series(conn, EntityId::new(), "2026-08-01", "2026-08-31").expect_err("unknown"),
        Error::NotFound("entity".into())
    );
}

#[test]
fn voided_entries_and_their_reversals_contribute_nothing() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    let spent = expense(conn, e, "2026-08-08", 9_999);
    let earned = income(conn, e, "2026-08-09", 5_000);
    void_entry(conn, spent, Locale::En).expect("void expense");
    void_entry(conn, earned, Locale::En).expect("void income");

    for (from, to) in [("2026-08-01", "2026-08-31"), ("2026-01-01", "2026-12-31")] {
        let s = series(conn, e, from, to);
        assert!(
            s.buckets
                .iter()
                .all(|b| b.income_minor == 0 && b.expenses_minor == 0),
            "{from}..{to}: {:?}",
            s.buckets
        );
    }
}

#[test]
fn transfers_move_neither_side_and_hidden_entries_still_count() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    post(conn, e, "2026-08-06", "1020", "1010", 30_000);
    let hidden = expense(conn, e, "2026-08-07", 1_200);
    set_entry_hidden(conn, hidden, true).expect("hide");

    let s = series(conn, e, "2026-08-01", "2026-08-31");
    assert_eq!(s.total_income_minor, 0);
    assert_eq!(
        s.total_expenses_minor, 1_200,
        "Hidden only leaves exports; the dashboard still counts it"
    );
}

#[test]
fn an_open_window_spans_the_books_active_entries() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    let today = date("2026-09-15");

    assert_eq!(
        activity_window(conn, e, None, None, today).expect("empty books"),
        (today, today)
    );

    let voided = expense(conn, e, "2026-01-02", 100);
    void_entry(conn, voided, Locale::En).expect("void");
    expense(conn, e, "2026-03-05", 100);
    income(conn, e, "2026-08-20", 100);

    assert_eq!(
        activity_window(conn, e, None, None, today).expect("open"),
        (date("2026-03-05"), date("2026-08-20"))
    );
    assert_eq!(
        activity_window(conn, e, Some("2026-04-01"), None, today).expect("from only"),
        (date("2026-04-01"), date("2026-08-20"))
    );
    assert_eq!(
        activity_window(conn, e, None, Some("2026-05-01"), today).expect("to only"),
        (date("2026-03-05"), date("2026-05-01"))
    );
}

#[test]
fn a_one_sided_window_never_runs_backwards() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    let today = date("2026-09-15");
    expense(conn, e, "2026-03-05", 100);
    income(conn, e, "2026-08-20", 100);

    assert_eq!(
        activity_window(conn, e, Some("2026-10-01"), None, today).expect("after the last entry"),
        (date("2026-10-01"), date("2026-10-01"))
    );
    assert_eq!(
        activity_window(conn, e, None, Some("2026-01-01"), today).expect("before the first entry"),
        (date("2026-01-01"), date("2026-01-01"))
    );
}

#[test]
fn an_empty_book_falls_back_to_today_for_the_open_side() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    let today = date("2026-09-15");

    assert_eq!(
        activity_window(conn, e, Some("2026-02-01"), None, today).expect("from only"),
        (date("2026-02-01"), today)
    );
    assert_eq!(
        activity_window(conn, e, None, Some("2026-02-01"), today).expect("to only"),
        (date("2026-02-01"), date("2026-02-01"))
    );
}

#[test]
fn an_explicit_inverted_window_is_a_validation_error() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);
    let today = date("2026-09-15");

    assert!(matches!(
        activity_window(conn, e, Some("2026-05-01"), Some("2026-04-01"), today),
        Err(Error::Validation(ValidationError::DateRangeInverted))
    ));
    assert_eq!(
        activity_window(conn, EntityId::new(), None, None, today).expect_err("unknown"),
        Error::NotFound("entity".into())
    );
}
