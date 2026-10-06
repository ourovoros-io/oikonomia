//! A case fold for SQL text comparisons that covers every letter.
//!
//! `SQLite`'s own `lower()`, `upper()` and `LIKE` fold ASCII letters only, so
//! "Τρόφιμα" and "τρόφιμα", or "É" and "é", compare as different. The
//! application-defined `fold(text)` function folds with Rust's Unicode rules
//! instead, and [`fold_case`] does the same on the Rust side so a search term
//! and the stored text always fold alike.

use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::{Value, ValueRef};

/// Name of the SQL function, as used in queries: `fold(column) = fold(?1)`.
///
/// This is the one place the name is spelled for registration; the queries
/// that call the function spell it in their SQL text.
const FOLD_FUNCTION: &str = "fold";

/// Registers the `fold(text)` SQL function on `conn`.
///
/// Every connection to a vault must call this once after it is opened, before
/// any query that uses `fold`. The function folds text with [`fold_case`]; a
/// `NULL` argument stays `NULL`, and an integer, real or blob argument is
/// returned unchanged, so a stray non-text value never fails the whole query.
///
/// The function is registered per connection and is not stored in the
/// database, so it must never be used in an index, generated column, view or
/// trigger: another tool opening the file would not find it.
///
/// # Errors
///
/// Returns the `SQLite` error if the function cannot be registered.
pub fn register_fold(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        FOLD_FUNCTION,
        1,
        // Deterministic: the same text always folds to the same result.
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let value = match context.get_raw(0) {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(number) => Value::Integer(number),
                ValueRef::Real(number) => Value::Real(number),
                ValueRef::Blob(bytes) => Value::Blob(bytes.to_vec()),
                // SQLite text is valid UTF-8; invalid bytes, were any stored,
                // fold with the replacement character rather than failing.
                ValueRef::Text(bytes) => Value::Text(fold_case(&String::from_utf8_lossy(bytes))),
            };

            Ok(value)
        },
    )
}

/// Folds `text` for a case-insensitive comparison.
///
/// Use this on the Rust side of a comparison, and `fold(...)` on the SQL side
/// (see [`register_fold`]), so both sides fold the same way. Exactly this:
///
/// - Every letter is lowercased on its own, in every script. `str::to_lowercase`
///   would pick the final sigma "ς" for a capital "Σ" ending the text, so each
///   letter is lowercased separately instead.
/// - Greek final sigma "ς" becomes "σ", so a typed "σ" matches a stored "ς".
/// - Greek accents are removed, because capitals are written without them:
///   ά έ ή ί ό ύ ώ become α ε η ι ο υ ω, and the dialytika forms ϊ ϋ ΐ ΰ become
///   ι υ ι υ. "ΛΟΓΑΡΙΑΣΜΟΣ" therefore matches "Λογαριασμός".
///
/// Its limits:
///
/// - Accents of other languages stay distinct: "e" is not "é", "a" is not "ä".
/// - "ß" is only lowercased, not expanded: it is not equal to "ss".
/// - There is no Unicode normalisation: a letter written as a base letter plus
///   a combining accent does not equal the precomposed letter.
#[must_use]
pub fn fold_case(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(unaccented_greek)
        .collect()
}

/// Returns a Greek letter without its accent and with sigma in one form; any
/// other letter is returned as it is. The input is already lowercase.
const fn unaccented_greek(letter: char) -> char {
    match letter {
        'ς' => 'σ',
        'ά' => 'α',
        'έ' => 'ε',
        'ή' => 'η',
        'ί' | 'ϊ' | 'ΐ' => 'ι',
        'ό' => 'ο',
        'ύ' | 'ϋ' | 'ΰ' => 'υ',
        'ώ' => 'ω',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::types::Value;

    /// The text `SELECT fold(?1)` gives for `sql_text`; `None` is SQL `NULL`.
    fn folded(sql_text: Option<&str>) -> Option<String> {
        let conn = Connection::open_in_memory().expect("memory");
        register_fold(&conn).expect("register");

        conn.query_row("SELECT fold(?1)", [sql_text], |row| row.get(0))
            .expect("fold")
    }

    /// The value `SELECT <expression>` gives on a connection with `fold`.
    fn value_of(expression: &str) -> Value {
        let conn = Connection::open_in_memory().expect("memory");
        register_fold(&conn).expect("register");

        conn.query_row(&format!("SELECT {expression}"), [], |row| row.get(0))
            .expect("query")
    }

    #[test]
    fn it_lowercases_every_script() {
        assert_eq!(folded(Some("ΛΟΓΑΡΙΑΣΜΌΣ")).as_deref(), Some("λογαριασμοσ"));
        assert_eq!(folded(Some("ÉPICERIE")).as_deref(), Some("épicerie"));
        assert_eq!(folded(Some("ÄRZTLICHE")).as_deref(), Some("ärztliche"));
        assert_eq!(folded(Some("Groceries")).as_deref(), Some("groceries"));
    }

    #[test]
    fn a_final_sigma_folds_like_a_medial_one() {
        assert_eq!(folded(Some("ΛΟΓΑΡΙΑΣ")), folded(Some("λογαριασ")));
        assert_eq!(folded(Some("λογαριασμός")), folded(Some("ΛΟΓΑΡΙΑΣΜΌΣ")));
        assert_eq!(folded(Some("ς")).as_deref(), Some("σ"));
    }

    #[test]
    fn it_drops_every_greek_accent() {
        for (accented, plain) in [
            ("ά", "α"),
            ("έ", "ε"),
            ("ή", "η"),
            ("ί", "ι"),
            ("ό", "ο"),
            ("ύ", "υ"),
            ("ώ", "ω"),
            ("ϊ", "ι"),
            ("ϋ", "υ"),
            ("ΐ", "ι"),
            ("ΰ", "υ"),
            ("Ά", "α"),
            ("Έ", "ε"),
            ("Ή", "η"),
            ("Ί", "ι"),
            ("Ό", "ο"),
            ("Ύ", "υ"),
            ("Ώ", "ω"),
            ("Ϊ", "ι"),
            ("Ϋ", "υ"),
        ] {
            assert_eq!(fold_case(accented), plain, "{accented:?}");
        }

        assert_eq!(fold_case("ΛΟΓΑΡΙΑΣΜΟΣ"), fold_case("Λογαριασμός"));
        assert_eq!(fold_case("Τροφιμα"), fold_case("Τρόφιμα"));
    }

    #[test]
    fn other_accents_and_the_sharp_s_stay_distinct() {
        assert_ne!(fold_case("e"), fold_case("é"));
        assert_ne!(fold_case("a"), fold_case("ä"));
        assert_ne!(fold_case("epicerie"), fold_case("Épicerie"));
        assert_ne!(fold_case("Arztliche"), fold_case("Ärztliche"));
        assert_eq!(fold_case("Épicerie"), "épicerie");
        assert_eq!(fold_case("Straße"), "straße");
        assert_ne!(fold_case("Straße"), fold_case("Strasse"));
    }

    #[test]
    fn it_keeps_accents_and_null() {
        assert_eq!(folded(Some("é")).as_deref(), Some("é"));
        assert_eq!(folded(None), None);
    }

    #[test]
    fn a_value_that_is_not_text_is_returned_unchanged() {
        assert_eq!(value_of("fold(NULL)"), Value::Null);
        assert_eq!(value_of("fold(42)"), Value::Integer(42));
        assert_eq!(value_of("fold(1.5)"), Value::Real(1.5));
        assert_eq!(value_of("fold(x'00ff')"), Value::Blob(vec![0x00, 0xff]));
        assert_eq!(value_of("fold('ΑΒΓ')"), Value::Text("αβγ".to_owned()));
    }
}
