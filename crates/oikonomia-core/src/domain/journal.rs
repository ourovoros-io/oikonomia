//! Journal entries, their lines, and the rule that makes an entry postable.
//!
//! # Model
//!
//! A [`JournalEntry`] is the header of one transaction: its book, date and
//! description. Its [`JournalLine`]s say where the money moved, each naming
//! one account and an amount on one side.
//!
//! A line holds one `amount` and the [`Side`] it is on, so it cannot be a
//! debit and a credit at once. The amount is a [`Money`], which is never
//! negative; nothing in the types makes it greater than zero, so that is the
//! one thing about a single line [`validate_lines_for_post`] still checks.
//!
//! # Two columns
//!
//! Outside this crate a line is written in two columns, a debit and a
//! credit, of which one is zero: as JSON for the UI, in the `journal_lines`
//! table and in the journal CSV export. [`Side::from_columns`] is the one
//! reading of that form and [`Side::columns`] the one writing of it, and a
//! [`JournalLine`] serializes through them, so the JSON keeps its `debit` and
//! `credit` fields.
//!
//! # The posting rule
//!
//! [`validate_lines_for_post`] is the double-entry invariant, checked on the
//! lines alone before anything is written:
//!
//! 1. there are at least two lines;
//! 2. every line has an amount greater than zero;
//! 3. the debits and the credits add up to the same total.
//!
//! Everything that needs the database (the accounts exist, belong to the
//! entry's entity and, except for the reversing entry of a void, are active)
//! is checked in [`crate::ledger`], which calls this function as part of
//! every post.
//!
//! # Lifecycle
//!
//! The date, description and lines of a posted entry do not change, and an
//! entry is removed only together with its whole entity. A mistake is
//! corrected by a void, which posts a second entry with the sides swapped and
//! links the two (see [`crate::ledger::void_entry`]), so the book keeps both.
//! Of the fields of a [`JournalEntry`], only `hidden` changes after posting.

use crate::domain::account::AccountId;
use crate::domain::define_id;
use crate::domain::entity::EntityId;
use crate::error::{Error, Result};
use crate::money::Money;
use serde::{Deserialize, Serialize};
use time::Date;

define_id! {
    /// Identifies one [`JournalEntry`].
    JournalEntryId
}

define_id! {
    /// Identifies one [`JournalLine`].
    JournalLineId
}

/// Whether a journal entry counts towards balances.
///
/// Serialized in `snake_case`, which is also how the status is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    /// Not posted, so left out of balances and reports. Core reads a stored
    /// draft but has no function that creates one.
    Draft,
    /// Posted: it counts towards balances and is corrected only by a void.
    Posted,
}

/// The header of a journal entry; its lines are [`JournalLine`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// The entry's own id.
    pub id: JournalEntryId,
    /// The entity whose journal this entry is in.
    pub entity_id: EntityId,
    /// The accounting date, serialized as `YYYY-MM-DD`.
    #[serde(with = "crate::util::serde_date")]
    pub entry_date: Date,
    /// What the entry is for, as the user wrote it or as core generated it.
    /// It may be empty.
    pub description: String,
    /// An outside reference such as a check or invoice number, if any.
    pub reference: Option<String>,
    /// Whether the entry is a draft or posted.
    pub status: EntryStatus,
    /// `true` when the owner marked the entry hidden. A hidden entry stays in
    /// the book and in every in-app list and balance;
    /// [`crate::csv::export_journal_csv`] and
    /// [`crate::ledger::profit_and_loss_export`] leave it out. It is not an
    /// extra layer of encryption.
    pub hidden: bool,
}

/// The side of an account a line posts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The left side: it raises an asset or an expense and lowers a
    /// liability, equity or income.
    Debit,
    /// The right side: it raises a liability, equity or income and lowers an
    /// asset or an expense.
    Credit,
}

impl Side {
    /// Returns the other side, which is where the same amount goes when an
    /// entry is reversed.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Debit => Self::Credit,
            Self::Credit => Self::Debit,
        }
    }

    /// Reads a line written in two columns: returns the side that has an
    /// amount, and the amount.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::domain::Side;
    /// use oikonomia_core::{Error, Money};
    ///
    /// let (amount, nothing) = (Money::from_minor(4_500)?, Money::ZERO);
    ///
    /// assert_eq!(Side::from_columns(amount, nothing), Ok((Side::Debit, amount)));
    /// assert_eq!(Side::from_columns(nothing, amount), Ok((Side::Credit, amount)));
    /// assert_eq!(Side::from_columns(amount, amount), Err(Error::InvalidLineAmounts));
    /// assert_eq!(Side::from_columns(nothing, nothing), Err(Error::InvalidLineAmounts));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidLineAmounts`] when both columns are zero or
    /// both are greater than zero.
    pub fn from_columns(debit: Money, credit: Money) -> Result<(Self, Money)> {
        // `Money` is never negative, so "greater than zero" is "has an
        // amount".
        match (debit.amount_minor() > 0, credit.amount_minor() > 0) {
            (true, false) => Ok((Self::Debit, debit)),
            (false, true) => Ok((Self::Credit, credit)),
            (true, true) | (false, false) => Err(Error::InvalidLineAmounts),
        }
    }

    /// Writes `amount` on this side in two columns: returns the debit and
    /// then the credit, of which the other side's is zero.
    #[must_use]
    pub const fn columns(self, amount: Money) -> (Money, Money) {
        match self {
            Self::Debit => (amount, Money::ZERO),
            Self::Credit => (Money::ZERO, amount),
        }
    }
}

/// One debit or credit of one account, as part of a [`JournalEntry`].
///
/// The amount is greater than zero on a line that
/// [`validate_lines_for_post`] accepts and on every line read from the vault.
///
/// It serializes in two columns, as the UI reads it: a `debit` and a
/// `credit`, each a [`Money`], with zero on the side the line is not on.
/// Reading that form back refuses a line with both or neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "JournalLineColumns", try_from = "JournalLineColumns")]
pub struct JournalLine {
    /// The line's own id.
    pub id: JournalLineId,
    /// The entry this line is part of.
    pub entry_id: JournalEntryId,
    /// The account the amount is debited or credited to.
    pub account_id: AccountId,
    /// The amount debited or credited.
    pub amount: Money,
    /// Whether the amount is debited or credited.
    pub side: Side,
    /// A note on this line alone, if any.
    pub memo: Option<String>,
}

impl JournalLine {
    /// Returns the amount debited, or zero when the line is a credit.
    #[must_use]
    pub const fn debit(&self) -> Money {
        self.side.columns(self.amount).0
    }

    /// Returns the amount credited, or zero when the line is a debit.
    #[must_use]
    pub const fn credit(&self) -> Money {
        self.side.columns(self.amount).1
    }
}

/// The JSON shape of a [`JournalLine`]: its amount in two columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalLineColumns {
    /// The line's own id.
    id: JournalLineId,
    /// The entry this line is part of.
    entry_id: JournalEntryId,
    /// The account the amount is debited or credited to.
    account_id: AccountId,
    /// The amount debited, or zero when the line is a credit.
    debit: Money,
    /// The amount credited, or zero when the line is a debit.
    credit: Money,
    /// A note on this line alone, if any.
    memo: Option<String>,
}

impl From<JournalLine> for JournalLineColumns {
    /// Writes the amount of `line` in the column of its side.
    fn from(line: JournalLine) -> Self {
        let (debit, credit) = line.side.columns(line.amount);

        Self {
            id: line.id,
            entry_id: line.entry_id,
            account_id: line.account_id,
            debit,
            credit,
            memo: line.memo,
        }
    }
}

impl TryFrom<JournalLineColumns> for JournalLine {
    type Error = Error;

    /// Reads the side and the amount of a line from its two columns.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidLineAmounts`] when both columns are zero or
    /// both are greater than zero.
    fn try_from(columns: JournalLineColumns) -> Result<Self> {
        let (side, amount) = Side::from_columns(columns.debit, columns.credit)?;

        Ok(Self {
            id: columns.id,
            entry_id: columns.entry_id,
            account_id: columns.account_id,
            amount,
            side,
            memo: columns.memo,
        })
    }
}

/// Checks that `lines` can be posted as one journal entry.
///
/// The lines pass when there are at least two of them, each has an amount
/// greater than zero, and the debits and credits add up to the same total.
/// Nothing else is looked at: not the accounts, not the
/// ids, not the memos.
///
/// # Examples
///
/// ```
/// use oikonomia_core::domain::{
///     AccountId, JournalEntryId, JournalLine, JournalLineId, Side, validate_lines_for_post,
/// };
/// use oikonomia_core::{Error, Money};
///
/// # fn line(entry_id: JournalEntryId, side: Side, minor: i64) -> Result<JournalLine, Error> {
/// #     Ok(JournalLine {
/// #         id: JournalLineId::generate(),
/// #         entry_id,
/// #         account_id: AccountId::generate(),
/// #         amount: Money::from_minor(minor)?,
/// #         side,
/// #         memo: None,
/// #     })
/// # }
/// let entry = JournalEntryId::generate();
///
/// // 45.00 of groceries paid from the bank: one debit, one credit.
/// let balanced = [line(entry, Side::Debit, 4_500)?, line(entry, Side::Credit, 4_500)?];
/// assert_eq!(validate_lines_for_post(&balanced), Ok(()));
///
/// let unbalanced = [line(entry, Side::Debit, 4_500)?, line(entry, Side::Credit, 4_000)?];
/// assert_eq!(
///     validate_lines_for_post(&unbalanced),
///     Err(Error::UnbalancedEntry { debits: 4_500, credits: 4_000 })
/// );
/// # Ok::<(), Error>(())
/// ```
///
/// # Errors
///
/// The first failure is returned. The line count is checked first, then the
/// lines one at a time in order, each for its amount and then for the running
/// totals, and the balance last:
///
/// - [`Error::TooFewLines`] when there are fewer than two lines.
/// - [`Error::InvalidLineAmounts`] for a line whose amount is zero.
/// - [`Error::MoneyOverflow`] when adding a line takes the total of the
///   debits or of the credits past `i64::MAX`.
/// - [`Error::UnbalancedEntry`], carrying both totals, when the total of the
///   debits differs from the total of the credits.
pub fn validate_lines_for_post(lines: &[JournalLine]) -> Result<()> {
    if lines.len() < 2 {
        return Err(Error::TooFewLines);
    }

    let mut debits: i64 = 0;
    let mut credits: i64 = 0;

    for line in lines {
        // The side is part of the type; what is left to check about one line
        // is that it moves something.
        let amount = line.amount.amount_minor();
        if amount == 0 {
            return Err(Error::InvalidLineAmounts);
        }

        match line.side {
            Side::Debit => debits = debits.checked_add(amount).ok_or(Error::MoneyOverflow)?,
            Side::Credit => credits = credits.checked_add(amount).ok_or(Error::MoneyOverflow)?,
        }
    }

    if debits != credits {
        return Err(Error::UnbalancedEntry { debits, credits });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line with fresh ids, read from the two columns `debit` and `credit`.
    pub(super) fn journal_line(debit: i64, credit: i64) -> Result<JournalLine> {
        let debit = Money::from_minor(debit)?;
        let credit = Money::from_minor(credit)?;
        let (side, amount) = Side::from_columns(debit, credit)?;

        Ok(sided_line(side, amount))
    }

    /// A line with fresh ids and `amount` on `side`.
    fn sided_line(side: Side, amount: Money) -> JournalLine {
        JournalLine {
            id: JournalLineId::generate(),
            entry_id: JournalEntryId::generate(),
            account_id: AccountId::generate(),
            amount,
            side,
            memo: None,
        }
    }

    #[test]
    fn balanced_two_line_entry_ok() {
        let lines = vec![journal_line(500, 0).unwrap(), journal_line(0, 500).unwrap()];
        assert!(validate_lines_for_post(&lines).is_ok());
    }

    #[test]
    fn unbalanced_rejected() {
        let lines = vec![journal_line(500, 0).unwrap(), journal_line(0, 400).unwrap()];
        assert_eq!(
            validate_lines_for_post(&lines),
            Err(Error::UnbalancedEntry {
                debits: 500,
                credits: 400,
            })
        );
    }

    #[test]
    fn too_few_lines_rejected() {
        let lines = vec![journal_line(100, 0).unwrap()];
        assert_eq!(validate_lines_for_post(&lines), Err(Error::TooFewLines));
    }

    #[test]
    fn a_line_with_both_sides_or_neither_cannot_be_built_from_two_columns() {
        assert_eq!(
            journal_line(50, 50).map(|_| ()),
            Err(Error::InvalidLineAmounts)
        );
        assert_eq!(
            journal_line(0, 0).map(|_| ()),
            Err(Error::InvalidLineAmounts)
        );
    }

    #[test]
    fn a_line_built_with_a_zero_amount_is_rejected_at_post() {
        let lines = vec![
            sided_line(Side::Debit, Money::ZERO),
            journal_line(100, 0).unwrap(),
            journal_line(0, 100).unwrap(),
        ];
        assert_eq!(
            validate_lines_for_post(&lines),
            Err(Error::InvalidLineAmounts)
        );
    }

    #[test]
    fn a_line_serializes_in_two_columns_and_reads_back() {
        let line = journal_line(0, 4_500).unwrap();
        let json = serde_json::to_value(&line).unwrap();

        assert_eq!(json["debit"]["amount_minor"], 0);
        assert_eq!(json["credit"]["amount_minor"], 4_500);
        assert!(json.get("amount").is_none() && json.get("side").is_none());
        assert_eq!(serde_json::from_value::<JournalLine>(json).unwrap(), line);
    }

    #[test]
    fn json_with_both_columns_filled_or_both_empty_is_not_a_line() {
        let line = journal_line(4_500, 0).unwrap();
        let mut both = serde_json::to_value(&line).unwrap();
        both["credit"]["amount_minor"] = 1.into();
        let mut neither = serde_json::to_value(&line).unwrap();
        neither["debit"]["amount_minor"] = 0.into();

        assert!(serde_json::from_value::<JournalLine>(both).is_err());
        assert!(serde_json::from_value::<JournalLine>(neither).is_err());
    }

    #[test]
    fn reversing_a_line_moves_its_amount_to_the_other_column() {
        let line = journal_line(4_500, 0).unwrap();
        let reversed = JournalLine {
            side: line.side.opposite(),
            ..line.clone()
        };

        assert_eq!((line.debit(), line.credit()), (line.amount, Money::ZERO));
        assert_eq!(
            (reversed.debit(), reversed.credit()),
            (Money::ZERO, line.amount)
        );
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::tests::journal_line;
    use super::*;

    /// The debit and credit of one line as it is written in two columns:
    /// mostly one-sided, as a postable line is, and sometimes empty or
    /// two-sided.
    fn line_amounts() -> impl Strategy<Value = (i64, i64)> {
        prop_oneof![
            4 => (1_i64..4).prop_map(|debit| (debit, 0)),
            4 => (1_i64..4).prop_map(|credit| (0, credit)),
            1 => Just((0, 0)),
            1 => (1_i64..4, 1_i64..4),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        // The three rules, stated on the two-column form an entry arrives
        // in. A line with both columns or neither is refused when it is
        // read; the rest is refused by `validate_lines_for_post`.
        #[test]
        fn lines_are_accepted_exactly_when_they_follow_the_three_rules(
            amounts in prop::collection::vec(line_amounts(), 0..6),
        ) {
            let accepted = amounts
                .iter()
                .map(|&(debit, credit)| journal_line(debit, credit))
                .collect::<Result<Vec<JournalLine>>>()
                .and_then(|lines| validate_lines_for_post(&lines))
                .is_ok();

            let every_line_is_one_sided =
                amounts.iter().all(|&(debit, credit)| (debit > 0) != (credit > 0));
            let debits: i64 = amounts.iter().map(|&(debit, _)| debit).sum();
            let credits: i64 = amounts.iter().map(|&(_, credit)| credit).sum();
            let follows_the_rules =
                amounts.len() >= 2 && every_line_is_one_sided && debits == credits;

            prop_assert_eq!(accepted, follows_the_rules);
        }

        #[test]
        fn a_line_cannot_be_built_with_both_sides(debit in 1_i64.., credit in 1_i64..) {
            prop_assert_eq!(
                journal_line(debit, credit).map(|_| ()),
                Err(Error::InvalidLineAmounts)
            );
        }

        #[test]
        fn a_line_has_its_amount_in_exactly_one_column(debit in 0_i64.., credit in 0_i64..) {
            let Ok(line) = journal_line(debit, credit) else {
                // Refused: both columns were filled, or neither.
                prop_assert_eq!(debit > 0, credit > 0);
                return Ok(());
            };

            prop_assert_ne!(
                line.debit().amount_minor() > 0,
                line.credit().amount_minor() > 0
            );
            prop_assert_eq!(line.debit().amount_minor(), debit);
            prop_assert_eq!(line.credit().amount_minor(), credit);
        }

        // A typed line, written in two columns and read back, is the same
        // line.
        #[test]
        fn a_line_survives_its_two_column_form(
            amount in 1_i64..,
            is_debit in any::<bool>(),
        ) {
            let side = if is_debit { Side::Debit } else { Side::Credit };
            let amount = Money::from_minor(amount).unwrap();
            let (debit, credit) = side.columns(amount);

            prop_assert_eq!(Side::from_columns(debit, credit), Ok((side, amount)));
            prop_assert_eq!(side.opposite().opposite(), side);

            let line = JournalLine {
                id: JournalLineId::generate(),
                entry_id: JournalEntryId::generate(),
                account_id: AccountId::generate(),
                amount,
                side,
                memo: None,
            };
            let json = serde_json::to_string(&line).unwrap();
            prop_assert_eq!(serde_json::from_str::<JournalLine>(&json).unwrap(), line);
        }
    }
}
