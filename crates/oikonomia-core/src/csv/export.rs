//! Journal CSV export (current entity) and a parser for round-trip tests.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use csv::{ReaderBuilder, Trim, Writer};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::CsvError;
use crate::domain::EntityId;
use crate::error::{Error, Result};
use crate::ledger::get_entity;
use crate::util::format_date;

/// Export column header for integer debit minor units.
pub const DEBIT_MINOR_COLUMN: &str = "debit_minor";
/// Export column header for integer credit minor units.
pub const CREDIT_MINOR_COLUMN: &str = "credit_minor";

/// `posted` or `voided` in the export `status` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalCsvStatus {
    /// Active posted entry (not voided).
    Posted,
    /// Voided original or its reversing entry.
    Voided,
}

/// One journal line as written by [`export_journal_csv`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalCsvLine {
    /// ISO date.
    pub date: String,
    /// Entry description.
    pub description: String,
    /// Optional reference.
    pub reference: Option<String>,
    /// Account code.
    pub account_code: String,
    /// Account display name.
    pub account_name: String,
    /// Debit in integer minor units (0 on the credit side).
    pub debit_minor: i64,
    /// Credit in integer minor units (0 on the debit side).
    pub credit_minor: i64,
    /// `posted` or `voided`.
    pub status: JournalCsvStatus,
}

/// Leading characters that Excel, `LibreOffice` and Numbers read as the start
/// of a formula.
const FORMULA_TRIGGERS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// Make a text cell inert for spreadsheet apps.
///
/// A bank memo or OCR'd line can start with `=`; exported as-is it becomes a
/// live `HYPERLINK`/DDE formula on the accountant's machine. A leading
/// apostrophe turns the cell into literal text. [`restore_formula`] undoes it.
fn neutralize_formula(cell: &str) -> Cow<'_, str> {
    if cell.starts_with(FORMULA_TRIGGERS) {
        Cow::Owned(format!("'{cell}"))
    } else {
        Cow::Borrowed(cell)
    }
}

/// Inverse of [`neutralize_formula`], so a parsed export equals the ledger.
fn restore_formula(cell: &str) -> &str {
    cell.strip_prefix('\'')
        .filter(|rest| rest.starts_with(FORMULA_TRIGGERS))
        .unwrap_or(cell)
}

/// Suggested filename for the native save dialog.
#[must_use]
pub fn default_journal_export_file_name(entity_name: &str) -> String {
    let slug = sanitize_file_stem(entity_name);
    format!("oikonomia-journal-{slug}-{}.csv", local_iso_date())
}

fn local_iso_date() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format_date(now.date())
}

fn sanitize_file_stem(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, ' ' | '-' | '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "entity".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Render the current entity's posted journal (including voided rows) as CSV.
///
/// Columns: `date`, `description`, `reference`, `account_code`, `account_name`,
/// `debit_minor`, `credit_minor`, `status`. Amounts are **integer minor units**,
/// never floating point. `status` is `posted` or `voided`.
///
/// Hidden entries are omitted. Voided-but-visible rows still export;
/// hidden-and-voided do not. There is no "include hidden" switch in v1.
///
/// # Errors
///
/// Unknown entity or database failures.
pub fn export_journal_csv(conn: &Connection, entity_id: EntityId) -> Result<String> {
    let _entity = get_entity(conn, entity_id)?;
    let mut stmt = conn
        .prepare(
            "
            SELECT je.entry_date, je.description, je.reference,
                   a.code, a.name,
                   jl.debit_minor, jl.credit_minor,
                   CASE
                       WHEN je.voided_by_entry_id IS NOT NULL
                         OR EXISTS (
                             SELECT 1 FROM journal_entries x
                             WHERE x.voided_by_entry_id = je.id
                         )
                       THEN 'voided'
                       ELSE 'posted'
                   END AS status
            FROM journal_lines jl
            JOIN journal_entries je ON je.id = jl.entry_id
            JOIN accounts a ON a.id = jl.account_id
            WHERE je.entity_id = ?1
              AND je.status = 'posted'
              AND (je.hidden = 0 OR je.hidden IS NULL)
            ORDER BY je.entry_date ASC, je.created_at ASC, jl.line_order ASC
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut rows = stmt
        .query([entity_id.0.to_string()])
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut buf = Vec::new();
    {
        let mut writer = Writer::from_writer(&mut buf);
        writer
            .write_record([
                "date",
                "description",
                "reference",
                "account_code",
                "account_name",
                DEBIT_MINOR_COLUMN,
                CREDIT_MINOR_COLUMN,
                "status",
            ])
            .map_err(|err| Error::CsvParse(err.to_string()))?;

        while let Some(row) = rows.next().map_err(|err| Error::Io(err.to_string()))? {
            let date: String = row.get(0).map_err(|err| Error::Io(err.to_string()))?;
            let description: String = row.get(1).map_err(|err| Error::Io(err.to_string()))?;
            let reference: Option<String> = row.get(2).map_err(|err| Error::Io(err.to_string()))?;
            let code: String = row.get(3).map_err(|err| Error::Io(err.to_string()))?;
            let name: String = row.get(4).map_err(|err| Error::Io(err.to_string()))?;
            let debit: i64 = row.get(5).map_err(|err| Error::Io(err.to_string()))?;
            let credit: i64 = row.get(6).map_err(|err| Error::Io(err.to_string()))?;
            let status: String = row.get(7).map_err(|err| Error::Io(err.to_string()))?;

            let description = neutralize_formula(&description);
            let reference = neutralize_formula(reference.as_deref().unwrap_or(""));
            let code = neutralize_formula(&code);
            let name = neutralize_formula(&name);
            writer
                .write_record([
                    date.as_str(),
                    &*description,
                    &*reference,
                    &*code,
                    &*name,
                    &debit.to_string(),
                    &credit.to_string(),
                    status.as_str(),
                ])
                .map_err(|err| Error::CsvParse(err.to_string()))?;
        }
        writer
            .flush()
            .map_err(|err| Error::CsvParse(err.to_string()))?;
    }

    String::from_utf8(buf).map_err(|_| CsvError::NotUtf8.into())
}

/// Write [`export_journal_csv`] to `path`, appending `.csv` when missing.
///
/// # Errors
///
/// Export or filesystem errors.
pub fn write_journal_csv_file(
    conn: &Connection,
    entity_id: EntityId,
    path: &Path,
) -> Result<PathBuf> {
    let dest = ensure_csv_path(path.to_path_buf());
    let text = export_journal_csv(conn, entity_id)?;
    std::fs::write(&dest, text.as_bytes()).map_err(|err| Error::Io(err.to_string()))?;
    Ok(dest)
}

/// Append `.csv` when the path has no CSV extension.
#[must_use]
pub fn ensure_csv_path(path: PathBuf) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("csv") => path,
        _ => {
            let mut name = path.file_name().map_or_else(
                || std::ffi::OsString::from("journal"),
                std::ffi::OsString::from,
            );
            name.push(".csv");
            match path.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(parent) => parent.join(name),
                None => PathBuf::from(name),
            }
        }
    }
}

/// Parse a CSV produced by [`export_journal_csv`].
///
/// # Errors
///
/// Missing headers, invalid integers, or unknown status.
pub fn parse_journal_export(text: &str) -> Result<Vec<JournalCsvLine>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(CsvError::Empty.into());
    }
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(Trim::All)
        .from_reader(trimmed.as_bytes());

    let headers = reader
        .headers()
        .map_err(|err| Error::CsvParse(err.to_string()))?
        .clone();
    let date_i = require_col(&headers, "date")?;
    let desc_i = require_col(&headers, "description")?;
    let ref_i = require_col(&headers, "reference")?;
    let code_i = require_col(&headers, "account_code")?;
    let name_i = require_col(&headers, "account_name")?;
    let debit_i = require_col(&headers, DEBIT_MINOR_COLUMN)?;
    let credit_i = require_col(&headers, CREDIT_MINOR_COLUMN)?;
    let status_i = require_col(&headers, "status")?;

    let mut out = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|err| Error::CsvParse(err.to_string()))?;
        let reference = restore_formula(record.get(ref_i).unwrap_or("").trim());
        let status_raw = record.get(status_i).unwrap_or("").trim();
        let status = match status_raw {
            "posted" => JournalCsvStatus::Posted,
            "voided" => JournalCsvStatus::Voided,
            other => {
                return Err(Error::CsvParse(format!("unknown journal status: {other}")));
            }
        };
        out.push(JournalCsvLine {
            date: record.get(date_i).unwrap_or("").to_owned(),
            description: restore_formula(record.get(desc_i).unwrap_or("")).to_owned(),
            reference: if reference.is_empty() {
                None
            } else {
                Some(reference.to_owned())
            },
            account_code: restore_formula(record.get(code_i).unwrap_or("")).to_owned(),
            account_name: restore_formula(record.get(name_i).unwrap_or("")).to_owned(),
            debit_minor: parse_i64(record.get(debit_i).unwrap_or(""))?,
            credit_minor: parse_i64(record.get(credit_i).unwrap_or(""))?,
            status,
        });
    }
    Ok(out)
}

fn require_col(headers: &csv::StringRecord, name: &str) -> Result<usize> {
    headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case(name))
        .ok_or_else(|| Error::CsvParse(format!("journal CSV is missing column {name}")))
}

fn parse_i64(raw: &str) -> Result<i64> {
    raw.trim()
        .parse()
        .map_err(|_| Error::CsvParse(format!("invalid integer: {raw}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_extension_is_added_only_when_missing() {
        let cases = [
            ("books/journal.csv", "books/journal.csv"),
            ("books/journal.CSV", "books/journal.CSV"),
            ("books/journal", "books/journal.csv"),
            ("books/journal.txt", "books/journal.txt.csv"),
            ("journal", "journal.csv"),
            ("my.books/2026", "my.books/2026.csv"),
            // No file name at all: fall back to a default one.
            ("", "journal.csv"),
            ("..", "journal.csv"),
        ];
        for (input, want) in cases {
            assert_eq!(
                ensure_csv_path(PathBuf::from(input)),
                PathBuf::from(want),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn formula_triggers_get_a_literal_text_prefix() {
        let cases = [
            ("=SUM(A1)", "'=SUM(A1)"),
            ("+1", "'+1"),
            ("-5% discount", "'-5% discount"),
            ("@user", "'@user"),
            ("\tcmd", "'\tcmd"),
            ("\rcmd", "'\rcmd"),
            ("Groceries", "Groceries"),
            ("'quoted", "'quoted"),
            ("", ""),
        ];
        for (input, want) in cases {
            assert_eq!(neutralize_formula(input), want, "input {input:?}");
        }
    }

    #[test]
    fn restore_strips_only_the_guard_prefix() {
        let cases = [
            ("'=SUM(A1)", "=SUM(A1)"),
            ("'-5% discount", "-5% discount"),
            ("'quoted", "'quoted"),
            ("Groceries", "Groceries"),
            ("'", "'"),
        ];
        for (input, want) in cases {
            assert_eq!(restore_formula(input), want, "input {input:?}");
        }
    }

    #[test]
    fn sanitize_entity_name_for_filename() {
        let acme = default_journal_export_file_name("Acme Ltd");
        assert!(acme.starts_with("oikonomia-journal-acme-ltd-"), "{acme}");
        assert_eq!(
            std::path::Path::new(&acme)
                .extension()
                .and_then(|e| e.to_str()),
            Some("csv")
        );
        let name = default_journal_export_file_name("!!!");
        assert!(name.contains("entity"), "{name}");
        assert_eq!(
            std::path::Path::new(&name)
                .extension()
                .and_then(|e| e.to_str()),
            Some("csv")
        );
    }
}
