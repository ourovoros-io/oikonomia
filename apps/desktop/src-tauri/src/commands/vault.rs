//! Vault lifecycle commands: status, unlock, lock, backup and restore.

use crate::commands::support::{await_blocking, require_granted_path, with_vault_blocking};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use oikonomia_core::prefs::load_ui_prefs;
use oikonomia_core::vault::{BACKUP_EXTENSION, VaultStatus, default_backup_file_name};
use std::path::PathBuf;
use tauri::{Emitter, State};
use zeroize::Zeroizing;

/// Return vault lock lifecycle status.
///
/// Async so that the probe waits for the vault mutex on the blocking pool,
/// not on the main thread, while a long operation such as a rekey holds it.
/// A probe does not count as activity; the idle heartbeat is [`vault_touch`].
#[tauri::command]
pub(crate) async fn vault_status(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    let vault = state.vault();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let guard = vault.acquire();
        Ok(guard.status())
    }))
    .await
}

/// Heartbeat for the idle watchdog. Separate from [`vault_status`] so lock
/// probes (quick-add focus) do not extend the idle window.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn vault_touch(state: State<'_, AppState>) {
    state.touch();
}

/// Create a new encrypted vault with the master password.
///
/// The password is wiped from memory when the command returns; only the
/// derived key lives on, inside `SQLCipher`.
#[tauri::command]
pub(crate) async fn vault_init(
    state: State<'_, AppState>,
    password: Zeroizing<String>,
) -> CommandResult<VaultStatus> {
    with_vault_blocking(&state, move |vault| {
        vault.init(&password)?;
        Ok(vault.status())
    })
    .await
}

/// Unlock an existing vault. The password is wiped when the command returns.
#[tauri::command]
pub(crate) async fn vault_unlock(
    state: State<'_, AppState>,
    password: Zeroizing<String>,
) -> CommandResult<VaultStatus> {
    with_vault_blocking(&state, move |vault| {
        vault.unlock(&password)?;
        Ok(vault.status())
    })
    .await
}

/// Change the master password (requires the current password). Both
/// passwords are wiped when the command returns.
///
/// Returns the status the vault is left in. The watchdog follows it whatever
/// it is, so this does not depend on whether core unlocks a locked vault as
/// part of the change.
#[tauri::command]
pub(crate) async fn vault_change_password(
    state: State<'_, AppState>,
    old: Zeroizing<String>,
    new: Zeroizing<String>,
) -> CommandResult<VaultStatus> {
    with_vault_blocking(&state, move |vault| {
        vault.change_password(&old, &new)?;
        Ok(vault.status())
    })
    .await
}

/// Lock the vault for this session.
#[tauri::command]
pub(crate) async fn vault_lock(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<VaultStatus> {
    let status = with_vault_blocking(&state, move |vault| {
        vault.lock();
        Ok(vault.status())
    })
    .await?;
    let _ = app.emit("vault-locked", ());
    Ok(status)
}

/// Write a portable ciphertext archive. Does not lock; an unlocked session
/// stays unlocked.
///
/// Always presents a native save dialog (same pattern as [`document_export`](crate::commands::document_export)).
/// The suggested filename is `oikonomia-backup-YYYY-MM-DD.oikonomia-backup`
/// using the local calendar date. An unlocked vault is snapshotted with
/// `VACUUM INTO` so the copy is consistent without closing `SQLCipher`.
/// The archive is `vault.db` plus `vault.header.json` only: it is not
/// re-encrypted and never stores the master password.
///
/// Returns the destination path, or `None` if the user cancelled.
#[tauri::command]
pub(crate) async fn vault_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let file_name = default_backup_file_name();
    let data_dir = state.data_dir().to_path_buf();
    let picked = await_blocking(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            use tauri_plugin_dialog::DialogExt;

            // Read in here because it is file I/O, which stays off the
            // async workers.
            let filter_label = crate::tray::backup_filter_label(load_ui_prefs(&data_dir).locale);
            Ok(app
                .dialog()
                .file()
                .add_filter(filter_label, &[BACKUP_EXTENSION])
                .set_file_name(&file_name)
                .blocking_save_file())
        }
    }))
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let dest = with_backup_extension(file_path.into_path().map_err(|e| {
        CommandError::desktop(
            DesktopError::SaveLocationInvalid,
            format!("invalid save location: {e}"),
        )
    })?);

    with_vault_blocking(&state, move |vault| {
        vault.backup_to(&dest)?;
        Ok(dest.display().to_string())
    })
    .await
    .map(Some)
}

/// Restore a portable vault archive and leave the vault locked.
///
/// `path` is the archive to unpack. When `path` is `None`, a native open
/// dialog chooses the file (the in-app path). A concrete path is accepted only
/// if the user granted it ([`AppState::grant_paths`]), so the webview cannot
/// name arbitrary files. The grant is not tied to this command: besides the
/// path [`vault_pick_backup`] returned, any file the user dropped on a window
/// or picked for a CSV import passes the check, and is then rejected only if
/// it is not a backup archive. Decrypt is not performed; the owner unlocks
/// afterwards with the existing master password.
///
/// Existing vault files are not overwritten unless `replace` is `true`.
/// An uninitialized data directory accepts `replace: false`.
///
/// Returns the archive path that was restored, or `None` if the user cancelled
/// the open dialog.
#[tauri::command]
pub(crate) async fn vault_restore(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
    replace: bool,
) -> CommandResult<Option<String>> {
    // Refuse an ungranted path before touching the session.
    let granted = path
        .map(|chosen| require_granted_path(&state, &chosen))
        .transpose()?;
    lock_vault_session(&app, &state).await?;

    let archive = if let Some(chosen) = granted {
        chosen
    } else {
        let Some(picked) = pick_backup_path(&app, &state).await? else {
            return Ok(None);
        };
        picked
    };

    with_vault_blocking(&state, move |vault| {
        vault.restore_from(&archive, replace)?;
        Ok(archive.display().to_string())
    })
    .await
    .map(Some)
}

/// Choose a backup file via a native open dialog.
///
/// Read-only: does not restore, lock, or write vault files. Returns the chosen
/// path, or `None` if the user cancelled. The frontend confirms, then calls
/// [`vault_restore`] with that path and `replace`.
#[tauri::command]
pub(crate) async fn vault_pick_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let Some(path) = pick_backup_path(&app, &state).await? else {
        return Ok(None);
    };
    Ok(Some(path.display().to_string()))
}

/// Native Open dialog for a `.oikonomia-backup` file. `None` if cancelled.
/// The chosen path is granted so [`vault_restore`] may receive it back.
async fn pick_backup_path(
    app: &tauri::AppHandle,
    state: &AppState,
) -> CommandResult<Option<PathBuf>> {
    let data_dir = state.data_dir().to_path_buf();
    let picked = await_blocking(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            use tauri_plugin_dialog::DialogExt;

            // Read in here because it is file I/O, which stays off the
            // async workers.
            let filter_label = crate::tray::backup_filter_label(load_ui_prefs(&data_dir).locale);
            Ok(app
                .dialog()
                .file()
                .add_filter(filter_label, &[BACKUP_EXTENSION])
                .blocking_pick_file())
        }
    }))
    .await?;
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|e| {
        CommandError::desktop(
            DesktopError::SaveLocationInvalid,
            format!("invalid backup location: {e}"),
        )
    })?;
    state.grant_paths([path.clone()]);
    Ok(Some(path))
}

/// Close any open `SQLCipher` connection and notify the UI when the session
/// actually transitioned from unlocked to locked.
async fn lock_vault_session(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
) -> CommandResult<()> {
    let was_unlocked = with_vault_blocking(state, |vault| {
        let was_unlocked = vault.status() == VaultStatus::Unlocked;
        vault.lock();
        Ok(was_unlocked)
    })
    .await?;
    if was_unlocked {
        let _ = app.emit("vault-locked", ());
    }
    Ok(())
}

fn with_backup_extension(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext == BACKUP_EXTENSION => path,
        _ => {
            let mut name = path.file_name().map_or_else(
                || std::ffi::OsString::from("oikonomia"),
                std::ffi::OsString::from,
            );
            name.push(".");
            name.push(BACKUP_EXTENSION);
            match path.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(parent) => parent.join(name),
                None => std::path::PathBuf::from(name),
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn backup_extension_is_added_only_when_missing() {
        use super::with_backup_extension;
        use std::path::PathBuf;

        let cases = [
            ("books/2026.oikonomia-backup", "books/2026.oikonomia-backup"),
            ("books/2026", "books/2026.oikonomia-backup"),
            ("books/2026.zip", "books/2026.zip.oikonomia-backup"),
            // The extension is matched exactly; another case is another extension.
            (
                "2026.OIKONOMIA-BACKUP",
                "2026.OIKONOMIA-BACKUP.oikonomia-backup",
            ),
            ("2026", "2026.oikonomia-backup"),
            // No file name at all: fall back to a default one.
            ("", "oikonomia.oikonomia-backup"),
        ];
        for (input, want) in cases {
            assert_eq!(
                with_backup_extension(PathBuf::from(input)),
                PathBuf::from(want),
                "input {input:?}"
            );
        }
    }
}
