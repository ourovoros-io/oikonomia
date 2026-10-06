//! The bookkeeping vocabulary: entities, accounts, journal entries and lines.
//!
//! These are plain data types with no storage behind them. [`crate::ledger`]
//! reads and writes them; this module only says what they are and holds the
//! one rule that needs nothing but the data, [`validate_lines_for_post`].
//!
//! # How the pieces relate
//!
//! - An [`Entity`] is one set of books (a household, a company) in one base
//!   currency.
//! - An [`Account`] belongs to one entity and has an [`AccountType`], which
//!   decides whether a debit raises or lowers its balance.
//! - A [`JournalEntry`] belongs to one entity and is dated. Its
//!   [`JournalLine`]s each debit or credit one account of the same entity.
//! - A recurring template, identified by [`RecurringTemplateId`], is a saved
//!   entry with a cadence and a next date. Its data type lives in
//!   [`crate::ledger`], which posts it when asked.
//!
//! # Identifiers
//!
//! Every record has its own id type ([`EntityId`], [`AccountId`],
//! [`JournalEntryId`], [`JournalLineId`], [`RecurringTemplateId`]), a wrapper
//! around a random UUID, so an account id cannot be passed where an entry id
//! is expected. Each serializes as the bare UUID string.
//!
//! # Wire form
//!
//! The types derive `Serialize` and `Deserialize` because the desktop shell
//! sends them to the web UI as JSON. Enums go out in `snake_case`, dates as
//! `YYYY-MM-DD` ([`crate::util::serde_date`]) and amounts as
//! [`Money`](crate::Money).

mod account;
mod entity;
mod journal;
mod recurring;

pub use account::{Account, AccountId, AccountType};
pub use entity::{ChartTemplate, Entity, EntityId};
pub use journal::{
    EntryStatus, JournalEntry, JournalEntryId, JournalLine, JournalLineId, validate_lines_for_post,
};
pub use recurring::RecurringTemplateId;
