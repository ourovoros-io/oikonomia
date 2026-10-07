//! Journal entries and lines with double-entry validation.

use super::account::AccountId;
use super::entity::EntityId;
use crate::error::{Error, Result};
use crate::money::Money;
use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

/// Stable journal entry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JournalEntryId(pub Uuid);

impl JournalEntryId {
    /// Generate a new random entry id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JournalEntryId {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable journal line id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JournalLineId(pub Uuid);

impl JournalLineId {
    /// Generate a new random line id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JournalLineId {
    fn default() -> Self {
        Self::new()
    }
}

/// Lifecycle of a journal entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    /// Editable; does not affect balances.
    Draft,
    /// Immutable; affects balances. Correct via void/reverse.
    Posted,
}

/// Header for a multi-line journal entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Primary key.
    pub id: JournalEntryId,
    /// Owning entity.
    pub entity_id: EntityId,
    /// Accounting date.
    #[serde(with = "crate::util::serde_date")]
    pub entry_date: Date,
    /// User-facing description.
    pub description: String,
    /// Optional external reference (check #, invoice #).
    pub reference: Option<String>,
    /// Draft vs posted.
    pub status: EntryStatus,
    /// Owner-only visibility flag. Hidden rows stay in the book and in
    /// list/get/register; [`crate::csv::export_journal_csv`] omits them.
    /// Not extra encryption.
    pub hidden: bool,
}

/// One debit or credit line on a journal entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLine {
    /// Primary key.
    pub id: JournalLineId,
    /// Parent entry.
    pub entry_id: JournalEntryId,
    /// Account to debit or credit.
    pub account_id: AccountId,
    /// Debit amount (minor units); exclusive with credit.
    pub debit: Money,
    /// Credit amount (minor units); exclusive with debit.
    pub credit: Money,
    /// Optional line memo.
    pub memo: Option<String>,
}

/// Validate lines before posting a journal entry.
///
/// Rules:
/// - at least two lines
/// - each line is debit XOR credit (exactly one side non-zero)
/// - total debits equal total credits
///
/// # Errors
///
/// Returns [`Error::TooFewLines`], [`Error::InvalidLineAmounts`],
/// [`Error::UnbalancedEntry`], or [`Error::MoneyOverflow`] when a rule fails.
pub fn validate_lines_for_post(lines: &[JournalLine]) -> Result<()> {
    if lines.len() < 2 {
        return Err(Error::TooFewLines);
    }

    let mut debits: i64 = 0;
    let mut credits: i64 = 0;

    for line in lines {
        let d = line.debit.amount_minor();
        let c = line.credit.amount_minor();

        let debit_side = d > 0;
        let credit_side = c > 0;

        if debit_side == credit_side {
            // both zero or both non-zero
            return Err(Error::InvalidLineAmounts);
        }

        debits = debits.checked_add(d).ok_or(Error::MoneyOverflow)?;
        credits = credits.checked_add(c).ok_or(Error::MoneyOverflow)?;
    }

    if debits != credits {
        return Err(Error::UnbalancedEntry { debits, credits });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal_line(debit: i64, credit: i64) -> JournalLine {
        let debit = Money::from_minor(debit).unwrap();
        let credit = Money::from_minor(credit).unwrap();

        JournalLine {
            id: JournalLineId::new(),
            entry_id: JournalEntryId::new(),
            account_id: AccountId::new(),
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
        // first line both debit and credit non-zero
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

    fn journal_line(debit: i64, credit: i64) -> JournalLine {
        JournalLine {
            id: JournalLineId::new(),
            entry_id: JournalEntryId::new(),
            account_id: AccountId::new(),
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
