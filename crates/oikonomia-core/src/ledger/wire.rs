//! The requests the UI sends, in the flat shape they have as JSON.
//!
//! # Two types per request
//!
//! A request crosses IPC as JSON whose shape the web UI mirrors by hand, and
//! that shape is loose on purpose: a date is text, and a form is sent as far
//! as the user has filled it in. Inside the crate the same request is a
//! strict type that cannot hold a malformed date. Each request therefore
//! exists twice:
//!
//! | On the wire | Inside the crate |
//! |-------------|------------------|
//! | [`PostJournalRequest`] | [`PostJournal`] |
//! | [`PostSimpleEntryRequest`] | [`PostSimpleEntry`] |
//! | [`CreateRecurringTemplateRequest`] | [`CreateRecurringTemplate`] |
//! | [`UpdateRecurringTemplateRequest`] | [`UpdateRecurringTemplate`] |
//!
//! The wire type derives the serde traits and nothing else is done to it.
//! One `TryFrom` turns it into the strict type, and the ledger functions take
//! only the strict type. The test `tests/wire_json.rs` pins the JSON of every
//! wire type.
//!
//! # Why the wire types stay loose
//!
//! A value the JSON layer refuses never reaches this crate. The desktop
//! shell reports such a failure as text, which the UI can only show as an
//! unknown error. A date that is not a date must reach the user as
//! `invalid_date`, so the wire types keep the date as a `String` and the
//! conversion reports it as
//! [`ValidationError::InvalidDate`](crate::error::ValidationError::InvalidDate).
//! Types that only leave the crate, the views, carry a [`time::Date`] and
//! serialize it with [`crate::util::serde_date`], because nothing can be
//! wrong with a value core wrote itself.
//!
//! # Order of the checks
//!
//! A conversion looks at the request alone and so runs before anything that
//! needs the database. A request with two faults, a malformed date and an
//! account that does not exist, is reported for the date.

use crate::domain::{AccountId, EntityId, RecurringTemplateId};
use crate::error::{Error, Result};
use crate::ledger::journals::{
    CreateJournalLine, PostJournal, PostSimpleEntry, SimpleBillStatus, SimpleEntryKind,
};
use crate::ledger::recurring::{
    CreateRecurringTemplate, RecurringCadence, UpdateRecurringTemplate,
};
use crate::util::parse_date;
use serde::{Deserialize, Serialize};

/// The wire form of [`PostJournal`]: a whole entry, with its lines spelled
/// out and its date as text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostJournalRequest {
    /// Entity whose books the entry goes into.
    pub entity_id: EntityId,
    /// Accounting date as `YYYY-MM-DD`.
    pub entry_date: String,
    /// What the entry is for.
    pub description: String,
    /// The user's own reference for the entry, such as an invoice number.
    pub reference: Option<String>,
    /// The lines of the entry.
    pub lines: Vec<CreateJournalLine>,
}

impl TryFrom<PostJournalRequest> for PostJournal {
    type Error = Error;

    /// Parses the date of `request`; everything else is taken as it is.
    ///
    /// # Errors
    ///
    /// [`ValidationError::InvalidDate`](crate::error::ValidationError::InvalidDate)
    /// when `entry_date` is not a `YYYY-MM-DD` date.
    fn try_from(request: PostJournalRequest) -> Result<Self> {
        Ok(Self {
            entity_id: request.entity_id,
            entry_date: parse_date(&request.entry_date)?,
            description: request.description,
            reference: request.reference,
            lines: request.lines,
        })
    }
}

/// The wire form of [`PostSimpleEntry`]: the simple entry form as the user
/// has filled it in so far.
///
/// It is also the shape of a suggestion the crate hands to the UI, such as a
/// row of a bank statement before the user has picked its accounts, which is
/// why every account is optional here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostSimpleEntryRequest {
    /// Entity whose books the entry goes into.
    pub entity_id: EntityId,
    /// What kind of entry this is.
    pub kind: SimpleEntryKind,
    /// Payment state of a bill; required when `kind` is
    /// [`SimpleEntryKind::Bill`].
    pub bill_status: Option<SimpleBillStatus>,
    /// Accounting date as `YYYY-MM-DD`.
    pub entry_date: String,
    /// What the entry is for.
    pub description: String,
    /// The user's own reference for the entry.
    pub reference: Option<String>,
    /// Amount in minor units.
    pub amount_minor: i64,
    /// Expense or income category account.
    pub category_account_id: Option<AccountId>,
    /// Bank, cash or card account.
    pub wallet_account_id: Option<AccountId>,
    /// Bills payable liability account.
    pub payable_account_id: Option<AccountId>,
    /// Account a transfer takes from.
    pub from_account_id: Option<AccountId>,
    /// Account a transfer pays into.
    pub to_account_id: Option<AccountId>,
}

impl TryFrom<PostSimpleEntryRequest> for PostSimpleEntry {
    type Error = Error;

    /// Parses the date of `request`; everything else is taken as it is.
    ///
    /// # Errors
    ///
    /// [`ValidationError::InvalidDate`](crate::error::ValidationError::InvalidDate)
    /// when `entry_date` is not a `YYYY-MM-DD` date.
    fn try_from(request: PostSimpleEntryRequest) -> Result<Self> {
        Ok(Self {
            entity_id: request.entity_id,
            kind: request.kind,
            bill_status: request.bill_status,
            entry_date: parse_date(&request.entry_date)?,
            description: request.description,
            reference: request.reference,
            amount_minor: request.amount_minor,
            category_account_id: request.category_account_id,
            wallet_account_id: request.wallet_account_id,
            payable_account_id: request.payable_account_id,
            from_account_id: request.from_account_id,
            to_account_id: request.to_account_id,
        })
    }
}

/// The wire form of [`CreateRecurringTemplate`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecurringTemplateRequest {
    /// Entity the template, and every entry it posts, belongs to.
    pub entity_id: EntityId,
    /// Name of the template.
    pub name: String,
    /// Kind of entry the template posts.
    pub kind: SimpleEntryKind,
    /// Payment state of a bill; required when `kind` is
    /// [`SimpleEntryKind::Bill`].
    pub bill_status: Option<SimpleBillStatus>,
    /// Amount in minor units.
    pub amount_minor: i64,
    /// How often the template recurs.
    pub cadence: RecurringCadence,
    /// Day of the month, for a monthly cadence.
    pub day_of_month: Option<u8>,
    /// Expense or income category account.
    pub category_account_id: Option<AccountId>,
    /// Bank, cash or card account.
    pub wallet_account_id: Option<AccountId>,
    /// Bills payable liability account.
    pub payable_account_id: Option<AccountId>,
    /// Account a transfer takes from.
    pub from_account_id: Option<AccountId>,
    /// Account a transfer pays into.
    pub to_account_id: Option<AccountId>,
    /// Note stored on the template.
    pub memo: Option<String>,
    /// Next occurrence as `YYYY-MM-DD`.
    pub next_date: String,
}

impl TryFrom<CreateRecurringTemplateRequest> for CreateRecurringTemplate {
    type Error = Error;

    /// Parses the date of `request`; everything else is taken as it is.
    ///
    /// # Errors
    ///
    /// [`ValidationError::InvalidDate`](crate::error::ValidationError::InvalidDate)
    /// when `next_date` is not a `YYYY-MM-DD` date.
    fn try_from(request: CreateRecurringTemplateRequest) -> Result<Self> {
        Ok(Self {
            entity_id: request.entity_id,
            name: request.name,
            kind: request.kind,
            bill_status: request.bill_status,
            amount_minor: request.amount_minor,
            cadence: request.cadence,
            day_of_month: request.day_of_month,
            category_account_id: request.category_account_id,
            wallet_account_id: request.wallet_account_id,
            payable_account_id: request.payable_account_id,
            from_account_id: request.from_account_id,
            to_account_id: request.to_account_id,
            memo: request.memo,
            next_date: parse_date(&request.next_date)?,
        })
    }
}

/// The wire form of [`UpdateRecurringTemplate`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecurringTemplateRequest {
    /// Template to change.
    pub id: RecurringTemplateId,
    /// Name of the template.
    pub name: String,
    /// Kind of entry the template posts.
    pub kind: SimpleEntryKind,
    /// Payment state of a bill; required when `kind` is
    /// [`SimpleEntryKind::Bill`].
    pub bill_status: Option<SimpleBillStatus>,
    /// Amount in minor units.
    pub amount_minor: i64,
    /// How often the template recurs.
    pub cadence: RecurringCadence,
    /// Day of the month, for a monthly cadence.
    pub day_of_month: Option<u8>,
    /// Expense or income category account.
    pub category_account_id: Option<AccountId>,
    /// Bank, cash or card account.
    pub wallet_account_id: Option<AccountId>,
    /// Bills payable liability account.
    pub payable_account_id: Option<AccountId>,
    /// Account a transfer takes from.
    pub from_account_id: Option<AccountId>,
    /// Account a transfer pays into.
    pub to_account_id: Option<AccountId>,
    /// Note stored on the template.
    pub memo: Option<String>,
    /// Next occurrence as `YYYY-MM-DD`.
    pub next_date: String,
}

impl TryFrom<UpdateRecurringTemplateRequest> for UpdateRecurringTemplate {
    type Error = Error;

    /// Parses the date of `request`; everything else is taken as it is.
    ///
    /// # Errors
    ///
    /// [`ValidationError::InvalidDate`](crate::error::ValidationError::InvalidDate)
    /// when `next_date` is not a `YYYY-MM-DD` date.
    fn try_from(request: UpdateRecurringTemplateRequest) -> Result<Self> {
        Ok(Self {
            id: request.id,
            name: request.name,
            kind: request.kind,
            bill_status: request.bill_status,
            amount_minor: request.amount_minor,
            cadence: request.cadence,
            day_of_month: request.day_of_month,
            category_account_id: request.category_account_id,
            wallet_account_id: request.wallet_account_id,
            payable_account_id: request.payable_account_id,
            from_account_id: request.from_account_id,
            to_account_id: request.to_account_id,
            memo: request.memo,
            next_date: parse_date(&request.next_date)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ValidationError;

    /// A request whose only fault, if any, is its date.
    fn simple_request(entry_date: &str) -> PostSimpleEntryRequest {
        PostSimpleEntryRequest {
            entity_id: EntityId::generate(),
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            entry_date: entry_date.to_owned(),
            description: "Groceries".to_owned(),
            reference: None,
            amount_minor: 4_500,
            category_account_id: Some(AccountId::generate()),
            wallet_account_id: Some(AccountId::generate()),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
        }
    }

    /// The error every conversion gives for a date written as `text`.
    fn invalid_date(text: &str) -> Error {
        Error::Validation(ValidationError::InvalidDate {
            value: text.to_owned(),
        })
    }

    #[test]
    fn a_simple_entry_request_keeps_its_date_as_a_date() {
        let entry = PostSimpleEntry::try_from(simple_request("2026-08-10")).unwrap();

        assert_eq!(entry.entry_date, parse_date("2026-08-10").unwrap());
        assert_eq!(entry.amount_minor, 4_500);
    }

    #[test]
    fn a_malformed_date_in_any_request_is_the_invalid_date_error() {
        let text = "10/08/2026";

        assert_eq!(
            PostSimpleEntry::try_from(simple_request(text)).map(|_| ()),
            Err(invalid_date(text))
        );
        assert_eq!(
            PostJournal::try_from(PostJournalRequest {
                entity_id: EntityId::generate(),
                entry_date: text.to_owned(),
                description: String::new(),
                reference: None,
                lines: Vec::new(),
            })
            .map(|_| ()),
            Err(invalid_date(text))
        );

        let simple = simple_request("2026-08-10");
        let create = CreateRecurringTemplateRequest {
            entity_id: simple.entity_id,
            name: "Rent".to_owned(),
            kind: simple.kind,
            bill_status: None,
            amount_minor: 1,
            cadence: RecurringCadence::Weekly,
            day_of_month: None,
            category_account_id: simple.category_account_id,
            wallet_account_id: simple.wallet_account_id,
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
            memo: None,
            next_date: text.to_owned(),
        };
        let update = UpdateRecurringTemplateRequest {
            id: RecurringTemplateId::generate(),
            name: create.name.clone(),
            kind: create.kind,
            bill_status: None,
            amount_minor: 1,
            cadence: create.cadence,
            day_of_month: None,
            category_account_id: create.category_account_id,
            wallet_account_id: create.wallet_account_id,
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
            memo: None,
            next_date: text.to_owned(),
        };

        assert_eq!(
            CreateRecurringTemplate::try_from(create).map(|_| ()),
            Err(invalid_date(text))
        );
        assert_eq!(
            UpdateRecurringTemplate::try_from(update).map(|_| ()),
            Err(invalid_date(text))
        );
    }
}
