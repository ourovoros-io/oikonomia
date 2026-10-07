//! The update commands, `update_check` and `update_install`.
//!
//! This is the app's only path to the network, and it runs only on the
//! user's click: nothing here starts a check when the app starts.
//!
//! The webview supplies nothing. The feed URL, the endpoint and the public
//! key are compiled in (`oikonomia-update`, `crate::update_key`), so a
//! compromised page cannot point the updater elsewhere. HTTP is `ureq`
//! inside `oikonomia-update`, called on the blocking pool. An install hands
//! the downloaded and verified file to
//! [`crate::update_exec::VerifiedPathInstaller`], which replaces the running
//! copy.

use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use crate::update_exec::{InstallKind, VerifiedPathInstaller};
use crate::update_key::UPDATER_PUBLIC_KEY;
use oikonomia_update::{
    ArtifactInstaller, CheckOutcome, CheckStart, ClientConfig, InstallHandoff, InstallOutcome,
    UpdateError, UpdateMachine, UpdateStatus, VerifiedOffer, install_offer, perform_check,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

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
    /// The offer the machine handed out for this install.
    offer: VerifiedOffer,
    /// Whether [`Self::finish`] has applied an outcome.
    finished: bool,
}

impl<'a> PendingInstall<'a> {
    /// Marks `machine` as installing and takes its offer.
    ///
    /// # Errors
    ///
    /// Returns [`oikonomia_update::UpdateError::InstallNotAvailable`] unless
    /// the machine holds an offer this copy may install.
    fn begin(machine: &'a Mutex<UpdateMachine>) -> oikonomia_update::Result<Self> {
        let offer = crate::state::lock_update(machine).begin_install()?;

        Ok(Self {
            machine,
            offer,
            finished: false,
        })
    }

    /// Applies `outcome` to the machine.
    fn finish(mut self, outcome: &InstallOutcome) {
        crate::state::lock_update(self.machine).finish_install(outcome);
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

/// Installs the update the last check offered.
///
/// Downloads into the updater cache, outside the vault data dir, verifies hash
/// and signature in memory, writes the verified file, then execs that path.
/// A copy replaced in place restarts into the new version. On Windows the
/// installer process replaces the files, so the app exits and the installer
/// starts the new version.
///
/// From any state but [`UpdateStatus::Available`] this is a typed hard error,
/// not a silent no-op. An install that was begun and then failed is the
/// status `Failed` with the code of what stopped it.
///
/// # Errors
///
/// Returns `update_install_not_allowed` unless the last check found an
/// update this copy may install, the code of the
/// [`UpdateError`] when the client configuration cannot be built,
/// `cache_dir_unavailable` when the system names no cache directory, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn update_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<UpdateStatus> {
    let cache = updater_cache_dir(app.path().app_cache_dir())?;
    let machine = state.update_machine();
    let version = env!("CARGO_PKG_VERSION").to_owned();

    let outcome = match tauri::async_runtime::spawn_blocking(move || {
        let kind = InstallKind::detect();
        let config = ClientConfig::production(UPDATER_PUBLIC_KEY, &version, cache, kind.route())?;
        install_available_update(&machine, &config, &VerifiedPathInstaller::new(kind))
    })
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(err)) => return Err(CommandError::from(err)),
        Err(err) => {
            return Err(CommandError::desktop(
                DesktopError::TaskFailed,
                format!("background task failed: {err}"),
            ));
        }
    };

    match outcome {
        // Built from the outcome, not read back from the machine: a check may
        // have begun since, and its `Checking` is not the answer to this install.
        InstallOutcome::Failed(error) => Ok(UpdateStatus::Failed {
            code: Some(error.code().to_owned()),
        }),
        InstallOutcome::Installed(InstallHandoff::Replaced) => app.restart(),
        InstallOutcome::Installed(InstallHandoff::InstallerStarted) => {
            app.exit(0);
            Ok(UpdateStatus::Idle)
        }
    }
}

/// Runs the install for the IPC command: downloads, verifies in memory,
/// writes the verified file, then execs that path.
///
/// Does not call a plugin check and does not read an unsigned feed URL.
///
/// The machine is not locked during the download: [`PendingInstall`] puts
/// it in its installing state, in which it refuses a second install and a
/// check, so neither can run against the same cache file.
///
/// A failed install is logged here with its cause and returned as
/// [`InstallOutcome::Failed`].
///
/// # Errors
///
/// Returns [`oikonomia_update::UpdateError::InstallNotAvailable`] unless the
/// machine holds an offer this copy may install.
fn install_available_update(
    machine: &Mutex<UpdateMachine>,
    config: &ClientConfig,
    installer: &impl ArtifactInstaller,
) -> oikonomia_update::Result<InstallOutcome> {
    let pending = PendingInstall::begin(machine)?;
    let outcome = install_offer(config, &pending.offer, installer);
    if let InstallOutcome::Failed(error) = &outcome {
        log_failure("update install failed", error);
    }
    pending.finish(&outcome);

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{install_available_update, run_check, updater_cache_dir};
    use crate::update_key::UPDATER_PUBLIC_KEY;
    use oikonomia_update::{
        ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallRoute, InstallStep,
        UpdateError, UpdateMachine, UpdateStatus,
    };
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

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
        let err = install_available_update(&machine, &config, &installer).expect_err("idle");
        assert_eq!(err.code(), "update_install_not_allowed");
        assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
        assert_eq!(machine.lock().expect("lock").status(), UpdateStatus::Idle);
    }
}
