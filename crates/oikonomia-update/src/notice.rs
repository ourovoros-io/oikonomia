//! The "updated from X to Y" notice that survives the restart into a new
//! version.
//!
//! Two records in the app's config directory carry it across:
//!
//! - [`PENDING_MARKER_FILE`], written by the install just before the
//!   verified artifact is handed over ([`write_pending_marker`]). It names
//!   the version that ran the install and the version it installed.
//! - [`LAST_RUN_FILE`], the version of the last start, rewritten on every
//!   start ([`record_last_run`]). It covers an update the marker did not
//!   describe: one made by a version that wrote no marker, or by hand.
//!
//! [`take_notice`] reads both once. A notice is given only for the running
//! version and only when it is newer than the one it replaced, so a marker
//! left by an install that never completed (a Windows installer the user
//! cancelled) or by a downgrade shows nothing. The marker is removed every
//! time it is read, valid or not.
//!
//! Neither record is trusted for anything but this one line of text. They
//! hold version numbers, and a version that does not parse reads as absent.

use crate::version::parse_version;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file name of the marker an install writes before the handoff.
pub const PENDING_MARKER_FILE: &str = "update_pending.json";

/// The file name of the record of the version that last started.
pub const LAST_RUN_FILE: &str = "last_run_version";

/// The one-time notice that this copy was updated, as the webview receives
/// it: `{"from": "0.1.4", "to": "0.1.5"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateNotice {
    /// The version that ran before.
    from: String,
    /// The version running now.
    to: String,
}

/// The marker as it is written to disk.
#[derive(Debug, Serialize, Deserialize)]
struct PendingMarker {
    /// The version that ran the install.
    from: String,
    /// The version it installed.
    to: String,
}

/// Writes the marker saying that `from` is handing over to `to`, in
/// `config_dir`, replacing any marker there.
///
/// Atomic: the marker goes to a temporary file beside it, which is synced
/// and then renamed over [`PENDING_MARKER_FILE`], so a crash leaves either
/// the old marker or the new one, never half of one. The directory is
/// created when missing.
///
/// Call it only once the artifact has verified, and remove the marker with
/// [`remove_pending_marker`] when the handoff then fails.
///
/// # Errors
///
/// Returns the I/O error of creating the directory, writing or syncing the
/// temporary file, or renaming it.
pub fn write_pending_marker(
    config_dir: &Path,
    from: &Version,
    to: &Version,
) -> std::io::Result<()> {
    let marker = PendingMarker {
        from: from.to_string(),
        to: to.to_string(),
    };
    let body = serde_json::to_vec(&marker).map_err(std::io::Error::other)?;

    write_atomically(config_dir, PENDING_MARKER_FILE, &body)
}

/// Removes the marker from `config_dir`, if there is one.
///
/// Cleanup after a handoff that failed, so that a later start does not
/// claim an update that did not happen. A failure is logged, not returned.
pub fn remove_pending_marker(config_dir: &Path) {
    remove_if_present(&config_dir.join(PENDING_MARKER_FILE));
}

/// Records `running` as the version of this start in `config_dir`, and
/// returns the version the previous start recorded.
///
/// Called once on every start. `None` when there was no record, as on the
/// first start, or one that does not parse. A failure to write is logged,
/// not returned: the app runs the same without the record.
#[must_use]
pub fn record_last_run(config_dir: &Path, running: &Version) -> Option<Version> {
    let path = config_dir.join(LAST_RUN_FILE);
    let previous = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| parse_version(&text).ok());

    if let Err(error) = write_atomically(config_dir, LAST_RUN_FILE, running.to_string().as_bytes())
    {
        log::warn!(
            "last run version could not be written to {}: {error}",
            path.display()
        );
    }

    previous
}

/// Returns the notice that this copy was updated to `running`, once, and
/// removes the marker.
///
/// The marker in `config_dir` comes first: it is used when its `to` is
/// `running` and its `from` is older. Otherwise `last_run`, the version
/// [`record_last_run`] returned at this start, is used when it is older
/// than `running`. Anything else is no notice.
///
/// The marker is removed whatever it held, so it is read once. `last_run`
/// is the caller's to forget: pass it once.
#[must_use]
pub fn take_notice(
    config_dir: &Path,
    running: &Version,
    last_run: Option<&Version>,
) -> Option<UpdateNotice> {
    let path = config_dir.join(PENDING_MARKER_FILE);
    let marker = std::fs::read(&path).ok();
    if marker.is_some() {
        remove_if_present(&path);
    }

    let from_marker = marker
        .and_then(|body| serde_json::from_slice::<PendingMarker>(&body).ok())
        .and_then(|marker| {
            let from = parse_version(&marker.from).ok()?;
            let to = parse_version(&marker.to).ok()?;
            (to == *running).then_some(from)
        });

    let from = from_marker
        .filter(|from| from < running)
        .or_else(|| last_run.filter(|from| *from < running).cloned())?;

    Some(UpdateNotice {
        from: from.to_string(),
        to: running.to_string(),
    })
}

/// Writes `body` to `name` in `dir` through a temporary file and a rename.
fn write_atomically(dir: &Path, name: &str, body: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    std::fs::create_dir_all(dir)?;
    let destination = dir.join(name);
    let staging: PathBuf = dir.join(format!("{name}.{}.tmp", std::process::id()));

    let written = std::fs::File::create(&staging)
        .and_then(|mut file| {
            file.write_all(body)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&staging, &destination));
    if written.is_err() {
        remove_if_present(&staging);
    }

    written
}

/// Removes the file at `path`; a file that is already gone is not a
/// failure, and any other failure is logged.
fn remove_if_present(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("could not remove {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LAST_RUN_FILE, PENDING_MARKER_FILE, record_last_run, remove_pending_marker, take_notice,
        write_pending_marker,
    };
    use semver::Version;

    fn version(text: &str) -> Version {
        Version::parse(text).expect("version")
    }

    #[test]
    fn a_marker_for_the_running_version_gives_the_notice_once() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write_pending_marker(dir.path(), &version("0.1.4"), &version("0.1.5")).expect("marker");

        let first = take_notice(dir.path(), &version("0.1.5"), None).expect("a notice");
        let second = take_notice(dir.path(), &version("0.1.5"), None);

        assert_eq!(
            serde_json::to_value(&first).expect("json"),
            serde_json::json!({ "from": "0.1.4", "to": "0.1.5" })
        );
        assert_eq!(second, None);
        assert!(!dir.path().join(PENDING_MARKER_FILE).exists());
    }

    #[test]
    fn a_marker_for_another_version_is_deleted_without_a_notice() {
        let dir = tempfile::tempdir().expect("temporary directory");
        // A Windows installer the user cancelled: 0.1.4 is still running.
        write_pending_marker(dir.path(), &version("0.1.4"), &version("0.1.5")).expect("marker");

        assert_eq!(take_notice(dir.path(), &version("0.1.4"), None), None);
        assert!(!dir.path().join(PENDING_MARKER_FILE).exists());
    }

    #[test]
    fn a_marker_that_is_not_an_upgrade_or_not_a_marker_is_deleted_without_a_notice() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write_pending_marker(dir.path(), &version("0.1.6"), &version("0.1.5")).expect("marker");
        assert_eq!(take_notice(dir.path(), &version("0.1.5"), None), None);

        std::fs::write(dir.path().join(PENDING_MARKER_FILE), b"{not json").expect("garbage");
        assert_eq!(take_notice(dir.path(), &version("0.1.5"), None), None);
        assert!(!dir.path().join(PENDING_MARKER_FILE).exists());
    }

    #[test]
    fn without_a_marker_the_last_run_version_gives_the_notice() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let running = version("0.1.4");
        std::fs::write(dir.path().join(LAST_RUN_FILE), "0.1.3\n").expect("record");

        let previous = record_last_run(dir.path(), &running);
        let notice = take_notice(dir.path(), &running, previous.as_ref()).expect("a notice");

        assert_eq!(
            (notice.from.as_str(), notice.to.as_str()),
            ("0.1.3", "0.1.4")
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(LAST_RUN_FILE)).expect("record"),
            "0.1.4"
        );
        // The next start finds its own version recorded and gives none.
        let next = record_last_run(dir.path(), &running);
        assert_eq!(take_notice(dir.path(), &running, next.as_ref()), None);
    }

    #[test]
    fn a_first_start_or_a_downgrade_gives_no_notice() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let running = version("0.1.4");

        assert_eq!(record_last_run(dir.path(), &running), None);
        assert_eq!(take_notice(dir.path(), &running, None), None);
        assert_eq!(
            take_notice(dir.path(), &running, Some(&version("0.1.5"))),
            None
        );
    }

    #[test]
    fn a_mismatched_marker_does_not_hide_the_last_run_notice() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write_pending_marker(dir.path(), &version("0.1.4"), &version("0.1.9")).expect("marker");

        let notice = take_notice(dir.path(), &version("0.1.5"), Some(&version("0.1.4")))
            .expect("the last run notice");

        assert_eq!(
            (notice.from.as_str(), notice.to.as_str()),
            ("0.1.4", "0.1.5")
        );
    }

    #[test]
    fn the_marker_is_written_whole_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let config = dir.path().join("not-yet-created");

        write_pending_marker(&config, &version("0.1.4"), &version("0.1.5")).expect("marker");
        write_pending_marker(&config, &version("0.1.4"), &version("0.1.6")).expect("again");

        let names: Vec<String> = std::fs::read_dir(&config)
            .expect("list")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec![PENDING_MARKER_FILE.to_owned()]);
        let body: serde_json::Value =
            serde_json::from_slice(&std::fs::read(config.join(PENDING_MARKER_FILE)).expect("read"))
                .expect("json");
        assert_eq!(body, serde_json::json!({ "from": "0.1.4", "to": "0.1.6" }));

        remove_pending_marker(&config);
        remove_pending_marker(&config);
        assert!(!config.join(PENDING_MARKER_FILE).exists());
    }
}
