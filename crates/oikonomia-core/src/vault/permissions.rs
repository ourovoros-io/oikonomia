//! Owner-only modes for vault files.
//!
//! On Unix the data directory is `0700` and every vault file `0600`, so no
//! other local account can copy the ciphertext for an offline password
//! search. Windows relies on the per-user ACL of the `AppData` directory.
//!
//! A new file gets that mode at creation, not afterwards. A file that
//! `SQLite` is about to create (the database, a backup snapshot) is created
//! empty here first, because `SQLite` itself would create it under the umask.
//!
//! Creating with the mode is what the guarantee rests on, and it fails the
//! operation when it fails. Tightening something that already exists
//! (`restrict_to_owner`) is best effort and only logs: a filesystem that
//! cannot hold a mode must not make an existing vault unopenable.

use std::fs::{DirBuilder, File, OpenOptions};
use std::path::Path;

use crate::error::{IoContext, Result};

/// Mode of the data directory: owner may list, enter and write.
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;
/// Mode of every vault file: owner may read and write.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

/// Creates `dir` and any missing parents, readable only by the owner.
///
/// An existing directory is tightened to the same mode. Existing parents are
/// left as they are.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) when the directory cannot be created, including when `dir`
/// exists and is not a directory.
pub(crate) fn create_private_dir(dir: &Path) -> Result<()> {
    private_dir_builder()
        .create(dir)
        .io("create private directory")?;
    restrict_to_owner(dir);
    Ok(())
}

/// Creates or truncates `path` as a file only the owner can read, and
/// returns it open for writing.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) when the file cannot be created or opened for writing.
pub(crate) fn create_private_file(path: &Path) -> Result<File> {
    let file = private_file_options()
        .open(path)
        .io("create private file")?;

    // The creation mode does not apply to a stale file left by a crash.
    restrict_to_owner(path);
    Ok(file)
}

/// Restricts an existing file or directory to its owner.
///
/// Best effort: a filesystem that cannot hold the mode (a FAT stick, a file
/// owned by someone else) must not make the vault unusable, so a failure is
/// logged instead of returned.
#[cfg(unix)]
pub(crate) fn restrict_to_owner(path: &Path) {
    if let Err(err) = set_owner_only_mode(path) {
        // The path may be a backup destination the user chose.
        log::warn!(
            "could not restrict {} to its owner: {err}",
            crate::error::PrivateDetail(path.display())
        );
    }
}

/// Does nothing: Windows has no file modes, and the per-user ACL of
/// `AppData` already keeps other accounts out.
#[cfg(not(unix))]
pub(crate) fn restrict_to_owner(_path: &Path) {}

/// Returns a recursive directory builder that creates at [`DIR_MODE`].
#[cfg(unix)]
fn private_dir_builder() -> DirBuilder {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(DIR_MODE);
    builder
}

/// Returns a recursive directory builder; there is no mode to set outside
/// Unix.
#[cfg(not(unix))]
fn private_dir_builder() -> DirBuilder {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    builder
}

/// Returns options that create or truncate a file for writing at
/// [`FILE_MODE`].
///
/// The mode applies only when the file is created, which is why
/// [`create_private_file`] tightens the path afterwards as well.
#[cfg(unix)]
fn private_file_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .truncate(true)
        .mode(FILE_MODE);
    options
}

/// Returns options that create or truncate a file for writing.
#[cfg(not(unix))]
fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    options
}

/// Sets `path` to [`DIR_MODE`] when it is a directory and to [`FILE_MODE`]
/// otherwise.
#[cfg(unix)]
fn set_owner_only_mode(path: &Path) -> std::io::Result<()> {
    use std::fs::{Permissions, set_permissions};
    use std::os::unix::fs::PermissionsExt;

    let mode = if path.is_dir() { DIR_MODE } else { FILE_MODE };
    set_permissions(path, Permissions::from_mode(mode))
}

/// Returns the permission bits of `path`, for the tests across the vault
/// that pin the owner-only modes.
#[cfg(test)]
#[cfg(unix)]
pub(crate) fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path).expect("the file under test exists");
    metadata.permissions().mode() & 0o777
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn private_dir_is_created_with_parents_at_owner_only_mode() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("outer").join("inner");

        create_private_dir(&nested).expect("create nested");

        assert_eq!(mode_of(&nested), 0o700);
        assert_eq!(mode_of(&dir.path().join("outer")), 0o700);
    }

    #[test]
    fn existing_dir_is_tightened() {
        let dir = tempdir().expect("tempdir");
        let loose = dir.path().join("loose");
        fs::create_dir(&loose).expect("create");
        fs::set_permissions(&loose, fs::Permissions::from_mode(0o755)).expect("loosen");

        create_private_dir(&loose).expect("tighten");

        assert_eq!(mode_of(&loose), 0o700);
    }

    #[test]
    fn private_file_truncates_and_tightens_a_stale_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("stale");
        fs::write(&path, b"old contents").expect("stale file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("loosen");

        let mut file = create_private_file(&path).expect("recreate");
        file.write_all(b"new").expect("write");
        drop(file);

        assert_eq!(fs::read(&path).expect("read"), b"new");
        assert_eq!(mode_of(&path), 0o600);
    }
}
