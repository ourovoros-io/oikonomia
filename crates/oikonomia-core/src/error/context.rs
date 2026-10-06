//! Conversions from a lower-level failure to [`Error`] at the call site.
//!
//! A call that can fail below this crate names what core was doing and picks
//! the variant by what failed:
//!
//! ```ignore
//! conn.execute(sql, params).database("insert journal entry")?;
//! fs::rename(from, to).io("rename vault header")?;
//! serde_json::to_string(&prefs).serialization("encode preferences")?;
//! ```
//!
//! [`DatabaseContext`], [`IoContext`] and [`SerializationContext`] are each
//! implemented for one foreign error type only, so the compiler refuses
//! `.io(..)` on a `rusqlite` result: a database failure cannot be filed under
//! the code for a file failure by mistake. [`CryptoContext`] and
//! [`AnalysisContext`] take any error, because the failures they describe come
//! from several libraries (Argon2 and `SQLCipher`; the image decoder and the
//! OCR engine) and are told apart by where they happen, not by their type.
//!
//! The operation is a lowercase phrase with no trailing period that reads
//! after "cannot", such as `"open vault database"`.

use crate::error::{Error, Result};
use std::fmt::Display;

/// Turns a `rusqlite` failure into [`Error::Database`].
pub(crate) trait DatabaseContext<T> {
    /// Reports the failure as a database failure during `operation`.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] carrying `operation` and the driver's text.
    fn database(self, operation: &'static str) -> Result<T>;
}

impl<T> DatabaseContext<T> for rusqlite::Result<T> {
    fn database(self, operation: &'static str) -> Result<T> {
        self.map_err(|err| Error::database(operation, err))
    }
}

/// Turns a `std::io` failure into [`Error::Io`].
pub(crate) trait IoContext<T> {
    /// Reports the failure as a file or directory failure during `operation`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] carrying `operation` and the operating system's text.
    fn io(self, operation: &'static str) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn io(self, operation: &'static str) -> Result<T> {
        self.map_err(|err| Error::io(operation, err))
    }
}

/// Turns a `serde_json` failure into [`Error::Serialization`].
pub(crate) trait SerializationContext<T> {
    /// Reports the failure as an encoding or decoding failure during
    /// `operation`.
    ///
    /// # Errors
    ///
    /// [`Error::Serialization`] carrying `operation` and the encoder's text.
    fn serialization(self, operation: &'static str) -> Result<T>;
}

impl<T> SerializationContext<T> for serde_json::Result<T> {
    fn serialization(self, operation: &'static str) -> Result<T> {
        self.map_err(|err| Error::serialization(operation, err))
    }
}

/// Turns a key-derivation or cipher-setting failure into [`Error::Crypto`].
pub(crate) trait CryptoContext<T> {
    /// Reports the failure as a cryptographic failure during `operation`.
    ///
    /// # Errors
    ///
    /// [`Error::Crypto`] carrying `operation` and the library's text.
    fn crypto(self, operation: &'static str) -> Result<T>;
}

impl<T, E> CryptoContext<T> for std::result::Result<T, E>
where
    E: Display,
{
    fn crypto(self, operation: &'static str) -> Result<T> {
        self.map_err(|err| Error::crypto(operation, err))
    }
}

/// Turns an image-decoding or OCR failure into [`Error::Analysis`].
pub(crate) trait AnalysisContext<T> {
    /// Reports the failure as a failure to read an image during `operation`.
    ///
    /// # Errors
    ///
    /// [`Error::Analysis`] carrying `operation` and the library's text.
    fn analysis(self, operation: &'static str) -> Result<T>;
}

impl<T, E> AnalysisContext<T> for std::result::Result<T, E>
where
    E: Display,
{
    fn analysis(self, operation: &'static str) -> Result<T> {
        self.map_err(|err| Error::analysis(operation, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_database_failure_keeps_the_operation_and_the_driver_text() {
        let failed: rusqlite::Result<()> = Err(rusqlite::Error::QueryReturnedNoRows);

        let err = failed.database("read schema version").expect_err("failed");

        assert_eq!(err.code(), "database");
        assert_eq!(
            err.to_string(),
            "read schema version: Query returned no rows"
        );
    }

    #[test]
    fn a_file_failure_keeps_the_operation_and_the_system_text() {
        let failed: std::io::Result<()> = Err(std::io::Error::other("disk on fire"));

        let err = failed.io("write vault header").expect_err("failed");

        assert_eq!(err.code(), "io");
        assert_eq!(err.to_string(), "write vault header: disk on fire");
    }

    #[test]
    fn an_encoding_failure_is_a_serialization_error() {
        let failed = serde_json::from_str::<u8>("nope");

        let err = failed.serialization("decode a number").expect_err("failed");

        assert_eq!(err.code(), "serialization");
        assert!(err.to_string().starts_with("decode a number: "), "{err}");
    }

    #[test]
    fn crypto_and_analysis_take_any_displayable_failure() {
        let failed: std::result::Result<(), &str> = Err("bad parameters");

        assert_eq!(
            failed.crypto("derive vault key").map_err(|err| err.code()),
            Err("crypto")
        );
        assert_eq!(
            failed.analysis("decode image").map_err(|err| err.code()),
            Err("analysis")
        );
    }
}
