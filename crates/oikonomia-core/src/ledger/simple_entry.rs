//! The accounts of a simple entry: which two a kind of entry needs, and
//! which of them is debited.
//!
//! The simple entry form and a recurring template both describe an entry by
//! its kind and by accounts named for the part they play, not by debit and
//! credit. This module is the one place that knows what each kind needs.
//!
//! # The seven cases
//!
//! A simple entry is one of seven shapes ([`SimpleEntryShape`]): an expense,
//! an income received or still to be received, a bill paid, owed or paid
//! off, or a transfer. [`SimpleEntryAccounts`] has one variant per shape, so
//! a value of it always names exactly the two accounts its shape posts to.
//! Its documentation has the table of what each variant debits and credits.
//!
//! # The flat form
//!
//! On the wire, in the `recurring_templates` table and in the preferences
//! file, the same information is flat: a kind, an optional bill status, and
//! five optional accounts ([`SimpleEntryRoleAccounts`]). That form can say
//! things that mean nothing, such as a transfer with a category or a bill
//! with no status. `journal_entries` stores the kind and the status of the
//! same pair; its lines hold the accounts.
//!
//! The pair of a kind and a status converts to a shape in one place,
//! [`SimpleEntryShape::from_pair`], and back in one place,
//! [`SimpleEntryShape::kind`] and [`SimpleEntryShape::bill_status`]. A shape
//! and the five accounts convert to [`SimpleEntryAccounts`] through
//! [`SimpleEntryAccounts::from_roles`], and back through
//! [`SimpleEntryAccounts::shape`] and [`SimpleEntryAccounts::roles`].
//!
//! Converting reads only what the shape needs. An account the flat form
//! holds for a part the shape does not have is dropped, and so is a bill
//! status on an entry that is neither a bill nor an income. An income is the
//! one other kind that reads the status, and only for `unpaid`: income not
//! yet received.
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
    /// Money received, or invoiced and still to be received.
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

    /// Returns the kind whose [`identifier`](Self::identifier) is `text`, or
    /// `None` for any other text.
    #[must_use]
    pub fn from_identifier(text: &str) -> Option<Self> {
        match text {
            "expense" => Some(Self::Expense),
            "income" => Some(Self::Income),
            "bill" => Some(Self::Bill),
            "transfer" => Some(Self::Transfer),
            _ => None,
        }
    }
}

/// Payment state for [`SimpleEntryKind::Bill`], and for
/// [`SimpleEntryKind::Income`], where only [`Unpaid`](Self::Unpaid) has a
/// meaning: the income is invoiced and not yet received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleBillStatus {
    /// Paid immediately from a wallet account.
    Paid,
    /// Recorded as owed against a payable account. For an income: recorded as
    /// owed to the book against a receivable account.
    Unpaid,
    /// Settle a previously recorded payable from a wallet account.
    PayExisting,
}

impl SimpleBillStatus {
    /// Returns the status as the UI and the vault write it: `paid`, `unpaid`
    /// or `pay_existing`.
    ///
    /// This is the text serde writes and the text stored in
    /// `recurring_templates.bill_status`, so it is part of the vault format.
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Paid => "paid",
            Self::Unpaid => "unpaid",
            Self::PayExisting => "pay_existing",
        }
    }

    /// Returns the status whose [`identifier`](Self::identifier) is `text`,
    /// or `None` for any other text.
    #[must_use]
    pub fn from_identifier(text: &str) -> Option<Self> {
        match text {
            "paid" => Some(Self::Paid),
            "unpaid" => Some(Self::Unpaid),
            "pay_existing" => Some(Self::PayExisting),
            _ => None,
        }
    }
}

/// What a simple entry is: its kind and, where the kind has one, its
/// payment state. One variant per combination that means something:
///
/// | Shape | Kind | Bill status |
/// |-------|------|-------------|
/// | `Expense` | expense | none |
/// | `Income` | income | none |
/// | `IncomeReceivable` | income | unpaid |
/// | `BillPaid` | bill | paid |
/// | `BillUnpaid` | bill | unpaid |
/// | `BillPayment` | bill | pay existing |
/// | `Transfer` | transfer | none |
///
/// The vault and the wire hold a shape as that pair: the `kind` and
/// `bill_status` columns of `journal_entries` and `recurring_templates`, and
/// the `kind` and `bill_status` fields of the JSON. It serializes as those
/// two fields, so a struct that flattens it reads as the pair.
///
/// # Examples
///
/// ```
/// use oikonomia_core::ledger::{SimpleBillStatus, SimpleEntryKind, SimpleEntryShape};
///
/// let unpaid = Some(SimpleBillStatus::Unpaid);
/// let shape = SimpleEntryShape::from_pair(SimpleEntryKind::Bill, unpaid);
/// assert_eq!(shape, Some(SimpleEntryShape::BillUnpaid));
///
/// // A status means nothing on an expense, so it is dropped.
/// let expense = SimpleEntryShape::from_pair(SimpleEntryKind::Expense, unpaid);
/// assert_eq!(expense.map(SimpleEntryShape::bill_status), Some(None));
///
/// // A bill must say whether it is paid.
/// assert_eq!(SimpleEntryShape::from_pair(SimpleEntryKind::Bill, None), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    into = "crate::ledger::wire::SimpleEntryShapeWire",
    try_from = "crate::ledger::wire::SimpleEntryShapeWire"
)]
pub enum SimpleEntryShape {
    /// Money spent now.
    Expense,
    /// Money received.
    Income,
    /// Income invoiced and not yet received.
    IncomeReceivable,
    /// A bill paid at once.
    BillPaid,
    /// A bill recorded as owed.
    BillUnpaid,
    /// A payment of a bill recorded earlier.
    BillPayment,
    /// Money moved between the entity's own accounts.
    Transfer,
}

impl SimpleEntryShape {
    /// Returns the shape of an entry of `kind` with `bill_status`, and `None`
    /// for a bill without a status, the one pair no shape stands for.
    ///
    /// The status is read only for a bill, and for an income, where only
    /// `unpaid` has a meaning; any other status is dropped.
    #[must_use]
    pub const fn from_pair(
        kind: SimpleEntryKind,
        bill_status: Option<SimpleBillStatus>,
    ) -> Option<Self> {
        let shape = match (kind, bill_status) {
            (SimpleEntryKind::Expense, _) => Self::Expense,
            (SimpleEntryKind::Income, Some(SimpleBillStatus::Unpaid)) => Self::IncomeReceivable,
            (
                SimpleEntryKind::Income,
                None | Some(SimpleBillStatus::Paid | SimpleBillStatus::PayExisting),
            ) => Self::Income,
            (SimpleEntryKind::Bill, None) => return None,
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::Paid)) => Self::BillPaid,
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::Unpaid)) => Self::BillUnpaid,
            (SimpleEntryKind::Bill, Some(SimpleBillStatus::PayExisting)) => Self::BillPayment,
            (SimpleEntryKind::Transfer, _) => Self::Transfer,
        };
        Some(shape)
    }

    /// Returns the kind of an entry of this shape.
    #[must_use]
    pub const fn kind(self) -> SimpleEntryKind {
        match self {
            Self::Expense => SimpleEntryKind::Expense,
            Self::Income | Self::IncomeReceivable => SimpleEntryKind::Income,
            Self::BillPaid | Self::BillUnpaid | Self::BillPayment => SimpleEntryKind::Bill,
            Self::Transfer => SimpleEntryKind::Transfer,
        }
    }

    /// Returns the payment state of a bill or of an income not yet received,
    /// and `None` for any other shape.
    #[must_use]
    pub const fn bill_status(self) -> Option<SimpleBillStatus> {
        match self {
            Self::BillPaid => Some(SimpleBillStatus::Paid),
            Self::BillUnpaid | Self::IncomeReceivable => Some(SimpleBillStatus::Unpaid),
            Self::BillPayment => Some(SimpleBillStatus::PayExisting),
            Self::Expense | Self::Income | Self::Transfer => None,
        }
    }
}

/// The two accounts of a simple entry, named for the part each plays.
///
/// Each variant is the [`SimpleEntryShape`] of the same name with the
/// accounts that shape needs, so a value says which accounts are posted to
/// and which is debited:
///
/// | Variant | Debit | Credit |
/// |---------|-------|--------|
/// | `Expense` | category (expense) | wallet (asset or liability) |
/// | `Income` | wallet (asset) | category (income) |
/// | `IncomeReceivable` | receivable (asset) | category (income) |
/// | `BillPaid` | category (expense) | wallet (asset or liability) |
/// | `BillUnpaid` | category (expense) | payable (liability) |
/// | `BillPayment` | payable (liability) | wallet (asset or liability) |
/// | `Transfer` | to (asset or liability) | from (asset or liability) |
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
/// use oikonomia_core::ledger::{SimpleEntryAccounts, SimpleEntryRoleAccounts, SimpleEntryShape};
///
/// let (power, bills_payable) = (AccountId::generate(), AccountId::generate());
///
/// // An unpaid bill debits its category and credits the payable account.
/// let unpaid = SimpleEntryAccounts::BillUnpaid { category: power, payable: bills_payable };
/// assert_eq!(unpaid.shape(), SimpleEntryShape::BillUnpaid);
/// assert_eq!((unpaid.debit_account(), unpaid.credit_account()), (power, bills_payable));
///
/// // The flat form the UI sends converts to the same value.
/// let roles = SimpleEntryRoleAccounts {
///     category: Some(power),
///     payable: Some(bills_payable),
///     ..SimpleEntryRoleAccounts::default()
/// };
/// assert_eq!(SimpleEntryAccounts::from_roles(SimpleEntryShape::BillUnpaid, roles), Ok(unpaid));
/// assert_eq!(unpaid.roles(), roles);
/// ```
///
/// It serializes in the flat form of the simple entry form's fields: the
/// kind, the bill status and the five role accounts side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    into = "crate::ledger::wire::SimpleEntryAccountsWire",
    try_from = "crate::ledger::wire::SimpleEntryAccountsWire"
)]
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
    /// Income invoiced and not yet received: the receivable account is debited
    /// and the category credited. Its flat form holds the receivable account
    /// in the `payable` part, the one part of the open item a kind has.
    IncomeReceivable {
        /// The income account the invoice is for.
        category: AccountId,
        /// The asset account the money is owed on.
        receivable: AccountId,
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
    /// Builds the accounts of an entry of `shape` from the five optional
    /// accounts of the flat form.
    ///
    /// Only the two accounts the shape needs are read.
    ///
    /// # Errors
    ///
    /// [`ValidationError::AccountRequired`], naming the part, when an account
    /// the shape needs is `None`. Of two missing accounts the debited one is
    /// reported.
    pub fn from_roles(shape: SimpleEntryShape, roles: SimpleEntryRoleAccounts) -> Result<Self> {
        Self::from_roles_or_missing_part(shape, roles).map_err(Error::from)
    }

    /// Builds the accounts from the flat form, saying which part of it is
    /// missing when it does not hold what the shape needs.
    ///
    /// The row mapper of a stored template uses this to name the damaged
    /// column; [`SimpleEntryAccounts::from_roles`] turns the same answer
    /// into a validation error.
    pub(crate) fn from_roles_or_missing_part(
        shape: SimpleEntryShape,
        roles: SimpleEntryRoleAccounts,
    ) -> std::result::Result<Self, MissingPart> {
        use AccountRole::{
            BillCategory, BillsPayable, Category, Deposit, Income, Payment, Receivable,
            TransferDestination, TransferSource,
        };

        // In every arm the debited account is required before the credited
        // one, so that of two missing accounts the same one is reported as
        // when the accounts were looked up debit first.
        match shape {
            SimpleEntryShape::Expense => Ok(Self::Expense {
                category: roles.category.ok_or(MissingPart::Category(Category))?,
                wallet: roles.wallet.ok_or(MissingPart::Wallet(Payment))?,
            }),
            SimpleEntryShape::IncomeReceivable => {
                let receivable = roles.payable.ok_or(MissingPart::Payable(Receivable))?;
                let category = roles.category.ok_or(MissingPart::Category(Income))?;
                Ok(Self::IncomeReceivable {
                    category,
                    receivable,
                })
            }
            SimpleEntryShape::Income => {
                let wallet = roles.wallet.ok_or(MissingPart::Wallet(Deposit))?;
                let category = roles.category.ok_or(MissingPart::Category(Income))?;
                Ok(Self::Income { category, wallet })
            }
            SimpleEntryShape::BillPaid => Ok(Self::BillPaid {
                category: roles.category.ok_or(MissingPart::Category(BillCategory))?,
                wallet: roles.wallet.ok_or(MissingPart::Wallet(Payment))?,
            }),
            SimpleEntryShape::BillUnpaid => Ok(Self::BillUnpaid {
                category: roles.category.ok_or(MissingPart::Category(BillCategory))?,
                payable: roles.payable.ok_or(MissingPart::Payable(BillsPayable))?,
            }),
            SimpleEntryShape::BillPayment => Ok(Self::BillPayment {
                payable: roles.payable.ok_or(MissingPart::Payable(BillsPayable))?,
                wallet: roles.wallet.ok_or(MissingPart::Wallet(Payment))?,
            }),
            SimpleEntryShape::Transfer => {
                let to = roles
                    .to
                    .ok_or(MissingPart::TransferDestination(TransferDestination))?;
                let from = roles
                    .from
                    .ok_or(MissingPart::TransferSource(TransferSource))?;
                Ok(Self::Transfer { from, to })
            }
        }
    }

    /// Builds the accounts of a posted entry of `shape` from the account it
    /// debits and the one it credits: the reverse of
    /// [`debit_account`](Self::debit_account) and
    /// [`credit_account`](Self::credit_account).
    ///
    /// The account types are not checked; the caller knows whether they
    /// were.
    pub(crate) const fn from_sides(
        shape: SimpleEntryShape,
        debit: AccountId,
        credit: AccountId,
    ) -> Self {
        match shape {
            SimpleEntryShape::Expense => Self::Expense {
                category: debit,
                wallet: credit,
            },
            SimpleEntryShape::Income => Self::Income {
                category: credit,
                wallet: debit,
            },
            SimpleEntryShape::IncomeReceivable => Self::IncomeReceivable {
                category: credit,
                receivable: debit,
            },
            SimpleEntryShape::BillPaid => Self::BillPaid {
                category: debit,
                wallet: credit,
            },
            SimpleEntryShape::BillUnpaid => Self::BillUnpaid {
                category: debit,
                payable: credit,
            },
            SimpleEntryShape::BillPayment => Self::BillPayment {
                payable: debit,
                wallet: credit,
            },
            SimpleEntryShape::Transfer => Self::Transfer {
                from: credit,
                to: debit,
            },
        }
    }

    /// Returns the shape of entry these accounts belong to.
    #[must_use]
    pub const fn shape(&self) -> SimpleEntryShape {
        match self {
            Self::Expense { .. } => SimpleEntryShape::Expense,
            Self::Income { .. } => SimpleEntryShape::Income,
            Self::IncomeReceivable { .. } => SimpleEntryShape::IncomeReceivable,
            Self::BillPaid { .. } => SimpleEntryShape::BillPaid,
            Self::BillUnpaid { .. } => SimpleEntryShape::BillUnpaid,
            Self::BillPayment { .. } => SimpleEntryShape::BillPayment,
            Self::Transfer { .. } => SimpleEntryShape::Transfer,
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
            Self::IncomeReceivable {
                category,
                receivable,
            } => SimpleEntryRoleAccounts {
                category: Some(category),
                payable: Some(receivable),
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
            Self::IncomeReceivable {
                category,
                receivable,
            } => (
                side(receivable, AccountRole::Receivable, &[Asset]),
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
/// This is the layout the five `*_account_id` fields of a request, the five
/// role columns of `recurring_templates` and a
/// [`LastRoleAccounts`](crate::prefs::LastRoleAccounts) of the preferences
/// file all share. On its own it says nothing about which accounts belong
/// together; [`SimpleEntryAccounts::from_roles`] reads it for a shape.
///
/// It serializes as the five fields under their wire names, so a struct that
/// flattens it reads as the five fields.
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
/// | `payable` | `payable_account_id` | `bills_payable`, or `receivable` for an unpaid income |
/// | `from` | `from_account_id` | `transfer_source` |
/// | `to` | `to_account_id` | `transfer_destination` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SimpleEntryRoleAccounts {
    /// Expense or income category account (`category_account_id`).
    #[serde(rename = "category_account_id")]
    pub category: Option<AccountId>,
    /// Bank, cash or card account (`wallet_account_id`).
    #[serde(rename = "wallet_account_id")]
    pub wallet: Option<AccountId>,
    /// Bills payable liability account, or the receivable asset account of an
    /// unpaid income (`payable_account_id`).
    #[serde(rename = "payable_account_id")]
    pub payable: Option<AccountId>,
    /// Account a transfer takes from (`from_account_id`).
    #[serde(rename = "from_account_id")]
    pub from: Option<AccountId>,
    /// Account a transfer pays into (`to_account_id`).
    #[serde(rename = "to_account_id")]
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

/// What a flat set of roles lacks for its shape.
///
/// Each variant is named for the flat field that is empty and carries the
/// part that field plays for the shape, as an error names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingPart {
    /// The category account is missing.
    Category(AccountRole),
    /// The wallet account is missing.
    Wallet(AccountRole),
    /// The payable account is missing.
    Payable(AccountRole),
    /// The source account of a transfer is missing.
    TransferSource(AccountRole),
    /// The destination account of a transfer is missing.
    TransferDestination(AccountRole),
}

impl From<MissingPart> for Error {
    /// Reports the missing part as the refusal the UI words:
    /// [`ValidationError::AccountRequired`] with the part.
    fn from(missing: MissingPart) -> Self {
        match missing {
            MissingPart::Category(role)
            | MissingPart::Wallet(role)
            | MissingPart::Payable(role)
            | MissingPart::TransferSource(role)
            | MissingPart::TransferDestination(role) => {
                ValidationError::AccountRequired { role }.into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shape.
    const SHAPES: [SimpleEntryShape; 7] = [
        SimpleEntryShape::Expense,
        SimpleEntryShape::Income,
        SimpleEntryShape::IncomeReceivable,
        SimpleEntryShape::BillPaid,
        SimpleEntryShape::BillUnpaid,
        SimpleEntryShape::BillPayment,
        SimpleEntryShape::Transfer,
    ];

    /// Every kind.
    const KINDS: [SimpleEntryKind; 4] = [
        SimpleEntryKind::Expense,
        SimpleEntryKind::Income,
        SimpleEntryKind::Bill,
        SimpleEntryKind::Transfer,
    ];

    /// Every bill status, and none.
    const STATUSES: [Option<SimpleBillStatus>; 4] = [
        None,
        Some(SimpleBillStatus::Paid),
        Some(SimpleBillStatus::Unpaid),
        Some(SimpleBillStatus::PayExisting),
    ];

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
    fn every_shape_is_stored_as_a_pair_that_reads_back_as_it() {
        for shape in SHAPES {
            assert_eq!(
                SimpleEntryShape::from_pair(shape.kind(), shape.bill_status()),
                Some(shape),
                "{shape:?}"
            );
        }
    }

    #[test]
    fn every_pair_but_a_bill_without_a_status_has_a_shape_of_its_kind() {
        for kind in KINDS {
            for status in STATUSES {
                let shape = SimpleEntryShape::from_pair(kind, status);
                let is_bill_without_status = kind == SimpleEntryKind::Bill && status.is_none();

                assert_eq!(
                    shape.is_none(),
                    is_bill_without_status,
                    "{kind:?} {status:?}"
                );
                assert!(
                    shape.is_none_or(|shape| shape.kind() == kind),
                    "{kind:?} {status:?}"
                );
            }
        }
    }

    #[test]
    fn a_bill_status_is_read_only_where_it_means_something() {
        use SimpleBillStatus::{Paid, PayExisting, Unpaid};

        for status in STATUSES {
            assert_eq!(
                SimpleEntryShape::from_pair(SimpleEntryKind::Expense, status),
                Some(SimpleEntryShape::Expense)
            );
            assert_eq!(
                SimpleEntryShape::from_pair(SimpleEntryKind::Transfer, status),
                Some(SimpleEntryShape::Transfer)
            );
        }
        for status in [None, Some(Paid), Some(PayExisting)] {
            assert_eq!(
                SimpleEntryShape::from_pair(SimpleEntryKind::Income, status),
                Some(SimpleEntryShape::Income)
            );
        }
        assert_eq!(
            SimpleEntryShape::from_pair(SimpleEntryKind::Income, Some(Unpaid)),
            Some(SimpleEntryShape::IncomeReceivable)
        );
    }

    #[test]
    fn a_shape_serializes_as_its_kind_and_bill_status() {
        for shape in SHAPES {
            let json = serde_json::to_value(shape).unwrap();

            assert_eq!(
                json,
                serde_json::json!({ "kind": shape.kind(), "bill_status": shape.bill_status() })
            );
            assert_eq!(
                serde_json::from_value::<SimpleEntryShape>(json).unwrap(),
                shape
            );
        }
        assert!(
            serde_json::from_value::<SimpleEntryShape>(
                serde_json::json!({ "kind": "bill", "bill_status": null })
            )
            .is_err()
        );
    }

    #[test]
    fn each_shape_debits_and_credits_the_accounts_of_its_row() {
        let roles = all_roles();
        let (category, wallet, payable) = (roles.category, roles.wallet, roles.payable);
        let sides = |shape| {
            SimpleEntryAccounts::from_roles(shape, roles).map(|accounts| {
                (
                    Some(accounts.debit_account()),
                    Some(accounts.credit_account()),
                )
            })
        };

        assert_eq!(sides(SimpleEntryShape::Expense), Ok((category, wallet)));
        assert_eq!(sides(SimpleEntryShape::Income), Ok((wallet, category)));
        assert_eq!(
            sides(SimpleEntryShape::IncomeReceivable),
            Ok((payable, category))
        );
        assert_eq!(sides(SimpleEntryShape::BillPaid), Ok((category, wallet)));
        assert_eq!(sides(SimpleEntryShape::BillUnpaid), Ok((category, payable)));
        assert_eq!(sides(SimpleEntryShape::BillPayment), Ok((payable, wallet)));
        assert_eq!(
            sides(SimpleEntryShape::Transfer),
            Ok((roles.to, roles.from))
        );
    }

    #[test]
    fn a_missing_account_is_reported_under_the_part_it_plays_for_the_shape() {
        use SimpleEntryShape::{
            BillPaid, BillPayment, BillUnpaid, Expense, Income, IncomeReceivable, Transfer,
        };

        let none = SimpleEntryRoleAccounts::default();
        let missing = |shape, roles| SimpleEntryAccounts::from_roles(shape, roles).map(|_| ());
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
        for (shape, role) in [
            (Expense, AccountRole::Category),
            (Income, AccountRole::Deposit),
            (IncomeReceivable, AccountRole::Receivable),
            (BillPaid, AccountRole::BillCategory),
            (BillUnpaid, AccountRole::BillCategory),
            (BillPayment, AccountRole::BillsPayable),
            (Transfer, AccountRole::TransferDestination),
        ] {
            assert_eq!(
                missing(shape, none),
                Err(account_required(role)),
                "{shape:?}"
            );
        }

        // With the debited part filled in, the credited one is reported.
        for (shape, roles, role) in [
            (Expense, with_category, AccountRole::Payment),
            (Income, with_wallet, AccountRole::Income),
            (IncomeReceivable, with_payable, AccountRole::Income),
            (BillPaid, with_category, AccountRole::Payment),
            (BillUnpaid, with_category, AccountRole::BillsPayable),
            (BillPayment, with_payable, AccountRole::Payment),
            (Transfer, with_to, AccountRole::TransferSource),
        ] {
            assert_eq!(
                missing(shape, roles),
                Err(account_required(role)),
                "{shape:?}"
            );
        }
    }

    #[test]
    fn the_identifier_of_a_kind_is_the_text_serde_writes() {
        for kind in KINDS {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::from(kind.identifier())
            );
            assert_eq!(
                SimpleEntryKind::from_identifier(kind.identifier()),
                Some(kind)
            );
        }
        assert_eq!(SimpleEntryKind::from_identifier("Expense"), None);
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// Any shape.
    fn shapes() -> impl Strategy<Value = SimpleEntryShape> {
        prop_oneof![
            Just(SimpleEntryShape::Expense),
            Just(SimpleEntryShape::Income),
            Just(SimpleEntryShape::IncomeReceivable),
            Just(SimpleEntryShape::BillPaid),
            Just(SimpleEntryShape::BillUnpaid),
            Just(SimpleEntryShape::BillPayment),
            Just(SimpleEntryShape::Transfer),
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

    /// The two flat fields a shape reads, as (field is filled) flags in the
    /// order category, wallet, payable, from, to.
    fn needed(shape: SimpleEntryShape) -> [bool; 5] {
        match shape {
            SimpleEntryShape::Expense | SimpleEntryShape::Income | SimpleEntryShape::BillPaid => {
                [true, true, false, false, false]
            }
            SimpleEntryShape::IncomeReceivable | SimpleEntryShape::BillUnpaid => {
                [true, false, true, false, false]
            }
            SimpleEntryShape::BillPayment => [false, true, true, false, false],
            SimpleEntryShape::Transfer => [false, false, false, true, true],
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
        fn the_flat_form_converts_exactly_when_it_holds_what_the_shape_needs(
            shape in shapes(),
            roles in role_sets(),
        ) {
            let has_what_the_shape_needs = needed(shape)
                .iter()
                .zip(filled(roles))
                .all(|(&needed, filled)| filled || !needed);

            prop_assert_eq!(
                SimpleEntryAccounts::from_roles(shape, roles).is_ok(),
                has_what_the_shape_needs
            );
        }

        #[test]
        fn converting_to_the_flat_form_and_back_gives_the_same_accounts(
            shape in shapes(),
            roles in role_sets(),
        ) {
            let Ok(accounts) = SimpleEntryAccounts::from_roles(shape, roles) else {
                return Ok(());
            };

            // The flat form of the typed value holds exactly the fields the
            // shape needs, each as it was given.
            let flat = accounts.roles();
            prop_assert_eq!(filled(flat), needed(shape));
            prop_assert_eq!(accounts.shape(), shape);

            let again = SimpleEntryAccounts::from_roles(accounts.shape(), flat);
            prop_assert_eq!(again, Ok(accounts));
        }

        #[test]
        fn the_sides_of_posted_accounts_give_the_same_accounts_back(
            shape in shapes(),
            roles in role_sets(),
        ) {
            let Ok(accounts) = SimpleEntryAccounts::from_roles(shape, roles) else {
                return Ok(());
            };

            let again = SimpleEntryAccounts::from_sides(
                accounts.shape(),
                accounts.debit_account(),
                accounts.credit_account(),
            );
            prop_assert_eq!(again, accounts);
        }

        #[test]
        fn accounts_serialize_flat_and_read_back(
            shape in shapes(),
            roles in role_sets(),
        ) {
            let Ok(accounts) = SimpleEntryAccounts::from_roles(shape, roles) else {
                return Ok(());
            };

            let json = serde_json::to_string(&accounts).expect("accounts serialize");
            let again: SimpleEntryAccounts =
                serde_json::from_str(&json).expect("serialized accounts read back");
            prop_assert_eq!(again, accounts);
        }
    }
}
