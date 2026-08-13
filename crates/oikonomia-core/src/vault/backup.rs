//! Portable backup of the `SQLCipher` ciphertext plus public vault header.
//!
//! The archive is not a second encryption layer: it stores `vault.db` and
//! `vault.header.json` as ciphertext. The master password is never stored.
//! `vault.header.json.tmp` is crash-recovery state for password change, not a
//! source of truth, and is omitted.
//!
//! An unlocked vault is snapshotted with `VACUUM INTO` so WAL is folded
//! without closing the session. A locked vault is a quiescent file copy.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::paths::{vault_db_path, vault_header_path, vault_staged_header_path};
use super::store::Vault;
use crate::error::{Error, Result};

/// Unencrypted magic. Identifies the file; contains no secrets.
pub const MAGIC: &[u8; 8] = b"OIKOBACK";

/// Archive format version written by this crate.
pub const FORMAT_VERSION: u16 = 1;

/// File extension for portable vault backups (no leading dot).
pub const BACKUP_EXTENSION: &str = "oikonomia-backup";

/// Default native-save filename: `oikonomia-backup-YYYY-MM-DD.oikonomia-backup`.
///
/// Uses the local calendar date so consecutive daily backups sort in Finder.
#[must_use]
pub fn default_backup_file_name() -> String {
    format!("oikonomia-backup-{}.{BACKUP_EXTENSION}", local_iso_date())
}

fn local_iso_date() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    crate::util::format_date(now.date())
}

const MEMBER_DB: &str = "vault.db";
const MEMBER_HEADER: &str = "vault.header.json";
const RESTORE_DB_TMP: &str = "vault.db.restore-tmp";
const RESTORE_HEADER_TMP: &str = "vault.header.json.restore-tmp";
const SNAPSHOT_DB_TMP: &str = "vault.db.backup-tmp";

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
/// present; [`Error::Io`] on filesystem failures.
pub fn backup_to_path(data_dir: &Path, dest: &Path) -> Result<()> {
    let header_path = vault_header_path(data_dir);
    let db_path = vault_db_path(data_dir);
    ensure_vault_files(&header_path, &db_path)?;
    write_archive_from_paths(&header_path, &db_path, dest)
}

/// Unpack a backup archive into `data_dir`.
///
/// Decrypt is not required: members are ciphertext plus the public header.
/// Existing `vault.db` / `vault.header.json` are left untouched unless
/// `replace` is true. WAL/SHM sidecars and `vault.header.json.tmp` are
/// removed after a successful replace so unlock cannot mix old recovery
/// state with restored files.
///
/// Staging files are written next to the vault, then renamed into place.
/// `replace` is required only when a vault already exists; an uninitialized
/// data directory accepts `replace: false`.
///
/// # Errors
///
/// [`Error::BackupInvalid`] for a non-archive, truncated, or incomplete
/// file; [`Error::RestoreWouldOverwrite`] when vault files exist and
/// `replace` is false; [`Error::Io`] on filesystem failures.
pub fn restore_from_path(archive: &Path, data_dir: &Path, replace: bool) -> Result<()> {
    fs::create_dir_all(data_dir).map_err(|err| Error::Io(err.to_string()))?;

    let header_dest = vault_header_path(data_dir);
    let db_dest = vault_db_path(data_dir);
    let header_tmp = data_dir.join(RESTORE_HEADER_TMP);
    let db_tmp = data_dir.join(RESTORE_DB_TMP);
    let _ = fs::remove_file(&header_tmp);
    let _ = fs::remove_file(&db_tmp);

    // Unpack to staging first so a bad archive is rejected even when a vault
    // already exists and `replace` is false. Staging names never clobber the
    // live files.
    let unpack = unpack_archive_to_staging(archive, &header_tmp, &db_tmp);
    if let Err(err) = unpack {
        let _ = fs::remove_file(&header_tmp);
        let _ = fs::remove_file(&db_tmp);
        return Err(err);
    }

    let vault_present = header_dest.exists() || db_dest.exists();
    if vault_present && !replace {
        let _ = fs::remove_file(&header_tmp);
        let _ = fs::remove_file(&db_tmp);
        return Err(Error::RestoreWouldOverwrite);
    }

    fs::rename(&header_tmp, &header_dest).map_err(|err| Error::Io(err.to_string()))?;
    fs::rename(&db_tmp, &db_dest).map_err(|err| Error::Io(err.to_string()))?;
    sync_parent(&header_dest);

    remove_db_sidecars(&db_dest);
    let _ = fs::remove_file(vault_staged_header_path(data_dir));
    Ok(())
}

impl Vault {
    /// Write a portable ciphertext archive to `dest` without changing lock state.
    ///
    /// Unlocked: `VACUUM INTO` a temp file (`SQLCipher` keeps the dest keyed with
    /// the live connection's key), then pack it with `vault.header.json`.
    /// Locked: copy the on-disk pair (no writer).
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
        (true, true) => Ok(()),
        (false, false) => Err(Error::VaultUninitialized),
        (true, false) => Err(Error::VaultCorrupt(
            "vault header exists without database".into(),
        )),
        (false, true) => Err(Error::VaultCorrupt(
            "vault database exists without header".into(),
        )),
    }
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

    let snap = data_dir.join(SNAPSHOT_DB_TMP);
    discard_snapshot(&snap);

    let vacuum = vacuum_into_encrypted(conn, &snap);
    if let Err(err) = vacuum {
        discard_snapshot(&snap);
        return Err(err);
    }

    let packed = write_archive_from_paths(&header_path, &snap, dest);
    discard_snapshot(&snap);
    packed
}

fn vacuum_into_encrypted(conn: &Connection, dest: &Path) -> Result<()> {
    let path = dest
        .to_str()
        .ok_or_else(|| Error::Io("backup snapshot path is not UTF-8".into()))?;
    let escaped = path.replace('\'', "''");
    conn.execute(&format!("VACUUM INTO '{escaped}'"), [])
        .map_err(|err| Error::Io(err.to_string()))?;
    reject_plaintext_sqlite(dest)
}

fn reject_plaintext_sqlite(path: &Path) -> Result<()> {
    let mut magic = [0u8; 6];
    let mut file = File::open(path).map_err(|err| Error::Io(err.to_string()))?;
    file.read_exact(&mut magic).map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            Error::Io("backup snapshot is empty".into())
        } else {
            Error::Io(err.to_string())
        }
    })?;
    if &magic == b"SQLite" {
        return Err(Error::Io(
            "online backup produced a plaintext database".into(),
        ));
    }
    Ok(())
}

fn discard_snapshot(path: &Path) {
    let _ = fs::remove_file(path);
    remove_db_sidecars(path);
}

fn write_archive_from_paths(header_path: &Path, db_path: &Path, dest: &Path) -> Result<()> {
    let tmp = sibling_temp(dest, ".tmp")?;
    let result = (|| {
        if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|err| Error::Io(err.to_string()))?;
        }

        let mut out = File::create(&tmp).map_err(|err| Error::Io(err.to_string()))?;
        out.write_all(MAGIC)
            .map_err(|err| Error::Io(err.to_string()))?;
        out.write_all(&FORMAT_VERSION.to_le_bytes())
            .map_err(|err| Error::Io(err.to_string()))?;
        write_member_from_path(&mut out, MEMBER_HEADER, header_path)?;
        write_member_from_path(&mut out, MEMBER_DB, db_path)?;
        out.sync_all().map_err(|err| Error::Io(err.to_string()))?;
        drop(out);

        fs::rename(&tmp, dest).map_err(|err| Error::Io(err.to_string()))?;
        sync_parent(dest);
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
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
    let mut file = File::create(dest).map_err(|err| Error::Io(err.to_string()))?;
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

fn sibling_temp(dest: &Path, suffix: &str) -> Result<PathBuf> {
    let name = dest
        .file_name()
        .ok_or_else(|| Error::Io("backup destination has no file name".into()))?;
    let mut tmp_name = name.to_os_string();
    tmp_name.push(suffix);
    let parent = dest.parent().filter(|p| !p.as_os_str().is_empty());
    Ok(match parent {
        Some(parent) => parent.join(tmp_name),
        None => PathBuf::from(tmp_name),
    })
}

fn sync_parent(path: &Path) {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
}

fn remove_db_sidecars(db_path: &Path) {
    let wal = PathBuf::from(format!("{}-wal", db_path.display()));
    let shm = PathBuf::from(format!("{}-shm", db_path.display()));
    let _ = fs::remove_file(wal);
    let _ = fs::remove_file(shm);
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
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::*;
    use crate::vault::VaultStatus;
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
