//! Portable backup of the `SQLCipher` ciphertext plus public vault header.
//!
//! The archive is not a second encryption layer: it stores `vault.db` and
//! `vault.header.json` as ciphertext. The master password is never stored.
//! `vault.header.json.tmp`, the header staged by a password change, is never
//! packed.
//!
//! An unlocked vault is snapshotted with `VACUUM INTO` so WAL is folded
//! without closing the session. A locked vault is copied file by file, and
//! that copy is a usable backup only when the two files are the whole vault
//! and belong together. Two things a crash leaves break that, and a locked
//! backup refuses the vault while either is there:
//!
//! | Leftover                  | Crash during       | Why a file copy is wrong          |
//! |---------------------------|--------------------|-----------------------------------|
//! | `vault.db-wal`, not empty | A session.         | Commits in the log are left out.  |
//! | `vault.header.json.tmp`   | A password change. | The published header may not fit. |
//!
//! - **Write-ahead log.** The database runs in WAL mode, and after a crash
//!   committed transactions can still sit in the log. Unlocking reads it,
//!   and `SQLite` folds it into `vault.db` when the last connection closes
//!   (<https://www.sqlite.org/wal.html>, "Avoiding Excessively Large WAL
//!   Files").
//! - **Staged header.** A password change writes it before the rekey and
//!   renames it over the published header after. If the process died
//!   between the two, the staged header is the one whose key fits the
//!   database, and an archive with the published one opens with no
//!   password. If it died before the rekey, the published header still
//!   fits. Telling the two cases apart takes the key, which a locked backup
//!   does not have, so both are refused. A successful unlock settles
//!   either: it publishes the staged header when that is the one that fits
//!   and removes it otherwise.
//!
//! Both refusals end the same way: unlock the vault once, then back up. The
//! backup of an unlocked vault checks neither. Its snapshot includes the
//! log, and the unlock it follows has already settled the header.
//!
//! # Format
//!
//! All integers are little-endian. There is no padding and no alignment.
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0 | 8 | Magic, the ASCII bytes `OIKOBACK`. |
//! | 8 | 2 | Format version, `u16`. This crate writes and reads only `1`. |
//! | 10 | | First member. |
//! | | | Second member, directly after the first. |
//!
//! A member is:
//!
//! | Size | Field |
//! |------|-------|
//! | 2 | Length of the name in bytes, `u16`. |
//! | name length | Name, UTF-8, not terminated. |
//! | 8 | Length of the contents in bytes, `u64`. |
//! | contents length | Contents: the bytes of the file, unchanged. |
//!
//! The file ends with the last byte of the second member. There is no member
//! count, no index, no trailer and no checksum.
//!
//! The writer emits exactly two members, `vault.header.json` and then
//! `vault.db`. The reader is stricter than the layout and looser than the
//! writer in one respect only, the order:
//!
//! - It accepts the two members in either order, each exactly once. A
//!   missing member, a repeated one, or any other name is rejected.
//! - A name length of 0 or above 255 is rejected, as is a name that is not
//!   UTF-8 or contains `/`, `\`, a NUL or `..`. Names are compared with the
//!   two known ones and never used as paths.
//! - A contents length of 0 is rejected.
//! - A file that ends inside a field or inside contents is rejected as
//!   truncated, and so is one with any byte after the second member.
//!
//! Because nothing is checksummed, a changed byte inside a member is not
//! noticed by the restore itself. The restore only checks that the header
//! parses as a header of a known format and that the database does not
//! start with the magic of a plaintext `SQLite` file. Damage to the database
//! surfaces afterwards: `SQLCipher` stores a MAC with every page and checks
//! it when the page is read (<https://www.zetetic.net/sqlcipher/design/>).
//! Damage to a header that still parses surfaces at unlock, as a corrupt
//! vault when a parameter is out of range and as a wrong password when the
//! salt or a cost changed.
//!
//! # Restore protocol
//!
//! A restore replaces two files that are only usable together, and no
//! filesystem renames two files atomically, so the swap is a sequence that
//! [`Vault::open_path`] can settle from whatever files a crash left:
//!
//! 1. Unpack the archive to `vault.header.json.restore-tmp` and
//!    `vault.db.restore-tmp`, then check them: the header must parse as a
//!    header of a known format, and the database must not be plaintext
//!    `SQLite`. Nothing live has been touched; a failure removes the two files.
//! 2. Remove the live database's `-shm` index. `SQLite` rebuilds it from the
//!    log, so nothing is lost if the swap is undone.
//! 3. Rename the unpacked header to `vault.header.json.restore-new`. This
//!    file exists exactly while the swap is in progress.
//! 4. Rename the live header, write-ahead log and database, where present,
//!    to `vault.header.json.restore-old`, `vault.db-wal.restore-old` and
//!    `vault.db.restore-old`. The log moves with its database: `SQLite` would
//!    replay a log left next to a different database into it, and removing
//!    it would drop commits a crashed session left there.
//! 5. Rename `vault.db.restore-tmp` to `vault.db`.
//! 6. Rename `vault.header.json.restore-new` to `vault.header.json`. The
//!    restore is committed.
//! 7. Remove the three `restore-old` files.
//!
//! An error in steps 3 to 6 is undone on the spot, and a crash is settled on
//! the next open, by the same rules:
//!
//! - `restore-new` exists: the swap did not commit, so it is undone. If
//!   `vault.db.restore-tmp` is gone, step 5 ran and `vault.db` is the new
//!   database; it is renamed back first, so that this test stays true however
//!   often the undo itself is interrupted. Then the `restore-old` files are
//!   renamed back, `restore-new` is removed, and last the unpacked files.
//!   For the same reason `vault.db.restore-tmp` is never removed while
//!   `restore-new` exists.
//! - `restore-new` is gone but a `restore-old` file exists: step 6 ran, so
//!   the restore is finished by removing them.
//! - Only `restore-tmp` files exist: step 1 was interrupted; they are removed.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use rusqlite::Connection;

use crate::error::{BackupDefect, DatabaseContext, Error, IoContext, Result, VaultCorruption};
use crate::vault::files::{
    discard_database_files, discard_file, local_iso_date, remove_files_if_present,
    rename_if_present, rename_synced, sibling_path, sync_parent_dir,
};
use crate::vault::header::VaultHeader;
use crate::vault::paths::{
    RestorePaths, STAGED_SUFFIX, backup_snapshot_db_path, db_sidecar_paths, vault_db_path,
    vault_header_path, vault_init_header_path, vault_staged_header_path,
};
use crate::vault::permissions::{create_private_dir, create_private_file};
use crate::vault::store::Vault;

/// File extension for portable vault backups (no leading dot).
pub const BACKUP_EXTENSION: &str = "oikonomia-backup";

/// Magic at the start of an archive. Identifies the file; holds no secret.
pub(super) const MAGIC: &[u8; 8] = b"OIKOBACK";

/// Archive format version this crate writes, and the only one it reads.
pub(super) const FORMAT_VERSION: u16 = 1;

/// Archive member name of the database.
///
/// Part of the archive format. It equals the file name in the data directory
/// today, and has to keep this value if that file is ever renamed.
const MEMBER_DB: &str = "vault.db";
/// Archive member name of the header, fixed by the format like [`MEMBER_DB`].
const MEMBER_HEADER: &str = "vault.header.json";

/// Longest member name the reader accepts, in bytes.
const MAX_MEMBER_NAME_LEN: u16 = 255;

/// Size of the buffer members are copied through, in bytes.
const COPY_BUFFER_LEN: usize = 16_384;

/// Returns the default file name for a backup,
/// `oikonomia-backup-YYYY-MM-DD.oikonomia-backup`.
///
/// The date is the local one where it can be read and the UTC one otherwise,
/// and is written year first so that backups sort by date.
#[must_use]
pub fn default_backup_file_name() -> String {
    format!("oikonomia-backup-{}.{BACKUP_EXTENSION}", local_iso_date())
}

/// Writes an archive of a locked vault's two files to `dest`, creating the
/// parent directory of `dest` when it is missing.
///
/// Copies `vault.header.json` and `vault.db` as they are on disk, so it is
/// for a vault with no open connection; [`Vault::backup_to`] handles an
/// unlocked one. The archive is written as `dest` + `.tmp`, readable only by
/// its owner, and renamed into place, so `dest` never holds half an archive.
///
/// # Errors
///
/// - [`Error::VaultUninitialized`] when neither vault file exists.
/// - [`Error::VaultCorrupt`] when only one of the two files exists
///   ([`VaultCorruption::HeaderWithoutDatabase`],
///   [`VaultCorruption::DatabaseWithoutHeader`]), when either is empty
///   ([`VaultCorruption::EmptyFile`]), when `vault.db-wal` holds pages a
///   file copy would leave out ([`VaultCorruption::UnmergedWriteAheadLog`]),
///   or when a password change left its staged header behind
///   ([`VaultCorruption::UnfinishedPasswordChange`]). The last two are
///   settled by unlocking the vault once; see the module doc.
/// - [`Error::Io`] when `dest` has no file name, when a file cannot be
///   inspected, read, created, written or renamed, or when a vault file
///   becomes shorter while it is being copied.
pub fn backup_to_path(data_dir: &Path, dest: &Path) -> Result<()> {
    let header_path = vault_header_path(data_dir);
    let db_path = vault_db_path(data_dir);
    ensure_vault_files(&header_path, &db_path)?;
    ensure_no_unmerged_wal(&db_path)?;
    ensure_no_staged_header(data_dir)?;
    write_archive_from_paths(&header_path, &db_path, dest)
}

/// Unpacks a backup archive into `data_dir`, creating the directory when it
/// is missing and restricting it to its owner.
///
/// No password is needed: the members are ciphertext and the public header.
/// Existing `vault.db` / `vault.header.json` are left untouched unless
/// `replace` is true. `replace` is required only when a vault already
/// exists; an uninitialized data directory accepts `replace: false`.
///
/// The caller must hold no open connection to the vault in `data_dir`;
/// [`Vault::restore_from`] locks first for that reason.
///
/// The archive is unpacked next to the vault and checked before any live
/// file is touched, and the two files are swapped by the restore protocol in
/// the module doc, so a failure or a crash leaves either the previous vault
/// or the restored one, never one's header with the other's database. The
/// previous vault's WAL/SHM sidecars and `vault.header.json.tmp` do not
/// survive a completed replace, so unlock cannot mix old recovery state with
/// restored files.
///
/// # Errors
///
/// - [`Error::BackupInvalid`] when `archive` does not follow the format in
///   the module doc, its header member is not a vault header of a known
///   format, or its database member is a plaintext `SQLite` file. This is
///   checked first, so it is returned even when `replace` is false and a
///   vault exists.
/// - [`Error::RestoreWouldOverwrite`] when a vault file exists and `replace`
///   is false.
/// - [`Error::Io`] when `archive` cannot be opened or read, the directory
///   cannot be created, or a vault file cannot be written, renamed or
///   removed. When putting the previous files back failed as well, the
///   message says so, and the next [`Vault::open_path`] completes the undo.
pub fn restore_from_path(archive: &Path, data_dir: &Path, replace: bool) -> Result<()> {
    restore_pair(archive, data_dir, replace).map(|_header| ())
}

/// Does what [`restore_from_path`] documents and returns the header that is
/// now published, as it was checked before the swap.
///
/// Nothing after the swap commits can fail: the cleanup that follows only
/// logs. `Ok` therefore means the vault files are the archive's, and `Err`
/// means they are not.
fn restore_pair(archive: &Path, data_dir: &Path, replace: bool) -> Result<VaultHeader> {
    create_private_dir(data_dir)?;
    recover_interrupted_restore(data_dir)?;
    let paths = RestorePaths::new(data_dir);

    // Unpack and check first, so a bad archive is rejected even when a vault
    // already exists and `replace` is false.
    let restored = unpack_and_verify(archive, &paths).and_then(|header| {
        refuse_overwrite(&paths, replace)?;
        swap_in_unpacked_pair(&paths)?;
        Ok(header)
    });
    let header = match restored {
        Ok(header) => header,
        Err(err) => {
            discard_unpacked_unless_swap_pending(&paths);
            return Err(err);
        }
    };

    // Recovery state of the vault that was replaced.
    discard_file(&vault_staged_header_path(data_dir));
    discard_file(&vault_init_header_path(data_dir));
    Ok(header)
}

/// Settles a restore that a crash interrupted, by the rules in the module
/// doc.
///
/// Undoes a swap that did not commit, finishes one that did, and removes a
/// half-unpacked archive. Does nothing when no restore files exist.
///
/// # Errors
///
/// [`Error::Io`] when a file cannot be renamed or removed; the files are
/// left for the next attempt.
pub(crate) fn recover_interrupted_restore(data_dir: &Path) -> Result<()> {
    let paths = RestorePaths::new(data_dir);

    if paths.verified_header.exists() {
        undo_swap(&paths)?;
    } else {
        remove_files_if_present(&[&paths.old_header, &paths.old_db, &paths.old_wal])?;
    }
    remove_files_if_present(&[&paths.unpacked_header, &paths.unpacked_db])
}

/// Removes the database snapshot an online backup left behind when the
/// process died before the backup removed it.
///
/// The snapshot is a full copy of the ciphertext, and nothing reads it
/// again: each online backup makes its own. A snapshot that is not there is
/// the usual case and not an error.
///
/// The names `SQLite` would use for a write-ahead log and its index beside
/// the snapshot are removed with it, as the backup's own cleanup does.
///
/// Must not run while another handle on the same directory is writing an
/// online backup: that backup can then fail with [`Error::Io`] for its
/// missing snapshot, or this call can fail because the snapshot is open.
/// A failed backup leaves no archive and can be repeated.
///
/// # Errors
///
/// [`Error::Io`] when one of these files exists and cannot be removed.
pub(crate) fn remove_stale_snapshot(data_dir: &Path) -> Result<()> {
    let snapshot = backup_snapshot_db_path(data_dir);
    let [wal, shm] = db_sidecar_paths(&snapshot);
    remove_files_if_present(&[&snapshot, &wal, &shm])
}

impl Vault {
    /// Writes a backup archive of this vault to `dest` without changing its
    /// lock state, creating the parent directory of `dest` when it is missing.
    ///
    /// Unlocked: the database is snapshotted with `VACUUM INTO` through the
    /// open connection, so the session stays open and its write-ahead log is
    /// included, and the snapshot is packed with `vault.header.json`. The
    /// snapshot is encrypted under the same key as the vault. Locked: the
    /// two files are copied as [`backup_to_path`] does.
    ///
    /// # Errors
    ///
    /// - [`Error::VaultUninitialized`] when no vault exists.
    /// - [`Error::VaultCorrupt`] when the header file is missing or empty,
    ///   and, for a locked vault, in the other cases [`backup_to_path`]
    ///   lists.
    /// - [`Error::Io`] when `dest` has no file name, when a file cannot be
    ///   read, created, written or renamed, or when a file becomes shorter
    ///   while it is being copied. For an unlocked vault also when the data
    ///   directory path is not UTF-8.
    /// - [`Error::Database`] when the snapshot of an unlocked vault fails.
    /// - [`Error::Crypto`] when that snapshot comes out as a plaintext
    ///   database; no archive is written then.
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        match self.connection() {
            Ok(conn) => backup_from_open_connection(conn, self.data_dir(), dest),
            Err(Error::VaultLocked) => backup_to_path(self.data_dir(), dest),
            Err(other) => Err(other),
        }
    }

    /// Closes the connection, unpacks `archive` into the data directory and
    /// takes over the restored header, leaving the vault locked.
    ///
    /// The connection is closed before anything is checked, so the vault is
    /// not unlocked afterwards whether the restore succeeded or not.
    ///
    /// What the handle holds after each outcome:
    ///
    /// | Outcome                               | Vault files    | Header held    |
    /// |---------------------------------------|----------------|----------------|
    /// | `Ok`                                  | The archive's. | The archive's. |
    /// | `Err`, nothing swapped or swap undone | The previous.  | The previous.  |
    /// | `Err`, and the undo failed as well    | Mid-swap.      | The previous.  |
    ///
    /// "The previous" is no header at all when there was no vault. The
    /// header is not read back from disk after the swap: it is the one that
    /// was checked before the swap and then renamed into place, so no step
    /// that could fail follows the commit, and `Ok` never leaves this handle
    /// on the header of the vault that was replaced.
    ///
    /// In the last row the error is the [`Error::Io`] whose operation is
    /// `put previous vault files back`. The next [`Vault::open_path`]
    /// completes the undo, after which the files are the previous vault
    /// again and match the header this handle kept. Until then this handle
    /// must not be used: an unlock through it fails, or, when the archive
    /// was made from this same vault, opens a database that the undo will
    /// move away again. Open a new handle.
    ///
    /// # Errors
    ///
    /// Everything [`restore_from_path`] returns.
    pub fn restore_from(&mut self, archive: &Path, replace: bool) -> Result<()> {
        self.lock();
        let header = restore_pair(archive, self.data_dir(), replace)?;
        self.adopt_published_header(header);
        Ok(())
    }
}

/// Removes the unpacked archive after a failed restore.
///
/// Not while `restore-new` exists, which is the case when the undo itself
/// failed: the undo that the next open runs reads a missing
/// `vault.db.restore-tmp` as "the live database is the new one", and would
/// set the previous database aside and then delete it.
fn discard_unpacked_unless_swap_pending(paths: &RestorePaths) {
    if paths.verified_header.exists() {
        return;
    }
    discard_file(&paths.unpacked_header);
    discard_file(&paths.unpacked_db);
}

/// Step 1 of the restore protocol: unpacks `archive` to the two
/// `restore-tmp` files, checks them and returns the unpacked header.
///
/// A header the vault would call corrupt is reported as an invalid backup
/// here: the vault on disk is fine, and it is the archive that is not.
fn unpack_and_verify(archive: &Path, paths: &RestorePaths) -> Result<VaultHeader> {
    unpack_archive_to_staging(archive, &paths.unpacked_header, &paths.unpacked_db)?;

    let header = load_archive_header(&paths.unpacked_header)?;
    if is_plaintext_sqlite(&paths.unpacked_db)? {
        return Err(Error::BackupInvalid(BackupDefect::DatabaseNotEncrypted));
    }
    Ok(header)
}

/// Loads the header unpacked from an archive, with the checks
/// [`VaultHeader::load`] makes.
///
/// A header from a later build is a defect of the archive here as well, and
/// not [`Error::VaultTooNew`]: that error is about the vault in the data
/// directory, which this restore has not touched.
///
/// # Errors
///
/// [`Error::BackupInvalid`] with [`BackupDefect::UnusableHeader`] when the
/// file is not a header or names a format version other than the one this
/// build reads; [`Error::Io`] when it cannot be read.
fn load_archive_header(path: &Path) -> Result<VaultHeader> {
    let unusable = |reason| Error::BackupInvalid(BackupDefect::UnusableHeader(reason));

    let header = match VaultHeader::read(path) {
        Ok(header) => header,
        Err(Error::VaultCorrupt(reason)) => return Err(unusable(reason)),
        Err(other) => return Err(other),
    };
    match header.check_format() {
        Ok(()) => Ok(header),
        Err(Error::VaultTooNew { .. }) => Err(unusable(VaultCorruption::UnsupportedFormat {
            version: header.version,
        })),
        Err(Error::VaultCorrupt(reason)) => Err(unusable(reason)),
        Err(other) => Err(other),
    }
}

/// Returns [`Error::RestoreWouldOverwrite`] when a live vault file exists
/// and the caller did not ask to replace it.
///
/// Either file counts, so a restore never silently completes half a vault.
fn refuse_overwrite(paths: &RestorePaths, replace: bool) -> Result<()> {
    let vault_present = paths.header.exists() || paths.db.exists();
    if vault_present && !replace {
        return Err(Error::RestoreWouldOverwrite);
    }
    Ok(())
}

/// Steps 2 to 7 of the restore protocol. On an error the previous files are
/// back in place, unless the error says that putting them back failed too.
fn swap_in_unpacked_pair(paths: &RestorePaths) -> Result<()> {
    remove_files_if_present(&[&paths.shm])?;

    if let Err(err) = replace_live_pair(paths) {
        return Err(match undo_swap(paths) {
            Ok(()) => err,
            Err(undo_err) => Error::io(
                "put previous vault files back",
                format_args!("{undo_err}; the restore had failed with: {err}"),
            ),
        });
    }

    // Committed. A leftover here is removed by the next open.
    discard_file(&paths.old_header);
    discard_file(&paths.old_db);
    discard_file(&paths.old_wal);
    Ok(())
}

/// Steps 3 to 6 of the restore protocol.
fn replace_live_pair(paths: &RestorePaths) -> Result<()> {
    rename_synced(&paths.unpacked_header, &paths.verified_header)?;

    rename_if_present(&paths.header, &paths.old_header)?;
    rename_if_present(&paths.wal, &paths.old_wal)?;
    rename_if_present(&paths.db, &paths.old_db)?;

    fs::rename(&paths.unpacked_db, &paths.db).io("move restored database into place")?;
    rename_synced(&paths.verified_header, &paths.header)
}

/// Puts the previous pair back while `restore-new` still marks the swap as
/// uncommitted.
///
/// Every step is a no-op when repeated, and `restore-new` is removed only
/// after the previous pair is back, so an interrupted undo is simply run
/// again.
fn undo_swap(paths: &RestorePaths) -> Result<()> {
    // With the unpacked database gone, `vault.db` is the new one. Renaming it
    // back, instead of removing it, restores the condition this test reads:
    // after the old database returns below, a repeated undo must not take it
    // for the new one.
    if !paths.unpacked_db.exists() {
        rename_if_present(&paths.db, &paths.unpacked_db)?;
    }
    rename_if_present(&paths.old_header, &paths.header)?;
    rename_if_present(&paths.old_db, &paths.db)?;
    rename_if_present(&paths.old_wal, &paths.wal)?;

    remove_files_if_present(&[&paths.verified_header])?;
    sync_parent_dir(&paths.header);
    Ok(())
}

/// Checks that the header and the database both exist as files.
///
/// # Errors
///
/// [`Error::VaultUninitialized`] when neither exists; [`Error::VaultCorrupt`]
/// when only one does.
fn ensure_vault_files(header_path: &Path, db_path: &Path) -> Result<()> {
    match (header_path.is_file(), db_path.is_file()) {
        (true, true) => Ok(()),
        (false, false) => Err(Error::VaultUninitialized),
        (true, false) => Err(Error::VaultCorrupt(VaultCorruption::HeaderWithoutDatabase)),
        (false, true) => Err(Error::VaultCorrupt(VaultCorruption::DatabaseWithoutHeader)),
    }
}

/// Refuses a vault beside which a password change left its staged header.
///
/// The staged header is there either because the change died before the
/// rekey, and the published header still fits the database, or because it
/// died after, and only the staged one does. Deciding takes the key, so the
/// file's presence alone is the refusal. Packing the staged header instead
/// would get the first case wrong the same way.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] with [`VaultCorruption::UnfinishedPasswordChange`]
/// when the staged header exists; [`Error::Io`] when that cannot be
/// determined.
fn ensure_no_staged_header(data_dir: &Path) -> Result<()> {
    let staged_header_exists = vault_staged_header_path(data_dir)
        .try_exists()
        .io("look for a staged vault header")?;

    if staged_header_exists {
        return Err(Error::VaultCorrupt(
            VaultCorruption::UnfinishedPasswordChange,
        ));
    }
    Ok(())
}

/// Refuses a database whose write-ahead log still holds pages.
///
/// `SQLite` removes the log when the last connection closes cleanly, so a
/// non-empty one next to a locked vault is left from a crash and can hold
/// committed transactions that are not in `vault.db` yet.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] with [`VaultCorruption::UnmergedWriteAheadLog`]
/// when the log is not empty; [`Error::Io`] when it cannot be inspected.
fn ensure_no_unmerged_wal(db_path: &Path) -> Result<()> {
    let [wal_path, _shm_path] = db_sidecar_paths(db_path);
    let wal_len = match fs::metadata(&wal_path) {
        Ok(metadata) => metadata.len(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => 0,
        Err(err) => return Err(Error::io("inspect write-ahead log", err)),
    };

    if wal_len > 0 {
        return Err(Error::VaultCorrupt(VaultCorruption::UnmergedWriteAheadLog));
    }
    Ok(())
}

/// Snapshots the database behind `conn` and packs the snapshot with the
/// on-disk header into an archive at `dest`. The session stays open.
///
/// The snapshot is `vault.db.backup-tmp` in the data directory. It is
/// removed before this returns. One left by a crash is normally gone by
/// then, removed when the vault was opened; this removes it first all the
/// same, because `VACUUM INTO` refuses a target that is not empty.
fn backup_from_open_connection(conn: &Connection, data_dir: &Path, dest: &Path) -> Result<()> {
    let header_path = vault_header_path(data_dir);
    if !header_path.is_file() {
        return Err(Error::VaultCorrupt(VaultCorruption::DatabaseWithoutHeader));
    }

    let snapshot = backup_snapshot_db_path(data_dir);
    discard_database_files(&snapshot);

    let vacuum = vacuum_into_encrypted(conn, &snapshot);
    if let Err(err) = vacuum {
        discard_database_files(&snapshot);
        return Err(err);
    }

    let packed = write_archive_from_paths(&header_path, &snapshot, dest);
    discard_database_files(&snapshot);
    packed
}

/// Writes a compacted copy of the database behind `conn` to `dest` with
/// `VACUUM INTO`, and checks that the copy is not plaintext.
///
/// The target path is written into the statement as a string literal with
/// its quotes doubled, which is why a path that is not UTF-8 is refused.
///
/// # Errors
///
/// [`Error::Io`] when `dest` is not UTF-8 or cannot be created;
/// [`Error::Database`] when the statement fails; [`Error::Crypto`] when the
/// copy starts with the plaintext `SQLite` magic.
fn vacuum_into_encrypted(conn: &Connection, dest: &Path) -> Result<()> {
    let path = dest
        .to_str()
        .ok_or_else(|| Error::io("name backup snapshot", "the path is not UTF-8"))?;
    let escaped = path.replace('\'', "''");

    // SQLite would create the snapshot under the umask, leaving a copy of the
    // whole vault readable by other accounts until it is packed and removed.
    // `VACUUM INTO` accepts a target that exists as long as it is empty
    // (https://www.sqlite.org/lang_vacuum.html#vacuuminto).
    drop(create_private_file(dest)?);

    conn.execute(&format!("VACUUM INTO '{escaped}'"), [])
        .database("snapshot vault database")?;
    if is_plaintext_sqlite(dest)? {
        return Err(Error::crypto(
            "snapshot vault database",
            "the snapshot is not encrypted",
        ));
    }
    Ok(())
}

/// Whether the file at `path` starts with the magic of a plaintext `SQLite`
/// database. A file shorter than the magic does not.
fn is_plaintext_sqlite(path: &Path) -> Result<bool> {
    const SQLITE_MAGIC: &[u8; 6] = b"SQLite";

    let mut magic = [0u8; SQLITE_MAGIC.len()];
    let mut file = File::open(path).io("open database to check encryption")?;
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(&magic == SQLITE_MAGIC),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(err) => Err(Error::io("read database to check encryption", err)),
    }
}

/// Writes the archive of `header_path` and `db_path` to `dest`: magic,
/// version, then the header member and the database member.
///
/// The archive is written under [`STAGED_SUFFIX`], flushed, and renamed over
/// `dest`. A failure removes the staged file and leaves `dest` as it was.
/// A missing parent directory of `dest` is created with default permissions:
/// it is the user's directory, not the vault's.
fn write_archive_from_paths(header_path: &Path, db_path: &Path, dest: &Path) -> Result<()> {
    let staged = sibling_path(dest, STAGED_SUFFIX)?;
    let result = (|| {
        let parent = dest.parent().filter(|dir| !dir.as_os_str().is_empty());
        if let Some(parent) = parent {
            fs::create_dir_all(parent).io("create backup directory")?;
        }

        let mut archive = create_private_file(&staged)?;
        archive.write_all(MAGIC).io("write backup archive")?;
        archive
            .write_all(&FORMAT_VERSION.to_le_bytes())
            .io("write backup archive")?;
        write_member_from_path(&mut archive, MEMBER_HEADER, header_path)?;
        write_member_from_path(&mut archive, MEMBER_DB, db_path)?;
        archive.sync_all().io("sync backup archive")?;
        drop(archive);

        rename_synced(&staged, dest)
    })();

    if result.is_err() {
        discard_file(&staged);
    }
    result
}

/// Reads `archive` and writes its two members to `header_dest` and
/// `db_dest`, enforcing the reader rules of the format in the module doc.
///
/// On an error the destinations may hold part of a member; the caller
/// removes them.
///
/// # Errors
///
/// [`Error::BackupInvalid`] when the archive breaks the format;
/// [`Error::Io`] when it cannot be opened or read, or a destination cannot
/// be written.
fn unpack_archive_to_staging(archive: &Path, header_dest: &Path, db_dest: &Path) -> Result<()> {
    let mut input = File::open(archive).io("open backup archive")?;
    let mut magic = [0u8; 8];
    read_exact_or_truncated(&mut input, &mut magic)?;
    if &magic != MAGIC {
        return Err(Error::BackupInvalid(BackupDefect::NotABackup));
    }

    let version = read_u16_le(&mut input)?;
    if version != FORMAT_VERSION {
        return Err(Error::BackupInvalid(BackupDefect::UnsupportedVersion {
            version,
        }));
    }

    let mut saw_header = false;
    let mut saw_db = false;
    for _ in 0..2 {
        let Some(name) = read_member_name_or_eof(&mut input)? else {
            break;
        };
        let len = read_u64_le(&mut input)?;
        if len == 0 {
            return Err(Error::BackupInvalid(BackupDefect::EmptyMember { name }));
        }

        match name.as_str() {
            MEMBER_HEADER => {
                if saw_header {
                    return Err(Error::BackupInvalid(BackupDefect::DuplicateMember {
                        name: MEMBER_HEADER,
                    }));
                }
                write_exact_member(&mut input, header_dest, len)?;
                saw_header = true;
            }
            MEMBER_DB => {
                if saw_db {
                    return Err(Error::BackupInvalid(BackupDefect::DuplicateMember {
                        name: MEMBER_DB,
                    }));
                }
                write_exact_member(&mut input, db_dest, len)?;
                saw_db = true;
            }
            _ => {
                return Err(Error::BackupInvalid(BackupDefect::UnexpectedMember {
                    name,
                }));
            }
        }
    }

    if !saw_header {
        return Err(Error::BackupInvalid(BackupDefect::MissingMember {
            name: MEMBER_HEADER,
        }));
    }
    if !saw_db {
        return Err(Error::BackupInvalid(BackupDefect::MissingMember {
            name: MEMBER_DB,
        }));
    }

    let mut extra = [0u8; 1];
    match input.read(&mut extra) {
        Ok(0) => Ok(()),
        Ok(_) => Err(Error::BackupInvalid(BackupDefect::TrailingData)),
        Err(err) => Err(Error::io("read backup archive", err)),
    }
}

/// Appends the file at `path` to `archive` as the member `name`.
///
/// The length written is the file's size when it was opened; see
/// [`write_member`] for what happens when the file changes meanwhile.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be opened or inspected, and
/// everything [`write_member`] returns.
fn write_member_from_path(archive: &mut impl Write, name: &'static str, path: &Path) -> Result<()> {
    let mut source = File::open(path).io("open vault file for backup")?;
    let len = source.metadata().io("inspect vault file for backup")?.len();
    write_member(archive, name, &mut source, len)
}

/// Appends the next `len` bytes of `source` to `archive` as the member
/// `name`.
///
/// Exactly `len` bytes are copied, so the member matches its length field
/// even if the file behind `source` grows meanwhile. The source is a
/// parameter, and not opened here, so that a test can hand in one that ends
/// early.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] when `len` is 0: the reader rejects an empty
/// member, so the archive would not restore. [`Error::Io`] when `source`
/// cannot be read, the archive cannot be written, or `source` ends before
/// `len` bytes: the file became shorter after its size was read, and the
/// member already written is not what its length field says.
fn write_member(
    archive: &mut impl Write,
    name: &'static str,
    source: &mut impl Read,
    len: u64,
) -> Result<()> {
    if len == 0 {
        return Err(Error::VaultCorrupt(VaultCorruption::EmptyFile {
            file: name,
        }));
    }
    write_member_prefix(archive, name, len)?;

    let copied = copy_at_most(source, archive, len)?;
    if copied < len {
        return Err(Error::io(
            "copy vault file into backup",
            format_args!("{name} ended after {copied} of {len} bytes"),
        ));
    }
    Ok(())
}

/// Writes the fields that precede a member's contents: name length, name,
/// contents length.
///
/// # Errors
///
/// [`Error::BackupInvalid`] when `name` is longer than a `u16` can count;
/// [`Error::Io`] when the archive cannot be written.
fn write_member_prefix(archive: &mut impl Write, name: &str, len: u64) -> Result<()> {
    let name_bytes = name.as_bytes();
    let name_len = u16::try_from(name_bytes.len())
        .map_err(|_| Error::BackupInvalid(BackupDefect::MemberNameLength))?;
    archive
        .write_all(&name_len.to_le_bytes())
        .io("write backup archive")?;
    archive.write_all(name_bytes).io("write backup archive")?;
    archive
        .write_all(&len.to_le_bytes())
        .io("write backup archive")?;
    Ok(())
}

/// Copies the next `len` bytes of `input` into a new owner-only file at
/// `dest` and flushes it.
///
/// # Errors
///
/// [`Error::BackupInvalid`] with [`BackupDefect::Truncated`] when `input`,
/// the archive, ends before `len` bytes; [`Error::Io`] when it cannot be
/// read or `dest` cannot be created, written or flushed.
fn write_exact_member(input: &mut impl Read, dest: &Path, len: u64) -> Result<()> {
    let mut file = create_private_file(dest)?;
    if copy_at_most(input, &mut file, len)? < len {
        return Err(Error::BackupInvalid(BackupDefect::Truncated));
    }
    file.sync_all().io("sync restored file")?;
    Ok(())
}

/// Copies `len` bytes from `reader` to `writer` through a fixed buffer, so a
/// member of any size is never held in memory, and returns how many bytes
/// were copied.
///
/// The count is below `len` exactly when `reader` ended first. What that
/// means differs by direction, so the caller decides: an archive that ends
/// inside a member is a truncated backup, and a vault file that ends early
/// while it is packed is a failed copy.
///
/// # Errors
///
/// [`Error::Io`] when a read or a write fails.
fn copy_at_most(reader: &mut impl Read, writer: &mut impl Write, len: u64) -> Result<u64> {
    let mut remaining = len;
    let mut buffer = vec![0u8; COPY_BUFFER_LEN];
    while remaining > 0 {
        // `usize` to `u64` and back only fails on a platform where one does
        // not fit the other; it is reported instead of truncated.
        let capacity = u64::try_from(buffer.len()).map_err(|_| length_overflow())?;
        let wanted = usize::try_from(remaining.min(capacity)).map_err(|_| length_overflow())?;
        let read = match reader.read(&mut buffer[..wanted]) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(Error::io("read backup member", err)),
        };
        writer
            .write_all(&buffer[..read])
            .io("write backup member")?;
        let copied = u64::try_from(read).map_err(|_| length_overflow())?;
        remaining = remaining.checked_sub(copied).ok_or_else(length_overflow)?;
    }
    len.checked_sub(remaining).ok_or_else(length_overflow)
}

/// The error for a length that does not convert between `usize` and `u64`
/// while a member is copied.
fn length_overflow() -> Error {
    Error::io("copy backup member", "length does not fit this platform")
}

/// Reads the name of the next member, or returns `None` when the archive
/// ends cleanly where a member would start.
///
/// # Errors
///
/// [`Error::BackupInvalid`] when the name length is 0 or above
/// [`MAX_MEMBER_NAME_LEN`], the archive ends inside the name, or the name is
/// not UTF-8 or holds a path separator, a NUL or `..`. The last check is
/// defence in depth: names are only ever compared, never opened.
fn read_member_name_or_eof(reader: &mut impl Read) -> Result<Option<String>> {
    let Some(name_len) = read_optional_u16_le(reader)? else {
        return Ok(None);
    };
    if name_len == 0 || name_len > MAX_MEMBER_NAME_LEN {
        return Err(Error::BackupInvalid(BackupDefect::MemberNameLength));
    }
    let mut name_bytes = vec![0u8; usize::from(name_len)];
    read_exact_or_truncated(reader, &mut name_bytes)?;
    let name = String::from_utf8(name_bytes)
        .map_err(|_| Error::BackupInvalid(BackupDefect::MemberNameNotUtf8))?;
    if name.contains('/') || name.contains('\\') || name.contains('\0') || name.contains("..") {
        return Err(Error::BackupInvalid(BackupDefect::UnexpectedMember {
            name,
        }));
    }
    Ok(Some(name))
}

/// Reads a little-endian `u16`, or returns `None` when `reader` is already
/// at its end.
///
/// A reader that ends after one of the two bytes is truncated, not ended.
fn read_optional_u16_le(reader: &mut impl Read) -> Result<Option<u16>> {
    let mut bytes = [0u8; 2];
    match reader.read(&mut bytes) {
        Ok(0) => Ok(None),
        Ok(2) => Ok(Some(u16::from_le_bytes(bytes))),
        Ok(read) => {
            read_exact_or_truncated(reader, &mut bytes[read..])?;
            Ok(Some(u16::from_le_bytes(bytes)))
        }
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(err) => Err(Error::io("read backup archive", err)),
    }
}

/// Reads a little-endian `u16`; the end of `reader` is a truncated archive.
fn read_u16_le(reader: &mut impl Read) -> Result<u16> {
    let mut bytes = [0u8; 2];
    read_exact_or_truncated(reader, &mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

/// Reads a little-endian `u64`; the end of `reader` is a truncated archive.
fn read_u64_le(reader: &mut impl Read) -> Result<u64> {
    let mut bytes = [0u8; 8];
    read_exact_or_truncated(reader, &mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

/// Fills `buffer` from `reader`.
///
/// # Errors
///
/// [`Error::BackupInvalid`] when `reader` ends first; [`Error::Io`] for any
/// other read failure.
fn read_exact_or_truncated(reader: &mut impl Read, buffer: &mut [u8]) -> Result<()> {
    reader.read_exact(buffer).map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            Error::BackupInvalid(BackupDefect::Truncated)
        } else {
            Error::io("read backup archive", err)
        }
    })
}

/// Encodes an archive from named members, for tests of truncated and
/// incomplete archives.
#[cfg(test)]
fn encode_members(members: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut archive = Vec::new();
    archive.extend_from_slice(MAGIC);
    archive.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    for (name, data) in members {
        let len = u64::try_from(data.len())
            .map_err(|_| Error::io("encode backup member", "member too large"))?;
        write_member_prefix(&mut archive, name, len)?;
        archive.extend_from_slice(data);
    }
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::vault::VaultStatus;
    #[cfg(unix)]
    use crate::vault::permissions::mode_of;
    use tempfile::TempDir;

    const PASSWORD: &str = "UNIQUE-MASTER-PASSWORD-TOKEN-9f3a";

    fn init_vault() -> (TempDir, Vault) {
        let dir = TempDir::new().expect("tempdir");
        let mut vault = Vault::open_path(dir.path()).expect("open vault");
        vault.init(PASSWORD).expect("init");
        (dir, vault)
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).expect("write");
    }

    fn backup_of(vault: &Vault) -> (TempDir, PathBuf) {
        let dest = TempDir::new().expect("archive dir");
        let archive = dest.path().join("books.oikonomia-backup");
        vault.backup_to(&archive).expect("backup");
        (dest, archive)
    }

    fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn round_trip_backup_restore_unlocks_with_original_password() {
        let (src, mut vault) = init_vault();
        vault.lock();

        let header_before = fs::read(vault_header_path(src.path())).expect("header");
        let db_before = fs::read(vault_db_path(src.path())).expect("db");

        let (_archive_dir, archive) = backup_of(&vault);

        let restore_dir = TempDir::new().expect("restore");
        restore_from_path(&archive, restore_dir.path(), false).expect("restore");

        assert_eq!(
            fs::read(vault_header_path(restore_dir.path())).expect("header"),
            header_before
        );
        assert_eq!(
            fs::read(vault_db_path(restore_dir.path())).expect("db"),
            db_before
        );

        let mut restored = Vault::open_path(restore_dir.path()).expect("open restored");
        assert_eq!(restored.status(), VaultStatus::Locked);
        restored
            .unlock(PASSWORD)
            .expect("original master password must unlock the restored vault");
        assert_eq!(restored.status(), VaultStatus::Unlocked);
        restored.lock();
        restored
            .unlock("not the password!!")
            .expect_err("wrong password must still fail");
    }

    #[test]
    fn incomplete_archive_header_only_db_only_truncated_rejected() {
        let dest = TempDir::new().expect("dir");

        let header_only = encode_members(&[(MEMBER_HEADER, b"{\"kdf\":\"argon2id\"}")])
            .expect("encode header-only");
        let path = dest.path().join("header-only.oikonomia-backup");
        write_file(&path, &header_only);
        let err = restore_from_path(&path, dest.path(), true).expect_err("header-only");
        assert_eq!(
            err,
            Error::BackupInvalid(BackupDefect::MissingMember { name: MEMBER_DB })
        );

        let db_only =
            encode_members(&[(MEMBER_DB, b"not-sqlite-ciphertext")]).expect("encode db-only");
        let path = dest.path().join("db-only.oikonomia-backup");
        write_file(&path, &db_only);
        let err = restore_from_path(&path, dest.path(), true).expect_err("db-only");
        assert_eq!(
            err,
            Error::BackupInvalid(BackupDefect::MissingMember {
                name: MEMBER_HEADER
            })
        );

        let valid = encode_members(&[
            (MEMBER_HEADER, b"{\"kdf\":\"argon2id\"}"),
            (MEMBER_DB, b"ciphertext-bytes"),
        ])
        .expect("encode");
        let truncated_path = dest.path().join("truncated.oikonomia-backup");
        write_file(&truncated_path, &valid[..valid.len() - 4]);
        let err = restore_from_path(&truncated_path, dest.path(), true).expect_err("truncated");
        assert_eq!(err, Error::BackupInvalid(BackupDefect::Truncated));
    }

    #[test]
    fn restore_without_replace_does_not_overwrite() {
        let (src, mut vault) = init_vault();
        vault.lock();
        let header_before = fs::read(vault_header_path(src.path())).expect("header");
        let db_before = fs::read(vault_db_path(src.path())).expect("db");

        let other = TempDir::new().expect("other");
        let mut other_vault = Vault::open_path(other.path()).expect("open other");
        other_vault
            .init("a different password 12")
            .expect("init other");
        other_vault.lock();
        let archive = other.path().join("other.oikonomia-backup");
        other_vault.backup_to(&archive).expect("backup other");

        let err = restore_from_path(&archive, src.path(), false).expect_err("no replace");
        assert_eq!(err, Error::RestoreWouldOverwrite);

        assert_eq!(
            fs::read(vault_header_path(src.path())).expect("header after"),
            header_before
        );
        assert_eq!(
            fs::read(vault_db_path(src.path())).expect("db after"),
            db_before
        );

        vault.unlock(PASSWORD).expect("original vault still opens");
    }

    #[test]
    fn restore_into_uninitialized_succeeds_without_replace() {
        let (_src, vault) = init_vault();
        let (_archive_dir, archive) = backup_of(&vault);

        let restore_dir = TempDir::new().expect("empty");
        let mut restored = Vault::open_path(restore_dir.path()).expect("open empty");
        assert_eq!(restored.status(), VaultStatus::Uninitialized);
        assert!(!vault_header_path(restore_dir.path()).exists());
        assert!(!vault_db_path(restore_dir.path()).exists());

        restored
            .restore_from(&archive, false)
            .expect("replace is not required on an empty data dir");
        assert_eq!(restored.status(), VaultStatus::Locked);
        restored
            .unlock(PASSWORD)
            .expect("restored empty-dir vault unlocks");
    }

    #[test]
    fn restore_while_locked_succeeds_and_leaves_locked() {
        let (_src, source) = init_vault();
        let (_archive_dir, archive) = backup_of(&source);

        let (_dest_dir, mut dest) = init_vault_with("a different password 12");
        dest.lock();
        assert_eq!(dest.status(), VaultStatus::Locked);

        dest.restore_from(&archive, true)
            .expect("replace into a locked vault");
        assert_eq!(dest.status(), VaultStatus::Locked);
        dest.unlock(PASSWORD)
            .expect("restored locked vault unlocks with the backup password");
        dest.lock();
        dest.unlock("a different password 12")
            .expect_err("pre-restore password must no longer open the vault");
    }

    #[test]
    fn restore_while_unlocked_locks_then_replaces() {
        let (_src, source) = init_vault();
        let (_archive_dir, archive) = backup_of(&source);

        let (_dest_dir, mut dest) = init_vault_with("a different password 12");
        assert_eq!(dest.status(), VaultStatus::Unlocked);

        dest.restore_from(&archive, true)
            .expect("replace must lock first");
        assert_eq!(dest.status(), VaultStatus::Locked);
        dest.unlock(PASSWORD)
            .expect("replaced vault unlocks with the backup password");
    }

    #[test]
    fn a_restore_leaves_the_handle_on_the_restored_vault_without_reopening_the_directory() {
        let (_src, source) = init_vault();
        let (_archive_dir, archive) = backup_of(&source);
        let (dest_dir, mut dest) = init_vault_with(OTHER_PASSWORD);
        // Opening this directory fails from here on: a directory sits where
        // open removes a leftover snapshot. The restore itself does not
        // touch that name, so the vault files are replaced all the same.
        fs::create_dir(backup_snapshot_db_path(dest_dir.path())).expect("block reopening");
        Vault::open_path(dest_dir.path())
            .map(|vault| vault.status())
            .expect_err("the scenario needs a directory that cannot be opened");

        dest.restore_from(&archive, true)
            .expect("the files were replaced, so the restore succeeded");

        assert_eq!(dest.status(), VaultStatus::Locked);
        dest.unlock(PASSWORD)
            .expect("the handle derives its key from the restored header");
    }

    #[test]
    fn a_refused_restore_leaves_the_handle_on_the_previous_vault() {
        let (_src, source) = init_vault();
        let (_archive_dir, archive) = backup_of(&source);
        let (_dest_dir, mut dest) = init_vault_with(OTHER_PASSWORD);

        assert_eq!(
            dest.restore_from(&archive, false),
            Err(Error::RestoreWouldOverwrite)
        );

        assert_eq!(
            dest.status(),
            VaultStatus::Locked,
            "locked before the check"
        );
        dest.unlock(OTHER_PASSWORD)
            .expect("the previous vault and its header are still in place");
    }

    #[test]
    fn random_non_magic_file_is_rejected() {
        let dest = TempDir::new().expect("dir");
        let garbage = dest.path().join("garbage.oikonomia-backup");
        write_file(&garbage, b"this is not a backup file at all");
        let err = restore_from_path(&garbage, dest.path(), true).expect_err("garbage");
        assert_eq!(err, Error::BackupInvalid(BackupDefect::NotABackup));

        let zipish = dest.path().join("random.bin");
        write_file(&zipish, &[0x50, 0x4b, 0x03, 0x04, 0xff, 0x00]);
        let err = restore_from_path(&zipish, dest.path(), true).expect_err("zip magic");
        assert!(
            matches!(err, Error::BackupInvalid(_)),
            "non-magic must be BackupInvalid, got {err:?}"
        );
    }

    #[test]
    fn backup_while_unlocked_stays_unlocked_and_round_trips() {
        let (_src, vault) = init_vault();
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        let conn = vault.connection().expect("conn");
        crate::ledger::create_entity(
            conn,
            &crate::ledger::CreateEntity {
                name: "Personal".into(),
                base_currency: "EUR".into(),
                chart_template: crate::domain::ChartTemplate::Personal,
                fiscal_year_start_month: Some(1),
            },
            crate::prefs::Locale::En,
        )
        .expect("entity so WAL has pages");

        let (_archive_dir, archive) = backup_of(&vault);
        assert_eq!(
            vault.status(),
            VaultStatus::Unlocked,
            "backup must not lock an open session"
        );
        let still_open = vault.connection().expect("session still open");
        let live = crate::ledger::list_entities(still_open).expect("list live");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].name, "Personal");

        let restore_dir = TempDir::new().expect("restore");
        restore_from_path(&archive, restore_dir.path(), false).expect("restore");
        let db_bytes = fs::read(vault_db_path(restore_dir.path())).expect("db");
        assert!(db_bytes.len() > 16);
        assert_ne!(&db_bytes[0..6], b"SQLite", "archive must stay ciphertext");

        let mut restored = Vault::open_path(restore_dir.path()).expect("open");
        restored
            .unlock(PASSWORD)
            .expect("original master password must unlock the snapshot");
        let restored_entities =
            crate::ledger::list_entities(restored.connection().expect("conn")).expect("list");
        assert_eq!(restored_entities.len(), 1);
        assert_eq!(restored_entities[0].name, "Personal");
    }

    fn create_personal_entity(vault: &Vault) {
        crate::ledger::create_entity(
            vault.connection().expect("conn"),
            &crate::ledger::CreateEntity {
                name: "Personal".into(),
                base_currency: "EUR".into(),
                chart_template: crate::domain::ChartTemplate::Personal,
                fiscal_year_start_month: Some(1),
            },
            crate::prefs::Locale::En,
        )
        .expect("entity");
    }

    /// Copies the vault files as a crash would leave them: the session never
    /// closed, so committed transactions are still in the write-ahead log.
    fn crash_image_of(live: &Path) -> TempDir {
        let image = TempDir::new().expect("crash image dir");
        let [live_wal, _shm] = db_sidecar_paths(&vault_db_path(live));
        let [image_wal, _shm] = db_sidecar_paths(&vault_db_path(image.path()));
        for (from, to) in [
            (vault_header_path(live), vault_header_path(image.path())),
            (vault_db_path(live), vault_db_path(image.path())),
            (live_wal, image_wal.clone()),
        ] {
            fs::copy(from, to).expect("copy vault file");
        }
        assert!(
            fs::metadata(&image_wal).expect("wal").len() > 0,
            "the scenario needs committed pages in the write-ahead log"
        );
        image
    }

    #[test]
    fn locked_backup_refuses_a_vault_with_unmerged_wal_pages() {
        let (live, vault) = init_vault();
        create_personal_entity(&vault);
        let image = crash_image_of(live.path());
        let dest = image.path().join("books.oikonomia-backup");

        let err = backup_to_path(image.path(), &dest).expect_err("the log is not in the archive");

        assert_eq!(
            err,
            Error::VaultCorrupt(VaultCorruption::UnmergedWriteAheadLog)
        );
        assert!(!dest.exists(), "no partial archive");
    }

    #[test]
    fn unlocking_once_makes_a_crashed_vault_backable() {
        let (live, vault) = init_vault();
        create_personal_entity(&vault);
        let image = crash_image_of(live.path());

        let mut recovered = Vault::open_path(image.path()).expect("open crash image");
        recovered.unlock(PASSWORD).expect("unlock replays the log");
        recovered.lock();
        let (_archive_dir, archive) = backup_of(&recovered);

        let restore_dir = TempDir::new().expect("restore");
        restore_from_path(&archive, restore_dir.path(), false).expect("restore");
        let mut restored = Vault::open_path(restore_dir.path()).expect("open restored");
        restored.unlock(PASSWORD).expect("unlock restored");
        let entities =
            crate::ledger::list_entities(restored.connection().expect("conn")).expect("list");
        assert_eq!(entities.len(), 1, "the logged transaction is in the backup");
    }

    const OTHER_PASSWORD: &str = "a different password 12";

    // The on-disk names of the restore protocol, spelled out so a rename of
    // one is a visible format change.
    const UNPACKED_HEADER: &str = "vault.header.json.restore-tmp";
    const UNPACKED_DB: &str = "vault.db.restore-tmp";
    const VERIFIED_HEADER: &str = "vault.header.json.restore-new";
    const OLD_HEADER: &str = "vault.header.json.restore-old";
    const OLD_DB: &str = "vault.db.restore-old";
    const OLD_WAL: &str = "vault.db-wal.restore-old";
    const RESTORE_NAMES: [&str; 6] = [
        UNPACKED_HEADER,
        UNPACKED_DB,
        VERIFIED_HEADER,
        OLD_HEADER,
        OLD_DB,
        OLD_WAL,
    ];

    /// A locked vault under [`PASSWORD`] and, in a second directory, the
    /// files of a vault under [`OTHER_PASSWORD`] that a restore brings in.
    fn live_and_incoming() -> (TempDir, TempDir) {
        let (live, mut live_vault) = init_vault();
        live_vault.lock();
        let (incoming, mut incoming_vault) = init_vault_with(OTHER_PASSWORD);
        incoming_vault.lock();
        (live, incoming)
    }

    fn move_file(from: &Path, to: &Path) {
        fs::rename(from, to).expect("move file");
    }

    fn assert_no_restore_files(data_dir: &Path) {
        for name in RESTORE_NAMES {
            assert!(!data_dir.join(name).exists(), "{name} must be gone");
        }
    }

    fn assert_opens_with(data_dir: &Path, password: &str) {
        let mut vault = Vault::open_path(data_dir).expect("open");
        assert_eq!(vault.status(), VaultStatus::Locked);
        vault.unlock(password).expect("the expected password opens");
    }

    #[test]
    fn restore_rejects_a_header_that_is_not_a_vault_header() {
        let (live, incoming) = live_and_incoming();
        let db = fs::read(vault_db_path(incoming.path())).expect("db");
        let archive = live.path().join("junk-header.oikonomia-backup");
        let bytes = encode_members(&[(MEMBER_HEADER, b"{\"kdf\":\"argon2id\"}"), (MEMBER_DB, &db)])
            .expect("encode");
        write_file(&archive, &bytes);

        let err = restore_from_path(&archive, live.path(), true).expect_err("junk header");

        assert!(
            matches!(err, Error::BackupInvalid(BackupDefect::UnusableHeader(_))),
            "got {err:?}"
        );
        assert_no_restore_files(live.path());
        assert_opens_with(live.path(), PASSWORD);
    }

    #[test]
    fn restore_rejects_a_header_from_a_newer_vault_format() {
        let (live, incoming) = live_and_incoming();
        let db = fs::read(vault_db_path(incoming.path())).expect("db");
        let mut header =
            VaultHeader::load(&vault_header_path(incoming.path())).expect("incoming header");
        header.version += 1;
        let header = serde_json::to_vec(&header).expect("encode header");
        let archive = live.path().join("newer.oikonomia-backup");
        let bytes = encode_members(&[(MEMBER_HEADER, &header), (MEMBER_DB, &db)]).expect("encode");
        write_file(&archive, &bytes);

        let err = restore_from_path(&archive, live.path(), true).expect_err("newer format");

        assert_eq!(
            err,
            Error::BackupInvalid(BackupDefect::UnusableHeader(
                VaultCorruption::UnsupportedFormat { version: 2 }
            )),
            "the archive is what cannot be used, not the vault in place"
        );
        assert_no_restore_files(live.path());
        assert_opens_with(live.path(), PASSWORD);
    }

    #[test]
    fn restore_rejects_a_plaintext_database() {
        let (live, incoming) = live_and_incoming();
        let header = fs::read(vault_header_path(incoming.path())).expect("header");
        let plain_path = incoming.path().join("plain.db");
        rusqlite::Connection::open(&plain_path)
            .expect("open plaintext sqlite")
            .execute_batch("CREATE TABLE t (id INTEGER);")
            .expect("schema");
        let plain = fs::read(&plain_path).expect("plaintext db");
        let archive = live.path().join("plaintext.oikonomia-backup");
        let bytes =
            encode_members(&[(MEMBER_HEADER, &header), (MEMBER_DB, &plain)]).expect("encode");
        write_file(&archive, &bytes);

        let err = restore_from_path(&archive, live.path(), true).expect_err("plaintext db");

        assert_eq!(
            err,
            Error::BackupInvalid(BackupDefect::DatabaseNotEncrypted)
        );
        assert_no_restore_files(live.path());
        assert_opens_with(live.path(), PASSWORD);
    }

    #[test]
    fn open_removes_a_half_unpacked_archive() {
        let (live, incoming) = live_and_incoming();
        fs::copy(
            vault_header_path(incoming.path()),
            live.path().join(UNPACKED_HEADER),
        )
        .expect("leftover header");

        assert_opens_with(live.path(), PASSWORD);
        assert_no_restore_files(live.path());
    }

    #[test]
    fn open_removes_the_snapshot_of_an_interrupted_online_backup() {
        let (dir, vault) = init_vault();
        drop(vault);
        let snapshot = dir.path().join("vault.db.backup-tmp");
        assert_eq!(snapshot, backup_snapshot_db_path(dir.path()));
        write_file(&snapshot, b"a second copy of the ciphertext");

        assert_opens_with(dir.path(), PASSWORD);

        assert!(!snapshot.exists(), "the leftover snapshot must be removed");
    }

    #[test]
    fn open_reports_a_snapshot_it_cannot_remove() {
        let (dir, vault) = init_vault();
        drop(vault);
        // A directory in the snapshot's place: removing it as a file fails
        // on every platform, for a reason other than "not found".
        let snapshot = backup_snapshot_db_path(dir.path());
        fs::create_dir(&snapshot).expect("directory in the snapshot's place");

        let err = Vault::open_path(dir.path())
            .map(|vault| vault.status())
            .expect_err("only a missing snapshot is ignored");

        assert!(
            matches!(
                err,
                Error::Io {
                    operation: "remove vault file",
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn open_undoes_a_restore_that_died_before_the_old_pair_moved() {
        let (live, incoming) = live_and_incoming();
        move_file(
            &vault_header_path(incoming.path()),
            &live.path().join(VERIFIED_HEADER),
        );
        move_file(
            &vault_db_path(incoming.path()),
            &live.path().join(UNPACKED_DB),
        );

        assert_opens_with(live.path(), PASSWORD);
        assert_no_restore_files(live.path());
    }

    #[test]
    fn open_undoes_a_restore_that_died_with_only_the_header_set_aside() {
        let (live, incoming) = live_and_incoming();
        move_file(
            &vault_header_path(incoming.path()),
            &live.path().join(VERIFIED_HEADER),
        );
        move_file(
            &vault_db_path(incoming.path()),
            &live.path().join(UNPACKED_DB),
        );
        move_file(
            &vault_header_path(live.path()),
            &live.path().join(OLD_HEADER),
        );

        assert_opens_with(live.path(), PASSWORD);
        assert_no_restore_files(live.path());
    }

    #[test]
    fn open_undoes_a_restore_that_died_after_the_new_database_went_in() {
        // The state the old code could leave for good: one vault's database
        // with no header that opens it.
        let (live, incoming) = live_and_incoming();
        move_file(
            &vault_header_path(live.path()),
            &live.path().join(OLD_HEADER),
        );
        move_file(&vault_db_path(live.path()), &live.path().join(OLD_DB));
        move_file(
            &vault_header_path(incoming.path()),
            &live.path().join(VERIFIED_HEADER),
        );
        move_file(&vault_db_path(incoming.path()), &vault_db_path(live.path()));

        assert_opens_with(live.path(), PASSWORD);
        assert_no_restore_files(live.path());
    }

    #[test]
    fn open_undoes_an_interrupted_restore_into_an_empty_directory() {
        let (_live, incoming) = live_and_incoming();
        let empty = TempDir::new().expect("empty");
        move_file(
            &vault_header_path(incoming.path()),
            &empty.path().join(VERIFIED_HEADER),
        );
        move_file(
            &vault_db_path(incoming.path()),
            &vault_db_path(empty.path()),
        );

        let vault = Vault::open_path(empty.path()).expect("open");

        assert_eq!(vault.status(), VaultStatus::Uninitialized);
        assert!(!vault_db_path(empty.path()).exists(), "half-restored db");
        assert_no_restore_files(empty.path());
    }

    #[test]
    fn open_finishes_a_restore_that_died_before_removing_the_old_pair() {
        let (live, incoming) = live_and_incoming();
        move_file(
            &vault_header_path(live.path()),
            &live.path().join(OLD_HEADER),
        );
        move_file(&vault_db_path(live.path()), &live.path().join(OLD_DB));
        move_file(
            &vault_header_path(incoming.path()),
            &vault_header_path(live.path()),
        );
        move_file(&vault_db_path(incoming.path()), &vault_db_path(live.path()));

        assert_opens_with(live.path(), OTHER_PASSWORD);
        assert_no_restore_files(live.path());
    }

    #[test]
    fn restore_removes_the_old_sidecars_and_recovery_files() {
        let (live, incoming) = live_and_incoming();
        let archive = incoming.path().join("incoming.oikonomia-backup");
        backup_to_path(incoming.path(), &archive).expect("backup incoming");
        let [wal, shm] = db_sidecar_paths(&vault_db_path(live.path()));
        write_file(&wal, b"");
        write_file(&shm, b"stale index");

        restore_from_path(&archive, live.path(), true).expect("restore");

        assert!(!wal.exists(), "old write-ahead log");
        assert!(!shm.exists(), "old shared-memory index");
        assert_no_restore_files(live.path());
        assert_opens_with(live.path(), OTHER_PASSWORD);
    }

    #[test]
    fn master_password_never_appears_in_backup_bytes() {
        let (_src, vault) = init_vault();
        let (_archive_dir, archive) = backup_of(&vault);
        let bytes = fs::read(&archive).expect("read archive");
        assert!(
            !contains_bytes(&bytes, PASSWORD.as_bytes()),
            "master password must not appear in the portable archive"
        );
        assert!(
            !contains_bytes(&bytes, b"UNIQUE-MASTER-PASSWORD"),
            "password prefix must not appear in the portable archive"
        );
    }

    #[test]
    fn restore_with_replace_overwrites_and_drops_staged_header() {
        let (src, mut vault) = init_vault();
        let staged = vault_staged_header_path(src.path());
        write_file(&staged, b"stale-staged-header");

        let other = TempDir::new().expect("other");
        let mut other_vault = Vault::open_path(other.path()).expect("open other");
        other_vault
            .init("a different password 12")
            .expect("init other");
        let archive = other.path().join("other.oikonomia-backup");
        other_vault.backup_to(&archive).expect("backup other");
        let expected_header = fs::read(vault_header_path(other.path())).expect("other header");

        vault.restore_from(&archive, true).expect("replace restore");
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(!staged.exists(), "staged password-change header must go");
        assert_eq!(
            fs::read(vault_header_path(src.path())).expect("replaced header"),
            expected_header
        );
        vault
            .unlock("a different password 12")
            .expect("replaced vault unlocks with the backup password");
        vault.lock();
        vault
            .unlock(PASSWORD)
            .expect_err("pre-restore password must no longer open the vault");
    }

    #[test]
    fn staged_header_tmp_is_not_included_in_backup() {
        let (src, vault) = init_vault();
        let staged = vault_staged_header_path(src.path());
        write_file(&staged, b"must-not-be-in-backup");

        let (_dest, archive) = backup_of(&vault);
        let restore_dir = TempDir::new().expect("restore");
        restore_from_path(&archive, restore_dir.path(), false).expect("restore");
        assert!(
            !vault_staged_header_path(restore_dir.path()).exists(),
            "restore must not plant vault.header.json.tmp"
        );

        let archive_bytes = fs::read(&archive).expect("read archive");
        assert!(
            !contains_bytes(&archive_bytes, b"must-not-be-in-backup"),
            "password-change tmp header must not be packed"
        );

        let crafted = encode_members(&[
            (MEMBER_HEADER, b"{\"kdf\":\"argon2id\"}"),
            (MEMBER_DB, b"ciphertext-bytes"),
            ("vault.header.json.tmp", b"sneaky"),
        ])
        .expect("encode with tmp");
        let crafted_path = src.path().join("with-tmp.oikonomia-backup");
        write_file(&crafted_path, &crafted);
        let err =
            restore_from_path(&crafted_path, restore_dir.path(), true).expect_err("tmp member");
        assert!(
            matches!(
                err,
                Error::BackupInvalid(
                    BackupDefect::TrailingData | BackupDefect::UnexpectedMember { .. }
                )
            ),
            "got {err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn online_snapshot_is_owner_only() {
        let (dir, vault) = init_vault();
        let snapshot = backup_snapshot_db_path(dir.path());

        vacuum_into_encrypted(vault.connection().expect("conn"), &snapshot).expect("snapshot");

        assert_eq!(mode_of(&snapshot), 0o600, "a copy of the whole vault");
    }

    #[cfg(unix)]
    #[test]
    fn backup_and_restore_write_owner_only_files() {
        let (_src, vault) = init_vault();
        let (_archive_dir, archive) = backup_of(&vault);
        assert_eq!(mode_of(&archive), 0o600, "archive");

        let restore_dir = TempDir::new().expect("restore");
        let data_dir = restore_dir.path().join("vault");
        restore_from_path(&archive, &data_dir, false).expect("restore");
        assert_eq!(mode_of(&data_dir), 0o700, "data directory");
        assert_eq!(mode_of(&vault_header_path(&data_dir)), 0o600, "header");
        assert_eq!(mode_of(&vault_db_path(&data_dir)), 0o600, "database");
    }

    #[test]
    fn backup_uninitialized_is_error() {
        let dir = TempDir::new().expect("dir");
        let dest = dir.path().join("x.oikonomia-backup");
        let err = backup_to_path(dir.path(), &dest).expect_err("empty");
        assert_eq!(err, Error::VaultUninitialized);
    }

    #[test]
    fn a_vault_file_that_shrinks_while_it_is_packed_is_a_failed_copy() {
        // The size was read as 4096 bytes and the file then ends after 10.
        let mut shrunk: &[u8] = b"ten bytes!";
        let mut archive = Vec::new();

        let err = write_member(&mut archive, MEMBER_DB, &mut shrunk, 4096)
            .expect_err("the member is shorter than its length field");

        assert_eq!(
            err,
            Error::Io {
                operation: "copy vault file into backup",
                detail: "vault.db ended after 10 of 4096 bytes".to_owned(),
            },
            "nothing is wrong with an archive: none was being read"
        );
    }

    #[test]
    fn a_member_is_as_long_as_its_length_field_when_the_file_grows() {
        let contents = b"0123456789";
        let mut grown: &[u8] = b"0123456789 and bytes appended after the size was read";
        let mut archive = Vec::new();

        write_member(&mut archive, MEMBER_DB, &mut grown, 10).expect("write member");

        // A member ends with its eight-byte length field and its contents.
        let contents_start = archive.len() - contents.len();
        let length_field: [u8; 8] = archive[contents_start - 8..contents_start]
            .try_into()
            .expect("eight bytes");
        assert_eq!(&archive[contents_start..], contents);
        assert_eq!(u64::from_le_bytes(length_field), 10);
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let dir = TempDir::new().expect("dir");
        let mut bytes = encode_members(&[
            (MEMBER_HEADER, b"{\"kdf\":\"argon2id\"}"),
            (MEMBER_DB, b"ciphertext-bytes"),
        ])
        .expect("encode");
        bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
        let path = dir.path().join("v99.oikonomia-backup");
        write_file(&path, &bytes);
        let err = restore_from_path(&path, dir.path(), true).expect_err("v99");
        assert_eq!(
            err,
            Error::BackupInvalid(BackupDefect::UnsupportedVersion { version: 99 })
        );
    }

    #[test]
    fn plaintext_sqlite_is_told_apart_from_a_vault_database() {
        let (dir, vault) = init_vault();
        drop(vault);
        let plain_path = dir.path().join("plain.db");
        rusqlite::Connection::open(&plain_path)
            .expect("open plaintext sqlite")
            .execute_batch("CREATE TABLE t (id INTEGER);")
            .expect("schema");
        let short_path = dir.path().join("short.db");
        write_file(&short_path, b"SQL");

        assert!(is_plaintext_sqlite(&plain_path).expect("plain"));
        assert!(!is_plaintext_sqlite(&vault_db_path(dir.path())).expect("vault"));
        assert!(!is_plaintext_sqlite(&short_path).expect("short"));
    }

    #[test]
    fn a_failed_swap_puts_the_previous_pair_back() {
        let (live, incoming) = live_and_incoming();
        let paths = RestorePaths::new(live.path());
        // Only the header was unpacked, so the swap fails at the database
        // rename, after the live pair has been set aside.
        move_file(&vault_header_path(incoming.path()), &paths.unpacked_header);

        let err = swap_in_unpacked_pair(&paths).expect_err("no database to swap in");

        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
        assert!(!paths.verified_header.exists(), "swap marker");
        assert!(!paths.old_header.exists() && !paths.old_db.exists());
        assert_opens_with(live.path(), PASSWORD);
    }

    #[test]
    fn a_failed_swap_puts_the_previous_write_ahead_log_back() {
        let (live, incoming) = live_and_incoming();
        let paths = RestorePaths::new(live.path());
        let [wal, _shm] = db_sidecar_paths(&paths.db);
        write_file(&wal, b"commits a crash left in the log");
        move_file(&vault_header_path(incoming.path()), &paths.unpacked_header);

        swap_in_unpacked_pair(&paths).expect_err("no database to swap in");

        assert_eq!(
            fs::read(&wal).expect("log is back"),
            b"commits a crash left in the log"
        );
    }

    #[test]
    fn a_failed_restore_keeps_the_unpacked_database_while_the_swap_is_pending() {
        let dir = TempDir::new().expect("dir");
        let paths = RestorePaths::new(dir.path());
        for path in [&paths.unpacked_header, &paths.unpacked_db] {
            write_file(path, b"unpacked");
        }

        write_file(&paths.verified_header, b"marker");
        discard_unpacked_unless_swap_pending(&paths);
        assert!(
            paths.unpacked_db.exists(),
            "the pending undo reads this file"
        );

        fs::remove_file(&paths.verified_header).expect("swap settled");
        discard_unpacked_unless_swap_pending(&paths);
        assert!(!paths.unpacked_db.exists() && !paths.unpacked_header.exists());
    }

    #[test]
    fn open_undoes_a_restore_and_returns_the_previous_write_ahead_log() {
        let (live, incoming) = live_and_incoming();
        let paths = RestorePaths::new(live.path());
        move_file(&paths.header, &paths.old_header);
        move_file(&paths.db, &paths.old_db);
        write_file(&live.path().join(OLD_WAL), b"");
        move_file(&vault_header_path(incoming.path()), &paths.verified_header);
        move_file(&vault_db_path(incoming.path()), &paths.db);

        assert_opens_with(live.path(), PASSWORD);
        assert_no_restore_files(live.path());
    }

    #[test]
    fn an_undo_that_is_run_twice_keeps_the_previous_database() {
        let (live, incoming) = live_and_incoming();
        let paths = RestorePaths::new(live.path());
        move_file(&paths.header, &paths.old_header);
        move_file(&paths.db, &paths.old_db);
        move_file(&vault_header_path(incoming.path()), &paths.verified_header);
        move_file(&vault_db_path(incoming.path()), &paths.db);

        // A crash between the old pair returning and the marker going away
        // leaves the marker for a second run.
        undo_swap(&paths).expect("first undo");
        fs::copy(&paths.header, &paths.verified_header).expect("marker left behind");
        undo_swap(&paths).expect("second undo");

        assert_opens_with(live.path(), PASSWORD);
    }

    #[test]
    fn default_backup_file_name_is_dated_local_iso() {
        let name = default_backup_file_name();
        let prefix = "oikonomia-backup-";
        let suffix = format!(".{BACKUP_EXTENSION}");
        assert!(name.starts_with(prefix), "{name}");
        assert!(name.ends_with(&suffix), "{name}");
        let date = name
            .strip_prefix(prefix)
            .and_then(|dated| dated.strip_suffix(suffix.as_str()))
            .expect("dated backup name");
        crate::util::parse_date(date).expect("YYYY-MM-DD");
        assert_eq!(date, local_iso_date());
    }

    fn init_vault_with(password: &str) -> (TempDir, Vault) {
        let dir = TempDir::new().expect("tempdir");
        let mut vault = Vault::open_path(dir.path()).expect("open vault");
        vault.init(password).expect("init");
        (dir, vault)
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;
    use tempfile::TempDir;

    use super::*;

    /// Bytes of three kinds: anything; the magic and version followed by
    /// anything; and well-formed members cut off at an arbitrary byte.
    fn archive_bytes() -> impl Strategy<Value = Vec<u8>> {
        let anything = prop::collection::vec(any::<u8>(), 0..512);
        let past_the_version = anything.clone().prop_map(|rest| {
            let mut bytes = MAGIC.to_vec();
            bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
            bytes.extend(rest);
            bytes
        });

        let name = prop::sample::select(vec![MEMBER_HEADER, MEMBER_DB, "other"]);
        let member = (name, prop::collection::vec(any::<u8>(), 0..64));
        let cut_members = (
            prop::collection::vec(member, 0..4),
            any::<prop::sample::Index>(),
        )
            .prop_map(|(members, cut)| {
                let members: Vec<(&str, &[u8])> = members
                    .iter()
                    .map(|(name, data)| (*name, data.as_slice()))
                    .collect();
                let mut bytes = encode_members(&members).unwrap();
                bytes.truncate(cut.index(bytes.len() + 1));
                bytes
            });

        prop_oneof![anything, past_the_version, cut_members]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn unpacking_any_bytes_returns_instead_of_panicking(bytes in archive_bytes()) {
            let dir = TempDir::new().unwrap();
            let archive = dir.path().join("archive");
            fs::write(&archive, &bytes).unwrap();

            let _ = unpack_archive_to_staging(
                &archive,
                &dir.path().join("header"),
                &dir.path().join("database"),
            );
        }
    }
}
