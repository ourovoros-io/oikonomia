//! Journal CSV export of one entity, and the parser that reads it back.
//!
//! The export is one row per journal line, with amounts as integer minor
//! units so that no spreadsheet or locale reformats them on the way.
//!
//! # Formula guard
//!
//! The file is opened in spreadsheet apps, and its text cells come from
//! bank memos, OCR and the user: a cell that starts with `=`, `+`, `-`, `@`,
//! a tab or a carriage return is run as a formula there
//! (<https://owasp.org/www-community/attacks/CSV_Injection>). Every text
//! cell (description, reference, account code, account name) is therefore
//! written through `neutralize_formula`, which puts an apostrophe in front
//! of such a cell. The other columns need no guard: the date and the status
//! are written by this crate, and the amounts are non-negative integers.
//!
//! The guard is reversible. A cell that already starts with an apostrophe
//! is guarded as well, so a leading apostrophe in the file always means
//! "one was added", and `restore_formula` removes exactly that one.
//! [`parse_journal_export`] applies it, so a parsed export equals the
//! ledger text. Both directions are pinned by a property test over
//! arbitrary strings.

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use csv::{ReaderBuilder, StringRecord, Trim, Writer};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::csv::CsvError;
use crate::domain::EntityId;
use crate::error::{DatabaseContext, Error, Result};
use crate::ledger::get_entity;
use crate::vault::files::{local_iso_date, replace_private_file};

/// Export column header for integer debit minor units.
pub(super) const DEBIT_MINOR_COLUMN: &str = "debit_minor";
/// Export column header for integer credit minor units.
pub(super) const CREDIT_MINOR_COLUMN: &str = "credit_minor";

/// `posted` or `voided` in the export `status` column.
///
/// The two words are the file format: this crate writes them and
/// [`parse_journal_export`] accepts no other. Nothing matches on the enum
/// exhaustively, here or in the desktop crate, which does not use it; a
/// third status would be a change to the export format first.
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

/// Returns the file name to suggest when saving an export,
/// `oikonomia-journal-<entity>-YYYY-MM-DD.csv`.
///
/// `<entity>` is `entity_name` reduced to lowercase ASCII letters, digits
/// and single hyphens, or `entity` when nothing is left of it.
#[must_use]
pub fn default_journal_export_file_name(entity_name: &str) -> String {
    let slug = sanitize_file_stem(entity_name);
    format!("oikonomia-journal-{slug}-{}.csv", local_iso_date())
}

/// Renders the posted journal of an entity, voided entries included, as CSV
/// text.
///
/// Columns: `date`, `description`, `reference`, `account_code`, `account_name`,
/// `debit_minor`, `credit_minor`, `status`. Amounts are **integer minor units**,
/// never floating point. `status` is `posted` or `voided`. One row is written
/// per journal line, ordered by entry date, then creation time, then line.
///
/// Text cells that a spreadsheet would run as a formula are prefixed with an
/// apostrophe; [`parse_journal_export`] removes it again.
///
/// Hidden entries are omitted. Voided-but-visible rows still export;
/// hidden-and-voided do not. There is no "include hidden" switch in v1.
///
/// # Errors
///
/// - [`Error::NotFound`] when the entity does not exist.
/// - [`Error::Database`] on database errors.
/// - [`Error::CsvParse`] when the CSV writer reports an error.
pub fn export_journal_csv(conn: &Connection, entity_id: EntityId) -> Result<String> {
    let _entity = get_entity(conn, entity_id)?;
    let mut statement = conn
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
        .database("read journal for export")?;

    let mut rows = statement
        .query([entity_id.0.to_string()])
        .database("read journal for export")?;

    let mut csv_bytes = Vec::new();
    {
        let mut writer = Writer::from_writer(&mut csv_bytes);
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

        while let Some(row) = rows.next().database("read journal for export")? {
            let date: String = row.get(0).database("read journal export row")?;
            let description: String = row.get(1).database("read journal export row")?;
            let reference: Option<String> = row.get(2).database("read journal export row")?;
            let code: String = row.get(3).database("read journal export row")?;
            let name: String = row.get(4).database("read journal export row")?;
            let debit: i64 = row.get(5).database("read journal export row")?;
            let credit: i64 = row.get(6).database("read journal export row")?;
            let status: String = row.get(7).database("read journal export row")?;

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

    // Every cell came from a `String`, so this cannot fail; the conversion
    // is checked anyway because the alternative is an `unsafe` one.
    String::from_utf8(csv_bytes).map_err(|_| CsvError::NotUtf8.into())
}

/// Writes [`export_journal_csv`] to `path`, appending `.csv` when missing,
/// and returns the path written.
///
/// The export is the journal in plaintext, so the file is readable only by
/// its owner (on Unix), and it is written under a temporary name and renamed
/// into place so `path` never holds half an export. The directory of `path`
/// must exist.
///
/// # Errors
///
/// - Every error of [`export_journal_csv`]; nothing is written then.
/// - [`Error::Io`] when the file cannot be created, written or renamed.
pub fn write_journal_csv_file(
    conn: &Connection,
    entity_id: EntityId,
    path: &Path,
) -> Result<PathBuf> {
    let dest = ensure_csv_path(path.to_path_buf());
    let text = export_journal_csv(conn, entity_id)?;
    replace_private_file(&dest, text.as_bytes())?;
    Ok(dest)
}

/// Returns `path` with `.csv` appended unless its extension already is
/// `csv` in any case.
///
/// The extension is appended, never replaced: `journal.txt` becomes
/// `journal.txt.csv`. A path with no file name gets `journal.csv`.
#[must_use]
pub fn ensure_csv_path(path: PathBuf) -> PathBuf {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("csv") => path,
        _ => {
            let mut name = path
                .file_name()
                .map_or_else(|| OsString::from("journal"), OsString::from);
            name.push(".csv");
            match path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
                Some(parent) => parent.join(name),
                None => PathBuf::from(name),
            }
        }
    }
}

/// Parses CSV text produced by [`export_journal_csv`] back into its lines,
/// undoing the formula guard on the text cells.
///
/// Columns are found by header name, without regard to ASCII case and in any
/// order; extra columns are ignored.
///
/// # Errors
///
/// [`Error::CsvParse`] in every case: the text is empty, the header row or
/// a record cannot be read, one of the eight columns is missing, a `status`
/// cell is neither `posted` nor `voided`, or a `debit_minor` or
/// `credit_minor` cell is not an integer.
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
    let date_column = require_column(&headers, "date")?;
    let description_column = require_column(&headers, "description")?;
    let reference_column = require_column(&headers, "reference")?;
    let code_column = require_column(&headers, "account_code")?;
    let name_column = require_column(&headers, "account_name")?;
    let debit_column = require_column(&headers, DEBIT_MINOR_COLUMN)?;
    let credit_column = require_column(&headers, CREDIT_MINOR_COLUMN)?;
    let status_column = require_column(&headers, "status")?;

    let mut lines = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|err| Error::CsvParse(err.to_string()))?;
        let cell = |column: usize| record.get(column).unwrap_or("");

        let reference = restore_formula(cell(reference_column).trim());
        let status = match cell(status_column).trim() {
            "posted" => JournalCsvStatus::Posted,
            "voided" => JournalCsvStatus::Voided,
            other => {
                return Err(Error::CsvParse(format!("unknown journal status: {other}")));
            }
        };
        lines.push(JournalCsvLine {
            date: cell(date_column).to_owned(),
            description: restore_formula(cell(description_column)).to_owned(),
            // The export writes a missing reference as an empty cell.
            reference: (!reference.is_empty()).then(|| reference.to_owned()),
            account_code: restore_formula(cell(code_column)).to_owned(),
            account_name: restore_formula(cell(name_column)).to_owned(),
            debit_minor: parse_minor_units(cell(debit_column))?,
            credit_minor: parse_minor_units(cell(credit_column))?,
            status,
        });
    }
    Ok(lines)
}

/// Leading characters that spreadsheet apps read as the start of a formula
/// (<https://owasp.org/www-community/attacks/CSV_Injection>).
const FORMULA_TRIGGERS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// The prefix that makes a spreadsheet app read a cell as literal text.
const TEXT_GUARD: char = '\'';

/// Makes a text cell inert for spreadsheet apps.
///
/// A bank memo or OCR'd line can start with `=`; exported as-is it becomes a
/// live `HYPERLINK`/DDE formula on the accountant's machine. A leading
/// apostrophe turns the cell into literal text.
///
/// A cell that already starts with an apostrophe is guarded too. Otherwise
/// the ledger text `'=foo` and the guarded form of `=foo` would be the same
/// exported cell, and [`restore_formula`] could not tell them apart.
fn neutralize_formula(cell: &str) -> Cow<'_, str> {
    if cell.starts_with(FORMULA_TRIGGERS) || cell.starts_with(TEXT_GUARD) {
        Cow::Owned(format!("{TEXT_GUARD}{cell}"))
    } else {
        Cow::Borrowed(cell)
    }
}

/// Exact inverse of [`neutralize_formula`], so a parsed export equals the
/// ledger: strips one apostrophe when what follows is a cell that
/// `neutralize_formula` would have guarded.
fn restore_formula(cell: &str) -> &str {
    cell.strip_prefix(TEXT_GUARD)
        .filter(|rest| rest.starts_with(FORMULA_TRIGGERS) || rest.starts_with(TEXT_GUARD))
        .unwrap_or(cell)
}

/// Reduces an entity name to a file-name stem that is safe on every
/// platform: lowercase ASCII letters and digits, with each run of spaces,
/// hyphens and underscores as one hyphen.
///
/// Every other character is dropped, so a name in a non-Latin script comes
/// out empty and the stem falls back to `entity`.
fn sanitize_file_stem(name: &str) -> String {
    let mut stem = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            stem.push(character.to_ascii_lowercase());
        } else if matches!(character, ' ' | '-' | '_') && !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let trimmed = stem.trim_matches('-');
    if trimmed.is_empty() {
        "entity".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Returns the index of the header equal to `name`, ignoring ASCII case.
///
/// # Errors
///
/// [`Error::CsvParse`] naming the column when the export has none.
fn require_column(headers: &StringRecord, name: &str) -> Result<usize> {
    headers
        .iter()
        .position(|header| header.eq_ignore_ascii_case(name))
        .ok_or_else(|| Error::CsvParse(format!("journal CSV is missing column {name}")))
}

/// Parses a `debit_minor` or `credit_minor` cell.
///
/// # Errors
///
/// [`Error::CsvParse`] carrying the cell when it is not an integer that
/// fits an `i64`.
fn parse_minor_units(raw: &str) -> Result<i64> {
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
            ("'quoted", "''quoted"),
            ("'=SUM(A1)", "''=SUM(A1)"),
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
            ("''quoted", "'quoted"),
            ("''=SUM(A1)", "'=SUM(A1)"),
            ("Groceries", "Groceries"),
            // Never written by the export: left as they are.
            ("'quoted", "'quoted"),
            ("'", "'"),
        ];
        for (input, want) in cases {
            assert_eq!(restore_formula(input), want, "input {input:?}");
        }
    }

    #[test]
    fn neutralize_then_restore_returns_the_original_cell() {
        let cells = [
            "",
            "Groceries",
            "=SUM(A1)",
            "+1",
            "-5% discount",
            "@user",
            "\tcmd",
            "\rcmd",
            "'",
            "''",
            "'quoted",
            "'=foo",
            "''=foo",
            "'-5",
            "it's fine",
        ];
        for cell in cells {
            let exported = neutralize_formula(cell);
            assert!(
                !exported.starts_with(FORMULA_TRIGGERS),
                "exported {exported:?} still starts a formula"
            );
            assert_eq!(restore_formula(&exported), cell, "exported {exported:?}");
        }
    }

    #[test]
    fn sanitize_entity_name_for_filename() {
        let acme = default_journal_export_file_name("Acme Ltd");
        assert!(acme.starts_with("oikonomia-journal-acme-ltd-"), "{acme}");
        assert_eq!(
            std::path::Path::new(&acme)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("csv")
        );
        let name = default_journal_export_file_name("!!!");
        assert!(name.contains("entity"), "{name}");
        assert_eq!(
            std::path::Path::new(&name)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("csv")
        );
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;
    use proptest::test_runner::TestRunner;
    use tempfile::TempDir;

    use super::*;
    use crate::domain::ChartTemplate;
    use crate::ledger::{
        CreateEntity, CreateJournalLine, PostJournal, create_entity, list_accounts, post_entry,
    };
    use crate::prefs::Locale;
    use crate::vault::Vault;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn restoring_a_guarded_cell_gives_the_cell_back(cell in any::<String>()) {
            let guarded = neutralize_formula(&cell);

            prop_assert_eq!(restore_formula(&guarded), cell.as_str());
        }
    }

    /// Posts one two-line entry with `description` in a new book, and returns
    /// the book with the description the ledger stored.
    fn post_in_new_book(conn: &Connection, number: u32, description: &str) -> (EntityId, String) {
        let book = CreateEntity {
            name: format!("Book {number}"),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        };
        let entity = create_entity(conn, &book, Locale::En).unwrap();
        let accounts = list_accounts(conn, entity.id).unwrap();
        let line = |index: usize, debit_minor: i64, credit_minor: i64| CreateJournalLine {
            account_id: accounts[index].id,
            debit_minor,
            credit_minor,
            memo: None,
        };

        let entry = PostJournal {
            entity_id: entity.id,
            entry_date: "2026-03-15".into(),
            description: description.into(),
            reference: None,
            lines: vec![line(0, 100, 0), line(1, 0, 100)],
        };
        let posted = post_entry(conn, &entry).unwrap();

        (entity.id, posted.entry.description)
    }

    // One vault for the whole run: deriving its key costs far more than a
    // case does, so the cases share it and each one posts into a book of its
    // own.
    #[test]
    fn an_exported_description_parses_back_as_the_ledger_stores_it() {
        let dir = TempDir::new().unwrap();
        let mut vault = Vault::open_path(dir.path()).unwrap();
        vault.init("correct horse battery staple").unwrap();
        let conn = vault.connection().unwrap();
        let books = std::cell::Cell::new(0_u32);

        let mut runner = TestRunner::new(ProptestConfig::with_cases(PROPERTY_CASES));
        let outcome = runner.run(&any::<String>(), |description| {
            books.set(books.get() + 1);
            let (entity_id, stored) = post_in_new_book(conn, books.get(), &description);

            let exported = export_journal_csv(conn, entity_id).unwrap();
            let lines = parse_journal_export(&exported);
            prop_assert!(lines.is_ok(), "{:?} from {:?}", lines, exported);

            let descriptions: Vec<String> = lines
                .unwrap()
                .into_iter()
                .map(|line| line.description)
                .collect();
            prop_assert_eq!(descriptions, vec![stored.clone(), stored]);
            Ok(())
        });

        outcome.unwrap();
    }
}
