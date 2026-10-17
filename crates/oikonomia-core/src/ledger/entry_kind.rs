//! What the entry list and the dashboard call an entry, and the simple entry
//! form an edit of it opens on. The rules are on [`ListedEntryKind`] and
//! [`LineFallback::simple_form`]; this module holds them and the line
//! fallback they share for entries that store too little.

use crate::coa::default_role_codes;
use crate::db::{collect_rows, read_column, stored_id};
use crate::domain::{AccountId, AccountType, EntityId, JournalLine, Side};
use crate::error::{AccountRole, DatabaseContext, Result};
use crate::ledger::accounts::{list_accounts, opening_balance_account};
use crate::ledger::entities::get_entity;
use crate::ledger::simple_entry::{
    RoleAccount, SimpleBillStatus, SimpleEntryAccounts, SimpleEntryKind,
};
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

impl ListedEntryKind {
    /// Returns the simple-entry kind of the same name, `None` for
    /// an opening balance and for [`Other`](Self::Other).
    const fn simple_kind(self) -> Option<SimpleEntryKind> {
        match self {
            Self::Expense => Some(SimpleEntryKind::Expense),
            Self::Income => Some(SimpleEntryKind::Income),
            Self::Bill => Some(SimpleEntryKind::Bill),
            Self::Transfer => Some(SimpleEntryKind::Transfer),
            Self::OpeningBalance | Self::Other => None,
        }
    }
}

/// What a simple entry was posted as: the `kind` and `bill_status` columns of
/// `journal_entries`.
///
/// An entry that stores no kind has no `PostedAs` at all, whatever its status
/// column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PostedAs {
    /// The kind it was posted as (schema v10).
    pub(crate) kind: SimpleEntryKind,
    /// The bill status it was posted with (schema v11): `None` for an entry
    /// whose accounts have none, and for a bill posted before v11.
    pub(crate) bill_status: Option<SimpleBillStatus>,
}

impl PostedAs {
    /// Returns whether the form of an entry posted as this needs the line
    /// fallback: a bill always has a status from v11, so a bill without one
    /// was posted before.
    fn lacks_status(self) -> bool {
        self.kind == SimpleEntryKind::Bill && self.bill_status.is_none()
    }
}

impl From<SimpleEntryAccounts> for PostedAs {
    fn from(accounts: SimpleEntryAccounts) -> Self {
        Self {
            kind: accounts.kind(),
            bill_status: accounts.bill_status(),
        }
    }
}

/// Returns whether reading an entry posted as `posted_as` needs a loaded
/// [`LineFallback`]: one that stores no kind, or a bill posted before v11.
fn needs_fallback(posted_as: Option<PostedAs>) -> bool {
    posted_as.is_none_or(PostedAs::lacks_status)
}

/// What the line fallback needs to know about one book: the type of each
/// account, which accounts hold opening balances, bills and receivables.
#[derive(Debug, Default)]
pub(crate) struct LineFallback {
    /// The type of every account of the book, archived ones included.
    account_types: HashMap<AccountId, AccountType>,
    /// The account a line on which makes an entry an opening balance, if
    /// the book has an active equity account.
    opening_balance_account: Option<AccountId>,
    /// The accounts a line on which makes an entry a bill.
    bill_accounts: HashSet<AccountId>,
    /// The template's receivable accounts: an income debited to one of them
    /// is read as invoiced and not yet received.
    receivable_accounts: HashSet<AccountId>,
}

impl LineFallback {
    /// Loads the fallback for the book of `entity_id` when an entry posted as
    /// one of `posted` needs it, and returns an empty one otherwise, which
    /// reads nothing of the book.
    ///
    /// Every reader of entries takes its fallback from here, so none can
    /// pass an empty one for an entry that needs the book.
    ///
    /// # Errors
    ///
    /// Those of [`load`](Self::load), only when an entry needs it.
    pub(crate) fn load_for(
        conn: &Connection,
        entity_id: EntityId,
        posted: impl IntoIterator<Item = Option<PostedAs>>,
    ) -> Result<Self> {
        if posted.into_iter().any(needs_fallback) {
            Self::load(conn, entity_id)
        } else {
            Ok(Self::default())
        }
    }

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

        let receivable_codes = default_role_codes(entity.chart_template, AccountRole::Receivable);
        let receivable_accounts = accounts
            .iter()
            .filter(|account| {
                account.account_type == AccountType::Asset
                    && receivable_codes.contains(&account.code.as_str())
            })
            .map(|account| account.id)
            .collect();

        let opening_balance_account = opening_balance_account(&accounts).map(|account| account.id);

        let account_types = accounts
            .iter()
            .map(|account| (account.id, account.account_type))
            .collect();
        Ok(Self {
            account_types,
            opening_balance_account,
            bill_accounts,
            receivable_accounts,
        })
    }

    /// Returns what an entry posted as `posted_as` with `lines` is listed
    /// as: the stored kind, or the one [`classify`](Self::classify) gives.
    pub(crate) fn listed_kind(
        &self,
        posted_as: Option<PostedAs>,
        lines: &[JournalLine],
    ) -> ListedEntryKind {
        posted_as.map_or_else(|| self.classify(lines), |posted| posted.kind.into())
    }

    /// Returns the simple entry form that describes an entry posted as
    /// `posted_as` with `lines`, or `None` when no form does.
    ///
    /// # Stored form first
    ///
    /// An entry posted as a simple entry since v11 stores its kind and bill
    /// status, and with its debit and credit line they give the form exactly
    /// ([`SimpleEntryAccounts::from_sides`]); posting checked the account
    /// types then, and an account's type never changes.
    ///
    /// # The fallback
    ///
    /// A bill posted under v10 stores no status, and an entry posted before
    /// v10, or line by line, stores no kind. Its kind is the stored one or
    /// what [`classify`](Self::classify) lists it as, and its status is read
    /// from its lines:
    ///
    /// - A bill that debits an expense is unpaid when it credits a bill
    ///   account and paid otherwise. A bill that debits anything else pays an
    ///   existing bill.
    /// - An income debited to the template's receivable account is invoiced
    ///   and not yet received.
    ///
    /// An income that stores its kind and no status reads as received, as
    /// one posted since v11 is. One invoiced under v10 reads that way too,
    /// with the receivable as its wallet: v10 was in no release, and saving
    /// that form posts the same lines.
    ///
    /// The form found that way is kept only when each account has a type
    /// its part accepts and the two differ, the checks posting makes, so an
    /// entry the simple form cannot post, such as an opening balance, has
    /// none.
    ///
    /// Only an entry of exactly one debit line and one credit line has a
    /// form.
    pub(crate) fn simple_form(
        &self,
        posted_as: Option<PostedAs>,
        lines: &[JournalLine],
    ) -> Option<SimpleEntryAccounts> {
        let (debit, credit) = debit_and_credit(lines)?;

        let kind = match posted_as {
            Some(posted) if !posted.lacks_status() => {
                return SimpleEntryAccounts::from_sides(
                    posted.kind,
                    posted.bill_status,
                    debit,
                    credit,
                );
            }
            Some(posted) => posted.kind,
            None => self.classify(lines).simple_kind()?,
        };
        let status = self.line_status(kind, debit, credit);
        SimpleEntryAccounts::from_sides(kind, status, debit, credit)
            .filter(|form| self.accepts(form))
    }

    /// Reads the bill status of an entry of `kind` from its lines, by the
    /// rules on [`simple_form`](Self::simple_form).
    fn line_status(
        &self,
        kind: SimpleEntryKind,
        debit: AccountId,
        credit: AccountId,
    ) -> Option<SimpleBillStatus> {
        match kind {
            SimpleEntryKind::Bill => {
                if self.account_types.get(&debit) != Some(&AccountType::Expense) {
                    Some(SimpleBillStatus::PayExisting)
                } else if self.bill_accounts.contains(&credit) {
                    Some(SimpleBillStatus::Unpaid)
                } else {
                    Some(SimpleBillStatus::Paid)
                }
            }
            SimpleEntryKind::Income => self
                .receivable_accounts
                .contains(&debit)
                .then_some(SimpleBillStatus::Unpaid),
            SimpleEntryKind::Expense | SimpleEntryKind::Transfer => None,
        }
    }

    /// Returns whether posting `form` would pass the type checks: each
    /// account has a type its part accepts, and the two accounts differ.
    fn accepts(&self, form: &SimpleEntryAccounts) -> bool {
        let (debit, credit) = form.sides();
        let fits = |side: &RoleAccount| {
            self.account_types
                .get(&side.id)
                .is_some_and(|account_type| side.allowed.contains(account_type))
        };
        debit.id != credit.id && fits(&debit) && fits(&credit)
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

/// Returns the account of the debit line and of the credit line of an entry
/// of exactly those two lines, and `None` for any other entry.
fn debit_and_credit(lines: &[JournalLine]) -> Option<(AccountId, AccountId)> {
    match lines {
        [first, second] => match (first.side, second.side) {
            (Side::Debit, Side::Credit) => Some((first.account_id, second.account_id)),
            (Side::Credit, Side::Debit) => Some((second.account_id, first.account_id)),
            (Side::Debit, Side::Debit) | (Side::Credit, Side::Credit) => None,
        },
        _ => None,
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
    use crate::domain::{JournalEntryId, JournalLineId};
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
    /// `opening` as its opening balance account and `receivable` as its
    /// receivable account.
    struct Book {
        fallback: LineFallback,
        asset: AccountId,
        receivable: AccountId,
        liability: AccountId,
        bills: AccountId,
        opening: AccountId,
        equity: AccountId,
        income: AccountId,
        expense: AccountId,
    }

    fn book() -> Book {
        let [
            asset,
            receivable,
            liability,
            bills,
            opening,
            equity,
            income,
            expense,
        ] = std::array::from_fn(|_| AccountId::generate());
        let account_types = HashMap::from([
            (asset, AccountType::Asset),
            (receivable, AccountType::Asset),
            (liability, AccountType::Liability),
            (bills, AccountType::Liability),
            (opening, AccountType::Equity),
            (equity, AccountType::Equity),
            (income, AccountType::Income),
            (expense, AccountType::Expense),
        ]);
        Book {
            fallback: LineFallback {
                account_types,
                opening_balance_account: Some(opening),
                bill_accounts: HashSet::from([bills]),
                receivable_accounts: HashSet::from([receivable]),
            },
            asset,
            receivable,
            liability,
            bills,
            opening,
            equity,
            income,
            expense,
        }
    }

    fn two_lines(debit: AccountId, credit: AccountId) -> [JournalLine; 2] {
        [line(debit, Side::Debit), line(credit, Side::Credit)]
    }

    fn classify(book: &Book, debit: AccountId, credit: AccountId) -> ListedEntryKind {
        book.fallback.classify(&two_lines(debit, credit))
    }

    /// The form of an entry posted as `posted_as` that debits `debit` and
    /// credits `credit`, read with `fallback`.
    fn form(
        fallback: &LineFallback,
        posted_as: Option<PostedAs>,
        debit: AccountId,
        credit: AccountId,
    ) -> Option<SimpleEntryAccounts> {
        fallback.simple_form(posted_as, &two_lines(debit, credit))
    }

    /// A bill posted under v10: its kind is stored and its status is not.
    const V10_BILL: Option<PostedAs> = Some(PostedAs {
        kind: SimpleEntryKind::Bill,
        bill_status: None,
    });

    #[test]
    fn a_stored_form_is_read_from_the_lines_without_the_fallback() {
        let book = book();
        // A paid bill whose wallet is a liability has the lines of an unpaid
        // one; only the stored status tells them apart.
        let paid_by_card = SimpleEntryAccounts::BillPaid {
            category: book.expense,
            wallet: book.bills,
        };

        let read = form(
            &LineFallback::default(),
            Some(PostedAs::from(paid_by_card)),
            book.expense,
            book.bills,
        );

        assert_eq!(read, Some(paid_by_card));
        assert!(!needs_fallback(Some(PostedAs::from(paid_by_card))));
    }

    #[test]
    fn a_v10_bill_takes_its_status_from_its_lines() {
        let book = book();
        let fallback = &book.fallback;

        assert!(needs_fallback(V10_BILL));
        assert_eq!(
            form(fallback, V10_BILL, book.expense, book.bills),
            Some(SimpleEntryAccounts::BillUnpaid {
                category: book.expense,
                payable: book.bills,
            })
        );
        // A card is not a bill account, so a bill credited to one was paid.
        assert_eq!(
            form(fallback, V10_BILL, book.expense, book.liability),
            Some(SimpleEntryAccounts::BillPaid {
                category: book.expense,
                wallet: book.liability,
            })
        );
        assert_eq!(
            form(fallback, V10_BILL, book.bills, book.asset),
            Some(SimpleEntryAccounts::BillPayment {
                payable: book.bills,
                wallet: book.asset,
            })
        );
    }

    #[test]
    fn an_income_that_stores_no_status_reads_as_received() {
        let book = book();
        let income = Some(PostedAs {
            kind: SimpleEntryKind::Income,
            bill_status: None,
        });

        // Even on the receivable: only a stored status says it is owed.
        assert!(!needs_fallback(income));
        assert_eq!(
            form(
                &LineFallback::default(),
                income,
                book.receivable,
                book.income
            ),
            Some(SimpleEntryAccounts::Income {
                category: book.income,
                wallet: book.receivable,
            })
        );
    }

    #[test]
    fn an_entry_without_a_kind_takes_the_form_its_lines_suggest() {
        let book = book();
        let fallback = &book.fallback;

        assert!(needs_fallback(None));
        assert_eq!(
            form(fallback, None, book.expense, book.asset),
            Some(SimpleEntryAccounts::Expense {
                category: book.expense,
                wallet: book.asset,
            })
        );
        assert_eq!(
            form(fallback, None, book.receivable, book.income),
            Some(SimpleEntryAccounts::IncomeReceivable {
                category: book.income,
                receivable: book.receivable,
            })
        );
        assert_eq!(
            form(fallback, None, book.asset, book.income),
            Some(SimpleEntryAccounts::Income {
                category: book.income,
                wallet: book.asset,
            })
        );
        assert_eq!(
            form(fallback, None, book.liability, book.asset),
            Some(SimpleEntryAccounts::Transfer {
                from: book.asset,
                to: book.liability,
            })
        );
    }

    #[test]
    fn an_entry_the_simple_form_cannot_post_has_no_form() {
        let book = book();
        let fallback = &book.fallback;

        // An opening balance, against equity.
        assert_eq!(form(fallback, None, book.asset, book.equity), None);
        // Listed as an expense, but the credit is an income, which no wallet
        // accepts.
        assert_eq!(form(fallback, None, book.expense, book.income), None);
        // A manual entry of three lines.
        let three = [
            line(book.expense, Side::Debit),
            line(book.expense, Side::Debit),
            line(book.asset, Side::Credit),
        ];
        assert_eq!(fallback.simple_form(None, &three), None);
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
