//! What an error, a path or a foreign error's text contributes to a log line.
//!
//! A release build of the desktop app keeps a local log of warnings and
//! errors. Nothing from the ledger may reach that file: no amount,
//! description, merchant, account or entity name, document name or contents,
//! password or key material, and no path the user chose. The log line is
//! formatted at the call site, long before the logger sees it, so the choice
//! of what goes in is made here, by the two types every `log::` call in the
//! workspace uses for anything that is not a fixed phrase, a number or one of
//! the application's own paths:
//!
//! - [`LogText`], from [`Error::log_text`], for an [`enum@Error`];
//! - [`PrivateDetail`] for a path that may be the user's, and for the text of
//!   a foreign error that may quote its input.
//!
//! # Reduced unless asked otherwise
//!
//! Both types write their reduced form, described below, until
//! [`enable_log_detail`] is called. After it they write everything: the full
//! `Display` of the error and the value inside a [`PrivateDetail`]. The
//! desktop shell calls it in a debug build only, where the log is the
//! developer's own console, so a release build never leaves the reduced
//! form. A program that forgets the call logs less, never more, and a test
//! in any crate sees what a release build would write.
//!
//! The switch is process-wide because the `log` facade is: a log line is
//! formatted wherever the failure happens, with no handle to pass a choice
//! through. Each form is written by one private function per type that takes
//! the choice as an argument, which is how the tests here reach the full
//! form without touching the switch.
//!
//! # The reduced form of an error
//!
//! The code always. After it, only what cannot hold user data:
//!
//! - for a failure below the crate, the `operation`, a fixed phrase written
//!   at the call site. The `detail` is left out: a `serde` message can quote
//!   its input, a file error names a path that may be the user's backup
//!   destination, and a driver error can carry a stored value;
//! - for a corrupt vault or an invalid backup, the reason without its
//!   free-text fields, so the log still tells a header that does not parse
//!   from a database that is missing;
//! - for a variant whose payload is a version or a kind of record, the
//!   message;
//! - for an unbalanced entry, a validation error and a CSV error, nothing
//!   more. Their payloads are amounts, names and cell values.
//!
//! Every match here is exhaustive, so a new variant does not compile until
//! its reduced form is decided.

use crate::error::{BackupDefect, Error, VaultCorruption};
use std::fmt::{self, Display, Formatter};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether log lines carry lower-level detail. Unset until
/// [`enable_log_detail`] is called, and never unset again.
///
/// The flag guards no other data, so `Relaxed` is enough: a line formatted
/// while the flag is being set gets one form or the other, both of which are
/// valid before the call returns.
static DETAIL_IN_LOGS: AtomicBool = AtomicBool::new(false);

/// What stands in a reduced log line where a [`PrivateDetail`] was.
const WITHHELD: &str = "<withheld>";

/// Makes every later log line carry full detail: the `Display` text of an
/// error, and paths and foreign error text that are otherwise withheld.
///
/// For a debug build, whose log is the developer's console. A build that
/// writes its log where it outlives the session must not call this: the
/// detail can hold paths the user chose and values from the ledger. There is
/// no way to turn it off again.
///
/// # Examples
///
/// ```
/// use oikonomia_core::Error;
/// use oikonomia_core::error::enable_log_detail;
///
/// let unbalanced = Error::UnbalancedEntry { debits: 100, credits: 50 };
/// assert_eq!(unbalanced.log_text().to_string(), "unbalanced_entry");
///
/// enable_log_detail();
/// assert_eq!(unbalanced.log_text().to_string(), unbalanced.to_string());
/// ```
pub fn enable_log_detail() {
    DETAIL_IN_LOGS.store(true, Ordering::Relaxed);
}

/// Returns whether [`enable_log_detail`] has been called.
fn detail_in_logs() -> bool {
    DETAIL_IN_LOGS.load(Ordering::Relaxed)
}

/// An [`enum@Error`] as a log line shows it; see [`Error::log_text`].
#[derive(Debug, Clone, Copy)]
pub struct LogText<'a>(&'a Error);

impl Display for LogText<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write_error(f, self.0, detail_in_logs())
    }
}

impl Error {
    /// Returns the error in the form a log line may hold.
    ///
    /// That is the code, followed by the operation or the reason where there
    /// is one, and never a `detail`, an amount, a name or a cell value; the
    /// [module documentation](self) lists what each variant keeps. After
    /// [`enable_log_detail`] it is the `Display` text.
    ///
    /// Use it for every error interpolated into a `log::` call.
    ///
    /// # Examples
    ///
    /// ```
    /// use oikonomia_core::Error;
    ///
    /// let line = format!("unlock failed: {}", Error::VaultLocked.log_text());
    ///
    /// assert!(line.contains("vault is locked"));
    /// ```
    #[must_use]
    pub fn log_text(&self) -> LogText<'_> {
        LogText(self)
    }
}

/// A value that reaches the log only after [`enable_log_detail`].
///
/// For a path that may be one the user chose (a backup destination and the
/// files staged beside it), and for the text of a foreign error that may
/// quote its input. Until then `<withheld>` is written in its place.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PrivateDetail<T>(pub(crate) T);

impl<T: Display> Display for PrivateDetail<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write_private_detail(f, &self.0, detail_in_logs())
    }
}

/// Writes `value` when `with_detail` is set, and the placeholder otherwise.
fn write_private_detail(
    f: &mut Formatter<'_>,
    value: &dyn Display,
    with_detail: bool,
) -> fmt::Result {
    if with_detail {
        value.fmt(f)
    } else {
        f.write_str(WITHHELD)
    }
}

/// Writes `error` in full when `with_detail` is set, and in its reduced form
/// otherwise.
fn write_error(f: &mut Formatter<'_>, error: &Error, with_detail: bool) -> fmt::Result {
    if with_detail {
        return error.fmt(f);
    }

    let code = error.code();
    match error {
        // No payload, or one that is a version or a kind of record.
        Error::VaultUninitialized
        | Error::VaultLocked
        | Error::InvalidPassword
        | Error::TooFewLines
        | Error::InvalidLineAmounts
        | Error::AccountWrongEntity
        | Error::MoneyOverflow
        | Error::NegativeMoney
        | Error::VaultTooNew { .. }
        | Error::RestoreWouldOverwrite
        | Error::NotFound(_) => write!(f, "{code}: {error}"),

        // Amounts, names and cell values.
        Error::UnbalancedEntry { .. } | Error::Validation(_) | Error::Csv(_) => f.write_str(code),

        Error::Database { operation, .. }
        | Error::Io { operation, .. }
        | Error::Serialization { operation, .. }
        | Error::Crypto { operation, .. }
        | Error::Analysis { operation, .. } => write!(f, "{code}: {operation}"),

        Error::VaultCorrupt(reason) => {
            write!(f, "{code}: ")?;
            write_corruption(f, reason)
        }
        Error::BackupInvalid(defect) => {
            write!(f, "{code}: ")?;
            write_defect(f, defect)
        }
    }
}

/// Writes `reason` without its free-text fields.
///
/// A `column` is kept: it is the name of a table column, written in core.
fn write_corruption(f: &mut Formatter<'_>, reason: &VaultCorruption) -> fmt::Result {
    match reason {
        VaultCorruption::Column { column, .. } => write!(f, "{column} holds an unusable value"),
        VaultCorruption::HeaderUnreadable { .. } => f.write_str("vault header does not parse"),
        VaultCorruption::HeaderField { field, .. } => {
            write!(f, "vault header has an unusable {field}")
        }
        VaultCorruption::UnsupportedFormat { .. }
        | VaultCorruption::MissingMetaTable
        | VaultCorruption::MissingSchemaVersion
        | VaultCorruption::HeaderWithoutDatabase
        | VaultCorruption::DatabaseWithoutHeader
        | VaultCorruption::EmptyFile { .. }
        | VaultCorruption::UnmergedWriteAheadLog
        | VaultCorruption::UnfinishedPasswordChange
        | VaultCorruption::Setting { .. }
        | VaultCorruption::InvalidJournalLines { .. } => reason.fmt(f),
    }
}

/// Writes `defect` without the member names the archive itself supplied.
fn write_defect(f: &mut Formatter<'_>, defect: &BackupDefect) -> fmt::Result {
    match defect {
        BackupDefect::EmptyMember { .. } => f.write_str("a member is empty"),
        BackupDefect::UnexpectedMember { .. } => f.write_str("unexpected member"),
        BackupDefect::UnusableHeader(reason) => {
            f.write_str("vault header in the backup is not usable: ")?;
            write_corruption(f, reason)
        }
        BackupDefect::NotABackup
        | BackupDefect::UnsupportedVersion { .. }
        | BackupDefect::Truncated
        | BackupDefect::TrailingData
        | BackupDefect::DuplicateMember { .. }
        | BackupDefect::MissingMember { .. }
        | BackupDefect::MemberNameLength
        | BackupDefect::MemberNameNotUtf8
        | BackupDefect::DatabaseNotEncrypted => defect.fmt(f),
    }
}

#[cfg(test)]
mod tests {
    use super::{PrivateDetail, WITHHELD, write_error, write_private_detail};
    use crate::csv::CsvError;
    use crate::error::{BackupDefect, Error, Resource, ValidationError, VaultCorruption};
    use std::collections::BTreeSet;
    use std::fmt::{self, Display, Formatter};

    /// The text every free-text field of the samples carries.
    const SENTINEL: &str = "Acme-Payroll-7731";

    /// The amount every amount field of the samples carries.
    const SENTINEL_AMOUNT: i64 = 918_273_645;

    /// An error formatted in the reduced form (`with_detail` unset) or in
    /// full, whatever the process-wide switch says.
    struct Rendered<'a>(&'a Error, bool);

    impl Display for Rendered<'_> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write_error(f, self.0, self.1)
        }
    }

    /// Returns the reduced form of `error`, the one a release build logs.
    fn release_form(error: &Error) -> String {
        Rendered(error, false).to_string()
    }

    /// Returns every reason a vault is corrupt for that has a free-text
    /// field, with the sentinel in each such field.
    fn corruptions_with_text() -> Vec<VaultCorruption> {
        vec![
            VaultCorruption::Column {
                column: "journal_lines.amount_minor".into(),
                detail: format!("not an amount: {SENTINEL}"),
            },
            VaultCorruption::HeaderUnreadable {
                detail: format!("invalid type: string \"{SENTINEL}\""),
            },
            VaultCorruption::HeaderField {
                field: "salt",
                detail: SENTINEL.into(),
            },
        ]
    }

    /// Returns one error per variant, with the sentinel in every field that
    /// can hold text or an amount, and one per reason or defect that has
    /// such a field.
    fn samples() -> Vec<Error> {
        let text = || SENTINEL.to_owned();
        let failure = |build: fn(&'static str, String) -> Error| build("write backup", text());

        let mut samples = vec![
            Error::VaultUninitialized,
            Error::VaultLocked,
            Error::InvalidPassword,
            Error::UnbalancedEntry {
                debits: SENTINEL_AMOUNT,
                credits: 1,
            },
            Error::TooFewLines,
            Error::InvalidLineAmounts,
            Error::AccountWrongEntity,
            Error::MoneyOverflow,
            Error::NegativeMoney,
            Error::Validation(ValidationError::Internal { detail: text() }),
            failure(|operation, detail| Error::Database { operation, detail }),
            failure(|operation, detail| Error::Io { operation, detail }),
            failure(|operation, detail| Error::Serialization { operation, detail }),
            failure(|operation, detail| Error::Crypto { operation, detail }),
            Error::VaultCorrupt(VaultCorruption::UnfinishedPasswordChange),
            Error::VaultTooNew {
                found: 9,
                supported: 8,
            },
            Error::BackupInvalid(BackupDefect::EmptyMember { name: text() }),
            Error::BackupInvalid(BackupDefect::UnexpectedMember { name: text() }),
            Error::RestoreWouldOverwrite,
            Error::NotFound(Resource::Account),
            failure(|operation, detail| Error::Analysis { operation, detail }),
            Error::Csv(CsvError::InvalidAmount(text())),
            Error::Csv(CsvError::Malformed { detail: text() }),
        ];

        for reason in corruptions_with_text() {
            samples.push(Error::BackupInvalid(BackupDefect::UnusableHeader(
                reason.clone(),
            )));
            samples.push(Error::VaultCorrupt(reason));
        }
        samples
    }

    /// The samples would prove nothing about a variant they leave out.
    #[test]
    fn the_samples_cover_every_code_of_the_error() {
        let sampled: BTreeSet<&str> = samples().iter().map(Error::code).collect();

        for code in Error::ALL_CODES {
            assert!(sampled.contains(code), "no sample has the code {code}");
        }
        assert!(sampled.contains("validation_internal"));
        assert!(sampled.contains("csv_invalid_amount"));
    }

    #[test]
    fn the_release_form_never_holds_text_or_an_amount_from_the_error() {
        for error in samples() {
            let line = release_form(&error);

            assert!(!line.contains(SENTINEL), "{error:?} wrote: {line}");
            assert!(
                !line.contains(&SENTINEL_AMOUNT.to_string()),
                "{error:?} wrote: {line}"
            );
        }
    }

    /// Guards the test above: were the sentinel not in the full text, its
    /// absence from the release form would show nothing.
    #[test]
    fn the_full_form_is_the_display_text_and_holds_what_the_release_form_drops() {
        let mut with_sentinel = 0;

        for error in samples() {
            let full = Rendered(&error, true).to_string();

            assert_eq!(full, error.to_string());
            if full.contains(SENTINEL) || full.contains(&SENTINEL_AMOUNT.to_string()) {
                with_sentinel += 1;
            }
        }
        assert_eq!(with_sentinel, 17);
    }

    #[test]
    fn the_release_form_starts_with_the_code() {
        for error in samples() {
            let line = release_form(&error);

            assert!(line.starts_with(error.code()), "{error:?} wrote: {line}");
        }
    }

    #[test]
    fn a_failure_below_the_crate_keeps_its_operation() {
        let error = Error::Io {
            operation: "rename vault file",
            detail: format!("/Users/someone/Documents/{SENTINEL}: denied"),
        };

        assert_eq!(release_form(&error), "io: rename vault file");
    }

    #[test]
    fn a_corrupt_vault_keeps_the_reason_without_its_text() {
        let unreadable = Error::VaultCorrupt(VaultCorruption::HeaderUnreadable {
            detail: SENTINEL.into(),
        });
        let column = Error::VaultCorrupt(VaultCorruption::Column {
            column: "accounts.id".into(),
            detail: SENTINEL.into(),
        });
        let missing = Error::VaultCorrupt(VaultCorruption::HeaderWithoutDatabase);

        assert_eq!(
            release_form(&unreadable),
            "vault_corrupt: vault header does not parse"
        );
        assert_eq!(
            release_form(&column),
            "vault_corrupt: accounts.id holds an unusable value"
        );
        assert_eq!(
            release_form(&missing),
            "vault_corrupt: vault header exists without database"
        );
    }

    #[test]
    fn a_rule_or_a_cell_keeps_only_its_code() {
        let taken = Error::Validation(ValidationError::Internal {
            detail: SENTINEL.into(),
        });
        let cell = Error::Csv(CsvError::InvalidDate(SENTINEL.into()));
        let unbalanced = Error::UnbalancedEntry {
            debits: SENTINEL_AMOUNT,
            credits: 2,
        };

        assert_eq!(release_form(&taken), "validation_internal");
        assert_eq!(release_form(&cell), "csv_invalid_date");
        assert_eq!(release_form(&unbalanced), "unbalanced_entry");
    }

    #[test]
    fn a_version_or_a_kind_of_record_is_kept() {
        let too_new = Error::VaultTooNew {
            found: 9,
            supported: 8,
        };

        assert_eq!(
            release_form(&too_new),
            "vault_too_new: vault version 9 is newer than this build supports (8)"
        );
        assert_eq!(
            release_form(&Error::NotFound(Resource::Account)),
            format!("not_found: {}", Error::NotFound(Resource::Account))
        );
    }

    /// A private detail formatted with the detail withheld or written.
    struct Private<'a>(&'a str, bool);

    impl Display for Private<'_> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write_private_detail(f, &self.0, self.1)
        }
    }

    /// No test of this crate calls `enable_log_detail`, so what `Display`
    /// writes here is what it writes in a release build.
    #[test]
    fn display_writes_the_reduced_form_until_detail_is_enabled() {
        let error = Error::Io {
            operation: "rename vault file",
            detail: SENTINEL.into(),
        };

        assert_eq!(error.log_text().to_string(), "io: rename vault file");
        assert_eq!(PrivateDetail(SENTINEL).to_string(), WITHHELD);
    }

    #[test]
    fn a_private_detail_is_withheld_in_the_release_form_only() {
        assert_eq!(Private(SENTINEL, false).to_string(), WITHHELD);
        assert_eq!(Private(SENTINEL, true).to_string(), SENTINEL);
    }
}
