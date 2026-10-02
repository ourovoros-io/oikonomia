//! A case fold for SQL text comparisons that covers every letter.
//!
//! `SQLite`'s own `lower()`, `upper()` and `LIKE` fold ASCII letters only, so
//! "Τρόφιμα" and "τρόφιμα", or "É" and "é", compare as different. The
//! application-defined `fold(text)` function lowercases with Rust's Unicode
//! rules instead. Only case is folded: "e" and "é" stay different letters.

use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

/// Name of the SQL function, as used in queries: `fold(column) = fold(?1)`.
///
/// The function exists only on a connection that went through
/// [`register_fold`]; it is not stored in the database, so it must never
/// appear in an index, generated column, view or trigger.
pub const FOLD_FUNCTION: &str = "fold";

/// Register the `fold(text)` SQL function on `conn`.
///
/// Every connection to a vault must call this once after it is opened, before
/// any query that uses `fold`. A `NULL` argument folds to `NULL`.
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
            let text: Option<String> = context.get(0)?;

            Ok(text.map(|text| fold_case(&text)))
        },
    )
}

/// Fold `text` for a case-insensitive comparison.
///
/// Use this on the Rust side of a comparison so both sides fold the same way.
///
/// Each letter is lowercased on its own. `str::to_lowercase` would pick the
/// final sigma "ς" for a capital "Σ" at the end of the text, so a search for
/// "ΛΟΓΑΡΙΑΣ" would fold to "λογαριας" and miss "λογαριασμός"; and a stored
/// "ς" must equal a typed "σ". Greek sigma therefore folds to "σ" in every
/// position, as Unicode case folding does.
#[must_use]
pub fn fold_case(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|letter| if letter == 'ς' { 'σ' } else { letter })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(clippy::expect_used, reason = "tests fail loudly by design")]
    fn folded(sql_text: Option<&str>) -> Option<String> {
        let conn = Connection::open_in_memory().expect("memory");
        register_fold(&conn).expect("register");

        conn.query_row("SELECT fold(?1)", [sql_text], |row| row.get(0))
            .expect("fold")
    }

    #[test]
    fn it_lowercases_every_script() {
        assert_eq!(folded(Some("ΛΟΓΑΡΙΑΣΜΌΣ")).as_deref(), Some("λογαριασμόσ"));
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
    fn it_keeps_accents_and_null() {
        assert_eq!(folded(Some("é")).as_deref(), Some("é"));
        assert_eq!(folded(None), None);
    }
}
