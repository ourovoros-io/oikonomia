//! File writes and removals shared by the vault, its backups and the exports.
//!
//! A file that must not be seen half-written is written under a sibling name,
//! flushed, and renamed over its destination; the rename is what makes the
//! new contents visible, so a crash leaves either the old file or the new
//! one. The sibling sits in the destination's directory because a rename is
//! only atomic within one filesystem.
//!
//! Two kinds of removal are kept apart by name. `remove_*` returns the
//! error, for a step whose failure must stop the caller. `discard_*` logs
//! it, for cleanup on a path that is already returning an error or that
//! leaves nothing worse than a stale file behind.
//!
//! Every write here creates its file through `vault::permissions`, so a
//! file this module creates is owner-only before its first byte is written.
//!
//! `local_iso_date` is here as well: the backup and the export, the two
//! callers outside the vault proper, both stamp their default file name
//! with it.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use time::OffsetDateTime;

use crate::error::{Error, IoContext, PrivateDetail, Result};
use crate::util::format_date;
use crate::vault::paths::{STAGED_SUFFIX, db_sidecar_paths, with_appended};
use crate::vault::permissions::create_private_file;

/// Returns `path` with `suffix` appended to its file name.
///
/// # Errors
///
/// [`Error::Io`] when `path` has no file name to append to (`..`, `/`).
pub(crate) fn sibling_path(path: &Path, suffix: &str) -> Result<PathBuf> {
    if path.file_name().is_none() {
        return Err(Error::io(
            "name staged file",
            format_args!("destination has no file name: {}", path.display()),
        ));
    }
    Ok(with_appended(path, suffix))
}

/// Creates or truncates the owner-only file `path`, writes `bytes` and
/// flushes both the file and its directory entry to disk.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be created, written or flushed.
pub(crate) fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = create_private_file(path)?;
    file.write_all(bytes).io("write private file")?;
    file.sync_all().io("sync private file")?;
    sync_parent_dir(path);
    Ok(())
}

/// Replaces `dest` with an owner-only file holding `bytes`.
///
/// The bytes go to `dest` + [`STAGED_SUFFIX`] first and are renamed into
/// place, so a reader of `dest` never sees a partial file, and a failure
/// leaves whatever `dest` held before.
///
/// # Errors
///
/// [`Error::Io`] when `dest` has no file name or the write or rename fails.
pub(crate) fn replace_private_file(dest: &Path, bytes: &[u8]) -> Result<()> {
    let staged = sibling_path(dest, STAGED_SUFFIX)?;

    let replaced = write_private_file(&staged, bytes).and_then(|()| rename_synced(&staged, dest));
    if replaced.is_err() {
        discard_file(&staged);
    }
    replaced
}

/// Renames `from` over `to` and flushes the directory so the rename survives
/// a crash.
///
/// # Errors
///
/// [`Error::Io`] when the rename fails, including when `from` does not exist.
pub(crate) fn rename_synced(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).io("rename file into place")?;
    sync_parent_dir(to);
    Ok(())
}

/// Flushes the directory holding `path`, so a create, rename or removal of
/// `path` survives a crash.
///
/// Best effort, because there is nothing useful to do about a failure: the
/// operation itself already succeeded, and Windows cannot open a directory
/// as a file at all (there the filesystem journals the entry itself).
pub(crate) fn sync_parent_dir(path: &Path) {
    let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) else {
        return;
    };
    match File::open(parent).and_then(|dir| dir.sync_all()) {
        Ok(()) => {}
        Err(err) => log::debug!(
            "could not flush directory {}: {err}",
            PrivateDetail(parent.display())
        ),
    }
}

/// Removes `path`; a file that is already gone counts as removed.
///
/// # Errors
///
/// The error of the removal when `path` exists and cannot be removed.
pub(crate) fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Renames `from` to `to`; a `from` that does not exist is left as a no-op,
/// for steps that are repeated when an interrupted sequence is resumed.
///
/// # Errors
///
/// [`Error::Io`] when `from` exists and cannot be renamed.
pub(crate) fn rename_if_present(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(Error::io(
            "rename vault file",
            format_args!("{} to {}: {err}", from.display(), to.display()),
        )),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Removes each of `paths` that exists.
///
/// # Errors
///
/// [`Error::Io`] naming the first file that exists and cannot be removed.
pub(crate) fn remove_files_if_present(paths: &[&Path]) -> Result<()> {
    for path in paths {
        remove_file_if_present(path).map_err(|err| {
            Error::io(
                "remove vault file",
                format_args!("{}: {err}", path.display()),
            )
        })?;
    }
    Ok(())
}

/// Removes a leftover file, logging a failure instead of returning it.
///
/// For a caller with no error to return it through: cleanup after a failure
/// that is already being reported, or of a stale file the next write
/// truncates anyway.
pub(crate) fn discard_file(path: &Path) {
    if let Err(err) = remove_file_if_present(path) {
        // The path may be beside a backup destination the user chose.
        log::warn!("could not remove {}: {err}", PrivateDetail(path.display()));
    }
}

/// Discards the database at `db_path` together with its WAL and SHM
/// sidecars, under the same terms as [`discard_file`].
pub(crate) fn discard_database_files(db_path: &Path) {
    discard_file(db_path);
    for sidecar in db_sidecar_paths(db_path) {
        discard_file(&sidecar);
    }
}

/// Returns today's date as `YYYY-MM-DD`, for the default names of backup and
/// export files.
///
/// The date is the local one, so a file saved late in the evening carries
/// the day the user sees on the clock. `time` refuses to read the local
/// offset where doing so is unsound (`OffsetDateTime::now_local` returns
/// `IndeterminateOffset`, on Linux in a process with more than one thread);
/// the UTC date is used then.
pub(crate) fn local_iso_date() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    format_date(now.date())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn sibling_path_appends_to_the_file_name() {
        let sibling = sibling_path(Path::new("books/journal.csv"), ".tmp").expect("sibling");
        assert_eq!(sibling, PathBuf::from("books/journal.csv.tmp"));

        let bare = sibling_path(Path::new("journal"), ".tmp").expect("bare name");
        assert_eq!(bare, PathBuf::from("journal.tmp"));
    }

    #[test]
    fn sibling_path_needs_a_file_name() {
        for path in ["..", "/", ""] {
            let err = sibling_path(Path::new(path), ".tmp").expect_err(path);
            assert!(matches!(err, Error::Io { .. }), "{path:?} gave {err:?}");
        }
    }

    #[test]
    fn removing_a_missing_file_is_not_an_error() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("absent");

        remove_file_if_present(&path).expect("a missing file counts as removed");
    }

    #[test]
    fn removing_a_directory_as_a_file_is_an_error() {
        let dir = tempdir().expect("tempdir");

        remove_file_if_present(dir.path()).expect_err("a directory is not a removable file");
    }

    #[test]
    fn discard_database_files_removes_the_database_and_its_sidecars() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("vault.db");
        let [wal, shm] = db_sidecar_paths(&db_path);
        for path in [&db_path, &wal, &shm] {
            fs::write(path, b"x").expect("write");
        }

        discard_database_files(&db_path);

        for path in [&db_path, &wal, &shm] {
            assert!(!path.exists(), "{} must be gone", path.display());
        }
    }
}
