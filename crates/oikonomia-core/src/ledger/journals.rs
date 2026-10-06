//! Journal entry posting, listing, void, and account register.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::db::{corrupt_column, fold_case, read_column, stored_date, stored_uuid};
use crate::domain::{
    Account, AccountId, EntityId, EntryStatus, JournalEntry, JournalEntryId, JournalLine,
    JournalLineId, validate_lines_for_post,
};
use crate::error::{AccountRole, Error, Result, ValidationError};
use crate::ledger::accounts::{get_account, list_accounts};
use crate::ledger::balance::{
    ACTIVE_ENTRY_PREDICATE, account_balance_as_of, add_minor, normal_balance,
};
use crate::money::Money;
use crate::prefs::Locale;
use crate::text::{opening_balance_description, void_description, void_memo};
use crate::util::{format_date, now_utc_string, parse_date};

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
    /// True if this entry is out of the active books: it has been voided, or
    /// it is the reversing entry that a void posted.
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
    /// Owner-only hidden flag from the parent entry.
    pub hidden: bool,
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
///
/// The text is case-folded here and the searched columns are folded in SQL
/// with `fold(...)`, so the match ignores case for every letter, not only
/// ASCII ones.
fn like_pattern(text: &str) -> String {
    let escaped = fold_case(text)
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// The entries [`list_entries`] returns, as a predicate on `journal_entries je`.
///
/// It binds `?1` entity, `?2` and `?3` the date bounds, `?4` the folded `LIKE`
/// pattern and `?5` an account, in the order of [`ListedEntries::bound`]. The
/// header query and the line query both use it, so they always agree on the
/// set and neither binds a value per entry.
const LISTED_ENTRIES_PREDICATE: &str = "
    je.entity_id = ?1
    AND je.status = 'posted'
    AND (?2 IS NULL OR je.entry_date >= ?2)
    AND (?3 IS NULL OR je.entry_date <= ?3)
    AND (?4 IS NULL
         OR fold(je.description) LIKE ?4 ESCAPE '\\'
         OR fold(je.reference) LIKE ?4 ESCAPE '\\'
         OR EXISTS (
             SELECT 1 FROM journal_lines memo_line
             WHERE memo_line.entry_id = je.id AND fold(memo_line.memo) LIKE ?4 ESCAPE '\\'
         ))
    AND (?5 IS NULL OR EXISTS (
         SELECT 1 FROM journal_lines account_line
         WHERE account_line.entry_id = je.id AND account_line.account_id = ?5
    ))
";

/// The values [`LISTED_ENTRIES_PREDICATE`] binds, normalized from an [`EntryFilter`].
struct ListedEntries {
    /// `?1`: the entity whose entries are listed.
    entity: String,
    /// `?2`: inclusive lower date bound as `YYYY-MM-DD`, if any.
    date_from: Option<String>,
    /// `?3`: inclusive upper date bound as `YYYY-MM-DD`, if any.
    date_to: Option<String>,
    /// `?4`: case-folded `LIKE` pattern for the text search, if any.
    pattern: Option<String>,
    /// `?5`: an account that a listed entry must have a line on, if any.
    account: Option<String>,
}

impl ListedEntries {
    fn new(entity_id: EntityId, filter: &EntryFilter) -> Result<Self> {
        // Normalize before binding: SQL compares date TEXT lexicographically, so a
        // lenient input like `2026-3-5` must become `2026-03-05` first.
        let normalized = |date: Option<&str>| -> Result<Option<String>> {
            Ok(date.map(parse_date).transpose()?.map(format_date))
        };

        Ok(Self {
            entity: entity_id.0.to_string(),
            date_from: normalized(filter.date_from.as_deref())?,
            date_to: normalized(filter.date_to.as_deref())?,
            pattern: filter
                .text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(like_pattern),
            account: filter.account_id.map(|id| id.0.to_string()),
        })
    }

    /// The bound values, `?1` to `?5`.
    fn bound(&self) -> [&dyn rusqlite::ToSql; 5] {
        [
            &self.entity,
            &self.date_from,
            &self.date_to,
            &self.pattern,
            &self.account,
        ]
    }
}

/// Lists posted entries for an entity (newest first) matching `filter`.
///
/// # Errors
///
/// [`Error::Validation`] for a malformed date in `filter`;
/// [`Error::VaultCorrupt`] for a stored id, date, status or amount that does
/// not parse; database errors as [`Error::Io`].
pub fn list_entries(
    conn: &Connection,
    entity_id: EntityId,
    filter: &EntryFilter,
) -> Result<Vec<PostedEntryView>> {
    let listed = ListedEntries::new(entity_id, filter)?;
    let headers = load_listed_headers(conn, &listed)?;
    let mut lines_by_entry = load_listed_lines(conn, &listed)?;

    Ok(headers
        .into_iter()
        .map(|(entry, is_voided)| PostedEntryView {
            lines: lines_by_entry.remove(&entry.id).unwrap_or_default(),
            entry,
            is_voided,
        })
        .collect())
}

/// Entry headers for [`list_entries`], newest first, each with its voided flag.
fn load_listed_headers(
    conn: &Connection,
    listed: &ListedEntries,
) -> Result<Vec<(JournalEntry, bool)>> {
    let sql = format!(
        "
        SELECT je.id, je.entity_id, je.entry_date, je.description, je.reference,
               je.status, je.hidden, je.voided_by_entry_id,
               je.voided_by_entry_id IS NOT NULL
                   OR EXISTS (
                       SELECT 1 FROM journal_entries x
                       WHERE x.voided_by_entry_id = je.id
                   ) AS is_voided
        FROM journal_entries je
        WHERE {LISTED_ENTRIES_PREDICATE}
        ORDER BY je.entry_date DESC, je.created_at DESC
        "
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map(listed.bound(), |row| {
            let is_voided: i64 = row.get(8)?;
            Ok((map_entry_row(row), is_voided != 0))
        })
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut headers = Vec::new();
    for row in rows {
        let (entry, is_voided) = row.map_err(|err| Error::Io(err.to_string()))?;
        headers.push((entry?, is_voided));
    }
    Ok(headers)
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
            SELECT id, entity_id, entry_date, description, reference, status, hidden,
                   voided_by_entry_id
            FROM journal_entries WHERE id = ?1
            ",
            [id.0.to_string()],
            |row| {
                let voided: Option<String> = row.get(7)?;
                Ok((map_entry_row(row), voided.is_some()))
            },
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound("journal entry".into()),
            other => Error::Io(other.to_string()),
        })?;
    let entry = entry?;

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

/// Sets the owner-only hidden flag on an existing journal entry.
///
/// Hidden keeps an entry out of what leaves the app: the journal CSV export
/// omits it, and so does the accountant profit and loss
/// ([`profit_and_loss_export`](crate::ledger::profit_and_loss_export)). It is
/// not extra encryption and not a second password. Inside the app the owner
/// still sees the row via [`list_entries`], [`get_entry`] and
/// [`account_register`], and every other report counts it.
///
/// v1: any existing entry the owner can load (draft or posted, including
/// voided) can be hidden or unhidden.
///
/// # Errors
///
/// [`Error::NotFound`] for an unknown entry; [`Error::VaultCorrupt`] for a
/// stored row that does not parse; database errors as [`Error::Io`].
pub fn set_entry_hidden(
    conn: &Connection,
    id: JournalEntryId,
    hidden: bool,
) -> Result<PostedEntryView> {
    let n = conn
        .execute(
            "UPDATE journal_entries SET hidden = ?1 WHERE id = ?2",
            rusqlite::params![i64::from(hidden), id.0.to_string()],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if n == 0 {
        return Err(Error::NotFound("journal entry".into()));
    }
    get_entry(conn, id)
}

/// Validates and posts a journal entry atomically.
///
/// # Errors
///
/// - [`ValidationError::InvalidDate`] for a malformed entry date.
/// - [`Error::NotFound`] for an unknown account.
/// - [`Error::AccountWrongEntity`] for an account of another entity.
/// - [`ValidationError::AccountInactive`] for an archived account.
/// - [`Error::NegativeMoney`] for a negative debit or credit.
/// - [`Error::TooFewLines`] for fewer than two lines.
/// - [`Error::InvalidLineAmounts`] for a line that is not debit XOR credit.
/// - [`Error::MoneyOverflow`] when the debits or the credits do not fit in
///   `i64`.
/// - [`Error::UnbalancedEntry`] when debits and credits differ.
/// - [`Error::VaultCorrupt`] for a stored row that does not parse.
/// - [`Error::Io`] on database errors.
pub fn post_entry(conn: &Connection, input: &PostJournal) -> Result<PostedEntryView> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let view = insert_posted_entry(&tx, input, false, ArchivedAccounts::Refuse)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(view)
}

/// Whether a new entry may post to an archived account.
#[derive(Debug, Clone, Copy)]
enum ArchivedAccounts {
    /// The rule for every entry the user writes.
    Refuse,
    /// Only for the reversing entry of a void: it must use the accounts of the
    /// entry it reverses, and archiving one of them later must not make that
    /// entry impossible to void or edit.
    Accept,
}

/// Insert header + lines without transaction management.
///
/// Callers own the transaction: [`post_entry`] and [`void_entry`] wrap this so
/// a failure mid-insert can never leave a partial posted entry behind.
/// `hidden` is for [`void_entry`]: a reverse of a hidden original stays hidden.
fn insert_posted_entry(
    conn: &Connection,
    input: &PostJournal,
    hidden: bool,
    archived: ArchivedAccounts,
) -> Result<PostedEntryView> {
    // Description may be empty (tray quick-add memo is optional); still trim.
    let description = input.description.trim();

    let entry_date = parse_date(&input.entry_date)?;
    let entry_id = JournalEntryId::new();

    let mut lines = Vec::with_capacity(input.lines.len());
    for raw in &input.lines {
        let account = get_account(conn, raw.account_id)?;
        ensure_in_book(&account, input.entity_id)?;
        match archived {
            ArchivedAccounts::Refuse => ensure_active(&account)?,
            ArchivedAccounts::Accept => {}
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
            status, created_at, posted_at, voided_by_entry_id, hidden
        ) VALUES (?1, ?2, ?3, ?4, ?5, 'posted', ?6, ?6, NULL, ?7)
        ",
        rusqlite::params![
            entry_id.0.to_string(),
            input.entity_id.0.to_string(),
            date_s,
            description,
            reference,
            now,
            i64::from(hidden),
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

/// Build and insert the simple-form entry without transaction management.
///
/// Callers own the transaction: [`post_simple_entry`] wraps this, and
/// `documents::post_simple_entry_with_document` composes it with the
/// document save inside one transaction.
pub(crate) fn post_simple_entry_unchecked(
    conn: &Connection,
    input: &PostSimpleEntry,
) -> Result<PostedEntryView> {
    post_simple_entry_unchecked_hidden(conn, input, false)
}

/// Like [`post_simple_entry_unchecked`], copying `hidden` onto the new row.
pub(crate) fn post_simple_entry_unchecked_hidden(
    conn: &Connection,
    input: &PostSimpleEntry,
    hidden: bool,
) -> Result<PostedEntryView> {
    if input.amount_minor <= 0 {
        return Err(Error::Validation(ValidationError::AmountNotPositive));
    }

    let (debit_account, credit_account) = simple_entry_sides(conn, input)?;

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

    insert_posted_entry(
        conn,
        &PostJournal {
            entity_id: input.entity_id,
            entry_date: input.entry_date.clone(),
            description: input.description.clone(),
            reference: input.reference.clone(),
            lines,
        },
        hidden,
        ArchivedAccounts::Refuse,
    )
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
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let view = post_simple_entry_unchecked(&tx, input)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(view)
}

/// Checks the role accounts of `input` exactly as [`post_simple_entry`] does.
///
/// Posting resolves its accounts through the same [`simple_entry_sides`], so
/// an input that passes here cannot be refused for its accounts at post time
/// unless an account changes in between.
///
/// # Errors
///
/// Those [`simple_entry_sides`] returns.
pub(crate) fn ensure_simple_entry_roles(conn: &Connection, input: &PostSimpleEntry) -> Result<()> {
    simple_entry_sides(conn, input).map(|_| ())
}

/// Resolves the (debit, credit) account pair for a simple entry.
///
/// # Errors
///
/// - [`ValidationError::AccountRequired`], [`ValidationError::AccountWrongType`]
///   or [`ValidationError::BillStatusRequired`] when a role the kind needs is
///   empty or filled with an account of the wrong type.
/// - [`ValidationError::SameAccount`] when both sides are one account.
/// - [`Error::AccountWrongEntity`] or [`ValidationError::AccountInactive`]
///   when an account belongs to another book or is archived.
/// - [`Error::NotFound`] for an unknown account id.
fn simple_entry_sides(
    conn: &Connection,
    input: &PostSimpleEntry,
) -> Result<(AccountId, AccountId)> {
    let (debit, credit) = simple_entry_role_accounts(conn, input)?;
    if debit.id == credit.id {
        return Err(Error::Validation(ValidationError::SameAccount));
    }

    for account in [&debit, &credit] {
        ensure_in_book(account, input.entity_id)?;
        ensure_active(account)?;
    }
    Ok((debit.id, credit.id))
}

/// Checks that `account` belongs to the book of `entity_id`.
fn ensure_in_book(account: &Account, entity_id: EntityId) -> Result<()> {
    if account.entity_id != entity_id {
        return Err(Error::AccountWrongEntity);
    }
    Ok(())
}

/// Checks that `account` is not archived.
fn ensure_active(account: &Account) -> Result<()> {
    if !account.is_active {
        return Err(Error::Validation(ValidationError::AccountInactive {
            code: account.code.clone(),
        }));
    }
    Ok(())
}

/// Loads the (debit, credit) accounts the kind of `input` maps its roles to,
/// checking only that each role is filled with an account of an allowed type.
fn simple_entry_role_accounts(
    conn: &Connection,
    input: &PostSimpleEntry,
) -> Result<(Account, Account)> {
    use crate::domain::AccountType::{Asset, Expense, Income, Liability};

    let role =
        |id: Option<AccountId>, role: AccountRole, allowed: &[crate::domain::AccountType]| {
            let id = id.ok_or(Error::Validation(ValidationError::AccountRequired { role }))?;
            let account = get_account(conn, id)?;
            if !allowed.contains(&account.account_type) {
                return Err(Error::Validation(ValidationError::AccountWrongType {
                    role,
                    code: account.code.clone(),
                }));
            }
            Ok(account)
        };

    match input.kind {
        SimpleEntryKind::Expense => Ok((
            role(input.category_account_id, AccountRole::Category, &[Expense])?,
            role(
                input.wallet_account_id,
                AccountRole::Payment,
                &[Asset, Liability],
            )?,
        )),
        SimpleEntryKind::Income => Ok((
            role(input.wallet_account_id, AccountRole::Deposit, &[Asset])?,
            role(input.category_account_id, AccountRole::Income, &[Income])?,
        )),
        SimpleEntryKind::Bill => match input.bill_status {
            Some(SimpleBillStatus::Paid) => Ok((
                role(
                    input.category_account_id,
                    AccountRole::BillCategory,
                    &[Expense],
                )?,
                role(
                    input.wallet_account_id,
                    AccountRole::Payment,
                    &[Asset, Liability],
                )?,
            )),
            Some(SimpleBillStatus::Unpaid) => Ok((
                role(
                    input.category_account_id,
                    AccountRole::BillCategory,
                    &[Expense],
                )?,
                role(
                    input.payable_account_id,
                    AccountRole::BillsPayable,
                    &[Liability],
                )?,
            )),
            Some(SimpleBillStatus::PayExisting) => Ok((
                role(
                    input.payable_account_id,
                    AccountRole::BillsPayable,
                    &[Liability],
                )?,
                role(
                    input.wallet_account_id,
                    AccountRole::Payment,
                    &[Asset, Liability],
                )?,
            )),
            None => Err(Error::Validation(ValidationError::BillStatusRequired)),
        },
        SimpleEntryKind::Transfer => Ok((
            role(
                input.to_account_id,
                AccountRole::TransferDestination,
                &[Asset, Liability],
            )?,
            role(
                input.from_account_id,
                AccountRole::TransferSource,
                &[Asset, Liability],
            )?,
        )),
    }
}

/// Void a posted entry by posting a reverse entry and linking `voided_by`.
///
/// The reverse entry's description and memo are written in `locale`.
///
/// The reverse insert and both link updates happen in one transaction.
/// If the original is hidden, the reverse `VOID:` row inherits that flag so
/// journal CSV omits both. A visible void still exports the original and the
/// reverse.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entry.
/// - [`ValidationError::EntryAlreadyVoided`] when the entry is voided or is
///   itself a reversing entry.
/// - [`ValidationError::EntryNotPosted`] when the entry is a draft.
/// - [`Error::VaultCorrupt`] for a stored row that does not parse.
/// - [`Error::Io`] on database errors.
///
/// An archived account is not an error here: the reversing entry posts to the
/// original's accounts even when one of them has since been archived, so that
/// archiving an account never makes its entries impossible to void.
pub fn void_entry(conn: &Connection, id: JournalEntryId, locale: Locale) -> Result<VoidResult> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let result = void_entry_in_tx(&tx, id, locale)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(result)
}

fn void_entry_in_tx(conn: &Connection, id: JournalEntryId, locale: Locale) -> Result<VoidResult> {
    let view = get_entry(conn, id)?;
    if view.is_voided {
        return Err(Error::Validation(ValidationError::EntryAlreadyVoided));
    }
    if view.entry.status != EntryStatus::Posted {
        return Err(Error::Validation(ValidationError::EntryNotPosted));
    }

    let reverse_lines: Vec<CreateJournalLine> = view
        .lines
        .iter()
        .map(|line| CreateJournalLine {
            account_id: line.account_id,
            debit_minor: line.credit.amount_minor(),
            credit_minor: line.debit.amount_minor(),
            memo: Some(void_memo(locale).into()),
        })
        .collect();

    let reverse_input = PostJournal {
        entity_id: view.entry.entity_id,
        entry_date: format_date(view.entry.entry_date),
        description: void_description(locale, &view.entry.description),
        reference: view.entry.reference.clone(),
        lines: reverse_lines,
    };
    let reverse = insert_posted_entry(
        conn,
        &reverse_input,
        view.entry.hidden,
        ArchivedAccounts::Accept,
    )?;

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

/// Correct a posted entry: void the original and post the replacement in one
/// transaction, moving any attached documents to the replacement.
///
/// Posted entries stay immutable — an edit is a void plus repost so the audit
/// trail survives. The UI hides voided pairs, so this reads as an in-place edit.
/// A hidden original yields a hidden replacement (and a hidden VOID reverse)
/// so journal CSV omits the whole edit. A visible original stays visible.
/// The reversing entry's description and memo are written in `locale`.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entry.
/// - [`ValidationError::WrongBook`] when `input` names another entity than
///   the original's.
/// - The errors of [`void_entry`] for the original.
/// - The errors of [`post_simple_entry`] for the replacement, including
///   [`ValidationError::AccountInactive`]: the reversal may post to an
///   archived account of the original, but the replacement is a new entry and
///   may not.
///
/// On any error nothing is changed; the void is rolled back with the post.
pub fn replace_simple_entry(
    conn: &Connection,
    original_id: JournalEntryId,
    input: &PostSimpleEntry,
    locale: Locale,
) -> Result<PostedEntryView> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;

    let original = get_entry(&tx, original_id)?;
    if original.entry.entity_id != input.entity_id {
        return Err(Error::Validation(ValidationError::WrongBook));
    }

    void_entry_in_tx(&tx, original_id, locale)?;
    let replacement = post_simple_entry_unchecked_hidden(&tx, input, original.entry.hidden)?;

    tx.execute(
        "UPDATE documents SET entry_id = ?1 WHERE entry_id = ?2",
        rusqlite::params![
            replacement.entry.id.0.to_string(),
            original_id.0.to_string(),
        ],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(replacement)
}

/// Set an asset or liability account's balance as of a date by posting the
/// difference against the entity's Opening Balances equity account.
///
/// The user states what the account actually holds; the gap between that and
/// the ledger becomes one adjustment entry, so repeating the call converges on
/// the stated balance instead of stacking duplicates. The entry's description
/// is written in `locale`.
///
/// # Errors
///
/// Wrong account type, inactive account, no equity account to post against,
/// a target equal to the current balance, or DB errors.
pub fn set_account_opening_balance(
    conn: &Connection,
    account_id: AccountId,
    target_minor: i64,
    as_of: &str,
    locale: Locale,
) -> Result<PostedEntryView> {
    use crate::domain::AccountType;

    let account = get_account(conn, account_id)?;
    if account.account_type != AccountType::Asset && account.account_type != AccountType::Liability
    {
        return Err(Error::Validation(
            ValidationError::OpeningBalanceAccountType,
        ));
    }
    if !account.is_active {
        return Err(Error::Validation(ValidationError::AccountInactive {
            code: account.code.clone(),
        }));
    }

    let as_of_d = parse_date(as_of)?;
    let current = account_balance_as_of(conn, account_id, account.account_type, as_of_d)?;
    let delta = target_minor
        .checked_sub(current)
        .ok_or(Error::MoneyOverflow)?;
    if delta == 0 {
        return Err(Error::Validation(ValidationError::OpeningBalanceUnchanged));
    }

    // Prefer the system Opening Balances account; fall back to any active
    // equity account so hand-built (blank template) charts still work.
    let accounts = list_accounts(conn, account.entity_id)?;
    let equity = accounts
        .iter()
        .find(|a| a.account_type == AccountType::Equity && a.is_system && a.is_active)
        .or_else(|| {
            accounts
                .iter()
                .find(|a| a.account_type == AccountType::Equity && a.is_active)
        })
        .ok_or(Error::Validation(ValidationError::NoEquityAccount))?;

    // A debit-normal account grows by debiting: a positive delta debits the
    // account and credits equity; every other combination flips the sides.
    let amount = delta.checked_abs().ok_or(Error::MoneyOverflow)?;
    let account_on_debit_side = account.account_type.is_debit_normal() == (delta > 0);
    let (debit_id, credit_id) = if account_on_debit_side {
        (account.id, equity.id)
    } else {
        (equity.id, account.id)
    };

    post_entry(
        conn,
        &PostJournal {
            entity_id: account.entity_id,
            entry_date: format_date(as_of_d),
            description: opening_balance_description(locale, &account.name),
            reference: None,
            lines: vec![
                CreateJournalLine {
                    account_id: debit_id,
                    debit_minor: amount,
                    credit_minor: 0,
                    memo: None,
                },
                CreateJournalLine {
                    account_id: credit_id,
                    debit_minor: 0,
                    credit_minor: amount,
                    memo: None,
                },
            ],
        },
    )
}

/// Account register with running balance (oldest first in range).
///
/// # Errors
///
/// [`Error::NotFound`] for an unknown account; [`Error::Validation`] for a
/// malformed date; [`Error::MoneyOverflow`] when the running balance does not
/// fit in `i64`; [`Error::VaultCorrupt`] for a stored id or date that does not
/// parse; database errors as [`Error::Io`].
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
        normal_balance(account.account_type, prior.0, prior.1)?
    } else {
        0
    };

    let list_sql = format!(
        "
        SELECT je.id, je.entry_date, je.description,
               jl.debit_minor, jl.credit_minor, je.hidden
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        WHERE jl.account_id = ?1
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
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut result = Vec::new();
    for row in rows {
        let (id_s, date_s, description, debit, credit, hidden) =
            row.map_err(|err| Error::Io(err.to_string()))?;
        let entry_id = JournalEntryId(stored_uuid("journal_entries.id", &id_s)?);
        let entry_date = stored_date("journal_entries.entry_date", &date_s)?;
        let delta = normal_balance(account.account_type, debit, credit)?;
        running = add_minor(running, delta)?;
        result.push(RegisterLine {
            entry_id,
            entry_date,
            description,
            debit_minor: debit,
            credit_minor: credit,
            balance_minor: running,
            hidden: hidden != 0,
        });
    }

    Ok(result)
}

/// Lines of every entry [`list_entries`] returns, grouped by entry and in line
/// order within each.
///
/// The entries are selected by joining on [`LISTED_ENTRIES_PREDICATE`] rather
/// than by an `IN` list of their ids: a list binds one variable per entry, and
/// `SQLite` refuses a statement with more than `SQLITE_MAX_VARIABLE_NUMBER`
/// of them (32766 in the bundled build), which a large book exceeds.
fn load_listed_lines(
    conn: &Connection,
    listed: &ListedEntries,
) -> Result<HashMap<JournalEntryId, Vec<JournalLine>>> {
    let sql = format!(
        "
        SELECT jl.id, jl.entry_id, jl.account_id, jl.debit_minor, jl.credit_minor, jl.memo
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        WHERE {LISTED_ENTRIES_PREDICATE}
        ORDER BY jl.entry_id, jl.line_order
        "
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| Error::Io(err.to_string()))?;
    let rows = stmt
        .query_map(listed.bound(), |row| Ok(map_line_row(row)))
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut grouped: HashMap<JournalEntryId, Vec<JournalLine>> = HashMap::new();
    for row in rows {
        let line = row.map_err(|err| Error::Io(err.to_string()))??;
        grouped.entry(line.entry_id).or_default().push(line);
    }
    Ok(grouped)
}

/// Maps a row selected as `id, entry_id, account_id, debit_minor,
/// credit_minor, memo`.
fn map_line_row(row: &rusqlite::Row<'_>) -> Result<JournalLine> {
    let id = stored_uuid("journal_lines.id", &read_column::<String>(row, 0)?)?;
    let entry_id = stored_uuid("journal_lines.entry_id", &read_column::<String>(row, 1)?)?;
    let account_id = stored_uuid("journal_lines.account_id", &read_column::<String>(row, 2)?)?;

    Ok(JournalLine {
        id: JournalLineId(id),
        entry_id: JournalEntryId(entry_id),
        account_id: AccountId(account_id),
        debit: stored_amount("journal_lines.debit_minor", read_column(row, 3)?)?,
        credit: stored_amount("journal_lines.credit_minor", read_column(row, 4)?)?,
        memo: read_column(row, 5)?,
    })
}

/// A line amount as stored; the schema's CHECK keeps it non-negative.
fn stored_amount(column: &str, minor: i64) -> Result<Money> {
    Money::from_minor(minor)
        .map_err(|_| corrupt_column(column, format_args!("negative amount: {minor}")))
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
        .query_map([entry_id.0.to_string()], |row| Ok(map_line_row(row)))
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut lines = Vec::new();
    for row in rows {
        lines.push(row.map_err(|err| Error::Io(err.to_string()))??);
    }
    Ok(lines)
}

/// Maps a row whose first seven columns are `id, entity_id, entry_date,
/// description, reference, status, hidden`.
fn map_entry_row(row: &rusqlite::Row<'_>) -> Result<JournalEntry> {
    let id = stored_uuid("journal_entries.id", &read_column::<String>(row, 0)?)?;
    let entity_id = stored_uuid("journal_entries.entity_id", &read_column::<String>(row, 1)?)?;
    let entry_date = stored_date(
        "journal_entries.entry_date",
        &read_column::<String>(row, 2)?,
    )?;
    let status = match read_column::<String>(row, 5)?.as_str() {
        "posted" => EntryStatus::Posted,
        "draft" => EntryStatus::Draft,
        other => {
            return Err(corrupt_column(
                "journal_entries.status",
                format_args!("unknown entry status: {other}"),
            ));
        }
    };

    Ok(JournalEntry {
        id: JournalEntryId(id),
        entity_id: EntityId(entity_id),
        entry_date,
        description: read_column(row, 3)?,
        reference: read_column(row, 4)?,
        status,
        hidden: read_column::<i64>(row, 6)? != 0,
    })
}
