//! Journal entries, their lines, and the rule that makes an entry postable.
//!
//! # Model
//!
//! A [`JournalEntry`] is the header of one transaction: its book, date and
//! description. Its [`JournalLine`]s say where the money moved, each naming
//! one account and an amount on one side.
//!
//! A line has both a `debit` and a `credit` field, of which exactly one is
//! greater than zero. Both are [`Money`], so neither can be negative, but the
//! type alone does not stop a line with both sides filled or both empty;
//! [`validate_lines_for_post`] does.
//!
//! # The posting rule
//!
//! [`validate_lines_for_post`] is the double-entry invariant, checked on the
//! lines alone before anything is written:
//!
//! 1. there are at least two lines;
//! 2. every line has an amount on exactly one side;
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

/// One debit or credit of one account, as part of a [`JournalEntry`].
///
/// Exactly one of `debit` and `credit` is greater than zero on a line that
/// [`validate_lines_for_post`] accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLine {
    /// The line's own id.
    pub id: JournalLineId,
    /// The entry this line is part of.
    pub entry_id: JournalEntryId,
    /// The account the amount is debited or credited to.
    pub account_id: AccountId,
    /// The amount debited, or zero when the line is a credit.
    pub debit: Money,
    /// The amount credited, or zero when the line is a debit.
    pub credit: Money,
    /// A note on this line alone, if any.
    pub memo: Option<String>,
}

/// Checks that `lines` can be posted as one journal entry.
///
/// The lines pass when there are at least two of them, each has an amount
/// greater than zero on exactly one side, and the debits and credits add up
/// to the same total. Nothing else is looked at: not the accounts, not the
/// ids, not the memos.
///
/// # Examples
///
/// ```
/// use oikonomia_core::domain::{
///     AccountId, JournalEntryId, JournalLine, JournalLineId, validate_lines_for_post,
/// };
/// use oikonomia_core::{Error, Money};
///
/// # fn line(entry_id: JournalEntryId, debit: i64, credit: i64) -> Result<JournalLine, Error> {
/// #     Ok(JournalLine {
/// #         id: JournalLineId::generate(),
/// #         entry_id,
/// #         account_id: AccountId::generate(),
/// #         debit: Money::from_minor(debit)?,
/// #         credit: Money::from_minor(credit)?,
/// #         memo: None,
/// #     })
/// # }
/// let entry = JournalEntryId::generate();
///
/// // 45.00 of groceries paid from the bank: one debit, one credit.
/// let balanced = [line(entry, 4_500, 0)?, line(entry, 0, 4_500)?];
/// assert_eq!(validate_lines_for_post(&balanced), Ok(()));
///
/// let unbalanced = [line(entry, 4_500, 0)?, line(entry, 0, 4_000)?];
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
/// lines one at a time in order, each for its sides and then for the running
/// totals, and the balance last:
///
/// - [`Error::TooFewLines`] when there are fewer than two lines.
/// - [`Error::InvalidLineAmounts`] for a line whose debit and credit are both
///   zero or both greater than zero.
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
        let debit = line.debit.amount_minor();
        let credit = line.credit.amount_minor();

        // `Money` is never negative, so "greater than zero" is "has an
        // amount". A postable line has an amount on one side and not the
        // other; equal answers mean both sides or neither.
        let has_debit = debit > 0;
        let has_credit = credit > 0;
        if has_debit == has_credit {
            return Err(Error::InvalidLineAmounts);
        }

        debits = debits.checked_add(debit).ok_or(Error::MoneyOverflow)?;
        credits = credits.checked_add(credit).ok_or(Error::MoneyOverflow)?;
    }

    if debits != credits {
        return Err(Error::UnbalancedEntry { debits, credits });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line with fresh ids and the given amounts on its two sides.
    fn journal_line(debit: i64, credit: i64) -> JournalLine {
        let debit = Money::from_minor(debit).unwrap();
        let credit = Money::from_minor(credit).unwrap();

        JournalLine {
            id: JournalLineId::generate(),
            entry_id: JournalEntryId::generate(),
            account_id: AccountId::generate(),
            debit,
            credit,
            memo: None,
        }
    }

    #[test]
    fn balanced_two_line_entry_ok() {
        let lines = vec![journal_line(500, 0), journal_line(0, 500)];
        assert!(validate_lines_for_post(&lines).is_ok());
    }

    #[test]
    fn unbalanced_rejected() {
        let lines = vec![journal_line(500, 0), journal_line(0, 400)];
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
        let lines = vec![journal_line(100, 0)];
        assert_eq!(validate_lines_for_post(&lines), Err(Error::TooFewLines));
    }

    #[test]
    fn both_sides_rejected() {
        let lines = vec![journal_line(50, 50), journal_line(0, 50)];
        assert_eq!(
            validate_lines_for_post(&lines),
            Err(Error::InvalidLineAmounts)
        );
    }

    #[test]
    fn zero_line_rejected() {
        let lines = vec![
            journal_line(0, 0),
            journal_line(100, 0),
            journal_line(0, 100),
        ];
        assert_eq!(
            validate_lines_for_post(&lines),
            Err(Error::InvalidLineAmounts)
        );
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// The debit and credit of one line: mostly one-sided, as a postable line
    /// is, and sometimes empty or two-sided.
    fn line_amounts() -> impl Strategy<Value = (i64, i64)> {
        prop_oneof![
            4 => (1_i64..4).prop_map(|debit| (debit, 0)),
            4 => (1_i64..4).prop_map(|credit| (0, credit)),
            1 => Just((0, 0)),
            1 => (1_i64..4, 1_i64..4),
        ]
    }

    /// A line with fresh ids and the given amounts on its two sides.
    fn journal_line(debit: i64, credit: i64) -> JournalLine {
        JournalLine {
            id: JournalLineId::generate(),
            entry_id: JournalEntryId::generate(),
            account_id: AccountId::generate(),
            debit: Money::from_minor(debit).unwrap(),
            credit: Money::from_minor(credit).unwrap(),
            memo: None,
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn lines_are_accepted_exactly_when_they_follow_the_three_rules(
            amounts in prop::collection::vec(line_amounts(), 0..6),
        ) {
            let lines: Vec<JournalLine> =
                amounts.iter().map(|&(debit, credit)| journal_line(debit, credit)).collect();

            let every_line_is_one_sided =
                amounts.iter().all(|&(debit, credit)| (debit > 0) != (credit > 0));
            let debits: i64 = amounts.iter().map(|&(debit, _)| debit).sum();
            let credits: i64 = amounts.iter().map(|&(_, credit)| credit).sum();
            let follows_the_rules =
                amounts.len() >= 2 && every_line_is_one_sided && debits == credits;

            prop_assert_eq!(validate_lines_for_post(&lines).is_ok(), follows_the_rules);
        }
    }
}
