//! Bank CSV reader: delimiter detection, headers, quoted fields, per-row errors.

use std::fs;
use std::path::Path;

use csv::{ReaderBuilder, StringRecord, Trim};

use super::amount::parse_signed_minor;
use super::{CsvColumnMapping, CsvError, CsvRowOutcome, MAX_CSV_BYTES, ParsedBankRow};
use crate::error::Error;
use crate::ledger::SimpleEntryKind;
use crate::ui_text::{UiText, UiTextCode};
use crate::util::format_date;
use time::{Date, Month};

type CsvResult<T> = std::result::Result<T, CsvError>;

/// Read a UTF-8 CSV file, rejecting oversized or non-file paths.
///
/// Strips a leading UTF-8 BOM. Does not post or inspect the ledger.
///
/// # Errors
///
/// [`Error::Io`] on filesystem failures; [`Error::CsvParse`] when the file is
/// too large or not UTF-8.
pub fn read_csv_text(path: &Path) -> crate::error::Result<String> {
    let meta = fs::metadata(path).map_err(|err| Error::Io(err.to_string()))?;
    if !meta.is_file() {
        return Err(Error::Io(format!("not a file: {}", path.display())));
    }
    if meta.len() > MAX_CSV_BYTES {
        return Err(CsvError::TooLarge.into());
    }
    let bytes = fs::read(path).map_err(|err| Error::Io(err.to_string()))?;
    let text = String::from_utf8(bytes).map_err(|_| CsvError::NotUtf8)?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// Parse `YYYY-MM-DD` or `DD/MM/YYYY` (day and month may be unpadded).
///
/// A four-digit first segment with `/` is treated as `YYYY/MM/DD`. US
/// `MM/DD/YYYY` is not supported — `03/13/2026` is rejected (month 13).
///
/// # Errors
///
/// [`CsvError::MissingDate`] when the cell is empty or only whitespace;
/// [`CsvError::InvalidDate`] when it is not a valid calendar date.
pub fn parse_csv_date(raw: &str) -> CsvResult<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(CsvError::MissingDate);
    }

    if let Some(parts) = split_three(s, '-')
        && parts[0].len() == 4
    {
        return Ok(format_date(calendar_date(parts[0], parts[1], parts[2], s)?));
    }
    if let Some(parts) = split_three(s, '/')
        && parts[0].len() == 4
    {
        return Ok(format_date(calendar_date(parts[0], parts[1], parts[2], s)?));
    }
    if let Some(parts) = split_three(s, '/')
        && parts[2].len() == 4
    {
        return Ok(format_date(calendar_date(parts[2], parts[1], parts[0], s)?));
    }
    Err(CsvError::InvalidDate(s.to_owned()))
}

fn split_three(s: &str, sep: char) -> Option<[&str; 3]> {
    let mut parts = s.split(sep);
    let a = parts.next()?;
    let b = parts.next()?;
    let c = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some([a, b, c])
}

fn calendar_date(year: &str, month: &str, day: &str, raw: &str) -> CsvResult<Date> {
    let year: i32 = year
        .parse()
        .map_err(|_| CsvError::InvalidDate(raw.to_owned()))?;
    let month_num: u8 = month
        .parse()
        .map_err(|_| CsvError::InvalidDate(raw.to_owned()))?;
    let day_num: u8 = day
        .parse()
        .map_err(|_| CsvError::InvalidDate(raw.to_owned()))?;
    let month = Month::try_from(month_num).map_err(|_| CsvError::InvalidDate(raw.to_owned()))?;
    Date::from_calendar_date(year, month, day_num)
        .map_err(|_| CsvError::InvalidDate(raw.to_owned()))
}

#[derive(Debug, Clone, Copy)]
enum Column {
    Date,
    Description,
    Amount,
    Debit,
    Credit,
    Reference,
    Direction,
}

#[derive(Debug, Clone, Copy, Default)]
struct ColumnMap {
    date: Option<usize>,
    description: Option<usize>,
    amount: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
    reference: Option<usize>,
    direction: Option<usize>,
}

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

/// Parse a bank CSV into per-row outcomes. Never writes to the ledger.
///
/// Accepts comma or semicolon delimiters and RFC 4180 quoted fields.
/// `exponent` is the entity currency's minor-unit exponent (2 for EUR).
/// `mapping` overrides header auto-detect when `Some`.
///
/// # Errors
///
/// File-level problems (empty, missing date/amount headers, invalid mapping).
/// Malformed **rows** are returned as [`CsvRowOutcome::Invalid`], not as `Err`.
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
        .map_err(|err| Error::CsvParse(err.to_string()))?
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

fn detect_delimiter(text: &str) -> u8 {
    let Some(line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return b',';
    };
    let mut comma = 0u32;
    let mut semi = 0u32;
    let mut in_quotes = false;
    for c in line.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ',' && !in_quotes {
            comma = comma.saturating_add(1);
        } else if c == ';' && !in_quotes {
            semi = semi.saturating_add(1);
        }
    }
    if semi > comma { b';' } else { b',' }
}

fn auto_map_headers(headers: &StringRecord) -> ColumnMap {
    let mut map = ColumnMap::default();
    for (idx, name) in headers.iter().enumerate() {
        let Some(kind) = classify_header(name) else {
            continue;
        };
        match kind {
            Column::Date if map.date.is_none() => map.date = Some(idx),
            Column::Description if map.description.is_none() => map.description = Some(idx),
            Column::Amount if map.amount.is_none() => map.amount = Some(idx),
            Column::Debit if map.debit.is_none() => map.debit = Some(idx),
            Column::Credit if map.credit.is_none() => map.credit = Some(idx),
            Column::Reference if map.reference.is_none() => map.reference = Some(idx),
            Column::Direction if map.direction.is_none() => map.direction = Some(idx),
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

fn require_auto_map(map: ColumnMap) -> crate::error::Result<ColumnMap> {
    if map.date.is_none() {
        return Err(CsvError::MissingDateColumn.into());
    }
    if map.amount.is_none() && map.debit.is_none() && map.credit.is_none() {
        return Err(CsvError::MissingAmountColumn.into());
    }
    Ok(map)
}

fn mapping_from_headers(headers: &StringRecord, map: ColumnMap) -> CsvColumnMapping {
    let name = |idx: Option<usize>| idx.map(|i| headers.get(i).unwrap_or("").to_owned());
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

fn trimmed_nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

fn header_index(headers: &StringRecord, name: &str) -> CsvResult<usize> {
    let needle = name.trim();
    headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case(needle))
        .ok_or_else(|| CsvError::InvalidMapping(format!("CSV has no column named '{name}'")))
}

fn resolve_user_mapping(
    headers: &StringRecord,
    mapping: &CsvColumnMapping,
) -> crate::error::Result<ColumnMap> {
    let Some(date) = trimmed_nonempty(mapping.date.as_deref()) else {
        return Err(CsvError::InvalidMapping("CSV mapping is missing a date column".into()).into());
    };
    let Some(description) = trimmed_nonempty(mapping.description.as_deref()) else {
        return Err(
            CsvError::InvalidMapping("CSV mapping is missing a description column".into()).into(),
        );
    };
    let amount = trimmed_nonempty(mapping.amount.as_deref());
    let debit = trimmed_nonempty(mapping.debit.as_deref());
    let credit = trimmed_nonempty(mapping.credit.as_deref());
    let reference = trimmed_nonempty(mapping.reference.as_deref());
    let direction = trimmed_nonempty(mapping.direction.as_deref());

    let has_amount = amount.is_some();
    let has_dc = debit.is_some() || credit.is_some();
    if has_amount && has_dc {
        return Err(CsvError::InvalidMapping(
            "CSV mapping cannot set both amount and debit/credit".into(),
        )
        .into());
    }
    if !has_amount && (debit.is_none() || credit.is_none()) {
        return Err(CsvError::InvalidMapping(
            "CSV mapping is missing an amount column (provide amount, or debit and credit)".into(),
        )
        .into());
    }

    Ok(ColumnMap {
        date: Some(header_index(headers, date)?),
        description: Some(header_index(headers, description)?),
        amount: amount.map(|n| header_index(headers, n)).transpose()?,
        debit: debit.map(|n| header_index(headers, n)).transpose()?,
        credit: credit.map(|n| header_index(headers, n)).transpose()?,
        reference: reference.map(|n| header_index(headers, n)).transpose()?,
        direction: direction.map(|n| header_index(headers, n)).transpose()?,
    })
}

fn classify_header(raw: &str) -> Option<Column> {
    let spaced = raw.trim().to_lowercase().replace(['_', '-'], " ");
    let n: String = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact: String = n.chars().filter(|c| !c.is_whitespace()).collect();

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
        | CsvError::MissingHeader
        | CsvError::MissingDateColumn
        | CsvError::MissingAmountColumn
        | CsvError::InvalidMapping(_) => {
            log::warn!("CSV record {source_row} could not be read: {err}");
            UiText::new(UiTextCode::CsvUnreadableRow)
        }
    }
}

fn parse_record_inner(
    source_row: u32,
    record: &StringRecord,
    columns: ColumnMap,
    exponent: u8,
) -> CsvResult<ParsedBankRow> {
    let Some(date_idx) = columns.date else {
        return Err(CsvError::MissingDate);
    };
    let date_raw = record_cell(record, date_idx);
    let entry_date = parse_csv_date(date_raw)?;
    let description = columns
        .description
        .map(|idx| record_cell(record, idx).trim().to_owned())
        .unwrap_or_default();
    let reference = columns.reference.and_then(|idx| {
        let value = record_cell(record, idx).trim();
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

fn signed_amount(record: &StringRecord, columns: ColumnMap, exponent: u8) -> CsvResult<i64> {
    if let Some(idx) = columns.amount {
        let mut signed = parse_signed_minor(record_cell(record, idx), exponent)?;
        if let Some(dir_idx) = columns.direction {
            signed = apply_direction(signed, record_cell(record, dir_idx))?;
        }
        return Ok(signed);
    }

    let debit = optional_signed(record, columns.debit, exponent)?;
    let credit = optional_signed(record, columns.credit, exponent)?;
    match (debit, credit) {
        (None, None) => Err(CsvError::MissingAmount),
        (Some(d), None) => Ok(-d.abs()),
        (None, Some(c)) => Ok(c.abs()),
        (Some(d), Some(c)) => d
            .abs()
            .checked_neg()
            .and_then(|out| out.checked_add(c.abs()))
            .ok_or(CsvError::AmountOverflow),
    }
}

fn optional_signed(
    record: &StringRecord,
    idx: Option<usize>,
    exponent: u8,
) -> CsvResult<Option<i64>> {
    let Some(idx) = idx else {
        return Ok(None);
    };
    let raw = record_cell(record, idx).trim();
    if raw.is_empty() {
        return Ok(None);
    }
    parse_signed_minor(raw, exponent).map(Some)
}

fn apply_direction(signed: i64, raw: &str) -> CsvResult<i64> {
    let n = raw.trim().to_lowercase();
    if n.is_empty() {
        return Ok(signed);
    }
    if matches!(
        n.as_str(),
        "d" | "dr" | "debit" | "withdrawal" | "expense" | "out"
    ) {
        return signed
            .checked_abs()
            .map(|v| -v)
            .ok_or(CsvError::AmountOverflow);
    }
    if matches!(
        n.as_str(),
        "c" | "cr" | "credit" | "deposit" | "income" | "in"
    ) {
        return signed.checked_abs().ok_or(CsvError::AmountOverflow);
    }
    Err(CsvError::InvalidType(raw.trim().to_owned()))
}

fn record_cell(record: &StringRecord, idx: usize) -> &str {
    record.get(idx).unwrap_or("")
}

#[cfg(test)]
#[expect(clippy::panic, reason = "tests fail loudly by design")]
mod tests {
    use super::*;

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
        assert!(matches!(err, Error::CsvParse(_)));
        let err = parse_bank_csv("", 2, None).expect_err("empty");
        assert!(matches!(err, Error::CsvParse(_)));
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
        assert!(
            matches!(err, Error::CsvParse(ref message) if message.contains("Nope")),
            "{err}"
        );
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
        assert!(
            matches!(err, Error::CsvParse(ref m) if m.contains("date")),
            "{err}"
        );

        let missing_desc = CsvColumnMapping {
            date: Some("Date".into()),
            amount: Some("Amount".into()),
            ..CsvColumnMapping::default()
        };
        let err = parse_bank_csv(csv, 2, Some(&missing_desc)).expect_err("desc");
        assert!(
            matches!(err, Error::CsvParse(ref m) if m.contains("description")),
            "{err}"
        );

        let missing_amount = column_mapping("Date", "Description", None, None, None);
        let err = parse_bank_csv(csv, 2, Some(&missing_amount)).expect_err("amount");
        assert!(
            matches!(err, Error::CsvParse(ref m) if m.contains("amount")),
            "{err}"
        );

        let both = column_mapping(
            "Date",
            "Description",
            Some("Amount"),
            Some("Debit"),
            Some("Credit"),
        );
        let err = parse_bank_csv(csv, 2, Some(&both)).expect_err("both");
        assert!(
            matches!(err, Error::CsvParse(ref m) if m.contains("both")),
            "{err}"
        );

        let unknown = column_mapping("Date", "Nope", Some("Amount"), None, None);
        let err = parse_bank_csv(csv, 2, Some(&unknown)).expect_err("unknown");
        assert!(
            matches!(err, Error::CsvParse(ref m) if m.contains("Nope")),
            "{err}"
        );
    }
}
