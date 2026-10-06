//! Portable backup of the `SQLCipher` ciphertext plus public vault header.
//!
//! The archive is not a second encryption layer: it stores `vault.db` and
//! `vault.header.json` as ciphertext. The master password is never stored.
//! `vault.header.json.tmp` is crash-recovery state for password change, not a
//! source of truth, and is omitted.
//!
//! An unlocked vault is snapshotted with `VACUUM INTO` so WAL is folded
//! without closing the session. A locked vault is copied file by file, which
//! is only complete when `vault.db-wal` is absent or empty: the database
//! runs in WAL mode, and after a crash committed transactions can still sit
//! in that log. Such a vault is refused until it has been unlocked once,
//! which replays the log into `vault.db`.
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

use super::paths::{vault_db_path, vault_header_path, vault_staged_header_path};
use super::permissions::{create_private_dir, create_private_file};
use super::store::Vault;
use crate::error::{Error, Result};
use crate::vault::files::{
    discard_database_files, discard_file, local_iso_date, remove_files_if_present,
    rename_if_present, rename_synced, sibling_path, sync_parent_dir,
};
use crate::vault::header::VaultHeader;
use crate::vault::paths::{
    RestorePaths, backup_snapshot_db_path, db_sidecar_paths, vault_init_header_path,
};

/// Unencrypted magic. Identifies the file; contains no secrets.
pub(super) const MAGIC: &[u8; 8] = b"OIKOBACK";

/// Archive format version written by this crate.
pub(super) const FORMAT_VERSION: u16 = 1;

/// File extension for portable vault backups (no leading dot).
pub const BACKUP_EXTENSION: &str = "oikonomia-backup";

/// Default native-save filename: `oikonomia-backup-YYYY-MM-DD.oikonomia-backup`.
///
/// Uses the local calendar date so consecutive daily backups sort in Finder.
#[must_use]
pub fn default_backup_file_name() -> String {
    format!("oikonomia-backup-{}.{BACKUP_EXTENSION}", local_iso_date())
}

const MEMBER_DB: &str = "vault.db";
const MEMBER_HEADER: &str = "vault.header.json";

/// Write a portable archive of the ciphertext vault files at `dest`.
///
/// File-copies `vault.db` and `vault.header.json` when there is no open
/// writer (locked / files on disk only). The destination is written as
/// `dest` + `.tmp` in the same directory, then renamed into place.
///
/// # Errors
///
/// [`Error::VaultUninitialized`] when neither vault file exists;
/// [`Error::VaultCorrupt`] when only one of the two source-of-truth files is
/// present, or when `vault.db-wal` holds pages a file copy would leave out;
/// [`Error::Io`] on filesystem failures.
pub fn backup_to_path(data_dir: &Path, dest: &Path) -> Result<()> {
    let header_path = vault_header_path(data_dir);
    let db_path = vault_db_path(data_dir);
    ensure_vault_files(&header_path, &db_path)?;
    write_archive_from_paths(&header_path, &db_path, dest)
}

/// Unpacks a backup archive into `data_dir`, leaving the vault locked.
///
/// Decrypt is not required: members are ciphertext plus the public header.
/// Existing `vault.db` / `vault.header.json` are left untouched unless
/// `replace` is true. `replace` is required only when a vault already
/// exists; an uninitialized data directory accepts `replace: false`.
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
/// [`Error::BackupInvalid`] for a non-archive, truncated, or incomplete
/// file, a header member that is not a vault header of a known format, or a
/// database member that is not encrypted;
/// [`Error::RestoreWouldOverwrite`] when vault files exist and `replace` is
/// false; [`Error::Io`] on filesystem failures.
pub fn restore_from_path(archive: &Path, data_dir: &Path, replace: bool) -> Result<()> {
    create_private_dir(data_dir)?;
    recover_interrupted_restore(data_dir)?;
    let paths = RestorePaths::new(data_dir);

    // Unpack and check first, so a bad archive is rejected even when a vault
    // already exists and `replace` is false.
    let restored = unpack_and_verify(archive, &paths)
        .and_then(|()| refuse_overwrite(&paths, replace))
        .and_then(|()| swap_in_unpacked_pair(&paths));
    if let Err(err) = restored {
        discard_unpacked_unless_swap_pending(&paths);
        return Err(err);
    }

    // Recovery state of the vault that was replaced.
    discard_file(&vault_staged_header_path(data_dir));
    discard_file(&vault_init_header_path(data_dir));
    Ok(())
}

/// Settles a restore that a crash interrupted, by the rules in the module
/// doc: undoes a swap that did not commit, finishes one that did, and
/// removes a half-unpacked archive. Does nothing when no restore files exist.
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

fn unpack_and_verify(archive: &Path, paths: &RestorePaths) -> Result<()> {
    unpack_archive_to_staging(archive, &paths.unpacked_header, &paths.unpacked_db)?;

    match VaultHeader::load(&paths.unpacked_header) {
        Ok(_header) => {}
        Err(Error::VaultCorrupt(reason)) => {
            return Err(Error::BackupInvalid(format!(
                "{MEMBER_HEADER} is not a usable vault header: {reason}"
            )));
        }
        Err(other) => return Err(other),
    }
    if is_plaintext_sqlite(&paths.unpacked_db)? {
        return Err(Error::BackupInvalid(format!(
            "{MEMBER_DB} is not encrypted"
        )));
    }
    Ok(())
}

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
            Err(undo_err) => Error::Io(format!(
                "{err}; putting the previous vault files back also failed: {undo_err}"
            )),
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

    fs::rename(&paths.unpacked_db, &paths.db).map_err(|err| Error::Io(err.to_string()))?;
    rename_synced(&paths.verified_header, &paths.header)
}

/// Puts the previous pair back while `restore-new` still marks the swap as
/// uncommitted. Every step is a no-op when repeated, and `restore-new` is
/// removed only after the previous pair is back, so an interrupted undo is
/// simply run again.
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

impl Vault {
    /// Write a portable ciphertext archive to `dest` without changing lock state.
    ///
    /// Unlocked: `VACUUM INTO` a temp file (`SQLCipher` keeps the dest keyed with
    /// the live connection's key), then pack it with `vault.header.json`.
    /// Locked: copy the on-disk pair, as [`backup_to_path`] does.
    ///
    /// # Errors
    ///
    /// See [`backup_to_path`].
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        match self.connection() {
            Ok(conn) => backup_from_open_connection(conn, self.data_dir(), dest),
            Err(Error::VaultLocked) => backup_to_path(self.data_dir(), dest),
            Err(other) => Err(other),
        }
    }

    /// Lock if needed, unpack `archive` into this vault's data directory, and
    /// leave the vault locked.
    ///
    /// In-memory header state is reloaded from the restored files. The
    /// `SQLCipher` connection is not reopened.
    ///
    /// # Errors
    ///
    /// See [`restore_from_path`]. Reload failures surface as
    /// [`Error::VaultCorrupt`] or [`Error::Io`].
    pub fn restore_from(&mut self, archive: &Path, replace: bool) -> Result<()> {
        self.lock();
        let data_dir = self.data_dir().to_path_buf();
        restore_from_path(archive, &data_dir, replace)?;
        *self = Self::open_path(data_dir)?;
        Ok(())
    }
}

fn ensure_vault_files(header_path: &Path, db_path: &Path) -> Result<()> {
    match (header_path.is_file(), db_path.is_file()) {
        (true, true) => ensure_no_unmerged_wal(db_path),
        (false, false) => Err(Error::VaultUninitialized),
        (true, false) => Err(Error::VaultCorrupt(
            "vault header exists without database".into(),
        )),
        (false, true) => Err(Error::VaultCorrupt(
            "vault database exists without header".into(),
        )),
    }
}

/// Refuses a database whose write-ahead log still holds pages.
///
/// `SQLite` removes the log when the last connection closes cleanly, so a
/// non-empty one next to a locked vault is left from a crash and can hold
/// committed transactions that are not in `vault.db` yet.
fn ensure_no_unmerged_wal(db_path: &Path) -> Result<()> {
    let [wal_path, _shm_path] = db_sidecar_paths(db_path);
    let wal_len = match fs::metadata(&wal_path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => 0,
        Err(err) => return Err(Error::Io(err.to_string())),
    };

    if wal_len > 0 {
        return Err(Error::VaultCorrupt(
            "vault database has changes still in its write-ahead log; \
             unlock the vault once before backing up"
                .into(),
        ));
    }
    Ok(())
}

/// Consistent snapshot of an open `SQLCipher` connection, packed with the
/// on-disk header. The live session stays open.
fn backup_from_open_connection(conn: &Connection, data_dir: &Path, dest: &Path) -> Result<()> {
    let header_path = vault_header_path(data_dir);
    if !header_path.is_file() {
        return Err(Error::VaultCorrupt(
            "database exists without vault header".into(),
        ));
    }

    let snap = backup_snapshot_db_path(data_dir);
    discard_database_files(&snap);

    let vacuum = vacuum_into_encrypted(conn, &snap);
    if let Err(err) = vacuum {
        discard_database_files(&snap);
        return Err(err);
    }

    let packed = write_archive_from_paths(&header_path, &snap, dest);
    discard_database_files(&snap);
    packed
}

fn vacuum_into_encrypted(conn: &Connection, dest: &Path) -> Result<()> {
    let path = dest
        .to_str()
        .ok_or_else(|| Error::Io("backup snapshot path is not UTF-8".into()))?;
    let escaped = path.replace('\'', "''");

    // SQLite would create the snapshot under the umask, leaving a copy of the
    // whole vault readable by other accounts until it is packed and removed.
    // `VACUUM INTO` accepts a target that exists as long as it is empty
    // (https://www.sqlite.org/lang_vacuum.html#vacuuminto).
    drop(create_private_file(dest)?);

    conn.execute(&format!("VACUUM INTO '{escaped}'"), [])
        .map_err(|err| Error::Io(err.to_string()))?;
    if is_plaintext_sqlite(dest)? {
        return Err(Error::Io(
            "online backup produced a plaintext database".into(),
        ));
    }
    Ok(())
}

/// Whether the file at `path` starts with the magic of a plaintext `SQLite`
/// database. A file shorter than the magic does not.
fn is_plaintext_sqlite(path: &Path) -> Result<bool> {
    const SQLITE_MAGIC: &[u8; 6] = b"SQLite";

    let mut magic = [0u8; SQLITE_MAGIC.len()];
    let mut file = File::open(path).map_err(|err| Error::Io(err.to_string()))?;
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(&magic == SQLITE_MAGIC),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(err) => Err(Error::Io(err.to_string())),
    }
}

fn write_archive_from_paths(header_path: &Path, db_path: &Path, dest: &Path) -> Result<()> {
    let tmp = sibling_path(dest, ".tmp")?;
    let result = (|| {
        if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|err| Error::Io(err.to_string()))?;
        }

        let mut out = create_private_file(&tmp)?;
        out.write_all(MAGIC)
            .map_err(|err| Error::Io(err.to_string()))?;
        out.write_all(&FORMAT_VERSION.to_le_bytes())
            .map_err(|err| Error::Io(err.to_string()))?;
        write_member_from_path(&mut out, MEMBER_HEADER, header_path)?;
        write_member_from_path(&mut out, MEMBER_DB, db_path)?;
        out.sync_all().map_err(|err| Error::Io(err.to_string()))?;
        drop(out);

        rename_synced(&tmp, dest)
    })();

    if result.is_err() {
        discard_file(&tmp);
    }
    result
}

fn unpack_archive_to_staging(archive: &Path, header_tmp: &Path, db_tmp: &Path) -> Result<()> {
    let mut input = File::open(archive).map_err(|err| Error::Io(err.to_string()))?;
    let mut magic = [0u8; 8];
    read_exact_or_truncated(&mut input, &mut magic)?;
    if &magic != MAGIC {
        return Err(Error::BackupInvalid("not an Oikonomia vault backup".into()));
    }

    let version = read_u16_le(&mut input)?;
    if version != FORMAT_VERSION {
        return Err(Error::BackupInvalid(format!(
            "unsupported backup version {version}"
        )));
    }

    let mut saw_header = false;
    let mut saw_db = false;
    for _ in 0..2 {
        let Some(name) = read_member_name_or_eof(&mut input)? else {
            break;
        };
        let len = read_u64_le(&mut input)?;
        if len == 0 {
            return Err(Error::BackupInvalid(format!("{name} is empty")));
        }

        match name.as_str() {
            MEMBER_HEADER => {
                if saw_header {
                    return Err(Error::BackupInvalid(
                        "backup has duplicate vault.header.json".into(),
                    ));
                }
                write_exact_member(&mut input, header_tmp, len)?;
                saw_header = true;
            }
            MEMBER_DB => {
                if saw_db {
                    return Err(Error::BackupInvalid("backup has duplicate vault.db".into()));
                }
                write_exact_member(&mut input, db_tmp, len)?;
                saw_db = true;
            }
            other => {
                return Err(Error::BackupInvalid(format!("unexpected member {other}")));
            }
        }
    }

    if !saw_header {
        return Err(Error::BackupInvalid(
            "backup is missing vault.header.json".into(),
        ));
    }
    if !saw_db {
        return Err(Error::BackupInvalid("backup is missing vault.db".into()));
    }

    let mut extra = [0u8; 1];
    match input.read(&mut extra) {
        Ok(0) => Ok(()),
        Ok(_) => Err(Error::BackupInvalid("backup has trailing data".into())),
        Err(err) => Err(Error::Io(err.to_string())),
    }
}

fn write_member_from_path(out: &mut impl Write, name: &str, path: &Path) -> Result<()> {
    let mut src = File::open(path).map_err(|err| Error::Io(err.to_string()))?;
    let len = src
        .metadata()
        .map_err(|err| Error::Io(err.to_string()))?
        .len();
    if len == 0 {
        return Err(Error::VaultCorrupt(format!("{name} is empty")));
    }
    write_member_prefix(out, name, len)?;
    copy_exact(&mut src, out, len)?;
    Ok(())
}

fn write_member_prefix(out: &mut impl Write, name: &str, len: u64) -> Result<()> {
    let name_bytes = name.as_bytes();
    let name_len = u16::try_from(name_bytes.len())
        .map_err(|_| Error::BackupInvalid("member name too long".into()))?;
    out.write_all(&name_len.to_le_bytes())
        .map_err(|err| Error::Io(err.to_string()))?;
    out.write_all(name_bytes)
        .map_err(|err| Error::Io(err.to_string()))?;
    out.write_all(&len.to_le_bytes())
        .map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

fn write_exact_member(input: &mut impl Read, dest: &Path, len: u64) -> Result<()> {
    let mut file = create_private_file(dest)?;
    copy_exact(input, &mut file, len)?;
    file.sync_all().map_err(|err| Error::Io(err.to_string()))?;
    Ok(())
}

fn copy_exact(reader: &mut impl Read, writer: &mut impl Write, len: u64) -> Result<()> {
    let mut remaining = len;
    let mut buf = vec![0u8; 16_384];
    while remaining > 0 {
        let cap = u64::try_from(buf.len())
            .map_err(|_| Error::Io("backup copy length overflow".into()))?;
        let want = usize::try_from(remaining.min(cap))
            .map_err(|_| Error::Io("backup copy length overflow".into()))?;
        let n = match reader.read(&mut buf[..want]) {
            Ok(0) => return Err(Error::BackupInvalid("backup is truncated".into())),
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                return Err(Error::BackupInvalid("backup is truncated".into()));
            }
            Err(err) => return Err(Error::Io(err.to_string())),
        };
        writer
            .write_all(&buf[..n])
            .map_err(|err| Error::Io(err.to_string()))?;
        let n = u64::try_from(n).map_err(|_| Error::Io("backup copy length overflow".into()))?;
        remaining = remaining
            .checked_sub(n)
            .ok_or_else(|| Error::Io("backup copy length overflow".into()))?;
    }
    Ok(())
}

fn read_member_name_or_eof(reader: &mut impl Read) -> Result<Option<String>> {
    let Some(name_len) = read_optional_u16_le(reader)? else {
        return Ok(None);
    };
    if name_len == 0 || name_len > 255 {
        return Err(Error::BackupInvalid("invalid member name length".into()));
    }
    let mut buf = vec![0u8; usize::from(name_len)];
    read_exact_or_truncated(reader, &mut buf)?;
    let name = String::from_utf8(buf)
        .map_err(|_| Error::BackupInvalid("member name is not UTF-8".into()))?;
    if name.contains('/') || name.contains('\\') || name.contains('\0') || name.contains("..") {
        return Err(Error::BackupInvalid(format!("unexpected member {name}")));
    }
    Ok(Some(name))
}

fn read_optional_u16_le(reader: &mut impl Read) -> Result<Option<u16>> {
    let mut buf = [0u8; 2];
    match reader.read(&mut buf) {
        Ok(0) => Ok(None),
        Ok(2) => Ok(Some(u16::from_le_bytes(buf))),
        Ok(n) => {
            read_exact_or_truncated(reader, &mut buf[n..])?;
            Ok(Some(u16::from_le_bytes(buf)))
        }
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(err) => Err(Error::Io(err.to_string())),
    }
}

fn read_u16_le(reader: &mut impl Read) -> Result<u16> {
    let mut buf = [0u8; 2];
    read_exact_or_truncated(reader, &mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u64_le(reader: &mut impl Read) -> Result<u64> {
    let mut buf = [0u8; 8];
    read_exact_or_truncated(reader, &mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

fn read_exact_or_truncated(reader: &mut impl Read, buf: &mut [u8]) -> Result<()> {
    reader.read_exact(buf).map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            Error::BackupInvalid("backup is truncated".into())
        } else {
            Error::Io(err.to_string())
        }
    })
}

/// Encode an archive from named members. Test helper for truncated/incomplete cases.
#[cfg(test)]
fn encode_members(members: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    for (name, data) in members {
        let len = u64::try_from(data.len()).map_err(|_| Error::Io("member too large".into()))?;
        write_member_prefix(&mut out, name, len)?;
        out.extend_from_slice(data);
    }
    Ok(out)
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
        assert!(
            matches!(err, Error::BackupInvalid(ref msg) if msg.contains("vault.db")),
            "got {err:?}"
        );

        let db_only =
            encode_members(&[(MEMBER_DB, b"not-sqlite-ciphertext")]).expect("encode db-only");
        let path = dest.path().join("db-only.oikonomia-backup");
        write_file(&path, &db_only);
        let err = restore_from_path(&path, dest.path(), true).expect_err("db-only");
        assert!(
            matches!(err, Error::BackupInvalid(ref msg) if msg.contains("vault.header.json")),
            "got {err:?}"
        );

        let valid = encode_members(&[
            (MEMBER_HEADER, b"{\"kdf\":\"argon2id\"}"),
            (MEMBER_DB, b"ciphertext-bytes"),
        ])
        .expect("encode");
        let truncated_path = dest.path().join("truncated.oikonomia-backup");
        write_file(&truncated_path, &valid[..valid.len() - 4]);
        let err = restore_from_path(&truncated_path, dest.path(), true).expect_err("truncated");
        assert!(
            matches!(err, Error::BackupInvalid(_)),
            "truncated must be BackupInvalid, got {err:?}"
        );
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
    fn random_non_magic_file_is_rejected() {
        let dest = TempDir::new().expect("dir");
        let garbage = dest.path().join("garbage.oikonomia-backup");
        write_file(&garbage, b"this is not a backup file at all");
        let err = restore_from_path(&garbage, dest.path(), true).expect_err("garbage");
        assert!(
            matches!(err, Error::BackupInvalid(ref msg) if msg.contains("not an Oikonomia")),
            "got {err:?}"
        );

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

        assert!(
            matches!(err, Error::VaultCorrupt(ref message) if message.contains("unlock")),
            "got {err:?}"
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
            matches!(err, Error::BackupInvalid(ref message) if message.contains("header")),
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

        assert!(matches!(err, Error::BackupInvalid(_)), "got {err:?}");
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

        assert!(
            matches!(err, Error::BackupInvalid(ref message) if message.contains("not encrypted")),
            "got {err:?}"
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
            matches!(err, Error::BackupInvalid(ref msg) if msg.contains("trailing") || msg.contains("unexpected")),
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
        assert!(
            matches!(err, Error::BackupInvalid(ref msg) if msg.contains("version")),
            "got {err:?}"
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

        assert!(matches!(err, Error::Io(_)), "got {err:?}");
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
            .and_then(|s| s.strip_suffix(suffix.as_str()))
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
