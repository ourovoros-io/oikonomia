//! The bookkeeping vocabulary: entities, accounts, journal entries and lines.
//!
//! These are plain data types with no storage behind them. [`crate::ledger`]
//! reads and writes them; this module only says what they are and holds the
//! one rule that needs nothing but the data, [`validate_lines_for_post`].
//!
//! # How the pieces relate
//!
//! - An [`Entity`] is one set of books (a household, a company) in one base
//!   currency, named by a [`CurrencyCode`].
//! - An [`Account`] belongs to one entity and has an [`AccountType`], which
//!   decides whether a debit raises or lowers its balance.
//! - A [`JournalEntry`] belongs to one entity and is dated. Its
//!   [`JournalLine`]s each put an amount on one [`Side`], debit or credit, of
//!   one account of the same entity.
//! - A recurring template, identified by [`RecurringTemplateId`], is a saved
//!   entry with a cadence and a next date. Its data type lives in
//!   [`crate::ledger`], which posts it when asked.
//!
//! # Identifiers
//!
//! Every record has its own id type ([`EntityId`], [`AccountId`],
//! [`JournalEntryId`], [`JournalLineId`], [`RecurringTemplateId`], and
//! [`DocumentId`](crate::documents::DocumentId) beside the document store),
//! a wrapper around a random UUID, so an account id cannot be passed where an
//! entry id is expected. All of them are written by one macro and so have the
//! same surface: `generate()` for a new record, `as_uuid()`, `Display` and
//! `FromStr` for the hyphenated text, and serde as the bare UUID string. None
//! has a `Default`, so no derived `Default` can mint an id by accident.
//!
//! # Wire form
//!
//! The types derive `Serialize` and `Deserialize` because the desktop shell
//! sends them to the web UI as JSON. Enums go out in `snake_case`, dates as
//! `YYYY-MM-DD` ([`crate::util::serde_date`]), a month as its number from 1
//! to 12 ([`crate::util::serde_month`]), a currency as its three letters and
//! amounts as [`Money`](crate::Money).

mod account;
mod currency;
mod entity;
mod id;
mod journal;
mod recurring;

pub use account::{Account, AccountId, AccountType};
pub use currency::CurrencyCode;
pub use entity::{ChartTemplate, Entity, EntityId};
pub(crate) use id::define_id;
pub use journal::{
    EntryStatus, JournalEntry, JournalEntryId, JournalLine, JournalLineId, Side,
    validate_lines_for_post,
};
pub use recurring::RecurringTemplateId;
