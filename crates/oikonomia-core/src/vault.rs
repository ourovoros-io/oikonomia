//! Encrypted vault: an Argon2id-derived key over a `SQLCipher` database.
//!
//! A vault is two files that are only usable together: the database, every
//! page of it ciphertext, and a public header with the salt and the Argon2id
//! parameters that turn the master password into the database key. The
//! password and the key are never written to disk.
//!
//! No filesystem replaces two files in one step, so every operation that
//! changes the pair (first run, password change, restore) goes through a
//! temporary file whose presence tells the next open how far the operation
//! got. The table below is the complete set.
//!
//! # Files
//!
//! All of them live in the data directory ([`default_data_dir`]). The names
//! are defined in `vault/paths.rs` and nowhere else.
//!
//! | File | Holds | Left behind by a crash during | Settled by |
//! |------|-------|-------------------------------|------------|
//! | `vault.db` | The `SQLCipher` database. | | |
//! | `vault.db-wal`, `vault.db-shm` | `SQLite`'s write-ahead log and its index. Log pages are encrypted with the database key.[^wal] | A session: the log can hold commits that are not in `vault.db` yet. | The next session: [`Vault::unlock`] reads the log, and `SQLite` folds it into `vault.db` when the session closes.[^checkpoint] A locked backup refuses the vault until then. |
//! | `vault.header.json` | The public header ([`VaultHeader`]). A vault exists exactly when this file does. | | |
//! | `vault.header.json.init` | The header of a vault that is being created. | [`Vault::init`], before the header is published. The database beside it holds only the schema. | [`Vault::open_path`] removes it and the database; the vault reads as uninitialized again. |
//! | `vault.header.json.tmp` | The next header during a password change. | [`Vault::change_password`]. If the rekey ran, this header is the one whose key fits. | [`Vault::unlock`]: when the published header's key does not fit, it tries this one and publishes it. A stale one is removed by the next successful unlock. |
//! | `vault.header.json.restore-tmp`, `vault.db.restore-tmp` | A backup archive as unpacked, not yet checked. | A restore, before any live file is touched. | [`Vault::open_path`] removes them. |
//! | `vault.header.json.restore-new` | The restore marker: the unpacked header after its checks. It exists exactly while the live pair is being swapped. | A restore, mid-swap. | [`Vault::open_path`] undoes the swap; the previous vault is back. |
//! | `vault.header.json.restore-old`, `vault.db.restore-old`, `vault.db-wal.restore-old` | The previous live files, set aside during the swap. | A restore, mid-swap or just after it committed. | [`Vault::open_path`] renames them back while the marker exists, and removes them when it is gone. |
//! | `vault.db.backup-tmp` | The snapshot an online backup packs into the archive. | [`Vault::backup_to`] on an unlocked vault. | The next online backup removes it before writing its own. Nothing else does: until then a second copy of the ciphertext stays in the data directory. |
//!
//! A backup archive and a CSV export are written outside the data
//! directory, as the destination plus `.tmp`, and renamed into place. A crash
//! leaves that sibling next to the destination the user chose; the next
//! write to the same destination truncates it.
//!
//! [^wal]: <https://www.zetetic.net/sqlcipher/design/>, "Write Ahead Log
//!     Files".
//!
//! [^checkpoint]: <https://www.sqlite.org/wal.html>, "Avoiding Excessively
//!     Large WAL Files": the last connection to close does a final checkpoint
//!     and deletes the log.
//!
//! The restore steps and the rules that settle them are in the module doc of
//! `vault/backup.rs`, next to the archive format.
//!
//! # Lock states
//!
//! A [`Vault`] is in one of the three [`VaultStatus`] states, derived from
//! what it holds and never stored:
//!
//! | State | Holds | Reached by |
//! |-------|-------|------------|
//! | `Uninitialized` | No header, no connection. | [`Vault::open_path`] on a directory with no vault. |
//! | `Locked` | The header. | [`Vault::open_path`] on a directory with a vault, [`Vault::lock`], [`Vault::restore_from`]. |
//! | `Unlocked` | The header and an open connection. | [`Vault::init`], [`Vault::unlock`]. |
//!
//! [`Vault::change_password`] and [`Vault::backup_to`] keep the state they
//! find; the cases where a failed password change ends locked are listed on
//! that method. The key itself is not held: it is derived, handed to
//! `SQLCipher`, and wiped. What the process cannot wipe is stated on
//! [`Vault::lock`].
//!
//! # Invariants and where they are enforced
//!
//! - **Encrypted at rest.** Every vault connection is opened by
//!   `open_sqlcipher` in `vault/store.rs`, which sets the key before any
//!   other statement. A backup checks that the snapshot it made is not a
//!   plaintext `SQLite` file, and a restore refuses an archive whose database
//!   is one (`vault/backup.rs`).
//! - **Owner-only files.** Every file and the data directory are created
//!   through `vault/permissions.rs` (`0600` and `0700` on Unix), and
//!   [`Vault::open_path`] tightens a vault that predates the rule.
//! - **Header and database match.** The three protocols above: the header is
//!   published last on first run, staged around the rekey on a password
//!   change, and swapped behind a marker on restore.
//! - **A header this build cannot use is rejected, not guessed at.** The
//!   format version is checked when the header is loaded
//!   (`vault/header.rs`), and the key-derivation parameters are bounded
//!   before Argon2 runs (`vault/crypto.rs`), so a tampered header cannot ask
//!   for gigabytes of memory.
//! - **Minimum password length.** Checked in `vault/store.rs` wherever a
//!   password is set: [`Vault::init`] and [`Vault::change_password`].

mod backup;
mod crypto;
pub(crate) mod files;
mod header;
mod paths;
mod permissions;
mod store;

pub use backup::{BACKUP_EXTENSION, backup_to_path, default_backup_file_name, restore_from_path};
pub use header::VaultHeader;
pub use paths::{default_data_dir, vault_db_path, vault_header_path};
pub use store::{Vault, VaultStatus};
