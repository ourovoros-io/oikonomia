//! The requests the UI sends and the views it reads, in the flat shape they
//! have as JSON.
//!
//! # Two types per shape
//!
//! A request crosses IPC as JSON whose shape the web UI mirrors by hand, and
//! that shape is loose on purpose: a date is text, and a form is sent as far
//! as the user has filled it in, so an account may be missing or a bill may
//! not say whether it is paid. Inside the crate the same request is a strict
//! type that cannot say any of that. Each request therefore exists twice:
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
//! only the strict type.
//!
//! A view goes the other way. [`RecurringTemplateView`] is strict in memory
//! and serializes through a private flat twin, so the UI keeps reading the
//! kind, the bill status and five optional accounts side by side.
//!
//! The test `tests/wire_json.rs` pins the JSON of every type here.
//!
//! # Why the wire types stay loose
//!
//! A value the JSON layer refuses never reaches this crate. The desktop
//! shell reports such a failure as text, which the UI can only show as an
//! unknown error. A date that is not a date must reach the user as
//! `invalid_date` and a missing account as `account_required`, so the wire
//! types accept both and the conversion reports them as a
//! [`ValidationError`]. Types that only leave the crate, the views, carry a
//! [`time::Date`] and serialize it with [`crate::util::serde_date`], because
//! nothing can be wrong with a value core wrote itself.
//!
//! # Order of the checks
//!
//! A conversion looks at the request alone and so runs before anything that
//! needs the database. A request with two faults, a missing account and an
//! account that does not exist, is reported for the missing one.
//!
//! Within a conversion the order is the one the ledger had when it checked
//! everything itself: the amount, then what else the values alone decide (a
//! template's name and day of the month), then the accounts the kind needs,
//! debited one first. The date is read last for a simple entry and before
//! the accounts for a template.

use crate::domain::{AccountId, EntityId, RecurringTemplateId};
use crate::error::{Error, Result, ValidationError};
use crate::ledger::journals::{CreateJournalLine, PostJournal, PostSimpleEntry};
use crate::ledger::recurring::{
    CreateRecurringTemplate, RecurringCadence, RecurringTemplateFields, RecurringTemplateView,
    UpdateRecurringTemplate, check_template_values,
};
use crate::ledger::simple_entry::{
    SimpleBillStatus, SimpleEntryAccounts, SimpleEntryKind, SimpleEntryRoleAccounts,
};
use crate::util::parse_date;
use serde::{Deserialize, Serialize};
use time::Date;

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
    /// [`ValidationError::InvalidDate`] when `entry_date` is not a
    /// `YYYY-MM-DD` date.
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

    /// Checks that `request` holds what its kind needs and types it.
    ///
    /// An account in a part the kind does not have is dropped, and so is a
    /// bill status on an entry that is not a bill.
    ///
    /// # Errors
    ///
    /// The first of these that applies:
    ///
    /// - [`ValidationError::AmountNotPositive`] for an amount of zero or less.
    /// - [`ValidationError::BillStatusRequired`] for a bill without a status.
    /// - [`ValidationError::AccountRequired`], naming the part, when an
    ///   account the kind needs is missing.
    /// - [`ValidationError::InvalidDate`] when `entry_date` is not a
    ///   `YYYY-MM-DD` date.
    fn try_from(request: PostSimpleEntryRequest) -> Result<Self> {
        if request.amount_minor <= 0 {
            return Err(ValidationError::AmountNotPositive.into());
        }
        let accounts = SimpleEntryAccounts::from_roles(
            request.kind,
            request.bill_status,
            SimpleEntryRoleAccounts {
                category: request.category_account_id,
                wallet: request.wallet_account_id,
                payable: request.payable_account_id,
                from: request.from_account_id,
                to: request.to_account_id,
            },
        )?;

        Ok(Self {
            entity_id: request.entity_id,
            accounts,
            entry_date: parse_date(&request.entry_date)?,
            description: request.description,
            reference: request.reference,
            amount_minor: request.amount_minor,
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

    /// Checks that `request` holds what its kind needs and types it.
    ///
    /// # Errors
    ///
    /// The first of these that applies:
    ///
    /// - [`ValidationError::NameRequired`] for a name that is blank.
    /// - [`ValidationError::AmountNotPositive`] for an amount of zero or less.
    /// - [`ValidationError::DayOfMonthInvalid`] for a monthly cadence with no
    ///   day or one outside 1 to 31, and for another cadence with a day.
    /// - [`ValidationError::InvalidDate`] when `next_date` is not a
    ///   `YYYY-MM-DD` date.
    /// - [`ValidationError::BillStatusRequired`] for a bill without a status.
    /// - [`ValidationError::AccountRequired`], naming the part, when an
    ///   account the kind needs is missing.
    fn try_from(request: CreateRecurringTemplateRequest) -> Result<Self> {
        check_template_values(
            &request.name,
            request.amount_minor,
            request.cadence,
            request.day_of_month,
        )?;
        let next_date = parse_date(&request.next_date)?;
        let accounts = SimpleEntryAccounts::from_roles(
            request.kind,
            request.bill_status,
            SimpleEntryRoleAccounts {
                category: request.category_account_id,
                wallet: request.wallet_account_id,
                payable: request.payable_account_id,
                from: request.from_account_id,
                to: request.to_account_id,
            },
        )?;

        Ok(Self {
            entity_id: request.entity_id,
            fields: RecurringTemplateFields {
                name: request.name,
                amount_minor: request.amount_minor,
                cadence: request.cadence,
                day_of_month: request.day_of_month,
                accounts,
                memo: request.memo,
                next_date,
            },
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

    /// Checks that `request` holds what its kind needs and types it.
    ///
    /// # Errors
    ///
    /// Those of the conversion of a [`CreateRecurringTemplateRequest`], in
    /// the same order.
    fn try_from(request: UpdateRecurringTemplateRequest) -> Result<Self> {
        check_template_values(
            &request.name,
            request.amount_minor,
            request.cadence,
            request.day_of_month,
        )?;
        let next_date = parse_date(&request.next_date)?;
        let accounts = SimpleEntryAccounts::from_roles(
            request.kind,
            request.bill_status,
            SimpleEntryRoleAccounts {
                category: request.category_account_id,
                wallet: request.wallet_account_id,
                payable: request.payable_account_id,
                from: request.from_account_id,
                to: request.to_account_id,
            },
        )?;

        Ok(Self {
            id: request.id,
            fields: RecurringTemplateFields {
                name: request.name,
                amount_minor: request.amount_minor,
                cadence: request.cadence,
                day_of_month: request.day_of_month,
                accounts,
                memo: request.memo,
                next_date,
            },
        })
    }
}

/// The JSON shape of a [`RecurringTemplateView`].
///
/// The view converts into this to be serialized and out of it when read
/// back, so the type itself never appears in a signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecurringTemplateViewWire {
    /// Id of the template.
    id: RecurringTemplateId,
    /// Entity the template belongs to.
    entity_id: EntityId,
    /// Name of the template and description of the entries it posts.
    name: String,
    /// Kind of entry the template posts.
    kind: SimpleEntryKind,
    /// Payment state of a bill; `None` for the other kinds.
    bill_status: Option<SimpleBillStatus>,
    /// Amount in minor units.
    amount_minor: i64,
    /// How often the template recurs.
    cadence: RecurringCadence,
    /// Day of the month for a monthly template; `None` for the others.
    day_of_month: Option<u8>,
    /// Expense or income category account, when the kind has one.
    category_account_id: Option<AccountId>,
    /// Bank, cash or card account, when the kind has one.
    wallet_account_id: Option<AccountId>,
    /// Bills payable liability account, when the kind has one.
    payable_account_id: Option<AccountId>,
    /// Account a transfer takes from.
    from_account_id: Option<AccountId>,
    /// Account a transfer pays into.
    to_account_id: Option<AccountId>,
    /// Note stored on the template; `None` when there is none.
    memo: Option<String>,
    /// Next scheduled occurrence.
    #[serde(with = "crate::util::serde_date")]
    next_date: Date,
    /// Whether the next occurrence is on or before the day the view was
    /// built for.
    due: bool,
}

impl From<RecurringTemplateView> for RecurringTemplateViewWire {
    /// Spells the accounts of `view` out as a kind, a bill status and five
    /// optional accounts.
    fn from(view: RecurringTemplateView) -> Self {
        let fields = view.fields;
        let roles = fields.accounts.roles();

        Self {
            id: view.id,
            entity_id: view.entity_id,
            name: fields.name,
            kind: fields.accounts.kind(),
            bill_status: fields.accounts.bill_status(),
            amount_minor: fields.amount_minor,
            cadence: fields.cadence,
            day_of_month: fields.day_of_month,
            category_account_id: roles.category,
            wallet_account_id: roles.wallet,
            payable_account_id: roles.payable,
            from_account_id: roles.from,
            to_account_id: roles.to,
            memo: fields.memo,
            next_date: fields.next_date,
            due: view.due,
        }
    }
}

impl TryFrom<RecurringTemplateViewWire> for RecurringTemplateView {
    type Error = Error;

    /// Reads a serialized view back.
    ///
    /// # Errors
    ///
    /// [`ValidationError::BillStatusRequired`] or
    /// [`ValidationError::AccountRequired`] when `wire` does not hold what
    /// its kind needs, which a view this crate serialized always does.
    fn try_from(wire: RecurringTemplateViewWire) -> Result<Self> {
        let accounts = SimpleEntryAccounts::from_roles(
            wire.kind,
            wire.bill_status,
            SimpleEntryRoleAccounts {
                category: wire.category_account_id,
                wallet: wire.wallet_account_id,
                payable: wire.payable_account_id,
                from: wire.from_account_id,
                to: wire.to_account_id,
            },
        )?;

        Ok(Self {
            id: wire.id,
            entity_id: wire.entity_id,
            fields: RecurringTemplateFields {
                name: wire.name,
                amount_minor: wire.amount_minor,
                cadence: wire.cadence,
                day_of_month: wire.day_of_month,
                accounts,
                memo: wire.memo,
                next_date: wire.next_date,
            },
            due: wire.due,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{AccountRole, NameField};

    /// A complete expense of 45.00 on 10 August 2026.
    fn expense_request() -> PostSimpleEntryRequest {
        PostSimpleEntryRequest {
            entity_id: EntityId::generate(),
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            entry_date: "2026-08-10".to_owned(),
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

    /// A complete weekly expense template.
    fn template_request() -> CreateRecurringTemplateRequest {
        let expense = expense_request();

        CreateRecurringTemplateRequest {
            entity_id: expense.entity_id,
            name: "Rent".to_owned(),
            kind: expense.kind,
            bill_status: None,
            amount_minor: 80_000,
            cadence: RecurringCadence::Weekly,
            day_of_month: None,
            category_account_id: expense.category_account_id,
            wallet_account_id: expense.wallet_account_id,
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
            memo: None,
            next_date: "2026-09-01".to_owned(),
        }
    }

    /// The update request that holds what `create` holds.
    fn as_update(create: CreateRecurringTemplateRequest) -> UpdateRecurringTemplateRequest {
        UpdateRecurringTemplateRequest {
            id: RecurringTemplateId::generate(),
            name: create.name,
            kind: create.kind,
            bill_status: create.bill_status,
            amount_minor: create.amount_minor,
            cadence: create.cadence,
            day_of_month: create.day_of_month,
            category_account_id: create.category_account_id,
            wallet_account_id: create.wallet_account_id,
            payable_account_id: create.payable_account_id,
            from_account_id: create.from_account_id,
            to_account_id: create.to_account_id,
            memo: create.memo,
            next_date: create.next_date,
        }
    }

    /// The error of converting a simple entry request.
    fn simple_error(request: PostSimpleEntryRequest) -> Option<Error> {
        PostSimpleEntry::try_from(request).err()
    }

    /// The errors of converting `request` as a create and as an update, which
    /// must be the same.
    fn template_error(request: CreateRecurringTemplateRequest) -> Option<Error> {
        let created = CreateRecurringTemplate::try_from(request.clone()).err();
        let updated = UpdateRecurringTemplate::try_from(as_update(request)).err();

        assert_eq!(created, updated);
        created
    }

    /// The error for a date written as `text`.
    fn invalid_date(text: &str) -> Error {
        ValidationError::InvalidDate {
            value: text.to_owned(),
        }
        .into()
    }

    #[test]
    fn a_complete_simple_entry_request_converts_to_its_accounts_and_date() {
        let request = expense_request();
        let (category, wallet) = (request.category_account_id, request.wallet_account_id);
        let entry = PostSimpleEntry::try_from(request).unwrap();

        assert_eq!(
            entry.accounts.roles(),
            SimpleEntryRoleAccounts {
                category,
                wallet,
                ..SimpleEntryRoleAccounts::default()
            }
        );
        assert_eq!(entry.accounts.kind(), SimpleEntryKind::Expense);
        assert_eq!(entry.entry_date, parse_date("2026-08-10").unwrap());
        assert_eq!(entry.amount_minor, 4_500);
    }

    #[test]
    fn each_fault_of_a_simple_entry_request_keeps_its_code() {
        let request = expense_request;

        assert_eq!(
            simple_error(PostSimpleEntryRequest {
                amount_minor: 0,
                ..request()
            }),
            Some(ValidationError::AmountNotPositive.into())
        );
        assert_eq!(
            simple_error(PostSimpleEntryRequest {
                wallet_account_id: None,
                ..request()
            }),
            Some(
                ValidationError::AccountRequired {
                    role: AccountRole::Payment
                }
                .into()
            )
        );
        assert_eq!(
            simple_error(PostSimpleEntryRequest {
                kind: SimpleEntryKind::Bill,
                ..request()
            }),
            Some(ValidationError::BillStatusRequired.into())
        );
        assert_eq!(
            simple_error(PostSimpleEntryRequest {
                entry_date: "10/08/2026".to_owned(),
                ..request()
            }),
            Some(invalid_date("10/08/2026"))
        );
    }

    #[test]
    fn a_simple_entry_request_with_several_faults_reports_the_amount_then_the_accounts() {
        let everything_wrong = PostSimpleEntryRequest {
            amount_minor: -1,
            category_account_id: None,
            entry_date: "soon".to_owned(),
            ..expense_request()
        };
        let accounts_and_date_wrong = PostSimpleEntryRequest {
            amount_minor: 1,
            ..everything_wrong.clone()
        };

        assert_eq!(
            simple_error(everything_wrong),
            Some(ValidationError::AmountNotPositive.into())
        );
        assert_eq!(
            simple_error(accounts_and_date_wrong),
            Some(
                ValidationError::AccountRequired {
                    role: AccountRole::Category
                }
                .into()
            )
        );
    }

    #[test]
    fn a_complete_template_request_converts_as_a_create_and_as_an_update() {
        let request = template_request();
        let created = CreateRecurringTemplate::try_from(request.clone()).unwrap();
        let updated = UpdateRecurringTemplate::try_from(as_update(request.clone())).unwrap();

        assert_eq!(created.entity_id, request.entity_id);
        assert_eq!(created.fields, updated.fields);
        assert_eq!(created.fields.name, "Rent");
        assert_eq!(created.fields.next_date, parse_date("2026-09-01").unwrap());
        assert_eq!(created.fields.accounts.kind(), SimpleEntryKind::Expense);
    }

    #[test]
    fn each_fault_of_a_template_request_keeps_its_code() {
        let request = template_request;

        assert_eq!(
            template_error(CreateRecurringTemplateRequest {
                name: "  ".to_owned(),
                ..request()
            }),
            Some(
                ValidationError::NameRequired {
                    field: NameField::TemplateName
                }
                .into()
            )
        );
        assert_eq!(
            template_error(CreateRecurringTemplateRequest {
                amount_minor: 0,
                ..request()
            }),
            Some(ValidationError::AmountNotPositive.into())
        );
        assert_eq!(
            template_error(CreateRecurringTemplateRequest {
                day_of_month: Some(3),
                ..request()
            }),
            Some(ValidationError::DayOfMonthInvalid.into())
        );
        assert_eq!(
            template_error(CreateRecurringTemplateRequest {
                next_date: "2026-9-1".to_owned(),
                ..request()
            }),
            Some(invalid_date("2026-9-1"))
        );
        assert_eq!(
            template_error(CreateRecurringTemplateRequest {
                kind: SimpleEntryKind::Transfer,
                ..request()
            }),
            Some(
                ValidationError::AccountRequired {
                    role: AccountRole::TransferDestination
                }
                .into()
            )
        );
        assert_eq!(
            template_error(CreateRecurringTemplateRequest {
                kind: SimpleEntryKind::Bill,
                ..request()
            }),
            Some(ValidationError::BillStatusRequired.into())
        );
    }

    #[test]
    fn a_view_serializes_flat_and_reads_back_as_the_same_view() {
        let request = template_request();
        let view = RecurringTemplateView {
            id: RecurringTemplateId::generate(),
            entity_id: request.entity_id,
            fields: CreateRecurringTemplate::try_from(request).unwrap().fields,
            due: true,
        };

        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["kind"], "expense");
        assert_eq!(json["bill_status"], serde_json::Value::Null);
        assert_eq!(json["payable_account_id"], serde_json::Value::Null);
        assert_eq!(json["next_date"], "2026-09-01");
        assert_eq!(json["due"], true);
        assert!(json.get("fields").is_none() && json.get("accounts").is_none());

        assert_eq!(
            serde_json::from_value::<RecurringTemplateView>(json).unwrap(),
            view
        );
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// Any kind.
    fn kinds() -> impl Strategy<Value = SimpleEntryKind> {
        prop_oneof![
            Just(SimpleEntryKind::Expense),
            Just(SimpleEntryKind::Income),
            Just(SimpleEntryKind::Bill),
            Just(SimpleEntryKind::Transfer),
        ]
    }

    /// Any bill status, or none.
    fn statuses() -> impl Strategy<Value = Option<SimpleBillStatus>> {
        prop_oneof![
            Just(None),
            Just(Some(SimpleBillStatus::Paid)),
            Just(Some(SimpleBillStatus::Unpaid)),
            Just(Some(SimpleBillStatus::PayExisting)),
        ]
    }

    /// An account that is filled in or left empty.
    fn optional_account() -> impl Strategy<Value = Option<AccountId>> {
        prop::option::of(
            any::<[u8; 16]>().prop_map(|bytes| AccountId::from(uuid::Uuid::from_bytes(bytes))),
        )
    }

    /// A template request with any kind, status and accounts, and otherwise
    /// valid.
    fn template_requests() -> impl Strategy<Value = CreateRecurringTemplateRequest> {
        let accounts = (
            optional_account(),
            optional_account(),
            optional_account(),
            optional_account(),
            optional_account(),
        );

        (kinds(), statuses(), accounts).prop_map(
            |(kind, bill_status, (category, wallet, payable, from, to))| {
                CreateRecurringTemplateRequest {
                    entity_id: EntityId::from(uuid::Uuid::nil()),
                    name: "Rent".to_owned(),
                    kind,
                    bill_status,
                    amount_minor: 1,
                    cadence: RecurringCadence::Monthly,
                    day_of_month: Some(1),
                    category_account_id: category,
                    wallet_account_id: wallet,
                    payable_account_id: payable,
                    from_account_id: from,
                    to_account_id: to,
                    memo: None,
                    next_date: "2026-09-01".to_owned(),
                }
            },
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        // Wire to typed to wire: what the typed value serializes is the
        // request with the parts its kind does not have blanked, and typing
        // that again changes nothing.
        #[test]
        fn a_template_survives_the_wire_once_it_is_typed(request in template_requests()) {
            let Ok(created) = CreateRecurringTemplate::try_from(request.clone()) else {
                return Ok(());
            };
            let view = RecurringTemplateView {
                id: RecurringTemplateId::from(uuid::Uuid::nil()),
                entity_id: created.entity_id,
                fields: created.fields,
                due: false,
            };

            let wire = RecurringTemplateViewWire::from(view.clone());
            prop_assert_eq!(wire.kind, request.kind);
            for (written, sent) in [
                (wire.category_account_id, request.category_account_id),
                (wire.wallet_account_id, request.wallet_account_id),
                (wire.payable_account_id, request.payable_account_id),
                (wire.from_account_id, request.from_account_id),
                (wire.to_account_id, request.to_account_id),
            ] {
                prop_assert!(written.is_none() || written == sent);
            }

            let json = serde_json::to_string(&view).unwrap();
            let read: RecurringTemplateView = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(&read, &view);
            prop_assert_eq!(serde_json::to_string(&read).unwrap(), json);
        }
    }
}
