//! The journal: posting entries, listing them, voiding and correcting them,
//! and the register of one account.
//!
//! # Posting
//!
//! An entry is a header and at least two lines. Each line puts a positive
//! amount on one [`Side`] of one account, and the debits of an entry equal
//! its credits. A line cannot be on both sides: [`PostJournalLine`] and
//! [`JournalLine`] hold one amount and one side. Every entry is written by the private
//! `post_entry_in_tx`, which checks those rules
//! ([`validate_lines_for_post`]) and that each account belongs to the entry's
//! entity; no other code inserts into the journal. The schema repeats the
//! rule for a single line as a `CHECK`.
//!
//! The simple entry form posts through [`post_simple_entry`]. Its input names
//! accounts by the part they play, as a [`SimpleEntryAccounts`], and that
//! type holds the mapping from a kind of entry to a debit and a credit, so
//! the UI holds no accounting rule. What needs the database is checked here:
//! that each account exists, has a type its part accepts, belongs to the
//! entry's entity, is not archived, and is not the account on the other
//! side.
//!
//! # Entries are not edited
//!
//! Once posted, an entry's date, description, amounts and accounts never
//! change. Two things about it can: the hidden flag
//! ([`set_entry_hidden`]) and the void link. A correction is a void followed
//! by a new entry ([`replace_simple_entry`]), so what was posted before stays
//! on record. An entry is removed only when its whole entity is deleted.
//!
//! # Voiding
//!
//! [`void_entry`] posts a reversing entry, with every debit and credit
//! swapped, and links the two through `voided_by_entry_id` in both
//! directions. Either link takes an entry out of the active books; the views
//! here report that as [`PostedEntryView::is_voided`], for the original and
//! for its reversal alike. Nothing is deleted.
//!
//! # Order
//!
//! Entries are ordered by their date, then by when they were created, then
//! by `rowid`. Creation time is stored to the second, and an import posts
//! many entries of one date within a second, so the first two do not settle
//! the order on their own. `journal_entries` has a text primary key and so
//! keeps `SQLite`'s own `rowid`, which for a new row is one more than the
//! largest in the table (<https://www.sqlite.org/autoinc.html>): among the
//! entries of a book it rises in the order they were posted. `VACUUM` is the
//! one statement that renumbers such rowids, and nothing here runs it.
//! [`list_entries`] reads that order backwards, the register and the journal
//! export forwards.
//!
//! # Transactions
//!
//! A public function that writes more than one row opens a transaction and
//! commits it. The work itself is in a helper that takes the connection and
//! leaves the transaction to its caller, named `_in_tx` when private and
//! `_unchecked` when it is `pub(crate)` for another module to compose with
//! writes of its own.

use crate::db::{collect_rows, corrupt_column, fold_case, read_column, stored_date, stored_id};
use crate::domain::{
    Account, AccountId, AccountType, EntityId, EntryStatus, JournalEntry, JournalEntryId,
    JournalLine, JournalLineId, Side, validate_lines_for_post,
};
use crate::error::{DatabaseContext, Error, Resource, Result, ValidationError};
use crate::ledger::accounts::{get_account, list_accounts};
use crate::ledger::balance::{
    ACTIVE_ENTRY_PREDICATE, account_balance_as_of, add_minor, normal_balance,
};
use crate::ledger::simple_entry::{RoleAccount, SimpleEntryAccounts};
use crate::money::Money;
use crate::prefs::Locale;
use crate::text::{opening_balance_description, void_description, void_memo};
use crate::util::{format_date, now_utc_string};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::Date;

/// One line of a [`PostJournal`]: an amount on one side of one account.
///
/// The UI sends it in two columns, as a
/// [`JournalLineRequest`](crate::ledger::JournalLineRequest), which converts
/// into this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostJournalLine {
    /// Account the line posts to. It must belong to the entry's entity and
    /// not be archived.
    pub account_id: AccountId,
    /// Amount debited or credited; it must be greater than zero.
    pub amount: Money,
    /// Whether the amount is debited or credited.
    pub side: Side,
    /// Note on the line, stored as given. `None` stores none.
    pub memo: Option<String>,
}

impl PostJournalLine {
    /// Returns a line that debits `account_id` by `amount`, with no memo.
    #[must_use]
    pub const fn debit(account_id: AccountId, amount: Money) -> Self {
        Self {
            account_id,
            amount,
            side: Side::Debit,
            memo: None,
        }
    }

    /// Returns a line that credits `account_id` by `amount`, with no memo.
    #[must_use]
    pub const fn credit(account_id: AccountId, amount: Money) -> Self {
        Self {
            account_id,
            amount,
            side: Side::Credit,
            memo: None,
        }
    }
}

/// Input for [`post_entry`]: a whole entry, with its lines spelled out.
///
/// The UI sends it as a
/// [`PostJournalRequest`](crate::ledger::PostJournalRequest), which converts
/// into this.
#[derive(Debug, Clone)]
pub struct PostJournal {
    /// Entity whose books the entry goes into.
    pub entity_id: EntityId,
    /// Accounting date.
    pub entry_date: Date,
    /// What the entry is for. Surrounding whitespace is trimmed; it may be
    /// empty.
    pub description: String,
    /// The user's own reference for the entry, such as an invoice number.
    /// Surrounding whitespace is trimmed and a blank one is stored as none.
    pub reference: Option<String>,
    /// At least two lines, whose debits add up to their credits.
    pub lines: Vec<PostJournalLine>,
}

/// An entry as the UI shows it: its header, its lines and whether it counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostedEntryView {
    /// The entry itself: date, description, reference, status and hidden
    /// flag.
    pub entry: JournalEntry,
    /// The entry's lines, in the order they were posted.
    pub lines: Vec<JournalLine>,
    /// True if this entry is out of the active books: it has been voided, or
    /// it is the reversing entry that a void posted.
    pub is_voided: bool,
}

/// One line of [`account_register`]: a posting to the account, with the
/// balance after it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterLine {
    /// Entry the line belongs to.
    pub entry_id: JournalEntryId,
    /// Accounting date of the entry.
    #[serde(with = "crate::util::serde_date")]
    pub entry_date: Date,
    /// Description of the entry.
    pub description: String,
    /// Amount the line debits the account, in minor units; 0 on a credit.
    pub debit_minor: i64,
    /// Amount the line credits the account, in minor units; 0 on a debit.
    pub credit_minor: i64,
    /// Balance of the account after this line, in minor units, signed towards
    /// the normal side of the account's type. It counts every active entry up
    /// to this line, those before the register's first day included.
    pub balance_minor: i64,
    /// Whether the line's entry is hidden.
    pub hidden: bool,
}

/// Input for [`post_simple_entry`]: one amount and the two accounts it moves
/// between.
///
/// [`SimpleEntryAccounts`] says which accounts those are and which is
/// debited, so this cannot describe an entry with an account missing or with
/// a bill that does not say whether it is paid.
///
/// The UI sends it as a
/// [`PostSimpleEntryRequest`](crate::ledger::PostSimpleEntryRequest), which
/// converts into this.
#[derive(Debug, Clone)]
pub struct PostSimpleEntry {
    /// Entity whose books the entry goes into.
    pub entity_id: EntityId,
    /// The kind of entry and the accounts it debits and credits.
    pub accounts: SimpleEntryAccounts,
    /// Accounting date.
    pub entry_date: Date,
    /// What the entry is for. Surrounding whitespace is trimmed; it may be
    /// empty.
    pub description: String,
    /// The user's own reference for the entry. Surrounding whitespace is
    /// trimmed and a blank one is stored as none.
    pub reference: Option<String>,
    /// Positive amount in minor units.
    pub amount_minor: i64,
}

/// Result of [`void_entry`]: the two entries the void linked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoidResult {
    /// The entry that was voided.
    pub original_id: JournalEntryId,
    /// The reversing entry the void posted.
    pub reverse_id: JournalEntryId,
}

/// Optional predicates for [`list_entries`]; every `None` means "no filter".
#[derive(Debug, Clone, Default)]
pub struct EntryFilter {
    /// Case-insensitive substring over description, reference, and line
    /// memos. Text that is blank after trimming is no filter.
    pub text: Option<String>,
    /// Only entries dated on or after this day.
    pub date_from: Option<Date>,
    /// Only entries dated on or before this day.
    pub date_to: Option<Date>,
    /// Only entries with at least one line on this account.
    pub account_id: Option<AccountId>,
}

/// Lists the posted entries of an entity that match `filter`, newest first.
///
/// Voided entries and their reversals are listed too, marked by
/// [`PostedEntryView::is_voided`]. Entries of one date are ordered by when
/// they were created, latest first, and entries created within one second by
/// the order they were posted in, last first.
/// An entity that does not exist has no entries and gives an empty list.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] for a stored id, date, status or amount that does
/// not parse; database errors as [`Error::Database`].
pub fn list_entries(
    conn: &Connection,
    entity_id: EntityId,
    filter: &EntryFilter,
) -> Result<Vec<PostedEntryView>> {
    let listed = ListedEntries::new(entity_id, filter);
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

/// Returns one entry with its lines, whatever its status.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entry.
/// - [`Error::VaultCorrupt`] for a stored id, date, status or amount that
///   does not parse.
/// - [`Error::Database`] on database errors.
pub fn get_entry(conn: &Connection, id: JournalEntryId) -> Result<PostedEntryView> {
    let (entry, has_void_link) = conn
        .query_row(
            "
            SELECT id, entity_id, entry_date, description, reference, status, hidden,
                   voided_by_entry_id
            FROM journal_entries WHERE id = ?1
            ",
            [id.to_string()],
            |row| {
                let voided_by: Option<String> = row.get(7)?;
                Ok((map_entry_row(row), voided_by.is_some()))
            },
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::JournalEntry),
            other => Error::database("read journal entry", other),
        })?;
    let entry = entry?;

    let is_voided = has_void_link || entry_is_void_reverse(conn, entry.id)?;
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
/// Any entry can be hidden or shown again, whatever its status, a voided one
/// included.
///
/// # Errors
///
/// [`Error::NotFound`] for an unknown entry; [`Error::VaultCorrupt`] for a
/// stored row that does not parse; database errors as [`Error::Database`].
pub fn set_entry_hidden(
    conn: &Connection,
    id: JournalEntryId,
    hidden: bool,
) -> Result<PostedEntryView> {
    let updated = conn
        .execute(
            "UPDATE journal_entries SET hidden = ?1 WHERE id = ?2",
            rusqlite::params![i64::from(hidden), id.to_string()],
        )
        .database("set journal entry visibility")?;
    if updated == 0 {
        return Err(Error::NotFound(Resource::JournalEntry));
    }
    get_entry(conn, id)
}

/// Validates and posts a journal entry atomically.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown account.
/// - [`Error::AccountWrongEntity`] for an account of another entity.
/// - [`ValidationError::AccountInactive`] for an archived account.
/// - [`Error::TooFewLines`] for fewer than two lines.
/// - [`Error::InvalidLineAmounts`] for a line whose amount is zero.
/// - [`Error::MoneyOverflow`] when the debits or the credits do not fit in
///   `i64`.
/// - [`Error::UnbalancedEntry`] when debits and credits differ.
/// - [`Error::VaultCorrupt`] for a stored row that does not parse.
/// - [`Error::Database`] on database errors.
pub fn post_entry(conn: &Connection, input: &PostJournal) -> Result<PostedEntryView> {
    let tx = conn
        .unchecked_transaction()
        .database("begin journal entry post")?;
    let view = post_entry_in_tx(&tx, input, false, ArchivedAccounts::Refuse)?;
    tx.commit().database("commit journal entry post")?;
    Ok(view)
}

/// Builds and posts the journal entry for a simple-form input, atomically.
///
/// The kind → debit/credit mapping lives in [`SimpleEntryAccounts`] so the
/// UI never carries accounting rules; each account's type is checked here
/// before posting.
///
/// # Errors
///
/// - [`ValidationError::AmountNotPositive`] for an amount of zero or less.
/// - [`ValidationError::AccountWrongType`] when a role holds an account of a
///   type the kind does not allow there.
/// - [`ValidationError::SameAccount`] when both sides are one account.
/// - [`Error::NotFound`] for an unknown account.
/// - [`Error::AccountWrongEntity`] for an account of another entity.
/// - [`ValidationError::AccountInactive`] for an archived account.
/// - [`Error::VaultCorrupt`] for a stored row that does not parse.
/// - [`Error::Database`] on database errors.
pub fn post_simple_entry(conn: &Connection, input: &PostSimpleEntry) -> Result<PostedEntryView> {
    let tx = conn
        .unchecked_transaction()
        .database("begin journal entry post")?;
    let view = post_simple_entry_unchecked(&tx, input)?;
    tx.commit().database("commit journal entry post")?;
    Ok(view)
}

/// Voids a posted entry by posting a reversing entry and linking the two.
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
/// - [`Error::Database`] on database errors.
///
/// An archived account is not an error here: the reversing entry posts to the
/// original's accounts even when one of them has since been archived, so that
/// archiving an account never makes its entries impossible to void.
pub fn void_entry(conn: &Connection, id: JournalEntryId, locale: Locale) -> Result<VoidResult> {
    let tx = conn
        .unchecked_transaction()
        .database("begin journal entry void")?;
    let result = void_entry_in_tx(&tx, id, locale)?;
    tx.commit().database("commit journal entry void")?;
    Ok(result)
}

/// Corrects a posted entry: voids the original and posts the replacement in
/// one transaction, moving any attached documents to the replacement.
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
        .database("begin journal entry replacement")?;

    let original = get_entry(&tx, original_id)?;
    if original.entry.entity_id != input.entity_id {
        return Err(ValidationError::WrongBook.into());
    }

    void_entry_in_tx(&tx, original_id, locale)?;
    let replacement = post_simple_entry_unchecked_hidden(&tx, input, original.entry.hidden)?;

    tx.execute(
        "UPDATE documents SET entry_id = ?1 WHERE entry_id = ?2",
        rusqlite::params![replacement.entry.id.to_string(), original_id.to_string(),],
    )
    .database("move documents to replacement entry")?;

    tx.commit().database("commit journal entry replacement")?;
    Ok(replacement)
}

/// Sets an asset or liability account's balance as of a date by posting the
/// difference against the entity's Opening Balances equity account.
///
/// The user states what the account actually holds; the gap between that and
/// the ledger becomes one adjustment entry, so repeating the call converges on
/// the stated balance instead of stacking duplicates. The entry's description
/// is written in `locale`.
///
/// `target_minor` is in minor units and signed like the account's balance.
/// The contra account is the entity's active system equity account, or any
/// other active equity account when the chart has no such system account.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown account.
/// - [`ValidationError::OpeningBalanceAccountType`] unless the account is an
///   asset or a liability.
/// - [`ValidationError::AccountInactive`] for an archived account.
/// - [`ValidationError::OpeningBalanceUnchanged`] when the account already
///   has that balance on that date.
/// - [`ValidationError::NoEquityAccount`] when the entity has no active
///   equity account to post against.
/// - [`Error::MoneyOverflow`] when the difference between the target and the
///   current balance does not fit in `i64`.
/// - [`Error::VaultCorrupt`] for a stored row that does not parse.
/// - [`Error::Database`] on database errors.
pub fn set_account_opening_balance(
    conn: &Connection,
    account_id: AccountId,
    target_minor: i64,
    as_of: Date,
    locale: Locale,
) -> Result<PostedEntryView> {
    let account = get_account(conn, account_id)?;
    if account.account_type != AccountType::Asset && account.account_type != AccountType::Liability
    {
        return Err(ValidationError::OpeningBalanceAccountType.into());
    }
    ensure_active(&account)?;

    let current = account_balance_as_of(conn, account_id, account.account_type, as_of)?;
    let delta = target_minor
        .checked_sub(current)
        .ok_or(Error::MoneyOverflow)?;
    if delta == 0 {
        return Err(ValidationError::OpeningBalanceUnchanged.into());
    }

    // Prefer the system Opening Balances account; fall back to any active
    // equity account so hand-built (blank template) charts still work.
    let accounts = list_accounts(conn, account.entity_id)?;
    let is_active_equity =
        |candidate: &&Account| candidate.account_type == AccountType::Equity && candidate.is_active;
    let equity = accounts
        .iter()
        .find(|candidate| is_active_equity(candidate) && candidate.is_system)
        .or_else(|| accounts.iter().find(is_active_equity))
        .ok_or(ValidationError::NoEquityAccount)?;

    // A debit-normal account grows by debiting: a positive delta debits the
    // account and credits equity; every other combination flips the sides.
    let amount = Money::from_minor(delta.checked_abs().ok_or(Error::MoneyOverflow)?)?;
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
            entry_date: as_of,
            description: opening_balance_description(locale, &account.name),
            reference: None,
            lines: vec![
                PostJournalLine::debit(debit_id, amount),
                PostJournalLine::credit(credit_id, amount),
            ],
        },
    )
}

/// Lists the lines of active entries on an account between `from` and `to`
/// inclusive, oldest first, each with the running balance.
///
/// `None` leaves that end of the range open. Lines of one date are ordered by
/// when their entries were created, entries created within one second by the
/// order they were posted in, and lines of one entry by their order in it.
/// The running balance starts from
/// the account's balance on the day before `from`, so the first line's
/// balance is the account's true balance and not just the sum of the lines
/// shown.
///
/// # Errors
///
/// [`Error::NotFound`] for an unknown account; [`Error::MoneyOverflow`] when
/// the running balance does not fit in `i64`; [`Error::VaultCorrupt`] for a
/// stored id or date that does not
/// parse; database errors as [`Error::Database`].
pub fn account_register(
    conn: &Connection,
    account_id: AccountId,
    from: Option<Date>,
    to: Option<Date>,
) -> Result<Vec<RegisterLine>> {
    let account = get_account(conn, account_id)?;
    let from = from.map(format_date);
    let to = to.map(format_date);

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
        let (prior_debits, prior_credits): (i64, i64) = conn
            .query_row(
                &prior_sql,
                rusqlite::params![account_id.to_string(), from],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .database("sum account activity before period")?;
        normal_balance(account.account_type, prior_debits, prior_credits)?
    } else {
        0
    };

    let mut stmt = conn
        .prepare(&register_sql())
        .database("read account register")?;

    let rows = stmt
        .query_map(rusqlite::params![account_id.to_string(), from, to], |row| {
            Ok(map_register_row(row))
        })
        .database("read account register")?;

    let mut register = collect_rows("read account register", rows)?;
    for line in &mut register {
        let change = normal_balance(account.account_type, line.debit_minor, line.credit_minor)?;
        running = add_minor(running, change)?;
        line.balance_minor = running;
    }

    Ok(register)
}

/// Builds and inserts the simple-form entry without transaction management.
///
/// Callers own the transaction: [`post_simple_entry`] wraps this, and
/// `documents::post_simple_entry_with_document` composes it with the
/// document save inside one transaction.
///
/// # Errors
///
/// Those of [`post_simple_entry`].
pub(crate) fn post_simple_entry_unchecked(
    conn: &Connection,
    input: &PostSimpleEntry,
) -> Result<PostedEntryView> {
    post_simple_entry_unchecked_hidden(conn, input, false)
}

/// Builds and inserts the simple-form entry with the given hidden flag,
/// without transaction management.
///
/// [`replace_simple_entry`] passes the flag of the entry it replaces, so that
/// correcting a hidden entry does not bring it back into the exports.
///
/// # Errors
///
/// Those of [`post_simple_entry`].
pub(crate) fn post_simple_entry_unchecked_hidden(
    conn: &Connection,
    input: &PostSimpleEntry,
    hidden: bool,
) -> Result<PostedEntryView> {
    if input.amount_minor <= 0 {
        return Err(ValidationError::AmountNotPositive.into());
    }

    let (debit_account, credit_account) =
        simple_entry_sides(conn, input.entity_id, input.accounts)?;

    let amount = Money::from_minor(input.amount_minor)?;
    let lines = vec![
        PostJournalLine::debit(debit_account, amount),
        PostJournalLine::credit(credit_account, amount),
    ];

    post_entry_in_tx(
        conn,
        &PostJournal {
            entity_id: input.entity_id,
            entry_date: input.entry_date,
            description: input.description.clone(),
            reference: input.reference.clone(),
            lines,
        },
        hidden,
        ArchivedAccounts::Refuse,
    )
}

/// Checks `accounts` for an entry in the book of `entity_id` exactly as
/// [`post_simple_entry`] does.
///
/// Posting resolves its accounts through the same [`simple_entry_sides`], so
/// accounts that pass here cannot be refused at post time unless one of them
/// changes in between.
///
/// # Errors
///
/// Those [`simple_entry_sides`] returns.
pub(crate) fn ensure_simple_entry_accounts(
    conn: &Connection,
    entity_id: EntityId,
    accounts: SimpleEntryAccounts,
) -> Result<()> {
    simple_entry_sides(conn, entity_id, accounts).map(|_| ())
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
    /// `?1`: the id of the entity whose entries are listed.
    entity_id: String,
    /// `?2`: inclusive lower date bound as `YYYY-MM-DD`, if any.
    date_from: Option<String>,
    /// `?3`: inclusive upper date bound as `YYYY-MM-DD`, if any.
    date_to: Option<String>,
    /// `?4`: case-folded `LIKE` pattern for the text search, if any.
    pattern: Option<String>,
    /// `?5`: the id of an account that a listed entry must have a line on, if
    /// any.
    account_id: Option<String>,
}

impl ListedEntries {
    /// Normalizes `filter` into the values the predicate binds.
    fn new(entity_id: EntityId, filter: &EntryFilter) -> Self {
        // SQL compares the date column as text, which agrees with date order
        // because `format_date` writes every date in the same fixed width.
        Self {
            entity_id: entity_id.to_string(),
            date_from: filter.date_from.map(format_date),
            date_to: filter.date_to.map(format_date),
            pattern: filter
                .text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(like_pattern),
            account_id: filter.account_id.map(|id| id.to_string()),
        }
    }

    /// The bound values, `?1` to `?5`.
    fn bound(&self) -> [&dyn rusqlite::ToSql; 5] {
        [
            &self.entity_id,
            &self.date_from,
            &self.date_to,
            &self.pattern,
            &self.account_id,
        ]
    }
}

/// Wraps trimmed user text in `%…%`, escaping LIKE wildcards so `%`/`_`
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

/// Loads the entries [`list_entries`] returns, newest first, each with
/// whether it is out of the active books.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored id, date or status that does not
///   parse.
/// - [`Error::Database`] on database errors.
fn load_listed_headers(
    conn: &Connection,
    listed: &ListedEntries,
) -> Result<Vec<(JournalEntry, bool)>> {
    let mut stmt = conn
        .prepare(&listed_headers_sql())
        .database("list journal entries")?;

    let rows = stmt
        .query_map(listed.bound(), |row| {
            let is_voided: i64 = row.get(8)?;
            Ok(map_entry_row(row).map(|entry| (entry, is_voided != 0)))
        })
        .database("list journal entries")?;

    collect_rows("list journal entries", rows)
}

/// The query of [`load_listed_headers`]: the entries
/// [`LISTED_ENTRIES_PREDICATE`] selects, each with whether it is voided.
///
/// The voided test looks, once per listed entry, for an entry that names it
/// in `voided_by_entry_id`. The index `idx_entries_voided_by` answers that
/// lookup; without it each one reads the whole table, and the listing takes
/// time with the square of the number of entries.
fn listed_headers_sql() -> String {
    format!(
        "
        SELECT je.id, je.entity_id, je.entry_date, je.description, je.reference,
               je.status, je.hidden, je.voided_by_entry_id,
               je.voided_by_entry_id IS NOT NULL
                   OR EXISTS (
                       SELECT 1 FROM journal_entries voider
                       WHERE voider.voided_by_entry_id = je.id
                   ) AS is_voided
        FROM journal_entries je
        WHERE {LISTED_ENTRIES_PREDICATE}
        ORDER BY je.entry_date DESC, je.created_at DESC, je.rowid DESC
        "
    )
}

/// The query of [`account_register`]: the lines of active entries on account
/// `?1` dated from `?2` through `?3`, where a `NULL` bound is open.
fn register_sql() -> String {
    format!(
        "
        SELECT je.id, je.entry_date, je.description,
               jl.debit_minor, jl.credit_minor, je.hidden
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        WHERE jl.account_id = ?1
          AND {ACTIVE_ENTRY_PREDICATE}
          AND (?2 IS NULL OR je.entry_date >= ?2)
          AND (?3 IS NULL OR je.entry_date <= ?3)
        ORDER BY je.entry_date ASC, je.created_at ASC, je.rowid ASC, jl.line_order ASC
        "
    )
}

/// Loads the lines of every entry [`list_entries`] returns, grouped by entry
/// and in line order within each.
///
/// The entries are selected by joining on [`LISTED_ENTRIES_PREDICATE`] rather
/// than by an `IN` list of their ids: a list binds one variable per entry, and
/// `SQLite` refuses a statement with more than `SQLITE_MAX_VARIABLE_NUMBER`
/// of them (<https://www.sqlite.org/limits.html#max_variable_number>; 32766
/// in the bundled build), which a large book exceeds. The test
/// `tests/entry_list_scale.rs` lists a book past that limit.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored id or amount that does not parse.
/// - [`Error::Database`] on database errors.
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
    let mut stmt = conn.prepare(&sql).database("list journal lines")?;
    let rows = stmt
        .query_map(listed.bound(), |row| Ok(map_line_row(row)))
        .database("list journal lines")?;

    let mut grouped: HashMap<JournalEntryId, Vec<JournalLine>> = HashMap::new();
    for line in collect_rows("list journal lines", rows)? {
        grouped.entry(line.entry_id).or_default().push(line);
    }
    Ok(grouped)
}

/// Loads the lines of one entry, in the order they were posted.
///
/// # Errors
///
/// - [`Error::VaultCorrupt`] for a stored id or amount that does not parse.
/// - [`Error::Database`] on database errors.
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
        .database("read journal lines")?;

    let rows = stmt
        .query_map([entry_id.to_string()], |row| Ok(map_line_row(row)))
        .database("read journal lines")?;

    collect_rows("read journal lines", rows)
}

/// Returns whether another entry names `id` in its `voided_by_entry_id`.
///
/// That is the second half of the void test in [`get_entry`]: it finds a
/// reversing entry that carries no link of its own.
///
/// # Errors
///
/// [`Error::Database`] on database errors.
fn entry_is_void_reverse(conn: &Connection, id: JournalEntryId) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "
            SELECT COUNT(1) FROM journal_entries
            WHERE voided_by_entry_id = ?1
            ",
            [id.to_string()],
            |row| row.get(0),
        )
        .database("check journal entry is a reversal")?;
    Ok(count > 0)
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

/// Validates `input` and inserts its header and lines, without transaction
/// management.
///
/// Callers own the transaction: [`post_entry`] and [`void_entry`] wrap this so
/// a failure mid-insert can never leave a partial posted entry behind.
/// `hidden` is for [`void_entry`]: a reverse of a hidden original stays hidden.
///
/// # Errors
///
/// Those of [`post_entry`]; with [`ArchivedAccounts::Accept`], an archived
/// account is not one of them.
fn post_entry_in_tx(
    conn: &Connection,
    input: &PostJournal,
    hidden: bool,
    archived: ArchivedAccounts,
) -> Result<PostedEntryView> {
    // An empty description is accepted: the quick-add form leaves it optional.
    let description = input.description.trim();

    let entry_date = input.entry_date;
    let entry_id = JournalEntryId::generate();

    let mut lines = Vec::with_capacity(input.lines.len());
    for line_input in &input.lines {
        let account = get_account(conn, line_input.account_id)?;
        ensure_in_book(&account, input.entity_id)?;
        match archived {
            ArchivedAccounts::Refuse => ensure_active(&account)?,
            ArchivedAccounts::Accept => {}
        }

        lines.push(JournalLine {
            id: JournalLineId::generate(),
            entry_id,
            account_id: line_input.account_id,
            amount: line_input.amount,
            side: line_input.side,
            memo: line_input.memo.clone(),
        });
    }

    validate_lines_for_post(&lines)?;

    let now = now_utc_string();
    let reference = input
        .reference
        .as_ref()
        .map(|reference| reference.trim().to_owned())
        .filter(|reference| !reference.is_empty());

    conn.execute(
        "
        INSERT INTO journal_entries (
            id, entity_id, entry_date, description, reference,
            status, created_at, posted_at, voided_by_entry_id, hidden
        ) VALUES (?1, ?2, ?3, ?4, ?5, 'posted', ?6, ?6, NULL, ?7)
        ",
        rusqlite::params![
            entry_id.to_string(),
            input.entity_id.to_string(),
            format_date(entry_date),
            description,
            reference,
            now,
            i64::from(hidden),
        ],
    )
    .database("insert journal entry")?;

    for (order, line) in lines.iter().enumerate() {
        conn.execute(
            "
            INSERT INTO journal_lines (
                id, entry_id, account_id, debit_minor, credit_minor, memo, line_order
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            rusqlite::params![
                line.id.to_string(),
                entry_id.to_string(),
                line.account_id.to_string(),
                line.debit().amount_minor(),
                line.credit().amount_minor(),
                line.memo,
                i32::try_from(order).unwrap_or(i32::MAX),
            ],
        )
        .database("insert journal line")?;
    }

    get_entry(conn, entry_id)
}

/// Resolves the (debit, credit) account pair for a simple entry, checking
/// each account as posting does.
///
/// The debited account is looked up and checked for its type first, then the
/// credited one; the remaining checks follow for both.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown account id.
/// - [`ValidationError::AccountWrongType`] when an account has a type its
///   part does not accept.
/// - [`ValidationError::SameAccount`] when both sides are one account.
/// - [`Error::AccountWrongEntity`] or [`ValidationError::AccountInactive`]
///   when an account belongs to another book or is archived.
/// - [`Error::VaultCorrupt`] for a stored account that does not parse.
/// - [`Error::Database`] on database errors.
fn simple_entry_sides(
    conn: &Connection,
    entity_id: EntityId,
    accounts: SimpleEntryAccounts,
) -> Result<(AccountId, AccountId)> {
    let (debit, credit) = accounts.sides();
    let debit = role_account(conn, debit)?;
    let credit = role_account(conn, credit)?;

    // Two parts that accept different types cannot hold one account, so this
    // is reached only where both accept the same types, as in a transfer.
    if debit.id == credit.id {
        return Err(ValidationError::SameAccount.into());
    }

    for account in [&debit, &credit] {
        ensure_in_book(account, entity_id)?;
        ensure_active(account)?;
    }
    Ok((debit.id, credit.id))
}

/// Loads the account on one side of a simple entry and checks that its type
/// is one the part accepts.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown account id.
/// - [`ValidationError::AccountWrongType`], naming the part and the
///   account's code, for an account of a type the part does not accept.
/// - [`Error::VaultCorrupt`] for a stored account that does not parse.
/// - [`Error::Database`] on database errors.
fn role_account(conn: &Connection, side: RoleAccount) -> Result<Account> {
    let account = get_account(conn, side.id)?;
    if !side.allowed.contains(&account.account_type) {
        return Err(ValidationError::AccountWrongType {
            role: side.role,
            code: account.code,
        }
        .into());
    }
    Ok(account)
}

/// Checks that `account` belongs to the book of `entity_id`.
///
/// # Errors
///
/// [`Error::AccountWrongEntity`] when it belongs to another.
fn ensure_in_book(account: &Account, entity_id: EntityId) -> Result<()> {
    if account.entity_id != entity_id {
        return Err(Error::AccountWrongEntity);
    }
    Ok(())
}

/// Checks that `account` is not archived.
///
/// # Errors
///
/// [`ValidationError::AccountInactive`], carrying the account's code, when it
/// is.
fn ensure_active(account: &Account) -> Result<()> {
    if !account.is_active {
        return Err(ValidationError::AccountInactive {
            code: account.code.clone(),
        }
        .into());
    }
    Ok(())
}

/// Posts the reversing entry of `id` and links the two, without transaction
/// management.
///
/// The caller owns the transaction: the reversal and the two links must be
/// written together, and [`replace_simple_entry`] adds the replacement entry
/// to the same transaction.
///
/// # Errors
///
/// Those of [`void_entry`].
fn void_entry_in_tx(conn: &Connection, id: JournalEntryId, locale: Locale) -> Result<VoidResult> {
    let view = get_entry(conn, id)?;
    if view.is_voided {
        return Err(ValidationError::EntryAlreadyVoided.into());
    }
    if view.entry.status != EntryStatus::Posted {
        return Err(ValidationError::EntryNotPosted.into());
    }

    let reverse_lines: Vec<PostJournalLine> = view
        .lines
        .iter()
        .map(|line| PostJournalLine {
            account_id: line.account_id,
            amount: line.amount,
            side: line.side.opposite(),
            memo: Some(void_memo(locale).into()),
        })
        .collect();

    let reverse_input = PostJournal {
        entity_id: view.entry.entity_id,
        entry_date: view.entry.entry_date,
        description: void_description(locale, &view.entry.description),
        reference: view.entry.reference.clone(),
        lines: reverse_lines,
    };
    let reverse = post_entry_in_tx(
        conn,
        &reverse_input,
        view.entry.hidden,
        ArchivedAccounts::Accept,
    )?;

    conn.execute(
        "UPDATE journal_entries SET voided_by_entry_id = ?1 WHERE id = ?2",
        rusqlite::params![reverse.entry.id.to_string(), id.to_string()],
    )
    .database("link voided entry to its reversal")?;

    // The reversal is linked back to the original as well, so that it
    // carries its own mark of being out of the active books. A vault whose
    // voids wrote only the first link is still read correctly: every reader
    // also looks for an entry that points at the one it is reading.
    conn.execute(
        "UPDATE journal_entries SET voided_by_entry_id = ?1 WHERE id = ?2",
        rusqlite::params![id.to_string(), reverse.entry.id.to_string()],
    )
    .database("link reversal to voided entry")?;

    Ok(VoidResult {
        original_id: id,
        reverse_id: reverse.entry.id,
    })
}

/// Maps a row selected as `id, entry_id, account_id, debit_minor,
/// credit_minor, memo`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when an id does not parse, an
/// amount is negative, both amounts are zero or both are greater than zero,
/// or a column has the wrong storage class.
fn map_line_row(row: &rusqlite::Row<'_>) -> Result<JournalLine> {
    let id = stored_id("journal_lines.id", &read_column::<String>(row, 0)?)?;
    let entry_id = stored_id("journal_lines.entry_id", &read_column::<String>(row, 1)?)?;
    let account_id = stored_id("journal_lines.account_id", &read_column::<String>(row, 2)?)?;

    let debit = stored_amount("journal_lines.debit_minor", read_column(row, 3)?)?;
    let credit = stored_amount("journal_lines.credit_minor", read_column(row, 4)?)?;
    // The schema's `CHECK` allows a line one side only, so a row with both
    // or neither was not written by the application.
    let (side, amount) = Side::from_columns(debit, credit).map_err(|_| {
        corrupt_column(
            "journal_lines.debit_minor",
            format_args!(
                "a line with a debit of {} and a credit of {}",
                debit.amount_minor(),
                credit.amount_minor()
            ),
        )
    })?;

    Ok(JournalLine {
        id,
        entry_id,
        account_id,
        amount,
        side,
        memo: read_column(row, 5)?,
    })
}

/// Maps a row of [`register_sql`], selected as `id, entry_date, description,
/// debit_minor, credit_minor, hidden`, with a balance of 0 for
/// [`account_register`] to fill in.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when the id or the date does
/// not parse, or a column has the wrong storage class.
fn map_register_row(row: &rusqlite::Row<'_>) -> Result<RegisterLine> {
    let entry_id = stored_id("journal_entries.id", &read_column::<String>(row, 0)?)?;
    let entry_date = stored_date(
        "journal_entries.entry_date",
        &read_column::<String>(row, 1)?,
    )?;

    Ok(RegisterLine {
        entry_id,
        entry_date,
        description: read_column(row, 2)?,
        debit_minor: read_column(row, 3)?,
        credit_minor: read_column(row, 4)?,
        balance_minor: 0,
        hidden: read_column::<i64>(row, 5)? != 0,
    })
}

/// Returns a line amount as stored; the schema's `CHECK` keeps it
/// non-negative.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `column` when `minor` is negative.
fn stored_amount(column: &str, minor: i64) -> Result<Money> {
    Money::from_minor(minor)
        .map_err(|_| corrupt_column(column, format_args!("negative amount: {minor}")))
}

/// Maps a row whose first seven columns are `id, entity_id, entry_date,
/// description, reference, status, hidden`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when an id, the date or the
/// status does not parse, or a column has the wrong storage class.
fn map_entry_row(row: &rusqlite::Row<'_>) -> Result<JournalEntry> {
    let id = stored_id("journal_entries.id", &read_column::<String>(row, 0)?)?;
    let entity_id = stored_id("journal_entries.entity_id", &read_column::<String>(row, 1)?)?;
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
        id,
        entity_id,
        entry_date,
        description: read_column(row, 3)?,
        reference: read_column(row, 4)?,
        status,
        hidden: read_column::<i64>(row, 6)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrated_connection, query_plan};

    /// The index a lookup by `voided_by_entry_id` must go through.
    const VOID_LINK_INDEX: &str = "idx_entries_voided_by";

    #[test]
    fn listing_entries_finds_the_void_link_through_its_index() {
        let conn = migrated_connection();
        let listed = ListedEntries::new(EntityId::generate(), &EntryFilter::default());

        let plan = query_plan(&conn, &listed_headers_sql(), &listed.bound());

        assert!(plan.contains(VOID_LINK_INDEX), "{plan}");
    }

    #[test]
    fn the_account_filter_of_a_listing_reads_only_the_lines_of_each_entry() {
        let conn = migrated_connection();
        let filter = EntryFilter {
            account_id: Some(AccountId::generate()),
            ..EntryFilter::default()
        };
        let listed = ListedEntries::new(EntityId::generate(), &filter);

        let plan = query_plan(&conn, &listed_headers_sql(), &listed.bound());

        assert!(plan.contains("idx_lines_entry_account"), "{plan}");
    }

    #[test]
    fn the_register_tests_for_a_void_through_the_index() {
        let conn = migrated_connection();
        let account = AccountId::generate().to_string();
        let open: Option<String> = None;

        let plan = query_plan(&conn, &register_sql(), &[&account, &open, &open]);

        assert!(plan.contains(VOID_LINK_INDEX), "{plan}");
    }
}
