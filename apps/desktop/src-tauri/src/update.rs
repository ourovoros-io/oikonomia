//! The update commands: `update_check`, `update_install`, `update_cancel`
//! and `update_take_notice`.
//!
//! This is the app's only path to the network, and it runs only on the
//! user's click: nothing here starts a check when the app starts.
//!
//! # What the webview sees of an install
//!
//! `update_install` takes a channel and sends it [`InstallProgress`]
//! reports: `downloading` with the bytes received and the `Content-Length`
//! when the server sent one, at most ten a second plus a last one, then one
//! `installing`. The `installing` report stays on screen for at least
//! [`INSTALLING_HOLD`] before the handoff, so the window does not vanish
//! the instant the download ends. `update_cancel` stops a download until
//! the moment `installing` is sent. After the restart, `update_take_notice`
//! returns the one-time "updated from X to Y" notice.
//!
//! The webview supplies nothing. The feed URL, the endpoint and the public
//! key are compiled in (`oikonomia-update`, `crate::update_key`), so a
//! compromised page cannot point the updater elsewhere. HTTP is `ureq`
//! inside `oikonomia-update`, called on the blocking pool. An install hands
//! the downloaded and verified file to
//! [`crate::update_exec::VerifiedPathInstaller`], which replaces the running
//! copy.

use crate::commands::run_blocking;
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use crate::update_exec::{InstallKind, VerifiedPathInstaller};
use crate::update_key::UPDATER_PUBLIC_KEY;
use oikonomia_update::{
    ArtifactInstaller, CheckOutcome, CheckStart, ClientConfig, InstallControl, InstallHandoff,
    InstallOutcome, InstallProgress, UpdateError, UpdateMachine, UpdateNotice, UpdateStatus,
    VerifiedOffer, install_offer_reporting, perform_check, record_last_run, remove_pending_marker,
    take_notice, write_pending_marker,
};
use semver::Version;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{Manager, State};

/// How long the `installing` report is on screen, at least, before the
/// handoff restarts or closes the app.
///
/// The webview words the step ("the window will close and open again by
/// itself"); without a pause the window would close before it painted.
pub(crate) const INSTALLING_HOLD: Duration = Duration::from_millis(1500);

/// The updater's state besides the machine: the cancel switch of the
/// install in flight, and what the notice after an update is built from.
///
/// Managed by Tauri beside [`AppState`]; built once at start by
/// [`UpdaterState::at_start`].
#[derive(Debug)]
pub(crate) struct UpdaterState {
    /// The switch of the install in flight, or `None` when none is.
    install: Mutex<Option<InstallControl>>,
    /// Where the update marker and the last-run record live: the app's
    /// config directory, or `None` when the system names none.
    config_dir: Option<PathBuf>,
    /// The version the previous start recorded, until the notice takes it.
    last_run: Mutex<Option<Version>>,
}

impl UpdaterState {
    /// Records this start's version in `config_dir` and returns the state
    /// that remembers the previous one for the notice.
    pub(crate) fn at_start(config_dir: Option<PathBuf>) -> Self {
        let last_run = match (&config_dir, running_version()) {
            (Some(dir), Some(running)) => record_last_run(dir, &running),
            (None, _) | (_, None) => None,
        };

        Self {
            install: Mutex::new(None),
            config_dir,
            last_run: Mutex::new(last_run),
        }
    }

    /// Locks the install slot. It holds one value, which a panic cannot
    /// leave half written, so a poisoned lock is used as it is.
    fn install_slot(&self) -> MutexGuard<'_, Option<InstallControl>> {
        self.install.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Returns the version of this build, or `None` if it does not parse, which
/// the workspace version always does.
fn running_version() -> Option<Version> {
    Version::parse(env!("CARGO_PKG_VERSION")).ok()
}

/// Registers `control` as the install in flight for as long as the value
/// lives.
///
/// Only the first of two overlapping installs registers. The second is
/// refused by the machine a moment later, and must neither replace the
/// first one's switch nor clear it when it ends.
struct RegisteredInstall<'a> {
    /// The state the control is registered in.
    updater: &'a UpdaterState,
    /// Whether this value registered its control, and so clears the slot.
    registered: bool,
}

impl<'a> RegisteredInstall<'a> {
    /// Registers `control` in `updater`, unless another install is
    /// registered there.
    fn new(updater: &'a UpdaterState, control: InstallControl) -> Self {
        let mut slot = updater.install_slot();
        let registered = slot.is_none();
        if registered {
            *slot = Some(control);
        }

        Self {
            updater,
            registered,
        }
    }
}

impl Drop for RegisteredInstall<'_> {
    fn drop(&mut self) {
        if self.registered {
            *self.updater.install_slot() = None;
        }
    }
}

/// Returns where a downloaded update waits to be installed: a directory of
/// its own under `app_cache_dir`, this user's cache location for the app as
/// Tauri's path resolver reports it.
///
/// Not the system temporary directory. On Linux that is shared by every
/// account, and whoever creates the directory first could swap the verified
/// file before it is installed. Not the vault data directory either.
///
/// # Errors
///
/// Returns `cache_dir_unavailable` when the system names no cache directory,
/// which is when `app_cache_dir` is an error.
fn updater_cache_dir(app_cache_dir: tauri::Result<PathBuf>) -> CommandResult<PathBuf> {
    let cache = app_cache_dir.map_err(|err| {
        CommandError::desktop(
            DesktopError::CacheDirUnavailable,
            format!("no cache directory for updates: {err}"),
        )
    })?;
    Ok(cache.join("updater"))
}

/// Checks for a newer version, on the user's click, and returns the update
/// status.
///
/// Needs no vault; the unlock screen offers it. Fetches `latest.json` and its
/// detached signature `latest.json.sig`, verifies the signature with the
/// compiled-in minisign key, and checks the artifact URL against the
/// allow-list. Does not download the artifact. A check that cannot complete
/// is the status `Failed` with the code of what stopped it, not an error. The
/// cause is logged here.
///
/// HTTP is `ureq` on the blocking pool, so the async runtime is not stalled.
///
/// # Errors
///
/// Returns `cache_dir_unavailable` when the system names no cache directory,
/// and `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn update_check(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<UpdateStatus> {
    let cache = updater_cache_dir(app.path().app_cache_dir())?;
    let version = env!("CARGO_PKG_VERSION").to_owned();

    run_check(state.update_machine(), move || {
        let route = InstallKind::detect().route();
        let outcome = match ClientConfig::production(UPDATER_PUBLIC_KEY, &version, cache, route) {
            Ok(config) => perform_check(&config),
            Err(error) => CheckOutcome::Failed(error),
        };
        if let CheckOutcome::Failed(error) = &outcome {
            log_failure("update check failed", error);
        }

        outcome
    })
    .await
}

/// Logs `error` under `context`, with its cause when it has one.
///
/// The update crate does not log how a check or an install ended; this is
/// where a failure is recorded. The message of an [`UpdateError`] leaves the cause out, and the
/// webview is sent only the code, so without the cause here a cache failure
/// would nowhere say which operation the system refused.
fn log_failure(context: &str, error: &UpdateError) {
    match std::error::Error::source(error) {
        Some(cause) => log::warn!("{context}: {error}: {cause}"),
        None => log::warn!("{context}: {error}"),
    }
}

/// Runs `check` on the blocking pool and applies its outcome to `machine`.
///
/// While an install is in flight the check is not run and the status stays
/// [`UpdateStatus::Installing`], which is what this returns.
///
/// The machine is locked only on the blocking pool, never by the task that
/// polls this future, so an async worker never waits on the mutex.
async fn run_check(
    machine: Arc<Mutex<UpdateMachine>>,
    check: impl FnOnce() -> CheckOutcome + Send + 'static,
) -> CommandResult<UpdateStatus> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let Some(pending) = PendingCheck::begin(&machine) else {
            return crate::state::lock_update(&machine).status();
        };
        let outcome = check();
        pending.finish(outcome)
    })
    .await;

    joined.map_err(|err| {
        CommandError::desktop(
            DesktopError::TaskFailed,
            format!("background task failed: {err}"),
        )
    })
}

/// A check that has begun on the machine and has to end on it.
///
/// Dropped unfinished, which happens when the check panics, it abandons the
/// check, and the machine reads failed with no code. Otherwise the status
/// would stay `Checking` for the rest of the session, because nothing else
/// ends a check.
struct PendingCheck<'a> {
    /// The machine the check was begun on.
    machine: &'a Mutex<UpdateMachine>,
    /// Whether [`Self::finish`] has applied an outcome.
    finished: bool,
}

impl<'a> PendingCheck<'a> {
    /// Marks `machine` as checking, or returns `None` when it refuses a
    /// check because an install is in flight.
    fn begin(machine: &'a Mutex<UpdateMachine>) -> Option<Self> {
        match crate::state::lock_update(machine).begin_check() {
            CheckStart::Started => Some(Self {
                machine,
                finished: false,
            }),
            CheckStart::InstallInProgress => None,
        }
    }

    /// Applies `outcome` and returns the status it led to.
    fn finish(mut self, outcome: CheckOutcome) -> UpdateStatus {
        let mut machine = crate::state::lock_update(self.machine);
        machine.finish_check(outcome);
        self.finished = true;

        machine.status()
    }
}

impl Drop for PendingCheck<'_> {
    fn drop(&mut self) {
        if !self.finished {
            crate::state::lock_update(self.machine).abandon_check();
        }
    }
}

/// An install that has begun on the machine and has to end on it.
///
/// The machine is locked to begin and again to finish, and not in between:
/// the download can take minutes, and a check that arrives meanwhile must be
/// answered, not left waiting on the mutex.
///
/// Dropped unfinished, which happens when the install panics, it abandons
/// the install, and the machine reads failed with no code. Otherwise the
/// status would stay `Installing` for the rest of the session and refuse
/// every further check.
struct PendingInstall<'a> {
    /// The machine the install was begun on.
    machine: &'a Mutex<UpdateMachine>,
    /// Whether [`Self::finish`] has applied an outcome.
    finished: bool,
}

impl<'a> PendingInstall<'a> {
    /// Marks `machine` as installing and takes its offer, which goes back
    /// with [`Self::finish`].
    ///
    /// # Errors
    ///
    /// Returns [`oikonomia_update::UpdateError::InstallNotAvailable`] unless
    /// the machine holds an offer this copy may install.
    fn begin(machine: &'a Mutex<UpdateMachine>) -> oikonomia_update::Result<(Self, VerifiedOffer)> {
        let offer = crate::state::lock_update(machine).begin_install()?;

        Ok((
            Self {
                machine,
                finished: false,
            },
            offer,
        ))
    }

    /// Applies `outcome` to the machine and gives it back `offer`, which a
    /// cancelled install leaves installable.
    fn finish(mut self, outcome: &InstallOutcome, offer: VerifiedOffer) {
        crate::state::lock_update(self.machine).finish_install(outcome, offer);
        self.finished = true;
    }
}

impl Drop for PendingInstall<'_> {
    fn drop(&mut self) {
        if !self.finished {
            crate::state::lock_update(self.machine).abandon_install();
        }
    }
}

/// Installs the update the last check offered, and returns the update status.
///
/// Needs no vault. The command downloads the artifact, verifies its hash and
/// signature in memory, writes the verified file into the updater cache,
/// which is outside the vault data directory, and hands that file to the
/// installer for this copy ([`crate::update_exec`]). A copy replaced in place
/// restarts into the new version, so the command does not return. On Windows
/// the installer process replaces the files, so the app exits and the
/// installer starts the new version.
///
/// `on_progress` receives the [`InstallProgress`] reports the module
/// documentation describes.
///
/// Just before the handoff, once the artifact has verified, the update
/// marker is written to the config directory for [`update_take_notice`],
/// and the `installing` report is held for [`INSTALLING_HOLD`]. A handoff
/// that fails removes the marker again.
///
/// The command is refused with an error unless the status is
/// [`UpdateStatus::Available`]; it is never a silent no-op. An install that
/// was begun and then failed is not an error: it is the status `Failed` with
/// the code of what stopped it: `update_network` for the connection,
/// `update_artifact_integrity` for the digest or signature,
/// `update_cache_io` for the disk, `update_install_failed` for the
/// installer, among others. The cause is logged here. An install stopped by
/// [`update_cancel`] returns [`UpdateStatus::Cancelled`], and the update is
/// available to install again.
///
/// # Errors
///
/// Returns `update_install_not_allowed` unless the last check found an
/// update this copy may install, the code of the
/// [`UpdateError`] when the client configuration cannot be built,
/// `cache_dir_unavailable` when the system names no cache directory, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn update_install<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    updater: State<'_, UpdaterState>,
    on_progress: Channel<InstallProgress>,
) -> CommandResult<UpdateStatus> {
    let cache = updater_cache_dir(app.path().app_cache_dir())?;
    let machine = state.update_machine();
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let config_dir = updater.config_dir.clone();
    let control = InstallControl::new();
    let _registered = RegisteredInstall::new(&updater, control.clone());

    let outcome = run_blocking(move || {
        let kind = InstallKind::detect();
        let config = ClientConfig::production(UPDATER_PUBLIC_KEY, &version, cache, kind.route())?;
        let outcome = install_available_update(
            &machine,
            &config,
            &VerifiedPathInstaller::new(kind),
            &Handoff {
                config_dir: config_dir.as_deref(),
                hold: INSTALLING_HOLD,
            },
            Watch {
                control: &control,
                progress: &mut |report| forward_progress(&on_progress, report),
            },
        )?;
        Ok(outcome)
    })
    .await?;

    Ok(answer_install(&app, outcome))
}

/// Returns the status that answers the install that ended in `outcome`,
/// after restarting or closing the app when the installer took over.
///
/// Built from the outcome, not read back from the machine: a check may have
/// begun since, and its `Checking` is not the answer to this install.
fn answer_install<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    outcome: InstallOutcome,
) -> UpdateStatus {
    match outcome {
        InstallOutcome::Failed(error) => UpdateStatus::Failed {
            code: Some(error.code().to_owned()),
        },
        InstallOutcome::Cancelled => UpdateStatus::Cancelled,
        InstallOutcome::Installed(InstallHandoff::Replaced) => app.restart(),
        InstallOutcome::Installed(InstallHandoff::InstallerStarted) => {
            app.exit(0);
            UpdateStatus::Idle
        }
    }
}

/// Cancels the download of the install in flight, and returns whether it
/// did.
///
/// The download stops at its next chunk, leaves no file in the cache and
/// writes no update marker, and `update_install` returns
/// [`UpdateStatus::Cancelled`]. False when no install is in flight, or when
/// it has reported `installing`: from there the handoff runs to its end.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn update_cancel(updater: State<'_, UpdaterState>) -> bool {
    cancel_install(&updater)
}

/// The body of [`update_cancel`], without the Tauri handle.
fn cancel_install(updater: &UpdaterState) -> bool {
    updater
        .install_slot()
        .as_ref()
        .is_some_and(InstallControl::cancel)
}

/// Returns the notice that this copy was updated, once: `{from, to}`, or
/// `null`.
///
/// Built from the marker the install before the restart wrote, or, without
/// a usable one, from the version the previous start recorded. Only an
/// update to the running version from an older one gives a notice; the
/// marker is removed whatever it held. A second call returns `null`.
///
/// # Errors
///
/// Returns `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn update_take_notice(
    updater: State<'_, UpdaterState>,
) -> CommandResult<Option<UpdateNotice>> {
    let config_dir = updater.config_dir.clone();
    let last_run = updater
        .last_run
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();

    run_blocking(move || {
        let notice = match (config_dir, running_version()) {
            (Some(config_dir), Some(running)) => {
                take_notice(&config_dir, &running, last_run.as_ref())
            }
            (None, _) | (_, None) => None,
        };
        Ok(notice)
    })
    .await
}

/// Sends `report` to the webview over `channel`.
///
/// A webview that is gone misses the report, and the install goes on
/// without it, so the failure is logged and not returned.
fn forward_progress(channel: &Channel<InstallProgress>, report: InstallProgress) {
    if let Err(err) = channel.send(report) {
        log::warn!("update progress could not be sent: {err}");
    }
}

/// Who follows an install: the switch that cancels it and the sink of its
/// progress reports.
struct Watch<'a> {
    /// The install's cancel switch.
    control: &'a InstallControl,
    /// Where progress reports go.
    progress: &'a mut dyn FnMut(InstallProgress),
}

/// What happens around the handoff of a verified artifact: the update
/// marker and the pause on `installing`.
struct Handoff<'a> {
    /// Where the marker is written, or `None` to write none.
    config_dir: Option<&'a Path>,
    /// How long to wait, after `installing` was reported, before the
    /// installer runs.
    hold: Duration,
}

/// An installer that writes the update marker, holds, and then hands the
/// artifact to `inner`.
///
/// It is only ever called with a verified artifact, after `installing` was
/// reported, so the marker is written after the signature check and never
/// for a download that failed or was cancelled.
struct MarkingInstaller<'a, I> {
    /// The installer for this copy.
    inner: &'a I,
    /// The marker and the hold.
    handoff: &'a Handoff<'a>,
    /// The running version.
    from: Version,
    /// The version being installed.
    to: Version,
}

impl<I: ArtifactInstaller> ArtifactInstaller for MarkingInstaller<'_, I> {
    fn install(&self, artifact: &Path) -> oikonomia_update::Result<InstallHandoff> {
        // A marker that cannot be written costs the notice only, and the
        // last-run record still gives it, so the install goes on.
        if let Some(dir) = self.handoff.config_dir
            && let Err(err) = write_pending_marker(dir, &self.from, &self.to)
        {
            log::warn!("update marker could not be written: {err}");
        }

        std::thread::sleep(self.handoff.hold);

        let handed = self.inner.install(artifact);
        if handed.is_err()
            && let Some(dir) = self.handoff.config_dir
        {
            remove_pending_marker(dir);
        }
        handed
    }
}

/// Installs the offer `machine` holds with `installer`, and returns how the
/// install ended.
///
/// This is the body of [`update_install`], apart from the Tauri handles, so
/// that a test can run it with an installer of its own. The update crate
/// downloads the artifact, verifies it in memory and writes the verified
/// file ([`install_offer_reporting`]), reporting to the progress sink of
/// `watch` and stopping when its control is cancelled; `installer` is then
/// called with that file, after the marker and the hold of `handoff`. No
/// updater plugin is involved and no unsigned feed URL is read.
///
/// The machine is not locked during the download: [`PendingInstall`] puts
/// it in its installing state, in which it refuses a second install and a
/// check, so neither can run against the same cache file. A cancelled
/// install gives the offer back to the machine.
///
/// A failed install is logged here with its cause and returned as
/// [`InstallOutcome::Failed`].
///
/// # Errors
///
/// Returns [`oikonomia_update::UpdateError::InstallNotAvailable`] unless the
/// machine holds an offer this copy may install, and
/// [`oikonomia_update::UpdateError::InvalidVersion`] when this build's own
/// version does not parse, before the machine is touched.
fn install_available_update(
    machine: &Mutex<UpdateMachine>,
    config: &ClientConfig,
    installer: &impl ArtifactInstaller,
    handoff: &Handoff<'_>,
    Watch { control, progress }: Watch<'_>,
) -> oikonomia_update::Result<InstallOutcome> {
    let from = running_version().ok_or_else(|| UpdateError::InvalidVersion {
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })?;
    let (pending, offer) = PendingInstall::begin(machine)?;
    let marking = MarkingInstaller {
        inner: installer,
        handoff,
        from,
        to: offer.version().clone(),
    };
    let outcome = install_offer_reporting(config, &offer, &marking, control, progress);
    if let InstallOutcome::Failed(error) = &outcome {
        log_failure("update install failed", error);
    }
    pending.finish(&outcome, offer);

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{
        Handoff, MarkingInstaller, RegisteredInstall, UpdaterState, Watch, cancel_install,
        install_available_update, run_check, updater_cache_dir,
    };
    use crate::update_key::UPDATER_PUBLIC_KEY;
    use oikonomia_update::{
        ArtifactInstaller, CheckOutcome, ClientConfig, InstallControl, InstallHandoff,
        InstallRoute, InstallStep, LAST_RUN_FILE, PENDING_MARKER_FILE, UpdateError, UpdateMachine,
        UpdateStatus,
    };
    use semver::Version;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// An installer that records whether the update marker was on disk
    /// when it was called, and how long after `since` that was.
    struct WitnessInstaller<'a> {
        /// The config directory the marker is written to.
        config_dir: &'a Path,
        /// The moment `installing` was reported.
        since: Instant,
        /// Whether the call fails.
        fail: bool,
        /// What the call saw: the marker, and the time since `since`.
        seen: Mutex<Option<(bool, Duration)>>,
    }

    impl ArtifactInstaller for WitnessInstaller<'_> {
        fn install(&self, _artifact: &Path) -> oikonomia_update::Result<InstallHandoff> {
            let marker = self.config_dir.join(PENDING_MARKER_FILE).exists();
            *self.seen.lock().expect("seen") = Some((marker, self.since.elapsed()));
            if self.fail {
                return Err(UpdateError::InstallFailed {
                    step: InstallStep::StartInstaller,
                });
            }
            Ok(InstallHandoff::InstallerStarted)
        }
    }

    /// Hands an artifact to a [`WitnessInstaller`] through the marking
    /// installer with `hold`, and returns what it saw and the result.
    fn hand_off(
        config_dir: &Path,
        hold: Duration,
        fail: bool,
    ) -> (
        Option<(bool, Duration)>,
        oikonomia_update::Result<InstallHandoff>,
    ) {
        let witness = WitnessInstaller {
            config_dir,
            since: Instant::now(),
            fail,
            seen: Mutex::new(None),
        };
        let handoff = Handoff {
            config_dir: Some(config_dir),
            hold,
        };
        let marking = MarkingInstaller {
            inner: &witness,
            handoff: &handoff,
            from: Version::new(0, 1, 4),
            to: Version::new(0, 1, 5),
        };

        let result = marking.install(Path::new("verified-artifact"));
        let seen = *witness.seen.lock().expect("seen");
        (seen, result)
    }

    #[test]
    fn the_handoff_writes_the_marker_and_holds_installing_before_the_installer_runs() {
        let config = tempfile::tempdir().expect("config directory");

        let (seen, result) = hand_off(config.path(), Duration::from_millis(150), false);

        assert_eq!(result.ok(), Some(InstallHandoff::InstallerStarted));
        let (marker, waited) = seen.expect("the installer ran");
        assert!(marker, "the marker exists before the handoff");
        assert!(waited >= Duration::from_millis(150), "{waited:?}");
        let body: serde_json::Value = serde_json::from_slice(
            &std::fs::read(config.path().join(PENDING_MARKER_FILE)).expect("marker"),
        )
        .expect("json");
        assert_eq!(body, serde_json::json!({ "from": "0.1.4", "to": "0.1.5" }));
    }

    #[test]
    fn a_handoff_that_fails_removes_the_marker() {
        let config = tempfile::tempdir().expect("config directory");

        let (seen, result) = hand_off(config.path(), Duration::ZERO, true);

        assert!(seen.is_some_and(|(marker, _)| marker));
        assert_eq!(
            result.expect_err("installer failed").code(),
            "update_install_failed"
        );
        assert!(!config.path().join(PENDING_MARKER_FILE).exists());
    }

    #[test]
    fn the_installing_hold_is_long_enough_to_read_the_restart_line() {
        assert!(super::INSTALLING_HOLD >= Duration::from_millis(1500));
    }

    #[test]
    fn a_cancel_reaches_only_the_install_in_flight_and_only_once() {
        let updater = UpdaterState::at_start(None);
        assert!(!cancel_install(&updater), "nothing in flight");

        let control = InstallControl::new();
        {
            let _registered = RegisteredInstall::new(&updater, control.clone());
            {
                // A second install that overlaps it neither takes its
                // switch nor clears it when it is refused.
                let overlapping = InstallControl::new();
                let refused = RegisteredInstall::new(&updater, overlapping.clone());
                drop(refused);
                assert!(!overlapping.is_cancelled());
            }
            assert!(cancel_install(&updater));
            assert!(!cancel_install(&updater), "already cancelled");
        }
        assert!(control.is_cancelled());
        assert!(!cancel_install(&updater), "the install ended");
    }

    #[test]
    fn every_start_records_its_version_and_remembers_the_previous_one() {
        let config = tempfile::tempdir().expect("config directory");
        std::fs::write(config.path().join(LAST_RUN_FILE), "0.0.1").expect("record");

        let updater = UpdaterState::at_start(Some(config.path().to_path_buf()));

        assert_eq!(
            std::fs::read_to_string(config.path().join(LAST_RUN_FILE)).expect("record"),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            *updater.last_run.lock().expect("last run"),
            Some(Version::new(0, 0, 1))
        );
    }

    /// An installer that counts its calls and fails each one.
    struct SpyInstaller {
        /// How many times `install` was called.
        calls: AtomicUsize,
    }

    impl ArtifactInstaller for SpyInstaller {
        fn install(&self, _artifact: &Path) -> oikonomia_update::Result<InstallHandoff> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(UpdateError::InstallFailed {
                step: InstallStep::Replace,
            })
        }
    }

    #[test]
    fn the_update_cache_is_a_directory_of_its_own_under_the_app_cache() {
        let cache = std::path::PathBuf::from("cache").join("io.oikonomia");

        assert_eq!(
            updater_cache_dir(Ok(cache.clone())).expect("a cache directory"),
            cache.join("updater")
        );
    }

    #[test]
    fn a_system_that_names_no_cache_directory_is_reported_as_that() {
        let refused = updater_cache_dir(Err(tauri::Error::UnknownPath)).expect_err("no cache");

        assert_eq!(refused.code, "cache_dir_unavailable");
        assert_ne!(refused.code, "task_failed", "no task is involved");
        assert_eq!(refused.params, std::collections::BTreeMap::new());
    }

    #[test]
    fn baked_key_is_a_nonempty_minisign_key() {
        assert!(UPDATER_PUBLIC_KEY.len() > 32);
        ClientConfig::production(
            UPDATER_PUBLIC_KEY,
            env!("CARGO_PKG_VERSION"),
            std::env::temp_dir().join("oiko-update-never-written"),
            InstallRoute::InApp,
        )
        .expect("ops minisign public key must decode");
    }

    #[test]
    fn tauri_conf_pubkey_matches_desktop_constant() {
        let conf = include_str!("../tauri.conf.json");
        let value: serde_json::Value = serde_json::from_str(conf).expect("tauri.conf.json");
        let pubkey = value
            .pointer("/plugins/updater/pubkey")
            .and_then(serde_json::Value::as_str)
            .expect("pubkey");
        assert_eq!(pubkey, UPDATER_PUBLIC_KEY);
        assert!(pubkey.len() > 32);
    }

    #[test]
    fn update_module_source_does_not_call_plugin_check_or_read_unsigned_feed() {
        let src = include_str!("update.rs");
        let product = src.split("#[cfg(test)]").next().unwrap_or(src);
        for needle in [
            ".check(",
            "download_and_install",
            "updater_builder",
            "UpdaterExt",
            "UPDATE_FEED_URL",
        ] {
            assert!(
                !product.contains(needle),
                "update.rs product code must not contain {needle}"
            );
        }
        let exec = include_str!("update_exec.rs");
        let exec_product = exec.split("#[cfg(test)]").next().unwrap_or(exec);
        for needle in [
            ".check(",
            "download_and_install",
            "updater_builder",
            "UpdaterExt",
            "UPDATE_FEED_URL",
        ] {
            assert!(
                !exec_product.contains(needle),
                "update_exec.rs product code must not contain {needle}"
            );
        }
    }

    #[test]
    fn a_check_that_dies_does_not_leave_the_machine_checking() {
        let machine = Arc::new(Mutex::new(UpdateMachine::new()));

        let result = tauri::async_runtime::block_on(run_check(Arc::clone(&machine), || {
            std::panic::resume_unwind(Box::new("check died"))
        }));

        assert_eq!(result.expect_err("join error").code, "task_failed");
        assert_eq!(
            crate::state::lock_update(&machine).status(),
            UpdateStatus::Failed { code: None }
        );
    }

    /// Runs a check that ends in `error` and returns the status the command
    /// would send, as the JSON the webview receives.
    fn failed_check_as_json(error: UpdateError) -> serde_json::Value {
        let machine = Arc::new(Mutex::new(UpdateMachine::new()));

        let status =
            tauri::async_runtime::block_on(run_check(machine, move || CheckOutcome::Failed(error)))
                .expect("a failed check is a status, not a command error");

        serde_json::to_value(status).expect("status json")
    }

    #[test]
    fn a_failed_check_sends_the_webview_the_code_of_its_cause() {
        let offline = failed_check_as_json(UpdateError::Network);
        let forged = failed_check_as_json(UpdateError::ManifestSignature);

        assert_eq!(
            offline,
            serde_json::json!({ "kind": "failed", "code": "update_network" })
        );
        assert_eq!(
            forged,
            serde_json::json!({ "kind": "failed", "code": "update_manifest_signature" })
        );
    }

    #[test]
    fn a_finished_check_reports_its_outcome() {
        let machine = Arc::new(Mutex::new(UpdateMachine::new()));

        let status = tauri::async_runtime::block_on(run_check(Arc::clone(&machine), || {
            CheckOutcome::UpToDate
        }));

        assert_eq!(status.ok(), Some(UpdateStatus::UpToDate));
    }

    // `PendingInstall`'s drop guard has no test here: reaching it needs a
    // machine that holds an offer, and a `VerifiedOffer` comes only from a
    // check against a feed. The update crate tests what the guard calls
    // (`an_install_that_dies_leaves_a_usable_machine`).
    #[test]
    fn install_helper_from_idle_is_hard_error_and_does_not_exec() {
        let machine = Mutex::new(UpdateMachine::new());
        let config = ClientConfig::production(
            UPDATER_PUBLIC_KEY,
            env!("CARGO_PKG_VERSION"),
            std::env::temp_dir().join("oiko-update-never-written"),
            InstallRoute::InApp,
        )
        .expect("production config");
        let installer = SpyInstaller {
            calls: AtomicUsize::new(0),
        };
        let handoff = Handoff {
            config_dir: None,
            hold: Duration::ZERO,
        };
        let err = install_available_update(
            &machine,
            &config,
            &installer,
            &handoff,
            Watch {
                control: &InstallControl::new(),
                progress: &mut |_| {},
            },
        )
        .expect_err("idle");
        assert_eq!(err.code(), "update_install_not_allowed");
        assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
        assert_eq!(machine.lock().expect("lock").status(), UpdateStatus::Idle);
    }
}

/// The update commands through the real IPC path: argument names, the
/// channel argument and the JSON the webview receives.
///
/// Not on Windows, where a test binary with the mock runtime fails before
/// starting; `commands::support::ipc_test_support` says why.
#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use super::{UpdaterState, update_cancel, update_install, update_take_notice};
    use crate::commands::ipc_test_support::MockApp;
    use oikonomia_update::{LAST_RUN_FILE, PENDING_MARKER_FILE};

    /// Starts the mock app with the update commands, its updater state
    /// keeping its records in `config_dir`.
    fn mock_app(label: &str, config_dir: &std::path::Path) -> MockApp {
        let (app, ()) = MockApp::start(
            label,
            tauri::generate_handler![update_install, update_cancel, update_take_notice],
            |_conn| (),
        );
        app.manage(UpdaterState::at_start(Some(config_dir.to_path_buf())));
        app
    }

    #[test]
    fn a_cancel_with_nothing_in_flight_answers_false() {
        let config = tempfile::tempdir().unwrap();
        let app = mock_app("update-cancel", config.path());

        assert_eq!(
            app.invoke("update_cancel", serde_json::json!({})),
            Ok(serde_json::json!(false))
        );
    }

    #[test]
    fn the_notice_of_a_marker_for_this_version_comes_once() {
        let config = tempfile::tempdir().unwrap();
        let app = mock_app("update-notice", config.path());
        std::fs::write(
            config.path().join(PENDING_MARKER_FILE),
            format!(r#"{{"from":"0.0.1","to":"{}"}}"#, env!("CARGO_PKG_VERSION")),
        )
        .unwrap();

        let first = app.invoke("update_take_notice", serde_json::json!({}));
        let second = app.invoke("update_take_notice", serde_json::json!({}));

        assert_eq!(
            first,
            Ok(serde_json::json!({ "from": "0.0.1", "to": env!("CARGO_PKG_VERSION") }))
        );
        assert_eq!(second, Ok(serde_json::Value::Null));
        assert!(!config.path().join(PENDING_MARKER_FILE).exists());
    }

    #[test]
    fn without_a_marker_the_notice_comes_from_the_last_run_record() {
        let config = tempfile::tempdir().unwrap();
        std::fs::write(config.path().join(LAST_RUN_FILE), "0.0.1").unwrap();
        let app = mock_app("update-last-run", config.path());

        let first = app.invoke("update_take_notice", serde_json::json!({}));
        let second = app.invoke("update_take_notice", serde_json::json!({}));

        assert_eq!(
            first,
            Ok(serde_json::json!({ "from": "0.0.1", "to": env!("CARGO_PKG_VERSION") }))
        );
        assert_eq!(second, Ok(serde_json::Value::Null));
    }

    #[test]
    fn a_marker_for_another_version_gives_no_notice_and_is_removed() {
        let config = tempfile::tempdir().unwrap();
        let app = mock_app("update-mismatch", config.path());
        std::fs::write(
            config.path().join(PENDING_MARKER_FILE),
            r#"{"from":"0.0.1","to":"999.0.0"}"#,
        )
        .unwrap();

        assert_eq!(
            app.invoke("update_take_notice", serde_json::json!({})),
            Ok(serde_json::Value::Null)
        );
        assert!(!config.path().join(PENDING_MARKER_FILE).exists());
    }

    #[test]
    fn an_install_takes_its_progress_channel_as_on_progress_and_needs_an_offer() {
        let config = tempfile::tempdir().unwrap();
        let app = mock_app("update-install-idle", config.path());

        let with_channel = app
            .invoke(
                "update_install",
                serde_json::json!({ "onProgress": "__CHANNEL__:7" }),
            )
            .expect_err("no offer");
        let without = app
            .invoke("update_install", serde_json::json!({}))
            .expect_err("no channel");

        assert_eq!(with_channel["code"], "update_install_not_allowed");
        assert!(
            without.to_string().contains("onProgress"),
            "the channel is required: {without}"
        );
        assert!(!config.path().join(PENDING_MARKER_FILE).exists());
        assert_eq!(
            app.invoke("update_cancel", serde_json::json!({})),
            Ok(serde_json::json!(false)),
            "a refused install leaves nothing to cancel"
        );
    }
}
