//! The accounts of a simple entry: which two a kind of entry needs, and
//! which of them is debited.
//!
//! The simple entry form and a recurring template both describe an entry by
//! its kind and by accounts named for the part they play, not by debit and
//! credit. This module is the one place that knows what each kind needs.
//!
//! # The six cases
//!
//! [`SimpleEntryAccounts`] has one variant per valid combination, so a value
//! of it always names exactly the two accounts its kind posts to. Its
//! documentation has the table of what each variant debits and credits.
//!
//! # The flat form
//!
//! On the wire, in the `recurring_templates` table and in the preferences
//! file, the same information is flat: a kind, an optional bill status, and
//! five optional accounts ([`SimpleEntryRoleAccounts`]). That form can say
//! things that mean nothing, such as a transfer with a category or a bill
//! with no status. [`SimpleEntryAccounts::from_roles`] is the one conversion
//! out of it, and [`SimpleEntryAccounts::roles`] the one back.
//!
//! Converting reads only the accounts the kind needs. An account the flat
//! form holds for a part the kind does not have is dropped, and so is a bill
//! status on an entry that is not a bill.
//!
//! # One part, three vocabularies
//!
//! The parts have different names on the wire, in the vault and preferences
//! file, and in an error. The documentation of [`SimpleEntryRoleAccounts`]
//! has the one table that maps them.

use crate::domain::{AccountId, AccountType};
use crate::error::{AccountRole, Error, Result, ValidationError};
use serde::{Deserialize, Serialize};

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

impl SimpleEntryKind {
    /// Returns the kind as the UI and the vault write it: `expense`, `income`,
    /// `bill` or `transfer`.
    ///
    /// This is the text serde writes, the text stored in
    /// `recurring_templates.kind`, and the second half of a key in the
    /// preferences file ([`crate::prefs::last_accounts_key`]).
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Income => "income",
            Self::Bill => "bill",
            Self::Transfer => "transfer",
        }
    }
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

/// The two accounts of a simple entry, named for the part each plays.
///
/// Each variant is one valid combination of a kind, a bill status and the
/// accounts that combination needs, so a value says which accounts are
/// posted to and which is debited:
///
/// | Variant | Kind, bill status | Debit | Credit |
/// |---------|-------------------|-------|--------|
/// | `Expense` | expense | category (expense) | wallet (asset or liability) |
/// | `Income` | income | wallet (asset) | category (income) |
/// | `BillPaid` | bill, paid | category (expense) | wallet (asset or liability) |
/// | `BillUnpaid` | bill, unpaid | category (expense) | payable (liability) |
/// | `BillPayment` | bill, pay existing | payable (liability) | wallet (asset or liability) |
/// | `Transfer` | transfer | to (asset or liability) | from (asset or liability) |
///
/// The account types in brackets are what each part accepts. They are not
/// part of the type, because an account's type is known only to the
/// database: [`post_simple_entry`](crate::ledger::post_simple_entry) checks
/// them, together with the rule that the two accounts differ, when it looks
/// the accounts up.
///
/// # Examples
///
/// ```
/// use oikonomia_core::domain::AccountId;
/// use oikonomia_core::ledger::{
///     SimpleBillStatus, SimpleEntryAccounts, SimpleEntryKind, SimpleEntryRoleAccounts,
/// };
///
/// let (power, bills_payable) = (AccountId::generate(), AccountId::generate());
///
/// // An unpaid bill debits its category and credits the payable account.
/// let unpaid = SimpleEntryAccounts::BillUnpaid { category: power, payable: bills_payable };
/// assert_eq!(unpaid.kind(), SimpleEntryKind::Bill);
/// assert_eq!(unpaid.bill_status(), Some(SimpleBillStatus::Unpaid));
/// assert_eq!((unpaid.debit_account(), unpaid.credit_account()), (power, bills_payable));
///
/// // The flat form the UI sends converts to the same value.
/// let roles = SimpleEntryRoleAccounts {
///     category: Some(power),
///     payable: Some(bills_payable),
///     ..SimpleEntryRoleAccounts::default()
/// };
/// let status = Some(SimpleBillStatus::Unpaid);
/// assert_eq!(SimpleEntryAccounts::from_roles(SimpleEntryKind::Bill, status, roles), Ok(unpaid));
/// assert_eq!(unpaid.roles(), roles);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleEntryAccounts {
    /// Money spent now: the category is debited and the wallet credited.
    Expense {
        /// The expense account the money went to.
        category: AccountId,
        /// The bank, cash or card account that paid.
        wallet: AccountId,
    },
    /// Money received: the wallet is debited and the category credited.
    Income {
        /// The income account the money came from.
        category: AccountId,
        /// The asset account the money was paid into.
        wallet: AccountId,
    },
    /// A bill paid at once: the category is debited and the wallet credited.
    BillPaid {
        /// The expense account the bill is for.
        category: AccountId,
        /// The bank, cash or card account that paid.
        wallet: AccountId,
    },
    /// A bill recorded as owed: the category is debited and the payable
    /// account credited.
    BillUnpaid {
        /// The expense account the bill is for.
        category: AccountId,
        /// The liability account the bill is owed on.
        payable: AccountId,
    },
    /// A payment of a bill recorded earlier: the payable account is debited
    /// and the wallet credited.
    BillPayment {
        /// The liability account the bill was owed on.
        payable: AccountId,
        /// The bank, cash or card account that paid.
        wallet: AccountId,
    },
    /// Money moved between the entity's own accounts: the destination is
    /// debited and the source credited.
    Transfer {
        /// The account the money leaves.
        from: AccountId,
        /// The account the money arrives in.
        to: AccountId,
    },
}

impl SimpleEntryAccounts {
    /// Builds the accounts from the flat form: a kind, a bill status and five
    /// optional accounts.
    ///
    /// Only the two accounts the kind needs are read. `bill_status` is read
    /// only for a bill.
    ///
    /// # Errors
    ///
    /// - [`ValidationError::BillStatusRequired`] for a bill without a status.
    /// - [`ValidationError::AccountRequired`], naming the part, when an
    ///   account the kind needs is `None`. Of two missing accounts the
    ///   debited one is reported.
    pub fn from_roles(
        kind: SimpleEntryKind,
        bill_status: Option<SimpleBillStatus>,
        roles: SimpleEntryRoleAccounts,
    ) -> Result<Self> {
        Self::try_from_roles(kind, bill_status, roles).map_err(Error::from)
    }

    /// Builds the accounts from the flat form, saying which part of it is
    /// missing when it does not hold what the kind needs.
    ///
    /// The row mapper of a stored template uses this to name the damaged
    /// column; [`SimpleEntryAccounts::from_roles`] turns the same answer
    /// into a validation error.
    pub(crate) fn try_from_roles(
        kind: SimpleEntryKind,
        bill_status: Option<SimpleBillStatus>,
        roles: SimpleEntryRoleAccounts,
    ) -> std::result::Result<Self, MissingPart> {
        use AccountRole::{
            BillCategory, BillsPayable, Category, Deposit, Income, Payment, TransferDestination,
            TransferSource,
        };

        // In every arm the debited account is required before the credited
        // one, so that of two missing accounts the same one is reported as
        // when the accounts were looked up debit first.
        match (kind, bill_status) {
            (SimpleEntryKind::Expense, _) => Ok(Self::Expense {
                category: roles.category.ok_or(MissingPart::Category(Category))?,
                wallet: roles.wallet.ok_or(MissingPart::Wallet(Payment))?,
            }),
            (SimpleEntryKind::Income, _) => {
                let wallet = roles.wallet.ok_or(MissingPart::Wallet(Deposit))?;
                let category = roles.category.ok_or(MissingPart::Category(Income))?;
                Ok(Self::Income { category, wallet })
            }
            (SimpleEntryKind::Bill, None) => Err(MissingPart::BillStatus),
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::Paid)) => Ok(Self::BillPaid {
                category: roles.category.ok_or(MissingPart::Category(BillCategory))?,
                wallet: roles.wallet.ok_or(MissingPart::Wallet(Payment))?,
            }),
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::Unpaid)) => Ok(Self::BillUnpaid {
                category: roles.category.ok_or(MissingPart::Category(BillCategory))?,
                payable: roles.payable.ok_or(MissingPart::Payable(BillsPayable))?,
            }),
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::PayExisting)) => Ok(Self::BillPayment {
                payable: roles.payable.ok_or(MissingPart::Payable(BillsPayable))?,
                wallet: roles.wallet.ok_or(MissingPart::Wallet(Payment))?,
            }),
            (SimpleEntryKind::Transfer, _) => {
                let to = roles.to.ok_or(MissingPart::To(TransferDestination))?;
                let from = roles.from.ok_or(MissingPart::From(TransferSource))?;
                Ok(Self::Transfer { from, to })
            }
        }
    }

    /// Returns the kind of entry these accounts belong to.
    #[must_use]
    pub const fn kind(&self) -> SimpleEntryKind {
        match self {
            Self::Expense { .. } => SimpleEntryKind::Expense,
            Self::Income { .. } => SimpleEntryKind::Income,
            Self::BillPaid { .. } | Self::BillUnpaid { .. } | Self::BillPayment { .. } => {
                SimpleEntryKind::Bill
            }
            Self::Transfer { .. } => SimpleEntryKind::Transfer,
        }
    }

    /// Returns the payment state of a bill, and `None` for any other kind.
    #[must_use]
    pub const fn bill_status(&self) -> Option<SimpleBillStatus> {
        match self {
            Self::BillPaid { .. } => Some(SimpleBillStatus::Paid),
            Self::BillUnpaid { .. } => Some(SimpleBillStatus::Unpaid),
            Self::BillPayment { .. } => Some(SimpleBillStatus::PayExisting),
            Self::Expense { .. } | Self::Income { .. } | Self::Transfer { .. } => None,
        }
    }

    /// Returns the account the entry debits.
    #[must_use]
    pub fn debit_account(&self) -> AccountId {
        self.sides().0.id
    }

    /// Returns the account the entry credits.
    #[must_use]
    pub fn credit_account(&self) -> AccountId {
        self.sides().1.id
    }

    /// Returns the flat form of these accounts: the two the kind has are
    /// `Some` and the other three `None`.
    #[must_use]
    pub fn roles(&self) -> SimpleEntryRoleAccounts {
        let none = SimpleEntryRoleAccounts::default();

        match *self {
            Self::Expense { category, wallet }
            | Self::Income { category, wallet }
            | Self::BillPaid { category, wallet } => SimpleEntryRoleAccounts {
                category: Some(category),
                wallet: Some(wallet),
                ..none
            },
            Self::BillUnpaid { category, payable } => SimpleEntryRoleAccounts {
                category: Some(category),
                payable: Some(payable),
                ..none
            },
            Self::BillPayment { payable, wallet } => SimpleEntryRoleAccounts {
                payable: Some(payable),
                wallet: Some(wallet),
                ..none
            },
            Self::Transfer { from, to } => SimpleEntryRoleAccounts {
                from: Some(from),
                to: Some(to),
                ..none
            },
        }
    }

    /// Returns the debited account and then the credited one, each with the
    /// part it plays and the account types that part accepts.
    pub(crate) fn sides(&self) -> (RoleAccount, RoleAccount) {
        use AccountType::{Asset, Expense, Income, Liability};

        /// The types a wallet that pays, a transfer source and a transfer
        /// destination accept.
        const ASSET_OR_LIABILITY: &[AccountType] = &[Asset, Liability];

        let side = |id, role, allowed| RoleAccount { id, role, allowed };

        match *self {
            Self::Expense { category, wallet } => (
                side(category, AccountRole::Category, &[Expense]),
                side(wallet, AccountRole::Payment, ASSET_OR_LIABILITY),
            ),
            Self::Income { category, wallet } => (
                side(wallet, AccountRole::Deposit, &[Asset]),
                side(category, AccountRole::Income, &[Income]),
            ),
            Self::BillPaid { category, wallet } => (
                side(category, AccountRole::BillCategory, &[Expense]),
                side(wallet, AccountRole::Payment, ASSET_OR_LIABILITY),
            ),
            Self::BillUnpaid { category, payable } => (
                side(category, AccountRole::BillCategory, &[Expense]),
                side(payable, AccountRole::BillsPayable, &[Liability]),
            ),
            Self::BillPayment { payable, wallet } => (
                side(payable, AccountRole::BillsPayable, &[Liability]),
                side(wallet, AccountRole::Payment, ASSET_OR_LIABILITY),
            ),
            Self::Transfer { from, to } => (
                side(to, AccountRole::TransferDestination, ASSET_OR_LIABILITY),
                side(from, AccountRole::TransferSource, ASSET_OR_LIABILITY),
            ),
        }
    }
}

/// The flat form of [`SimpleEntryAccounts`]: one optional account for each
/// part an entry of any kind can have.
///
/// This is the shape the five `*_account_id` fields of a request, the five
/// role columns of `recurring_templates` and a
/// [`LastRoleAccounts`](crate::prefs::LastRoleAccounts) of the preferences
/// file all share. On its own it says nothing about which accounts belong
/// together; [`SimpleEntryAccounts::from_roles`] reads it for a kind.
///
/// # One part, three vocabularies
///
/// The parts have different names in three places, for historical reasons.
/// The names on the wire are also the column names of `recurring_templates`
/// and the keys of the preferences file, so they are stored on users'
/// machines and are not renamed.
///
/// | Field here | Wire, column and preference key | [`AccountRole`] in an error |
/// |------------|---------------------------------|-----------------------------|
/// | `category` | `category_account_id` | `category`, `income` or `bill_category`, by kind |
/// | `wallet` | `wallet_account_id` | `payment`, or `deposit` for income |
/// | `payable` | `payable_account_id` | `bills_payable` |
/// | `from` | `from_account_id` | `transfer_source` |
/// | `to` | `to_account_id` | `transfer_destination` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimpleEntryRoleAccounts {
    /// Expense or income category account (`category_account_id`).
    pub category: Option<AccountId>,
    /// Bank, cash or card account (`wallet_account_id`).
    pub wallet: Option<AccountId>,
    /// Bills payable liability account (`payable_account_id`).
    pub payable: Option<AccountId>,
    /// Account a transfer takes from (`from_account_id`).
    pub from: Option<AccountId>,
    /// Account a transfer pays into (`to_account_id`).
    pub to: Option<AccountId>,
}

/// One side of a simple entry as posting checks it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RoleAccount {
    /// The account on this side.
    pub(crate) id: AccountId,
    /// The part the account plays, as an error names it.
    pub(crate) role: AccountRole,
    /// The account types this part accepts.
    pub(crate) allowed: &'static [AccountType],
}

/// What a flat set of roles lacks for its kind.
///
/// Each account variant is named for the flat field that is empty and carries
/// the part that field plays for the kind, as an error names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingPart {
    /// A bill has no bill status.
    BillStatus,
    /// The category account is missing.
    Category(AccountRole),
    /// The wallet account is missing.
    Wallet(AccountRole),
    /// The payable account is missing.
    Payable(AccountRole),
    /// The source account of a transfer is missing.
    From(AccountRole),
    /// The destination account of a transfer is missing.
    To(AccountRole),
}

impl From<MissingPart> for Error {
    /// Reports the missing part as the refusal the UI words:
    /// [`ValidationError::BillStatusRequired`], or
    /// [`ValidationError::AccountRequired`] with the part.
    fn from(missing: MissingPart) -> Self {
        match missing {
            MissingPart::BillStatus => ValidationError::BillStatusRequired.into(),
            MissingPart::Category(role)
            | MissingPart::Wallet(role)
            | MissingPart::Payable(role)
            | MissingPart::From(role)
            | MissingPart::To(role) => ValidationError::AccountRequired { role }.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Five distinct accounts, one in each flat field.
    fn all_roles() -> SimpleEntryRoleAccounts {
        SimpleEntryRoleAccounts {
            category: Some(AccountId::generate()),
            wallet: Some(AccountId::generate()),
            payable: Some(AccountId::generate()),
            from: Some(AccountId::generate()),
            to: Some(AccountId::generate()),
        }
    }

    /// The error for an account missing in the part `role`.
    fn account_required(role: AccountRole) -> Error {
        ValidationError::AccountRequired { role }.into()
    }

    #[test]
    fn each_kind_debits_and_credits_the_accounts_of_its_row() {
        let roles = all_roles();
        let (category, wallet, payable) = (roles.category, roles.wallet, roles.payable);
        let sides = |kind, status| {
            SimpleEntryAccounts::from_roles(kind, status, roles).map(|accounts| {
                (
                    Some(accounts.debit_account()),
                    Some(accounts.credit_account()),
                )
            })
        };

        assert_eq!(
            sides(SimpleEntryKind::Expense, None),
            Ok((category, wallet))
        );
        assert_eq!(sides(SimpleEntryKind::Income, None), Ok((wallet, category)));
        assert_eq!(
            sides(SimpleEntryKind::Bill, Some(SimpleBillStatus::Paid)),
            Ok((category, wallet))
        );
        assert_eq!(
            sides(SimpleEntryKind::Bill, Some(SimpleBillStatus::Unpaid)),
            Ok((category, payable))
        );
        assert_eq!(
            sides(SimpleEntryKind::Bill, Some(SimpleBillStatus::PayExisting)),
            Ok((payable, wallet))
        );
        assert_eq!(
            sides(SimpleEntryKind::Transfer, None),
            Ok((roles.to, roles.from))
        );
    }

    #[test]
    fn a_bill_without_a_status_is_refused_before_its_accounts_are_read() {
        assert_eq!(
            SimpleEntryAccounts::from_roles(
                SimpleEntryKind::Bill,
                None,
                SimpleEntryRoleAccounts::default()
            ),
            Err(ValidationError::BillStatusRequired.into())
        );
    }

    #[test]
    fn a_missing_account_is_reported_under_the_part_it_plays_for_the_kind() {
        let none = SimpleEntryRoleAccounts::default();
        let missing =
            |kind, status, roles| SimpleEntryAccounts::from_roles(kind, status, roles).map(|_| ());
        let paid = Some(SimpleBillStatus::Paid);
        let unpaid = Some(SimpleBillStatus::Unpaid);
        let pay_existing = Some(SimpleBillStatus::PayExisting);
        let with_category = SimpleEntryRoleAccounts {
            category: all_roles().category,
            ..none
        };
        let with_wallet = SimpleEntryRoleAccounts {
            wallet: all_roles().wallet,
            ..none
        };
        let with_payable = SimpleEntryRoleAccounts {
            payable: all_roles().payable,
            ..none
        };
        let with_to = SimpleEntryRoleAccounts {
            to: all_roles().to,
            ..none
        };

        // With nothing filled in, the debited part is the one reported.
        for (kind, status, role) in [
            (SimpleEntryKind::Expense, None, AccountRole::Category),
            (SimpleEntryKind::Income, None, AccountRole::Deposit),
            (SimpleEntryKind::Bill, paid, AccountRole::BillCategory),
            (SimpleEntryKind::Bill, unpaid, AccountRole::BillCategory),
            (
                SimpleEntryKind::Bill,
                pay_existing,
                AccountRole::BillsPayable,
            ),
            (
                SimpleEntryKind::Transfer,
                None,
                AccountRole::TransferDestination,
            ),
        ] {
            assert_eq!(
                missing(kind, status, none),
                Err(account_required(role)),
                "{kind:?}"
            );
        }

        // With the debited part filled in, the credited one is reported.
        for (kind, status, roles, role) in [
            (
                SimpleEntryKind::Expense,
                None,
                with_category,
                AccountRole::Payment,
            ),
            (
                SimpleEntryKind::Income,
                None,
                with_wallet,
                AccountRole::Income,
            ),
            (
                SimpleEntryKind::Bill,
                paid,
                with_category,
                AccountRole::Payment,
            ),
            (
                SimpleEntryKind::Bill,
                unpaid,
                with_category,
                AccountRole::BillsPayable,
            ),
            (
                SimpleEntryKind::Bill,
                pay_existing,
                with_payable,
                AccountRole::Payment,
            ),
            (
                SimpleEntryKind::Transfer,
                None,
                with_to,
                AccountRole::TransferSource,
            ),
        ] {
            assert_eq!(
                missing(kind, status, roles),
                Err(account_required(role)),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_bill_status_on_an_entry_that_is_not_a_bill_is_dropped() {
        let accounts = SimpleEntryAccounts::from_roles(
            SimpleEntryKind::Expense,
            Some(SimpleBillStatus::Unpaid),
            all_roles(),
        )
        .unwrap();

        assert_eq!(accounts.kind(), SimpleEntryKind::Expense);
        assert_eq!(accounts.bill_status(), None);
    }

    #[test]
    fn the_identifier_of_a_kind_is_the_text_serde_writes() {
        for kind in [
            SimpleEntryKind::Expense,
            SimpleEntryKind::Income,
            SimpleEntryKind::Bill,
            SimpleEntryKind::Transfer,
        ] {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::from(kind.identifier())
            );
        }
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

    /// A flat set of roles with each field filled or empty.
    fn role_sets() -> impl Strategy<Value = SimpleEntryRoleAccounts> {
        let account = || prop::option::of(any::<[u8; 16]>().prop_map(account_id));

        (account(), account(), account(), account(), account()).prop_map(
            |(category, wallet, payable, from, to)| SimpleEntryRoleAccounts {
                category,
                wallet,
                payable,
                from,
                to,
            },
        )
    }

    /// The account id with these bytes.
    fn account_id(bytes: [u8; 16]) -> AccountId {
        AccountId::from(uuid::Uuid::from_bytes(bytes))
    }

    /// The two flat fields a kind and status read, as (field is filled) flags
    /// in the order category, wallet, payable, from, to; `None` for a bill
    /// without a status.
    fn needed(kind: SimpleEntryKind, status: Option<SimpleBillStatus>) -> Option<[bool; 5]> {
        match (kind, status) {
            (SimpleEntryKind::Expense | SimpleEntryKind::Income, _)
            | (SimpleEntryKind::Bill, Some(SimpleBillStatus::Paid)) => {
                Some([true, true, false, false, false])
            }
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::Unpaid)) => {
                Some([true, false, true, false, false])
            }
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::PayExisting)) => {
                Some([false, true, true, false, false])
            }
            (SimpleEntryKind::Transfer, _) => Some([false, false, false, true, true]),
            (SimpleEntryKind::Bill, None) => None,
        }
    }

    /// Which of the five flat fields are filled, in the order of [`needed`].
    fn filled(roles: SimpleEntryRoleAccounts) -> [bool; 5] {
        [
            roles.category.is_some(),
            roles.wallet.is_some(),
            roles.payable.is_some(),
            roles.from.is_some(),
            roles.to.is_some(),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn the_flat_form_converts_exactly_when_it_holds_what_the_kind_needs(
            kind in kinds(),
            status in statuses(),
            roles in role_sets(),
        ) {
            let has_what_the_kind_needs = needed(kind, status).is_some_and(|needed| {
                needed.iter().zip(filled(roles)).all(|(&needed, filled)| filled || !needed)
            });

            prop_assert_eq!(
                SimpleEntryAccounts::from_roles(kind, status, roles).is_ok(),
                has_what_the_kind_needs
            );
        }

        #[test]
        fn converting_to_the_flat_form_and_back_gives_the_same_accounts(
            kind in kinds(),
            status in statuses(),
            roles in role_sets(),
        ) {
            let Ok(accounts) = SimpleEntryAccounts::from_roles(kind, status, roles) else {
                return Ok(());
            };

            // The flat form of the typed value holds exactly the fields the
            // kind needs, each as it was given.
            let flat = accounts.roles();
            prop_assert_eq!(Some(filled(flat)), needed(kind, status));
            prop_assert_eq!(accounts.kind(), kind);

            let again =
                SimpleEntryAccounts::from_roles(accounts.kind(), accounts.bill_status(), flat);
            prop_assert_eq!(again, Ok(accounts));
        }
    }
}
