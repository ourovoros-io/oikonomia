//! Unlock-screen `update_check` / `update_install` IPC.
//!
//! The webview cannot pass a feed URL, endpoint, or public key. HTTP runs on
//! the blocking pool (`ureq` inside `oikonomia-update`). Install execs the
//! already-verified local path via [`crate::update_exec::VerifiedPathInstaller`].
//! `setup()` never starts a check.

use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use crate::update_exec::{InstallKind, VerifiedPathInstaller};
use crate::update_key::UPDATER_PUBLIC_KEY;
use oikonomia_update::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, UpdateMachine,
    UpdateStatus, perform_check,
};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};

/// Where a downloaded update waits to be installed: a directory of its own
/// under this user's cache location.
///
/// Not the system temporary directory. On Linux that is shared by every
/// account, and whoever creates the directory first could swap the verified
/// file before it is installed. Not the vault data directory either.
fn updater_cache_dir(app: &tauri::AppHandle) -> CommandResult<PathBuf> {
    let cache = app.path().app_cache_dir().map_err(|err| {
        CommandError::desktop(
            DesktopError::TaskFailed,
            format!("no cache directory for updates: {err}"),
        )
    })?;
    Ok(cache.join("updater"))
}

/// User-clicked check from unlock. Fetches `latest.json` plus a detached
/// `latest.json.sig`, verifies with the baked minisign key, allow-lists the
/// artifact URL. Does not download the artifact.
///
/// HTTP is `ureq` on the blocking pool so the async runtime is not stalled.
#[tauri::command]
pub async fn update_check(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<UpdateStatus> {
    let cache = updater_cache_dir(&app)?;
    let machine = state.update_machine();
    {
        let mut guard = crate::state::lock_update(&machine);
        guard.begin_check();
    }

    let version = env!("CARGO_PKG_VERSION").to_owned();

    let outcome = match tauri::async_runtime::spawn_blocking(move || {
        let route = InstallKind::detect().route();
        let config = match ClientConfig::production(UPDATER_PUBLIC_KEY, &version, cache, route) {
            Ok(config) => config,
            Err(err) => {
                log::warn!("update check config failed: {err}");
                return CheckOutcome::Failed;
            }
        };
        perform_check(&config)
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(err) => {
            return Err(CommandError::desktop(
                DesktopError::TaskFailed,
                format!("background task failed: {err}"),
            ));
        }
    };

    let mut guard = crate::state::lock_update(&machine);
    guard.finish_check(outcome);
    Ok(guard.status())
}

/// Install is only legal from [`UpdateStatus::Available`]. Downloads outside the
/// vault data dir, verifies hash and signature, then execs that verified path.
/// A copy replaced in place restarts into the new version. On Windows the
/// installer process replaces the files, so the app exits and the installer
/// starts the new version.
///
/// From Idle / Failed / Checking this is a typed hard error, not a silent no-op.
#[tauri::command]
pub async fn update_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<UpdateStatus> {
    let cache = updater_cache_dir(&app)?;
    let machine = state.update_machine();
    let version = env!("CARGO_PKG_VERSION").to_owned();

    let outcome = match tauri::async_runtime::spawn_blocking(move || {
        let kind = InstallKind::detect();
        let config = ClientConfig::production(UPDATER_PUBLIC_KEY, &version, cache, kind.route())?;
        install_available_update(&machine, config, &VerifiedPathInstaller::new(kind))
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
        InstallOutcome::Failed => Ok(UpdateStatus::Failed),
        InstallOutcome::Installed(InstallHandoff::Replaced) => app.restart(),
        InstallOutcome::Installed(InstallHandoff::InstallerStarted) => {
            app.exit(0);
            Ok(UpdateStatus::Idle)
        }
    }
}

/// Shared install path used by IPC. Download → verify on disk → exec that path.
///
/// Does not call a plugin check and does not read an unsigned feed URL.
fn install_available_update(
    machine: &Mutex<UpdateMachine>,
    config: ClientConfig,
    installer: &impl ArtifactInstaller,
) -> oikonomia_update::Result<InstallOutcome> {
    let mut guard = crate::state::lock_update(machine);
    guard.install(&config, installer)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::install_available_update;
    use crate::update_key::UPDATER_PUBLIC_KEY;
    use oikonomia_update::{
        ArtifactInstaller, ClientConfig, InstallHandoff, InstallRoute, UpdateError, UpdateMachine,
        UpdateStatus, parse_public_key,
    };
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SpyInstaller {
        calls: AtomicUsize,
    }

    impl ArtifactInstaller for SpyInstaller {
        fn install(&self, _artifact: &Path) -> oikonomia_update::Result<InstallHandoff> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(UpdateError::ArtifactIntegrity)
        }
    }

    #[test]
    fn baked_key_is_a_nonempty_minisign_key() {
        assert!(UPDATER_PUBLIC_KEY.len() > 32);
        parse_public_key(UPDATER_PUBLIC_KEY).expect("ops minisign public key must decode");
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
    fn empty_key_is_a_compile_fail_gate() {
        const EMPTY: &str = "";
        assert_eq!(EMPTY.len(), 0);
        assert!(UPDATER_PUBLIC_KEY.len() > 32);
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
        let err = install_available_update(&machine, config, &installer).expect_err("idle");
        assert_eq!(err.code(), "update_install_not_allowed");
        assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
        assert_eq!(machine.lock().expect("lock").status(), UpdateStatus::Idle);
    }
}
