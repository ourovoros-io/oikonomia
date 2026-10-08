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
//! got.
//!
//! # Files
//!
//! All of them live in the data directory ([`default_data_dir`]). The names
//! are defined in `vault/paths.rs` and nowhere else. This is the complete
//! set:
//!
//! | File                            | Holds                                   |
//! |---------------------------------|-----------------------------------------|
//! | `vault.db`                      | The `SQLCipher` database.               |
//! | `vault.db-wal`                  | `SQLite`'s write-ahead log.[^wal]       |
//! | `vault.db-shm`                  | `SQLite`'s index into that log.         |
//! | `vault.header.json`             | The public header ([`VaultHeader`]).    |
//! | `vault.header.json.init`        | Header of a vault that is being made.   |
//! | `vault.header.json.tmp`         | Next header during a password change.   |
//! | `vault.header.json.restore-tmp` | Archive header as unpacked, unchecked.  |
//! | `vault.db.restore-tmp`          | Archive database as unpacked.           |
//! | `vault.header.json.restore-new` | The restore marker: the checked header. |
//! | `vault.header.json.restore-old` | Previous header, set aside by a restore.|
//! | `vault.db.restore-old`          | Previous database, set aside likewise.  |
//! | `vault.db-wal.restore-old`      | Previous log, set aside likewise.       |
//! | `vault.db.backup-tmp`           | Snapshot that an online backup packs.   |
//!
//! A vault exists exactly when `vault.header.json` does. The restore marker
//! exists exactly while a restore is swapping the live pair.
//!
//! [^wal]: Its pages are encrypted with the database key
//!     (<https://www.zetetic.net/sqlcipher/design/>, "Write Ahead Log Files").
//!
//! # What a crash leaves, and what settles it
//!
//! | Leftover              | Crash during                   | Settled by           |
//! |-----------------------|--------------------------------|----------------------|
//! | `vault.db-wal`        | A session.                     | The next session.    |
//! | `…header.json.init`   | [`Vault::init`].               | [`Vault::open_path`] |
//! | `…header.json.tmp`    | [`Vault::change_password`].    | [`Vault::unlock`]    |
//! | `….restore-tmp`       | A restore, while unpacking.    | [`Vault::open_path`] |
//! | `….restore-new`       | A restore, mid-swap.           | [`Vault::open_path`] |
//! | `….restore-old`       | A restore, mid-swap or after.  | [`Vault::open_path`] |
//! | `vault.db.backup-tmp` | [`Vault::backup_to`], unlocked.| [`Vault::open_path`] |
//!
//! - **Write-ahead log.** It can hold commits that are not in `vault.db`
//!   yet. Unlocking reads it, and `SQLite` folds it into `vault.db` when the
//!   session closes.[^checkpoint] A locked backup refuses the vault until
//!   then.
//! - **`.init`.** The header is published last on first run, so the database
//!   beside a lone `.init` holds only the schema. Both are removed and the
//!   vault reads as uninitialized again.
//! - **`.tmp`.** If the rekey ran, this header is the one whose key fits.
//!   When the published header's key does not fit, unlock tries this one and
//!   publishes it. A stale one is removed by the next successful unlock.
//!   Which of the two it is cannot be told without the password, so a
//!   locked backup refuses the vault until that unlock.
//! - **`restore-tmp`.** Without the marker no live file was touched yet, and
//!   they are removed. The unpacked database keeps this name during the
//!   swap as well, until it becomes `vault.db`; with the marker present it
//!   is part of the undo below.
//! - **`restore-new` and `restore-old`.** While the marker exists the swap
//!   did not commit: the `restore-old` files are renamed back and the
//!   previous vault is whole again. Once the marker is gone the swap
//!   committed, and the `restore-old` files are removed.
//! - **`backup-tmp`.** A second copy of the ciphertext, of no use once the
//!   backup that was packing it is gone. It is removed on the next open, and
//!   an online backup removes one it finds before writing its own.
//!
//! [^checkpoint]: <https://www.sqlite.org/wal.html>, "Avoiding Excessively
//!     Large WAL Files": the last connection to close does a final checkpoint
//!     and deletes the log.
//!
//! A backup archive and a CSV export are written outside the data
//! directory, as the destination plus `.tmp`, and renamed into place. A crash
//! leaves that sibling next to the destination the user chose; the next
//! write to the same destination truncates it.
//!
//! The restore steps and the rules that settle them are in the module doc of
//! `vault/backup.rs`, next to the archive format.
//!
//! # Lock states
//!
//! A [`Vault`] is in one of the three [`VaultStatus`] states, derived from
//! what it holds and never stored:
//!
//! | State           | Holds                       | Reached by               |
//! |-----------------|-----------------------------|--------------------------|
//! | `Uninitialized` | Nothing.                    | [`Vault::open_path`]     |
//! | `Locked`        | The header.                 | [`Vault::open_path`]     |
//! |                 |                             | [`Vault::lock`]          |
//! |                 |                             | [`Vault::restore_from`]  |
//! | `Unlocked`      | The header and a connection.| [`Vault::init`]          |
//! |                 |                             | [`Vault::unlock`]        |
//!
//! [`Vault::open_path`] gives `Locked` when the directory holds a vault and
//! `Uninitialized` when it does not. [`Vault::lock`] changes nothing on a
//! vault that is not unlocked.
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
//!   statement that reads the database. A backup checks that the snapshot it
//!   made is not a plaintext `SQLite` file, and a restore refuses an archive
//!   whose database is one (`vault/backup.rs`).
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
//!   for more than 1 GiB of memory.
//! - **Minimum password length.** Checked in `vault/store.rs` wherever a
//!   password is set: [`Vault::init`] and [`Vault::change_password`].

mod backup;
mod crypto;
pub(crate) mod files;
mod header;
mod paths;
pub(crate) mod permissions;
mod store;

pub use backup::{
    BACKUP_EXTENSION, backup_to_path, default_backup_file_name, restore_from_path,
    verify_backup_archive,
};
pub use header::VaultHeader;
pub use paths::{default_data_dir, vault_db_path, vault_header_path};
/// The open database handle that [`Vault::connection`] lends out.
///
/// Re-exported so a caller can name the type without depending on `rusqlite`
/// itself.
pub use rusqlite::Connection;
pub use store::{Vault, VaultStatus};
