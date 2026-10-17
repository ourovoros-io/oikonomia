//! Names for the columns of a bank CSV, so that a mapping can name each one.
//!
//! A [`CsvColumnMapping`] names columns by header, matched without regard to
//! ASCII case. A file may leave a header empty, which no mapping can name, or
//! repeat one, so that only the leftmost of the two can be named. Each column
//! therefore gets a name that is unique and not empty:
//!
//! | Header                                   | Name                       |
//! |------------------------------------------|----------------------------|
//! | Not empty, no equal header to its left   | The header as written      |
//! | Empty, the third column                  | `Column 3`                 |
//! | The second `Amount` (or `AMOUNT`)        | `Amount (2)` (`AMOUNT (2)`)|
//!
//! A made-up name that another header of the file already has, or that an
//! earlier made-up name took, takes the next free number instead: `Amount (3)`
//! when the file also has a header `Amount (2)`, `Column 3 (2)` when it has a
//! header `Column 3`. The names are reserved before any is made up, so a
//! header keeps its own text wherever it stands. Clean headers, and the names
//! detection and a mapping see for them, are therefore what they were before
//! any name was made up.
//!
//! A made-up name is English and is not meant to be shown: it is the key the
//! mapping sends back. The UI shows its label, a code with parameters it
//! words in the user's language.
//!
//! [`CsvColumnMapping`]: crate::csv::CsvColumnMapping

use std::collections::HashSet;

use csv::StringRecord;

use crate::csv::CsvHeader;
use crate::ui_text::{UiText, UiTextCode};

/// Names every column of `headers`, in file order. The module doc has the
/// rule.
pub(super) fn name_columns(headers: &StringRecord) -> Vec<CsvHeader> {
    // Every header as written is reserved first, so a made-up name never
    // takes the text of a header further right.
    let mut taken: HashSet<String> = headers
        .iter()
        .filter(|header| !header.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let mut seen = HashSet::new();

    headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            if !header.is_empty() && seen.insert(header.to_ascii_lowercase()) {
                CsvHeader {
                    name: header.to_owned(),
                    label: None,
                }
            } else {
                made_up_column(index, header, &mut taken)
            }
        })
        .collect()
}

/// The text a made-up name for an empty header starts with, before the
/// column's position.
const UNNAMED_COLUMN: &str = "Column";

/// Names the column at 0-based `index` whose `header` is empty or repeats one
/// further left, and records the name in `taken`.
fn made_up_column(index: usize, header: &str, taken: &mut HashSet<String>) -> CsvHeader {
    let position = index + 1;

    if header.is_empty() {
        let (name, occurrence) = first_free_name(&format!("{UNNAMED_COLUMN} {position}"), 1, taken);
        // A label without the number would read like the header that took
        // the plain name, in English at least.
        let label = if occurrence == 1 {
            UiText::new(UiTextCode::CsvUnnamedColumn)
        } else {
            UiText::new(UiTextCode::CsvUnnamedRepeatedColumn)
                .with_param("occurrence", occurrence.to_string())
        };
        return CsvHeader {
            name,
            label: Some(label.with_param("position", position.to_string())),
        };
    }

    let (name, occurrence) = first_free_name(header, 2, taken);
    let label = UiText::new(UiTextCode::CsvRepeatedColumn)
        .with_param("name", header)
        .with_param("occurrence", occurrence.to_string());
    CsvHeader {
        name,
        label: Some(label),
    }
}

/// Returns the first free name for `base`, with its occurrence, and records
/// it in `taken`.
///
/// Occurrence 1 is `base` itself and occurrence n is `base (n)`. The search
/// starts at occurrence `first`, and a name is free when `taken` lacks it,
/// ASCII case ignored.
fn first_free_name(base: &str, first: usize, taken: &mut HashSet<String>) -> (String, usize) {
    // Ends: each step that does not return finds a distinct name in `taken`,
    // which is finite, so at most `taken.len() + 1` steps are made.
    let mut occurrence = first;
    loop {
        let name = if occurrence == 1 {
            base.to_owned()
        } else {
            format!("{base} ({occurrence})")
        };
        if taken.insert(name.to_ascii_lowercase()) {
            return (name, occurrence);
        }
        occurrence += 1;
    }
}

#[cfg(test)]
mod tests {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use super::*;

    /// The names of `headers`, as [`name_columns`] gives them.
    fn names(headers: &[&str]) -> Vec<String> {
        name_columns(&StringRecord::from(headers.to_vec()))
            .into_iter()
            .map(|header| header.name)
            .collect()
    }

    #[test]
    fn clean_headers_keep_their_text_and_have_no_label() {
        let named = name_columns(&StringRecord::from(vec!["Date", "Description", "Amount"]));

        assert_eq!(
            named,
            ["Date", "Description", "Amount"].map(|name| CsvHeader {
                name: name.to_owned(),
                label: None,
            })
        );
    }

    #[test]
    fn an_empty_header_is_named_by_its_position() {
        let named = name_columns(&StringRecord::from(vec!["Date", "", "Amount"]));

        assert_eq!(named[1].name, "Column 2");
        assert_eq!(
            named[1].label,
            Some(UiText::new(UiTextCode::CsvUnnamedColumn).with_param("position", "2"))
        );
    }

    #[test]
    fn a_repeated_header_is_numbered_from_two() {
        let named = name_columns(&StringRecord::from(vec![
            "Amount", "Date", "AMOUNT", "amount",
        ]));

        assert_eq!(
            named
                .iter()
                .map(|header| header.name.as_str())
                .collect::<Vec<_>>(),
            ["Amount", "Date", "AMOUNT (2)", "amount (3)"]
        );
        assert_eq!(named[0].label, None);
        assert_eq!(
            named[2].label,
            Some(
                UiText::new(UiTextCode::CsvRepeatedColumn)
                    .with_param("name", "AMOUNT")
                    .with_param("occurrence", "2")
            )
        );
    }

    #[test]
    fn a_made_up_name_skips_one_the_file_already_has() {
        assert_eq!(
            names(&["Amount", "Amount", "amount (2)"]),
            ["Amount", "Amount (3)", "amount (2)"]
        );
        assert_eq!(names(&["", "column 1"]), ["Column 1 (2)", "column 1"]);
        assert_eq!(
            name_columns(&StringRecord::from(vec!["", "column 1"]))[0].label,
            Some(
                UiText::new(UiTextCode::CsvUnnamedRepeatedColumn)
                    .with_param("position", "1")
                    .with_param("occurrence", "2")
            )
        );
        assert_eq!(
            names(&["", "", "Column 2"]),
            ["Column 1", "Column 2 (2)", "Column 2"]
        );
    }

    /// Headers drawn from a small alphabet, so that empty, repeated and
    /// clashing headers are common.
    fn header_rows() -> impl Strategy<Value = Vec<String>> {
        let header = prop_oneof![
            Just(String::new()),
            "[aA]{1,2}",
            "[aA] \\([23]\\)",
            "[cC]olumn [1-4]( \\(2\\))?",
        ];
        prop::collection::vec(header, 1..8)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn names_are_unique_and_not_empty(headers in header_rows()) {
            let named = name_columns(&StringRecord::from(headers.clone()));
            let folded: HashSet<String> =
                named.iter().map(|header| header.name.to_ascii_lowercase()).collect();

            prop_assert_eq!(named.len(), headers.len());
            prop_assert_eq!(folded.len(), named.len());
            prop_assert!(named.iter().all(|header| !header.name.is_empty()));
        }

        #[test]
        fn naming_is_deterministic(headers in header_rows()) {
            let record = StringRecord::from(headers);

            prop_assert_eq!(name_columns(&record), name_columns(&record));
        }

        #[test]
        fn a_first_non_empty_header_keeps_its_text(headers in header_rows()) {
            let named = name_columns(&StringRecord::from(headers.clone()));
            let mut seen = HashSet::new();

            for (header, column) in headers.iter().zip(&named) {
                let clean = !header.is_empty() && seen.insert(header.to_ascii_lowercase());
                prop_assert_eq!(clean, column.label.is_none());
                if clean {
                    prop_assert_eq!(&column.name, header);
                }
            }
        }

        /// Names are unique, so labels worded as the English copy words them
        /// are told apart as well.
        #[test]
        fn an_english_label_reads_as_the_name(headers in header_rows()) {
            for column in name_columns(&StringRecord::from(headers)) {
                let Some(label) = &column.label else { continue };
                prop_assert_eq!(english(label), column.name);
            }
        }
    }

    /// `label` as `web/src/locales/en.json` words it.
    fn english(label: &UiText) -> String {
        let param = |name: &str| label.params.get(name).cloned().unwrap_or_default();
        match label.code {
            UiTextCode::CsvUnnamedColumn => format!("Column {}", param("position")),
            UiTextCode::CsvUnnamedRepeatedColumn => {
                format!("Column {} ({})", param("position"), param("occurrence"))
            }
            UiTextCode::CsvRepeatedColumn => {
                format!("{} ({})", param("name"), param("occurrence"))
            }
            other => format!("not a column label: {other:?}"),
        }
    }
}
