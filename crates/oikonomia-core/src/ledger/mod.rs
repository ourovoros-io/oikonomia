//! Persistence and queries for double-entry books.
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
//! That failure is reported as [`Error::Io`](crate::error::Error::Io), not as
//! `MoneyOverflow`.

mod accounts;
mod balance;
mod cash_flow;
mod entities;
mod journals;
mod recurring;
mod reports;
mod settings;

pub use accounts::{
    CreateAccount, UpdateAccount, archive_account, create_account, get_account, list_accounts,
    update_account,
};
pub use balance::{account_balance, account_balance_as_of, normal_balance};
pub use cash_flow::{
    CashFlowBucket, CashFlowGranularity, CashFlowSeries, DAILY_BUCKET_MAX_DAYS, activity_window,
    cash_flow_series,
};
pub use entities::{
    CreateEntity, archive_entity, count_entities, create_entity, delete_entity, get_entity,
    list_entities, update_entity,
};
pub(crate) use journals::post_simple_entry_unchecked;
pub use journals::{
    CreateJournalLine, EntryFilter, PostJournal, PostSimpleEntry, PostedEntryView, RegisterLine,
    SimpleBillStatus, SimpleEntryKind, VoidResult, account_register, get_entry, list_entries,
    post_entry, post_simple_entry, replace_simple_entry, set_account_opening_balance,
    set_entry_hidden, void_entry,
};
pub use recurring::{
    CreateRecurringTemplate, RecurringCadence, RecurringPostResult, RecurringTemplateView,
    UpdateRecurringTemplate, advance_next_date, create_recurring_template,
    delete_recurring_template, get_recurring_template, list_recurring_templates,
    list_recurring_templates_as_of, post_recurring_template, template_is_due,
    update_recurring_template,
};
pub use reports::{
    BalanceSheet, BalanceSheetSection, DashboardSummary, PnL, ReportLine, SyntheticLine,
    TopExpense, TrialBalance, balance_sheet, dashboard_summary, previous_window, profit_and_loss,
    profit_and_loss_export, trial_balance,
};
pub use settings::{DEFAULT_LOCK_TIMEOUT_SECS, get_lock_timeout_secs, set_lock_timeout_secs};
