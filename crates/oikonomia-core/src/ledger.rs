//! The double-entry ledger: entities, their charts of accounts, the journal,
//! recurring templates, reports, and the settings stored beside them.
//!
//! Every function that reads or writes the books takes the connection of an
//! unlocked vault. The submodules are private and their public items are
//! re-exported from here:
//!
//! - `entities`: the separate sets of books a vault holds.
//! - `accounts`: the chart of accounts of an entity.
//! - `journals`: posting, listing, voiding and correcting entries, and the
//!   register of one account.
//! - `balance`: the sign of a balance, the active-entry rule, and checked
//!   arithmetic on amounts.
//! - `calendar`: arithmetic on calendar months, shared by the reports and the
//!   recurring templates.
//! - `reports`: trial balance, profit and loss, balance sheet, dashboard.
//! - `cash_flow`: income and expenses over time.
//! - `recurring`: entry templates with a schedule.
//! - `settings`: settings kept in the vault.
//! - `simple_entry`: the kinds of simple entry and the accounts each needs.
//! - `wire`: the requests the UI sends, in their JSON shape, and their
//!   conversion into the strict inputs the functions here take.
//!
//! # Invariants
//!
//! The module enforces these, and a change to it must keep them:
//!
//! - **Amounts are integer minor units**, in `i64`. No amount, total or ratio
//!   is computed in floating point.
//! - **A posted entry balances.** It has at least two lines, each line is a
//!   debit or a credit and never both, and its debits add up to its credits.
//!   A line holds one amount and one side, so the types rule out a line on
//!   both; one function writes entries and checks the rest first, and the
//!   schema repeats the rule for a single line as a `CHECK`.
//! - **Accounting rules are decided here, not in the UI.** The simple entry
//!   form sends an amount and accounts named by the part they play, and
//!   [`SimpleEntryAccounts`] decides which is debited.
//! - **A write of several rows is atomic.** See [Transactions](#transactions).
//! - **A report filters entries inside its subquery**, never in the `ON`
//!   clause of a `LEFT JOIN`, where the filter would have no effect.
//! - **A stored value that does not parse is a corrupt vault.** A function
//!   that maps a stored row reports it as
//!   [`Error::VaultCorrupt`](crate::error::Error::VaultCorrupt) and fails,
//!   instead of leaving the row out or returning it half-read. Two values are
//!   tolerated because nothing is computed from them: an account's sort order
//!   outside `i32` reads as 0, and so does an entry count outside `usize`.
//!   The reports are the gap: they select accounts by type and so never map
//!   an account whose stored type is unknown. The `reports` module doc says
//!   what they rely on instead.
//!
//! # Inputs
//!
//! The functions here take typed values: a [`time::Date`], never date text,
//! and the strict inputs ([`PostJournal`] and its [`PostJournalLine`]s,
//! [`PostSimpleEntry`],
//! [`CreateRecurringTemplate`], [`UpdateRecurringTemplate`]). Text is parsed
//! once, where it enters. A request the UI sends as one JSON object arrives
//! in its wire form ([`PostJournalRequest`], [`PostSimpleEntryRequest`] and
//! the two template requests) and converts with `TryFrom`; a date sent as a
//! bare argument arrives as a [`DateText`](crate::util::DateText). Both
//! report a malformed date as
//! [`ValidationError::InvalidDate`](crate::error::ValidationError::InvalidDate).
//!
//! [`CreateEntity`] is the one input without a twin: it is the wire shape
//! itself, and [`create_entity`] parses its currency text and month number.
//!
//! # Active entries and voids
//!
//! A posted entry is never changed or deleted to take it back. Voiding it
//! ([`void_entry`]) posts a reversing entry, with every debit and credit
//! swapped, and links the two: the original's `voided_by_entry_id` names the
//! reversal and the reversal's names the original. An entry with that link in
//! either direction is voided.
//!
//! An *active* entry is one that is posted and is neither voided nor the
//! reversal of a void. Every balance and every report counts active entries
//! only, so a voided pair adds nothing anywhere, although both entries stay
//! in the journal and [`list_entries`] still returns them, marked as voided.
//!
//! # Hidden entries
//!
//! The hidden flag ([`set_entry_hidden`]) keeps an entry out of what leaves
//! the app. It changes exactly two outputs: the journal CSV export and
//! [`profit_and_loss_export`] leave hidden entries out. Everything shown
//! inside the app counts them: the entry list, the register, the dashboard,
//! the cash flow series and every other report. The reversal of a hidden
//! entry and the replacement of one are hidden too. The flag is not a second
//! layer of encryption.
//!
//! # Transactions
//!
//! A public function that writes more than one row opens a transaction and
//! commits it, so a failure part-way leaves nothing behind. Functions take a
//! shared `&Connection`, so the transaction is rusqlite's
//! `unchecked_transaction`. Opening a second one inside it fails at run
//! time; rusqlite documents that it does and leaves the error unspecified:
//! <https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html#method.unchecked_transaction>.
//! Such a function can therefore not be called from inside another
//! transaction.
//!
//! The work is for that reason in a helper that takes the connection and
//! opens nothing: the caller owns the transaction. A private helper that does
//! the work of one public function carries that function's name with `_in_tx`
//! added. A `pub(crate)` one, which another module can compose with writes of
//! its own, such as posting an entry together with its document, is named
//! `_unchecked`.
//!
//! # Overflow
//!
//! Amounts are `i64` minor units. A balance or total that does not fit is
//! never clamped or wrapped, because a wrong figure would be reported as a
//! right one: every addition and subtraction on amounts in this module is
//! checked and returns [`Error::MoneyOverflow`](crate::error::Error::MoneyOverflow).
//!
//! Totals that `SQLite` adds up with `SUM` are covered too, by a different
//! route: `SUM` over integers raises an error on overflow instead of wrapping
//! (<https://www.sqlite.org/lang_aggfunc.html#sumunc>), so the query fails.
//! That failure is reported as [`Error::Database`](crate::error::Error::Database), not as
//! `MoneyOverflow`.

mod accounts;
mod balance;
mod calendar;
mod cash_flow;
mod entities;
mod journals;
mod recurring;
mod reports;
mod settings;
mod simple_entry;
mod wire;

pub use accounts::{
    CreateAccount, UpdateAccount, archive_account, create_account, get_account, list_accounts,
    update_account,
};
pub(crate) use balance::ACTIVE_ENTRY_PREDICATE;
pub use balance::{account_balance, account_balance_as_of, normal_balance};
pub use cash_flow::{
    CashFlowBucket, CashFlowGranularity, CashFlowSeries, DAILY_BUCKET_MAX_DAYS, activity_window,
    cash_flow_series, cash_flow_series_for_window,
};
pub(crate) use entities::ensure_writable_entity;
pub use entities::{
    CreateEntity, archive_entity, count_entities, create_entity, delete_entity, get_entity,
    list_archived_entities, list_entities, unarchive_entity, update_entity,
};
pub(crate) use journals::post_simple_entry_unchecked;
pub use journals::{
    EntryFilter, PostJournal, PostJournalLine, PostSimpleEntry, PostedEntryView, RegisterLine,
    VoidResult, account_register, get_entry, list_entries, post_entry, post_simple_entry,
    replace_simple_entry, set_account_opening_balance, set_entry_hidden, void_entry,
};
pub use recurring::{
    CreateRecurringTemplate, DayOfMonth, RecurringCadence, RecurringPostResult, RecurringSchedule,
    RecurringTemplateFields, RecurringTemplateView, UpdateRecurringTemplate, advance_next_date,
    create_recurring_template, delete_recurring_template, get_recurring_template,
    list_recurring_templates, list_recurring_templates_as_of, post_recurring_template,
    template_is_due, update_recurring_template,
};
pub use reports::{
    BalanceSheet, BalanceSheetSection, DashboardSummary, PnL, ReportLine, SyntheticLine,
    TopExpense, TrialBalance, balance_sheet, dashboard_summary, previous_window, profit_and_loss,
    profit_and_loss_export, trial_balance,
};
pub use settings::{DEFAULT_LOCK_TIMEOUT_SECS, get_lock_timeout_secs, set_lock_timeout_secs};
pub use simple_entry::{
    SimpleBillStatus, SimpleEntryAccounts, SimpleEntryKind, SimpleEntryRoleAccounts,
};
pub use wire::{
    CreateRecurringTemplateRequest, JournalLineRequest, PostJournalRequest, PostSimpleEntryRequest,
    UpdateRecurringTemplateRequest,
};
