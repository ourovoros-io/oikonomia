//! The install as the webview follows it: download progress, the cancel,
//! and the installing report before the handoff.

use crate::client::ClientConfig;
use crate::client::{
    ArtifactInstaller, InstallHandoff, InstallOutcome, VerifiedOffer, install_offer_reporting,
};
use crate::error::UpdateError;
use crate::machine::UpdateMachine;
use crate::notice::{PENDING_MARKER_FILE, take_notice, write_pending_marker};
use crate::progress::{InstallControl, InstallProgress};
use crate::status::UpdateStatus;
use crate::tests::support::{
    available_offer, cache_dir, config_for_artifact_at, leftover_files, serve_zero_bytes,
    serve_zero_bytes_without_length, spy, test_keys,
};
use httptest::Server;
use semver::Version;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What happened during an install, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    /// A progress report.
    Progress(InstallProgress),
    /// The installer was called; `marker` says whether the update marker
    /// was on disk at that moment.
    Installer {
        /// Whether the marker existed when the installer was called.
        marker: bool,
    },
}

/// An installer that does what the desktop does around the handoff: writes
/// the update marker, then records that it ran and reports `handoff`.
struct MarkingInstaller {
    /// The config directory the marker is written to.
    config_dir: PathBuf,
    /// The shared event log.
    events: Arc<Mutex<Vec<Event>>>,
    /// The control of the install, to try a cancel from inside the handoff.
    control: InstallControl,
    /// Whether a cancel tried from inside the handoff succeeded.
    late_cancel: Mutex<Option<bool>>,
}

impl ArtifactInstaller for MarkingInstaller {
    fn install(&self, _artifact: &Path) -> crate::Result<InstallHandoff> {
        write_pending_marker(
            &self.config_dir,
            &Version::new(0, 1, 0),
            &Version::new(0, 2, 0),
        )
        .map_err(UpdateError::CacheIo)?;
        *self.late_cancel.lock().expect("late cancel") = Some(self.control.cancel());
        self.events.lock().expect("events").push(Event::Installer {
            marker: self.config_dir.join(PENDING_MARKER_FILE).exists(),
        });
        Ok(InstallHandoff::Replaced)
    }
}

/// A served 0.2.0 release of `size` zero bytes written `piece` at a time
/// with `pause` before each, with or without a `Content-Length`.
struct SlowRelease {
    /// The config for an app at 0.1.0.
    config: ClientConfig,
    /// The offer of the release.
    offer: VerifiedOffer,
    /// The feed server; kept alive for the test.
    _server: Server,
    /// The updater cache directory; kept alive for the test.
    cache: tempfile::TempDir,
}

fn slow_release(size: usize, piece: usize, pause: Duration, announce_length: bool) -> SlowRelease {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let payload = vec![0_u8; size];
    let address = if announce_length {
        serve_zero_bytes(size, piece, pause)
    } else {
        serve_zero_bytes_without_length(size, piece, pause)
    };
    let config = config_for_artifact_at(
        &server,
        (&public_key, &secret_key),
        address,
        &payload,
        cache.path(),
    );
    let offer = available_offer(&config);

    SlowRelease {
        config,
        offer,
        _server: server,
        cache,
    }
}

/// Runs the install of `release` with a [`MarkingInstaller`] writing into
/// `config_dir`, calling `on_progress` with each report as well, and
/// returns the outcome, the events and whether a cancel from inside the
/// handoff succeeded.
fn run_install(
    release: &SlowRelease,
    config_dir: &Path,
    control: &InstallControl,
    mut on_progress: impl FnMut(&InstallProgress),
) -> (InstallOutcome, Vec<Event>, Option<bool>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let installer = MarkingInstaller {
        config_dir: config_dir.to_path_buf(),
        events: Arc::clone(&events),
        control: control.clone(),
        late_cancel: Mutex::new(None),
    };
    let log = Arc::clone(&events);
    let mut progress = move |report: InstallProgress| {
        on_progress(&report);
        log.lock().expect("events").push(Event::Progress(report));
    };

    let outcome = install_offer_reporting(
        &release.config,
        &release.offer,
        &installer,
        control,
        &mut progress,
    );
    let events = events.lock().expect("events").clone();
    let late_cancel = *installer.late_cancel.lock().expect("late cancel");
    (outcome, events, late_cancel)
}

/// Returns the `received` of each downloading report in `events`, with
/// the totals they carried.
fn downloads(events: &[Event]) -> Vec<(u64, Option<u64>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Progress(InstallProgress::Downloading { received, total }) => {
                Some((*received, *total))
            }
            Event::Progress(InstallProgress::Installing) | Event::Installer { .. } => None,
        })
        .collect()
}

#[test]
fn progress_rises_to_the_whole_artifact_then_installing_comes_before_the_handoff() {
    let release = slow_release(4096, 512, Duration::from_millis(30), true);
    let config_dir = tempfile::tempdir().expect("config directory");

    let (outcome, events, late_cancel) =
        run_install(&release, config_dir.path(), &InstallControl::new(), |_| {});

    assert!(
        matches!(outcome, InstallOutcome::Installed(InstallHandoff::Replaced)),
        "{outcome:?}"
    );
    let downloads = downloads(&events);
    assert_eq!(downloads.first(), Some(&(0, Some(4096))), "{events:?}");
    assert_eq!(downloads.last(), Some(&(4096, Some(4096))), "{events:?}");
    assert!(
        downloads.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "received only rises and is never repeated: {downloads:?}"
    );
    let tail = &events[events.len() - 2..];
    assert_eq!(
        tail,
        [
            Event::Progress(InstallProgress::Installing),
            Event::Installer { marker: true },
        ]
    );
    assert_eq!(late_cancel, Some(false), "a cancel once installing began");
}

#[test]
fn a_download_without_a_content_length_reports_no_total() {
    let release = slow_release(2048, 256, Duration::from_millis(5), false);
    let config_dir = tempfile::tempdir().expect("config directory");

    let (outcome, events, _) =
        run_install(&release, config_dir.path(), &InstallControl::new(), |_| {});

    assert!(
        matches!(outcome, InstallOutcome::Installed(_)),
        "{outcome:?}"
    );
    let downloads = downloads(&events);
    assert!(
        downloads.iter().all(|(_, total)| total.is_none()),
        "{downloads:?}"
    );
    assert_eq!(downloads.last(), Some(&(2048, None)));
}

#[test]
fn progress_is_sent_at_most_ten_times_a_second_plus_the_last() {
    // 100 pieces, 5 ms apart: about half a second of chunks.
    let release = slow_release(100 * 64, 64, Duration::from_millis(5), true);
    let config_dir = tempfile::tempdir().expect("config directory");

    let started = Instant::now();
    let (_, events, _) = run_install(&release, config_dir.path(), &InstallControl::new(), |_| {});
    let elapsed = started.elapsed();

    let sent = downloads(&events).len();
    // One per started 100 ms, plus the last report.
    let allowed = usize::try_from(elapsed.as_millis() / 100).expect("small") + 2;
    assert!(
        sent <= allowed,
        "{sent} reports in {elapsed:?}, at most {allowed} allowed"
    );
    assert!(sent >= 2, "the first and the last report at least: {sent}");
}

#[test]
fn a_cancel_during_the_download_stops_it_leaves_no_file_and_writes_no_marker() {
    let release = slow_release(64 * 1024, 1024, Duration::from_millis(10), true);
    let config_dir = tempfile::tempdir().expect("config directory");
    let control = InstallControl::new();
    let canceller = control.clone();
    let accepted = Arc::new(Mutex::new(None));
    let accepted_in = Arc::clone(&accepted);

    let (outcome, events, _) = run_install(&release, config_dir.path(), &control, move |report| {
        if let InstallProgress::Downloading { received, .. } = report
            && *received > 0
        {
            let mut accepted = accepted_in.lock().expect("accepted");
            if accepted.is_none() {
                *accepted = Some(canceller.cancel());
            }
        }
    });

    assert!(matches!(outcome, InstallOutcome::Cancelled), "{outcome:?}");
    assert_eq!(*accepted.lock().expect("accepted"), Some(true));
    assert!(
        !events.iter().any(|event| matches!(
            event,
            Event::Progress(InstallProgress::Installing) | Event::Installer { .. }
        )),
        "{events:?}"
    );
    let (received, _) = *downloads(&events).last().expect("a report");
    assert!(received < 64 * 1024, "the download stopped early");
    assert_eq!(leftover_files(release.cache.path()), Vec::<PathBuf>::new());
    assert!(!config_dir.path().join(PENDING_MARKER_FILE).exists());
    assert!(!control.cancel(), "nothing left to cancel");
}

#[test]
fn a_cancelled_install_hands_the_offer_back_to_the_machine() {
    let release = slow_release(8 * 1024, 1024, Duration::from_millis(10), true);
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&release.config),
        UpdateStatus::Available { .. }
    ));
    let offer = machine.begin_install().expect("available");
    let control = InstallControl::new();
    assert!(control.cancel());
    let (installer, calls) = spy(false);

    let outcome =
        install_offer_reporting(&release.config, &offer, &installer, &control, &mut |_| {});
    machine.finish_install(&outcome, offer);

    assert!(matches!(outcome, InstallOutcome::Cancelled), "{outcome:?}");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(matches!(machine.status(), UpdateStatus::Available { .. }));
}

#[test]
fn an_artifact_that_fails_its_signature_reaches_no_marker_and_no_installing_report() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    // The manifest signs other bytes than the ones served.
    let address = serve_zero_bytes(1024, 256, Duration::ZERO);
    let config = config_for_artifact_at(
        &server,
        (&public_key, &secret_key),
        address,
        b"other bytes",
        cache.path(),
    );
    let release = SlowRelease {
        offer: available_offer(&config),
        config,
        _server: server,
        cache,
    };
    let config_dir = tempfile::tempdir().expect("config directory");

    let (outcome, events, _) =
        run_install(&release, config_dir.path(), &InstallControl::new(), |_| {});

    assert!(
        matches!(
            outcome,
            InstallOutcome::Failed(UpdateError::ArtifactIntegrity)
        ),
        "{outcome:?}"
    );
    assert_eq!(downloads(&events).last(), Some(&(1024, Some(1024))));
    assert!(
        !events.iter().any(|event| matches!(
            event,
            Event::Progress(InstallProgress::Installing) | Event::Installer { .. }
        )),
        "{events:?}"
    );
    assert!(!config_dir.path().join(PENDING_MARKER_FILE).exists());
    assert_eq!(
        take_notice(config_dir.path(), &Version::new(0, 2, 0), None),
        None
    );
}

#[test]
fn the_marker_of_a_completed_install_gives_the_notice_after_the_restart() {
    let release = slow_release(1024, 1024, Duration::ZERO, true);
    let config_dir = tempfile::tempdir().expect("config directory");

    let (outcome, _, _) = run_install(&release, config_dir.path(), &InstallControl::new(), |_| {});
    assert!(
        matches!(outcome, InstallOutcome::Installed(_)),
        "{outcome:?}"
    );

    // What the new version finds when it starts.
    let notice = take_notice(config_dir.path(), &Version::new(0, 2, 0), None).expect("notice");
    assert_eq!(
        serde_json::to_value(&notice).expect("json"),
        serde_json::json!({ "from": "0.1.0", "to": "0.2.0" })
    );
    assert_eq!(
        take_notice(config_dir.path(), &Version::new(0, 2, 0), None),
        None
    );
}
