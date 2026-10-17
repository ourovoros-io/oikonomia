//! What the entry list and the dashboard call an entry: an expense, an
//! income, a bill, a transfer, or none of those.
//!
//! # Stored kind first
//!
//! An entry posted as a simple entry stores its [`SimpleEntryKind`] in
//! `journal_entries.kind` (schema v10), and that is what it is listed as. The
//! lines cannot stand in for it: a bill paid at once posts the same lines as
//! an expense.
//!
//! # The fallback for entries without one
//!
//! Entries posted before v10, journal entries posted line by line, void
//! reversals and opening balances store no kind. They are classified from
//! their lines ([`KindFallback::classify`]), in this order:
//!
//! 1. A line on a bill account: a bill. That is an unpaid bill or the payment
//!    of one; a bill paid at once has no such line and is an expense here.
//! 2. A line on an expense account: an expense.
//! 3. A line on an income account: an income.
//! 4. Every line on an asset or a liability: a transfer.
//! 5. Anything else, such as an opening balance against equity: other.
//!
//! A bill account is the book's default bills payable account
//! ([`default_account_for_role`] for [`AccountRole::BillsPayable`]) or the
//! payable account of any of the book's recurring bills that is unpaid or
//! pays an existing bill. Those are the accounts the book is set up to hold
//! bills on. Any other liability, such as a credit card, is not one: an
//! expense paid by card has the lines of an unpaid bill, and calling every
//! such expense a bill would mislabel more entries than it fixes.

use crate::db::{collect_rows, read_column, stored_id};
use crate::default_accounts::default_account_for_role;
use crate::domain::{AccountId, AccountType, EntityId, JournalLine};
use crate::error::{AccountRole, DatabaseContext, Result};
use crate::ledger::accounts::list_accounts;
use crate::ledger::entities::get_entity;
use crate::ledger::simple_entry::SimpleEntryKind;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The kind an entry is listed as. The UI words it; it is never shown as is.
///
/// Serialized as `expense`, `income`, `bill`, `transfer` or `other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListedEntryKind {
    /// Money spent now.
    Expense,
    /// Money received, or invoiced and still to be received.
    Income,
    /// A bill: paid, owed, or a payment against an owed bill.
    Bill,
    /// Money moved between the book's own accounts.
    Transfer,
    /// None of the above, such as an opening balance or a manual entry on
    /// equity.
    Other,
}

impl From<SimpleEntryKind> for ListedEntryKind {
    fn from(kind: SimpleEntryKind) -> Self {
        match kind {
            SimpleEntryKind::Expense => Self::Expense,
            SimpleEntryKind::Income => Self::Income,
            SimpleEntryKind::Bill => Self::Bill,
            SimpleEntryKind::Transfer => Self::Transfer,
        }
    }
}

/// What the line fallback needs to know about one book: the type of each
/// account and which accounts hold bills.
#[derive(Debug, Default)]
pub(crate) struct KindFallback {
    /// The type of every account of the book, archived ones included.
    account_types: HashMap<AccountId, AccountType>,
    /// The accounts a line on which makes an entry a bill.
    bill_accounts: HashSet<AccountId>,
}

impl KindFallback {
    /// Loads the fallback for the book of `entity_id`.
    ///
    /// # Errors
    ///
    /// - [`Error::NotFound`](crate::Error::NotFound) for an unknown entity.
    /// - [`Error::VaultCorrupt`](crate::Error::VaultCorrupt) for a stored
    ///   entity, account or template account id that does not parse.
    /// - [`Error::Database`](crate::Error::Database) on database errors.
    pub(crate) fn load(conn: &Connection, entity_id: EntityId) -> Result<Self> {
        let entity = get_entity(conn, entity_id)?;
        let accounts = list_accounts(conn, entity_id)?;

        let mut bill_accounts = template_bill_accounts(conn, entity_id)?;
        let default_payable =
            default_account_for_role(entity.chart_template, &accounts, AccountRole::BillsPayable);
        bill_accounts.extend(default_payable);

        let account_types = accounts
            .iter()
            .map(|account| (account.id, account.account_type))
            .collect();
        Ok(Self {
            account_types,
            bill_accounts,
        })
    }

    /// Classifies an entry that stores no kind from its lines, by the rules
    /// in the module documentation.
    pub(crate) fn classify(&self, lines: &[JournalLine]) -> ListedEntryKind {
        if lines
            .iter()
            .any(|line| self.bill_accounts.contains(&line.account_id))
        {
            return ListedEntryKind::Bill;
        }

        let types: Vec<Option<AccountType>> = lines
            .iter()
            .map(|line| self.account_types.get(&line.account_id).copied())
            .collect();
        if types.contains(&Some(AccountType::Expense)) {
            ListedEntryKind::Expense
        } else if types.contains(&Some(AccountType::Income)) {
            ListedEntryKind::Income
        } else if types.iter().all(|account_type| {
            matches!(
                account_type,
                Some(AccountType::Asset | AccountType::Liability)
            )
        }) {
            ListedEntryKind::Transfer
        } else {
            ListedEntryKind::Other
        }
    }
}

/// Returns the payable accounts of the book's recurring bills that post to
/// one: those recorded as unpaid and those that pay an existing bill.
///
/// A paid bill template may still hold a payable account in its flat form,
/// which posting ignores, so it is not read here.
///
/// # Errors
///
/// [`Error::VaultCorrupt`](crate::Error::VaultCorrupt) for an account id that
/// does not parse; [`Error::Database`](crate::Error::Database) on database
/// errors.
fn template_bill_accounts(conn: &Connection, entity_id: EntityId) -> Result<HashSet<AccountId>> {
    const OPERATION: &str = "list recurring bill accounts";

    let mut stmt = conn
        .prepare(
            "
            SELECT DISTINCT payable_account_id FROM recurring_templates
            WHERE entity_id = ?1
              AND kind = 'bill'
              AND bill_status IN ('unpaid', 'pay_existing')
              AND payable_account_id IS NOT NULL
            ",
        )
        .database(OPERATION)?;
    let rows = stmt
        .query_map([entity_id.to_string()], |row| {
            Ok(read_column::<String>(OPERATION, row, 0)
                .and_then(|id| stored_id("recurring_templates.payable_account_id", &id)))
        })
        .database(OPERATION)?;

    Ok(collect_rows(OPERATION, rows)?.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{JournalEntryId, JournalLineId, Side};
    use crate::money::Money;

    fn line(account_id: AccountId, side: Side) -> JournalLine {
        JournalLine {
            id: JournalLineId::generate(),
            entry_id: JournalEntryId::generate(),
            account_id,
            amount: Money::from_minor(100).unwrap(),
            side,
            memo: None,
        }
    }

    /// A book of one account of each type, with `bills` as its bill account.
    struct Book {
        fallback: KindFallback,
        asset: AccountId,
        liability: AccountId,
        bills: AccountId,
        equity: AccountId,
        income: AccountId,
        expense: AccountId,
    }

    fn book() -> Book {
        let [asset, liability, bills, equity, income, expense] =
            std::array::from_fn(|_| AccountId::generate());
        let account_types = HashMap::from([
            (asset, AccountType::Asset),
            (liability, AccountType::Liability),
            (bills, AccountType::Liability),
            (equity, AccountType::Equity),
            (income, AccountType::Income),
            (expense, AccountType::Expense),
        ]);
        Book {
            fallback: KindFallback {
                account_types,
                bill_accounts: HashSet::from([bills]),
            },
            asset,
            liability,
            bills,
            equity,
            income,
            expense,
        }
    }

    fn classify(book: &Book, debit: AccountId, credit: AccountId) -> ListedEntryKind {
        book.fallback
            .classify(&[line(debit, Side::Debit), line(credit, Side::Credit)])
    }

    #[test]
    fn a_line_on_a_bill_account_makes_a_bill() {
        let book = book();

        assert_eq!(
            classify(&book, book.expense, book.bills),
            ListedEntryKind::Bill
        );
        assert_eq!(
            classify(&book, book.bills, book.asset),
            ListedEntryKind::Bill
        );
    }

    #[test]
    fn other_lines_are_classified_by_account_type() {
        let book = book();

        assert_eq!(
            classify(&book, book.expense, book.asset),
            ListedEntryKind::Expense
        );
        // A card is a liability but not a bill account.
        assert_eq!(
            classify(&book, book.expense, book.liability),
            ListedEntryKind::Expense
        );
        assert_eq!(
            classify(&book, book.asset, book.income),
            ListedEntryKind::Income
        );
        assert_eq!(
            classify(&book, book.liability, book.asset),
            ListedEntryKind::Transfer
        );
        assert_eq!(
            classify(&book, book.asset, book.equity),
            ListedEntryKind::Other
        );
    }

    #[test]
    fn an_account_of_another_book_is_other() {
        let book = book();

        assert_eq!(
            classify(&book, book.asset, AccountId::generate()),
            ListedEntryKind::Other
        );
    }

    #[test]
    fn each_simple_kind_lists_as_its_own_code() {
        for kind in [
            SimpleEntryKind::Expense,
            SimpleEntryKind::Income,
            SimpleEntryKind::Bill,
            SimpleEntryKind::Transfer,
        ] {
            assert_eq!(
                serde_json::to_value(ListedEntryKind::from(kind)).unwrap(),
                serde_json::Value::from(kind.identifier())
            );
        }
    }
}
