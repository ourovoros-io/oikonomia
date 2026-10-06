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
//! Comma or semicolon. The first non-empty line decides: semicolon when it
//! has more semicolons than commas outside double quotes, comma otherwise.
//! Tabs and other delimiters are not detected. Fields may be quoted as in
//! RFC 4180, cells are trimmed, and a row may have fewer cells than the
//! header; a missing cell reads as empty.
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
//! # Dates
//!
//! | Form | Example | |
//! |------|---------|-|
//! | `YYYY-MM-DD` | `2026-03-05` | Accepted. |
//! | `YYYY/MM/DD` | `2026/03/05` | Accepted. |
//! | `DD/MM/YYYY` | `05/03/2026`, `5/3/2026` | Accepted. |
//! | `MM/DD/YYYY` | `03/13/2026` | Not supported. |
//! | `DD-MM-YYYY`, `DD.MM.YYYY` | `05.03.2026` | Rejected. |
//! | Two-digit year, month name, time of day | `05/03/26`, `5 Mar 2026` | Rejected. |
//!
//! Day and month may be written without a leading zero; the year is the
//! segment with four characters. The result is always `YYYY-MM-DD`.
//!
//! A slash date with the year last is always read day first. A US date is
//! rejected only when that reading is impossible (`03/13/2026`, month 13).
//! `03/04/2026` cannot be told apart from the European form and is read as
//! 3 April; a US statement has to be converted before import.

use std::fs;
use std::path::Path;

use csv::{ReaderBuilder, StringRecord, Trim};
use time::{Date, Month};

use crate::csv::amount::parse_signed_minor;
use crate::csv::{
    CsvColumnMapping, CsvError, CsvMappingProblem, CsvRowOutcome, MAX_CSV_BYTES, ParsedBankRow,
};
use crate::error::{Error, IoContext};
use crate::ledger::SimpleEntryKind;
use crate::ui_text::{UiText, UiTextCode};
use crate::util::format_date;

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
    let metadata = fs::metadata(path).io("inspect csv file")?;
    if !metadata.is_file() {
        return Err(Error::io(
            "read csv file",
            format_args!("not a regular file: {}", path.display()),
        ));
    }
    if metadata.len() > MAX_CSV_BYTES {
        return Err(CsvError::TooLarge.into());
    }
    let bytes = fs::read(path).io("read csv file")?;
    let text = String::from_utf8(bytes).map_err(|_| CsvError::NotUtf8)?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// Parses a date cell written as `YYYY-MM-DD`, `YYYY/MM/DD` or `DD/MM/YYYY`
/// and returns it as `YYYY-MM-DD`.
///
/// Day and month may be unpadded. US `MM/DD/YYYY` is not supported:
/// `03/13/2026` is rejected (month 13), and `03/04/2026` is read as 3 April.
///
/// # Errors
///
/// [`CsvError::MissingDate`] when the cell is empty or only whitespace;
/// [`CsvError::InvalidDate`], carrying the trimmed cell, when it is not in
/// one of the three forms or is not a date of the calendar.
pub fn parse_csv_date(raw: &str) -> CsvResult<String> {
    let cell = raw.trim();
    if cell.is_empty() {
        return Err(CsvError::MissingDate);
    }

    if let Some([year, month, day]) = split_three(cell, '-')
        && year.len() == 4
    {
        return Ok(format_date(calendar_date(year, month, day, cell)?));
    }
    if let Some([year, month, day]) = split_three(cell, '/')
        && year.len() == 4
    {
        return Ok(format_date(calendar_date(year, month, day, cell)?));
    }
    if let Some([day, month, year]) = split_three(cell, '/')
        && year.len() == 4
    {
        return Ok(format_date(calendar_date(year, month, day, cell)?));
    }
    Err(CsvError::InvalidDate(cell.to_owned()))
}

/// Parses a bank CSV into per-row outcomes. Never writes to the ledger.
///
/// Accepts comma or semicolon delimiters and RFC 4180 quoted fields.
/// `exponent` is the minor-unit exponent of the book's currency (2 for EUR).
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
    exponent: u8,
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
            Ok(record) => rows.push(parse_record(source_row, &record, columns, exponent)),
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

/// Picks comma or semicolon from the first non-empty line of `text`.
///
/// Semicolon needs strictly more occurrences than comma outside double
/// quotes, so a file with neither, or a tie, is read as comma-separated.
fn detect_delimiter(text: &str) -> u8 {
    let Some(line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return b',';
    };
    let mut commas = 0u32;
    let mut semicolons = 0u32;
    let mut in_quotes = false;
    for character in line.chars() {
        if character == '"' {
            in_quotes = !in_quotes;
        } else if character == ',' && !in_quotes {
            commas = commas.saturating_add(1);
        } else if character == ';' && !in_quotes {
            semicolons = semicolons.saturating_add(1);
        }
    }
    if semicolons > commas { b';' } else { b',' }
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
    exponent: u8,
) -> CsvRowOutcome {
    match parse_record_inner(source_row, record, columns, exponent) {
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
    exponent: u8,
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

    let signed = signed_amount(record, columns, exponent)?;
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
/// otherwise. With debit and credit columns the sign written in a cell is
/// ignored: a debit is money out and a credit is money in, and a row with
/// both is their difference, credit minus debit.
///
/// `abs` cannot overflow in here: [`parse_signed_minor`] negates a magnitude
/// that fits an `i64`, so it never returns `i64::MIN`.
fn signed_amount(record: &StringRecord, columns: ColumnMap, exponent: u8) -> CsvResult<i64> {
    if let Some(amount_index) = columns.amount {
        let signed = parse_signed_minor(record_cell(record, amount_index), exponent)?;
        return match columns.direction {
            Some(direction_index) => apply_direction(signed, record_cell(record, direction_index)),
            None => Ok(signed),
        };
    }

    let debit = optional_signed(record, columns.debit, exponent)?;
    let credit = optional_signed(record, columns.credit, exponent)?;
    match (debit, credit) {
        (None, None) => Err(CsvError::MissingAmount),
        (Some(debit), None) => Ok(-debit.abs()),
        (None, Some(credit)) => Ok(credit.abs()),
        (Some(debit), Some(credit)) => debit
            .abs()
            .checked_neg()
            .and_then(|money_out| money_out.checked_add(credit.abs()))
            .ok_or(CsvError::AmountOverflow),
    }
}

/// Parses the amount in column `index`, or returns `None` when there is no
/// such column or its cell is blank.
fn optional_signed(
    record: &StringRecord,
    index: Option<usize>,
    exponent: u8,
) -> CsvResult<Option<i64>> {
    let Some(index) = index else {
        return Ok(None);
    };
    let raw = record_cell(record, index).trim();
    if raw.is_empty() {
        return Ok(None);
    }
    parse_signed_minor(raw, exponent).map(Some)
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

    /// The problem of a mapping that names a header the file does not have.
    fn unknown_column(name: &str) -> CsvMappingProblem {
        CsvMappingProblem::UnknownColumn {
            name: name.to_owned(),
        }
    }

    fn parse_rows(text: &str) -> Vec<CsvRowOutcome> {
        parse_bank_csv(text, 2, None).expect("parse csv").rows
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
        assert_eq!(row.entry_date, "2026-03-15");
        assert_eq!(row.description, "Coffee");
        assert_eq!(row.signed_amount_minor, -350);
        assert_eq!(row.kind, SimpleEntryKind::Expense);
        assert_eq!(row.source_row, 2);
    }

    #[test]
    fn semicolon_quotes_european_date_and_amount() {
        let csv = "Date;Description;Amount\n15/03/2026;\"Coffee, Inc\";-1.234,56\n";
        let row = first_parsed_row(csv);
        assert_eq!(row.entry_date, "2026-03-15");
        assert_eq!(row.description, "Coffee, Inc");
        assert_eq!(row.signed_amount_minor, -123_456);
        assert_eq!(row.kind, SimpleEntryKind::Expense);
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
        let err = parse_bank_csv("Name,Memo\nfoo,bar\n", 2, None).expect_err("headers");
        assert_eq!(err, Error::Csv(CsvError::MissingDateColumn));
        let err = parse_bank_csv("", 2, None).expect_err("empty");
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
        assert_eq!(parse_csv_date("2026-03-05").expect("iso"), "2026-03-05");
        assert_eq!(parse_csv_date("5/3/2026").expect("dmy"), "2026-03-05");
        assert_eq!(parse_csv_date("2026/03/05").expect("ymd"), "2026-03-05");
        assert!(parse_csv_date("03/13/2026").is_err());
        assert!(parse_csv_date("32/01/2026").is_err());
    }

    #[test]
    fn date_forms_outside_the_three_documented_ones_are_rejected() {
        for cell in [
            "05.03.2026",
            "05-03-2026",
            "05/03/26",
            "5 Mar 2026",
            "2026-03-05 10:00",
            "2026-02-30",
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
        for cell in ["+5/+3/2026", "-123/01/02", "2026-+3-05", "5/3/+026"] {
            assert_eq!(
                parse_csv_date(cell),
                Err(CsvError::InvalidDate(cell.to_owned())),
                "{cell}"
            );
        }
    }

    #[test]
    fn a_slash_date_with_the_year_last_is_read_day_first() {
        assert_eq!(parse_csv_date("03/04/2026").unwrap(), "2026-04-03");
    }

    #[test]
    fn headers_are_classified_in_the_documented_order() {
        let csv = "Value Date,Booking-date,Transaction_Amount,Name,Memo\n\
            2026-03-05,2026-03-06,1.00,a,b\n";
        let detected = parse_bank_csv(csv, 2, None).unwrap().detected_mapping;

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
            2,
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
        let auto = parse_bank_csv(csv, 2, None).expect("auto");
        let CsvRowOutcome::Parsed(row) = &auto.rows[0] else {
            panic!("auto row");
        };
        assert_eq!(row.description, "Coffee");

        let mapping = column_mapping("Date", "Notes", Some("Amount"), None, None);
        let mapped = parse_bank_csv(csv, 2, Some(&mapping)).expect("mapped");
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
        let parsed = parse_bank_csv(csv, 2, Some(&mapping)).expect("dc");
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
        let auto = parse_bank_csv(UNSIGNED_WITH_TYPE, 2, None).expect("auto");
        let expected = vec![SimpleEntryKind::Expense, SimpleEntryKind::Income];
        assert_eq!(kinds_of(&auto), expected);

        // What the Map columns step sends back after one edit.
        let edited = CsvColumnMapping {
            description: Some("Notes".into()),
            ..auto.detected_mapping
        };
        let mapped = parse_bank_csv(UNSIGNED_WITH_TYPE, 2, Some(&edited)).expect("mapped");

        assert_eq!(kinds_of(&mapped), expected);
    }

    #[test]
    fn detected_mapping_names_the_direction_column() {
        let auto = parse_bank_csv(UNSIGNED_WITH_TYPE, 2, None).expect("auto");
        assert_eq!(auto.detected_mapping.direction.as_deref(), Some("Type"));

        let unsigned = parse_bank_csv("Date,Payee,Amount\n2026-03-15,Rent,8.00\n", 2, None)
            .expect("no direction column");
        assert_eq!(unsigned.detected_mapping.direction, None);
    }

    #[test]
    fn a_mapping_without_a_direction_reads_the_amount_sign() {
        let mapping = column_mapping("Date", "Payee", Some("Amount"), None, None);
        let mapped = parse_bank_csv(UNSIGNED_WITH_TYPE, 2, Some(&mapping)).expect("mapped");

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
        let err = parse_bank_csv(UNSIGNED_WITH_TYPE, 2, Some(&mapping)).expect_err("unknown");
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
        let err = parse_bank_csv(csv, 2, Some(&missing_date)).expect_err("date");
        assert_eq!(err, invalid_mapping(CsvMappingProblem::MissingDate));

        let missing_desc = CsvColumnMapping {
            date: Some("Date".into()),
            amount: Some("Amount".into()),
            ..CsvColumnMapping::default()
        };
        let err = parse_bank_csv(csv, 2, Some(&missing_desc)).expect_err("desc");
        assert_eq!(err, invalid_mapping(CsvMappingProblem::MissingDescription));

        let missing_amount = column_mapping("Date", "Description", None, None, None);
        let err = parse_bank_csv(csv, 2, Some(&missing_amount)).expect_err("amount");
        assert_eq!(err, invalid_mapping(CsvMappingProblem::MissingAmount));

        let both = column_mapping(
            "Date",
            "Description",
            Some("Amount"),
            Some("Debit"),
            Some("Credit"),
        );
        let err = parse_bank_csv(csv, 2, Some(&both)).expect_err("both");
        assert_eq!(
            err,
            invalid_mapping(CsvMappingProblem::AmountAndDebitOrCredit)
        );

        let unknown = column_mapping("Date", "Nope", Some("Amount"), None, None);
        let err = parse_bank_csv(csv, 2, Some(&unknown)).expect_err("unknown");
        assert_eq!(err, invalid_mapping(unknown_column("Nope")));
    }
}
