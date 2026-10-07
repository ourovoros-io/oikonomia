//! The download cap the update client enforces, shared with the release gate.
//!
//! `client::read_capped` stops when another read would make the buffer longer
//! than [`MAX_ARTIFACT_BYTES`]. A release artifact that is already over that
//! line can never be installed in-app, so the release tooling asks
//! [`check_artifact_file`] before publishing.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// The largest installer body, in bytes, that the update client will
/// download: 200 MiB.
///
/// The download accepts a body of exactly this length and refuses one byte
/// more. The macOS archive, the Linux `AppImage` and a Windows NSIS setup
/// that embeds the `WebView2` bootstrapper fit under this cap. The client
/// holds the artifact in memory until its digest and signature are checked,
/// so this is also the most memory an install takes.
pub const MAX_ARTIFACT_BYTES: usize = 200 * 1024 * 1024;

/// A file the update client would not finish downloading.
///
/// Exhaustive, like the other public enums of this unpublished crate.
#[derive(Debug, Error)]
pub enum ArtifactSizeError {
    /// The path's metadata could not be read.
    #[error("{path}: {source}")]
    Metadata {
        /// Path that was stated.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: std::io::Error,
    },

    /// The path exists and is not a regular file. A directory or a symlink
    /// is not an installer the client can download.
    #[error("{path} is not a regular file")]
    NotAFile {
        /// Path that was rejected.
        path: PathBuf,
    },

    /// [`MAX_ARTIFACT_BYTES`] does not fit in a `u64` file length, so no
    /// size can be compared with it. The check refuses the file.
    #[error("the update client's download limit does not fit in a file length")]
    LimitUnrepresentable,

    /// The file is longer than the client will download.
    #[error("{path} is {len} bytes; the update client downloads at most {limit} bytes")]
    OverLimit {
        /// Path that was too large.
        path: PathBuf,
        /// Length reported by the filesystem.
        len: u64,
        /// [`MAX_ARTIFACT_BYTES`] as a file length.
        limit: u64,
    },
}

/// Refuses an updater artifact the client would not finish downloading.
///
/// Returns the file's length when `path` is a regular file of at most
/// [`MAX_ARTIFACT_BYTES`] bytes. The comparison matches the download reader:
/// a file of exactly the cap is accepted.
///
/// # Errors
///
/// Returns [`ArtifactSizeError::Metadata`] when `path` cannot be examined,
/// [`ArtifactSizeError::NotAFile`] when it is a directory or a symbolic
/// link, [`ArtifactSizeError::LimitUnrepresentable`] when the cap does not
/// fit in a file length, and [`ArtifactSizeError::OverLimit`] when the file
/// is longer than the cap.
pub fn check_artifact_file(path: &Path) -> Result<u64, ArtifactSizeError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|source| ArtifactSizeError::Metadata {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(ArtifactSizeError::NotAFile {
            path: path.to_path_buf(),
        });
    }

    let len = metadata.len();
    let Some(limit) = u64::try_from(MAX_ARTIFACT_BYTES).ok() else {
        return Err(ArtifactSizeError::LimitUnrepresentable);
    };
    if len > limit {
        return Err(ArtifactSizeError::OverLimit {
            path: path.to_path_buf(),
            len,
            limit,
        });
    }

    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::{MAX_ARTIFACT_BYTES, check_artifact_file};

    fn limit() -> u64 {
        u64::try_from(MAX_ARTIFACT_BYTES).expect("the cap fits in a file length")
    }

    fn file_of_length(path: &std::path::Path, len: u64) {
        let file = std::fs::File::create(path).expect("create");
        file.set_len(len).expect("set length");
    }

    #[test]
    fn an_at_cap_file_passes_and_one_extra_byte_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let at_cap = dir.path().join("Oikonomia.app.tar.gz");
        let over = dir.path().join("Oikonomia_0.2.0_x64-setup.exe");
        file_of_length(&at_cap, limit());
        file_of_length(&over, limit() + 1);

        assert_eq!(check_artifact_file(&at_cap).expect("at cap"), limit());

        let error = check_artifact_file(&over).expect_err("one byte over");
        let message = error.to_string();
        assert!(message.contains(&(limit() + 1).to_string()), "{message}");
        assert!(message.contains(&limit().to_string()), "{message}");
        assert!(message.contains("x64-setup.exe"), "{message}");
    }

    #[test]
    fn a_missing_path_and_a_directory_fail() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("gone.app.tar.gz");

        let missing_error = check_artifact_file(&missing).expect_err("missing");
        assert!(
            missing_error.to_string().contains("gone.app.tar.gz"),
            "{missing_error}"
        );

        let directory_error = check_artifact_file(dir.path()).expect_err("directory");
        assert!(
            directory_error.to_string().contains("not a regular file"),
            "{directory_error}"
        );
    }
}
