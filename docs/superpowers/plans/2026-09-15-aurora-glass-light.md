# Aurora Glass: The Light (Phases 2 and 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw money as light. Rust computes a cash-flow series and four arc metrics. The web then builds the approved mockup's dashboard (hero with the cash-flow light, in/out pills, four arc tiles), the Transactions summary strip, a page-owned top bar with Quick add, and dialog type selectors in the Ledger colours.

**Architecture:** `oikonomia-core` gains `cash_flow_series` and `activity_window` (a new `ledger/cash_flow.rs`) and four computed fields on `DashboardSummary`. One IPC command and typed wrappers carry them to the web. The web splits the picture into pure, tested modules (`lib/cashFlowLight.ts` for series-to-points geometry, `lib/arc.ts` for value-to-angle) and thin painting components (`CashFlowLight` on two canvases, `Arc` in SVG). The pages then compose those pieces as in the mockup.

**Tech Stack:** Rust 1.88 (rusqlite 0.40, time 0.3.55), Tauri 2, React 19, TypeScript, Tailwind CSS 4, Vitest 4 with Testing Library.

**Spec:** `docs/superpowers/specs/2026-09-14-aurora-glass-ui-design.md` (read the "Resolved in the phase 2-3 plan" section at its end). Approved mockup: https://claude.ai/code/artifact/4a5a5904-43b0-4d9e-9a4a-4a5b48c28938 (Dashboard, Transactions, New entry frames).

**Branch:** `feat/aurora-glass-light`, created from `feat/aurora-glass-foundation` (PR #60, not merged). Open the pull request against `feat/aurora-glass-foundation`. When #60 merges, GitHub retargets it to `main`. The user asked for phases 2 and 3 as one piece (2026-09-15), so this is one PR.

## Global Constraints

- **Business logic stays in Rust** (repo CLAUDE.md: "All the buisness logic needs to live in Rust side.").
  - Every money figure, ratio, window and total is computed in `oikonomia-core`.
  - The web may format numbers, clamp a value to draw it, map numbers to pixels and angles, pick colours, and compute calendar period bounds, as `monthStartISO` already does.
  - The web never adds, subtracts or divides money to produce a figure a person reads.
- **Ledger colours** (spec, locked):
  - Money in `#1BA39A`, text tint `#6FD9C4`.
  - Money out `#E8603F`, text tint `#FF9B7E`.
  - Light gradient stops: in `#27BF93 -> #1BA39A -> #1E8DB0`, out `#F07A45 -> #E8603F -> #D64A5A`.
- **Brand light** `#2EE6A6 -> #37D5FF -> #7A8CFF` is for chrome only: primary buttons, active navigation, logo, Quick add, focus.
- **No particles:** no sparkles inside the light, no grain behind the aurora.
- **In and out differ by position** (above or below the zero line), by sign and by label, never by colour alone.
- **Motion:**
  - The light breathes only while `useMotionAllowed()` is true and the light is on screen, redrawing at most about 30 fps.
  - Otherwise it paints once, still.
  - Reduce Motion stills it.
- **Accessibility of the new graphics:**
  - The canvas carries a text alternative stating the period's in, out and net.
  - An arc is a `meter` with its value and label.
  - A `None` metric renders as an empty arc with an em dash, never as 0%.
- **Entry filter:** `ACTIVE_ENTRY_PREDICATE` (voided entries and their reversals excluded). Entry predicates go in a `WHERE` over inner joins, never in a `LEFT JOIN ... ON` clause.
- **Contrast** is measured against `BRIGHTEST_GLASS = [8, 66, 48]`, with plates composited over it.
  - Body text: at least 4.5:1.
  - Text of 24px or more, and UI graphics: at least 3:1.
- **Rust lints:**
  - clippy pedantic at `-D warnings`.
  - `#[expect(...)]`, never `#[allow(...)]`.
  - A `# Errors` section on every `pub fn` returning `Result`.
  - `#[must_use]` on pure `pub fn`s.
  - No `as` casts, no `unwrap`, no `panic`.
  - Integer literals of five or more digits use `_` separators.
- **i18n:** every new `web/src/locales/en.json` key is a flat dotted key. It is also added to `el.json`, `fr.json` and `de.json` as the same dotted path, nested. `web/src/lib/i18n.test.ts` fails otherwise ("locale parity guard", and "flatten(fr) key-set equals flatten(de) equals flatten(el)").
- **Test typing:** `web/tsconfig.app.json` excludes test files and has no `strict`, so `npx tsc -b` never checks fixtures. When a shared type changes, update every test fixture by hand.
- **Styling assertions:** new tests assert data attributes (`data-tone`, `data-net`, `data-money-pill`, `data-amount`), never Tailwind class strings.
- **Commits:** no `Co-Authored-By` or any other trailer, no emoji anywhere (code, comments, commit messages, PR text).
- **Subagent directive:** every dispatch prompt includes "Call hippius-mem recall about the task before making changes, and remember any durable decision/gotcha you discover."
- **Gates:**
  - Rust: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test -p oikonomia-core`.
  - Web: `cd web && npx tsc -b && npm run lint && npm test` (lint: 0 errors, no new warnings).

## File map

| File | Responsibility | Task |
|---|---|---|
| `crates/oikonomia-core/src/ledger/cash_flow.rs` (new) | Series, buckets, open-window resolution | 1 |
| `crates/oikonomia-core/tests/cash_flow_series.rs` (new) | Series and window invariants | 1 |
| `crates/oikonomia-core/tests/report_correctness.rs` | Randomised series == dashboard | 1 |
| `crates/oikonomia-core/src/ledger/reports.rs` | Arc metrics, `previous_window`, `ratio_bps` | 2 |
| `crates/oikonomia-core/tests/dashboard_correctness.rs` | Arc metric hand checks | 2 |
| `crates/oikonomia-core/src/ledger/mod.rs` | Exports | 1, 2 |
| `apps/desktop/src-tauri/src/commands.rs`, `lib.rs` | `cash_flow_series_cmd` | 3 |
| `web/src/lib/api.ts`, `web/src/lib/api.cashflow.test.ts` (new) | Types, wrapper, quarter bounds | 3, 9 |
| `web/src/lib/cashFlowLight.ts` (+ test) (new) | Pure light geometry and Ledger stops | 4 |
| `web/src/components/CashFlowLight.tsx` (+ test) (new) | Canvas painting, motion gate | 5 |
| `web/src/lib/arc.ts` (+ test) (new), `web/src/components/Arc.tsx` (+ test) (new) | Arc mapping, `Arc`, `ArcTile` | 6 |
| `web/src/index.css`, `web/tests/tokens.test.ts`, `web/src/components/ui.tsx` (+ test) | Light tokens, net figure, `MoneyPill`, `AmountPill`, `Segmented` tones | 7 |
| `web/src/lib/topBar.ts` (new), `web/src/components/TopBar.tsx` (+ test) (new), `web/src/App.tsx` (+ test) | Page-owned top bar, round Lock, Quick add, R19 | 8 |
| `web/src/pages/DashboardPage.tsx` (+ test) | Dashboard composition | 9 |
| `web/src/pages/TransactionsPage.tsx` (+ test), `web/src/components/DocumentDropZone.tsx` | Transactions composition | 10 |
| `web/src/pages/RecurringPage.tsx` (+ test) | Dialog type tones | 11 |
| locales `en/el/fr/de.json` | New copy | 8, 9, 10 |

---

### Task 1: Cash-flow series in Rust

**Files:**
- Create: `crates/oikonomia-core/src/ledger/cash_flow.rs`
- Modify: `crates/oikonomia-core/src/ledger/mod.rs`
- Create: `crates/oikonomia-core/tests/cash_flow_series.rs`
- Modify: `crates/oikonomia-core/tests/report_correctness.rs`

**Interfaces:**
- Consumes: `ACTIVE_ENTRY_PREDICATE`, `normal_balance`, `parse_account_type` (`ledger/balance.rs`); `get_entity`; `parse_date`, `format_date`.
- Produces (re-exported from `oikonomia_core::ledger`):

```rust
pub const DAILY_BUCKET_MAX_DAYS: i32 = 92;
pub enum CashFlowGranularity { Day, Month } // serde: "day" | "month"
pub struct CashFlowBucket {
    pub start: Date, pub end: Date,              // serde "YYYY-MM-DD"
    pub income_minor: i64, pub expenses_minor: i64,
    pub cumulative_income_minor: i64, pub cumulative_expenses_minor: i64,
}
pub struct CashFlowSeries {
    pub entity_id: EntityId, pub from: Date, pub to: Date,
    pub granularity: CashFlowGranularity,
    pub total_income_minor: i64, pub total_expenses_minor: i64, pub net_minor: i64,
    pub buckets: Vec<CashFlowBucket>,
}
pub fn cash_flow_series(conn: &Connection, entity_id: EntityId, from: &str, to: &str) -> Result<CashFlowSeries>;
pub fn activity_window(conn: &Connection, entity_id: EntityId, from: Option<&str>, to: Option<&str>, today: Date) -> Result<(Date, Date)>;
```

- [ ] **Step 1: Write the failing tests**

Create `crates/oikonomia-core/tests/cash_flow_series.rs`:

```rust
//! Cash-flow series: the numbers the light draws. They must agree with the
//! dashboard to the minor unit, cover the window exactly, and ignore voids.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::domain::{AccountId, ChartTemplate, EntityId, JournalEntryId};
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    CashFlowGranularity, CashFlowSeries, CreateEntity, CreateJournalLine, PostJournal,
    activity_window, cash_flow_series, create_entity, dashboard_summary, list_accounts,
    post_entry, set_entry_hidden, void_entry,
};
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
        assert!(bucket.start <= bucket.end, "bucket runs forward: {bucket:?}");
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
    void_entry(conn, voided).expect("void");
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
        assert_eq!(last.cumulative_income_minor, dash.income, "income {from}..{to}");
        assert_eq!(last.cumulative_expenses_minor, dash.expenses, "expenses {from}..{to}");
        assert_eq!(s.total_income_minor, dash.income, "total income {from}..{to}");
        assert_eq!(s.total_expenses_minor, dash.expenses, "total expenses {from}..{to}");
        assert_eq!(s.net_minor, dash.net_income, "net {from}..{to}");
        assert_eq!(s.buckets.iter().map(|b| b.income_minor).sum::<i64>(), dash.income);
        assert_eq!(s.buckets.iter().map(|b| b.expenses_minor).sum::<i64>(), dash.expenses);
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
    assert!(s.buckets.iter().all(|b| b.start == b.end), "one day per bucket");

    assert_eq!(s.buckets[1].cumulative_income_minor, 0, "nothing before the salary");
    let third = &s.buckets[2];
    assert_eq!(
        (third.start, third.income_minor, third.cumulative_income_minor),
        (date("2026-08-03"), 100_000, 100_000)
    );
    let fifth = &s.buckets[4];
    assert_eq!(
        (fifth.expenses_minor, fifth.cumulative_expenses_minor, fifth.cumulative_income_minor),
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
    assert_eq!((s.buckets[0].start, s.buckets[0].end), (date("2026-01-15"), date("2026-01-31")));
    assert_eq!((s.buckets[1].start, s.buckets[1].end), (date("2026-02-01"), date("2026-02-28")));
    assert_eq!((s.buckets[11].start, s.buckets[11].end), (date("2026-12-01"), date("2026-12-10")));
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
    assert_eq!((year.buckets[0].expenses_minor, year.buckets[1].expenses_minor), (1_000, 2_000));
    assert_eq!(year.buckets[0].income_minor, 500, "the window's first day counts");
    assert_eq!(year.buckets[11].income_minor, 700, "the window's last day counts");
    assert_eq!(year.total_expenses_minor, 3_000);

    let edge = series(conn, e, "2026-01-31", "2026-02-01");
    assert_eq!(
        edge.buckets.iter().map(|b| b.expenses_minor).collect::<Vec<_>>(),
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
    assert_eq!((s.total_income_minor, s.total_expenses_minor, s.net_minor), (0, 0, 0));
}

#[test]
fn inverted_and_malformed_windows_are_validation_errors() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = book(conn);

    assert!(matches!(
        cash_flow_series(conn, e, "2026-08-31", "2026-08-01"),
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        cash_flow_series(conn, e, "2026-13-01", "2026-12-31"),
        Err(Error::Validation(_))
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
    void_entry(conn, spent).expect("void expense");
    void_entry(conn, earned).expect("void income");

    for (from, to) in [("2026-08-01", "2026-08-31"), ("2026-01-01", "2026-12-31")] {
        let s = series(conn, e, from, to);
        assert!(
            s.buckets.iter().all(|b| b.income_minor == 0 && b.expenses_minor == 0),
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
    void_entry(conn, voided).expect("void");
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
        Err(Error::Validation(_))
    ));
    assert_eq!(
        activity_window(conn, EntityId::new(), None, None, today).expect_err("unknown"),
        Error::NotFound("entity".into())
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p oikonomia-core --test cash_flow_series`
Expected: FAIL to compile with "unresolved imports `oikonomia_core::ledger::CashFlowGranularity`" (and the other new names).

- [ ] **Step 3: Implement `ledger/cash_flow.rs`**

Create `crates/oikonomia-core/src/ledger/cash_flow.rs`:

```rust
//! Cash flow over a window: income and expenses per day or per calendar month,
//! with running totals, on exactly the basis `dashboard_summary` uses.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::domain::{AccountType, EntityId};
use crate::error::{Error, Result};
use crate::ledger::balance::{ACTIVE_ENTRY_PREDICATE, normal_balance, parse_account_type};
use crate::ledger::entities::get_entity;
use crate::util::{format_date, parse_date};

/// Windows of this many days or fewer get one bucket per day; longer windows
/// get one per calendar month. 92 days covers any calendar quarter.
pub const DAILY_BUCKET_MAX_DAYS: i32 = 92;

/// How a [`CashFlowSeries`] is bucketed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CashFlowGranularity {
    /// One bucket per calendar day.
    Day,
    /// One bucket per calendar month, clipped to the window.
    Month,
}

/// One bucket of a [`CashFlowSeries`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashFlowBucket {
    /// First day in the bucket (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub start: Date,
    /// Last day in the bucket (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub end: Date,
    /// Income in the bucket: credits minus debits on Income accounts.
    pub income_minor: i64,
    /// Expenses in the bucket: debits minus credits on Expense accounts.
    pub expenses_minor: i64,
    /// Income from the window's first day through `end`.
    pub cumulative_income_minor: i64,
    /// Expenses from the window's first day through `end`.
    pub cumulative_expenses_minor: i64,
}

/// Income and expenses across `[from, to]`, bucketed per day or per month.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashFlowSeries {
    /// Entity id.
    pub entity_id: EntityId,
    /// First day of the window (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub from: Date,
    /// Last day of the window (inclusive).
    #[serde(with = "crate::util::serde_date")]
    pub to: Date,
    /// Day or month buckets, chosen by the window's length.
    pub granularity: CashFlowGranularity,
    /// Income across the window; equals `dashboard_summary`'s `income`.
    pub total_income_minor: i64,
    /// Expenses across the window; equals `dashboard_summary`'s `expenses`.
    pub total_expenses_minor: i64,
    /// Income minus expenses across the window.
    pub net_minor: i64,
    /// Contiguous buckets covering `from..=to`, oldest first.
    pub buckets: Vec<CashFlowBucket>,
}

/// Income and expenses per day or per month between `from` and `to` inclusive.
///
/// Posted, active entries only (voided entries and their reversals are
/// excluded); Hidden entries count, as they do on the dashboard. The final
/// running totals always equal `dashboard_summary` for the same window.
///
/// # Errors
///
/// [`Error::Validation`] for a malformed date or `from > to`;
/// [`Error::NotFound`] for an unknown entity; database errors as [`Error::Io`].
pub fn cash_flow_series(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
) -> Result<CashFlowSeries> {
    let from_d = parse_date(from)?;
    let to_d = parse_date(to)?;
    if from_d > to_d {
        return Err(Error::Validation(
            "from date must be on or before to".into(),
        ));
    }
    let _ = get_entity(conn, entity_id)?;

    let granularity = granularity_for(from_d, to_d);
    let mut buckets: Vec<CashFlowBucket> = bucket_ranges(from_d, to_d, granularity)
        .into_iter()
        .map(|(start, end)| CashFlowBucket {
            start,
            end,
            income_minor: 0,
            expenses_minor: 0,
            cumulative_income_minor: 0,
            cumulative_expenses_minor: 0,
        })
        .collect();

    for day in daily_activity(conn, entity_id, from_d, to_d)? {
        // Buckets are sorted and contiguous, so the first one ending on or
        // after the day holds it.
        let index = buckets.partition_point(|bucket| bucket.end < day.date);
        if let Some(bucket) = buckets.get_mut(index) {
            bucket.income_minor = bucket.income_minor.saturating_add(day.income_minor);
            bucket.expenses_minor = bucket.expenses_minor.saturating_add(day.expenses_minor);
        }
    }

    let mut income = 0_i64;
    let mut expenses = 0_i64;
    for bucket in &mut buckets {
        income = income.saturating_add(bucket.income_minor);
        expenses = expenses.saturating_add(bucket.expenses_minor);
        bucket.cumulative_income_minor = income;
        bucket.cumulative_expenses_minor = expenses;
    }

    Ok(CashFlowSeries {
        entity_id,
        from: from_d,
        to: to_d,
        granularity,
        total_income_minor: income,
        total_expenses_minor: expenses,
        net_minor: income.saturating_sub(expenses),
        buckets,
    })
}

/// The window to draw when a date filter leaves one or both bounds empty.
///
/// An empty `from` becomes the book's first active entry date and an empty `to`
/// its last; with no entries, the open side falls back to the other bound, or
/// to `today`. A defaulted bound never lands on the wrong side of a given one.
///
/// # Errors
///
/// [`Error::Validation`] for a malformed date or an explicit `from > to`;
/// [`Error::NotFound`] for an unknown entity; database errors as [`Error::Io`].
pub fn activity_window(
    conn: &Connection,
    entity_id: EntityId,
    from: Option<&str>,
    to: Option<&str>,
    today: Date,
) -> Result<(Date, Date)> {
    let _ = get_entity(conn, entity_id)?;
    let from_d = from.map(parse_date).transpose()?;
    let to_d = to.map(parse_date).transpose()?;
    if let (Some(start), Some(end)) = (from_d, to_d) {
        if start > end {
            return Err(Error::Validation(
                "from date must be on or before to".into(),
            ));
        }
        return Ok((start, end));
    }

    let (earliest, latest) = active_entry_bounds(conn, entity_id)?;
    match (from_d, to_d) {
        (Some(start), None) => Ok((start, latest.unwrap_or(today).max(start))),
        (None, Some(end)) => Ok((earliest.unwrap_or(end).min(end), end)),
        _ => Ok((earliest.unwrap_or(today), latest.unwrap_or(today))),
    }
}

fn granularity_for(from: Date, to: Date) -> CashFlowGranularity {
    let days = to.to_julian_day() - from.to_julian_day() + 1;
    if days <= DAILY_BUCKET_MAX_DAYS {
        CashFlowGranularity::Day
    } else {
        CashFlowGranularity::Month
    }
}

fn bucket_ranges(from: Date, to: Date, granularity: CashFlowGranularity) -> Vec<(Date, Date)> {
    let mut ranges = Vec::new();
    let mut start = from;
    loop {
        let end = match granularity {
            CashFlowGranularity::Day => start,
            CashFlowGranularity::Month => last_day_of_month(start).min(to),
        };
        ranges.push((start, end));
        match end.next_day() {
            Some(next) if next <= to => start = next,
            _ => break,
        }
    }
    ranges
}

fn last_day_of_month(date: Date) -> Date {
    date.replace_day(date.month().length(date.year()))
        .unwrap_or(date)
}

/// Income and expenses on one calendar day.
struct DayActivity {
    date: Date,
    income_minor: i64,
    expenses_minor: i64,
}

fn daily_activity(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
) -> Result<Vec<DayActivity>> {
    let sql = format!(
        "
        SELECT je.entry_date, a.account_type,
               COALESCE(SUM(jl.debit_minor), 0),
               COALESCE(SUM(jl.credit_minor), 0)
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        JOIN accounts a ON a.id = jl.account_id
        WHERE a.entity_id = ?1
          AND a.account_type IN ('income', 'expense')
          AND je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
          AND je.entry_date >= ?2
          AND je.entry_date <= ?3
        GROUP BY je.entry_date, a.account_type
        ORDER BY je.entry_date
        "
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| Error::Io(err.to_string()))?;
    let rows = stmt
        .query_map(
            rusqlite::params![entity_id.0.to_string(), format_date(from), format_date(to)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut days: Vec<DayActivity> = Vec::new();
    for row in rows {
        let (date_text, type_text, debits, credits) =
            row.map_err(|err| Error::Io(err.to_string()))?;
        let date = parse_date(&date_text)?;
        let account_type = parse_account_type(&type_text)?;
        let amount = normal_balance(account_type, debits, credits);

        if days.last().map(|day| day.date) != Some(date) {
            days.push(DayActivity {
                date,
                income_minor: 0,
                expenses_minor: 0,
            });
        }
        if let Some(day) = days.last_mut() {
            match account_type {
                AccountType::Income => day.income_minor = day.income_minor.saturating_add(amount),
                AccountType::Expense => {
                    day.expenses_minor = day.expenses_minor.saturating_add(amount);
                }
                AccountType::Asset | AccountType::Liability | AccountType::Equity => {}
            }
        }
    }
    Ok(days)
}

fn active_entry_bounds(
    conn: &Connection,
    entity_id: EntityId,
) -> Result<(Option<Date>, Option<Date>)> {
    let sql = format!(
        "
        SELECT MIN(je.entry_date), MAX(je.entry_date)
        FROM journal_entries je
        WHERE je.entity_id = ?1
          AND je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
        "
    );
    let (earliest, latest): (Option<String>, Option<String>) = conn
        .query_row(&sql, rusqlite::params![entity_id.0.to_string()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|err| Error::Io(err.to_string()))?;

    Ok((
        earliest.as_deref().map(parse_date).transpose()?,
        latest.as_deref().map(parse_date).transpose()?,
    ))
}
```

In `crates/oikonomia-core/src/ledger/mod.rs`, add `mod cash_flow;` after `mod balance;`, and add this export block after `pub use balance::...`:

```rust
pub use cash_flow::{
    CashFlowBucket, CashFlowGranularity, CashFlowSeries, DAILY_BUCKET_MAX_DAYS, activity_window,
    cash_flow_series,
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p oikonomia-core --test cash_flow_series`
Expected: PASS, 14 tests.

- [ ] **Step 5: Extend the randomised ledgers**

In `crates/oikonomia-core/tests/report_correctness.rs`, add `cash_flow_series` to the `oikonomia_core::ledger::{...}` import. Add this helper after `fn line(...)`:

```rust
/// The light and the dashboard must never disagree, whatever the ledger holds.
fn assert_series_matches_dashboard(conn: &Connection, entity_id: EntityId, from: &str, to: &str) {
    let series = cash_flow_series(conn, entity_id, from, to).expect("series");
    let dash = dashboard_summary(conn, entity_id, from, to, to).expect("dash");
    let last = series.buckets.last().expect("bucket");
    assert_eq!(last.cumulative_income_minor, dash.income, "series income {from}..{to}");
    assert_eq!(last.cumulative_expenses_minor, dash.expenses, "series expenses {from}..{to}");
    assert_eq!(series.net_minor, dash.net_income, "series net {from}..{to}");
}
```

At the end of `randomized_entries_keep_reports_consistent`, add:

```rust
    for (from, to) in [
        ("2026-01-01", "2026-12-31"),
        ("2026-03-01", "2026-03-31"),
        ("2026-04-01", "2026-06-30"),
        ("2026-02-10", "2026-11-20"),
    ] {
        assert_series_matches_dashboard(conn, entity_id, from, to);
    }
```

At the end of `randomized_multi_year_entries_keep_tb_bs_and_ytd_pnl_aligned`, add:

```rust
    for (from, to) in [
        ("2025-01-01", "2027-12-31"),
        ("2026-07-01", "2026-09-30"),
        ("2025-11-15", "2026-02-14"),
    ] {
        assert_series_matches_dashboard(conn, entity_id, from, to);
    }
```

Add a new test at the end of the file:

```rust
#[test]
fn randomized_ledgers_with_voids_keep_the_series_equal_to_the_dashboard() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    let accounts = list_accounts(conn, entity_id).expect("accounts");

    let mut rng = StdRng::seed_from_u64(0x0915_2026);
    for index in 0..60 {
        let debit_idx = rng.random_range(0..accounts.len());
        let credit_idx = (debit_idx + rng.random_range(1..accounts.len())) % accounts.len();
        let minor = rng.random_range(1..=100_000);
        let month = rng.random_range(1..=12);
        let day = rng.random_range(1..=28);

        let view = post_entry(
            conn,
            &PostJournal {
                entity_id,
                entry_date: format!("2026-{month:02}-{day:02}"),
                description: "random".into(),
                reference: None,
                lines: vec![
                    CreateJournalLine {
                        account_id: accounts[debit_idx].id,
                        debit_minor: minor,
                        credit_minor: 0,
                        memo: None,
                    },
                    CreateJournalLine {
                        account_id: accounts[credit_idx].id,
                        debit_minor: 0,
                        credit_minor: minor,
                        memo: None,
                    },
                ],
            },
        )
        .expect("post random");
        if index % 5 == 0 {
            void_entry(conn, view.entry.id).expect("void random");
        }
    }

    for (from, to) in [
        ("2026-01-01", "2026-12-31"),
        ("2026-05-01", "2026-05-31"),
        ("2026-10-01", "2026-12-31"),
    ] {
        assert_series_matches_dashboard(conn, entity_id, from, to);
    }
}
```

- [ ] **Step 6: Run every gate for this task**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test -p oikonomia-core`
Expected: clippy clean; all core tests pass, including the 14 new ones and `randomized_ledgers_with_voids_keep_the_series_equal_to_the_dashboard`.

- [ ] **Step 7: Commit**

```bash
git add crates/oikonomia-core/src/ledger/cash_flow.rs crates/oikonomia-core/src/ledger/mod.rs crates/oikonomia-core/tests/cash_flow_series.rs crates/oikonomia-core/tests/report_correctness.rs
git commit -m "feat(core): cash-flow series for the light, on the dashboard's basis" -m "The light needs income and expenses per day (windows of 92 days or fewer) or per calendar month, with running totals. cash_flow_series computes them from posted, active entries with the same account basis as dashboard_summary, so the final totals always match it; randomised ledgers with voids pin that. activity_window resolves an open-ended Transactions date filter to the book's first and last active entries."
```

---
### Task 2: Arc metrics on the dashboard summary

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/reports.rs`
- Modify: `crates/oikonomia-core/src/ledger/mod.rs`
- Modify: `crates/oikonomia-core/tests/dashboard_correctness.rs`

**Interfaces:**
- Consumes: `sum_types_in_range`, `period_lines` (already in `reports.rs`).
- Produces (re-exported from `oikonomia_core::ledger`):

```rust
pub struct TopExpense { pub code: String, pub name: String, pub amount_minor: i64, pub share_bps: i64 }
// DashboardSummary gains:
pub savings_rate_bps: Option<i64>,     // net / income; None when income <= 0
pub spend_ratio_bps: Option<i64>,      // expenses / income; None when income <= 0
pub top_expense: Option<TopExpense>,   // None when expenses <= 0
pub net_vs_previous_bps: Option<i64>,  // (net - previous net) / |previous net|; None when previous net == 0
#[must_use] pub fn previous_window(from: Date, to: Date) -> Option<(Date, Date)>;
```

Serialized over IPC as `savings_rate_bps: number | null` and so on; `top_expense` as `{ code, name, amount_minor, share_bps } | null`.

**The previous window (plan decision D1, recorded in the spec):**
- A window of whole calendar months (from the 1st of a month to the last day of a month) steps back by the same number of calendar months: September compares with August, Q3 with Q2, and a year with the year before.
- Any other window steps back by the same number of days.
- The spec's literal "equally long window" would compare September (30 days) with 2-31 August, which is not what "last period" means to anyone reading the tile.

- [ ] **Step 1: Write the failing integration tests**

In `crates/oikonomia-core/tests/dashboard_correctness.rs`:
- Extend the ledger import to `CreateEntity, EntryFilter, PostSimpleEntry, SimpleBillStatus, SimpleEntryKind, create_entity, dashboard_summary, list_accounts, list_entries, post_simple_entry, previous_window, void_entry`.
- Add `use oikonomia_core::util::{format_date, parse_date};`.
- Append:

```rust
fn probe_book(conn: &Connection, name: &str) -> EntityId {
    create_entity(
        conn,
        &CreateEntity {
            name: name.into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity")
    .id
}

fn income_on(conn: &Connection, e: EntityId, date: &str, minor: i64) {
    let mut entry = base(e, Income, date, minor);
    entry.category_account_id = Some(account(conn, e, "4000"));
    entry.wallet_account_id = Some(account(conn, e, "1010"));
    post(conn, &entry);
}

fn expense_on(conn: &Connection, e: EntityId, date: &str, minor: i64) {
    let mut entry = base(e, Expense, date, minor);
    entry.category_account_id = Some(account(conn, e, "5100"));
    entry.wallet_account_id = Some(account(conn, e, "1010"));
    post(conn, &entry);
}

#[test]
fn arc_metrics_hand_checked() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let e = probe_book(conn, "Probe");
    seed_august_ledger(conn, e);

    let s = dashboard_summary(conn, e, "2026-08-01", "2026-08-31", "2026-08-10").expect("summary");

    assert_eq!(s.savings_rate_bps, Some(8_525), "85 247 / 100 000 = 85.247 %");
    assert_eq!(s.spend_ratio_bps, Some(1_475), "14 753 / 100 000 = 14.753 %");
    let top = s.top_expense.expect("an expense account leads");
    assert_eq!(
        (top.code.as_str(), top.name.as_str(), top.amount_minor, top.share_bps),
        ("5300", "Utilities", 7_253, 4_916),
        "the unpaid bill leads: 7 253 / 14 753 = 49.16 %"
    );
    assert_eq!(s.net_vs_previous_bps, None, "July is empty, so there is nothing to compare with");
}

#[test]
fn net_vs_previous_compares_with_the_previous_calendar_month() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");

    let grew = probe_book(conn, "Grew");
    income_on(conn, grew, "2026-07-10", 50_000);
    income_on(conn, grew, "2026-08-10", 60_000);

    let shrank = probe_book(conn, "Shrank");
    income_on(conn, shrank, "2026-07-10", 50_000);
    income_on(conn, shrank, "2026-08-10", 40_000);

    let recovered = probe_book(conn, "Recovered");
    expense_on(conn, recovered, "2026-07-10", 10_000);
    income_on(conn, recovered, "2026-08-10", 5_000);

    let change = |entity: EntityId| {
        dashboard_summary(conn, entity, "2026-08-01", "2026-08-31", "2026-08-31")
            .expect("summary")
            .net_vs_previous_bps
    };
    assert_eq!(change(grew), Some(2_000));
    assert_eq!(change(shrank), Some(-2_000));
    assert_eq!(
        change(recovered),
        Some(15_000),
        "from -10 000 to +5 000 is a rise of 150 % of the previous net's size"
    );
}

#[test]
fn arc_metrics_are_empty_when_there_is_nothing_to_divide_by() {
    let (_dir, vault) = setup();
    let conn = vault.connection().expect("conn");
    let summary = |entity: EntityId| {
        dashboard_summary(conn, entity, "2026-08-01", "2026-08-31", "2026-08-31").expect("summary")
    };

    let empty = summary(probe_book(conn, "Empty"));
    assert_eq!(
        (empty.savings_rate_bps, empty.spend_ratio_bps, empty.net_vs_previous_bps),
        (None, None, None)
    );
    assert!(empty.top_expense.is_none());

    let spender = probe_book(conn, "Spender");
    expense_on(conn, spender, "2026-08-05", 2_500);
    let spent = summary(spender);
    assert_eq!((spent.savings_rate_bps, spent.spend_ratio_bps), (None, None), "no income to divide by");
    assert_eq!(spent.top_expense.expect("top").share_bps, 10_000);

    let earner = probe_book(conn, "Earner");
    income_on(conn, earner, "2026-08-03", 1_000);
    let earned = summary(earner);
    assert_eq!((earned.savings_rate_bps, earned.spend_ratio_bps), (Some(10_000), Some(0)));
    assert!(earned.top_expense.is_none(), "no expenses, no top spend");
}

#[test]
fn previous_window_steps_back_by_calendar_months_or_by_days() {
    let window = |from: &str, to: &str| {
        previous_window(parse_date(from).expect("from"), parse_date(to).expect("to"))
            .map(|(start, end)| (format_date(start), format_date(end)))
    };
    let expect = |from: &str, to: &str| Some((from.to_owned(), to.to_owned()));

    assert_eq!(window("2026-09-01", "2026-09-30"), expect("2026-08-01", "2026-08-31"), "a month");
    assert_eq!(window("2026-07-01", "2026-09-30"), expect("2026-04-01", "2026-06-30"), "a quarter");
    assert_eq!(window("2026-01-01", "2026-12-31"), expect("2025-01-01", "2025-12-31"), "a year");
    assert_eq!(window("2026-01-01", "2026-01-31"), expect("2025-12-01", "2025-12-31"), "across new year");
    assert_eq!(window("2028-03-01", "2028-03-31"), expect("2028-02-01", "2028-02-29"), "into a leap February");
    assert_eq!(window("2026-08-05", "2026-08-14"), expect("2026-07-26", "2026-08-04"), "ten days");
    assert_eq!(window("2026-08-10", "2026-08-10"), expect("2026-08-09", "2026-08-09"), "one day");
    assert_eq!(window("2026-03-01", "2026-04-15"), expect("2026-01-14", "2026-02-28"), "not whole months");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p oikonomia-core --test dashboard_correctness`
Expected: FAIL to compile: "unresolved import `oikonomia_core::ledger::previous_window`" and "no field `savings_rate_bps` on type `DashboardSummary`".

- [ ] **Step 3: Implement the metrics**

In `crates/oikonomia-core/src/ledger/reports.rs`:

1. Add after the `DashboardSummary` struct's `recent_entry_count` field, inside the struct:

```rust
    /// Net income as a share of income, in basis points; `None` when income is
    /// zero or less.
    pub savings_rate_bps: Option<i64>,
    /// Expenses as a share of income, in basis points; `None` when income is
    /// zero or less.
    pub spend_ratio_bps: Option<i64>,
    /// The Expense account with the most spending in the window; `None` when
    /// there are no expenses.
    pub top_expense: Option<TopExpense>,
    /// Change in net income against [`previous_window`], in basis points of the
    /// previous net's size; `None` when the previous net is zero.
    pub net_vs_previous_bps: Option<i64>,
```

2. Add after the `DashboardSummary` struct:

```rust
/// The Expense account with the most spending in a dashboard window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopExpense {
    /// Account code.
    pub code: String,
    /// Account name.
    pub name: String,
    /// Spending on the account in the window.
    pub amount_minor: i64,
    /// `amount_minor` as a share of the window's expenses, in basis points.
    pub share_bps: i64,
}
```

3. In `dashboard_summary`, replace the final `Ok(DashboardSummary { ... })` with:

```rust
    let net_income = income.saturating_sub(expenses);
    let net_vs_previous_bps = match previous_window(from_d, to_d) {
        Some((previous_from, previous_to)) => {
            let previous_net = sum_types_in_range(
                conn,
                entity_id,
                &[AccountType::Income],
                previous_from,
                previous_to,
            )?
            .saturating_sub(sum_types_in_range(
                conn,
                entity_id,
                &[AccountType::Expense],
                previous_from,
                previous_to,
            )?);
            ratio_bps(
                net_income.saturating_sub(previous_net),
                previous_net.saturating_abs(),
            )
        }
        None => None,
    };
    let (savings_rate_bps, spend_ratio_bps) = if income > 0 {
        (ratio_bps(net_income, income), ratio_bps(expenses, income))
    } else {
        (None, None)
    };

    Ok(DashboardSummary {
        entity_id,
        base_currency: entity.base_currency,
        cash_like_assets,
        income,
        expenses,
        net_income,
        recent_entry_count: usize::try_from(count).unwrap_or(0),
        savings_rate_bps,
        spend_ratio_bps,
        top_expense: top_expense(conn, entity_id, from_d, to_d, expenses)?,
        net_vs_previous_bps,
    })
```

Extend its doc comment's first paragraph with: "It also carries the arc metrics (savings rate, spend ratio, top expense, and net against [`previous_window`]) in basis points, so the UI never divides."

4. Add after `dashboard_summary`:

```rust
/// The window a dashboard compares `[from, to]` against: the one just before it.
///
/// A window of whole calendar months (from the first of a month to the last
/// day of a month) steps back by the same number of calendar months, so a
/// month compares with the month before, a quarter with the quarter before and
/// a year with the year before. Any other window steps back by the same number
/// of days. `None` only at the edge of the calendar.
#[must_use]
pub fn previous_window(from: Date, to: Date) -> Option<(Date, Date)> {
    let previous_to = from.previous_day()?;
    let whole_months = from.day() == 1 && to.next_day().is_none_or(|next| next.day() == 1);
    if whole_months {
        let months = month_index(to) - month_index(from) + 1;
        return Some((date_from_month_index(month_index(from) - months)?, previous_to));
    }
    let length = to.to_julian_day() - from.to_julian_day();
    let previous_from = Date::from_julian_day(previous_to.to_julian_day() - length).ok()?;
    Some((previous_from, previous_to))
}

fn month_index(date: Date) -> i64 {
    i64::from(date.year()) * 12 + i64::from(u8::from(date.month())) - 1
}

fn date_from_month_index(index: i64) -> Option<Date> {
    let year = i32::try_from(index.div_euclid(12)).ok()?;
    let month = u8::try_from(index.rem_euclid(12) + 1).ok()?;
    Date::from_calendar_date(year, time::Month::try_from(month).ok()?, 1).ok()
}

/// The Expense account with the largest positive spend in the window; on a tie
/// the first in chart order wins.
fn top_expense(
    conn: &Connection,
    entity_id: EntityId,
    from: Date,
    to: Date,
    expenses: i64,
) -> Result<Option<TopExpense>> {
    if expenses <= 0 {
        return Ok(None);
    }
    let lines = period_lines(conn, entity_id, AccountType::Expense, from, to, false)?;
    let mut top: Option<&ReportLine> = None;
    for line in &lines {
        if line.balance_minor > 0 && top.is_none_or(|lead| line.balance_minor > lead.balance_minor) {
            top = Some(line);
        }
    }
    Ok(top.and_then(|line| {
        Some(TopExpense {
            code: line.code.clone(),
            name: line.name.clone(),
            amount_minor: line.balance_minor,
            share_bps: ratio_bps(line.balance_minor, expenses)?,
        })
    }))
}

/// `numerator / denominator` in basis points, rounded half away from zero;
/// `None` when the denominator is zero or the result does not fit in `i64`.
fn ratio_bps(numerator: i64, denominator: i64) -> Option<i64> {
    if denominator == 0 {
        return None;
    }
    let scaled = i128::from(numerator) * 10_000;
    let divisor = i128::from(denominator);
    let magnitude = (scaled.abs() + divisor.abs() / 2) / divisor.abs();
    let signed = if (scaled < 0) == (divisor < 0) {
        magnitude
    } else {
        -magnitude
    };
    i64::try_from(signed).ok()
}
```

5. Append a unit test module at the end of `reports.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_bps_rounds_half_away_from_zero() {
        assert_eq!(ratio_bps(1, 3), Some(3_333));
        assert_eq!(ratio_bps(2, 3), Some(6_667));
        assert_eq!(ratio_bps(1, 2), Some(5_000));
        assert_eq!(ratio_bps(1, 20_000), Some(1), "half a basis point rounds up");
        assert_eq!(ratio_bps(-1, 20_000), Some(-1), "and away from zero when negative");
        assert_eq!(ratio_bps(-1, 3), Some(-3_333));
        assert_eq!(ratio_bps(1, -3), Some(-3_333));
        assert_eq!(ratio_bps(-2, -3), Some(6_667));
        assert_eq!(ratio_bps(0, 7), Some(0));
    }

    #[test]
    fn ratio_bps_refuses_what_it_cannot_express() {
        assert_eq!(ratio_bps(5, 0), None, "no denominator");
        assert_eq!(ratio_bps(i64::MAX, 1), None, "does not fit in i64");
    }
}
```

In `crates/oikonomia-core/src/ledger/mod.rs`, extend the `pub use reports::{...}` list with `TopExpense` and `previous_window`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p oikonomia-core --test dashboard_correctness && cargo test -p oikonomia-core --lib ledger::reports`
Expected: PASS. That is 5 integration tests (the existing one plus 4 new) and 2 unit tests.

- [ ] **Step 5: Run every gate for this task**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test -p oikonomia-core && mkdir -p web/dist && cargo build -p oikonomia`
Expected: clippy clean, every core test passes, and the desktop crate builds. `DashboardSummary` is only constructed in `reports.rs`, so no other Rust site changes.

- [ ] **Step 6: Commit**

```bash
git add crates/oikonomia-core/src/ledger/reports.rs crates/oikonomia-core/src/ledger/mod.rs crates/oikonomia-core/tests/dashboard_correctness.rs
git commit -m "feat(core): arc metrics in basis points on the dashboard summary" -m "The four dashboard arcs read savings rate, spend ratio, top expense share and net against the previous period, all computed in Rust so the UI never divides. A window of whole calendar months compares with the same number of months before it; any other window with the same number of days. Each metric is None when there is nothing honest to divide by, and the UI draws that as an empty arc."
```

---

### Task 3: The IPC command and typed wrappers

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `web/src/lib/api.ts`
- Create: `web/src/lib/api.cashflow.test.ts`
- Modify: `web/src/pages/DashboardPage.test.tsx` (fixture only)

**Interfaces:**
- Consumes: `activity_window`, `cash_flow_series`, `CashFlowSeries` (Task 1); the new `DashboardSummary` fields (Task 2).
- Produces (TypeScript, in `web/src/lib/api.ts`):

```ts
export type TopExpense = { code: string; name: string; amount_minor: number; share_bps: number }
export type CashFlowGranularity = 'day' | 'month'
export type CashFlowBucket = { start: string; end: string; income_minor: number; expenses_minor: number; cumulative_income_minor: number; cumulative_expenses_minor: number }
export type CashFlowSeries = { entity_id: string; from: string; to: string; granularity: CashFlowGranularity; total_income_minor: number; total_expenses_minor: number; net_minor: number; buckets: CashFlowBucket[] }
// DashboardSummary gains: savings_rate_bps, spend_ratio_bps, net_vs_previous_bps: number | null; top_expense: TopExpense | null
api.cashFlowSeries(entityId: string, from: string | null, to: string | null): Promise<CashFlowSeries>
```

- [ ] **Step 1: Write the failing web test**

Create `web/src/lib/api.cashflow.test.ts`:

```ts
/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

import { invoke } from '@tauri-apps/api/core'
import { api, type CashFlowSeries, type DashboardSummary } from './api'

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-02',
  granularity: 'day',
  total_income_minor: 500,
  total_expenses_minor: 120,
  net_minor: 380,
  buckets: [
    {
      start: '2026-08-01',
      end: '2026-08-01',
      income_minor: 500,
      expenses_minor: 0,
      cumulative_income_minor: 500,
      cumulative_expenses_minor: 0,
    },
    {
      start: '2026-08-02',
      end: '2026-08-02',
      income_minor: 0,
      expenses_minor: 120,
      cumulative_income_minor: 500,
      cumulative_expenses_minor: 120,
    },
  ],
}

beforeEach(() => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
  vi.mocked(invoke).mockReset()
})

afterEach(() => {
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('cash flow over IPC', () => {
  test('cashFlowSeries passes open bounds to Rust as null', async () => {
    vi.mocked(invoke).mockResolvedValue(series)

    await expect(api.cashFlowSeries('e1', null, '2026-08-31')).resolves.toEqual(series)
    expect(invoke).toHaveBeenCalledWith('cash_flow_series_cmd', {
      entityId: 'e1',
      from: null,
      to: '2026-08-31',
    })
  })

  test('the dashboard summary carries the arc metrics, empty ones as null', async () => {
    const summary: DashboardSummary = {
      entity_id: 'e1',
      base_currency: 'EUR',
      cash_like_assets: 0,
      income: 0,
      expenses: 2500,
      net_income: -2500,
      recent_entry_count: 1,
      savings_rate_bps: null,
      spend_ratio_bps: null,
      top_expense: { code: '5100', name: 'Food', amount_minor: 2500, share_bps: 10000 },
      net_vs_previous_bps: null,
    }
    vi.mocked(invoke).mockResolvedValue(summary)

    const result = await api.dashboardSummary('e1', '2026-08-01', '2026-08-31', '2026-08-15')
    expect(result.savings_rate_bps).toBeNull()
    expect(result.top_expense?.share_bps).toBe(10000)
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd web && npx vitest run src/lib/api.cashflow.test.ts`
Expected: FAIL with "api.cashFlowSeries is not a function".

- [ ] **Step 3: Add the command and the wrappers**

In `apps/desktop/src-tauri/src/commands.rs`:
- Add `CashFlowSeries`, `activity_window` and `cash_flow_series` to the `use oikonomia_core::ledger::{...}` list.
- Add `use oikonomia_core::util::{format_date, utc_today};` beside the other `oikonomia_core` imports.
- Add after `dashboard_summary_cmd`:

```rust
/// Income and expenses per day or month for the cash-flow light. An empty
/// bound resolves to the book's first or last active entry (Transactions with
/// no date filter); both bounds set is the dashboard's period.
#[tauri::command]
pub async fn cash_flow_series_cmd(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: Option<String>,
    to: Option<String>,
) -> CommandResult<CashFlowSeries> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        let (start, end) =
            activity_window(conn, entity_id, from.as_deref(), to.as_deref(), utc_today())?;
        cash_flow_series(conn, entity_id, &format_date(start), &format_date(end))
    })
    .await
}
```

In `apps/desktop/src-tauri/src/lib.rs`, add `commands::cash_flow_series_cmd,` on the line after `commands::dashboard_summary_cmd,`.

In `web/src/lib/api.ts`, replace the `DashboardSummary` type with:

```ts
/** The Expense account with the most spending in a dashboard window. */
export type TopExpense = {
  code: string
  name: string
  amount_minor: number
  /** Share of the window's expenses, in basis points. */
  share_bps: number
}

export type DashboardSummary = {
  entity_id: string
  base_currency: string
  cash_like_assets: number
  income: number
  expenses: number
  net_income: number
  recent_entry_count: number
  /** Net as a share of income, in basis points; null when income is zero or less. */
  savings_rate_bps: number | null
  /** Expenses as a share of income, in basis points; null when income is zero or less. */
  spend_ratio_bps: number | null
  /** Largest Expense account in the window; null when there are no expenses. */
  top_expense: TopExpense | null
  /** Net against the previous period, in basis points of its size; null when that net is zero. */
  net_vs_previous_bps: number | null
}

export type CashFlowGranularity = 'day' | 'month'

export type CashFlowBucket = {
  start: string
  end: string
  income_minor: number
  expenses_minor: number
  cumulative_income_minor: number
  cumulative_expenses_minor: number
}

/** Income and expenses per day or month, computed in Rust on the dashboard's basis. */
export type CashFlowSeries = {
  entity_id: string
  from: string
  to: string
  granularity: CashFlowGranularity
  total_income_minor: number
  total_expenses_minor: number
  net_minor: number
  buckets: CashFlowBucket[]
}
```

Add to the `api` object after `dashboardSummary`:

```ts
  /**
   * The cash-flow light's data. A null bound lets Rust resolve it to the
   * book's first or last active entry.
   */
  cashFlowSeries: (entityId: string, from: string | null, to: string | null) =>
    call<CashFlowSeries>('cash_flow_series_cmd', { entityId, from, to }),
```

In `web/src/pages/DashboardPage.test.tsx`, add these fields to the `summary` fixture after `recent_entry_count: 2,`:

```ts
    savings_rate_bps: null,
    spend_ratio_bps: null,
    top_expense: null,
    net_vs_previous_bps: null,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/lib/api.cashflow.test.ts src/pages/DashboardPage.test.tsx`
Expected: PASS.

- [ ] **Step 5: Run every gate for this task**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && mkdir -p web/dist && cargo build -p oikonomia && cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs web/src/lib/api.ts web/src/lib/api.cashflow.test.ts web/src/pages/DashboardPage.test.tsx
git commit -m "feat: expose the cash-flow series and arc metrics over IPC" -m "cash_flow_series_cmd resolves an open date filter in Rust and returns the series with its totals; the web gets typed wrappers for it and for the dashboard summary's new basis-point fields."
```

---
### Task 4: The light's geometry, as a pure module

**Files:**
- Create: `web/src/lib/cashFlowLight.ts`
- Create: `web/src/lib/cashFlowLight.test.ts`

**Interfaces:**
- Consumes: `CashFlowSeries` (Task 3).
- Produces:

```ts
export const LEDGER_IN_STOPS: readonly ['#27bf93', '#1ba39a', '#1e8db0']
export const LEDGER_OUT_STOPS: readonly ['#f07a45', '#e8603f', '#d64a5a']
export type LedgerStops = readonly [string, string, string]
export type LightPoint = { x: number; y: number }
export type LightGeometry = { zeroY: number; inEdge: LightPoint[]; outEdge: LightPoint[] }
export type LightLayout = { width: number; height: number; samples?: number; time?: number; breathe?: number }
export function ease(u: number): number
export function cumulativeAt(amounts: readonly number[], u: number): number
export function zeroSplit(maxIn: number, maxOut: number): number
export function lightGeometry(series: CashFlowSeries | null, layout: LightLayout): LightGeometry
export function ledgerColourAt(stops: LedgerStops, t: number, alpha?: number): string
```

The geometry follows the approved mockup's drawing:
- In rises from the zero line and out hangs below it, on one shared scale.
- Each bucket's amount eases in across its own width, so the edge passes exactly through every running total at the bucket's end.
- The top 16% of the canvas stays empty for the text above it.

Unlike the mockup, the zero line moves: in gets the share of the height its peak needs (clamped to 30-80%). The mockup's fixed 74% only fit because out happened to be small, and a month where expenses beat income would run off the canvas.

- [ ] **Step 1: Write the failing tests**

Create `web/src/lib/cashFlowLight.test.ts`:

```ts
import { describe, expect, test } from 'vitest'

import type { CashFlowBucket, CashFlowSeries } from './api'
import {
  LEDGER_IN_STOPS,
  LEDGER_OUT_STOPS,
  cumulativeAt,
  ease,
  ledgerColourAt,
  lightGeometry,
  zeroSplit,
} from './cashFlowLight'

function bucket(day: number, income: number, expenses: number, cumIn: number, cumOut: number): CashFlowBucket {
  const iso = `2026-08-${String(day).padStart(2, '0')}`
  return {
    start: iso,
    end: iso,
    income_minor: income,
    expenses_minor: expenses,
    cumulative_income_minor: cumIn,
    cumulative_expenses_minor: cumOut,
  }
}

function seriesOf(buckets: CashFlowBucket[]): CashFlowSeries {
  const last = buckets[buckets.length - 1]
  return {
    entity_id: 'e1',
    from: buckets[0]?.start ?? '2026-08-01',
    to: last?.end ?? '2026-08-01',
    granularity: 'day',
    total_income_minor: last?.cumulative_income_minor ?? 0,
    total_expenses_minor: last?.cumulative_expenses_minor ?? 0,
    net_minor: (last?.cumulative_income_minor ?? 0) - (last?.cumulative_expenses_minor ?? 0),
    buckets,
  }
}

// In 300 on day 2, out 100 on day 3.
const august = seriesOf([
  bucket(1, 0, 0, 0, 0),
  bucket(2, 300, 0, 300, 0),
  bucket(3, 0, 100, 300, 100),
  bucket(4, 0, 0, 300, 100),
])

const layout = { width: 400, height: 200, samples: 41 }
// top = 200 * 0.16 = 32; plot = 200 - 32 - 4 = 164.
const TOP = 32
const BOTTOM = 196

describe('the Ledger stops', () => {
  test('are the spec gradient stops, in and out', () => {
    expect(LEDGER_IN_STOPS).toEqual(['#27bf93', '#1ba39a', '#1e8db0'])
    expect(LEDGER_OUT_STOPS).toEqual(['#f07a45', '#e8603f', '#d64a5a'])
  })

  test('ledgerColourAt walks the three stops', () => {
    expect(ledgerColourAt(LEDGER_IN_STOPS, 0, 0.5)).toBe('rgba(39,191,147,0.5)')
    expect(ledgerColourAt(LEDGER_IN_STOPS, 0.5)).toBe('rgba(27,163,154,1)')
    expect(ledgerColourAt(LEDGER_IN_STOPS, 1)).toBe('rgba(30,141,176,1)')
    expect(ledgerColourAt(LEDGER_OUT_STOPS, 2)).toBe('rgba(214,74,90,1)')
  })
})

describe('easing and running totals', () => {
  test('ease is flat outside the step and an S-curve across it', () => {
    expect([ease(-1), ease(0), ease(0.5), ease(1), ease(2)]).toEqual([0, 0, 0.5, 1, 1])
  })

  test('the edge passes through every bucket running total at the bucket end', () => {
    const amounts = [0, 300, 0, 0]
    expect([0, 1, 2, 3, 4].map((u) => cumulativeAt(amounts, u))).toEqual([0, 0, 300, 300, 300])
  })
})

describe('zeroSplit', () => {
  test('gives in the share of the height its peak needs, within 30-80%', () => {
    expect(zeroSplit(0, 0)).toBe(0.74)
    expect(zeroSplit(300, 100)).toBe(0.75)
    expect(zeroSplit(100, 0)).toBe(0.8)
    expect(zeroSplit(0, 100)).toBe(0.3)
    expect(zeroSplit(1, 99)).toBe(0.3)
  })
})

describe('lightGeometry', () => {
  test('in rises above the zero line and out hangs below it, edge to edge', () => {
    const g = lightGeometry(august, layout)

    expect(g.inEdge).toHaveLength(41)
    expect(g.inEdge[0]?.x).toBe(0)
    expect(g.inEdge[40]?.x).toBe(400)
    expect(g.zeroY).toBeCloseTo(TOP + 164 * 0.75)
    expect(g.inEdge[0]?.y).toBeCloseTo(g.zeroY)
    expect(g.inEdge.every((p) => p.y <= g.zeroY + 1e-9)).toBe(true)
    expect(g.outEdge.every((p) => p.y >= g.zeroY - 1e-9)).toBe(true)
  })

  test('both sides share one scale and fill the canvas without leaving it', () => {
    const g = lightGeometry(august, layout)
    const inLift = g.zeroY - (g.inEdge[40]?.y ?? 0)
    const outLift = (g.outEdge[40]?.y ?? 0) - g.zeroY

    expect(inLift / outLift).toBeCloseTo(3)
    expect(g.inEdge[40]?.y).toBeCloseTo(TOP)
    expect(g.outEdge[40]?.y).toBeCloseTo(BOTTOM)
  })

  test('a month where out beats in still fits, on the same scale', () => {
    const heavy = seriesOf([bucket(1, 100, 0, 100, 0), bucket(2, 0, 400, 100, 400)])
    const g = lightGeometry(heavy, layout)
    const inLift = g.zeroY - (g.inEdge[40]?.y ?? 0)
    const outLift = (g.outEdge[40]?.y ?? 0) - g.zeroY

    expect(outLift / inLift).toBeCloseTo(4)
    for (const p of [...g.inEdge, ...g.outEdge]) {
      expect(p.y).toBeGreaterThanOrEqual(TOP - 1e-9)
      expect(p.y).toBeLessThanOrEqual(BOTTOM + 1e-9)
    }
  })

  test('no series draws a flat line at rest', () => {
    const g = lightGeometry(null, layout)

    expect(g.zeroY).toBeCloseTo(TOP + 164 * 0.74)
    expect([...g.inEdge, ...g.outEdge].every((p) => Math.abs(p.y - g.zeroY) < 1e-9)).toBe(true)
  })

  test('a running total below zero stays on the zero line', () => {
    const refunds = seriesOf([bucket(1, 0, 0, 0, 0), bucket(2, -50, 0, -50, 0)])
    const g = lightGeometry(refunds, layout)

    expect(g.inEdge.every((p) => Math.abs(p.y - g.zeroY) < 1e-9)).toBe(true)
  })

  test('holding still ignores time; breathing moves the edge but never across the line', () => {
    const still = lightGeometry(august, { ...layout, time: 0, breathe: 0 })
    const later = lightGeometry(august, { ...layout, time: 5, breathe: 0 })
    expect(later).toEqual(still)

    const a = lightGeometry(august, { ...layout, time: 0, breathe: 1 })
    const b = lightGeometry(august, { ...layout, time: 1.3, breathe: 1 })
    expect(b.inEdge.map((p) => p.y)).not.toEqual(a.inEdge.map((p) => p.y))
    expect(b.inEdge.every((p) => p.y <= b.zeroY + 1e-9)).toBe(true)
    expect(b.outEdge.every((p) => p.y >= b.zeroY - 1e-9)).toBe(true)
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run src/lib/cashFlowLight.test.ts`
Expected: FAIL with "Failed to resolve import './cashFlowLight'".

- [ ] **Step 3: Implement the module**

Create `web/src/lib/cashFlowLight.ts`:

```ts
import type { CashFlowSeries } from './api'

/** The Ledger light for money in, left to right (spec: Rendering). */
export const LEDGER_IN_STOPS = ['#27bf93', '#1ba39a', '#1e8db0'] as const
/** The Ledger light for money out, left to right. */
export const LEDGER_OUT_STOPS = ['#f07a45', '#e8603f', '#d64a5a'] as const

export type LedgerStops = readonly [string, string, string]

export type LightPoint = { x: number; y: number }

export type LightGeometry = {
  /** Y of the zero line, in CSS pixels from the top of the canvas. */
  zeroY: number
  /** Upper edge of the in band, left to right; never below zeroY. */
  inEdge: LightPoint[]
  /** Lower edge of the out band, left to right; never above zeroY. */
  outEdge: LightPoint[]
}

export type LightLayout = {
  width: number
  height: number
  /** Points per edge, both ends included. */
  samples?: number
  /** Seconds; only matters while breathing. */
  time?: number
  /** 0 holds the light still; 1 is the full, gentle breathing amplitude. */
  breathe?: number
}

/** The top of the canvas stays clear for the text drawn above the light. */
const TOP_CLEARANCE = 0.16
const BOTTOM_CLEARANCE = 4
/** Where the zero line rests when there is nothing to draw (the mockup's). */
const RESTING_SPLIT = 0.74
const MIN_SPLIT = 0.3
const MAX_SPLIT = 0.8

/** Smoothstep: 0 before the step, 1 after it, an S-curve across it. */
export function ease(u: number): number {
  if (u <= 0) return 0
  if (u >= 1) return 1
  return u * u * (3 - 2 * u)
}

/**
 * The running total at `u`, measured in buckets from the window's start. Each
 * bucket's amount rises across its own width, so the result equals the
 * cumulative total exactly at every bucket's end.
 */
export function cumulativeAt(amounts: readonly number[], u: number): number {
  let total = 0
  amounts.forEach((amount, index) => {
    total += amount * ease(u - index)
  })
  return total
}

/** Share of the plot height above the zero line: in's share of both peaks. */
export function zeroSplit(maxIn: number, maxOut: number): number {
  if (maxIn <= 0 && maxOut <= 0) return RESTING_SPLIT
  const share = maxIn / (Math.max(0, maxIn) + Math.max(0, maxOut))
  return Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, share))
}

/** Maps a series onto canvas points. Pure: it draws nothing and reads no DOM. */
export function lightGeometry(
  series: CashFlowSeries | null,
  { width, height, samples = 140, time = 0, breathe = 0 }: LightLayout,
): LightGeometry {
  const top = height * TOP_CLEARANCE
  const plot = Math.max(0, height - top - BOTTOM_CLEARANCE)
  const buckets = series?.buckets ?? []
  const maxIn = buckets.reduce((peak, b) => Math.max(peak, b.cumulative_income_minor), 0)
  const maxOut = buckets.reduce((peak, b) => Math.max(peak, b.cumulative_expenses_minor), 0)

  const split = zeroSplit(maxIn, maxOut)
  const zeroY = top + plot * split
  // One scale for both sides: whichever would overflow its room sets it.
  const inScale = maxIn > 0 ? (plot * split) / maxIn : Number.POSITIVE_INFINITY
  const outScale = maxOut > 0 ? (plot * (1 - split)) / maxOut : Number.POSITIVE_INFINITY
  const scale = Math.min(inScale, outScale)
  const pixelsPerMinor = Number.isFinite(scale) ? scale : 0

  const count = Math.max(2, Math.round(samples))
  const n = buckets.length
  const wobbleCap = Math.min(2.4, height / 70)

  const edge = (amounts: number[], direction: -1 | 1, phase: number): LightPoint[] => {
    const points: LightPoint[] = []
    for (let i = 0; i < count; i += 1) {
      const t = i / (count - 1)
      const value = n === 0 ? 0 : Math.max(0, cumulativeAt(amounts, t * n))
      const lift = value * pixelsPerMinor
      // The wobble is at most a fifth of a thin band, so it never crosses the line.
      const wobble =
        breathe * Math.sin(i * 0.19 + time * 0.9 + phase) * wobbleCap * Math.min(1, lift / 12)
      points.push({ x: t * width, y: zeroY + direction * Math.max(0, lift + wobble) })
    }
    return points
  }

  return {
    zeroY,
    inEdge: edge(
      buckets.map((b) => b.income_minor),
      -1,
      0,
    ),
    outEdge: edge(
      buckets.map((b) => b.expenses_minor),
      1,
      2.1,
    ),
  }
}

function channels(hex: string): [number, number, number] {
  return [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16)) as [number, number, number]
}

/** The colour `t` of the way along a three-stop gradient (stops at 0, 0.5 and 1). */
export function ledgerColourAt(stops: LedgerStops, t: number, alpha = 1): string {
  const u = Math.min(1, Math.max(0, t))
  const [from, to, k] = u <= 0.5 ? [stops[0], stops[1], u * 2] : [stops[1], stops[2], (u - 0.5) * 2]
  const a = channels(from)
  const b = channels(to)
  const mix = a.map((value, i) => Math.round(value + ((b[i] ?? value) - value) * k))
  return `rgba(${mix.join(',')},${alpha})`
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/lib/cashFlowLight.test.ts`
Expected: PASS, 11 tests.

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/lib/cashFlowLight.ts web/src/lib/cashFlowLight.test.ts
git commit -m "feat(web): the cash-flow light's geometry as a pure, tested module" -m "Series to canvas points: in rises above a zero line and out hangs below it on one shared scale, easing through each bucket's running total. The zero line sits where both peaks fit, so a month where expenses beat income no longer runs off the canvas. Breathing is bounded so an edge never crosses the line."
```

---
### Task 5: `CashFlowLight`, the component that paints

**Files:**
- Create: `web/src/components/CashFlowLight.tsx`
- Create: `web/src/components/CashFlowLight.test.tsx`
- Modify: `web/src/index.css` (one rule in `@layer components`)

**Interfaces:**
- Consumes: `lightGeometry`, `ledgerColourAt`, `LEDGER_IN_STOPS`, `LEDGER_OUT_STOPS`, `LightPoint`, `LedgerStops` (Task 4); `useMotionAllowed` (`web/src/lib/motion.ts`); `CashFlowSeries` (Task 3).
- Produces:

```tsx
export function CashFlowLight(props: {
  series: CashFlowSeries | null
  /** Text alternative: the period's in, out and net, already formatted. */
  label: string
  /** Must give the element a height (e.g. "h-[170px]"). */
  className?: string
}): JSX.Element
```

It renders `<div role="img" aria-label={label}>` holding two `aria-hidden` canvases: a blurred glow layer (fills only) and a sharp layer (gradient fill at low alpha, a soft vertical sheen, the zero line, and bright ridges on both edges), painted exactly as in the approved mockup.

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/CashFlowLight.test.tsx`:

```tsx
/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

import type { CashFlowSeries } from '../lib/api'
import { CashFlowLight } from './CashFlowLight'

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-02',
  granularity: 'day',
  total_income_minor: 500,
  total_expenses_minor: 120,
  net_minor: 380,
  buckets: [
    { start: '2026-08-01', end: '2026-08-01', income_minor: 500, expenses_minor: 0, cumulative_income_minor: 500, cumulative_expenses_minor: 0 },
    { start: '2026-08-02', end: '2026-08-02', income_minor: 0, expenses_minor: 120, cumulative_income_minor: 500, cumulative_expenses_minor: 120 },
  ],
}

function fakeContext() {
  const gradient = { addColorStop: vi.fn() }
  return {
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    createLinearGradient: vi.fn(() => gradient),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    closePath: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    shadowBlur: 0,
    shadowColor: '',
    globalCompositeOperation: 'source-over',
  }
}

let context: ReturnType<typeof fakeContext> | null

beforeEach(() => {
  context = fakeContext()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation((() => context) as never)
  vi.spyOn(Element.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(Element.prototype, 'clientHeight', 'get').mockReturnValue(170)
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('CashFlowLight', () => {
  test('is an image named by its text alternative; the canvases stay silent', () => {
    render(<CashFlowLight series={series} label="Money in 5,00 €, money out 1,20 €, net +3,80 €, August 2026." className="h-[170px]" />)

    const light = screen.getByRole('img', { name: 'Money in 5,00 €, money out 1,20 €, net +3,80 €, August 2026.' })
    const canvases = light.querySelectorAll('canvas')
    expect(canvases).toHaveLength(2)
    canvases.forEach((canvas) => expect(canvas).toHaveAttribute('aria-hidden', 'true'))
  })

  test('paints both bands once it has a box to paint into', () => {
    render(<CashFlowLight series={series} label="light" />)

    expect(context?.fill).toHaveBeenCalled()
    expect(context?.stroke).toHaveBeenCalled()
    expect(context?.createLinearGradient).toHaveBeenCalled()
  })

  test('holds still when the window is not focused', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)
    const frame = vi.spyOn(window, 'requestAnimationFrame').mockImplementation(() => 1)

    render(<CashFlowLight series={series} label="light" />)

    expect(frame).not.toHaveBeenCalled()
  })

  test('breathes while the window is visible and focused', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const frame = vi.spyOn(window, 'requestAnimationFrame').mockImplementation(() => 1)
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined)

    render(<CashFlowLight series={series} label="light" />)

    expect(frame).toHaveBeenCalled()
  })

  test('survives a webview with no 2D canvas', () => {
    context = null

    expect(() => render(<CashFlowLight series={series} label="light" />)).not.toThrow()
    expect(screen.getByRole('img', { name: 'light' })).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run src/components/CashFlowLight.test.tsx`
Expected: FAIL with "Failed to resolve import './CashFlowLight'".

- [ ] **Step 3: Implement the component**

Create `web/src/components/CashFlowLight.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react'

import type { CashFlowSeries } from '../lib/api'
import {
  LEDGER_IN_STOPS,
  LEDGER_OUT_STOPS,
  ledgerColourAt,
  lightGeometry,
  type LedgerStops,
  type LightPoint,
} from '../lib/cashFlowLight'
import { cn } from '../lib/cn'
import { useMotionAllowed } from '../lib/motion'

/** Redraw at most about 30 times a second while breathing. */
const FRAME_MS = 33

type Layer = 'glow' | 'sharp'

function traceBand(ctx: CanvasRenderingContext2D, edge: LightPoint[], zeroY: number) {
  const first = edge[0]
  const last = edge[edge.length - 1]
  if (!first || !last) return
  ctx.beginPath()
  ctx.moveTo(first.x, zeroY)
  for (const point of edge) ctx.lineTo(point.x, point.y)
  ctx.lineTo(last.x, zeroY)
  ctx.closePath()
}

function fillBand(
  ctx: CanvasRenderingContext2D,
  edge: LightPoint[],
  zeroY: number,
  width: number,
  stops: LedgerStops,
  alpha: number,
) {
  const gradient = ctx.createLinearGradient(0, 0, width, 0)
  for (const offset of [0, 0.55, 1]) gradient.addColorStop(offset, ledgerColourAt(stops, offset, alpha))
  traceBand(ctx, edge, zeroY)
  ctx.fillStyle = gradient
  ctx.fill()
}

/** A soft white sheen, brightest at the band's far edge, fading to the line. */
function sheen(ctx: CanvasRenderingContext2D, edge: LightPoint[], zeroY: number, rising: boolean) {
  if (edge.length === 0) return
  const ys = edge.map((point) => point.y)
  const far = rising ? Math.min(...ys) : Math.max(...ys)
  if (far === zeroY) return
  const gradient = ctx.createLinearGradient(0, far, 0, zeroY)
  gradient.addColorStop(0, 'rgba(255,255,255,0.2)')
  gradient.addColorStop(0.35, 'rgba(255,255,255,0.05)')
  gradient.addColorStop(1, 'rgba(255,255,255,0)')
  ctx.save()
  ctx.globalCompositeOperation = 'lighter'
  traceBand(ctx, edge, zeroY)
  ctx.fillStyle = gradient
  ctx.fill()
  ctx.restore()
}

/** The bright edge: a glowing gradient stroke with a fine white core. */
function ridge(ctx: CanvasRenderingContext2D, edge: LightPoint[], width: number, stops: LedgerStops) {
  const first = edge[0]
  if (!first) return
  const gradient = ctx.createLinearGradient(0, 0, width, 0)
  gradient.addColorStop(0, ledgerColourAt(stops, 0, 0.95))
  gradient.addColorStop(1, ledgerColourAt(stops, 1, 0.95))
  ctx.save()
  ctx.shadowColor = ledgerColourAt(stops, 0.5, 0.9)
  ctx.shadowBlur = 10
  ctx.beginPath()
  ctx.moveTo(first.x, first.y)
  for (const point of edge.slice(1)) ctx.lineTo(point.x, point.y)
  ctx.strokeStyle = gradient
  ctx.lineWidth = 1.6
  ctx.stroke()
  ctx.shadowBlur = 0
  ctx.strokeStyle = 'rgba(255,255,255,0.55)'
  ctx.lineWidth = 0.6
  ctx.stroke()
  ctx.restore()
}

function paint(
  canvas: HTMLCanvasElement,
  layer: Layer,
  series: CashFlowSeries | null,
  time: number,
  breathe: number,
) {
  const width = canvas.clientWidth
  const height = canvas.clientHeight
  if (width === 0 || height === 0) return
  const ctx = canvas.getContext('2d')
  if (!ctx) return

  const dpr = Math.min(2, window.devicePixelRatio || 1)
  const pixelWidth = Math.round(width * dpr)
  const pixelHeight = Math.round(height * dpr)
  if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
    canvas.width = pixelWidth
    canvas.height = pixelHeight
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.clearRect(0, 0, width, height)

  const { zeroY, inEdge, outEdge } = lightGeometry(series, { width, height, time, breathe })

  if (layer === 'glow') {
    fillBand(ctx, inEdge, zeroY, width, LEDGER_IN_STOPS, 0.8)
    fillBand(ctx, outEdge, zeroY, width, LEDGER_OUT_STOPS, 0.8)
    return
  }

  fillBand(ctx, inEdge, zeroY, width, LEDGER_IN_STOPS, 0.34)
  sheen(ctx, inEdge, zeroY, true)
  fillBand(ctx, outEdge, zeroY, width, LEDGER_OUT_STOPS, 0.3)
  sheen(ctx, outEdge, zeroY, false)

  ctx.strokeStyle = 'rgba(255,255,255,0.14)'
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.moveTo(0, Math.round(zeroY) + 0.5)
  ctx.lineTo(width, Math.round(zeroY) + 0.5)
  ctx.stroke()

  ridge(ctx, inEdge, width, LEDGER_IN_STOPS)
  ridge(ctx, outEdge, width, LEDGER_OUT_STOPS)
}

/**
 * Money drawn as light: in rises above a zero line, out hangs below it, on one
 * scale. It breathes gently only while motion is allowed and it is on screen;
 * otherwise it is painted once and held still. The numbers it draws come from
 * Rust; this component only paints them.
 */
export function CashFlowLight({
  series,
  label,
  className = '',
}: {
  series: CashFlowSeries | null
  label: string
  className?: string
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  const glowRef = useRef<HTMLCanvasElement>(null)
  const sharpRef = useRef<HTMLCanvasElement>(null)
  const motionAllowed = useMotionAllowed()
  const [onScreen, setOnScreen] = useState(true)

  useEffect(() => {
    const root = rootRef.current
    if (!root || typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver((entries) => {
      const latest = entries[entries.length - 1]
      if (latest) setOnScreen(latest.isIntersecting)
    })
    observer.observe(root)
    return () => observer.disconnect()
  }, [])

  const breathing = motionAllowed && onScreen

  useEffect(() => {
    const root = rootRef.current
    const draw = (time: number, breathe: number) => {
      if (glowRef.current) paint(glowRef.current, 'glow', series, time, breathe)
      if (sharpRef.current) paint(sharpRef.current, 'sharp', series, time, breathe)
    }

    if (!breathing || typeof requestAnimationFrame !== 'function') {
      // At rest: paint once, and again only when the box changes size.
      draw(0, 0)
      if (!root || typeof ResizeObserver === 'undefined') return
      const observer = new ResizeObserver(() => draw(0, 0))
      observer.observe(root)
      return () => observer.disconnect()
    }

    let frame = 0
    let last = Number.NEGATIVE_INFINITY
    const tick = (now: number) => {
      frame = requestAnimationFrame(tick)
      if (now - last < FRAME_MS) return
      last = now
      draw(now / 1000, 1)
    }
    draw(0, 1)
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [series, breathing])

  return (
    <div ref={rootRef} role="img" aria-label={label} className={cn('pointer-events-none relative', className)}>
      <canvas ref={glowRef} aria-hidden="true" className="cash-flow-glow absolute inset-0 size-full" />
      <canvas ref={sharpRef} aria-hidden="true" className="absolute inset-0 size-full" />
    </div>
  )
}
```

In `web/src/index.css`, inside the first `@layer components { ... }` block, after the `.glass-scrim` rule, add:

```css
  /* The light's glow layer: fills only, blurred into a halo behind the sharp
     layer. Screen blending keeps it light, never muddy, over the glass. */
  .cash-flow-glow {
    filter: blur(14px) saturate(1.3);
    opacity: 0.9;
    mix-blend-mode: screen;
  }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/components/CashFlowLight.test.tsx`
Expected: PASS, 5 tests.

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/CashFlowLight.tsx web/src/components/CashFlowLight.test.tsx web/src/index.css
git commit -m "feat(web): CashFlowLight paints money as light on two canvases" -m "A blurred glow layer under a sharp layer with sheen, zero line and bright ridges, as in the approved mockup. It breathes at about 30 fps only while motion is allowed and it is on screen, otherwise paints once and holds still, and it carries a text alternative with the period's in, out and net."
```

---

### Task 6: Arcs

**Files:**
- Create: `web/src/lib/arc.ts`
- Create: `web/src/lib/arc.test.ts`
- Create: `web/src/components/Arc.tsx`
- Create: `web/src/components/Arc.test.tsx`

**Interfaces:**
- Consumes: `LEDGER_IN_STOPS`, `LEDGER_OUT_STOPS` (Task 4).
- Produces:

```ts
// web/src/lib/arc.ts
export const ARC_START_DEG = 135
export const ARC_SWEEP_DEG = 270
export function arcFraction(bps: number | null | undefined, fullScaleBps?: number): number | null
export function arcEndDeg(fraction: number): number
export function arcPoint(cx: number, cy: number, radius: number, deg: number): { x: number; y: number }
export function arcPath(cx: number, cy: number, radius: number, fromDeg: number, toDeg: number): string
export function formatPercentFromBps(bps: number, locale: string, options?: { signed?: boolean }): string
```

```tsx
// web/src/components/Arc.tsx
export type ArcTone = 'in' | 'out'
export function Arc(props: { fraction: number | null; tone: ArcTone; className?: string }): JSX.Element
export function ArcTile(props: {
  label: string; hint?: string; bps: number | null; tone: ArcTone; locale: string
  noValueLabel: string; signed?: boolean; fullScaleBps?: number
}): JSX.Element
```

`ArcTile` is a glass tile holding a `role="meter"`: `aria-valuenow` is 0-100, `aria-valuetext` is the formatted percent (or `noValueLabel`), and `aria-describedby` points at the hint. `signed` tiles (net against the previous period) light the arc by the magnitude of the change.

- [ ] **Step 1: Write the failing tests**

Create `web/src/lib/arc.test.ts`:

```ts
import { describe, expect, test } from 'vitest'

import { arcEndDeg, arcFraction, arcPath, arcPoint, formatPercentFromBps } from './arc'

describe('arcFraction', () => {
  test('maps basis points onto the arc, clamped', () => {
    expect(arcFraction(6900)).toBe(0.69)
    expect(arcFraction(0)).toBe(0)
    expect(arcFraction(-500)).toBe(0)
    expect(arcFraction(25000)).toBe(1)
    expect(arcFraction(2500, 5000)).toBe(0.5)
  })

  test('has no fraction for a missing value', () => {
    expect(arcFraction(null)).toBeNull()
    expect(arcFraction(undefined)).toBeNull()
    expect(arcFraction(Number.NaN)).toBeNull()
  })
})

describe('arc drawing', () => {
  test('sweeps 270 degrees clockwise from the lower left', () => {
    expect([arcEndDeg(0), arcEndDeg(0.5), arcEndDeg(1)]).toEqual([135, 270, 405])
    const start = arcPoint(35, 35, 26, 135)
    expect(start.x).toBeCloseTo(16.615, 2)
    expect(start.y).toBeCloseTo(53.385, 2)
  })

  test('uses the large-arc flag only past 180 degrees', () => {
    expect(arcPath(35, 35, 26, 135, 405)).toBe('M16.62 53.38A26 26 0 1 1 53.38 53.38')
    expect(arcPath(35, 35, 26, 135, 270)).toBe('M16.62 53.38A26 26 0 0 1 35.00 9.00')
  })
})

describe('formatPercentFromBps', () => {
  test('shows one decimal in the reader locale', () => {
    expect(formatPercentFromBps(6900, 'en')).toBe('69.0')
    expect(formatPercentFromBps(6900, 'el')).toBe('69,0')
  })

  test('signs a change only when asked', () => {
    expect(formatPercentFromBps(2000, 'en', { signed: true })).toBe('+20.0')
    expect(formatPercentFromBps(-2000, 'en', { signed: true })).toBe('-20.0')
    expect(formatPercentFromBps(0, 'en', { signed: true })).toBe('0.0')
    expect(formatPercentFromBps(2000, 'en')).toBe('20.0')
  })
})
```

Create `web/src/components/Arc.test.tsx`:

```tsx
/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { ArcTile } from './Arc'

afterEach(() => {
  cleanup()
})

describe('ArcTile', () => {
  test('is a meter carrying its value, label and hint', () => {
    render(<ArcTile label="Savings rate" hint="Net ÷ income" bps={6900} tone="in" locale="en" noValueLabel="No value yet" />)

    const meter = screen.getByRole('meter', { name: 'Savings rate' })
    expect(meter).toHaveAttribute('aria-valuenow', '69')
    expect(meter).toHaveAttribute('aria-valuetext', '69.0%')
    expect(meter).toHaveAccessibleDescription('Net ÷ income')
    expect(meter.querySelector('[data-arc-value]')).not.toBeNull()
    expect(meter.querySelector('svg')).toHaveAttribute('data-arc-tone', 'in')
  })

  test('a missing value is an empty arc with an em dash, never 0%', () => {
    render(<ArcTile label="Net vs last period" bps={null} tone="in" locale="en" noValueLabel="No value yet" />)

    const meter = screen.getByRole('meter', { name: 'Net vs last period' })
    expect(meter).not.toHaveAttribute('aria-valuenow')
    expect(meter).toHaveAttribute('aria-valuetext', 'No value yet')
    expect(meter).toHaveTextContent('—')
    expect(meter).not.toHaveTextContent('0')
    expect(meter.querySelector('[data-arc-value]')).toBeNull()
  })

  test('a signed change lights the arc by its size and keeps its sign in the text', () => {
    render(<ArcTile label="Net vs last period" bps={-2000} signed tone="out" locale="en" noValueLabel="No value yet" />)

    const meter = screen.getByRole('meter', { name: 'Net vs last period' })
    expect(meter).toHaveAttribute('aria-valuenow', '20')
    expect(meter).toHaveAttribute('aria-valuetext', '-20.0%')
    expect(meter.querySelector('svg')).toHaveAttribute('data-arc-tone', 'out')
  })

  test('values past full scale fill the arc and no further', () => {
    render(<ArcTile label="Spend ratio" bps={25000} tone="out" locale="en" noValueLabel="No value yet" />)

    expect(screen.getByRole('meter', { name: 'Spend ratio' })).toHaveAttribute('aria-valuenow', '100')
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run src/lib/arc.test.ts src/components/Arc.test.tsx`
Expected: FAIL with "Failed to resolve import './arc'" and "Failed to resolve import './Arc'".

- [ ] **Step 3: Implement**

Create `web/src/lib/arc.ts`:

```ts
/** The meter starts at the lower left (135 degrees, SVG angles run clockwise). */
export const ARC_START_DEG = 135
/** It sweeps three quarters of a turn, leaving the gap at the bottom. */
export const ARC_SWEEP_DEG = 270

/**
 * How much of the arc to light for a value in basis points: 0 to 1, clamped.
 * `null` when there is no value, so the caller draws an empty arc, never 0%.
 */
export function arcFraction(bps: number | null | undefined, fullScaleBps = 10_000): number | null {
  if (bps === null || bps === undefined || !Number.isFinite(bps)) return null
  return Math.min(1, Math.max(0, bps / fullScaleBps))
}

export function arcEndDeg(fraction: number): number {
  return ARC_START_DEG + ARC_SWEEP_DEG * fraction
}

export function arcPoint(cx: number, cy: number, radius: number, deg: number): { x: number; y: number } {
  const rad = (deg * Math.PI) / 180
  return { x: cx + radius * Math.cos(rad), y: cy + radius * Math.sin(rad) }
}

/** An SVG path along the circle from `fromDeg` to `toDeg`, clockwise. */
export function arcPath(cx: number, cy: number, radius: number, fromDeg: number, toDeg: number): string {
  const start = arcPoint(cx, cy, radius, fromDeg)
  const end = arcPoint(cx, cy, radius, toDeg)
  const largeArc = toDeg - fromDeg > 180 ? 1 : 0
  return `M${start.x.toFixed(2)} ${start.y.toFixed(2)}A${radius} ${radius} 0 ${largeArc} 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)}`
}

/** A basis-point value as a percent with one decimal, in the reader's locale. */
export function formatPercentFromBps(
  bps: number,
  locale: string,
  options: { signed?: boolean } = {},
): string {
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
    signDisplay: options.signed ? 'exceptZero' : 'auto',
  }).format(bps / 100)
}
```

Create `web/src/components/Arc.tsx`:

```tsx
import { useId } from 'react'

import { ARC_START_DEG, ARC_SWEEP_DEG, arcEndDeg, arcFraction, arcPath, arcPoint, formatPercentFromBps } from '../lib/arc'
import { LEDGER_IN_STOPS, LEDGER_OUT_STOPS } from '../lib/cashFlowLight'
import { cn } from '../lib/cn'

export type ArcTone = 'in' | 'out'

const CENTRE = 35
const RADIUS = 26

/**
 * The 270-degree meter: a quiet track, a Ledger gradient stroke over a blurred
 * bloom copy of itself, and a lit pointer dot at the value.
 */
export function Arc({ fraction, tone, className = '' }: { fraction: number | null; tone: ArcTone; className?: string }) {
  // useId returns characters that are not valid inside url(#...).
  const id = useId().replace(/[^a-zA-Z0-9_-]/g, '')
  const stops = tone === 'in' ? LEDGER_IN_STOPS : LEDGER_OUT_STOPS
  const lit = fraction !== null && fraction > 0
  const end = arcEndDeg(fraction ?? 0)
  const pointer = arcPoint(CENTRE, CENTRE, RADIUS, end)
  const valuePath = arcPath(CENTRE, CENTRE, RADIUS, ARC_START_DEG, end)

  return (
    <svg
      viewBox="0 0 70 70"
      aria-hidden="true"
      data-arc-tone={tone}
      className={cn('size-[76px] shrink-0 overflow-visible', className)}
    >
      <defs>
        <linearGradient id={`arc-gradient-${id}`} x1="0" y1="1" x2="1" y2="0">
          <stop offset="0" stopColor={stops[0]} />
          <stop offset="0.55" stopColor={stops[1]} />
          <stop offset="1" stopColor={stops[2]} />
        </linearGradient>
        <filter id={`arc-bloom-${id}`} x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation="4.5" />
        </filter>
      </defs>
      <path
        d={arcPath(CENTRE, CENTRE, RADIUS, ARC_START_DEG, ARC_START_DEG + ARC_SWEEP_DEG)}
        fill="none"
        stroke="rgba(255,255,255,0.1)"
        strokeWidth="3"
        strokeLinecap="round"
      />
      {lit ? (
        <g data-arc-value>
          <path d={valuePath} fill="none" stroke={`url(#arc-gradient-${id})`} strokeWidth="6" strokeLinecap="round" filter={`url(#arc-bloom-${id})`} opacity="0.85" />
          <path d={valuePath} fill="none" stroke={`url(#arc-gradient-${id})`} strokeWidth="3.2" strokeLinecap="round" />
          <circle cx={pointer.x} cy={pointer.y} r="5" fill={stops[1]} filter={`url(#arc-bloom-${id})`} />
          <circle cx={pointer.x} cy={pointer.y} r="2.6" fill="#f4f7fb" />
        </g>
      ) : null}
      <circle cx={CENTRE} cy={CENTRE} r="1.4" fill="#5c6472" />
    </svg>
  )
}

/** A glass tile: an arc, its percent, a label and a hint. */
export function ArcTile({
  label,
  hint,
  bps,
  tone,
  locale,
  noValueLabel,
  signed = false,
  fullScaleBps = 10_000,
}: {
  label: string
  hint?: string
  bps: number | null
  tone: ArcTone
  locale: string
  noValueLabel: string
  signed?: boolean
  fullScaleBps?: number
}) {
  const hintId = useId()
  const fraction = arcFraction(bps === null ? null : signed ? Math.abs(bps) : bps, fullScaleBps)
  const value = bps === null ? null : formatPercentFromBps(bps, locale, { signed })

  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
      aria-valuetext={value === null ? noValueLabel : `${value}%`}
      aria-describedby={hint ? hintId : undefined}
      className="glass-pane grid grid-cols-[76px_minmax(0,1fr)] items-center gap-3.5 rounded-[20px] px-4.5 py-4"
    >
      <Arc fraction={fraction} tone={tone} />
      <div className="min-w-0">
        <div aria-hidden="true" className="text-[26px] leading-none font-semibold tabular-nums text-[var(--color-fg)]">
          {value === null ? (
            '—'
          ) : (
            <>
              {value}
              <small className="ml-0.5 font-mono text-[11px] font-medium text-[var(--color-muted)]">%</small>
            </>
          )}
        </div>
        <div aria-hidden="true" className="mt-1.5 truncate text-sm font-medium text-[var(--color-fg)]">
          {label}
        </div>
        {hint ? (
          <div id={hintId} className="mt-0.5 truncate text-[12.5px] text-[var(--color-muted)]">
            {hint}
          </div>
        ) : null}
      </div>
    </div>
  )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/lib/arc.test.ts src/components/Arc.test.tsx`
Expected: PASS, 10 tests (6 + 4).

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/lib/arc.ts web/src/lib/arc.test.ts web/src/components/Arc.tsx web/src/components/Arc.test.tsx
git commit -m "feat(web): 270-degree arc meters for the dashboard tiles" -m "Pure value-to-angle mapping and percent formatting, tested; an SVG arc with a Ledger gradient, bloom and pointer dot; and a glass tile exposed as a meter. A missing value draws an empty arc with an em dash, never 0%."
```

---
### Task 7: Ledger surfaces: light tokens, the net figure, pills and selector tones

**Files:**
- Modify: `web/src/index.css`
- Modify: `web/tests/tokens.test.ts`
- Modify: `web/src/components/ui.tsx`
- Modify: `web/src/components/ui.test.tsx`

**Interfaces:**
- Consumes: the existing `--color-money-*` tokens.
- Produces:
  - CSS tokens `--color-money-in-a/b/c`, `--color-money-out-a/b/c`, `--color-on-money-in`, `--color-on-money-out`, `--color-plate-neutral`.
  - CSS classes `.net-figure-in` and `.net-figure-out`.
  - Components:

```tsx
export function MoneyPill(props: { tone: 'in' | 'out' | 'neutral'; label: string; value: string; size?: 'sm' | 'md' }): JSX.Element // data-money-pill={tone}
export function AmountPill(props: { tone: 'in' | 'out' | 'neutral'; children: ReactNode }): JSX.Element // data-amount={tone}
// Segmented options gain: tone?: 'money-in' | 'money-out'; the active button carries data-tone="money-in" | "money-out" | "neutral"
```

**Measured (2026-09-15)** against `BRIGHTEST_GLASS`, with plates composited over it:

| Pair | Contrast |
|---|---|
| Pill label `--color-fg-secondary` on the in plate | 4.58 |
| Pill label on the out plate | 5.08 |
| Pill label on the neutral plate | 4.54 |
| `--color-muted` on the in plate (why the label is not muted) | 4.04 |
| Money-in text on the in plate | 5.58 |
| Money-out text on the out plate | 5.12 |
| `--color-on-money-in` `#06110d` on `#27bf93` / `#1ba39a` / `#1e8db0` | 8.19 / 6.17 / 5.0 |
| `--color-on-money-out` `#1a0906` on `#f07a45` / `#e8603f` / `#d64a5a` | 6.98 / 5.69 / 4.59 |
| Net figure in stops `#6fd9c4` / `#27bf93` / `#1ba39a` | 6.77 / 4.89 / 3.69 |
| Net figure out stops `#ff9b7e` / `#f07a45` / `#e8603f` | 5.59 / 4.13 / 3.37 |

The net figure is never smaller than 34px, so its threshold is 3:1. The mockup's in gradient ended on `#1e8db0` (3.00:1, no margin), and its out gradient would end on `#d64a5a` (2.72:1, fails). Each figure gradient therefore starts on the text tint and stops one light stop earlier. This is decision D4.

- [ ] **Step 1: Write the failing tests**

In `web/tests/tokens.test.ts`, add inside `describe('Aurora glass tokens', ...)`:

```ts
  test('the light stops are the spec gradient, shared with the canvas', () => {
    // web/src/lib/cashFlowLight.ts paints with these same six values; its own
    // test pins them there.
    expect(['--color-money-in-a', '--color-money-in-b', '--color-money-in-c'].map((n) => token(n))).toEqual([
      '#27bf93',
      '#1ba39a',
      '#1e8db0',
    ])
    expect(['--color-money-out-a', '--color-money-out-b', '--color-money-out-c'].map((n) => token(n))).toEqual([
      '#f07a45',
      '#e8603f',
      '#d64a5a',
    ])
  })

  test('the label on a chosen entry type clears 4.5:1 across its whole gradient', () => {
    const onIn = rgb(token('--color-on-money-in'))
    const onOut = rgb(token('--color-on-money-out'))

    for (const stop of ['--color-money-in-a', '--color-money-in-b', '--color-money-in-c']) {
      expect(contrast(onIn, rgb(token(stop))), stop).toBeGreaterThanOrEqual(4.5)
    }
    for (const stop of ['--color-money-out-a', '--color-money-out-b', '--color-money-out-c']) {
      expect(contrast(onOut, rgb(token(stop))), stop).toBeGreaterThanOrEqual(4.5)
    }
  })

  test('every stop of the net figure clears 3:1, the large-text threshold', () => {
    for (const name of ['net-figure-in', 'net-figure-out']) {
      const rule = css.match(new RegExp(`\\.${name}\\s*\\{([^}]+)\\}`))
      expect(rule, `${name} rule exists`).not.toBeNull()

      const stops = (rule?.[1] ?? '').match(/#[0-9a-f]{6}/gi) ?? []
      expect(stops.length, `${name} has gradient stops`).toBeGreaterThanOrEqual(2)
      for (const stop of stops) {
        expect(contrast(rgb(stop.toLowerCase()), BRIGHTEST_GLASS), `${name} ${stop}`).toBeGreaterThanOrEqual(3)
      }
    }
  })

  test('money pills keep label and value legible on their plates', () => {
    const inPlate = compositeOver(rgba(token('--color-money-in-soft')), BRIGHTEST_GLASS)
    const outPlate = compositeOver(rgba(token('--color-money-out-soft')), BRIGHTEST_GLASS)
    const neutralPlate = compositeOver(rgba(token('--color-plate-neutral')), BRIGHTEST_GLASS)
    const label = rgb(token('--color-fg-secondary'))

    expect(contrast(label, inPlate), 'label on in').toBeGreaterThanOrEqual(4.5)
    expect(contrast(label, outPlate), 'label on out').toBeGreaterThanOrEqual(4.5)
    expect(contrast(label, neutralPlate), 'label on neutral').toBeGreaterThanOrEqual(4.5)
    expect(contrast(rgb(token('--color-money-in-text')), inPlate), 'in value').toBeGreaterThanOrEqual(4.5)
    expect(contrast(rgb(token('--color-money-out-text')), outPlate), 'out value').toBeGreaterThanOrEqual(4.5)
    expect(contrast(rgb(token('--color-fg')), neutralPlate), 'neutral value').toBeGreaterThanOrEqual(4.5)
  })
```

In `web/src/components/ui.test.tsx`:
- Change the imports to `import { useState } from 'react'`, `import userEvent from '@testing-library/user-event'` and `import { AmountPill, Button, ErrorBanner, Field, IconBadge, Input, MetricCard, MoneyPill, Segmented, Select } from './ui'`.
- Append:

```tsx
describe('MoneyPill and AmountPill', () => {
  test('a money pill carries its Ledger role as data and reads label then value', () => {
    render(<MoneyPill tone="in" label="In" value="74.500,00 €" />)

    const pill = document.querySelector('[data-money-pill="in"]')
    expect(pill).not.toBeNull()
    expect(pill).toHaveTextContent('In 74.500,00 €')
  })

  test('an amount pill marks its direction', () => {
    render(<AmountPill tone="out">-985,00 €</AmountPill>)

    expect(screen.getByText('-985,00 €')).toHaveAttribute('data-amount', 'out')
  })
})

describe('Segmented', () => {
  test('the chosen option wears its Ledger tone; untoned options stay neutral', async () => {
    function Harness() {
      const [value, setValue] = useState<'expense' | 'income' | 'transfer'>('expense')
      return (
        <Segmented
          value={value}
          onChange={setValue}
          options={[
            { id: 'expense', label: 'Expense', tone: 'money-out' },
            { id: 'income', label: 'Income', tone: 'money-in' },
            { id: 'transfer', label: 'Transfer' },
          ]}
        />
      )
    }
    render(<Harness />)

    expect(screen.getByRole('button', { name: 'Expense' })).toHaveAttribute('data-tone', 'money-out')
    expect(screen.getByRole('button', { name: 'Income' })).not.toHaveAttribute('data-tone')

    await userEvent.click(screen.getByRole('button', { name: 'Income' }))
    expect(screen.getByRole('button', { name: 'Income' })).toHaveAttribute('data-tone', 'money-in')

    await userEvent.click(screen.getByRole('button', { name: 'Transfer' }))
    expect(screen.getByRole('button', { name: 'Transfer' })).toHaveAttribute('data-tone', 'neutral')
    expect(screen.getByRole('button', { name: 'Expense' })).not.toHaveAttribute('data-tone')
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run tests/tokens.test.ts src/components/ui.test.tsx`
Expected: FAIL with "--color-money-in-a is declared" and "MoneyPill is not exported" (or its render error).

- [ ] **Step 3: Implement**

In `web/src/index.css`, in the `@theme` block, after `--color-money-out-soft: ...;`, add:

```css
  /* The light's gradient stops (spec: Rendering), shared with the canvas in
     src/lib/cashFlowLight.ts. Selected entry types wear them too. */
  --color-money-in-a: #27bf93;
  --color-money-in-b: #1ba39a;
  --color-money-in-c: #1e8db0;
  --color-money-out-a: #f07a45;
  --color-money-out-b: #e8603f;
  --color-money-out-c: #d64a5a;
  /* Dark labels on those gradients: at least 4.5:1 at every stop. */
  --color-on-money-in: #06110d;
  --color-on-money-out: #1a0906;
  /* The quiet plate under a neutral pill (balances, transfers). */
  --color-plate-neutral: rgba(255, 255, 255, 0.07);
```

In the first `@layer components { ... }` block, after the `.cash-flow-glow` rule, add:

```css
  /* The net figure: money drawn as light. It starts on the Ledger text tint
     and ends on a light stop; every stop holds 3:1 on the brightest glass,
     the threshold for the figure's large text. */
  .net-figure-in {
    background-image: linear-gradient(90deg, #6fd9c4, #27bf93 55%, #1ba39a);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    filter: drop-shadow(0 0 26px rgba(55, 213, 255, 0.28));
  }

  .net-figure-out {
    background-image: linear-gradient(90deg, #ff9b7e, #f07a45 55%, #e8603f);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    filter: drop-shadow(0 0 26px rgba(232, 96, 63, 0.24));
  }
```

In `web/src/components/ui.tsx`, add after `IconBadge`:

```tsx
/**
 * A labelled money figure on a tinted plate: IN and OUT in the Ledger tints,
 * neutral for balances. The label tier is measured to hold 4.5:1 on every
 * plate (tokens.test.ts).
 */
export function MoneyPill({
  tone,
  label,
  value,
  size = 'md',
}: {
  tone: 'in' | 'out' | 'neutral'
  label: string
  value: string
  size?: 'sm' | 'md'
}) {
  const tones = {
    in: 'bg-[var(--color-money-in-soft)] text-[var(--color-money-in-text)] shadow-[inset_0_0_0_1px_rgba(27,163,154,0.4)]',
    out: 'bg-[var(--color-money-out-soft)] text-[var(--color-money-out-text)] shadow-[inset_0_0_0_1px_rgba(232,96,63,0.4)]',
    neutral:
      'bg-[var(--color-plate-neutral)] text-[var(--color-fg)] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.12)]',
  }

  return (
    <span
      data-money-pill={tone}
      className={cn(
        'inline-flex items-center gap-2.5 rounded-full font-semibold whitespace-nowrap tabular-nums',
        size === 'sm' ? 'px-2.5 py-1.5 text-xs' : 'px-3.5 py-2 text-sm',
        tones[tone],
      )}
    >
      <span className="font-mono text-[10px] font-medium tracking-[0.14em] text-[var(--color-fg-secondary)] uppercase">
        {label}
      </span>{' '}
      {value}
    </span>
  )
}

/** A signed amount on a tinted plate, for list rows. */
export function AmountPill({
  tone,
  children,
}: {
  tone: 'in' | 'out' | 'neutral'
  children: ReactNode
}) {
  const tones = {
    in: 'bg-[var(--color-money-in-soft)] text-[var(--color-money-in-text)]',
    out: 'bg-[var(--color-money-out-soft)] text-[var(--color-money-out-text)]',
    neutral: 'bg-[var(--color-plate-neutral)] text-[var(--color-fg)]',
  }

  return (
    <span
      data-amount={tone}
      className={cn(
        'inline-flex shrink-0 items-center rounded-full px-3 py-1.5 text-sm font-semibold whitespace-nowrap tabular-nums',
        tones[tone],
      )}
    >
      {children}
    </span>
  )
}
```

Replace `Segmented` with:

```tsx
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  className = '',
}: {
  value: T
  onChange: (v: T) => void
  /**
   * `tone` colours an option while it is chosen: an entry type that is money
   * in or out wears that Ledger gradient; untoned options take the neutral plate.
   */
  options: Array<{ id: T; label: string; icon?: ReactNode; tone?: 'money-in' | 'money-out' }>
  className?: string
}) {
  const toned = {
    'money-in':
      'bg-[linear-gradient(90deg,var(--color-money-in-a),var(--color-money-in-b)_55%,var(--color-money-in-c))] font-semibold text-[var(--color-on-money-in)] shadow-[0_6px_22px_rgba(27,163,154,0.35)]',
    'money-out':
      'bg-[linear-gradient(90deg,var(--color-money-out-a),var(--color-money-out-b)_55%,var(--color-money-out-c))] font-semibold text-[var(--color-on-money-out)] shadow-[0_6px_22px_rgba(232,96,63,0.35)]',
  }

  return (
    <div
      className={cn(
        'inline-flex h-10 items-stretch gap-0.5 rounded-full bg-white/[0.04] p-1 shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)]',
        className,
      )}
    >
      {options.map((opt) => {
        const active = value === opt.id

        return (
          <button
            key={opt.id}
            type="button"
            aria-pressed={active}
            data-tone={active ? (opt.tone ?? 'neutral') : undefined}
            onClick={() => onChange(opt.id)}
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full px-3.5 text-[13px] font-medium transition',
              active
                ? opt.tone
                  ? toned[opt.tone]
                  : 'bg-white/10 text-[var(--color-fg)] shadow-[inset_0_1px_0_rgba(255,255,255,0.12)]'
                : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
            )}
          >
            {opt.icon}
            {opt.label}
          </button>
        )
      })}
    </div>
  )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run tests/tokens.test.ts src/components/ui.test.tsx`
Expected: PASS.

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/index.css web/tests/tokens.test.ts web/src/components/ui.tsx web/src/components/ui.test.tsx
git commit -m "feat(web): Ledger surfaces for the light, pills and entry types" -m "The light's six gradient stops become tokens shared with the canvas; dark labels on them hold 4.5:1 at every stop. The net figure glows in a Ledger gradient whose stops each hold 3:1 for large text. MoneyPill and AmountPill put money on measured plates, and Segmented options can wear their Ledger tone while chosen."
```

---
### Task 8: The shell: a page-owned top bar, round Lock, Quick add, and phase 1 carry-overs

**Files:**
- Create: `web/src/lib/topBar.ts`
- Create: `web/src/components/TopBar.tsx`
- Create: `web/src/components/TopBar.test.tsx`
- Modify: `web/src/App.tsx`
- Modify: `web/src/App.test.tsx`
- Modify: `web/src/pages/TransactionsPage.tsx` (props and one effect only)
- Modify: `web/src/pages/TransactionsPage.test.tsx` (one test)
- Modify: `web/src/components/ui.tsx` (`CollapsibleSection` tone type)
- Modify: `web/src/pages/SettingsPage.tsx:826`
- Modify: `web/src/locales/en.json`, `el.json`, `fr.json`, `de.json`

**Interfaces:**
- Consumes: nothing new.
- Produces:

```ts
// web/src/lib/topBar.ts
export type TopBarSlots = { title: HTMLElement | null; actions: HTMLElement | null; claimTitle: () => () => void }
export const TopBarContext: React.Context<TopBarSlots | null>
```

```tsx
// web/src/components/TopBar.tsx
export function TopBar(props: { title: string; subtitle?: string; actions?: ReactNode }): JSX.Element
// TransactionsPage props gain:
newEntryIntent?: number
onNewEntryIntentHandled?: () => void
```

How the top bar works:
- In the mockup, the top bar belongs to the page. Dashboard shows the book, its period and Month/Quarter/Year. Transactions shows its title and New entry. Lock sits at the far right on every page.
- App renders two empty slots in its header.
- A page renders `<TopBar>`, which portals its title and actions into those slots and claims the title while it is mounted. App shows the book's name and chart in the title slot only while nobody claims it, so Accounts, Reports, Documents and Settings keep today's header.
- Outside the shell (page tests), `TopBar` renders inline.
- The context lives in `lib/topBar.ts` so the component file exports only components, which Fast Refresh requires.

Carry-overs from phase 1:
- **R19:** the App test "held still while the vault is locked" could not fail, because jsdom has no focus. It gets a focus spy.
- **CollapsibleSection:** it drops the `success` tone. Success means a confirmation, and a section icon is chrome, so Settings' Books section uses `accent`.
- **Asset badges** keep `accent`. The spec defines success as the brand emerald, the same hex as accent, and account-type badges are identity, not confirmations (decision D15).

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/TopBar.test.tsx`:

```tsx
/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import { useCallback, useMemo, useState, type ReactNode } from 'react'
import { afterEach, describe, expect, test } from 'vitest'

import { TopBarContext } from '../lib/topBar'
import { TopBar } from './TopBar'

afterEach(() => {
  cleanup()
})

/** The same wiring App uses: two slots and a claim count. */
function Shell({ children }: { children?: ReactNode }) {
  const [title, setTitle] = useState<HTMLElement | null>(null)
  const [actions, setActions] = useState<HTMLElement | null>(null)
  const [claims, setClaims] = useState(0)
  const claimTitle = useCallback(() => {
    setClaims((n) => n + 1)
    return () => setClaims((n) => n - 1)
  }, [])
  const slots = useMemo(() => ({ title, actions, claimTitle }), [title, actions, claimTitle])

  return (
    <TopBarContext.Provider value={slots}>
      <header>
        <div data-testid="title-slot" ref={setTitle} />
        {claims === 0 ? <span>Book title</span> : null}
        <div data-testid="actions-slot" ref={setActions} />
      </header>
      <main>{children}</main>
    </TopBarContext.Provider>
  )
}

describe('TopBar', () => {
  test('outside the shell it renders inline, title first', () => {
    render(<TopBar title="Transactions" subtitle="Personal · EUR" actions={<button type="button">New Entry</button>} />)

    expect(screen.getByRole('heading', { level: 1, name: 'Transactions' })).toBeInTheDocument()
    expect(screen.getByText('Personal · EUR')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New Entry' })).toBeInTheDocument()
  })

  test('inside the shell it fills the header slots and hides the book title', async () => {
    render(
      <Shell>
        <TopBar title="Transactions" actions={<button type="button">New Entry</button>} />
      </Shell>,
    )

    await waitFor(() => {
      expect(within(screen.getByTestId('title-slot')).getByRole('heading', { name: 'Transactions' })).toBeInTheDocument()
    })
    expect(within(screen.getByTestId('actions-slot')).getByRole('button', { name: 'New Entry' })).toBeInTheDocument()
    expect(screen.queryByText('Book title')).toBeNull()
    expect(within(screen.getByRole('main')).queryByRole('heading')).toBeNull()
  })

  test('gives the title back when the page goes away', async () => {
    const { rerender } = render(
      <Shell>
        <TopBar title="Transactions" />
      </Shell>,
    )
    await waitFor(() => expect(screen.queryByText('Book title')).toBeNull())

    rerender(<Shell />)

    expect(await screen.findByText('Book title')).toBeInTheDocument()
  })
})
```

In `web/src/App.test.tsx`:

1. Replace the `./pages/DashboardPage` mock with:

```tsx
vi.mock('./pages/DashboardPage', async () => {
  const { TopBar } = await import('./components/TopBar')
  return {
    DashboardPage: ({ onCreateBook }: { onCreateBook?: () => void }) => (
      <div>
        <TopBar title="Dashboard stub title" actions={<button type="button">Stub period</button>} />
        Dashboard stub
        {onCreateBook ? (
          <button type="button" onClick={onCreateBook}>
            Create a book
          </button>
        ) : null}
      </div>
    ),
  }
})
```

2. Replace the `./pages/TransactionsPage` mock with:

```tsx
vi.mock('./pages/TransactionsPage', () => ({
  TransactionsPage: ({ newEntryIntent }: { newEntryIntent?: number }) => (
    <div>{`Transactions stub, new entry intent ${newEntryIntent ?? 0}`}</div>
  ),
}))
```

3. Add `within` to the `@testing-library/react` import, and add `vi.restoreAllMocks()` as the last line of `afterEach`.

4. Replace the two aurora tests with:

```tsx
  test('mounts exactly one aurora, above the shell, moving in a focused unlocked window', async () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Settings' })).toBeTruthy()
    })

    const auroras = document.querySelectorAll('.aurora')
    expect(auroras).toHaveLength(1)
    expect(auroras[0]).toHaveAttribute('data-moving', 'true')
  })

  test('mounts exactly one aurora, held still, while the vault is locked', async () => {
    // jsdom reports no focus, which would hold the aurora still on its own;
    // give the window focus so only the locked state can be what pauses it.
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    vi.mocked(vaultStatus).mockReset().mockResolvedValue('locked')
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Welcome back' })).toBeTruthy()
    })

    const auroras = document.querySelectorAll('.aurora')
    expect(auroras).toHaveLength(1)
    expect(auroras[0]).toHaveAttribute('data-moving', 'false')
  })
```

5. Append a new `describe` block:

```tsx
describe('App top bar and Quick add', () => {
  test('a page that owns the top bar replaces the book title with its own', async () => {
    render(<App />)
    const banner = await screen.findByRole('banner')

    expect(await within(banner).findByRole('heading', { name: 'Dashboard stub title' })).toBeTruthy()
    expect(within(banner).getByRole('button', { name: 'Stub period' })).toBeTruthy()
    expect(within(banner).queryByText('Personal')).toBeNull()
  })

  test('a page that does not own it keeps the book title', async () => {
    render(<App />)
    await userEvent.click(await screen.findByRole('button', { name: 'Accounts' }))

    const banner = screen.getByRole('banner')
    expect(await within(banner).findByText('Personal')).toBeTruthy()
    expect(within(banner).queryByRole('heading', { name: 'Dashboard stub title' })).toBeNull()
  })

  test('Quick add opens New entry on Transactions', async () => {
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Quick add' })).toBeEnabled()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Quick add' }))

    expect(await screen.findByText('Transactions stub, new entry intent 1')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Transactions' })).toHaveAttribute('aria-current', 'page')
  })

  test('Quick add waits for a book to add to', async () => {
    vi.mocked(api.entityList).mockReset().mockResolvedValue([])
    render(<App />)

    expect(await screen.findByRole('button', { name: 'Quick add' })).toBeDisabled()
  })
})
```

In `web/src/pages/TransactionsPage.test.tsx`, append:

```tsx
describe('TransactionsPage Quick add intent', () => {
  test('opens New entry once and reports the intent handled', async () => {
    const handled = vi.fn()
    render(<TransactionsPage entity={entity} newEntryIntent={1} onNewEntryIntentHandled={handled} />)

    expect(await screen.findByRole('heading', { name: 'New entry' })).toBeTruthy()
    expect(handled).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run src/components/TopBar.test.tsx src/App.test.tsx src/pages/TransactionsPage.test.tsx`
Expected: FAIL. TopBar fails with "Failed to resolve import '../lib/topBar'". App fails on the missing "Quick add" button and the stub title never appearing in the banner. Transactions fails because no "New entry" heading appears.

- [ ] **Step 3: Implement**

Create `web/src/lib/topBar.ts`:

```ts
import { createContext } from 'react'

/**
 * The app header's slots. A page that renders <TopBar> puts its title and
 * actions here; App shows the book's name only while no page claims the title.
 */
export type TopBarSlots = {
  /** Where the page title goes; null until the header has mounted. */
  title: HTMLElement | null
  /** Where the page's actions go, left of Lock; null until mounted. */
  actions: HTMLElement | null
  /** Call while a page owns the title; the returned function gives it back. */
  claimTitle: () => () => void
}

export const TopBarContext = createContext<TopBarSlots | null>(null)
```

Create `web/src/components/TopBar.tsx`:

```tsx
import { useContext, useEffect, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

import { TopBarContext } from '../lib/topBar'

/**
 * A page's title, subtitle and actions, drawn in the app header. Outside the
 * app shell (page tests, isolated renders) it renders inline instead.
 */
export function TopBar({
  title,
  subtitle,
  actions,
}: {
  title: string
  subtitle?: string
  actions?: ReactNode
}) {
  const slots = useContext(TopBarContext)
  const claimTitle = slots?.claimTitle

  useEffect(() => claimTitle?.(), [claimTitle])

  const heading = (
    <>
      <h1 className="truncate text-xl font-semibold tracking-tight text-[var(--color-fg)]">{title}</h1>
      {subtitle ? (
        <span className="truncate text-sm text-[var(--color-fg-secondary)]">{subtitle}</span>
      ) : null}
    </>
  )

  if (!slots) {
    return (
      <div className="mb-1 flex flex-wrap items-center justify-between gap-4">
        <div className="flex min-w-0 items-baseline gap-2.5">{heading}</div>
        {actions ? <div className="flex flex-wrap items-center gap-2.5">{actions}</div> : null}
      </div>
    )
  }

  return (
    <>
      {slots.title ? createPortal(heading, slots.title) : null}
      {actions && slots.actions ? createPortal(actions, slots.actions) : null}
    </>
  )
}
```

In `web/src/App.tsx`:

1. Imports:
   - `import { useCallback, useEffect, useMemo, useState } from 'react'`.
   - Add `Loader2` and `Plus` to the `lucide-react` import.
   - Add `import { TopBarContext, type TopBarSlots } from './lib/topBar'`.
2. After `const [license, setLicense] = ...`, add:

```tsx
  const [newEntryIntent, setNewEntryIntent] = useState(0)
  const [titleSlot, setTitleSlot] = useState<HTMLDivElement | null>(null)
  const [actionsSlot, setActionsSlot] = useState<HTMLDivElement | null>(null)
  const [titleClaims, setTitleClaims] = useState(0)
```

3. After `onCreateBookIntentHandled`, add:

```tsx
  // The sidebar's Quick add opens New entry on Transactions. As with the
  // create-book intent, the page resets it once handled, so a remount (main is
  // keyed by the active page) never replays it.
  const openNewEntry = useCallback(() => {
    setActive('transactions')
    setNewEntryIntent((n) => n + 1)
  }, [])

  const onNewEntryIntentHandled = useCallback(() => {
    setNewEntryIntent(0)
  }, [])

  const claimTitle = useCallback(() => {
    setTitleClaims((n) => n + 1)
    return () => setTitleClaims((n) => n - 1)
  }, [])

  const topBar = useMemo<TopBarSlots>(
    () => ({ title: titleSlot, actions: actionsSlot, claimTitle }),
    [titleSlot, actionsSlot, claimTitle],
  )
```

4. Wrap the unlocked shell `<div className="flex h-full min-h-0 text-[var(--color-fg)]"> ... </div>` in `<TopBarContext.Provider value={topBar}> ... </TopBarContext.Provider>`.

5. Between the books `</section>` and the version footer `<div className="px-1.5 font-mono ...">`, add:

```tsx
          <Button className="w-full" onClick={openNewEntry} disabled={!entity}>
            <Plus className="size-4" />
            {t('app.sidebar.quickAdd')}
          </Button>
```

6. Replace the whole `<header ...> ... </header>` with:

```tsx
          <header className="flex h-16 shrink-0 items-center justify-between gap-4 px-7">
            <div className="flex min-w-0 items-baseline gap-2.5">
              {/* A page rendering <TopBar> fills this slot and claims the title. */}
              <div ref={setTitleSlot} className="flex min-w-0 items-baseline gap-2.5" />
              {titleClaims === 0 ? (
                <>
                  <span className="truncate text-xl font-semibold tracking-tight">
                    {entity ? entity.name : t('app.noBookSelected')}
                  </span>
                  <span className="truncate text-sm text-[var(--color-fg-secondary)]">
                    {entity
                      ? t('app.entityChart', {
                          currency: entity.base_currency,
                          chart: t(`chart.${entity.chart_template}`),
                        })
                      : t('app.createEntityInSettings')}
                  </span>
                </>
              ) : null}
            </div>
            <div className="flex shrink-0 items-center gap-2.5">
              <div ref={setActionsSlot} className="flex items-center gap-2.5" />
              <button
                type="button"
                onClick={() => void onLock()}
                disabled={locking}
                aria-label={t('app.lockVault')}
                title={locking ? t('app.locking') : t('app.lock')}
                className="glass-pane inline-flex size-10 items-center justify-center rounded-full text-[var(--color-fg)] transition hover:bg-white/[0.08] disabled:cursor-not-allowed disabled:opacity-50"
              >
                {locking ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Lock className="size-4" strokeWidth={1.75} />
                )}
              </button>
            </div>
          </header>
```

7. Pass the intent to Transactions:

```tsx
                <TransactionsPage
                  key={entity?.id ?? 'none'}
                  entity={entity}
                  onCreateBook={openCreateBook}
                  newEntryIntent={newEntryIntent}
                  onNewEntryIntentHandled={onNewEntryIntentHandled}
                />
```

`Button` is still used by the sidebar's Quick add; keep its import.

In `web/src/pages/TransactionsPage.tsx`, replace `type Props = { entity: Entity | null; onCreateBook?: () => void }` with:

```tsx
type Props = {
  entity: Entity | null
  onCreateBook?: () => void
  /** Bumped by the sidebar's Quick add: open New entry once, then report it handled. */
  newEntryIntent?: number
  onNewEntryIntentHandled?: () => void
}
```

Change the signature to `export function TransactionsPage({ entity, onCreateBook, newEntryIntent, onNewEntryIntentHandled }: Props) {`. Add this effect directly after the entity-loading `useEffect` (the one ending `}, [entity?.id, debouncedSearch, fromDate, toDate, accountFilter])`):

```tsx
  // The sidebar's Quick add lands here. Reporting it handled lets App reset the
  // intent, so a later remount does not reopen the dialog.
  useEffect(() => {
    if (!entity || !newEntryIntent) return
    openNewEntry()
    onNewEntryIntentHandled?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [newEntryIntent, entity?.id])
```

In `web/src/components/ui.tsx`, in `CollapsibleSection`'s props, change `tone?: 'accent' | 'success' | 'danger' | 'warning' | 'info' | 'muted'` to:

```tsx
  /** Chrome tones only: success means a confirmation, never a section. */
  tone?: 'accent' | 'danger' | 'warning' | 'info' | 'muted'
```

In `web/src/pages/SettingsPage.tsx:826`, change `tone="success"` to `tone="accent"`.

Locales (edit by hand; do not rewrite the files through `JSON.stringify`, which reorders the numeric month keys):
- `en.json`: after `"app.lock": "Lock",` add `"app.sidebar.quickAdd": "Quick add",`.
- `el.json`, `fr.json`, `de.json`: inside `"app" > "sidebar"`, after `"versionEncrypted": ...`, add a comma and:
  - el: `"quickAdd": "Γρήγορη καταχώριση"`
  - fr: `"quickAdd": "Ajout rapide"`
  - de: `"quickAdd": "Schnell erfassen"`

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/components/TopBar.test.tsx src/App.test.tsx src/pages/TransactionsPage.test.tsx src/lib/i18n.test.ts`
Expected: PASS.

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green; `tsc` proves no caller still passes `tone="success"` to `CollapsibleSection`.

- [ ] **Step 6: Commit**

```bash
git add web/src/lib/topBar.ts web/src/components/TopBar.tsx web/src/components/TopBar.test.tsx web/src/App.tsx web/src/App.test.tsx web/src/pages/TransactionsPage.tsx web/src/pages/TransactionsPage.test.tsx web/src/components/ui.tsx web/src/pages/SettingsPage.tsx web/src/locales/en.json web/src/locales/el.json web/src/locales/fr.json web/src/locales/de.json
git commit -m "feat(web): pages own the top bar; Quick add and a round Lock in the shell" -m "As in the mockup, a page puts its title and actions in the app header through <TopBar>, and App shows the book title only while no page claims it. Lock becomes a round glass button. The sidebar gains Quick add, which opens New entry on Transactions through a one-shot intent the page resets. The locked-aurora test now gives the window focus so it can fail, and CollapsibleSection no longer accepts the success tone, which is reserved for confirmations."
```

---
### Task 9: The dashboard, composed as in the mockup

**Files:**
- Modify: `web/src/lib/api.ts` (quarter bounds)
- Create: `web/src/lib/api.period.test.ts`
- Replace: `web/src/pages/DashboardPage.tsx`
- Replace: `web/src/pages/DashboardPage.test.tsx`
- Modify: `web/src/components/ui.tsx` (delete `FlowBar`, which has no callers left)
- Modify: `web/src/locales/en.json`, `el.json`, `fr.json`, `de.json`

**Interfaces:**
- Consumes:
  - `api.dashboardSummary` and `api.cashFlowSeries` (Task 3).
  - `CashFlowLight` (Task 5).
  - `ArcTile` and `formatPercentFromBps` (Task 6).
  - `MoneyPill`, `AmountPill`, `.net-figure-in/out` and `Segmented` (Task 7).
  - `TopBar` (Task 8).
- Produces: `quarterStartISO(now?: Date): string` and `quarterEndISO(now?: Date): string` in `web/src/lib/api.ts`.

**Composition** (mockup frame 01; decisions D7 and D8):
- **Top bar:** the book name, "August 2026 · EUR", and Month / Quarter / Year.
- **Hero pane:**
  - The caption "Net this month" and the net figure, drawn as light.
  - "96.5% of everything that came in, kept." (only when the savings rate is above zero).
  - IN, OUT and ASSETS pills on the right.
  - The cash-flow light across the lower half.
- **Four arc tiles:** Savings rate, Spend ratio, Net vs last period, Top spend.
- **Recent activity:** the list, with amounts on pills.

**Removed from the dashboard** (named in the PR and the hand-off):
- The explanatory paragraph under the net figure.
- The "Offline invoice reader" caption.
- The income and expense bars (the light replaces them).
- The "Encrypted vault · local only" line; the sidebar already says encrypted.
- The "Overview" eyebrow and the second copy of the book name.
- The four metric cards. Income, expenses and assets move into the pills, and net is the hero figure.

The "N entries this month" count moves into Recent activity's description.

- [ ] **Step 1: Write the failing tests**

Create `web/src/lib/api.period.test.ts`:

```ts
import { describe, expect, test } from 'vitest'

import { quarterEndISO, quarterStartISO } from './api'

describe('calendar quarter bounds', () => {
  test('mid-quarter', () => {
    const now = new Date(2026, 7, 15)
    expect([quarterStartISO(now), quarterEndISO(now)]).toEqual(['2026-07-01', '2026-09-30'])
  })

  test('the first and last days of the year', () => {
    expect([quarterStartISO(new Date(2026, 0, 1)), quarterEndISO(new Date(2026, 0, 1))]).toEqual([
      '2026-01-01',
      '2026-03-31',
    ])
    expect([quarterStartISO(new Date(2026, 11, 31)), quarterEndISO(new Date(2026, 11, 31))]).toEqual([
      '2026-10-01',
      '2026-12-31',
    ])
  })
})
```

Replace `web/src/pages/DashboardPage.test.tsx` with:

```tsx
/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      dashboardSummary: vi.fn(),
      cashFlowSeries: vi.fn(),
      entryList: vi.fn(),
      accountList: vi.fn(),
    },
  }
})

// jsdom has no 2D canvas; the light's painting is tested on its own.
vi.mock('../components/CashFlowLight', () => ({
  CashFlowLight: ({ label }: { label: string }) => <div role="img" aria-label={label} />,
}))

import type { Account, CashFlowSeries, DashboardSummary, Entity, PostedEntryView } from '../lib/api'
import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { formatMoney, localeForCurrency } from '../lib/money'
import { DashboardPage } from './DashboardPage'

const money = (minor: number, signed = false) =>
  formatMoney(minor, 'EUR', localeForCurrency('EUR'), { signed })
/** Rendered money carries a no-break space; text matchers see a plain one. */
const plain = (text: string) => text.replace(/\s/g, ' ')

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

function account(over: Partial<Account> & Pick<Account, 'id' | 'name' | 'account_type'>): Account {
  return {
    entity_id: 'e1',
    code: '1000',
    parent_id: null,
    is_active: true,
    is_system: false,
    sort_order: 0,
    ...over,
  }
}

const accounts: Account[] = [
  account({ id: 'w1', name: 'Checking', account_type: 'asset' }),
  account({ id: 'exp1', name: 'Groceries', account_type: 'expense' }),
  account({ id: 'inc1', name: 'Salary', account_type: 'income' }),
]

const summary: DashboardSummary = {
  entity_id: 'e1',
  base_currency: 'EUR',
  cash_like_assets: 98765,
  income: 120000,
  expenses: 4250,
  net_income: 115750,
  recent_entry_count: 2,
  savings_rate_bps: 9646,
  spend_ratio_bps: 354,
  top_expense: { code: '5100', name: 'Groceries', amount_minor: 4250, share_bps: 10000 },
  net_vs_previous_bps: null,
}

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-31',
  granularity: 'day',
  total_income_minor: 120000,
  total_expenses_minor: 4250,
  net_minor: 115750,
  buckets: [],
}

function line(id: string, entryId: string, accountId: string, debit: number, credit: number) {
  return {
    id,
    entry_id: entryId,
    account_id: accountId,
    debit: { amount_minor: debit },
    credit: { amount_minor: credit },
    memo: null,
  }
}

const entries: PostedEntryView[] = [
  {
    entry: { id: 'j1', entity_id: 'e1', entry_date: '2026-08-12', description: 'Alpha supermarket', reference: null, status: 'posted', hidden: false },
    lines: [line('l1', 'j1', 'exp1', 4250, 0), line('l2', 'j1', 'w1', 0, 4250)],
    is_voided: false,
  },
  {
    entry: { id: 'j2', entity_id: 'e1', entry_date: '2026-08-11', description: 'Client invoice', reference: null, status: 'posted', hidden: false },
    lines: [line('l3', 'j2', 'w1', 120000, 0), line('l4', 'j2', 'inc1', 0, 120000)],
    is_voided: false,
  },
]

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date(2026, 7, 15, 12))
  vi.mocked(api.dashboardSummary).mockReset().mockResolvedValue(summary)
  vi.mocked(api.cashFlowSeries).mockReset().mockResolvedValue(series)
  vi.mocked(api.entryList).mockReset().mockResolvedValue(entries)
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.useRealTimers()
})

describe('DashboardPage empty-state CTA', () => {
  test('no-book empty state renders Create a book and calls onCreateBook', async () => {
    const onCreateBook = vi.fn()
    render(<DashboardPage entity={null} onCreateBook={onCreateBook} />)
    await userEvent.click(screen.getByRole('button', { name: 'Create a book' }))
    expect(onCreateBook).toHaveBeenCalledTimes(1)
  })

  test('renders without a CTA when onCreateBook is not supplied', () => {
    render(<DashboardPage entity={null} />)
    expect(screen.queryByRole('button', { name: 'Create a book' })).toBeNull()
  })
})

describe('DashboardPage activity row colours', () => {
  test('income and expense activity rows wear the Ledger money tones, never status colours', async () => {
    render(<DashboardPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText('Client invoice')).toBeTruthy()
    })

    const expenseBadge = screen.getByText('Alpha supermarket').closest('li')?.firstElementChild
    expect(expenseBadge?.className).toContain('bg-[var(--color-money-out-soft)]')
    const incomeBadge = screen.getByText('Client invoice').closest('li')?.firstElementChild
    expect(incomeBadge?.className).toContain('bg-[var(--color-money-in-soft)]')

    expect(screen.getByText('Alpha supermarket').closest('li')?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'out')
    expect(screen.getByText('Client invoice').closest('li')?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'in')
  })
})

describe('DashboardPage hero and arcs', () => {
  test('the top bar names the book and the period', async () => {
    render(<DashboardPage entity={entity} />)

    expect(await screen.findByRole('heading', { level: 1, name: 'Personal' })).toBeInTheDocument()
    expect(screen.getByText('August 2026 · EUR')).toBeInTheDocument()
  })

  test('the net is drawn as light, with in, out and assets beside it', async () => {
    render(<DashboardPage entity={entity} />)

    expect(
      await screen.findByRole('img', {
        name: `Money in ${money(120000)}, money out ${money(4250)}, net ${money(115750, true)}, August 2026.`,
      }),
    ).toBeInTheDocument()
    const figure = document.querySelector('[data-net]')
    expect(figure).toHaveAttribute('data-net', 'in')
    expect(figure).toHaveTextContent(plain(money(115750, true)))
    expect(document.querySelector('[data-money-pill="in"]')).toHaveTextContent(plain(`In ${money(120000)}`))
    expect(document.querySelector('[data-money-pill="out"]')).toHaveTextContent(plain(`Out ${money(4250)}`))
    expect(document.querySelector('[data-money-pill="neutral"]')).toHaveTextContent(plain(`Assets ${money(98765)}`))
    expect(screen.getByText('96.5% of everything that came in, kept.')).toBeInTheDocument()
  })

  test('arc tiles read their basis points, and a missing value says so', async () => {
    render(<DashboardPage entity={entity} />)

    await waitFor(() => {
      expect(screen.getByRole('meter', { name: 'Savings rate' })).toHaveAttribute('aria-valuetext', '96.5%')
    })
    expect(screen.getByRole('meter', { name: 'Spend ratio' })).toHaveAttribute('aria-valuetext', '3.5%')
    expect(screen.getByRole('meter', { name: 'Net vs last period' })).toHaveAttribute('aria-valuetext', 'No value yet')
    const top = screen.getByRole('meter', { name: 'Top spend' })
    expect(top).toHaveAttribute('aria-valuetext', '100.0%')
    expect(top).toHaveAccessibleDescription('Groceries')
    expect(screen.getByText('2 entries this month')).toBeInTheDocument()
  })

  test('Quarter asks Rust for the calendar quarter', async () => {
    render(<DashboardPage entity={entity} />)
    await waitFor(() => {
      expect(api.dashboardSummary).toHaveBeenCalledWith('e1', '2026-08-01', '2026-08-31', '2026-08-15')
    })

    await userEvent.click(screen.getByRole('button', { name: 'Quarter' }))

    await waitFor(() => {
      expect(api.dashboardSummary).toHaveBeenLastCalledWith('e1', '2026-07-01', '2026-09-30', '2026-08-15')
    })
    expect(api.cashFlowSeries).toHaveBeenLastCalledWith('e1', '2026-07-01', '2026-09-30')
    expect(screen.getByText('Q3 2026 · EUR')).toBeInTheDocument()
    expect(screen.getByRole('meter', { name: 'Net vs last period' })).toHaveAccessibleDescription('Against last quarter')
  })

  test('a loss wears the out light and claims nothing was kept', async () => {
    vi.mocked(api.dashboardSummary).mockResolvedValue({
      ...summary,
      income: 1000,
      expenses: 1500,
      net_income: -500,
      savings_rate_bps: -5000,
      spend_ratio_bps: 15000,
    })
    render(<DashboardPage entity={entity} />)

    await waitFor(() => {
      expect(document.querySelector('[data-net]')).toHaveAttribute('data-net', 'out')
    })
    expect(screen.queryByText(/of everything that came in/)).toBeNull()
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run src/lib/api.period.test.ts src/pages/DashboardPage.test.tsx`
Expected: FAIL. `quarterStartISO` is not exported, and the dashboard has no Quarter button, no meters and no `[data-net]`.

- [ ] **Step 3: Implement**

In `web/src/lib/api.ts`, after `monthEndISO`, add:

```ts
/** First day of the calendar quarter containing `now`. */
export function quarterStartISO(now: Date = new Date()): string {
  const firstMonth = Math.floor(now.getMonth() / 3) * 3
  return `${now.getFullYear()}-${String(firstMonth + 1).padStart(2, '0')}-01`
}

/** Last day of the calendar quarter containing `now`. */
export function quarterEndISO(now: Date = new Date()): string {
  const last = new Date(now.getFullYear(), Math.floor(now.getMonth() / 3) * 3 + 3, 0)
  return `${last.getFullYear()}-${String(last.getMonth() + 1).padStart(2, '0')}-${String(last.getDate()).padStart(2, '0')}`
}
```

Replace `web/src/pages/DashboardPage.tsx` with:

```tsx
import { useEffect, useMemo, useState } from 'react'
import { ArrowDownLeft, ArrowUpRight, Landmark, Receipt } from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  localeForCurrency,
  monthEndISO,
  monthStartISO,
  quarterEndISO,
  quarterStartISO,
  todayISO,
  yearEndISO,
  yearStartISO,
  type Account,
  type CashFlowSeries,
  type DashboardSummary,
  type Entity,
  type PostedEntryView,
} from '../lib/api'
import { formatPercentFromBps } from '../lib/arc'
import { ArcTile } from '../components/Arc'
import { CashFlowLight } from '../components/CashFlowLight'
import { TopBar } from '../components/TopBar'
import {
  AmountPill,
  Button,
  EmptyState,
  ErrorBanner,
  Hero,
  IconBadge,
  ListRow,
  MoneyPill,
  Panel,
  Segmented,
} from '../components/ui'
import { cn } from '../lib/cn'
import type { CommandError } from '../lib/tauri'
import { useI18n } from '../lib/I18nProvider'

type Props = { entity: Entity | null; onCreateBook?: () => void }

type Period = 'month' | 'quarter' | 'year'

type ActivityRow = {
  id: string
  date: string
  description: string
  signedMinor: number
  kind: 'income' | 'expense' | 'transfer' | 'other'
}

function inferActivity(view: PostedEntryView, accounts: Account[]): ActivityRow {
  const map = new Map(accounts.map((a) => [a.id, a]))
  const types = view.lines.map((l) => map.get(l.account_id)?.account_type)
  const amount = view.lines.reduce((s, l) => s + l.debit.amount_minor, 0)
  let kind: ActivityRow['kind'] = 'other'
  let signed = amount
  if (types.includes('expense')) {
    kind = 'expense'
    signed = -amount
  } else if (types.includes('income')) {
    kind = 'income'
    signed = amount
  } else if (types.every((t) => t === 'asset' || t === 'liability')) {
    kind = 'transfer'
  }
  return {
    id: view.entry.id,
    date: formatDate(view.entry.entry_date),
    description: view.entry.description,
    signedMinor: signed,
    kind,
  }
}

/**
 * The full calendar period containing today, so entries dated ahead (scanned
 * bills carry their due date) count toward it immediately.
 */
function periodBounds(period: Period): { from: string; to: string } {
  if (period === 'month') return { from: monthStartISO(), to: monthEndISO() }
  if (period === 'quarter') return { from: quarterStartISO(), to: quarterEndISO() }
  return { from: yearStartISO(), to: yearEndISO() }
}

export function DashboardPage({ entity, onCreateBook }: Props) {
  const { t, locale } = useI18n()
  const [data, setData] = useState<DashboardSummary | null>(null)
  const [series, setSeries] = useState<CashFlowSeries | null>(null)
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [period, setPeriod] = useState<Period>('month')

  const { from, to } = periodBounds(period)
  const assetsAsOf = todayISO()
  const periodWord = t(`dashboard.period.word.${period}`)

  const periodTitle = useMemo(() => {
    const start = new Date(`${from}T12:00:00`)
    if (period === 'month') {
      return new Intl.DateTimeFormat(locale, { month: 'long', year: 'numeric' }).format(start)
    }
    if (period === 'quarter') {
      return t('dashboard.period.quarterTitle', {
        quarter: Math.floor(start.getMonth() / 3) + 1,
        year: start.getFullYear(),
      })
    }
    return String(start.getFullYear())
  }, [from, period, locale, t])

  useEffect(() => {
    if (!entity) {
      setData(null)
      setSeries(null)
      setEntries([])
      setAccounts([])
      return
    }
    let cancelled = false
    setLoading(true)
    void (async () => {
      try {
        const [summary, flow, list, accts] = await Promise.all([
          api.dashboardSummary(entity.id, from, to, assetsAsOf),
          api.cashFlowSeries(entity.id, from, to),
          api.entryList(entity.id, { from, to }),
          api.accountList(entity.id),
        ])
        if (!cancelled) {
          setData(summary)
          setSeries(flow)
          setEntries(list)
          setAccounts(accts)
          setError(null)
        }
      } catch (err) {
        if (!cancelled) setError((err as CommandError).message)
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
    // assetsAsOf follows the same clock as from/to; refetching on it alone adds nothing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity, from, to])

  const activity = useMemo(() => {
    return entries
      .filter((e) => !e.is_voided)
      .slice(0, 8)
      .map((e) => inferActivity(e, accounts))
  }, [entries, accounts])

  if (!entity) {
    return (
      <EmptyState
        icon={<Landmark className="size-5" />}
        title={t('dash.createTitle')}
        body={t('dash.createBody')}
        action={
          onCreateBook ? (
            <Button onClick={onCreateBook}>{t('empty.createBook')}</Button>
          ) : undefined
        }
      />
    )
  }

  const ccy = entity.base_currency
  const loc = localeForCurrency(ccy)
  const money = (n: number, signed = false) => formatMoney(n, ccy, loc, { signed })

  const pending = loading && !data
  const income = data?.income ?? 0
  const expenses = data?.expenses ?? 0
  const net = data?.net_income ?? 0
  const assets = data?.cash_like_assets ?? 0
  const savings = data?.savings_rate_bps ?? null
  const previous = data?.net_vs_previous_bps ?? null
  const top = data?.top_expense ?? null
  const netTone = pending || net === 0 ? 'zero' : net > 0 ? 'in' : 'out'
  const noValue = t('dashboard.arc.noValue')

  return (
    <div className="space-y-3.5">
      <TopBar
        title={entity.name}
        subtitle={`${periodTitle} · ${ccy}`}
        actions={
          <Segmented<Period>
            value={period}
            onChange={setPeriod}
            options={[
              { id: 'month', label: t('dashboard.period.month') },
              { id: 'quarter', label: t('dashboard.period.quarter') },
              { id: 'year', label: t('dashboard.period.year') },
            ]}
          />
        }
      />

      <ErrorBanner message={error} />

      <Hero>
        <div className="flex flex-wrap items-start justify-between gap-6 px-7 pt-6">
          <div className="min-w-0">
            <p className="font-mono text-[11px] font-medium tracking-[0.16em] text-[var(--color-muted)] uppercase">
              {t('dash.netThis', { period: periodWord })}
            </p>
            <p
              data-net={netTone}
              title={money(net, true)}
              className={cn(
                'mt-2.5 truncate text-[clamp(2.75rem,5.2vw,4.125rem)] leading-none font-semibold tracking-[-0.02em] tabular-nums',
                netTone === 'in'
                  ? 'net-figure-in'
                  : netTone === 'out'
                    ? 'net-figure-out'
                    : 'text-[var(--color-fg)]',
              )}
            >
              {pending ? '—' : money(net, true)}
            </p>
            {savings !== null && savings > 0 ? (
              <p className="mt-2.5 text-[15px] text-[var(--color-fg-secondary)]">
                {t('dashboard.hero.kept', { percent: formatPercentFromBps(savings, locale) })}
              </p>
            ) : null}
          </div>
          <div className="flex shrink-0 flex-col items-end gap-2">
            <MoneyPill tone="in" label={t('dashboard.pill.in')} value={pending ? '—' : money(income)} />
            <MoneyPill tone="out" label={t('dashboard.pill.out')} value={pending ? '—' : money(expenses)} />
            <MoneyPill tone="neutral" label={t('dashboard.pill.assets')} value={pending ? '—' : money(assets)} />
          </div>
        </div>
        <CashFlowLight
          series={series}
          className="-mt-6 h-[170px]"
          label={
            data
              ? t('dashboard.light.label', {
                  income: money(income),
                  expenses: money(expenses),
                  net: money(net, true),
                  range: periodTitle,
                })
              : t('dashboard.light.empty')
          }
        />
      </Hero>

      <div className="grid gap-3.5 sm:grid-cols-2 lg:grid-cols-4">
        <ArcTile
          label={t('dashboard.arc.savings.label')}
          hint={t('dashboard.arc.savings.hint')}
          bps={savings}
          tone="in"
          locale={locale}
          noValueLabel={noValue}
        />
        <ArcTile
          label={t('dashboard.arc.spend.label')}
          hint={t('dashboard.arc.spend.hint')}
          bps={data?.spend_ratio_bps ?? null}
          tone="out"
          locale={locale}
          noValueLabel={noValue}
        />
        <ArcTile
          label={t('dashboard.arc.previous.label')}
          hint={t(`dashboard.arc.previous.hint.${period}`)}
          bps={previous}
          signed
          tone={previous !== null && previous < 0 ? 'out' : 'in'}
          locale={locale}
          noValueLabel={noValue}
        />
        <ArcTile
          label={t('dashboard.arc.top.label')}
          hint={top ? top.name : t('dashboard.arc.top.none')}
          bps={top ? top.share_bps : null}
          tone="out"
          locale={locale}
          noValueLabel={noValue}
        />
      </div>

      <Panel
        title={t('dash.recentActivity')}
        description={t('dash.entriesThis', {
          count: data?.recent_entry_count ?? 0,
          period: periodWord,
        })}
        icon={<Receipt className="size-4" />}
      >
        {activity.length === 0 ? (
          <div className="px-5 py-12 text-center text-sm text-[var(--color-muted)]">
            {loading ? t('common.loading') : t('dash.noEntries', { period: periodWord })}
          </div>
        ) : (
          <ul className="divide-y divide-[var(--color-border)]">
            {activity.map((row) => (
              <ListRow key={row.id}>
                <IconBadge
                  tone={
                    row.kind === 'income' ? 'money-in' : row.kind === 'expense' ? 'money-out' : 'muted'
                  }
                >
                  {row.kind === 'income' ? (
                    <ArrowDownLeft className="size-4" />
                  ) : row.kind === 'expense' ? (
                    <ArrowUpRight className="size-4" />
                  ) : (
                    <Landmark className="size-4" />
                  )}
                </IconBadge>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                    {row.description}
                  </div>
                  <div className="text-xs text-[var(--color-muted)]">
                    {row.date}
                    <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                    <span>{t(`kind.${row.kind}`)}</span>
                  </div>
                </div>
                <AmountPill
                  tone={row.kind === 'income' ? 'in' : row.kind === 'expense' ? 'out' : 'neutral'}
                >
                  {money(row.signedMinor, row.kind === 'income' || row.kind === 'expense')}
                </AmountPill>
              </ListRow>
            ))}
          </ul>
        )}
      </Panel>
    </div>
  )
}
```

In `web/src/components/ui.tsx`, delete the whole `FlowBar` function and its doc comment. `grep -rn FlowBar web/src` must then print nothing.

Locales (edit by hand):

`en.json`, after `"dash.noEntries": ...`:

```json
  "dashboard.period.quarter": "Quarter",
  "dashboard.period.word.quarter": "quarter",
  "dashboard.period.quarterTitle": "Q{quarter} {year}",
  "dashboard.hero.kept": "{percent}% of everything that came in, kept.",
  "dashboard.pill.in": "In",
  "dashboard.pill.out": "Out",
  "dashboard.pill.assets": "Assets",
  "dashboard.light.label": "Money in {income}, money out {expenses}, net {net}, {range}.",
  "dashboard.light.empty": "No cash flow to draw yet.",
  "dashboard.arc.noValue": "No value yet",
  "dashboard.arc.savings.label": "Savings rate",
  "dashboard.arc.savings.hint": "Net ÷ income",
  "dashboard.arc.spend.label": "Spend ratio",
  "dashboard.arc.spend.hint": "Out ÷ in",
  "dashboard.arc.previous.label": "Net vs last period",
  "dashboard.arc.previous.hint.month": "Against last month",
  "dashboard.arc.previous.hint.quarter": "Against last quarter",
  "dashboard.arc.previous.hint.year": "Against last year",
  "dashboard.arc.top.label": "Top spend",
  "dashboard.arc.top.none": "No expenses yet",
```

In each of `el.json`, `fr.json` and `de.json`, inside `"dashboard"`:
- In `"period"`, add `"quarter"` and `"quarterTitle"`.
- In `"period" > "word"`, add `"quarter"`.
- In `"hero"`, add `"kept"`.
- Add the objects `"pill"`, `"light"` and `"arc"`.

The values:

| Path | el | fr | de |
|---|---|---|---|
| `period.quarter` | Τρίμηνο | Trimestre | Quartal |
| `period.word.quarter` | τρίμηνο | de ce trimestre | Quartal |
| `period.quarterTitle` | {quarter}ο τρίμηνο {year} | T{quarter} {year} | Q{quarter} {year} |
| `hero.kept` | Κρατήσατε το {percent}% όσων μπήκαν. | {percent} % de tout ce qui est entré, conservé. | {percent} % von allem, was hereinkam, behalten. |
| `pill.in` | Εισροές | Entrées | Ein |
| `pill.out` | Εκροές | Sorties | Aus |
| `pill.assets` | Ενεργητικό | Actifs | Vermögen |
| `light.label` | Εισροές {income}, εκροές {expenses}, καθαρό {net}, {range}. | Entrées {income}, sorties {expenses}, net {net}, {range}. | Eingänge {income}, Ausgänge {expenses}, netto {net}, {range}. |
| `light.empty` | Δεν υπάρχει ακόμη ταμειακή ροή για σχεδίαση. | Aucun flux de trésorerie à afficher pour l’instant. | Noch kein Geldfluss zum Anzeigen. |
| `arc.noValue` | Δεν υπάρχει τιμή ακόμη | Pas encore de valeur | Noch kein Wert |
| `arc.savings.label` | Ποσοστό αποταμίευσης | Taux d’épargne | Sparquote |
| `arc.savings.hint` | Καθαρό ÷ έσοδα | Net ÷ revenus | Netto ÷ Einnahmen |
| `arc.spend.label` | Δείκτης δαπανών | Taux de dépense | Ausgabenquote |
| `arc.spend.hint` | Εκροές ÷ εισροές | Sorties ÷ entrées | Aus ÷ Ein |
| `arc.previous.label` | Καθαρό έναντι προηγούμενης περιόδου | Net vs période précédente | Netto ggü. Vorperiode |
| `arc.previous.hint.month` | Έναντι του προηγούμενου μήνα | Par rapport au mois dernier | Gegenüber dem Vormonat |
| `arc.previous.hint.quarter` | Έναντι του προηγούμενου τριμήνου | Par rapport au trimestre précédent | Gegenüber dem Vorquartal |
| `arc.previous.hint.year` | Έναντι του προηγούμενου έτους | Par rapport à l’année dernière | Gegenüber dem Vorjahr |
| `arc.top.label` | Μεγαλύτερη δαπάνη | Première dépense | Größte Ausgabe |
| `arc.top.none` | Δεν υπάρχουν έξοδα ακόμη | Aucune dépense pour l’instant | Noch keine Ausgaben |

For example, the Greek `"arc"` object is:

```json
    "arc": {
      "noValue": "Δεν υπάρχει τιμή ακόμη",
      "savings": { "label": "Ποσοστό αποταμίευσης", "hint": "Καθαρό ÷ έσοδα" },
      "spend": { "label": "Δείκτης δαπανών", "hint": "Εκροές ÷ εισροές" },
      "previous": {
        "label": "Καθαρό έναντι προηγούμενης περιόδου",
        "hint": {
          "month": "Έναντι του προηγούμενου μήνα",
          "quarter": "Έναντι του προηγούμενου τριμήνου",
          "year": "Έναντι του προηγούμενου έτους"
        }
      },
      "top": { "label": "Μεγαλύτερη δαπάνη", "none": "Δεν υπάρχουν έξοδα ακόμη" }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/lib/api.period.test.ts src/pages/DashboardPage.test.tsx src/lib/i18n.test.ts`
Expected: PASS, including the locale parity guard.

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/lib/api.ts web/src/lib/api.period.test.ts web/src/pages/DashboardPage.tsx web/src/pages/DashboardPage.test.tsx web/src/components/ui.tsx web/src/locales/en.json web/src/locales/el.json web/src/locales/fr.json web/src/locales/de.json
git commit -m "feat(web): the dashboard draws money as light" -m "The mockup's composition: the book, period and Month/Quarter/Year in the top bar; a hero with the net figure in the Ledger light, IN, OUT and ASSETS pills and the cash-flow light across its lower half; four arc tiles for savings rate, spend ratio, net against last period and top spend; recent activity with amounts on pills. The bars, metric cards and explanatory copy the light replaces are gone, and so is FlowBar."
```

---
### Task 10: Transactions, composed as in the mockup

**Files:**
- Modify: `web/src/pages/TransactionsPage.tsx`
- Modify: `web/src/pages/TransactionsPage.test.tsx`
- Modify: `web/src/components/DocumentDropZone.tsx` (presentation only)
- Modify: `web/src/locales/en.json`, `el.json`, `fr.json`, `de.json`

**Interfaces:**
- Consumes:
  - `api.cashFlowSeries` (Task 3).
  - `CashFlowLight` (Task 5).
  - `MoneyPill`, `AmountPill` and `.net-figure-in/out` (Task 7).
  - `TopBar` and the Quick add intent props (Task 8).
- Produces: nothing new for later tasks.

**Composition** (mockup frame 02; decision D9):
- **Top bar:** "Transactions", "Book · EUR" and New Entry. New Entry leaves the list toolbar.
- **A two-column row:**
  - A summary pane: "In view · 01/08/2026 – 31/08/2026", the net figure in the Ledger light, IN and OUT pills, and a strip of the light.
  - A compact drop zone.
- **One glass filter row:** search, from, to, account.
- **The entries list:** Recurring, Import CSV and Export CSV in its header. Amounts on pills. The delete action is revealed on hover and on keyboard focus.

The page's eyebrow, description and meta line go; the top bar replaces them.

**The summary follows the date filter only.**
- It asks Rust with `from` and `to` as `null` when unset, and Rust resolves them (Task 1 `activity_window`).
- Search and account filters do not narrow it, and a caption says so.
- An inverted range (from after to) draws nothing and says why; there is no window to ask Rust for.

The drop zone restyle is presentation only and is verified by the Task 12 screenshots. Its tests stay in `TransactionsPage.test.tsx`, which mocks it (ruling pattern R4).

- [ ] **Step 1: Write the failing tests**

In `web/src/pages/TransactionsPage.test.tsx`:

1. Add `cashFlowSeries: vi.fn(),` to the mocked `api` object.
2. Below the `DocumentDropZone` mock, add:

```tsx
// jsdom has no 2D canvas; the light's painting is tested on its own.
vi.mock('../components/CashFlowLight', () => ({
  CashFlowLight: ({ label }: { label: string }) => <div role="img" aria-label={label} />,
}))
```

3. Add `CashFlowSeries` to the type import from `'../lib/api'`, and add `import { formatMoney } from '../lib/money'` after `import { api } from '../lib/api'`.
4. Add after the `entity` fixture:

```tsx
const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-31',
  granularity: 'day',
  total_income_minor: 120000,
  total_expenses_minor: 4250,
  net_minor: 115750,
  buckets: [],
}

/** Rendered money carries a no-break space; text matchers see a plain one. */
const plain = (text: string) => text.replace(/\s/g, ' ')
```

5. In `beforeEach`, add `vi.mocked(api.cashFlowSeries).mockReset().mockResolvedValue(series)`.
6. Replace the test `'toolbar shows Recurring left of Import CSV, Export CSV, New Entry'` with:

```tsx
  test('the list header carries Recurring, Import CSV and Export CSV; New Entry sits in the top bar', async () => {
    await renderReady()
    const names = screen.getAllByRole('button').map((el) => el.textContent?.replace(/\s+/g, ' ').trim())
    const newEntry = names.indexOf('New Entry')
    const recurring = names.indexOf('Recurring')
    const importCsv = names.indexOf('Import CSV')
    const exportCsv = names.indexOf('Export CSV')
    expect(newEntry).toBeGreaterThanOrEqual(0)
    expect(recurring).toBeGreaterThan(newEntry)
    expect(importCsv).toBeGreaterThan(recurring)
    expect(exportCsv).toBeGreaterThan(importCsv)
    expect(names.filter((name) => name === 'New Entry')).toHaveLength(1)
  })
```

7. Append:

```tsx
describe('TransactionsPage summary', () => {
  test('shows the Rust totals for the whole book when no dates are set', async () => {
    await renderReady()

    await waitFor(() => {
      expect(api.cashFlowSeries).toHaveBeenCalledWith('e1', null, null)
    })
    const heading = await screen.findByRole('heading', { name: 'In view · 01/08/2026 – 31/08/2026' })
    const pane = heading.closest('section')
    expect(pane?.querySelector('[data-net]')).toHaveAttribute('data-net', 'in')
    expect(pane?.querySelector('[data-net]')).toHaveTextContent(plain(formatMoney(115750, 'EUR', undefined, { signed: true })))
    expect(pane?.querySelector('[data-money-pill="in"]')).toHaveTextContent(plain(`In ${formatMoney(120000, 'EUR')}`))
    expect(pane?.querySelector('[data-money-pill="out"]')).toHaveTextContent(plain(`Out ${formatMoney(4250, 'EUR')}`))
  })

  test('a date filter narrows the summary', async () => {
    await renderReady()

    await userEvent.type(screen.getByLabelText('Filter from date'), '01/08/2026')
    await userEvent.tab()

    await waitFor(() => {
      expect(api.cashFlowSeries).toHaveBeenLastCalledWith('e1', '2026-08-01', null)
    })
  })

  test('the account filter leaves the summary whole, and says so', async () => {
    await renderReady()
    const select = screen.getByRole('combobox', { name: 'Account' }) as HTMLSelectElement

    await userEvent.selectOptions(select, select.options[1]?.value ?? '')

    await waitFor(() => {
      expect(api.entryList).toHaveBeenLastCalledWith('e1', expect.objectContaining({ accountId: select.options[1]?.value }))
    })
    expect(api.cashFlowSeries).toHaveBeenLastCalledWith('e1', null, null)
    expect(screen.getByText("The account and search filters don't change this summary.")).toBeInTheDocument()
  })

  test('an inverted date range draws nothing and says why', async () => {
    await renderReady()

    await userEvent.type(screen.getByLabelText('Filter from date'), '31/08/2026')
    await userEvent.tab()
    await userEvent.type(screen.getByLabelText('Filter to date'), '01/08/2026')
    await userEvent.tab()

    expect(await screen.findByText('The From date must be on or before the To date.')).toBeInTheDocument()
    expect(api.cashFlowSeries).not.toHaveBeenCalledWith('e1', '2026-08-31', '2026-08-01')
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run src/pages/TransactionsPage.test.tsx`
Expected: FAIL. `cashFlowSeries` is never called, there is no "In view" heading, and New Entry still sits after Export CSV.

- [ ] **Step 3: Implement**

In `web/src/pages/TransactionsPage.tsx`:

1. Imports:
   - Add `type CashFlowSeries` to the `'../lib/api'` import.
   - Add `import { CashFlowLight } from '../components/CashFlowLight'` and `import { TopBar } from '../components/TopBar'`.
   - In the `'../components/ui'` import, remove `PageHeader` and add `AmountPill` and `MoneyPill`.

2. After `const [csvRoles, setCsvRoles] = ...`, add:

```tsx
  const [series, setSeries] = useState<CashFlowSeries | null>(null)
  const summaryHeadingId = useId()
```

3. After `const filtersActive = ...`, add:

```tsx
  /** From after To: there is no window to draw, so the summary says so instead. */
  const invalidRange = Boolean(fromDate && toDate && fromDate > toDate)
  /** The summary follows dates only; these filters narrow the list, not it. */
  const filtersNarrowList = Boolean(debouncedSearch.trim() || accountFilter)
```

4. Replace the body of `reload()` with:

```tsx
  async function reload() {
    if (!entity) return
    const rangeInverted = Boolean(fromDate && toDate && fromDate > toDate)
    const [e, a, d, flow] = await Promise.all([
      api.entryList(entity.id, {
        search: debouncedSearch.trim() || undefined,
        from: fromDate || undefined,
        to: toDate || undefined,
        accountId: accountFilter || undefined,
      }),
      api.accountList(entity.id),
      api.documentList(entity.id),
      // The summary follows the date filter only; Rust resolves an open bound.
      rangeInverted ? Promise.resolve(null) : api.cashFlowSeries(entity.id, fromDate || null, toDate || null),
    ])
    setEntries(e)
    setAccounts(a)
    setDocs(d)
    setSeries(flow)
    if (!categoryId && !walletId) {
      applyKindDefaults(kind, a)
    }
  }
```

5. Replace the recurring sub-view return with:

```tsx
  if (subview === 'recurring') {
    return (
      <>
        <TopBar title={t('tx.title')} subtitle={`${entity.name} · ${ccy}`} />
        <RecurringPage entity={entity} onBack={() => setSubview('journal')} />
      </>
    )
  }
```

6. In `csvActions`, delete the last `<Button size="sm" onClick={openNewEntry}> ... </Button>`, and add after `csvActions`:

```tsx
  const newEntryButton = (
    <Button onClick={openNewEntry}>
      <Plus className="size-4" />
      {t('tx.newEntry')}
    </Button>
  )

  const netTone = !series || series.net_minor === 0 ? 'zero' : series.net_minor > 0 ? 'in' : 'out'
  const range = series ? `${formatDate(series.from)} – ${formatDate(series.to)}` : null
```

7. Replace the `<PageHeader ... />` element with:

```tsx
      <TopBar title={t('tx.title')} subtitle={`${entity.name} · ${ccy}`} actions={newEntryButton} />
```

and change the root `<div className="space-y-6">` to `<div className="space-y-3.5">`.

8. Replace the `<DocumentDropZone ... />` element (keep its props exactly) with this row:

```tsx
      <div className="grid gap-3.5 lg:grid-cols-[minmax(0,1.75fr)_minmax(0,1fr)]">
        <section aria-labelledby={summaryHeadingId} className="glass-pane relative overflow-hidden rounded-[22px]">
          <div className="flex flex-wrap items-start justify-between gap-3.5 px-5.5 pt-4.5">
            <div className="min-w-0">
              <h2
                id={summaryHeadingId}
                className="font-mono text-[10.5px] font-medium tracking-[0.16em] text-[var(--color-muted)] uppercase"
              >
                {range ? t('tx.summary.inView', { range }) : t('tx.summary.inViewEmpty')}
              </h2>
              <p
                data-net={netTone}
                className={cn(
                  'mt-2 truncate text-[2.125rem] leading-none font-semibold tracking-[-0.01em] tabular-nums',
                  netTone === 'in' ? 'net-figure-in' : netTone === 'out' ? 'net-figure-out' : 'text-[var(--color-fg)]',
                )}
              >
                {series ? formatMoney(series.net_minor, ccy, undefined, { signed: true }) : '—'}
              </p>
              {invalidRange ? (
                <p className="mt-1.5 text-xs text-[var(--color-danger)]">{t('tx.summary.invalidRange')}</p>
              ) : filtersNarrowList ? (
                <p className="mt-1.5 text-xs text-[var(--color-muted)]">{t('tx.summary.wholeBook')}</p>
              ) : null}
            </div>
            <div className="flex flex-wrap gap-2">
              <MoneyPill
                tone="in"
                label={t('dashboard.pill.in')}
                value={series ? formatMoney(series.total_income_minor, ccy) : '—'}
              />
              <MoneyPill
                tone="out"
                label={t('dashboard.pill.out')}
                value={series ? formatMoney(series.total_expenses_minor, ccy) : '—'}
              />
            </div>
          </div>
          <CashFlowLight
            series={series}
            className="-mt-1 h-[76px]"
            label={
              series && range
                ? t('dashboard.light.label', {
                    income: formatMoney(series.total_income_minor, ccy),
                    expenses: formatMoney(series.total_expenses_minor, ccy),
                    net: formatMoney(series.net_minor, ccy, undefined, { signed: true }),
                    range,
                  })
                : t('dashboard.light.empty')
            }
          />
        </section>

        <section className="glass-pane rounded-[22px] p-2.5">
          <DocumentDropZone
            entityId={entity.id}
            onSuggestion={(s, source) => {
              setError(null)
              applySuggestion(s, source)
              if (s.source === 'none' && !s.amount_minor) {
                setError(s.notes || t('tx.couldNotReadDoc'))
              }
            }}
            onError={(msg) => setError(msg)}
          />
        </section>
      </div>
```

9. Replace the filter row `<div className="flex flex-wrap items-end gap-3"> ... </div>` with:

```tsx
      <div className="glass-pane grid gap-2 rounded-[18px] p-2 md:grid-cols-[minmax(0,1fr)_9.5rem_9.5rem_12rem]">
        <Field label={t('tx.search')}>
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t('tx.searchPlaceholder')}
          />
        </Field>
        <Field label={t('tx.from')}>
          <DateInput value={fromDate} onChange={setFromDate} aria-label={t('tx.filterFrom')} />
        </Field>
        <Field label={t('tx.to')}>
          <DateInput value={toDate} onChange={setToDate} aria-label={t('tx.filterTo')} />
        </Field>
        <Field label={t('tx.account')}>
          <Select
            value={accountFilter}
            onChange={(e) => setAccountFilter(e.target.value)}
            aria-label={t('tx.account')}
          >
            <option value="">{t('tx.allAccounts')}</option>
            {accounts
              .filter((a) => a.is_active)
              .map((a) => (
                <option key={a.id} value={a.id}>
                  {a.code} · {a.name}
                </option>
              ))}
          </Select>
        </Field>
      </div>
```

10. In the entries list:
   - Change the row `<li ... className="flex cursor-pointer items-center gap-4 px-5 py-3.5 transition hover:bg-[var(--color-surface-2)]/50">` to `className="group flex cursor-pointer items-center gap-4 px-5 py-3.5 transition hover:bg-white/[0.05] focus-within:bg-white/[0.05]"`.
   - Replace the amount `<div className={cn('shrink-0 text-sm font-semibold tabular-nums', ...)}> ... </div>` with:

```tsx
                  <AmountPill tone={kindLabel === 'income' ? 'in' : kindLabel === 'expense' ? 'out' : 'neutral'}>
                    {formatMoney(signed, ccy, undefined, {
                      signed: kindLabel === 'expense' || kindLabel === 'income',
                    })}
                  </AmountPill>
```

   - On the delete `Button`, change `className="h-8 w-8 shrink-0"` to `className="h-8 w-8 shrink-0 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"`.

11. `cn` is still used by the net figure; keep its import.

In `web/src/components/DocumentDropZone.tsx`, replace the component's `return ( ... )` with the compact, brand-lit well from the mockup (same handlers, same file input, same copy):

```tsx
  return (
    <div
      onDragOver={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!disabled) setDragOver(true)
      }}
      onDragEnter={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!disabled) setDragOver(true)
      }}
      onDragLeave={(e) => {
        e.preventDefault()
        // Only clear when leaving the zone itself
        if (e.currentTarget === e.target) setDragOver(false)
      }}
      onDrop={onHtmlDrop}
      className={cn(
        'relative h-full min-h-[7.5rem] overflow-hidden rounded-[15px] border-[1.5px] border-dashed px-4.5 py-4 text-left transition',
        dragOver
          ? 'border-[var(--color-accent-b)] bg-[var(--color-info-soft)]'
          : 'border-[rgba(55,213,255,0.32)]',
        disabled || busy ? 'opacity-60' : 'hover:border-[rgba(55,213,255,0.55)]',
      )}
    >
      {!dragOver ? (
        <div
          className="pointer-events-none absolute inset-0"
          style={{ background: 'radial-gradient(260px 120px at 50% 0%, rgba(55,213,255,0.08), transparent 70%)' }}
          aria-hidden
        />
      ) : null}
      <input
        type="file"
        accept="image/png,image/jpeg,image/webp,application/pdf,text/plain,.pdf,.png,.jpg,.jpeg,.webp,.txt"
        className="absolute inset-0 z-10 cursor-pointer opacity-0"
        disabled={disabled || busy}
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) void processFile(file)
          e.target.value = ''
        }}
      />
      <div className="pointer-events-none relative grid grid-cols-[46px_minmax(0,1fr)] items-center gap-3.5">
        <span className="flex size-[46px] items-center justify-center rounded-[14px] bg-[linear-gradient(135deg,var(--color-accent),var(--color-accent-b))] text-[var(--color-on-accent)] shadow-[0_0_24px_rgba(55,213,255,0.35)]">
          {busy ? <Loader2 className="size-5 animate-spin" /> : <FileUp className="size-5" strokeWidth={1.75} />}
        </span>
        <div className="min-w-0">
          <p className="text-[15px] leading-snug font-semibold text-[var(--color-fg)]">
            {busy ? t('drop.analyzing') : t('drop.title')}
          </p>
          <p className="mt-1 text-[12.5px] leading-snug text-[var(--color-muted)]">{t('drop.body')}</p>
        </div>
      </div>
      {status ? (
        <p className="pointer-events-none relative mt-3 flex items-start gap-1.5 text-[11px] leading-snug text-[var(--color-muted)]">
          <Sparkles className="mt-0.5 size-3 shrink-0 text-[var(--color-accent)]" />
          {status.hint}
        </p>
      ) : null}
      {localError ? (
        <p className="pointer-events-none relative mt-2 text-xs text-[var(--color-danger)]">{localError}</p>
      ) : null}
    </div>
  )
```

Locales (edit by hand):
- `en.json`, after `"tx.filterTo": "Filter to date",`:

```json
  "tx.summary.inView": "In view · {range}",
  "tx.summary.inViewEmpty": "In view",
  "tx.summary.wholeBook": "The account and search filters don't change this summary.",
  "tx.summary.invalidRange": "The From date must be on or before the To date.",
```

- `el.json`, `fr.json` and `de.json`: inside `"tx"`, add a `"summary"` object:
  - el: `{ "inView": "Σε προβολή · {range}", "inViewEmpty": "Σε προβολή", "wholeBook": "Τα φίλτρα λογαριασμού και αναζήτησης δεν αλλάζουν αυτή τη σύνοψη.", "invalidRange": "Η ημερομηνία Από πρέπει να είναι έως και την ημερομηνία Έως." }`
  - fr: `{ "inView": "Affiché · {range}", "inViewEmpty": "Affiché", "wholeBook": "Les filtres de compte et de recherche ne modifient pas ce résumé.", "invalidRange": "La date Du doit être antérieure ou égale à la date Au." }`
  - de: `{ "inView": "Angezeigt · {range}", "inViewEmpty": "Angezeigt", "wholeBook": "Konto- und Suchfilter ändern diese Übersicht nicht.", "invalidRange": "Das Von-Datum muss vor oder auf dem Bis-Datum liegen." }`

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/pages/TransactionsPage.test.tsx src/lib/i18n.test.ts`
Expected: PASS: every existing Transactions test plus the four summary tests and the rewritten toolbar test.

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/pages/TransactionsPage.tsx web/src/pages/TransactionsPage.test.tsx web/src/components/DocumentDropZone.tsx web/src/locales/en.json web/src/locales/el.json web/src/locales/fr.json web/src/locales/de.json
git commit -m "feat(web): Transactions gets the light, a glass filter row and a compact drop zone" -m "The mockup's ledger: New Entry in the top bar; a summary pane with the period's net in the Ledger light, IN and OUT pills and a strip of the cash-flow light beside a compact drop zone; one glass filter row; the list with Recurring and CSV in its header, amounts on pills and the delete action revealed on hover and focus. The summary follows the date filter only, with Rust filling an open bound, and says so when account or search filters narrow the list."
```

---
### Task 11: Dialogs wear the chosen entry type

**Files:**
- Modify: `web/src/pages/TransactionsPage.tsx` (the New entry type selector)
- Modify: `web/src/pages/TransactionsPage.test.tsx`
- Modify: `web/src/pages/RecurringPage.tsx` (the template type selector)
- Modify: `web/src/pages/RecurringPage.test.tsx`

**Interfaces:**
- Consumes: the `Segmented` option `tone` (Task 7).
- Produces: nothing new.

Spec, Dialogs: "The chosen entry type colours the type selector: Expense and Bill in the out gradient, Income in the in gradient, Transfer neutral." Every dialog already renders as glass through a portal since phase 1. The Quick add tray's selector is phase 4.

- [ ] **Step 1: Write the failing tests**

Append to `web/src/pages/TransactionsPage.test.tsx`:

```tsx
describe('TransactionsPage New entry type colours', () => {
  test('the chosen type wears its Ledger gradient; Transfer stays neutral', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    await screen.findByRole('heading', { name: 'New entry' })

    expect(screen.getByRole('button', { name: 'Expense' })).toHaveAttribute('data-tone', 'money-out')

    await userEvent.click(screen.getByRole('button', { name: 'Income' }))
    expect(screen.getByRole('button', { name: 'Income' })).toHaveAttribute('data-tone', 'money-in')

    await userEvent.click(screen.getByRole('button', { name: 'Bill' }))
    expect(screen.getByRole('button', { name: 'Bill' })).toHaveAttribute('data-tone', 'money-out')

    await userEvent.click(screen.getByRole('button', { name: 'Transfer' }))
    expect(screen.getByRole('button', { name: 'Transfer' })).toHaveAttribute('data-tone', 'neutral')
  })
})
```

Append to `web/src/pages/RecurringPage.test.tsx`:

```tsx
describe('RecurringPage template type colours', () => {
  test('the chosen type wears its Ledger gradient; Transfer stays neutral', async () => {
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await screen.findByRole('heading', { name: 'New template' })

    expect(screen.getByRole('button', { name: 'Expense' })).toHaveAttribute('data-tone', 'money-out')

    await userEvent.click(screen.getByRole('button', { name: 'Income' }))
    expect(screen.getByRole('button', { name: 'Income' })).toHaveAttribute('data-tone', 'money-in')

    await userEvent.click(screen.getByRole('button', { name: 'Transfer' }))
    expect(screen.getByRole('button', { name: 'Transfer' })).toHaveAttribute('data-tone', 'neutral')
  })
})
```

If the Recurring test file does not already import `userEvent` and `screen`, add them to its imports.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && npx vitest run src/pages/TransactionsPage.test.tsx src/pages/RecurringPage.test.tsx`
Expected: FAIL: the chosen Expense button has `data-tone="neutral"`, not `money-out`.

- [ ] **Step 3: Implement**

In `web/src/pages/TransactionsPage.tsx`, in the New entry modal's `Segmented<EntryKind>` options:
- Add `tone: 'money-out'` to the `expense` and `bill` options.
- Add `tone: 'money-in'` to the `income` option.
- Leave `transfer` without a tone.

For example:

```tsx
              {
                id: 'expense',
                label: t('kind.expense'),
                icon: <ArrowUpRight className="size-3.5" />,
                tone: 'money-out',
              },
```

In `web/src/pages/RecurringPage.tsx`, in the modal's `Segmented<RecurringKind>` options, make the same change: `expense` and `bill` get `tone: 'money-out'`, `income` gets `tone: 'money-in'`, `transfer` gets none.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/pages/TransactionsPage.test.tsx src/pages/RecurringPage.test.tsx`
Expected: PASS.

- [ ] **Step 5: Run the web gates**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/pages/TransactionsPage.tsx web/src/pages/TransactionsPage.test.tsx web/src/pages/RecurringPage.tsx web/src/pages/RecurringPage.test.tsx
git commit -m "feat(web): the chosen entry type colours its selector" -m "In New entry and in recurring templates, Expense and Bill wear the out gradient and Income the in gradient while chosen; Transfer stays neutral. The dark labels on those gradients are measured at 4.5:1 or better."
```

---

### Task 12: Verify, capture, open the PR, and hand the running app over

**Files:**
- Modify (outside the repository, in the session scratchpad): `aurora-qa/qa.mjs`, `aurora-qa/text-contrast.mjs`
- Create (scratchpad): `aurora-qa/pr-body-light.md`

**Interfaces:**
- Consumes: everything above.
- Produces: a pull request, screenshots, contrast numbers, and the running app.

- [ ] **Step 1: Run every gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cargo test -p oikonomia -- --skip vault_init_stamps_trial_without_unlock
./scripts/assert-core-offline.sh
cd web && npm ci && npx tsc -b && npm run lint && npm test && npm run build
```

Expected: all green. The desktop crate's one skipped test is the macOS Keychain prompt (ruling R15); CI runs it on Linux. `cargo deny check` still fails on RUSTSEC-2026-0285, identically on `main`. Do not fix it here.

The spec's smoke check (`scripts/smoke-macos.sh` on the bundled `.app`) needs Screen Recording permission. Run it if the permission is available. If not, record the skip in the PR, as phase 1 did (ruling R5); the running-app hand-off in Step 5 covers it.

- [ ] **Step 2: Teach the QA stub the new data**

In both `aurora-qa/qa.mjs` and `aurora-qa/text-contrast.mjs`, inside the `stubs(locale)` template:

1. Add the arc fields to `dashboard_summary_cmd`:

```js
      dashboard_summary_cmd: { entity_id: 'e1', base_currency: 'EUR', cash_like_assets: 12840022, income: 7450000, expenses: 2310450, net_income: 5139550, recent_entry_count: 16, savings_rate_bps: 6899, spend_ratio_bps: 3101, top_expense: { code: '5100', name: 'Payroll', amount_minor: 1150000, share_bps: 4977 }, net_vs_previous_bps: 1240 },
```

2. Before `const table = {`, add a series builder:

```js
    // The mockup's August: the same days and amounts, repeated for whatever
    // window is asked, so the light has the mockup's shape.
    const IN = { 2: 480000, 6: 1200000, 11: 2650000, 15: 820000, 19: 300000, 23: 1500000, 28: 500000 }
    const OUT = { 1: 120000, 5: 89900, 9: 345050, 12: 62000, 16: 230000, 20: 115000, 25: 1150000, 27: 98500, 29: 100000 }
    const iso = (d) => d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0')
    const seriesFor = (from, to) => {
      const start = new Date((from || '2026-08-01') + 'T12:00:00')
      const end = new Date((to || '2026-08-31') + 'T12:00:00')
      const buckets = []
      let cumIn = 0
      let cumOut = 0
      for (let d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
        const income = IN[d.getDate()] || 0
        const expenses = OUT[d.getDate()] || 0
        cumIn += income
        cumOut += expenses
        buckets.push({ start: iso(d), end: iso(d), income_minor: income, expenses_minor: expenses, cumulative_income_minor: cumIn, cumulative_expenses_minor: cumOut })
      }
      return { entity_id: 'e1', from: iso(start), to: iso(end), granularity: 'day', total_income_minor: cumIn, total_expenses_minor: cumOut, net_minor: cumIn - cumOut, buckets }
    }
```

3. In `invoke`, before `if (cmd in table)`, add:

```js
        if (cmd === 'cash_flow_series_cmd') return Promise.resolve(seriesFor(args && args.from, args && args.to))
```

Also change the signature to `invoke: (cmd, args) => {`.

4. In `qa.mjs`'s `screenshots`, after the per-screen `page.screenshot(...)`, add the New entry dialog:

```js
      if (SCREENS[index] === 'Transactions') {
        await page.getByRole('banner').getByRole('button').first().click()
        await page.waitForTimeout(600)
        await page.screenshot({ path: `${OUT}/${locale}-2b-new-entry.png` })
        await page.keyboard.press('Escape')
        await page.waitForTimeout(300)
      }
```

- [ ] **Step 3: Capture and measure**

Start `npx vite --port 5173 --strictPort` in `web/` in the background. Run `node qa.mjs .` and `node text-contrast.mjs` in `aurora-qa/`, then stop Vite.

Expected:
- `en: page errors []` and `el: page errors []`.
- Eleven screenshots per locale: five screens, plus the New entry dialog.
- `text-contrast.mjs` reports a worst case of at least 4.50:1 both on panes and on the aurora. Gradient text is skipped by design; tokens.test.ts covers it.

If a pill or caption falls below 4.5:1, raise that text one ink tier. Change the matching token test and its "Measured" note in Task 7's table, then re-run.

Look once at `en-1-dashboard.png`, `en-2-transactions.png` and `en-2b-new-entry.png` beside the mockup frames:
- The light spans the hero's lower half.
- Four arcs in a row.
- Pills top right.
- The summary strip beside the drop zone.
- The Expense type in the out gradient.

Fix only what is visibly broken.

- [ ] **Step 4: Open the pull request**

Write `aurora-qa/pr-body-light.md` with no emoji and no attribution lines. Include:
- What changed: the Rust series and metrics, and the web pieces.
- The decisions D1-D15 from the spec's "Resolved in the phase 2-3 plan" section.
- "What moved or went away" (Task 9 and Task 10 lists).
- "What you will not see yet": phase 4's unlock glass card, the Quick add tray window, the remaining pages' pass, the Greek-locale pass; the `⌘K` shortcut and navigation counts (deferred).
- The contrast numbers from Step 3.
- The known `cargo deny` failure.

```bash
git push -u origin feat/aurora-glass-light
gh pr create --base feat/aurora-glass-foundation --title "feat(ui): Aurora glass, the light (phases 2 and 3)" --body-file "$SCRATCH/aurora-qa/pr-body-light.md"
```

- [ ] **Step 5: Hand the running app to the user**

Start `make app` from the repository root on this branch, and send the English and Greek screenshots. Lead the message with what the user will not see yet (memory `phased-handoff-name-missing-visuals`), then what moved or went away. Ask them to check CPU in Activity Monitor on the unlocked Dashboard, idle and while scrolling Transactions, including the WebKit GPU process; the light now animates as well as the aurora. Do not merge before the user has seen it.
