//! What the entry list and the dashboard call an entry. The rules are on
//! [`ListedEntryKind`]; this module holds them and the line fallback.

use crate::coa::default_role_codes;
use crate::db::{collect_rows, read_column, stored_id};
use crate::domain::{AccountId, AccountType, EntityId, JournalLine};
use crate::error::{AccountRole, DatabaseContext, Result};
use crate::ledger::accounts::{list_accounts, opening_balance_account};
use crate::ledger::entities::get_entity;
use crate::ledger::simple_entry::SimpleEntryKind;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The kind an entry is listed as: an expense, an income, a bill, a transfer,
/// an opening balance, or none of those. A code the UI words; it is never
/// shown as is.
///
/// Serialized as `expense`, `income`, `bill`, `transfer`, `opening_balance`
/// or `other`. The UI has a label for each code, so a new variant is a wire
/// change the UI must follow (`tests/wire_json.rs` pins the codes).
///
/// # Stored kind first
///
/// An entry posted as a simple entry stores its
/// [`SimpleEntryKind`] in
/// `journal_entries.kind` (schema v10), and that is what it is listed as. The
/// lines cannot stand in for it: a bill paid at once posts the same lines as
/// an expense. The reversing entry of a void stores the kind of the entry it
/// reverses.
///
/// # The fallback for entries without one
///
/// Entries posted before v10, journal entries posted line by line, opening
/// balances and the reversals of such entries store no kind. They are
/// classified from their lines, in this order:
///
/// 1. A line on the book's opening balance account: an opening balance.
/// 2. A line on a bill account: a bill. That is an unpaid bill or the payment
///    of one; a bill paid at once has no such line and is an expense here.
/// 3. A line on an expense account: an expense.
/// 4. A line on an income account: an income.
/// 5. Every line on an asset or a liability: a transfer.
/// 6. Anything else, such as an owner's contribution against equity: other.
///
/// The opening balance account is the equity account
/// [`set_account_opening_balance`](crate::ledger::set_account_opening_balance)
/// posts against, picked by the same function: the template's Opening
/// Balances, or in a blank book its first active equity account. It comes
/// first so that an opening balance of the bills payable account is not a
/// bill, and any other entry with a line on it, a hand-posted correction
/// included, is listed as an opening balance too.
///
/// A bill account is an account the book is set up to hold bills on: the
/// template's bills payable account, found by its code and type whether or
/// not it is archived, and the payable account of any of the book's
/// recurring bills that is unpaid or pays an existing bill. No other
/// liability is one, so a blank book has a bill account only through its
/// recurring bills. A credit card is not one unless a recurring bill holds
/// its bills there: an expense paid by card has the lines of an unpaid bill,
/// and calling every such expense a bill would mislabel more entries than it
/// fixes.
///
/// The fallback is a best guess, read from the book as it is now: creating
/// or deleting a recurring bill can change what an older entry is listed as.
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
    /// The opening balance of an account, or a change to one.
    OpeningBalance,
    /// None of the above, such as a journal entry on equity.
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
/// account and which accounts hold opening balances and bills.
#[derive(Debug, Default)]
pub(crate) struct KindFallback {
    /// The type of every account of the book, archived ones included.
    account_types: HashMap<AccountId, AccountType>,
    /// The account a line on which makes an entry an opening balance, if
    /// the book has an active equity account.
    opening_balance_account: Option<AccountId>,
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

        // By identity only, archived or not: the by-type fallback of the
        // entry form's defaults would pick the first liability, often a card,
        // and archiving the payable must not relabel the bills posted on it.
        let payable_codes = default_role_codes(entity.chart_template, AccountRole::BillsPayable);
        let mut bill_accounts = template_bill_accounts(conn, entity_id)?;
        bill_accounts.extend(
            accounts
                .iter()
                .filter(|account| {
                    account.account_type == AccountType::Liability
                        && payable_codes.contains(&account.code.as_str())
                })
                .map(|account| account.id),
        );

        let opening_balance_account = opening_balance_account(&accounts).map(|account| account.id);
        let account_types = accounts
            .iter()
            .map(|account| (account.id, account.account_type))
            .collect();
        Ok(Self {
            account_types,
            opening_balance_account,
            bill_accounts,
        })
    }

    /// Classifies an entry that stores no kind from its lines, by the rules
    /// on [`ListedEntryKind`].
    pub(crate) fn classify(&self, lines: &[JournalLine]) -> ListedEntryKind {
        if lines
            .iter()
            .any(|line| Some(line.account_id) == self.opening_balance_account)
        {
            return ListedEntryKind::OpeningBalance;
        }
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

    /// A book of one account of each type, with `bills` as its bill account
    /// and `opening` as its opening balance account.
    struct Book {
        fallback: KindFallback,
        asset: AccountId,
        liability: AccountId,
        bills: AccountId,
        opening: AccountId,
        equity: AccountId,
        income: AccountId,
        expense: AccountId,
    }

    fn book() -> Book {
        let [asset, liability, bills, opening, equity, income, expense] =
            std::array::from_fn(|_| AccountId::generate());
        let account_types = HashMap::from([
            (asset, AccountType::Asset),
            (liability, AccountType::Liability),
            (bills, AccountType::Liability),
            (opening, AccountType::Equity),
            (equity, AccountType::Equity),
            (income, AccountType::Income),
            (expense, AccountType::Expense),
        ]);
        Book {
            fallback: KindFallback {
                account_types,
                opening_balance_account: Some(opening),
                bill_accounts: HashSet::from([bills]),
            },
            asset,
            liability,
            bills,
            opening,
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
    fn a_line_on_the_opening_balance_account_makes_an_opening_balance() {
        let book = book();

        assert_eq!(
            classify(&book, book.asset, book.opening),
            ListedEntryKind::OpeningBalance
        );
        assert_eq!(
            classify(&book, book.opening, book.liability),
            ListedEntryKind::OpeningBalance
        );
        // The opening balance of the bill account is not a bill.
        assert_eq!(
            classify(&book, book.opening, book.bills),
            ListedEntryKind::OpeningBalance
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
