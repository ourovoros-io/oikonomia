//! Why a bank CSV or a journal CSV cannot be read.
//!
//! A [`CsvError`] reaches the caller as [`Error::Csv`](crate::Error::Csv)
//! with its variant intact, so a caller or a test can match the problem
//! itself. Like every error of the crate it reaches the user as a code
//! ([`CsvError::code`]) and named values ([`CsvError::params`]), never as a
//! sentence written here.
//!
//! The variants about one cell of one row (a date that is not a date, an
//! amount that is zero) rarely travel as an error: a bad row makes that row
//! of the import preview invalid, with the same code as a
//! [`UiTextCode`](crate::ui_text::UiTextCode), and leaves the other rows
//! usable. A test pins the two spellings of each such code to each other.

use std::collections::BTreeMap;
use std::fmt;

/// What is wrong with a column mapping the caller supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvMappingProblem {
    /// The mapping names no date column.
    MissingDate,
    /// The mapping names no amount column, no debit column and no credit
    /// column.
    MissingAmount,
    /// The mapping names an amount column and also a debit or a credit
    /// column.
    AmountAndDebitOrCredit,
    /// The mapping names a header the file does not have.
    UnknownColumn {
        /// The header as the mapping gives it.
        name: String,
    },
}

impl CsvMappingProblem {
    /// Returns the stable `snake_case` identifier sent as the `problem`
    /// parameter.
    ///
    /// The UI has a sentence for each identifier and never shows the
    /// identifier itself. A test checks the identifiers against
    /// `web/src/lib/csvMappingProblems.json`.
    #[must_use]
    pub fn identifier(&self) -> &'static str {
        match self {
            Self::MissingDate => "missing_date",
            Self::MissingAmount => "missing_amount",
            Self::AmountAndDebitOrCredit => "amount_and_debit_or_credit",
            Self::UnknownColumn { .. } => "unknown_column",
        }
    }
}

impl fmt::Display for CsvMappingProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDate => formatter.write_str("the mapping has no date column"),
            Self::MissingAmount => {
                formatter.write_str("the mapping has no amount, debit or credit column")
            }
            Self::AmountAndDebitOrCredit => {
                formatter.write_str("the mapping sets both amount and debit or credit")
            }
            Self::UnknownColumn { name } => write!(formatter, "no column named '{name}'"),
        }
    }
}

/// A reason a bank CSV or a journal CSV cannot be read.
///
/// The enum is matched exhaustively by the code that words it; see the
/// [error module documentation](crate::error) for the policy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CsvError {
    /// File is empty or whitespace only.
    #[error("the CSV is empty")]
    Empty,
    /// Bytes were not valid UTF-8.
    #[error("the CSV is not valid UTF-8")]
    NotUtf8,
    /// File exceeds [`MAX_CSV_BYTES`](crate::csv::MAX_CSV_BYTES).
    #[error("the CSV is larger than 8 MB")]
    TooLarge,
    /// The header row or a record breaks the CSV format itself.
    #[error("the CSV cannot be read: {detail}")]
    Malformed {
        /// The reader's own text. For logs; never sent as a parameter.
        detail: String,
    },
    /// First row could not be used as headers.
    #[error("the CSV has no header row")]
    MissingHeader,
    /// The file is Oikonomia's own journal export, which the bank importer
    /// cannot read: it holds journal lines, not the movements of one
    /// account.
    #[error("the file is a journal export, not a bank statement")]
    JournalExport,
    /// A journal CSV lacks one of the columns the export writes.
    #[error("the journal CSV has no {column} column")]
    MissingColumn {
        /// The header the export writes for the column.
        column: &'static str,
    },
    /// A cell is not a supported date.
    #[error("invalid date: {0}")]
    InvalidDate(String),
    /// A cell is not a supported amount.
    #[error("invalid amount: {0}")]
    InvalidAmount(String),
    /// A type or direction cell is not one the import recognizes.
    #[error("invalid type: {0}")]
    InvalidType(String),
    /// A `status` cell of a journal CSV is neither `posted` nor `voided`.
    #[error("unknown journal status: {0}")]
    InvalidStatus(String),
    /// A `debit_minor` or `credit_minor` cell of a journal CSV is not an
    /// integer that fits an `i64`.
    #[error("invalid integer: {0}")]
    InvalidInteger(String),
    /// The date cell is empty or only whitespace.
    #[error("date is missing")]
    MissingDate,
    /// The amount cell is empty or only whitespace.
    #[error("amount is missing")]
    MissingAmount,
    /// Magnitude does not fit in `i64`.
    #[error("amount overflow")]
    AmountOverflow,
    /// Parsed amount is zero (simple entries require a positive amount).
    #[error("amount is zero")]
    ZeroAmount,
    /// Caller-supplied column mapping is incomplete or contradictory.
    #[error("{0}")]
    InvalidMapping(CsvMappingProblem),
}

impl CsvError {
    /// Every code [`CsvError::code`] can return, in the order of the
    /// variants.
    ///
    /// The desktop crate checks this list against `errorCodes.json`.
    pub const ALL_CODES: &'static [&'static str] = &[
        "csv_empty",
        "csv_not_utf8",
        "csv_too_large",
        "csv_parse",
        "csv_missing_header",
        "csv_journal_export",
        "csv_missing_column",
        "csv_invalid_date",
        "csv_invalid_amount",
        "csv_invalid_type",
        "csv_invalid_status",
        "csv_invalid_integer",
        "csv_missing_date",
        "csv_missing_amount",
        "csv_amount_overflow",
        "csv_zero_amount",
        "csv_invalid_mapping",
    ];

    /// Returns the stable `snake_case` identifier the UI maps to localized
    /// text.
    ///
    /// `csv_parse` is the code every CSV failure had before the variants
    /// were kept apart; it remains the code of a file that breaks the CSV
    /// format. A code about one cell is spelled like the
    /// [`UiTextCode`](crate::ui_text::UiTextCode) of the same problem.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "csv_empty",
            Self::NotUtf8 => "csv_not_utf8",
            Self::TooLarge => "csv_too_large",
            Self::Malformed { .. } => "csv_parse",
            Self::MissingHeader => "csv_missing_header",
            Self::JournalExport => "csv_journal_export",
            Self::MissingColumn { .. } => "csv_missing_column",
            Self::InvalidDate(_) => "csv_invalid_date",
            Self::InvalidAmount(_) => "csv_invalid_amount",
            Self::InvalidType(_) => "csv_invalid_type",
            Self::InvalidStatus(_) => "csv_invalid_status",
            Self::InvalidInteger(_) => "csv_invalid_integer",
            Self::MissingDate => "csv_missing_date",
            Self::MissingAmount => "csv_missing_amount",
            Self::AmountOverflow => "csv_amount_overflow",
            Self::ZeroAmount => "csv_zero_amount",
            Self::InvalidMapping(_) => "csv_invalid_mapping",
        }
    }

    /// Returns the values the UI substitutes into the localized text, by name.
    ///
    /// A cell travels as `value`, as written. A mapping problem travels as
    /// the `problem` identifier, with the header it could not find as
    /// `column`. The reader's text of a malformed file is left out on
    /// purpose.
    #[must_use]
    pub fn params(&self) -> BTreeMap<&'static str, String> {
        let mut params = BTreeMap::new();

        match self {
            Self::MissingColumn { column } => {
                params.insert("column", (*column).to_owned());
            }
            Self::InvalidDate(value)
            | Self::InvalidAmount(value)
            | Self::InvalidType(value)
            | Self::InvalidStatus(value)
            | Self::InvalidInteger(value) => {
                params.insert("value", value.clone());
            }
            Self::InvalidMapping(problem) => {
                params.insert("problem", problem.identifier().to_owned());
                if let CsvMappingProblem::UnknownColumn { name } = problem {
                    params.insert("column", name.clone());
                }
            }
            Self::Empty
            | Self::NotUtf8
            | Self::TooLarge
            | Self::Malformed { .. }
            | Self::MissingHeader
            | Self::JournalExport
            | Self::MissingDate
            | Self::MissingAmount
            | Self::AmountOverflow
            | Self::ZeroAmount => {}
        }

        params
    }
}

#[cfg(test)]
mod tests {
    use super::{CsvError, CsvMappingProblem};
    use crate::ui_text::UiTextCode;
    use oikonomia_test_support::listed_variants;
    use std::collections::{BTreeMap, BTreeSet};

    listed_variants! {
        patterns listed_errors for CsvError {
            CsvError::Empty,
            CsvError::NotUtf8,
            CsvError::TooLarge,
            CsvError::Malformed { .. },
            CsvError::MissingHeader,
            CsvError::JournalExport,
            CsvError::MissingColumn { .. },
            CsvError::InvalidDate(_),
            CsvError::InvalidAmount(_),
            CsvError::InvalidType(_),
            CsvError::InvalidStatus(_),
            CsvError::InvalidInteger(_),
            CsvError::MissingDate,
            CsvError::MissingAmount,
            CsvError::AmountOverflow,
            CsvError::ZeroAmount,
            CsvError::InvalidMapping(_),
        }
    }

    /// One value of every variant, in declaration order.
    ///
    /// The mapping sample is the problem with the most parameters, so the
    /// parameter fixture lists every name the copy may use.
    fn every_variant() -> Vec<CsvError> {
        vec![
            CsvError::Empty,
            CsvError::NotUtf8,
            CsvError::TooLarge,
            CsvError::Malformed { detail: "x".into() },
            CsvError::MissingHeader,
            CsvError::JournalExport,
            CsvError::MissingColumn { column: "date" },
            CsvError::InvalidDate("x".into()),
            CsvError::InvalidAmount("x".into()),
            CsvError::InvalidType("x".into()),
            CsvError::InvalidStatus("x".into()),
            CsvError::InvalidInteger("x".into()),
            CsvError::MissingDate,
            CsvError::MissingAmount,
            CsvError::AmountOverflow,
            CsvError::ZeroAmount,
            CsvError::InvalidMapping(CsvMappingProblem::UnknownColumn { name: "x".into() }),
        ]
    }

    /// Fails when `every_variant` has no sample for a listed variant, or when
    /// `ALL_CODES` is not the codes of the samples in order. The compiler
    /// checks `listed_errors` against the enum with an exhaustive `match`.
    #[test]
    fn all_codes_lists_the_code_of_every_variant() {
        let samples = every_variant();
        let codes: Vec<&str> = samples.iter().map(CsvError::code).collect();

        listed_errors::assert_every_position_once(
            samples.iter().map(listed_errors::position).collect(),
        );
        assert_eq!(codes, CsvError::ALL_CODES);
    }

    #[test]
    fn every_code_is_distinct_snake_case_and_starts_with_csv() {
        let codes: BTreeSet<&str> = CsvError::ALL_CODES.iter().copied().collect();

        assert_eq!(codes.len(), CsvError::ALL_CODES.len());
        for code in codes {
            assert!(code.starts_with("csv_"), "{code}");
            assert!(
                code.chars().all(|letter| {
                    letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '_'
                }),
                "{code}"
            );
        }
    }

    /// The wire spelling of a `UiTextCode`, which is its serde name.
    fn wire_code(code: UiTextCode) -> String {
        serde_json::to_value(code)
            .expect("a code serializes")
            .as_str()
            .expect("as a string")
            .to_owned()
    }

    #[test]
    fn a_problem_with_one_cell_has_the_code_of_its_row_note() {
        let pairs = [
            (
                CsvError::InvalidDate("x".into()),
                UiTextCode::CsvInvalidDate,
            ),
            (
                CsvError::InvalidAmount("x".into()),
                UiTextCode::CsvInvalidAmount,
            ),
            (
                CsvError::InvalidType("x".into()),
                UiTextCode::CsvInvalidType,
            ),
            (CsvError::MissingDate, UiTextCode::CsvMissingDate),
            (CsvError::MissingAmount, UiTextCode::CsvMissingAmount),
            (CsvError::ZeroAmount, UiTextCode::CsvZeroAmount),
            (CsvError::AmountOverflow, UiTextCode::CsvAmountOverflow),
        ];

        for (error, note) in pairs {
            assert_eq!(error.code(), wire_code(note), "{error:?}");
        }
    }

    #[test]
    fn a_cell_travels_as_the_value_parameter() {
        let error = CsvError::InvalidDate("31/31/2026".into());

        assert_eq!(
            error.params(),
            BTreeMap::from([("value", "31/31/2026".to_owned())])
        );
    }

    #[test]
    fn a_mapping_problem_travels_as_an_identifier_with_the_column_it_names() {
        let incomplete = CsvError::InvalidMapping(CsvMappingProblem::MissingDate);
        let unknown = CsvError::InvalidMapping(CsvMappingProblem::UnknownColumn {
            name: "Payee".into(),
        });

        assert_eq!(
            incomplete.params(),
            BTreeMap::from([("problem", "missing_date".to_owned())])
        );
        assert_eq!(
            unknown.params(),
            BTreeMap::from([
                ("column", "Payee".to_owned()),
                ("problem", "unknown_column".to_owned()),
            ])
        );
    }

    #[test]
    fn the_text_of_a_malformed_file_is_not_a_parameter() {
        let error = CsvError::Malformed {
            detail: "record 3 has 9 fields".into(),
        };

        assert_eq!(error.code(), "csv_parse");
        assert_eq!(error.params(), BTreeMap::new());
        assert!(error.to_string().contains("record 3 has 9 fields"));
    }

    listed_variants! {
        patterns listed_problems for CsvMappingProblem {
            CsvMappingProblem::MissingDate,
            CsvMappingProblem::MissingAmount,
            CsvMappingProblem::AmountAndDebitOrCredit,
            CsvMappingProblem::UnknownColumn { .. },
        }
    }

    /// One value of every mapping problem, in declaration order.
    fn every_problem() -> Vec<CsvMappingProblem> {
        vec![
            CsvMappingProblem::MissingDate,
            CsvMappingProblem::MissingAmount,
            CsvMappingProblem::AmountAndDebitOrCredit,
            CsvMappingProblem::UnknownColumn { name: "x".into() },
        ]
    }

    /// The UI words a mapping problem by its identifier and has a sentence
    /// for each one the fixture lists. The compiler checks `listed_problems`
    /// against the enum, so a new problem cannot be left out of the fixture.
    #[test]
    fn the_problems_fixture_lists_the_identifier_of_every_mapping_problem() {
        let samples = every_problem();
        let identifiers: Vec<&str> = samples.iter().map(CsvMappingProblem::identifier).collect();
        let pinned: Vec<String> = serde_json::from_str(include_str!(
            "../../../../web/src/lib/csvMappingProblems.json"
        ))
        .expect("csvMappingProblems.json parses");

        listed_problems::assert_every_position_once(
            samples.iter().map(listed_problems::position).collect(),
        );
        assert_eq!(identifiers, pinned);
    }

    /// The parameter names the shared fixture pins for each code that has any.
    fn pinned_params() -> BTreeMap<String, Vec<String>> {
        serde_json::from_str(include_str!("../../../../web/src/lib/errorCodeParams.json"))
            .expect("errorCodeParams.json parses")
    }

    #[test]
    fn the_params_fixture_lists_exactly_the_params_each_code_sends() {
        let pinned = pinned_params();

        for sample in every_variant() {
            let sent: Vec<String> = sample
                .params()
                .keys()
                .map(|name| (*name).to_owned())
                .collect();
            let listed = pinned.get(sample.code()).cloned().unwrap_or_default();

            assert_eq!(sent, listed, "errorCodeParams.json for {}", sample.code());
        }
    }

    #[test]
    fn every_message_starts_in_lowercase_and_has_no_trailing_period() {
        for error in every_variant() {
            let message = error.to_string();

            assert!(
                message
                    .chars()
                    .next()
                    .is_some_and(|first| !first.is_uppercase()),
                "{}: {message:?} must not be empty or start with a capital",
                error.code()
            );
            assert!(
                !message.ends_with('.'),
                "{}: {message:?} must not end with a period",
                error.code()
            );
        }
    }
}
