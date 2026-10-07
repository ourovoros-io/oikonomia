//! The ways stored data can be unusable: a damaged vault, a bad backup file.
//!
//! [`Error::VaultCorrupt`](crate::Error::VaultCorrupt) and
//! [`Error::BackupInvalid`](crate::Error::BackupInvalid) each carry one of
//! these enums in place of a sentence. The set of reasons is small and fixed,
//! so a test or a caller can match the reason itself and not a substring of
//! its message. The `Display` text is English diagnostic text for logs.
//!
//! Neither reason is sent to the UI as a parameter: the copy for each code
//! is one sentence, and what exactly is wrong helps a person reading a log,
//! not the user. A [`VaultCorruption`] does choose the code, though: most
//! reasons are `vault_corrupt`, and the two refusals of a locked backup that
//! one unlock settles are `vault_unlock_before_backup`.

use thiserror::Error;

/// Why stored vault data cannot be used.
///
/// The enum is matched exhaustively by the code that words it; see the
/// [module documentation](crate::error) for the policy.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VaultCorruption {
    /// A stored value is not one the application writes there.
    #[error("{column}: {detail}")]
    Column {
        /// Where the value was read: `table.column`, or the column's name in
        /// the query when the driver refused the value.
        column: String,
        /// What is wrong with the value.
        detail: String,
    },

    /// The header file is not a vault header.
    #[error("vault header does not parse: {detail}")]
    HeaderUnreadable {
        /// The decoder's own text.
        detail: String,
    },

    /// A field of the header holds a value no build writes.
    #[error("vault header has an unusable {field}: {detail}")]
    HeaderField {
        /// The field, in words: `salt`, `key derivation function`.
        field: &'static str,
        /// What is wrong with it.
        detail: String,
    },

    /// The header names a format version this build does not read.
    ///
    /// For the header of the vault in place this is only version 0, which
    /// no build writes: a version above the supported one is
    /// [`Error::VaultTooNew`](crate::Error::VaultTooNew). The header inside
    /// a backup is reported with this reason for either, wrapped in
    /// [`BackupDefect::UnusableHeader`].
    #[error("unsupported vault format {version}")]
    UnsupportedFormat {
        /// The format version the header names.
        version: u32,
    },

    /// The database decrypts but has no `vault_meta` table.
    #[error("vault metadata table is missing")]
    MissingMetaTable,

    /// The `vault_meta` table has no row to read the schema version from.
    #[error("vault metadata has no schema version")]
    MissingSchemaVersion,

    /// The header file exists and the database file does not.
    #[error("vault header exists without database")]
    HeaderWithoutDatabase,

    /// The database file exists and the header file does not.
    #[error("vault database exists without header")]
    DatabaseWithoutHeader,

    /// A vault file exists and holds nothing.
    #[error("{file} is empty")]
    EmptyFile {
        /// The file's name, such as `vault.db`.
        file: &'static str,
    },

    /// The write-ahead log beside a locked vault holds pages that a copy of
    /// the database file would leave out.
    #[error(
        "vault database has changes still in its write-ahead log; \
         unlock the vault once before backing up"
    )]
    UnmergedWriteAheadLog,

    /// The header a password change staged is still beside a locked vault.
    /// Either it or the published header fits the database, and only the
    /// password tells which, so a copy of the two vault files may be a
    /// backup that no password opens.
    #[error("a password change did not finish; unlock the vault once before backing up")]
    UnfinishedPasswordChange,

    /// A stored application setting is not a value the application writes.
    #[error("stored setting {key} is not valid")]
    Setting {
        /// The key of the setting in `app_settings`.
        key: &'static str,
    },

    /// Journal lines break the rule that a line is a debit or a credit and
    /// never both, so the migration that adds the rule as a constraint
    /// cannot run.
    #[error("{count} journal line(s) are not debit xor credit")]
    InvalidJournalLines {
        /// How many lines break the rule.
        count: i64,
    },
}

impl VaultCorruption {
    /// Returns the code [`Error::code`](crate::Error::code) gives a corrupt
    /// vault with this reason.
    ///
    /// [`Self::UnmergedWriteAheadLog`] and [`Self::UnfinishedPasswordChange`]
    /// are `vault_unlock_before_backup`. Only the backup of a locked vault
    /// returns them, the vault is sound, and one unlock settles both, so
    /// calling them corrupt would send the user after a repair they do not
    /// need. Every other reason is `vault_corrupt`.
    ///
    /// The match has no wildcard arm, so a new reason does not compile until
    /// its code is decided here. A new code also goes in
    /// [`Error::ALL_CODES`](crate::Error::ALL_CODES).
    #[must_use]
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::UnmergedWriteAheadLog | Self::UnfinishedPasswordChange => {
                "vault_unlock_before_backup"
            }
            Self::Column { .. }
            | Self::HeaderUnreadable { .. }
            | Self::HeaderField { .. }
            | Self::UnsupportedFormat { .. }
            | Self::MissingMetaTable
            | Self::MissingSchemaVersion
            | Self::HeaderWithoutDatabase
            | Self::DatabaseWithoutHeader
            | Self::EmptyFile { .. }
            | Self::Setting { .. }
            | Self::InvalidJournalLines { .. } => "vault_corrupt",
        }
    }
}

/// Why a file is not a usable Oikonomia backup.
///
/// The enum is matched exhaustively by the code that words it; see the
/// [module documentation](crate::error) for the policy.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackupDefect {
    /// The file does not start with the backup magic.
    #[error("not an Oikonomia vault backup")]
    NotABackup,

    /// The archive was written in a format this build does not read.
    #[error("unsupported backup version {version}")]
    UnsupportedVersion {
        /// The format version the archive names.
        version: u16,
    },

    /// The file ends before the archive does.
    #[error("backup is truncated")]
    Truncated,

    /// The file goes on after the archive ends.
    #[error("backup has trailing data")]
    TrailingData,

    /// A member has no contents.
    #[error("{name} is empty")]
    EmptyMember {
        /// The member's name as the archive gives it.
        name: String,
    },

    /// A member appears twice.
    #[error("backup has duplicate {name}")]
    DuplicateMember {
        /// The member's name.
        name: &'static str,
    },

    /// A member is neither the header nor the database, or its name is not
    /// a plain file name.
    #[error("unexpected member {name}")]
    UnexpectedMember {
        /// The member's name as the archive gives it.
        name: String,
    },

    /// The header or the database is not in the archive.
    #[error("backup is missing {name}")]
    MissingMember {
        /// The member's name.
        name: &'static str,
    },

    /// A member's name is empty or longer than the format allows.
    #[error("invalid member name length")]
    MemberNameLength,

    /// A member's name is not UTF-8.
    #[error("member name is not UTF-8")]
    MemberNameNotUtf8,

    /// The header in the archive is one the vault would call corrupt.
    #[error("vault header in the backup is not usable: {0}")]
    UnusableHeader(VaultCorruption),

    /// The database in the archive is a plaintext `SQLite` database.
    #[error("database in the backup is not encrypted")]
    DatabaseNotEncrypted,
}

#[cfg(test)]
mod tests {
    use super::{BackupDefect, VaultCorruption};
    use crate::Error;
    use oikonomia_test_support::listed_variants;
    use std::fmt::Display;

    listed_variants! {
        patterns listed_corruptions for VaultCorruption {
            VaultCorruption::Column { .. },
            VaultCorruption::HeaderUnreadable { .. },
            VaultCorruption::HeaderField { .. },
            VaultCorruption::UnsupportedFormat { .. },
            VaultCorruption::MissingMetaTable,
            VaultCorruption::MissingSchemaVersion,
            VaultCorruption::HeaderWithoutDatabase,
            VaultCorruption::DatabaseWithoutHeader,
            VaultCorruption::EmptyFile { .. },
            VaultCorruption::UnmergedWriteAheadLog,
            VaultCorruption::UnfinishedPasswordChange,
            VaultCorruption::Setting { .. },
            VaultCorruption::InvalidJournalLines { .. },
        }
    }

    /// One value of every reason, in declaration order.
    fn every_corruption() -> Vec<VaultCorruption> {
        vec![
            VaultCorruption::Column {
                column: "accounts.id".into(),
                detail: "not an id".into(),
            },
            VaultCorruption::HeaderUnreadable {
                detail: "expected value".into(),
            },
            VaultCorruption::HeaderField {
                field: "salt",
                detail: "not base64".into(),
            },
            VaultCorruption::UnsupportedFormat { version: 2 },
            VaultCorruption::MissingMetaTable,
            VaultCorruption::MissingSchemaVersion,
            VaultCorruption::HeaderWithoutDatabase,
            VaultCorruption::DatabaseWithoutHeader,
            VaultCorruption::EmptyFile { file: "vault.db" },
            VaultCorruption::UnmergedWriteAheadLog,
            VaultCorruption::UnfinishedPasswordChange,
            VaultCorruption::Setting {
                key: "lock_timeout_secs",
            },
            VaultCorruption::InvalidJournalLines { count: 1 },
        ]
    }

    listed_variants! {
        patterns listed_defects for BackupDefect {
            BackupDefect::NotABackup,
            BackupDefect::UnsupportedVersion { .. },
            BackupDefect::Truncated,
            BackupDefect::TrailingData,
            BackupDefect::EmptyMember { .. },
            BackupDefect::DuplicateMember { .. },
            BackupDefect::UnexpectedMember { .. },
            BackupDefect::MissingMember { .. },
            BackupDefect::MemberNameLength,
            BackupDefect::MemberNameNotUtf8,
            BackupDefect::UnusableHeader(_),
            BackupDefect::DatabaseNotEncrypted,
        }
    }

    /// One value of every defect, in declaration order.
    fn every_defect() -> Vec<BackupDefect> {
        vec![
            BackupDefect::NotABackup,
            BackupDefect::UnsupportedVersion { version: 99 },
            BackupDefect::Truncated,
            BackupDefect::TrailingData,
            BackupDefect::EmptyMember {
                name: "vault.db".into(),
            },
            BackupDefect::DuplicateMember { name: "vault.db" },
            BackupDefect::UnexpectedMember {
                name: "notes.txt".into(),
            },
            BackupDefect::MissingMember { name: "vault.db" },
            BackupDefect::MemberNameLength,
            BackupDefect::MemberNameNotUtf8,
            BackupDefect::UnusableHeader(VaultCorruption::MissingMetaTable),
            BackupDefect::DatabaseNotEncrypted,
        ]
    }

    /// Fails unless `message` starts in lowercase and has no trailing period.
    #[track_caller]
    fn assert_message_style(reason: &impl Display) {
        let message = reason.to_string();

        assert!(
            message
                .chars()
                .next()
                .is_some_and(|first| !first.is_uppercase()),
            "{message:?} must not be empty or start with a capital"
        );
        assert!(
            !message.ends_with('.'),
            "{message:?} must not end with a period"
        );
    }

    /// The compiler checks the two lists above against the enums with an
    /// exhaustive `match`; this checks that each has a sample for every
    /// variant, and the style of every sample's message.
    #[test]
    fn every_reason_has_a_lowercase_message_without_a_trailing_period() {
        let corruptions = every_corruption();
        let defects = every_defect();

        listed_corruptions::assert_every_position_once(
            corruptions
                .iter()
                .map(listed_corruptions::position)
                .collect(),
        );
        listed_defects::assert_every_position_once(
            defects.iter().map(listed_defects::position).collect(),
        );
        corruptions.iter().for_each(assert_message_style);
        defects.iter().for_each(assert_message_style);
    }

    #[test]
    fn every_reason_gives_a_code_the_error_lists() {
        for reason in every_corruption() {
            assert!(
                Error::ALL_CODES.contains(&reason.code()),
                "{reason:?} gives {}, which Error::ALL_CODES does not list",
                reason.code()
            );
        }
    }

    #[test]
    fn a_damaged_column_reads_as_its_name_and_what_is_wrong() {
        let reason = VaultCorruption::Column {
            column: "accounts.id".into(),
            detail: "not an id: nope".into(),
        };

        assert_eq!(reason.to_string(), "accounts.id: not an id: nope");
    }
}
