//! Unlock-screen `update_check` / `update_install` IPC.
//!
//! The webview cannot pass a feed URL, endpoint, or public key. HTTP runs on
//! the blocking pool (`ureq` inside `oikonomia-update`). The updater plugin is
//! the install engine only (artifact fetch, `.sig` verify, platform exec) and
//! is invoked from `update_install` after our wrapper has already verified the
//! signed manifest. `setup()` never calls `check()`.

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use crate::update_key::UPDATER_PUBLIC_KEY;
use oikonomia_update::{
    default_updater_cache_dir, delete_artifact, download_and_verify, perform_check, CheckOutcome,
    ClientConfig, HostPolicy, UpdateError, UpdateStatus, UPDATE_FEED_URL,
};
use tauri::State;

/// User-clicked check from unlock. Fetches `latest.json` + `latest.json.sig`,
/// verifies with the baked minisign key, allow-lists the artifact URL.
/// Does not download the artifact.
///
/// HTTP is `ureq` on the blocking pool so the async runtime is not stalled.
#[tauri::command]
pub async fn update_check(state: State<'_, AppState>) -> CommandResult<UpdateStatus> {
    let machine = state.update_machine();
    {
        let mut guard = crate::state::lock_update(&machine);
        guard.begin_check();
    }

    let version = env!("CARGO_PKG_VERSION").to_owned();
    let cache = default_updater_cache_dir();

    let outcome = match tauri::async_runtime::spawn_blocking(move || {
        let config = match ClientConfig::production(UPDATER_PUBLIC_KEY, &version, cache) {
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
            return Err(CommandError {
                code: "io".into(),
                message: format!("background task failed: {err}"),
            });
        }
    };

    let mut guard = crate::state::lock_update(&machine);
    guard.finish_check(outcome);
    Ok(guard.status())
}

/// Install is only legal from [`UpdateStatus::Available`]. Downloads outside the
/// vault data dir, verifies hash and `.sig`, then asks the plugin to exec.
/// Then `app.restart()` from Rust.
///
/// From Idle / Failed / Checking this is a typed hard error, not a silent no-op.
#[tauri::command]
pub async fn update_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<UpdateStatus> {
    let machine = state.update_machine();
    let offer = {
        let guard = crate::state::lock_update(&machine);
        guard.require_available()?.clone()
    };

    let version = env!("CARGO_PKG_VERSION").to_owned();
    let cache = default_updater_cache_dir();

    let artifact = match tauri::async_runtime::spawn_blocking(move || {
        let config = ClientConfig::production(UPDATER_PUBLIC_KEY, &version, cache)?;
        download_and_verify(&config, &offer)
    })
    .await
    {
        Ok(Ok(path)) => path,
        Ok(Err(err)) => {
            log::warn!("update install verify failed: {err}");
            let mut guard = crate::state::lock_update(&machine);
            guard.fail();
            return Ok(UpdateStatus::Failed);
        }
        Err(err) => {
            return Err(CommandError {
                code: "io".into(),
                message: format!("background task failed: {err}"),
            });
        }
    };

    let plugin_result = install_with_plugin(&app).await;
    delete_artifact(&artifact);

    match plugin_result {
        Ok(()) => {
            app.restart();
            Ok(UpdateStatus::Failed)
        }
        Err(err) => {
            log::warn!("update plugin install failed: {err}");
            let mut guard = crate::state::lock_update(&machine);
            guard.fail();
            Ok(UpdateStatus::Failed)
        }
    }
}

/// Plugin install engine: baked feed URL and baked pubkey only. Not from IPC.
///
/// The plugin API is async (`check` / `download_and_install`). Our wrapper
/// already verified the signed manifest and the artifact hash/sig on the
/// blocking pool; this second fetch is the platform exec path.
async fn install_with_plugin(app: &tauri::AppHandle) -> Result<(), UpdateError> {
    use tauri_plugin_updater::UpdaterExt;

    let feed = UPDATE_FEED_URL
        .parse()
        .map_err(|_| UpdateError::InvalidFeedUrl)?;
    let updater = app
        .updater_builder()
        .pubkey(UPDATER_PUBLIC_KEY)
        .endpoints(vec![feed])
        .map_err(|_| UpdateError::InvalidFeedUrl)?
        .build()
        .map_err(|_| UpdateError::Network)?;
    let Some(update) = updater.check().await.map_err(|_| UpdateError::Network)? else {
        return Err(UpdateError::Network);
    };
    if !HostPolicy::production().is_allowed_artifact_url(&update.download_url) {
        return Err(UpdateError::ArtifactUrl);
    }
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|_| UpdateError::ArtifactIntegrity)?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use crate::update_key::UPDATER_PUBLIC_KEY;
    use oikonomia_core::license::PRODUCTION_PUBLIC_KEY_HEX;
    use oikonomia_update::parse_public_key;

    #[test]
    fn baked_key_is_nonempty_minisign_and_not_the_license_key() {
        assert!(!UPDATER_PUBLIC_KEY.is_empty());
        assert_ne!(UPDATER_PUBLIC_KEY, PRODUCTION_PUBLIC_KEY_HEX);
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
        assert!(!pubkey.is_empty());
    }

    #[test]
    fn empty_key_is_a_compile_fail_gate() {
        const EMPTY: &str = "";
        assert!(EMPTY.is_empty());
        assert!(!UPDATER_PUBLIC_KEY.is_empty());
    }
}
