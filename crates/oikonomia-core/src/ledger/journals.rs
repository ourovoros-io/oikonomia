//! Journal entry posting, listing, void, and account register.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::domain::{
    AccountId, EntityId, EntryStatus, JournalEntry, JournalEntryId, JournalLine, JournalLineId,
    validate_lines_for_post,
};
use crate::error::{Error, Result};
use crate::ledger::accounts::get_account;
use crate::ledger::balance::{ACTIVE_ENTRY_PREDICATE, normal_balance};
use crate::money::Money;
use crate::util::{format_date, now_utc_string, parse_date, parse_uuid};

/// One line when posting a journal entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJournalLine {
    /// Account to debit or credit.
    pub account_id: AccountId,
    /// Debit minor units (0 if credit side).
    pub debit_minor: i64,
    /// Credit minor units (0 if debit side).
    pub credit_minor: i64,
    /// Optional memo.
    pub memo: Option<String>,
}

/// Post a balanced journal entry in one step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostJournal {
    /// Entity book.
    pub entity_id: EntityId,
    /// Accounting date `YYYY-MM-DD`.
    pub entry_date: String,
    /// Description.
    pub description: String,
    /// Optional reference.
    pub reference: Option<String>,
    /// Lines (≥ 2, balanced).
    pub lines: Vec<CreateJournalLine>,
}

/// Entry header plus lines for UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostedEntryView {
    /// Header.
    pub entry: JournalEntry,
    /// Lines.
    pub lines: Vec<JournalLine>,
    /// True if this entry has been voided.
    pub is_voided: bool,
}

/// One line in an account register.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterLine {
    /// Entry id.
    pub entry_id: JournalEntryId,
    /// Date.
    #[serde(with = "crate::util::serde_date")]
    pub entry_date: Date,
    /// Description.
    pub description: String,
    /// Debit.
    pub debit_minor: i64,
    /// Credit.
    pub credit_minor: i64,
    /// Running normal balance after this line.
    pub balance_minor: i64,
}

/// High-level kind for the simple entry form (no debit/credit knowledge in the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleEntryKind {
    /// Money spent now.
    Expense,
    /// Money received.
    Income,
    /// A bill: paid, owed, or a payment against an owed bill.
    Bill,
    /// Move money between own accounts.
    Transfer,
}

/// Payment state for [`SimpleEntryKind::Bill`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleBillStatus {
    /// Paid immediately from a wallet account.
    Paid,
    /// Recorded as owed against a payable account.
    Unpaid,
    /// Settle a previously recorded payable from a wallet account.
    PayExisting,
}

/// Input for [`post_simple_entry`]: one amount plus role accounts per kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostSimpleEntry {
    /// Entity book.
    pub entity_id: EntityId,
    /// Entry kind.
    pub kind: SimpleEntryKind,
    /// Required when `kind` is [`SimpleEntryKind::Bill`].
    pub bill_status: Option<SimpleBillStatus>,
    /// Accounting date `YYYY-MM-DD`.
    pub entry_date: String,
    /// Description.
    pub description: String,
    /// Optional reference.
    pub reference: Option<String>,
    /// Positive amount in minor units.
    pub amount_minor: i64,
    /// Expense or income category account.
    pub category_account_id: Option<AccountId>,
    /// Bank / cash / card account.
    pub wallet_account_id: Option<AccountId>,
    /// Bills payable / AP liability account.
    pub payable_account_id: Option<AccountId>,
    /// Transfer source.
    pub from_account_id: Option<AccountId>,
    /// Transfer destination.
    pub to_account_id: Option<AccountId>,
}

/// Result of voiding an entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoidResult {
    /// Original entry id.
    pub original_id: JournalEntryId,
    /// Reversing entry id.
    pub reverse_id: JournalEntryId,
}

/// Optional predicates for [`list_entries`]; every `None` means "no filter".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntryFilter {
    /// Case-insensitive substring over description, reference, and line memos.
    pub text: Option<String>,
    /// Inclusive ISO lower bound (`YYYY-MM-DD`).
    pub date_from: Option<String>,
    /// Inclusive ISO upper bound.
    pub date_to: Option<String>,
    /// Only entries with at least one line on this account.
    pub account_id: Option<AccountId>,
}

/// Wrap trimmed user text in `%…%`, escaping LIKE wildcards so `%`/`_`
/// in a search are literals, not patterns.
fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// List posted entries for an entity (newest first) matching `filter`.
///
/// # Errors
///
/// DB / validation errors.
pub fn list_entries(
    conn: &Connection,
    entity_id: EntityId,
    filter: &EntryFilter,
) -> Result<Vec<PostedEntryView>> {
    // Normalize before binding: SQL compares date TEXT lexicographically, so a
    // lenient input like `2026-3-5` must become `2026-03-05` first.
    let from = filter
        .date_from
        .as_deref()
        .map(parse_date)
        .transpose()?
        .map(format_date);
    let to = filter
        .date_to
        .as_deref()
        .map(parse_date)
        .transpose()?
        .map(format_date);

    let pattern = filter
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(like_pattern);
    let account = filter.account_id.map(|id| id.0.to_string());

    let mut stmt = conn
        .prepare(
            "
            SELECT je.id, je.entity_id, je.entry_date, je.description, je.reference,
                   je.status, je.voided_by_entry_id
            FROM journal_entries je
            WHERE je.entity_id = ?1
              AND je.status = 'posted'
              AND (?2 IS NULL OR je.entry_date >= ?2)
              AND (?3 IS NULL OR je.entry_date <= ?3)
              AND (?4 IS NULL
                   OR je.description LIKE ?4 ESCAPE '\\'
                   OR je.reference LIKE ?4 ESCAPE '\\'
                   OR EXISTS (
                       SELECT 1 FROM journal_lines jl
                       WHERE jl.entry_id = je.id AND jl.memo LIKE ?4 ESCAPE '\\'
                   ))
              AND (?5 IS NULL OR EXISTS (
                   SELECT 1 FROM journal_lines jl
                   WHERE jl.entry_id = je.id AND jl.account_id = ?5
              ))
            ORDER BY je.entry_date DESC, je.created_at DESC
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map(
            rusqlite::params![entity_id.0.to_string(), from, to, pattern, account],
            |row| {
                let voided: Option<String> = row.get(6)?;
                Ok((map_entry_row(row)?, voided.is_some()))
            },
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let (entry, mut is_voided) = row.map_err(|err| Error::Io(err.to_string()))?;
        // Also treat void-reversals as voided (including older data that only
        // linked the original → reverse, not reverse → original).
        if !is_voided {
            is_voided = entry_is_void_reverse(conn, entry.id)?;
        }
        let lines = load_lines(conn, entry.id)?;
        out.push(PostedEntryView {
            entry,
            lines,
            is_voided,
        });
    }
    Ok(out)
}

fn entry_is_void_reverse(conn: &Connection, id: JournalEntryId) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "
            SELECT COUNT(1) FROM journal_entries
            WHERE voided_by_entry_id = ?1
            ",
            [id.0.to_string()],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    Ok(count > 0)
}

/// Get one entry with lines.
///
/// # Errors
///
/// Not found or DB error.
pub fn get_entry(conn: &Connection, id: JournalEntryId) -> Result<PostedEntryView> {
    let (entry, mut is_voided) = conn
        .query_row(
            "
            SELECT id, entity_id, entry_date, description, reference, status, voided_by_entry_id
            FROM journal_entries WHERE id = ?1
            ",
            [id.0.to_string()],
            |row| {
                let voided: Option<String> = row.get(6)?;
                Ok((map_entry_row(row)?, voided.is_some()))
            },
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound("journal entry".into()),
            other => Error::Io(other.to_string()),
        })?;

    if !is_voided {
        is_voided = entry_is_void_reverse(conn, entry.id)?;
    }

    let lines = load_lines(conn, entry.id)?;
    Ok(PostedEntryView {
        entry,
        lines,
        is_voided,
    })
}

/// Validate and post a journal entry atomically.
///
/// # Errors
///
/// Unbalanced, wrong entity, or DB errors.
pub fn post_entry(conn: &Connection, input: &PostJournal) -> Result<PostedEntryView> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let view = insert_posted_entry(&tx, input)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(view)
}

/// Insert header + lines without transaction management.
///
/// Callers own the transaction: [`post_entry`] and [`void_entry`] wrap this so
/// a failure mid-insert can never leave a partial posted entry behind.
fn insert_posted_entry(conn: &Connection, input: &PostJournal) -> Result<PostedEntryView> {
    let description = input.description.trim();
    if description.is_empty() {
        return Err(Error::Validation("description is required".into()));
    }

    let entry_date = parse_date(&input.entry_date)?;
    let entry_id = JournalEntryId::new();

    let mut lines = Vec::with_capacity(input.lines.len());
    for raw in &input.lines {
        let account = get_account(conn, raw.account_id)?;
        if account.entity_id != input.entity_id {
            return Err(Error::AccountWrongEntity);
        }
        if !account.is_active {
            return Err(Error::Validation(format!(
                "account {} is inactive",
                account.code
            )));
        }

        let debit = Money::from_minor(raw.debit_minor)?;
        let credit = Money::from_minor(raw.credit_minor)?;
        lines.push(JournalLine {
            id: JournalLineId::new(),
            entry_id,
            account_id: raw.account_id,
            debit,
            credit,
            memo: raw.memo.clone(),
        });
    }

    validate_lines_for_post(&lines)?;

    let now = now_utc_string();
    let date_s = format_date(entry_date);
    let reference = input
        .reference
        .as_ref()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    conn.execute(
        "
        INSERT INTO journal_entries (
            id, entity_id, entry_date, description, reference,
            status, created_at, posted_at, voided_by_entry_id
        ) VALUES (?1, ?2, ?3, ?4, ?5, 'posted', ?6, ?6, NULL)
        ",
        rusqlite::params![
            entry_id.0.to_string(),
            input.entity_id.0.to_string(),
            date_s,
            description,
            reference,
            now,
        ],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    for (order, line) in lines.iter().enumerate() {
        conn.execute(
            "
            INSERT INTO journal_lines (
                id, entry_id, account_id, debit_minor, credit_minor, memo, line_order
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            rusqlite::params![
                line.id.0.to_string(),
                entry_id.0.to_string(),
                line.account_id.0.to_string(),
                line.debit.amount_minor(),
                line.credit.amount_minor(),
                line.memo,
                i32::try_from(order).unwrap_or(i32::MAX),
            ],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    }

    get_entry(conn, entry_id)
}

/// Build and post the journal entry for a simple-form input.
///
/// The kind → debit/credit mapping lives here so the UI never carries
/// accounting rules; each role account's type is checked before posting.
///
/// # Errors
///
/// [`Error::Validation`] for missing/mistyped role accounts or a
/// non-positive amount, plus all [`post_entry`] errors.
pub fn post_simple_entry(conn: &Connection, input: &PostSimpleEntry) -> Result<PostedEntryView> {
    if input.amount_minor <= 0 {
        return Err(Error::Validation("amount must be positive".into()));
    }

    let (debit_account, credit_account) = simple_entry_sides(conn, input)?;
    if debit_account == credit_account {
        return Err(Error::Validation(
            "entry needs two different accounts".into(),
        ));
    }

    let lines = vec![
        CreateJournalLine {
            account_id: debit_account,
            debit_minor: input.amount_minor,
            credit_minor: 0,
            memo: None,
        },
        CreateJournalLine {
            account_id: credit_account,
            debit_minor: 0,
            credit_minor: input.amount_minor,
            memo: None,
        },
    ];

    post_entry(
        conn,
        &PostJournal {
            entity_id: input.entity_id,
            entry_date: input.entry_date.clone(),
            description: input.description.clone(),
            reference: input.reference.clone(),
            lines,
        },
    )
}

/// Resolve the (debit, credit) account pair for a simple entry.
fn simple_entry_sides(
    conn: &Connection,
    input: &PostSimpleEntry,
) -> Result<(AccountId, AccountId)> {
    use crate::domain::AccountType::{Asset, Expense, Income, Liability};

    let role = |id: Option<AccountId>, role: &str, allowed: &[crate::domain::AccountType]| {
        let id = id.ok_or_else(|| Error::Validation(format!("{role} account is required")))?;
        let account = get_account(conn, id)?;
        if !allowed.contains(&account.account_type) {
            return Err(Error::Validation(format!(
                "{role} account {} has the wrong type for this entry",
                account.code
            )));
        }
        Ok(id)
    };

    match input.kind {
        SimpleEntryKind::Expense => Ok((
            role(input.category_account_id, "category", &[Expense])?,
            role(input.wallet_account_id, "payment", &[Asset, Liability])?,
        )),
        SimpleEntryKind::Income => Ok((
            role(input.wallet_account_id, "deposit", &[Asset])?,
            role(input.category_account_id, "income", &[Income])?,
        )),
        SimpleEntryKind::Bill => match input.bill_status {
            Some(SimpleBillStatus::Paid) => Ok((
                role(input.category_account_id, "bill category", &[Expense])?,
                role(input.wallet_account_id, "payment", &[Asset, Liability])?,
            )),
            Some(SimpleBillStatus::Unpaid) => Ok((
                role(input.category_account_id, "bill category", &[Expense])?,
                role(input.payable_account_id, "bills payable", &[Liability])?,
            )),
            Some(SimpleBillStatus::PayExisting) => Ok((
                role(input.payable_account_id, "bills payable", &[Liability])?,
                role(input.wallet_account_id, "payment", &[Asset, Liability])?,
            )),
            None => Err(Error::Validation("bill entries need a bill status".into())),
        },
        SimpleEntryKind::Transfer => Ok((
            role(
                input.to_account_id,
                "transfer destination",
                &[Asset, Liability],
            )?,
            role(
                input.from_account_id,
                "transfer source",
                &[Asset, Liability],
            )?,
        )),
    }
}

/// Void a posted entry by posting a reverse entry and linking `voided_by`.
///
/// The reverse insert and both link updates happen in one transaction.
///
/// # Errors
///
/// Already voided, not found, or DB error.
pub fn void_entry(conn: &Connection, id: JournalEntryId) -> Result<VoidResult> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let result = void_entry_in_tx(&tx, id)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(result)
}

fn void_entry_in_tx(conn: &Connection, id: JournalEntryId) -> Result<VoidResult> {
    let view = get_entry(conn, id)?;
    if view.is_voided {
        return Err(Error::Validation("entry is already voided".into()));
    }
    if view.entry.status != EntryStatus::Posted {
        return Err(Error::Validation(
            "only posted entries can be voided".into(),
        ));
    }

    let reverse_lines: Vec<CreateJournalLine> = view
        .lines
        .iter()
        .map(|line| CreateJournalLine {
            account_id: line.account_id,
            debit_minor: line.credit.amount_minor(),
            credit_minor: line.debit.amount_minor(),
            memo: Some("Void".into()),
        })
        .collect();

    let reverse_input = PostJournal {
        entity_id: view.entry.entity_id,
        entry_date: format_date(view.entry.entry_date),
        description: format!("VOID: {}", view.entry.description),
        reference: view.entry.reference.clone(),
        lines: reverse_lines,
    };
    let reverse = insert_posted_entry(conn, &reverse_input)?;

    // Link original → reverse (original is voided).
    conn.execute(
        "UPDATE journal_entries SET voided_by_entry_id = ?1 WHERE id = ?2",
        rusqlite::params![reverse.entry.id.0.to_string(), id.0.to_string()],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    // Link reverse → original so the reversing entry is also out of active books
    // and hidden from the UI (`is_voided`). Without this, delete left a "VOID: …"
    // row visible in Transactions.
    conn.execute(
        "UPDATE journal_entries SET voided_by_entry_id = ?1 WHERE id = ?2",
        rusqlite::params![id.0.to_string(), reverse.entry.id.0.to_string()],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    Ok(VoidResult {
        original_id: id,
        reverse_id: reverse.entry.id,
    })
}

/// Account register with running balance (oldest first in range).
///
/// # Errors
///
/// Not found or DB error.
pub fn account_register(
    conn: &Connection,
    account_id: AccountId,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<RegisterLine>> {
    let account = get_account(conn, account_id)?;
    let from = from.map(parse_date).transpose()?.map(format_date);
    let to = to.map(parse_date).transpose()?.map(format_date);

    let mut running = if let Some(from) = from.as_deref() {
        let prior_sql = format!(
            "
            SELECT COALESCE(SUM(jl.debit_minor),0), COALESCE(SUM(jl.credit_minor),0)
            FROM journal_lines jl
            JOIN journal_entries je ON je.id = jl.entry_id
            WHERE jl.account_id = ?1
              AND je.status = 'posted'
              AND {ACTIVE_ENTRY_PREDICATE}
              AND je.entry_date < ?2
            "
        );
        let prior: (i64, i64) = conn
            .query_row(
                &prior_sql,
                rusqlite::params![account_id.0.to_string(), from],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        normal_balance(account.account_type, prior.0, prior.1)
    } else {
        0
    };

    let list_sql = format!(
        "
        SELECT je.id, je.entry_date, je.description,
               jl.debit_minor, jl.credit_minor
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        WHERE jl.account_id = ?1
          AND je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
          AND (?2 IS NULL OR je.entry_date >= ?2)
          AND (?3 IS NULL OR je.entry_date <= ?3)
        ORDER BY je.entry_date ASC, je.created_at ASC, jl.line_order ASC
        "
    );
    let mut stmt = conn
        .prepare(&list_sql)
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map(
            rusqlite::params![account_id.0.to_string(), from, to],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut result = Vec::new();
    for row in rows {
        let (id_s, date_s, description, debit, credit) =
            row.map_err(|err| Error::Io(err.to_string()))?;
        let entry_id = JournalEntryId(parse_uuid(&id_s)?);
        let entry_date = parse_date(&date_s)?;
        let delta = normal_balance(account.account_type, debit, credit);
        running = running.saturating_add(delta);
        result.push(RegisterLine {
            entry_id,
            entry_date,
            description,
            debit_minor: debit,
            credit_minor: credit,
            balance_minor: running,
        });
    }

    Ok(result)
}

fn load_lines(conn: &Connection, entry_id: JournalEntryId) -> Result<Vec<JournalLine>> {
    let mut stmt = conn
        .prepare(
            "
            SELECT id, entry_id, account_id, debit_minor, credit_minor, memo
            FROM journal_lines
            WHERE entry_id = ?1
            ORDER BY line_order
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map([entry_id.0.to_string()], |row| {
            let id = parse_uuid(&row.get::<_, String>(0)?).map_err(|e| row_err(&e))?;
            let eid = parse_uuid(&row.get::<_, String>(1)?).map_err(|e| row_err(&e))?;
            let aid = parse_uuid(&row.get::<_, String>(2)?).map_err(|e| row_err(&e))?;
            let debit = Money::from_minor(row.get(3)?).map_err(|e| row_err(&e))?;
            let credit = Money::from_minor(row.get(4)?).map_err(|e| row_err(&e))?;
            Ok(JournalLine {
                id: JournalLineId(id),
                entry_id: JournalEntryId(eid),
                account_id: AccountId(aid),
                debit,
                credit,
                memo: row.get(5)?,
            })
        })
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut lines = Vec::new();
    for row in rows {
        lines.push(row.map_err(|err| Error::Io(err.to_string()))?);
    }
    Ok(lines)
}

fn map_entry_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JournalEntry> {
    let id = parse_uuid(&row.get::<_, String>(0)?).map_err(|e| row_err(&e))?;
    let entity_id = parse_uuid(&row.get::<_, String>(1)?).map_err(|e| row_err(&e))?;
    let date_s: String = row.get(2)?;
    let entry_date = parse_date(&date_s).map_err(|e| row_err(&e))?;
    let status_s: String = row.get(5)?;
    let status = match status_s.as_str() {
        "posted" => EntryStatus::Posted,
        "draft" => EntryStatus::Draft,
        other => {
            let err = Error::VaultCorrupt(format!("unknown entry status: {other}"));
            return Err(row_err(&err));
        }
    };

    Ok(JournalEntry {
        id: JournalEntryId(id),
        entity_id: EntityId(entity_id),
        entry_date,
        description: row.get(3)?,
        reference: row.get(4)?,
        status,
    })
}

fn row_err(err: &Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        err.to_string(),
    )))
}
