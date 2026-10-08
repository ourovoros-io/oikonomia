//! Bank CSV reader: delimiter detection, headers, quoted fields, per-row errors.
//!
//! Bank exports agree on little, so the reader settles each question with a
//! fixed rule and reports what it could not read instead of guessing. A
//! problem with the file as a whole is an `Err`; a problem with one row is a
//! [`CsvRowOutcome::Invalid`] in the result, so one bad line does not cost
//! the user the rest of the statement.
//!
//! # Delimiter
//!
//! Comma, semicolon or tab. The first non-empty line, the header, decides:
//! the delimiter is the one that splits it into the most fields, counting
//! only delimiters outside double quotes. A tie goes to the comma, and
//! between semicolon and tab to the semicolon, so a header with no
//! delimiter in it is read as comma-separated. Only the header is looked
//! at: a data row may hold any number of decimal commas without changing
//! the answer. Other delimiters, such as `|`, are not detected.
//!
//! Fields may be quoted as in RFC 4180, cells are trimmed, and a row may
//! have fewer cells than the header; a missing cell reads as empty.
//!
//! # Column detection
//!
//! Without an explicit [`CsvColumnMapping`], each header is lowercased and
//! stripped of spaces, `_`, `-` and accents (`fold_header`), then tested in
//! this order. Greek, German and French names are listed beside the English
//! ones in the table; the aliases themselves are `ROLE_ALIASES`. The first
//! test that passes names the column:
//!
//! | Order | Column      | Header                                               |
//! |-------|-------------|------------------------------------------------------|
//! | 1     | Date        | Contains `date`, `datum`, `ημερομηνια`,              |
//! |       |             | `buchungstag` or `wertstellung`                      |
//! |       |             | (`Booking date`, `Buchungsdatum`, `Ημερομηνία`)      |
//! | 2     | Amount      | `amount`, `value`, `sum`, `transactionamount`,       |
//! |       |             | `betrag`, `montant`, `importo`, `ποσο`               |
//! | 3     | Debit       | `debit`, `withdrawal`, `outflow`, `addebito`,        |
//! |       |             | `soll`, `χρεωση`                                     |
//! | 4     | Credit      | `credit`, `deposit`, `inflow`, `accredito`,          |
//! |       |             | `haben`, `πιστωση`                                   |
//! | 5     | Direction   | `type`, `dc`, `d/c`, `debitcredit`, `drcr`,          |
//! |       |             | `transactiontype`, and values that are directions    |
//! | 6     | Reference   | `reference`, `ref`, `check`, `cheque`, `checkno`,    |
//! |       |             | `chequeno`, `fitid`, `referenz`, `αναφορα`           |
//! | 7     | Description | `description`, `memo`, `narration`, `details`,       |
//! |       |             | `payee`, `particulars`, `narrative`, `libelle`,      |
//! |       |             | `beschreibung`, `verwendungszweck`, `descrizione`,   |
//! |       |             | `περιγραφη`, `αιτιολογια`, `transaction`, `name`     |
//!
//! Rows 2 to 7 match the whole header, not a part of it. When several
//! headers name the same column, the leftmost wins and the others are not
//! read.
//!
//! The direction column is the one role its header does not settle, because
//! banks also use `Type` for the kind of transaction (`POS`, `TRANSFER`),
//! and a column read as the direction makes every row with another word
//! invalid. A header of row 5 is the direction column only when every
//! non-empty cell under it in the first rows of the file, 50 of them
//! (`DIRECTION_SAMPLE_ROWS`), is a direction word (the list is in the next
//! section). A column with no value in those rows qualifies: it then changes
//! no sign. A header that fails the test has no role, and the next header of
//! row 5 to the right is tested in its place. Only the first rows are looked
//! at so that one odd cell far down a long statement costs that row and not
//! the column.
//!
//! The test has a cost. A column that mixes directions with other words
//! (`Debit`, `Credit`, `Fee`) fails it, so it is not read, and each amount
//! beside it keeps the sign it was written with: an unsigned debit is then
//! read as money in. The preview shows each row as an expense or an income
//! before anything is posted, and the Map columns step can name the column
//! as the direction; the rows with another word are then the invalid ones.
//!
//! A file needs a date column and at least one of amount, debit and credit.
//! When an amount column is present the debit and credit columns are not
//! read, and a direction column is read only beside an amount column.
//!
//! A file whose headers name no date column, or none of amount, debit and
//! credit, is not refused: the user can tell which column is which. Its rows
//! are not read, and the result names the missing columns
//! ([`ParsedBankCsv::missing_columns`]) beside the headers and what was
//! detected, for the caller to ask again with an explicit mapping.
//!
//! # Direction words
//!
//! Matched whole, without regard to case:
//!
//! | Reading   | Words                                                  |
//! |-----------|--------------------------------------------------------|
//! | Money out | `d`, `dr`, `debit`, `withdrawal`, `expense`, `out`     |
//! | Money in  | `c`, `cr`, `credit`, `deposit`, `income`, `in`         |
//!
//! An explicit mapping replaces all of this: its header names are matched
//! whole, without regard to ASCII case, and a column it leaves out is not
//! read. It needs what detection needs: a date column, and an amount column
//! or at least one of debit and credit, never an amount column beside one of
//! those. The description column is optional either way: a row of a file
//! without one gets an empty description, which the ledger accepts.
//!
//! # Debit and credit columns
//!
//! A file with a column per direction is read as the credit less the debit,
//! each cell taken with the sign it was written with. The result is the
//! row's signed amount, negative for money out:
//!
//! | Debit cell | Credit cell | Row              | Reading                      |
//! |------------|-------------|------------------|------------------------------|
//! | `800.00`   | blank       | −800.00, expense | Money out.                   |
//! | blank      | `2500.00`   | +2500.00, income | Money in.                    |
//! | `-800.00`  | blank       | +800.00, income  | A debit taken back.          |
//! | blank      | `-2500.00`  | −2500.00, expense| A credit taken back.         |
//! | `100.00`   | `30.00`     | −70.00, expense  | The credit less the debit.   |
//! | `0.00`     | `25.00`     | +25.00, income   | A zero written for a blank.  |
//! | `40.00`    | `40.00`     | invalid          | They cancel: a zero amount.  |
//! | blank      | blank       | invalid          | A missing amount.            |
//!
//! A negative cell is a reversal because that is what the banks that use the
//! two columns write it for: the column says which way the original
//! movement went, and the minus says this row undoes one. The reading has a
//! cost. An export that writes every withdrawal in its debit column with a
//! minus is read the wrong way round, and nothing in one cell tells the two
//! conventions apart; the preview shows each row as an expense or an income
//! before anything is posted.
//!
//! Oikonomia 0.1.0 dropped the sign of each cell instead: a debit was always
//! money out and a credit always money in. A row with a negative cell
//! therefore also carries the amount 0.1.0 read for it
//! ([`ParsedBankRow::legacy_signed_amount_minor`]), so that the preview can
//! flag it as a possible duplicate of the entry 0.1.0 imported from it.
//!
//! # Dates
//!
//! One rule reads every date. The cell is three numbers with one separator
//! between them, `-`, `/` or `.`, the same one in both places. When the
//! first number has four digits it is the year and the date is year, month,
//! day. Otherwise the last number has to have four digits, and the date is
//! day, month, year.
//!
//! | Form | Examples | |
//! |------|----------|-|
//! | Year first | `2026-03-05`, `2026/03/05`, `2026.03.05` | Accepted. |
//! | Day first | `05.03.2026`, `05-03-2026`, `05/03/2026`, `5.3.2026` | Accepted. |
//! | Month first | `03/13/2026` | Not supported. |
//! | Mixed separators | `05.03-2026`, `2026-03/05` | Rejected. |
//! | Two-digit year | `05/03/26` | Rejected. |
//! | Month name, time of day | `5 Mar 2026`, `2026-03-05 10:00` | Rejected. |
//! | A sign or a non-ASCII digit | `+5/3/2026`, `٥.٣.٢٠٢٦` | Rejected. |
//! | A day the calendar lacks | `2026-02-30`, `31.04.2026` | Rejected. |
//!
//! Day and month may be written without a leading zero. The year is the
//! number with four digits, so ISO `2026-03-05` can be read one way only: a
//! four-digit first number is never a day.
//!
//! A date with the year last is always read day first, whatever the
//! separator. A US date is rejected only when that reading is impossible
//! (`03/13/2026`, month 13). `03/04/2026` cannot be told apart from the
//! European form and is read as 3 April; a US statement has to be converted
//! before import.

use std::fs;
use std::path::Path;

use csv::{ReaderBuilder, StringRecord, Trim};
use time::{Date, Month};

use crate::csv::amount::parse_book_amount;
use crate::csv::{
    CsvColumnMapping, CsvError, CsvMappingProblem, CsvRequiredColumn, CsvRowOutcome, MAX_CSV_BYTES,
    ParsedBankRow,
};
use crate::domain::CurrencyCode;
use crate::error::{Error, IoContext, PrivateDetail};
use crate::ledger::SimpleEntryKind;
use crate::ui_text::{UiText, UiTextCode};

/// Result of a step that can only fail for a reason about the CSV itself.
type CsvResult<T> = std::result::Result<T, CsvError>;

/// Parsed bank CSV plus header metadata for the Map columns UI.
#[derive(Debug, Clone)]
pub struct ParsedBankCsv {
    /// Trimmed header names, file order.
    pub headers: Vec<String>,
    /// Auto-detected mapping, from the header aliases and for the direction
    /// column from its values, even when the caller overrode columns.
    pub detected_mapping: CsvColumnMapping,
    /// The required columns detection did not find, date before amount.
    ///
    /// Not empty only when the caller gave no mapping and the headers do not
    /// name them; `rows` is then empty, because no row was read.
    pub missing_columns: Vec<CsvRequiredColumn>,
    /// Data rows in file order.
    pub rows: Vec<CsvRowOutcome>,
}

/// Reads a CSV file as UTF-8 text, without a leading byte-order mark.
///
/// Does not parse the text and does not touch the ledger.
///
/// # Errors
///
/// - [`Error::Io`] when `path` cannot be inspected or read, or is not a
///   regular file.
/// - [`Error::Csv`] with [`CsvError::TooLarge`] when the file is larger than
///   [`MAX_CSV_BYTES`], or with [`CsvError::NotUtf8`] when it is not valid
///   UTF-8.
pub fn read_csv_text(path: &Path) -> crate::error::Result<String> {
    let metadata = fs::metadata(path).io("inspect CSV file")?;
    if !metadata.is_file() {
        return Err(Error::io(
            "read CSV file",
            format_args!("not a regular file: {}", path.display()),
        ));
    }
    if metadata.len() > MAX_CSV_BYTES {
        return Err(CsvError::TooLarge.into());
    }
    let bytes = fs::read(path).io("read CSV file")?;
    let text = String::from_utf8(bytes).map_err(|_| CsvError::NotUtf8)?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// Parses a date cell written year first (`YYYY-MM-DD`) or day first
/// (`DD.MM.YYYY`), with `-`, `/` or `.` between the three numbers.
///
/// The table in the module doc has the rule. Day and month may be unpadded.
/// A date with the year last is always read day first: US `MM/DD/YYYY` is
/// not supported, `03/13/2026` is rejected (month 13), and `03/04/2026` is
/// read as 3 April.
///
/// # Errors
///
/// [`CsvError::MissingDate`] when the cell is empty or only whitespace;
/// [`CsvError::InvalidDate`], carrying the trimmed cell, when it is not in
/// one of the two forms or is not a date of the calendar.
pub fn parse_csv_date(raw: &str) -> CsvResult<Date> {
    let cell = raw.trim();
    if cell.is_empty() {
        return Err(CsvError::MissingDate);
    }
    let invalid = || CsvError::InvalidDate(cell.to_owned());

    // The first separator in the cell is the one the whole date has to use,
    // so `05.03-2026` does not split into three and is rejected.
    let separator = cell
        .chars()
        .find(|character| DATE_SEPARATORS.contains(character));
    let [first, second, third] = separator
        .and_then(|separator| split_three(cell, separator))
        .ok_or_else(invalid)?;

    if first.len() == YEAR_DIGITS {
        calendar_date(first, second, third, cell)
    } else if third.len() == YEAR_DIGITS {
        calendar_date(third, second, first, cell)
    } else {
        Err(invalid())
    }
}

/// The characters a date cell may have between its three numbers.
const DATE_SEPARATORS: [char; 3] = ['-', '/', '.'];

/// The number of digits a year is written with. A date with a shorter or
/// longer year is rejected, so a segment of this length is the year.
const YEAR_DIGITS: usize = 4;

/// Parses a bank CSV into per-row outcomes. Never writes to the ledger.
///
/// Accepts comma, semicolon or tab delimiters, detected from the header row,
/// and RFC 4180 quoted fields.
/// `currency` is the base currency of the book the statement is for: its
/// number of decimals decides how an amount is read, and an amount cell
/// marked with the code of another currency, in capitals, is an invalid row.
/// `mapping` replaces header auto-detection when `Some`.
///
/// Without a `mapping`, a file in which no date column, or no amount, debit
/// or credit column, is detected is returned with those columns in
/// [`ParsedBankCsv::missing_columns`] and no rows. That is not an error,
/// because a mapping can still read the file.
///
/// # Errors
///
/// [`Error::Csv`] for a problem with the file as a whole:
/// [`CsvError::Empty`] when the text is empty, [`CsvError::Malformed`] when
/// the header row cannot be read, [`CsvError::MissingHeader`] when it has no
/// name in it, and [`CsvError::InvalidMapping`] when `mapping` is
/// incomplete, contradictory or names a header the file does not have.
///
/// A malformed **row** is not an error. It is returned as
/// [`CsvRowOutcome::Invalid`] in its place among the rows.
pub fn parse_bank_csv(
    text: &str,
    currency: CurrencyCode,
    mapping: Option<&CsvColumnMapping>,
) -> crate::error::Result<ParsedBankCsv> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(CsvError::Empty.into());
    }
    let delimiter = detect_delimiter(trimmed);
    let mut reader = ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(true)
        .trim(Trim::All)
        .from_reader(trimmed.as_bytes());

    let headers = reader
        .headers()
        .map_err(|err| CsvError::Malformed {
            detail: err.to_string(),
        })?
        .clone();
    if headers.is_empty() || headers.iter().all(str::is_empty) {
        return Err(CsvError::MissingHeader.into());
    }

    // Read whole before any column is chosen: the direction column is
    // detected from its values. The text is already in memory, and a file is
    // at most `MAX_CSV_BYTES`.
    let records: Vec<csv::Result<StringRecord>> = reader.records().collect();

    let detected = auto_map_headers(&headers, &records);
    let (missing_columns, rows) = match mapping {
        Some(user) => {
            let columns = resolve_user_mapping(&headers, user)?;
            (Vec::new(), parse_records(records, columns, currency))
        }
        None => match missing_required_columns(detected) {
            missing if missing.is_empty() => (missing, parse_records(records, detected, currency)),
            missing => (missing, Vec::new()),
        },
    };

    Ok(ParsedBankCsv {
        headers: headers.iter().map(str::to_owned).collect(),
        detected_mapping: mapping_from_headers(&headers, detected),
        missing_columns,
        rows,
    })
}

/// The role a column plays in a bank statement.
#[derive(Debug, Clone, Copy)]
enum Column {
    /// Booking date.
    Date,
    /// Free text: payee, memo, narration.
    Description,
    /// Signed amount.
    Amount,
    /// Money out, in a file with separate columns per direction.
    Debit,
    /// Money in, in a file with separate columns per direction.
    Credit,
    /// Reference or check number.
    Reference,
    /// Which way the money moved, for a file whose amounts are unsigned.
    Direction,
}

/// Where each role is in a record, as 0-based cell indexes.
///
/// `None` means the role has no column and is not read.
#[derive(Debug, Clone, Copy, Default)]
struct ColumnMap {
    /// Booking date. Always `Some` in a map rows are read with: one for
    /// which [`missing_required_columns`] is empty, or one from
    /// [`resolve_user_mapping`].
    date: Option<usize>,
    /// Description; a row of a file without one gets an empty description.
    description: Option<usize>,
    /// Signed amount. When `Some`, `debit` and `credit` are not read.
    amount: Option<usize>,
    /// Money out.
    debit: Option<usize>,
    /// Money in.
    credit: Option<usize>,
    /// Reference or check number.
    reference: Option<usize>,
    /// Direction of the amount. Read only when `amount` is `Some`.
    direction: Option<usize>,
}

/// Splits `text` on `separator` into exactly three segments, or returns
/// `None` when there are fewer or more.
fn split_three(text: &str, separator: char) -> Option<[&str; 3]> {
    let mut segments = text.split(separator);
    let first = segments.next()?;
    let second = segments.next()?;
    let third = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    Some([first, second, third])
}

/// Builds the calendar date from its three segments as written.
///
/// # Errors
///
/// [`CsvError::InvalidDate`] carrying `raw` when a segment is not made of
/// ASCII digits or the three do not name a day that exists (`31/02`).
fn calendar_date(year: &str, month: &str, day: &str, raw: &str) -> CsvResult<Date> {
    let invalid = || CsvError::InvalidDate(raw.to_owned());

    // `str::parse` takes a leading sign, so `+5/+3/2026` would be a date and
    // `5.3.-026` a year before the common era.
    let all_digits = |segment: &str| segment.bytes().all(|byte| byte.is_ascii_digit());
    if ![year, month, day].into_iter().all(all_digits) {
        return Err(invalid());
    }

    let year: i32 = year.parse().map_err(|_| invalid())?;
    let month: u8 = month.parse().map_err(|_| invalid())?;
    let day: u8 = day.parse().map_err(|_| invalid())?;

    let month = Month::try_from(month).map_err(|_| invalid())?;
    Date::from_calendar_date(year, month, day).map_err(|_| invalid())
}

/// The delimiters a statement may use, in the order a tie is settled: the
/// earlier one wins.
const DELIMITERS: [u8; 3] = *b",;\t";

/// Picks the delimiter from the first non-empty line of `text`, the header:
/// the one of [`DELIMITERS`] that splits it into the most fields.
///
/// A delimiter inside double quotes does not split. A later delimiter needs
/// strictly more fields than an earlier one, so a header that none of them
/// splits, and any tie, goes to the comma, and a tie between semicolon and
/// tab to the semicolon.
fn detect_delimiter(text: &str) -> u8 {
    let Some(header) = text.lines().find(|line| !line.trim().is_empty()) else {
        return b',';
    };

    let mut counts = [0_usize; DELIMITERS.len()];
    let mut in_quotes = false;
    for byte in header.bytes() {
        if byte == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            for (count, delimiter) in counts.iter_mut().zip(DELIMITERS) {
                if byte == delimiter {
                    *count += 1;
                }
            }
        }
    }

    // Strictly more, so an earlier delimiter keeps a tie.
    let mut chosen = (b',', 0);
    for (delimiter, count) in DELIMITERS.into_iter().zip(counts) {
        if count > chosen.1 {
            chosen = (delimiter, count);
        }
    }
    chosen.0
}

/// Detects the column of each role from the header names, and for the
/// direction column from the values of `records` as well.
///
/// The leftmost header of a role wins; a later one of the same role is left
/// unread. A header named like a direction column whose values are not
/// directions ([`holds_only_directions`]) has no role, and a later one may
/// take it.
fn auto_map_headers(headers: &StringRecord, records: &[csv::Result<StringRecord>]) -> ColumnMap {
    let mut map = ColumnMap::default();
    for (index, name) in headers.iter().enumerate() {
        let Some(kind) = classify_header(name) else {
            continue;
        };
        if matches!(kind, Column::Direction) && !holds_only_directions(records, index) {
            continue;
        }
        match kind {
            Column::Date if map.date.is_none() => map.date = Some(index),
            Column::Description if map.description.is_none() => map.description = Some(index),
            Column::Amount if map.amount.is_none() => map.amount = Some(index),
            Column::Debit if map.debit.is_none() => map.debit = Some(index),
            Column::Credit if map.credit.is_none() => map.credit = Some(index),
            Column::Reference if map.reference.is_none() => map.reference = Some(index),
            Column::Direction if map.direction.is_none() => map.direction = Some(index),
            Column::Date
            | Column::Description
            | Column::Amount
            | Column::Debit
            | Column::Credit
            | Column::Reference
            | Column::Direction => {}
        }
    }
    map
}

/// The number of data rows the direction column is detected from.
///
/// Enough to see both directions and any other word a bank writes in such a
/// column, and few enough that one odd cell far down a long statement makes
/// that row invalid instead of taking the column away from every row.
const DIRECTION_SAMPLE_ROWS: usize = 50;

/// Returns whether column `index` can be read as the direction column: every
/// non-empty cell of it in the first [`DIRECTION_SAMPLE_ROWS`] records is a
/// direction word.
///
/// A record among them that could not be read, or that is too short to have
/// the cell, has no word to test. A column with no value in those records
/// passes, since an empty direction cell leaves the amount's own sign.
fn holds_only_directions(records: &[csv::Result<StringRecord>], index: usize) -> bool {
    records
        .iter()
        .take(DIRECTION_SAMPLE_ROWS)
        .filter_map(|record| record.as_ref().ok())
        .map(|record| record_cell(record, index).trim())
        .filter(|cell| !cell.is_empty())
        .all(|cell| Direction::of_word(cell).is_some())
}

/// Returns the columns a row cannot do without that `map` lacks, date before
/// amount: the date, and the amount when none of amount, debit and credit
/// was found.
fn missing_required_columns(map: ColumnMap) -> Vec<CsvRequiredColumn> {
    let has_amount = map.amount.is_some() || map.debit.is_some() || map.credit.is_some();
    [
        (map.date.is_none(), CsvRequiredColumn::Date),
        (!has_amount, CsvRequiredColumn::Amount),
    ]
    .into_iter()
    .filter_map(|(missing, column)| missing.then_some(column))
    .collect()
}

/// Turns a map of cell indexes back into header names, for the UI to show
/// what was detected.
fn mapping_from_headers(headers: &StringRecord, map: ColumnMap) -> CsvColumnMapping {
    let name =
        |column: Option<usize>| column.map(|index| headers.get(index).unwrap_or("").to_owned());
    CsvColumnMapping {
        date: name(map.date),
        description: name(map.description),
        amount: name(map.amount),
        debit: name(map.debit),
        credit: name(map.credit),
        reference: name(map.reference),
        direction: name(map.direction),
    }
}

/// Returns the trimmed value, or `None` when it is absent or blank.
///
/// A mapping field the UI sends as an empty string means "not mapped".
fn trimmed_nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// Returns the index of the first header equal to `name`, ignoring ASCII
/// case and surrounding whitespace.
///
/// # Errors
///
/// [`CsvError::InvalidMapping`] naming the header when the file has none.
fn header_index(headers: &StringRecord, name: &str) -> CsvResult<usize> {
    let needle = name.trim();
    headers
        .iter()
        .position(|header| header.eq_ignore_ascii_case(needle))
        .ok_or_else(|| {
            CsvError::InvalidMapping(CsvMappingProblem::UnknownColumn {
                name: name.to_owned(),
            })
        })
}

/// Resolves an explicit mapping to cell indexes.
///
/// # Errors
///
/// [`Error::Csv`] with [`CsvError::InvalidMapping`] when the mapping
/// has no date, sets both an amount and a debit or credit column, sets none
/// of amount, debit and credit, or names a header the file does not have.
fn resolve_user_mapping(
    headers: &StringRecord,
    mapping: &CsvColumnMapping,
) -> crate::error::Result<ColumnMap> {
    let Some(date) = trimmed_nonempty(mapping.date.as_deref()) else {
        return Err(CsvError::InvalidMapping(CsvMappingProblem::MissingDate).into());
    };
    // Optional, as it is for detection: a row without one gets an empty
    // description, which the ledger accepts.
    let description = trimmed_nonempty(mapping.description.as_deref());
    let amount = trimmed_nonempty(mapping.amount.as_deref());
    let debit = trimmed_nonempty(mapping.debit.as_deref());
    let credit = trimmed_nonempty(mapping.credit.as_deref());
    let reference = trimmed_nonempty(mapping.reference.as_deref());
    let direction = trimmed_nonempty(mapping.direction.as_deref());

    let has_amount = amount.is_some();
    let has_debit_or_credit = debit.is_some() || credit.is_some();
    if has_amount && has_debit_or_credit {
        return Err(CsvError::InvalidMapping(CsvMappingProblem::AmountAndDebitOrCredit).into());
    }
    // One of the two columns is enough, as it is for detection: a file may
    // list only what left the account, or only what reached it.
    if !has_amount && !has_debit_or_credit {
        return Err(CsvError::InvalidMapping(CsvMappingProblem::MissingAmount).into());
    }

    let optional_index =
        |name: Option<&str>| name.map(|name| header_index(headers, name)).transpose();

    Ok(ColumnMap {
        date: Some(header_index(headers, date)?),
        description: optional_index(description)?,
        amount: optional_index(amount)?,
        debit: optional_index(debit)?,
        credit: optional_index(credit)?,
        reference: optional_index(reference)?,
        direction: optional_index(direction)?,
    })
}

/// Names the role of a header, or returns `None` for one that has none.
///
/// The header is compared in lowercase with spaces, `_` and `-` removed, so
/// `Booking Date`, `booking_date` and `BOOKING-DATE` are one header. The
/// tests run in the order of the table in the module doc; `date` is the only
/// one that matches a part of the header.
fn classify_header(raw: &str) -> Option<Column> {
    let compact = fold_header(raw);

    if DATE_WORDS.iter().any(|word| compact.contains(word)) {
        return Some(Column::Date);
    }
    ROLE_ALIASES
        .iter()
        .find(|(_, aliases)| aliases.contains(&compact.as_str()))
        .map(|(column, _)| *column)
}

/// Words a date header contains: English and French `date`, German `datum`,
/// Greek `ημερομηνια` (`Ημερομηνία`, folded), and the German `buchungstag` and
/// `wertstellung`.
const DATE_WORDS: [&str; 5] = ["date", "datum", "ημερομηνια", "buchungstag", "wertstellung"];

/// The whole-header aliases of each role other than the date, in the order
/// the module doc lists the tests. Each is in the form [`fold_header`]
/// leaves a header in.
const ROLE_ALIASES: [(Column, &[&str]); 6] = [
    (
        Column::Amount,
        &[
            "amount",
            "value",
            "sum",
            "transactionamount",
            "betrag",
            "montant",
            "importo",
            "ποσο",
            "ποσοσυναλλαγησ",
        ],
    ),
    (
        Column::Debit,
        &[
            "debit",
            "withdrawal",
            "outflow",
            "addebito",
            "soll",
            "χρεωση",
        ],
    ),
    (
        Column::Credit,
        &[
            "credit",
            "deposit",
            "inflow",
            "accredito",
            "haben",
            "πιστωση",
        ],
    ),
    (
        Column::Direction,
        &[
            "type",
            "dc",
            "d/c",
            "debitcredit",
            "drcr",
            "transactiontype",
        ],
    ),
    (
        Column::Reference,
        &[
            "reference",
            "ref",
            "check",
            "cheque",
            "checkno",
            "chequeno",
            "fitid",
            "referenz",
            "αναφορα",
            "αριθμοσαναφορασ",
        ],
    ),
    (
        Column::Description,
        &[
            "description",
            "memo",
            "narration",
            "details",
            "payee",
            "particulars",
            "narrative",
            "libelle",
            "beschreibung",
            "descrizione",
            "transaction",
            "name",
            "verwendungszweck",
            "buchungstext",
            "intitule",
            "περιγραφη",
            "αιτιολογια",
        ],
    ),
];

/// Lowercases a header and drops what banks vary freely: spaces, `_`, `-`,
/// Greek and common Latin accents, and the final sigma.
///
/// Folding the accents lets one alias match `Ποσό` and an upper-case `ΠΟΣΟ`
/// that carries none, and `Libellé` and `LIBELLE`.
fn fold_header(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '_' | '-'))
        .map(|character| match character {
            'ά' => 'α',
            'έ' => 'ε',
            'ή' => 'η',
            'ί' | 'ϊ' | 'ΐ' => 'ι',
            'ό' => 'ο',
            'ύ' | 'ϋ' | 'ΰ' => 'υ',
            'ώ' => 'ω',
            'ς' => 'σ',
            'é' | 'è' | 'ê' => 'e',
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            other => other,
        })
        .collect()
}
/// Reads every record into its outcome, in file order.
///
/// A record the reader could not split becomes an unreadable row, and its
/// cause is logged.
fn parse_records(
    records: Vec<csv::Result<StringRecord>>,
    columns: ColumnMap,
    currency: CurrencyCode,
) -> Vec<CsvRowOutcome> {
    let mut rows = Vec::with_capacity(records.len());
    for (index, record) in records.into_iter().enumerate() {
        // The header is record 1.
        let source_row = u32::try_from(index + 2).unwrap_or(u32::MAX);
        match record {
            Ok(record) => rows.push(parse_record(source_row, &record, columns, currency)),
            Err(err) => {
                log::warn!(
                    "CSV record {source_row} could not be read: {}",
                    PrivateDetail(&err)
                );
                rows.push(CsvRowOutcome::Invalid {
                    source_row,
                    reason: UiText::new(UiTextCode::CsvUnreadableRow),
                });
            }
        }
    }
    rows
}

/// Reads one record into its outcome; a row that cannot be used becomes
/// [`CsvRowOutcome::Invalid`] with the reason worded for the UI.
fn parse_record(
    source_row: u32,
    record: &StringRecord,
    columns: ColumnMap,
    currency: CurrencyCode,
) -> CsvRowOutcome {
    match parse_record_inner(source_row, record, columns, currency) {
        Ok(row) => CsvRowOutcome::Parsed(row),
        Err(err) => CsvRowOutcome::Invalid {
            source_row,
            reason: row_problem(source_row, &err),
        },
    }
}

/// The code and value that say why a row cannot be used.
///
/// The offending cell travels as the `value` parameter, as written. A problem
/// that is not about one row's cells cannot come out of reading a row; if one
/// ever does, the row is reported as unreadable and the cause is logged.
fn row_problem(source_row: u32, err: &CsvError) -> UiText {
    match err {
        CsvError::InvalidDate(value) => {
            UiText::new(UiTextCode::CsvInvalidDate).with_param("value", value.as_str())
        }
        CsvError::InvalidAmount(value) => {
            UiText::new(UiTextCode::CsvInvalidAmount).with_param("value", value.as_str())
        }
        CsvError::InvalidType(value) => {
            UiText::new(UiTextCode::CsvInvalidType).with_param("value", value.as_str())
        }
        CsvError::MissingDate => UiText::new(UiTextCode::CsvMissingDate),
        CsvError::MissingAmount => UiText::new(UiTextCode::CsvMissingAmount),
        CsvError::ZeroAmount => UiText::new(UiTextCode::CsvZeroAmount),
        CsvError::AmountOverflow => UiText::new(UiTextCode::CsvAmountOverflow),
        CsvError::Empty
        | CsvError::NotUtf8
        | CsvError::TooLarge
        | CsvError::Malformed { .. }
        | CsvError::MissingHeader
        | CsvError::MissingColumn { .. }
        | CsvError::InvalidStatus(_)
        | CsvError::InvalidInteger(_)
        | CsvError::InvalidMapping(_) => {
            // The code as well, because the reason itself can quote a cell.
            log::warn!(
                "CSV record {source_row} could not be read: {}: {}",
                err.code(),
                PrivateDetail(err)
            );
            UiText::new(UiTextCode::CsvUnreadableRow)
        }
    }
}

/// Reads one record into a bank row.
///
/// # Errors
///
/// The [`CsvError`] of the first cell that cannot be read, in the order
/// date, amount; [`CsvError::ZeroAmount`] for an amount of zero, which a
/// simple entry cannot carry.
fn parse_record_inner(
    source_row: u32,
    record: &StringRecord,
    columns: ColumnMap,
    currency: CurrencyCode,
) -> CsvResult<ParsedBankRow> {
    let Some(date_index) = columns.date else {
        return Err(CsvError::MissingDate);
    };
    let entry_date = parse_csv_date(record_cell(record, date_index))?;
    let description = columns
        .description
        .map(|index| record_cell(record, index).trim().to_owned())
        .unwrap_or_default();
    let reference = columns.reference.and_then(|index| {
        let value = record_cell(record, index).trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_owned())
        }
    });

    let RecordAmount {
        signed,
        legacy_signed,
    } = signed_amount(record, columns, currency)?;
    if signed == 0 {
        return Err(CsvError::ZeroAmount);
    }
    let amount_minor = signed.checked_abs().ok_or(CsvError::AmountOverflow)?;
    let kind = if signed < 0 {
        SimpleEntryKind::Expense
    } else {
        SimpleEntryKind::Income
    };

    Ok(ParsedBankRow {
        source_row,
        entry_date,
        description,
        reference,
        signed_amount_minor: signed,
        legacy_signed_amount_minor: legacy_signed,
        amount_minor,
        kind,
    })
}

/// The signed amount of a record, and the one Oikonomia 0.1.0 read for it
/// when that one was different.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecordAmount {
    /// Negative for money out.
    signed: i64,
    /// See [`ParsedBankRow::legacy_signed_amount_minor`].
    legacy_signed: Option<i64>,
}

/// Returns the signed amount of a record: negative for money out.
///
/// With an amount column, its own sign counts unless a direction cell says
/// otherwise. With debit and credit columns a debit is money out and a
/// credit is money in, each by the amount its cell holds with the sign it
/// was written with: the row is the credit less the debit. The table in the
/// module doc has the cases.
///
/// # Errors
///
/// The error of [`parse_book_amount`] for a cell that is not an amount,
/// [`CsvError::MissingAmount`] when the debit and the credit cell are both
/// blank, [`CsvError::InvalidType`] for a direction cell that names no
/// direction, and [`CsvError::AmountOverflow`] when the credit less the
/// debit does not fit in `i64`.
fn signed_amount(
    record: &StringRecord,
    columns: ColumnMap,
    currency: CurrencyCode,
) -> CsvResult<RecordAmount> {
    if let Some(amount_index) = columns.amount {
        let signed = parse_book_amount(record_cell(record, amount_index), currency)?;
        let signed = match columns.direction {
            Some(direction_index) => apply_direction(signed, record_cell(record, direction_index))?,
            None => signed,
        };
        // 0.1.0 read an amount column the same way.
        return Ok(RecordAmount {
            signed,
            legacy_signed: None,
        });
    }

    let debit = optional_signed(record, columns.debit, currency)?;
    let credit = optional_signed(record, columns.credit, currency)?;
    if debit.is_none() && credit.is_none() {
        return Err(CsvError::MissingAmount);
    }
    let signed = credit
        .unwrap_or(0)
        .checked_sub(debit.unwrap_or(0))
        .ok_or(CsvError::AmountOverflow)?;
    Ok(RecordAmount {
        signed,
        legacy_signed: amount_without_cell_signs(debit, credit).filter(|legacy| *legacy != signed),
    })
}

/// Returns the signed amount Oikonomia 0.1.0 gave a row of debit and credit
/// cells: each cell without its sign, the credit less the debit.
///
/// It differs from the signed amount only when a cell is written negative.
/// `None` when it is zero, which 0.1.0 refused as a row, or when a cell is
/// `i64::MIN`, which has no magnitude in `i64`.
fn amount_without_cell_signs(debit: Option<i64>, credit: Option<i64>) -> Option<i64> {
    let debit = debit.unwrap_or(0).checked_abs()?;
    let credit = credit.unwrap_or(0).checked_abs()?;
    credit.checked_sub(debit).filter(|minor| *minor != 0)
}

/// Parses the amount in column `index`, or returns `None` when there is no
/// such column or its cell is blank.
fn optional_signed(
    record: &StringRecord,
    index: Option<usize>,
    currency: CurrencyCode,
) -> CsvResult<Option<i64>> {
    let Some(index) = index else {
        return Ok(None);
    };
    let raw = record_cell(record, index).trim();
    if raw.is_empty() {
        return Ok(None);
    }
    parse_book_amount(raw, currency).map(Some)
}

/// Which way a direction cell says the money moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// Money leaving the account.
    Out,
    /// Money reaching the account.
    In,
}

impl Direction {
    /// Returns the direction `word` names, or `None` for any other text,
    /// the empty one included.
    ///
    /// The words are those of the table in the module doc, matched whole
    /// after trimming and without regard to case. Detection of the direction
    /// column and the reading of its cells both go through here, so within
    /// the rows detection looked at, a detected column holds no word a row
    /// is then refused for.
    fn of_word(word: &str) -> Option<Self> {
        match word.trim().to_lowercase().as_str() {
            "d" | "dr" | "debit" | "withdrawal" | "expense" | "out" => Some(Self::Out),
            "c" | "cr" | "credit" | "deposit" | "income" | "in" => Some(Self::In),
            _ => None,
        }
    }
}

/// Gives `signed` the sign its direction cell names, whatever sign it had.
///
/// A blank cell leaves the amount as it is.
///
/// # Errors
///
/// [`CsvError::InvalidType`] carrying the trimmed cell for a word that is
/// not a direction ([`Direction::of_word`]), and
/// [`CsvError::AmountOverflow`] for `i64::MIN`, which has no magnitude.
fn apply_direction(signed: i64, raw: &str) -> CsvResult<i64> {
    let word = raw.trim();
    if word.is_empty() {
        return Ok(signed);
    }
    let direction =
        Direction::of_word(word).ok_or_else(|| CsvError::InvalidType(word.to_owned()))?;
    let magnitude = signed.checked_abs().ok_or(CsvError::AmountOverflow)?;

    Ok(match direction {
        Direction::Out => -magnitude,
        Direction::In => magnitude,
    })
}

/// Returns the cell at `index`, or an empty string for a record that is
/// shorter than the header.
fn record_cell(record: &StringRecord, index: usize) -> &str {
    record.get(index).unwrap_or("")
}

#[cfg(test)]
#[expect(clippy::panic, reason = "tests fail loudly by design")]
mod tests {
    use super::*;

    /// The error a mapping with `problem` is refused with.
    fn invalid_mapping(problem: CsvMappingProblem) -> Error {
        Error::Csv(CsvError::InvalidMapping(problem))
    }

    /// The currency of the book the test statements are read for.
    fn eur() -> CurrencyCode {
        "EUR".parse().expect("a currency code")
    }

    /// The problem of a mapping that names a header the file does not have.
    fn unknown_column(name: &str) -> CsvMappingProblem {
        CsvMappingProblem::UnknownColumn {
            name: name.to_owned(),
        }
    }

    fn parse_rows(text: &str) -> Vec<CsvRowOutcome> {
        parse_bank_csv(text, eur(), None).expect("parse csv").rows
    }

    fn first_parsed_row(text: &str) -> ParsedBankRow {
        match parse_rows(text).into_iter().next() {
            Some(CsvRowOutcome::Parsed(row)) => row,
            other => panic!("expected parsed row, got {other:?}"),
        }
    }

    #[test]
    fn comma_iso_and_dot_amount() {
        let row = first_parsed_row("Date,Description,Amount\n2026-03-15,Coffee,-3.50\n");
        assert_eq!(crate::util::format_date(row.entry_date), "2026-03-15");
        assert_eq!(row.description, "Coffee");
        assert_eq!(row.signed_amount_minor, -350);
        assert_eq!(row.kind, SimpleEntryKind::Expense);
        assert_eq!(row.source_row, 2);
    }

    #[test]
    fn semicolon_quotes_european_date_and_amount() {
        let csv = "Date;Description;Amount\n15/03/2026;\"Coffee, Inc\";-1.234,56\n";
        let row = first_parsed_row(csv);
        assert_eq!(crate::util::format_date(row.entry_date), "2026-03-15");
        assert_eq!(row.description, "Coffee, Inc");
        assert_eq!(row.signed_amount_minor, -123_456);
        assert_eq!(row.kind, SimpleEntryKind::Expense);
    }

    #[test]
    fn a_tab_delimited_file_is_read() {
        let csv = "Date\tDescription\tAmount\n15.03.2026\tCoffee, Inc; Zürich\t-1'234.56\n";
        let row = first_parsed_row(csv);

        assert_eq!(row.entry_date, time::macros::date!(2026 - 03 - 15));
        assert_eq!(row.description, "Coffee, Inc; Zürich");
        assert_eq!(row.signed_amount_minor, -123_456);
    }

    #[test]
    fn the_delimiter_is_the_one_that_splits_the_header_into_the_most_fields() {
        assert_eq!(detect_delimiter("Date,Description,Amount"), b',');
        assert_eq!(detect_delimiter("Date;Description;Amount"), b';');
        assert_eq!(detect_delimiter("Date\tDescription\tAmount"), b'\t');
        assert_eq!(detect_delimiter("Date;Amount, EUR;Text"), b';');
        assert_eq!(detect_delimiter("Date\tAmount; EUR\tText, long"), b'\t');
        assert_eq!(
            detect_delimiter("\n\n  \nDate;Text;Amount\n1,5;2,5;3,5"),
            b';'
        );
    }

    #[test]
    fn a_tie_between_delimiters_goes_to_the_comma_then_the_semicolon() {
        assert_eq!(detect_delimiter("Date"), b',', "no delimiter at all");
        assert_eq!(detect_delimiter("Date,Text;Amount"), b',');
        assert_eq!(detect_delimiter("Date,Text\tAmount"), b',');
        assert_eq!(detect_delimiter("Date;Text\tAmount"), b';');
        assert_eq!(detect_delimiter(""), b',');
    }

    #[test]
    fn a_delimiter_inside_quotes_does_not_count() {
        assert_eq!(detect_delimiter("\"a;b;c\",Date,Amount"), b',');
        assert_eq!(detect_delimiter("\"a,b,c\"\tDate\tAmount"), b'\t');
    }

    #[test]
    fn positive_amount_is_income() {
        let row = first_parsed_row("date,memo,amount\n2026-01-02,Salary,\"1234,56\"\n");
        assert_eq!(row.kind, SimpleEntryKind::Income);
        assert_eq!(row.amount_minor, 123_456);
        assert_eq!(row.signed_amount_minor, 123_456);
    }

    #[test]
    fn a_trailing_minus_amount_is_an_expense() {
        let row = first_parsed_row("Date,Description,Amount\n2026-03-15,Coffee,3.50-\n");
        assert_eq!(row.signed_amount_minor, -350);
        assert_eq!(row.kind, SimpleEntryKind::Expense);
    }

    #[test]
    fn debit_credit_columns() {
        let csv =
            "Date,Description,Debit,Credit\n01/04/2026,Rent,800.00,\n01/04/2026,Pay,,2500.00\n";
        let rows = parse_rows(csv);
        let CsvRowOutcome::Parsed(rent) = &rows[0] else {
            panic!("rent");
        };
        let CsvRowOutcome::Parsed(pay) = &rows[1] else {
            panic!("pay");
        };
        assert_eq!(rent.kind, SimpleEntryKind::Expense);
        assert_eq!(rent.amount_minor, 80_000);
        assert_eq!(pay.kind, SimpleEntryKind::Income);
        assert_eq!(pay.amount_minor, 250_000);
    }

    /// The signed amount of the only row of a file with a debit and a credit
    /// column, whose two cells are `debit` and `credit`.
    fn debit_credit_amount(debit: &str, credit: &str) -> i64 {
        let csv = format!("Date,Description,Debit,Credit\n2026-04-01,Row,{debit},{credit}\n");
        first_parsed_row(&csv).signed_amount_minor
    }

    #[test]
    fn a_negative_debit_is_money_in_and_a_negative_credit_is_money_out() {
        assert_eq!(debit_credit_amount("800.00", ""), -80_000);
        assert_eq!(
            debit_credit_amount("-800.00", ""),
            80_000,
            "a reversed debit"
        );
        assert_eq!(debit_credit_amount("(800.00)", ""), 80_000);
        assert_eq!(debit_credit_amount("", "2500.00"), 250_000);
        assert_eq!(
            debit_credit_amount("", "-2500.00"),
            -250_000,
            "a reversed credit"
        );
        assert_eq!(debit_credit_amount("", "2500.00-"), -250_000);

        let reversal =
            first_parsed_row("Date,Description,Debit,Credit\n2026-04-01,Refund,-12.00,\n");
        assert_eq!(reversal.kind, SimpleEntryKind::Income);
        assert_eq!(reversal.amount_minor, 1_200);
    }

    /// The amount 0.1.0 read for the only row of a file with a debit and a
    /// credit column, whose two cells are `debit` and `credit`.
    fn legacy_debit_credit_amount(debit: &str, credit: &str) -> Option<i64> {
        let csv = format!("Date,Description,Debit,Credit\n2026-04-01,Row,{debit},{credit}\n");
        first_parsed_row(&csv).legacy_signed_amount_minor
    }

    #[test]
    fn only_a_negative_debit_or_credit_cell_carries_the_amount_0_1_0_read() {
        assert_eq!(legacy_debit_credit_amount("-5.00", ""), Some(-500));
        assert_eq!(legacy_debit_credit_amount("", "-6.00"), Some(600));
        assert_eq!(legacy_debit_credit_amount("-3.00", "-1.00"), Some(-200));
        assert_eq!(legacy_debit_credit_amount("-100.00", "30.00"), Some(-7_000));
        assert_eq!(legacy_debit_credit_amount("(800.00)", ""), Some(-80_000));
        assert_eq!(
            legacy_debit_credit_amount("-25.00", "25.00"),
            None,
            "0.1.0 read a zero, which it refused"
        );

        for (debit, credit) in [
            ("10.00", ""),
            ("", "20.00"),
            ("7.00", "2.00"),
            ("0.00", "25.00"),
        ] {
            assert_eq!(
                legacy_debit_credit_amount(debit, credit),
                None,
                "{debit},{credit}"
            );
        }
        for amount in ["-25.00", "25.00", "25.00-", "(25.00)"] {
            let csv = format!("Date,Description,Amount\n2026-04-01,Row,{amount}\n");
            assert_eq!(
                first_parsed_row(&csv).legacy_signed_amount_minor,
                None,
                "{amount}"
            );
        }
    }

    #[test]
    fn a_row_with_both_a_debit_and_a_credit_is_the_credit_less_the_debit() {
        assert_eq!(debit_credit_amount("100.00", "30.00"), -7_000);
        assert_eq!(debit_credit_amount("30.00", "100.00"), 7_000);
        assert_eq!(
            debit_credit_amount("0.00", "25.00"),
            2_500,
            "a zero for a blank"
        );
        assert_eq!(debit_credit_amount("25.00", "0.00"), -2_500);
        assert_eq!(debit_credit_amount("-100.00", "30.00"), 13_000);
        assert_eq!(debit_credit_amount("100.00", "-30.00"), -13_000);

        assert_eq!(
            reason_of_only_row("Date,Description,Debit,Credit\n2026-04-01,Row,40.00,40.00\n"),
            UiText::new(UiTextCode::CsvZeroAmount),
            "equal cells cancel, and a row of nothing cannot be posted"
        );
    }

    #[test]
    fn an_amount_in_another_currency_is_an_invalid_row_in_any_amount_column() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n2026-03-15,Hotel,-25.00 USD\n"),
            UiText::new(UiTextCode::CsvInvalidAmount).with_param("value", "-25.00 USD")
        );
        assert_eq!(
            reason_of_only_row("Date,Description,Debit,Credit\n2026-03-15,Hotel,USD 25,\n"),
            UiText::new(UiTextCode::CsvInvalidAmount).with_param("value", "USD 25")
        );
        assert_eq!(
            reason_of_only_row("Date,Description,Debit,Credit\n2026-03-15,Hotel,,25 GBP\n"),
            UiText::new(UiTextCode::CsvInvalidAmount).with_param("value", "25 GBP")
        );

        let usd: CurrencyCode = "USD".parse().unwrap();
        let in_a_dollar_book = parse_bank_csv(
            "Date,Description,Amount\n2026-03-15,Hotel,-25.00 USD\n",
            usd,
            None,
        )
        .unwrap();
        assert!(matches!(
            in_a_dollar_book.rows.as_slice(),
            [CsvRowOutcome::Parsed(row)] if row.signed_amount_minor == -2_500
        ));
    }

    #[test]
    fn a_statement_is_read_with_the_decimals_of_the_books_currency() {
        let yen: CurrencyCode = "JPY".parse().unwrap();
        let parsed = parse_bank_csv(
            "Date,Description,Amount\n2026-03-15,Ramen,\"-1,234\"\n",
            yen,
            None,
        )
        .unwrap();

        assert!(matches!(
            parsed.rows.as_slice(),
            [CsvRowOutcome::Parsed(row)] if row.signed_amount_minor == -1_234
        ));
    }

    #[test]
    fn junk_rows_are_invalid_not_file_errors() {
        let csv = "Date,Description,Amount\nnot-a-date,X,1.00\n2026-03-15,Y,abc\n2026-03-15,Z,0\n";
        let rows = parse_rows(csv);
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert!(
                matches!(row, CsvRowOutcome::Invalid { .. }),
                "expected invalid, got {row:?}"
            );
        }
    }

    /// The reason a single bad row is reported with.
    fn reason_of_only_row(csv: &str) -> UiText {
        match parse_rows(csv).into_iter().next() {
            Some(CsvRowOutcome::Invalid { reason, .. }) => reason,
            other => panic!("expected an invalid row, got {other:?}"),
        }
    }

    #[test]
    fn a_bad_date_is_reported_with_the_cell() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\nnot-a-date,X,1.00\n"),
            UiText::new(UiTextCode::CsvInvalidDate).with_param("value", "not-a-date")
        );
    }

    #[test]
    fn a_bad_amount_is_reported_with_the_cell() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n2026-03-15,Y,abc\n"),
            UiText::new(UiTextCode::CsvInvalidAmount).with_param("value", "abc")
        );
    }

    #[test]
    fn an_empty_date_cell_is_reported_as_missing() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n,X,1.00\n"),
            UiText::new(UiTextCode::CsvMissingDate)
        );
    }

    #[test]
    fn a_whitespace_only_date_cell_is_reported_as_missing() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n   ,X,1.00\n"),
            UiText::new(UiTextCode::CsvMissingDate)
        );
    }

    #[test]
    fn an_empty_amount_cell_is_reported_as_missing() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n2026-03-15,Y,\n"),
            UiText::new(UiTextCode::CsvMissingAmount)
        );
    }

    #[test]
    fn a_whitespace_only_amount_cell_is_reported_as_missing() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n2026-03-15,Y,   \n"),
            UiText::new(UiTextCode::CsvMissingAmount)
        );
    }

    #[test]
    fn empty_debit_and_credit_cells_are_reported_as_a_missing_amount() {
        assert_eq!(
            reason_of_only_row("Date,Description,Debit,Credit\n2026-03-15,Y,,\n"),
            UiText::new(UiTextCode::CsvMissingAmount)
        );
    }

    #[test]
    fn a_zero_amount_is_reported_without_a_value() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n2026-03-15,Z,0\n"),
            UiText::new(UiTextCode::CsvZeroAmount)
        );
    }

    #[test]
    fn an_amount_too_large_for_the_ledger_is_reported_as_overflow() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount\n2026-03-15,Big,99999999999999999999999\n"),
            UiText::new(UiTextCode::CsvAmountOverflow)
        );
    }

    #[test]
    fn a_problem_that_is_not_about_a_cell_reads_as_an_unreadable_row() {
        assert_eq!(
            row_problem(2, &CsvError::MissingHeader),
            UiText::new(UiTextCode::CsvUnreadableRow)
        );
    }

    #[test]
    fn an_empty_file_and_a_file_without_a_header_are_file_errors() {
        let err = parse_bank_csv("", eur(), None).expect_err("empty");
        assert_eq!(err, Error::Csv(CsvError::Empty));
        let err = parse_bank_csv(" \n\t\n", eur(), None).expect_err("blank");
        assert_eq!(err, Error::Csv(CsvError::Empty));
        let err = parse_bank_csv(",,\n1,2,3\n", eur(), None).expect_err("no header name");
        assert_eq!(err, Error::Csv(CsvError::MissingHeader));
    }

    #[test]
    fn a_file_without_a_required_column_names_it_and_reads_no_row() {
        use CsvRequiredColumn::{Amount, Date};

        for (csv, missing) in [
            ("When,Memo,Amount\n2026-03-15,Rent,-8.00\n", vec![Date]),
            ("Date,Memo,Paid\n2026-03-15,Rent,-8.00\n", vec![Amount]),
            (
                "When,Memo,Paid\n2026-03-15,Rent,-8.00\n",
                vec![Date, Amount],
            ),
        ] {
            let parsed = parse_bank_csv(csv, eur(), None).expect("not a file error");

            assert_eq!(parsed.missing_columns, missing, "{csv}");
            assert_eq!(parsed.rows, [], "{csv}");
            assert_eq!(parsed.headers.len(), 3, "{csv}");
            assert_eq!(
                parsed.detected_mapping.description.as_deref(),
                Some("Memo"),
                "{csv}"
            );
        }
    }

    #[test]
    fn a_debit_or_a_credit_column_alone_is_an_amount_column() {
        for csv in [
            "Date,Memo,Debit\n2026-03-15,Rent,8.00\n",
            "Date,Memo,Credit\n2026-03-15,Pay,8.00\n",
        ] {
            let parsed = parse_bank_csv(csv, eur(), None).expect("parse");

            assert_eq!(parsed.missing_columns, [], "{csv}");
            assert_eq!(parsed.rows.len(), 1, "{csv}");
        }
    }

    #[test]
    fn an_explicit_mapping_reads_a_file_detection_could_not() {
        let csv = "When,Memo,Paid\n2026-03-15,Rent,-8.00\n";
        let mapping = column_mapping("When", "Memo", Some("Paid"), None, None);
        let mapped = parse_bank_csv(csv, eur(), Some(&mapping)).expect("mapped");

        assert_eq!(mapped.missing_columns, []);
        assert_eq!(kinds_of(&mapped), [SimpleEntryKind::Expense]);
        assert_eq!(
            mapped.detected_mapping.date, None,
            "still what was detected"
        );
    }

    #[test]
    fn quoted_semicolon_inside_field() {
        let csv = "Date;Description;Amount\n2026-08-01;\"a;b;c\";10.00\n";
        let row = first_parsed_row(csv);
        assert_eq!(row.description, "a;b;c");
        assert_eq!(row.kind, SimpleEntryKind::Income);
    }

    #[test]
    fn iso_and_dmy_dates() {
        let fifth_of_march = time::macros::date!(2026 - 03 - 05);
        assert_eq!(parse_csv_date("2026-03-05"), Ok(fifth_of_march));
        assert_eq!(parse_csv_date("5/3/2026"), Ok(fifth_of_march));
        assert_eq!(parse_csv_date("2026/03/05"), Ok(fifth_of_march));
        assert!(parse_csv_date("03/13/2026").is_err());
        assert!(parse_csv_date("32/01/2026").is_err());
    }

    #[test]
    fn a_day_first_date_is_read_with_any_of_the_three_separators() {
        let fifth_of_march = time::macros::date!(2026 - 03 - 05);
        for cell in [
            "05.03.2026",
            "5.3.2026",
            "05-03-2026",
            "5-3-2026",
            "05/03/2026",
        ] {
            assert_eq!(parse_csv_date(cell), Ok(fifth_of_march), "{cell}");
        }
        // Day first with every separator, never month first.
        for cell in ["03.04.2026", "03-04-2026", "03/04/2026"] {
            assert_eq!(
                parse_csv_date(cell),
                Ok(time::macros::date!(2026 - 04 - 03)),
                "{cell}"
            );
        }
        for cell in ["03.13.2026", "03-13-2026"] {
            assert_eq!(
                parse_csv_date(cell),
                Err(CsvError::InvalidDate(cell.to_owned())),
                "{cell}"
            );
        }
    }

    #[test]
    fn a_four_digit_first_segment_is_a_year_with_any_separator() {
        let fifth_of_march = time::macros::date!(2026 - 03 - 05);
        for cell in ["2026-03-05", "2026/03/05", "2026.03.05", "2026-3-5"] {
            assert_eq!(parse_csv_date(cell), Ok(fifth_of_march), "{cell}");
        }
        // Never day first, even where that would be a date: 20 December.
        assert_eq!(
            parse_csv_date("2012-12-2020"),
            Err(CsvError::InvalidDate("2012-12-2020".to_owned()))
        );
    }

    #[test]
    fn date_forms_outside_the_documented_ones_are_rejected() {
        for cell in [
            "05.03-2026",
            "05/03.2026",
            "2026-03/05",
            "05 03 2026",
            "05032026",
            "05.03.2026.",
            "5.3",
            "05/03/26",
            "5 Mar 2026",
            "2026-03-05 10:00",
            "2026-02-30",
            "31.04.2026",
            "29-02-2026",
        ] {
            assert_eq!(
                parse_csv_date(cell),
                Err(CsvError::InvalidDate(cell.to_owned())),
                "{cell}"
            );
        }
    }

    #[test]
    fn a_signed_date_segment_is_rejected() {
        for cell in [
            "+5/+3/2026",
            "-123/01/02",
            "2026-+3-05",
            "5/3/+026",
            "-5-3-2026",
            "5.-3.2026",
            "+5.3.2026",
            "5.3.-026",
            "٥.٣.٢٠٢٦",
        ] {
            assert_eq!(
                parse_csv_date(cell),
                Err(CsvError::InvalidDate(cell.to_owned())),
                "{cell}"
            );
        }
    }

    #[test]
    fn a_slash_date_with_the_year_last_is_read_day_first() {
        assert_eq!(
            parse_csv_date("03/04/2026"),
            Ok(time::macros::date!(2026 - 04 - 03))
        );
    }

    #[test]
    fn headers_are_classified_in_the_documented_order() {
        let csv = "Value Date,Booking-date,Transaction_Amount,Name,Memo\n\
            2026-03-05,2026-03-06,1.00,a,b\n";
        let detected = parse_bank_csv(csv, eur(), None).unwrap().detected_mapping;

        assert_eq!(detected.date.as_deref(), Some("Value Date"));
        assert_eq!(detected.amount.as_deref(), Some("Transaction_Amount"));
        assert_eq!(detected.description.as_deref(), Some("Name"));
    }

    fn column_mapping(
        date: &str,
        description: &str,
        amount: Option<&str>,
        debit: Option<&str>,
        credit: Option<&str>,
    ) -> CsvColumnMapping {
        CsvColumnMapping {
            date: Some(date.into()),
            description: Some(description.into()),
            amount: amount.map(str::to_owned),
            debit: debit.map(str::to_owned),
            credit: credit.map(str::to_owned),
            reference: None,
            direction: None,
        }
    }

    #[test]
    fn greek_headers_are_detected_with_or_without_accents() {
        let csv = "Ημερομηνία;Περιγραφή;Ποσό\n15/03/2026;Καφές;-3,50\n";
        let upper = "ΗΜΕΡΟΜΗΝΙΑ;ΑΙΤΙΟΛΟΓΙΑ;ΠΟΣΟ\n15/03/2026;Καφές;-3,50\n";

        for text in [csv, upper] {
            let parsed = parse_bank_csv(text, eur(), None).unwrap();

            assert_eq!(parsed.missing_columns, vec![]);
            let CsvRowOutcome::Parsed(row) = &parsed.rows[0] else {
                panic!("row");
            };
            assert_eq!(row.signed_amount_minor, -350);
            assert_eq!(row.description, "Καφές");
        }
    }

    #[test]
    fn greek_debit_and_credit_columns_are_detected() {
        let csv = "Ημερομηνία συναλλαγής;Περιγραφή;Χρέωση;Πίστωση;Αριθμός αναφοράς\n\
            15/03/2026;Ενοίκιο;800,00;;A1\n";
        let detected = parse_bank_csv(csv, eur(), None).unwrap().detected_mapping;

        assert_eq!(detected.debit.as_deref(), Some("Χρέωση"));
        assert_eq!(detected.credit.as_deref(), Some("Πίστωση"));
        assert_eq!(detected.reference.as_deref(), Some("Αριθμός αναφοράς"));
    }

    #[test]
    fn german_and_french_headers_are_detected() {
        let german = "Buchungstag;Verwendungszweck;Soll;Haben;Referenz\n\
            15.03.2026;Miete;800,00;;R1\n";
        let french = "Date;Libellé;Débit;Crédit;Référence\n15/03/2026;Loyer;800,00;;R1\n";
        let datum = "Datum;Beschreibung;Betrag\n15.03.2026;Miete;-800,00\n";

        let german = parse_bank_csv(german, eur(), None)
            .unwrap()
            .detected_mapping;
        let french = parse_bank_csv(french, eur(), None)
            .unwrap()
            .detected_mapping;
        let datum = parse_bank_csv(datum, eur(), None).unwrap().detected_mapping;

        assert_eq!(german.date.as_deref(), Some("Buchungstag"));
        assert_eq!(german.description.as_deref(), Some("Verwendungszweck"));
        assert_eq!(german.debit.as_deref(), Some("Soll"));
        assert_eq!(german.credit.as_deref(), Some("Haben"));
        assert_eq!(german.reference.as_deref(), Some("Referenz"));
        assert_eq!(french.description.as_deref(), Some("Libellé"));
        assert_eq!(french.debit.as_deref(), Some("Débit"));
        assert_eq!(french.credit.as_deref(), Some("Crédit"));
        assert_eq!(french.reference.as_deref(), Some("Référence"));
        assert_eq!(datum.date.as_deref(), Some("Datum"));
        assert_eq!(datum.amount.as_deref(), Some("Betrag"));
    }

    #[test]
    fn omitted_mapping_auto_detects_headers() {
        let parsed = parse_bank_csv(
            "Date,Description,Amount\n2026-03-15,Coffee,-3.50\n",
            eur(),
            None,
        )
        .expect("auto");
        assert_eq!(
            parsed.headers,
            vec![
                "Date".to_owned(),
                "Description".to_owned(),
                "Amount".to_owned()
            ]
        );
        assert_eq!(parsed.detected_mapping.date.as_deref(), Some("Date"));
        assert_eq!(
            parsed.detected_mapping.description.as_deref(),
            Some("Description")
        );
        assert_eq!(parsed.detected_mapping.amount.as_deref(), Some("Amount"));
        let CsvRowOutcome::Parsed(row) = &parsed.rows[0] else {
            panic!("row");
        };
        assert_eq!(row.description, "Coffee");
    }

    #[test]
    fn mapping_override_uses_mapped_columns() {
        let csv = "Date,Payee,Notes,Amount\n2026-03-15,Coffee,ignored notes,-3.50\n";
        let auto = parse_bank_csv(csv, eur(), None).expect("auto");
        let CsvRowOutcome::Parsed(row) = &auto.rows[0] else {
            panic!("auto row");
        };
        assert_eq!(row.description, "Coffee");

        let mapping = column_mapping("Date", "Notes", Some("Amount"), None, None);
        let mapped = parse_bank_csv(csv, eur(), Some(&mapping)).expect("mapped");
        assert_eq!(
            mapped.detected_mapping.description.as_deref(),
            Some("Payee")
        );
        let CsvRowOutcome::Parsed(row) = &mapped.rows[0] else {
            panic!("mapped row");
        };
        assert_eq!(row.description, "ignored notes");
        assert_eq!(row.signed_amount_minor, -350);
    }

    #[test]
    fn mapping_override_debit_credit() {
        let csv = "When,What,Out,In\n01/04/2026,Rent,800.00,\n";
        let mapping = column_mapping("When", "What", None, Some("Out"), Some("In"));
        let parsed = parse_bank_csv(csv, eur(), Some(&mapping)).expect("dc");
        let CsvRowOutcome::Parsed(row) = &parsed.rows[0] else {
            panic!("row");
        };
        assert_eq!(row.kind, SimpleEntryKind::Expense);
        assert_eq!(row.amount_minor, 80_000);
        assert!(parsed.detected_mapping.date.is_none());
    }

    const UNSIGNED_WITH_TYPE: &str = "Date,Payee,Notes,Amount,Type\n\
        2026-03-15,Rent,flat,800.00,Debit\n\
        2026-03-16,Salary,march,2500.00,Credit\n";

    fn kinds_of(parsed: &ParsedBankCsv) -> Vec<SimpleEntryKind> {
        parsed
            .rows
            .iter()
            .map(|row| match row {
                CsvRowOutcome::Parsed(row) => row.kind,
                CsvRowOutcome::Invalid { .. } => panic!("expected parsed row, got {row:?}"),
            })
            .collect()
    }

    #[test]
    fn editing_a_mapping_keeps_the_detected_direction_column() {
        let auto = parse_bank_csv(UNSIGNED_WITH_TYPE, eur(), None).expect("auto");
        let expected = vec![SimpleEntryKind::Expense, SimpleEntryKind::Income];
        assert_eq!(kinds_of(&auto), expected);

        // What the Map columns step sends back after one edit.
        let edited = CsvColumnMapping {
            description: Some("Notes".into()),
            ..auto.detected_mapping
        };
        let mapped = parse_bank_csv(UNSIGNED_WITH_TYPE, eur(), Some(&edited)).expect("mapped");

        assert_eq!(kinds_of(&mapped), expected);
    }

    #[test]
    fn detected_mapping_names_the_direction_column() {
        let auto = parse_bank_csv(UNSIGNED_WITH_TYPE, eur(), None).expect("auto");
        assert_eq!(auto.detected_mapping.direction.as_deref(), Some("Type"));

        let unsigned = parse_bank_csv("Date,Payee,Amount\n2026-03-15,Rent,8.00\n", eur(), None)
            .expect("no direction column");
        assert_eq!(unsigned.detected_mapping.direction, None);
    }

    #[test]
    fn a_type_column_of_transaction_kinds_is_not_the_direction_column() {
        let csv = "Date,Payee,Amount,Type\n\
            2026-03-15,Shop,-8.00,POS\n\
            2026-03-16,Salary,2500.00,TRANSFER\n";
        let parsed = parse_bank_csv(csv, eur(), None).expect("parse");

        assert_eq!(parsed.detected_mapping.direction, None);
        assert_eq!(
            kinds_of(&parsed),
            [SimpleEntryKind::Expense, SimpleEntryKind::Income],
            "the amounts keep their own signs"
        );
    }

    #[test]
    fn one_word_that_is_no_direction_keeps_a_column_from_being_the_direction() {
        let csv = "Date,Payee,Amount,Type\n\
            2026-03-15,Rent,800.00,Debit\n\
            2026-03-16,Shop,8.00,POS\n";
        let parsed = parse_bank_csv(csv, eur(), None).expect("parse");

        assert_eq!(parsed.detected_mapping.direction, None);
        assert_eq!(
            kinds_of(&parsed),
            [SimpleEntryKind::Income, SimpleEntryKind::Income],
            "the documented cost: the unsigned debit keeps the sign it was written with"
        );
    }

    #[test]
    fn every_documented_direction_word_is_read_in_any_case_and_with_padding() {
        for word in ["d", "dr", "debit", "withdrawal", "expense", "out"] {
            assert_eq!(Direction::of_word(word), Some(Direction::Out), "{word}");
            let padded = format!("  {}\t", word.to_uppercase());
            assert_eq!(
                Direction::of_word(&padded),
                Some(Direction::Out),
                "{padded}"
            );
        }
        for word in ["c", "cr", "credit", "deposit", "income", "in"] {
            assert_eq!(Direction::of_word(word), Some(Direction::In), "{word}");
            let padded = format!(" {} ", word.to_uppercase());
            assert_eq!(Direction::of_word(&padded), Some(Direction::In), "{padded}");
        }
        for other in ["", "  ", "pos", "transfer", "debits", "in out", "+"] {
            assert_eq!(Direction::of_word(other), None, "{other:?}");
        }
    }

    #[test]
    fn a_direction_gives_its_sign_to_an_amount_of_either_sign() {
        assert_eq!(apply_direction(500, "out"), Ok(-500));
        assert_eq!(apply_direction(-500, "Withdrawal"), Ok(-500));
        assert_eq!(apply_direction(-500, "deposit"), Ok(500));
        assert_eq!(apply_direction(500, "IN"), Ok(500));
        assert_eq!(apply_direction(-500, " "), Ok(-500), "a blank cell");
        assert_eq!(
            apply_direction(500, " sideways "),
            Err(CsvError::InvalidType("sideways".into()))
        );
    }

    #[test]
    fn an_amount_without_a_magnitude_overflows_under_either_direction() {
        for word in ["d", "c"] {
            assert_eq!(
                apply_direction(i64::MIN, word),
                Err(CsvError::AmountOverflow),
                "{word}"
            );
        }
    }

    #[test]
    fn a_row_too_short_to_have_a_direction_cell_does_not_cost_the_column() {
        let csv = "Date,Payee,Amount,Type\n\
            2026-03-15,Rent,800.00,Debit\n\
            2026-03-16,Fee,-2.00\n";
        let parsed = parse_bank_csv(csv, eur(), None).expect("parse");

        assert_eq!(parsed.detected_mapping.direction.as_deref(), Some("Type"));
        assert_eq!(
            kinds_of(&parsed),
            [SimpleEntryKind::Expense, SimpleEntryKind::Expense]
        );
    }

    #[test]
    fn a_direction_column_with_empty_cells_or_none_at_all_is_still_detected() {
        let with_gaps = "Date,Payee,Amount,D/C\n\
            2026-03-15,Rent,800.00,dr\n\
            2026-03-16,Fee,-2.00,\n\
            2026-03-17,Salary,2500.00,CR\n";
        let parsed = parse_bank_csv(with_gaps, eur(), None).expect("parse");
        assert_eq!(parsed.detected_mapping.direction.as_deref(), Some("D/C"));
        assert_eq!(
            kinds_of(&parsed),
            [
                SimpleEntryKind::Expense,
                SimpleEntryKind::Expense,
                SimpleEntryKind::Income
            ]
        );

        let header_only = parse_bank_csv("Date,Payee,Amount,Type\n", eur(), None).expect("parse");
        assert_eq!(
            header_only.detected_mapping.direction.as_deref(),
            Some("Type")
        );
    }

    #[test]
    fn the_next_direction_header_is_taken_when_the_first_holds_other_words() {
        let csv = "Date,Payee,Amount,Type,D/C\n\
            2026-03-15,Rent,800.00,POS,D\n\
            2026-03-16,Salary,2500.00,TRANSFER,C\n";
        let parsed = parse_bank_csv(csv, eur(), None).expect("parse");

        assert_eq!(parsed.detected_mapping.direction.as_deref(), Some("D/C"));
        assert_eq!(
            kinds_of(&parsed),
            [SimpleEntryKind::Expense, SimpleEntryKind::Income]
        );
    }

    /// A statement of `rows` debit rows whose `Type` cell is `odd_word` in
    /// the 1-based data row `odd_row`.
    fn statement_with_one_odd_type(rows: usize, odd_row: usize, odd_word: &str) -> String {
        let mut csv = String::from("Date,Payee,Amount,Type\n");
        for row in 1..=rows {
            let word = if row == odd_row { odd_word } else { "Debit" };
            csv.push_str("2026-03-15,Rent,8.00,");
            csv.push_str(word);
            csv.push('\n');
        }
        csv
    }

    #[test]
    fn the_direction_column_is_detected_from_the_sampled_rows_only() {
        let rows = DIRECTION_SAMPLE_ROWS + 1;

        let inside = statement_with_one_odd_type(rows, DIRECTION_SAMPLE_ROWS, "POS");
        let parsed = parse_bank_csv(&inside, eur(), None).expect("parse");
        assert_eq!(
            parsed.detected_mapping.direction, None,
            "the last sampled row"
        );

        let outside = statement_with_one_odd_type(rows, rows, "POS");
        let parsed = parse_bank_csv(&outside, eur(), None).expect("parse");
        assert_eq!(parsed.detected_mapping.direction.as_deref(), Some("Type"));
        assert_eq!(
            parsed.rows.last(),
            Some(&CsvRowOutcome::Invalid {
                source_row: u32::try_from(rows + 1).unwrap(),
                reason: UiText::new(UiTextCode::CsvInvalidType).with_param("value", "POS"),
            }),
            "a word past the sample costs its own row only"
        );
    }

    #[test]
    fn an_explicit_direction_column_is_read_whatever_it_holds() {
        let csv = "Date,Payee,Amount,Type\n2026-03-15,Shop,8.00,POS\n";
        let mapping = CsvColumnMapping {
            direction: Some("Type".into()),
            ..column_mapping("Date", "Payee", Some("Amount"), None, None)
        };
        let mapped = parse_bank_csv(csv, eur(), Some(&mapping)).expect("mapped");

        assert_eq!(
            mapped.rows,
            [CsvRowOutcome::Invalid {
                source_row: 2,
                reason: UiText::new(UiTextCode::CsvInvalidType).with_param("value", "POS"),
            }]
        );
    }

    #[test]
    fn a_mapping_without_a_direction_reads_the_amount_sign() {
        let mapping = column_mapping("Date", "Payee", Some("Amount"), None, None);
        let mapped = parse_bank_csv(UNSIGNED_WITH_TYPE, eur(), Some(&mapping)).expect("mapped");

        assert_eq!(
            kinds_of(&mapped),
            vec![SimpleEntryKind::Income, SimpleEntryKind::Income]
        );
    }

    #[test]
    fn a_direction_column_that_is_not_in_the_file_is_rejected() {
        let mapping = CsvColumnMapping {
            direction: Some("Nope".into()),
            ..column_mapping("Date", "Payee", Some("Amount"), None, None)
        };
        let err = parse_bank_csv(UNSIGNED_WITH_TYPE, eur(), Some(&mapping)).expect_err("unknown");
        assert_eq!(err, invalid_mapping(unknown_column("Nope")));
    }

    #[test]
    fn a_mapping_without_the_direction_key_still_deserializes() {
        let mapping: CsvColumnMapping =
            serde_json::from_str(r#"{"date":"Date","description":"Payee","amount":"Amount"}"#)
                .expect("payload from a UI that predates the field");
        assert_eq!(mapping.direction, None);
    }

    /// One case of `web/src/lib/csvMappingVerdicts.json`.
    #[derive(Debug, serde::Deserialize)]
    struct MappingVerdict {
        /// The mapping, as the Map columns step sends it.
        mapping: CsvColumnMapping,
        /// `ready`, or the identifier of the problem the mapping is refused
        /// for.
        verdict: String,
    }

    /// The rule for a complete mapping is written twice: here, and in the
    /// web's `mappingReady`, which enables Continue in the Map columns step.
    /// Both read the same cases, so a mapping the step lets through is one
    /// core accepts, and the other way round.
    #[test]
    fn the_verdicts_fixture_gives_what_core_says_of_each_mapping() {
        let cases: Vec<MappingVerdict> = serde_json::from_str(include_str!(
            "../../../../web/src/lib/csvMappingVerdicts.json"
        ))
        .expect("csvMappingVerdicts.json parses");
        let headers = StringRecord::from(vec![
            "Date", "Memo", "Amount", "Debit", "Credit", "Ref", "Type",
        ]);

        assert!(!cases.is_empty());
        for case in cases {
            let verdict = match resolve_user_mapping(&headers, &case.mapping) {
                Ok(_) => "ready",
                Err(Error::Csv(CsvError::InvalidMapping(problem))) => problem.identifier(),
                Err(other) => panic!("{:?} gave {other:?}", case.mapping),
            };

            assert_eq!(verdict, case.verdict, "{:?}", case.mapping);
        }
    }

    #[test]
    fn a_mapping_with_only_a_debit_or_only_a_credit_column_reads_the_rows() {
        let csv = "When,What,Out,In\n01/04/2026,Rent,800.00,\n02/04/2026,Pay,,2500.00\n";

        let debit_only = column_mapping("When", "What", None, Some("Out"), None);
        let parsed = parse_bank_csv(csv, eur(), Some(&debit_only)).expect("debit only");
        assert_eq!(
            parsed.rows[0],
            parse_bank_csv(
                csv,
                eur(),
                Some(&column_mapping(
                    "When",
                    "What",
                    None,
                    Some("Out"),
                    Some("In")
                ))
            )
            .expect("both")
            .rows[0]
        );
        assert_eq!(
            parsed.rows[1],
            CsvRowOutcome::Invalid {
                source_row: 3,
                reason: UiText::new(UiTextCode::CsvMissingAmount),
            },
            "the credit column is not read"
        );

        let credit_only = column_mapping("When", "What", None, None, Some("In"));
        let parsed = parse_bank_csv(csv, eur(), Some(&credit_only)).expect("credit only");
        let CsvRowOutcome::Parsed(pay) = &parsed.rows[1] else {
            panic!("pay");
        };
        assert_eq!(pay.signed_amount_minor, 250_000);
    }

    #[test]
    fn a_mapping_without_a_description_reads_rows_with_an_empty_one() {
        let csv = "Date,Payee,Amount\n2026-03-15,Coffee,-3.50\n";

        for description in [None, Some(String::new()), Some("  ".to_owned())] {
            let mapping = CsvColumnMapping {
                date: Some("Date".into()),
                description: description.clone(),
                amount: Some("Amount".into()),
                ..CsvColumnMapping::default()
            };
            let parsed = parse_bank_csv(csv, eur(), Some(&mapping)).expect("no description");

            let CsvRowOutcome::Parsed(row) = &parsed.rows[0] else {
                panic!("{description:?}: {:?}", parsed.rows);
            };
            assert_eq!(row.description, "", "{description:?}");
            assert_eq!(row.signed_amount_minor, -350, "{description:?}");
        }
    }

    #[test]
    fn a_file_without_a_description_column_reads_the_same_detected_or_mapped() {
        let csv = "Date,Amount\n2026-03-15,-3.50\n";
        let detected = parse_bank_csv(csv, eur(), None).expect("detected");
        assert_eq!(detected.missing_columns, []);
        assert_eq!(detected.detected_mapping.description, None);

        let mapped = parse_bank_csv(csv, eur(), Some(&detected.detected_mapping)).expect("mapped");

        assert_eq!(mapped.rows, detected.rows);
        assert_eq!(mapped.rows.len(), 1);
    }

    #[test]
    fn invalid_mapping_is_rejected() {
        let csv = "Date,Description,Amount,Debit,Credit\n2026-03-15,X,-1.00,,\n";

        let missing_date = CsvColumnMapping {
            description: Some("Description".into()),
            amount: Some("Amount".into()),
            ..CsvColumnMapping::default()
        };
        let err = parse_bank_csv(csv, eur(), Some(&missing_date)).expect_err("date");
        assert_eq!(err, invalid_mapping(CsvMappingProblem::MissingDate));

        let missing_amount = column_mapping("Date", "Description", None, None, None);
        let err = parse_bank_csv(csv, eur(), Some(&missing_amount)).expect_err("amount");
        assert_eq!(err, invalid_mapping(CsvMappingProblem::MissingAmount));

        let both = column_mapping(
            "Date",
            "Description",
            Some("Amount"),
            Some("Debit"),
            Some("Credit"),
        );
        let err = parse_bank_csv(csv, eur(), Some(&both)).expect_err("both");
        assert_eq!(
            err,
            invalid_mapping(CsvMappingProblem::AmountAndDebitOrCredit)
        );

        let unknown = column_mapping("Date", "Nope", Some("Amount"), None, None);
        let err = parse_bank_csv(csv, eur(), Some(&unknown)).expect_err("unknown");
        assert_eq!(err, invalid_mapping(unknown_column("Nope")));
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// Any date a cell can name: one with a four-digit year.
    fn dates() -> impl Strategy<Value = Date> {
        (0_i32..=9999, 1_u8..=12, 1_u8..=31).prop_filter_map(
            "the month has no such day",
            |(year, month, day)| {
                let month = Month::try_from(month).ok()?;
                Date::from_calendar_date(year, month, day).ok()
            },
        )
    }

    /// `date` in every form the date table accepts: year first and day
    /// first, with each separator, with and without leading zeros on the day
    /// and the month.
    fn accepted_forms(date: Date) -> Vec<String> {
        let (year, month, day) = (date.year(), u8::from(date.month()), date.day());

        DATE_SEPARATORS
            .into_iter()
            .flat_map(|s| {
                [
                    format!("{year:04}{s}{month:02}{s}{day:02}"),
                    format!("{year:04}{s}{month}{s}{day}"),
                    format!("{day:02}{s}{month:02}{s}{year:04}"),
                    format!("{day}{s}{month}{s}{year:04}"),
                ]
            })
            .collect()
    }

    /// A cell of a direction column: one of the twelve words in lower or
    /// upper case, or nothing.
    fn direction_cells() -> impl Strategy<Value = String> {
        let words = prop::sample::select(vec![
            "",
            "d",
            "dr",
            "debit",
            "withdrawal",
            "expense",
            "out",
            "c",
            "cr",
            "credit",
            "deposit",
            "income",
            "in",
        ]);
        (words, any::<bool>()).prop_map(|(word, upper)| {
            if upper {
                word.to_uppercase()
            } else {
                word.to_owned()
            }
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        // Detection and the reading of a cell share one list of words, so a
        // column of them is always detected and refuses no row, at any
        // length on either side of the sample.
        #[test]
        fn a_column_of_direction_words_and_blanks_is_detected_and_refuses_no_row(
            cells in prop::collection::vec(direction_cells(), 0..120),
        ) {
            let mut csv = String::from("Date,Payee,Amount,Type\n");
            for cell in &cells {
                csv.push_str("2026-03-15,Rent,8.00,");
                csv.push_str(cell);
                csv.push('\n');
            }
            let currency: CurrencyCode = "EUR".parse().expect("a currency code");
            let parsed = parse_bank_csv(&csv, currency, None).expect("a readable file");

            prop_assert_eq!(parsed.detected_mapping.direction.as_deref(), Some("Type"));
            prop_assert_eq!(parsed.rows.len(), cells.len());
            for row in &parsed.rows {
                prop_assert!(matches!(row, CsvRowOutcome::Parsed(_)), "{:?}", row);
            }
        }

        #[test]
        fn a_date_written_in_any_accepted_form_parses_back_to_itself(date in dates()) {
            for cell in accepted_forms(date) {
                prop_assert_eq!(parse_csv_date(&cell), Ok(date), "{}", cell);
            }
        }

        #[test]
        fn parsing_any_text_as_a_date_returns_instead_of_panicking(raw in any::<String>()) {
            let _ = parse_csv_date(&raw);
        }

        #[test]
        fn parsing_date_shaped_text_returns_instead_of_panicking(
            raw in "[0-9./ +-]{0,14}",
        ) {
            let _ = parse_csv_date(&raw);
        }

        // A date is three runs of digits, so whatever parses holds nothing
        // but ASCII digits and one kind of separator, twice.
        #[test]
        fn whatever_parses_as_a_date_is_digits_around_one_separator(
            raw in "[0-9./ +-]{0,14}",
        ) {
            if parse_csv_date(&raw).is_ok() {
                let cell = raw.trim();
                let separators: Vec<char> =
                    cell.chars().filter(|character| !character.is_ascii_digit()).collect();

                prop_assert_eq!(separators.len(), 2, "{}", cell);
                prop_assert_eq!(separators[0], separators[1], "{}", cell);
                prop_assert!(DATE_SEPARATORS.contains(&separators[0]), "{}", cell);
            }
        }
    }
}
