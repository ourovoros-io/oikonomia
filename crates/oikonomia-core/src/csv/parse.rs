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
//! stripped of spaces, `_` and `-`, then tested in this order. The first
//! test that passes names the column:
//!
//! | Order | Column      | Header                                               |
//! |-------|-------------|------------------------------------------------------|
//! | 1     | Date        | Contains `date` (`Booking date`, `ValueDate`)        |
//! | 2     | Amount      | `amount`, `value`, `sum`, `transactionamount`,       |
//! |       |             | `betrag`, `montant`, `importo`                       |
//! | 3     | Debit       | `debit`, `withdrawal`, `outflow`, `addebito`         |
//! | 4     | Credit      | `credit`, `deposit`, `inflow`, `accredito`           |
//! | 5     | Direction   | `type`, `dc`, `d/c`, `debitcredit`, `drcr`,          |
//! |       |             | `transactiontype`                                    |
//! | 6     | Reference   | `reference`, `ref`, `check`, `cheque`, `checkno`,    |
//! |       |             | `chequeno`, `fitid`                                  |
//! | 7     | Description | `description`, `memo`, `narration`, `details`,       |
//! |       |             | `payee`, `particulars`, `narrative`, `libelle`,      |
//! |       |             | `libellé`, `beschreibung`, `descrizione`,            |
//! |       |             | `transaction`, `name`                                |
//!
//! Rows 2 to 7 match the whole header, not a part of it. When several
//! headers name the same column, the leftmost wins and the others are not
//! read.
//!
//! A file needs a date column and at least one of amount, debit and credit.
//! When an amount column is present the debit and credit columns are not
//! read, and a direction column is read only beside an amount column.
//!
//! An explicit mapping replaces all of this: its header names are matched
//! whole, without regard to ASCII case, and a column it leaves out is not
//! read.
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
    CsvColumnMapping, CsvError, CsvMappingProblem, CsvRowOutcome, MAX_CSV_BYTES, ParsedBankRow,
};
use crate::domain::CurrencyCode;
use crate::error::{Error, IoContext};
use crate::ledger::SimpleEntryKind;
use crate::ui_text::{UiText, UiTextCode};

/// Result of a step that can only fail for a reason about the CSV itself.
type CsvResult<T> = std::result::Result<T, CsvError>;

/// Parsed bank CSV plus header metadata for the Map columns UI.
#[derive(Debug, Clone)]
pub struct ParsedBankCsv {
    /// Trimmed header names, file order.
    pub headers: Vec<String>,
    /// Auto-detected mapping (aliases), even when the caller overrode columns.
    pub detected_mapping: CsvColumnMapping,
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
/// marked with the code of another currency is an invalid row.
/// `mapping` replaces header auto-detection when `Some`.
///
/// # Errors
///
/// [`Error::Csv`] for a problem with the file as a whole: the text is
/// empty, the header row cannot be read or has no name in it, no date
/// column or no amount, debit or credit column is detected, or `mapping`
/// is incomplete, contradictory or names a header the file does not have.
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

    let detected = auto_map_headers(&headers);
    let detected_mapping = mapping_from_headers(&headers, detected);
    let columns = match mapping {
        Some(user) => resolve_user_mapping(&headers, user)?,
        None => require_auto_map(detected)?,
    };

    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let source_row = u32::try_from(index + 2).unwrap_or(u32::MAX);
        match record {
            Ok(record) => rows.push(parse_record(source_row, &record, columns, currency)),
            Err(err) => {
                log::warn!("CSV record {source_row} could not be read: {err}");
                rows.push(CsvRowOutcome::Invalid {
                    source_row,
                    reason: UiText::new(UiTextCode::CsvUnreadableRow),
                });
            }
        }
    }
    Ok(ParsedBankCsv {
        headers: headers.iter().map(str::to_owned).collect(),
        detected_mapping,
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
    /// Booking date. Always `Some` once a map has passed
    /// [`require_auto_map`] or [`resolve_user_mapping`].
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
    // `-123/01/02` a year before the common era.
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

/// Detects the column of each role from the header names.
///
/// The leftmost header of a role wins; a later one of the same role is left
/// unread.
fn auto_map_headers(headers: &StringRecord) -> ColumnMap {
    let mut map = ColumnMap::default();
    for (index, name) in headers.iter().enumerate() {
        let Some(kind) = classify_header(name) else {
            continue;
        };
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

/// Checks that a detected map has the columns a row cannot do without.
///
/// # Errors
///
/// [`Error::Csv`] with [`CsvError::MissingDateColumn`] or, when none
/// of amount, debit and credit was found, [`CsvError::MissingAmountColumn`].
fn require_auto_map(map: ColumnMap) -> crate::error::Result<ColumnMap> {
    if map.date.is_none() {
        return Err(CsvError::MissingDateColumn.into());
    }
    if map.amount.is_none() && map.debit.is_none() && map.credit.is_none() {
        return Err(CsvError::MissingAmountColumn.into());
    }
    Ok(map)
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
/// has no date or no description, sets both an amount and a debit or credit
/// column, sets neither an amount nor both of debit and credit, or names a
/// header the file does not have.
fn resolve_user_mapping(
    headers: &StringRecord,
    mapping: &CsvColumnMapping,
) -> crate::error::Result<ColumnMap> {
    let Some(date) = trimmed_nonempty(mapping.date.as_deref()) else {
        return Err(CsvError::InvalidMapping(CsvMappingProblem::MissingDate).into());
    };
    let Some(description) = trimmed_nonempty(mapping.description.as_deref()) else {
        return Err(CsvError::InvalidMapping(CsvMappingProblem::MissingDescription).into());
    };
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
    if !has_amount && (debit.is_none() || credit.is_none()) {
        return Err(CsvError::InvalidMapping(CsvMappingProblem::MissingAmount).into());
    }

    let optional_index =
        |name: Option<&str>| name.map(|name| header_index(headers, name)).transpose();

    Ok(ColumnMap {
        date: Some(header_index(headers, date)?),
        description: Some(header_index(headers, description)?),
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
    let compact: String = raw
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '_' | '-'))
        .collect();

    if compact.contains("date") {
        return Some(Column::Date);
    }
    if matches!(
        compact.as_str(),
        "amount" | "value" | "sum" | "transactionamount" | "betrag" | "montant" | "importo"
    ) {
        return Some(Column::Amount);
    }
    if matches!(
        compact.as_str(),
        "debit" | "withdrawal" | "outflow" | "addebito"
    ) {
        return Some(Column::Debit);
    }
    if matches!(
        compact.as_str(),
        "credit" | "deposit" | "inflow" | "accredito"
    ) {
        return Some(Column::Credit);
    }
    if matches!(
        compact.as_str(),
        "type" | "dc" | "d/c" | "debitcredit" | "drcr" | "transactiontype"
    ) {
        return Some(Column::Direction);
    }
    if matches!(
        compact.as_str(),
        "reference" | "ref" | "check" | "cheque" | "checkno" | "chequeno" | "fitid"
    ) {
        return Some(Column::Reference);
    }
    if matches!(
        compact.as_str(),
        "description"
            | "memo"
            | "narration"
            | "details"
            | "payee"
            | "particulars"
            | "narrative"
            | "libelle"
            | "libellé"
            | "beschreibung"
            | "descrizione"
            | "transaction"
            | "name"
    ) {
        return Some(Column::Description);
    }
    None
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
        | CsvError::MissingDateColumn
        | CsvError::MissingAmountColumn
        | CsvError::MissingColumn { .. }
        | CsvError::InvalidStatus(_)
        | CsvError::InvalidInteger(_)
        | CsvError::InvalidMapping(_) => {
            log::warn!("CSV record {source_row} could not be read: {err}");
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

    let signed = signed_amount(record, columns, currency)?;
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
        amount_minor,
        kind,
    })
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
) -> CsvResult<i64> {
    if let Some(amount_index) = columns.amount {
        let signed = parse_book_amount(record_cell(record, amount_index), currency)?;
        return match columns.direction {
            Some(direction_index) => apply_direction(signed, record_cell(record, direction_index)),
            None => Ok(signed),
        };
    }

    let debit = optional_signed(record, columns.debit, currency)?;
    let credit = optional_signed(record, columns.credit, currency)?;
    if debit.is_none() && credit.is_none() {
        return Err(CsvError::MissingAmount);
    }
    credit
        .unwrap_or(0)
        .checked_sub(debit.unwrap_or(0))
        .ok_or(CsvError::AmountOverflow)
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

/// Gives `signed` the sign its direction cell names, whatever sign it had.
///
/// A blank cell leaves the amount as it is. The words are matched whole and
/// without regard to case: `d`, `dr`, `debit`, `withdrawal`, `expense` and
/// `out` mean money out; `c`, `cr`, `credit`, `deposit`, `income` and `in`
/// mean money in.
///
/// # Errors
///
/// [`CsvError::InvalidType`] carrying the trimmed cell for any other word.
fn apply_direction(signed: i64, raw: &str) -> CsvResult<i64> {
    let direction = raw.trim().to_lowercase();
    if direction.is_empty() {
        return Ok(signed);
    }
    if matches!(
        direction.as_str(),
        "d" | "dr" | "debit" | "withdrawal" | "expense" | "out"
    ) {
        return signed
            .checked_abs()
            .map(|magnitude| -magnitude)
            .ok_or(CsvError::AmountOverflow);
    }
    if matches!(
        direction.as_str(),
        "c" | "cr" | "credit" | "deposit" | "income" | "in"
    ) {
        return signed.checked_abs().ok_or(CsvError::AmountOverflow);
    }
    Err(CsvError::InvalidType(raw.trim().to_owned()))
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
    fn an_unknown_type_is_reported_with_the_cell() {
        assert_eq!(
            reason_of_only_row("Date,Description,Amount,Type\n2026-03-15,Y,5.00,sideways\n"),
            UiText::new(UiTextCode::CsvInvalidType).with_param("value", "sideways")
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
    fn missing_columns_are_file_errors() {
        let err = parse_bank_csv("Name,Memo\nfoo,bar\n", eur(), None).expect_err("headers");
        assert_eq!(err, Error::Csv(CsvError::MissingDateColumn));
        let err = parse_bank_csv("", eur(), None).expect_err("empty");
        assert_eq!(err, Error::Csv(CsvError::Empty));
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

        let missing_desc = CsvColumnMapping {
            date: Some("Date".into()),
            amount: Some("Amount".into()),
            ..CsvColumnMapping::default()
        };
        let err = parse_bank_csv(csv, eur(), Some(&missing_desc)).expect_err("desc");
        assert_eq!(err, invalid_mapping(CsvMappingProblem::MissingDescription));

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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

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
