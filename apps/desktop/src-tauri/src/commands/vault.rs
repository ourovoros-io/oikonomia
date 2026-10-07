//! Vault lifecycle commands: status, create, unlock, lock, change password,
//! back up and restore.
//!
//! These are the commands that change the vault's own state, so they take the
//! whole vault ([`with_vault_blocking`]), not just its connection. None of
//! them touches the idle watchdog: the vault guard does that as it is dropped.
//!
//! A lock made here is announced to every window with the `vault-locked`
//! event, the same event the watchdog emits for an idle lock
//! ([`crate::state::spawn_auto_lock`]).

use crate::commands::support::{
    FileDialog, dialog_path, require_granted_path, run_blocking, with_vault_blocking,
};
use crate::error::CommandResult;
use crate::state::{AppState, GrantPurpose};
use oikonomia_core::prefs::load_ui_prefs;
use oikonomia_core::vault::{BACKUP_EXTENSION, VaultStatus, default_backup_file_name};
use std::path::PathBuf;
use tauri::{Emitter, Runtime, State};
use zeroize::Zeroizing;

/// Returns whether the vault is uninitialized, locked or unlocked.
///
/// Works in every state. Async so that the probe waits for the vault mutex
/// on the blocking pool, not on the main thread, while a long operation such
/// as a password change holds it. A probe does not count as activity; the
/// idle heartbeat is [`vault_touch`].
///
/// # Errors
///
/// Returns `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn vault_status(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    let vault = state.vault();

    run_blocking(move || {
        let guard = vault.acquire();
        Ok(guard.status())
    })
    .await
}

/// Records user activity for the idle watchdog.
///
/// Separate from [`vault_status`] so that lock probes, such as the one the
/// quick-add window makes when it gains focus, do not extend the idle window.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn vault_touch(state: State<'_, AppState>) {
    state.touch();
}

/// Creates a new encrypted vault under the master password and leaves it
/// unlocked.
///
/// Requires that no vault exists yet. The password is wiped from memory when
/// the command returns; only the derived key lives on, inside `SQLCipher`.
///
/// # Errors
///
/// Returns `vault_already_initialized` when a vault exists,
/// `password_too_short` (with the minimum as `min`) for a password under the
/// minimum length, `crypto` when the key cannot be derived, `io` when the
/// vault files cannot be written, and `database` when the new database cannot
/// be set up. Returns `task_failed` when the blocking task panics.
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

/// Unlocks the vault with the master password and returns its status.
///
/// Requires an existing vault. Unlocking also brings an older database up to
/// the current schema. On a vault that is already unlocked this succeeds
/// without looking at the password, so it cannot serve to check one. The
/// password is wiped when the command returns.
///
/// # Errors
///
/// Returns `vault_uninitialized` when no vault exists, `invalid_password`
/// when the password is rejected, `vault_corrupt` when the header or the
/// database cannot be used, `vault_too_new` (with the versions as `found` and
/// `supported`) for a header or database written by a newer version, `crypto` when the
/// key cannot be derived, `io` when a file cannot be read, and `database`
/// when the database cannot be read or migrated. Returns `task_failed` when
/// the blocking task panics.
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

/// Changes the master password, given the current one, and returns the
/// status the vault is left in.
///
/// Requires an existing vault, locked or unlocked; core keeps that state, so
/// a locked vault stays locked. Both passwords are wiped when the command
/// returns. The watchdog follows the status that results, whatever it is.
///
/// # Errors
///
/// Returns `vault_uninitialized` when no vault exists, `password_too_short`
/// (with the minimum as `min`) for a new password under the minimum length,
/// `invalid_password` when the current password is rejected, `crypto` when a
/// key cannot be derived, and `io`, `database` or `vault_corrupt` when the
/// vault files cannot be rewritten. Returns `task_failed` when the blocking
/// task panics.
///
/// One of these can be returned after the change took effect: when the vault
/// was unlocked and cannot be reopened under the new key. It is then locked
/// and opens with the new password.
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

/// Locks the vault, emits `vault-locked` and returns the vault's status.
///
/// Works in every state; the event is emitted even when the vault was already
/// locked.
///
/// # Errors
///
/// Returns `task_failed` when the blocking task panics.
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

/// Writes a portable archive of the encrypted vault to a path chosen in a
/// native save dialog.
///
/// Requires an existing vault, locked or unlocked, and leaves its lock state
/// as it was. The suggested file name is
/// `oikonomia-backup-YYYY-MM-DD.oikonomia-backup`, using the local calendar
/// date, and the extension is added to a chosen name that lacks it. An
/// unlocked vault is snapshotted with `VACUUM INTO`, so the copy is
/// consistent without closing `SQLCipher`. The archive is `vault.db` plus
/// `vault.header.json` only: it is not re-encrypted and never stores the
/// master password.
///
/// Returns the destination path, or `None` if the user cancelled.
///
/// # Errors
///
/// Returns `save_location_invalid` when the dialog's answer is not a path,
/// `vault_uninitialized` when there is no vault to back up, `vault_corrupt`
/// when the vault files are incomplete, `vault_unlock_before_backup` when a
/// locked vault needs one unlock first (its write-ahead log holds changes or
/// a password change did not finish),
/// `io` when the archive cannot be written or a vault file becomes shorter
/// while it is copied, `database` when the open vault cannot be snapshotted,
/// and `crypto` when that snapshot is not encrypted. Returns `task_failed`
/// when a blocking task panics.
#[tauri::command]
pub(crate) async fn vault_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let file_name = default_backup_file_name();
    let data_dir = state.data_dir().to_path_buf();
    let picked = run_blocking({
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
    })
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let destination = with_backup_extension(dialog_path(file_path, FileDialog::Save)?);

    with_vault_blocking(&state, move |vault| {
        vault.backup_to(&destination)?;
        Ok(destination.display().to_string())
    })
    .await
    .map(Some)
}

/// Restores a portable vault archive and leaves the vault locked.
///
/// `path` is the archive to unpack. When it is `None`, a native open dialog
/// chooses the file. A given path is accepted only if the user picked it in
/// that dialog ([`GrantPurpose::Backup`]), which is the path
/// [`vault_pick_backup`] returned, so the webview cannot name arbitrary
/// files. A file the user dropped on a window or picked for a CSV import is
/// refused, whatever it holds.
///
/// The session is locked, and `vault-locked` emitted if it was unlocked,
/// before the dialog opens, so cancelling the dialog still leaves the vault
/// locked. Nothing is decrypted; the owner unlocks afterwards with the
/// password the archive was made under.
///
/// Existing vault files are not overwritten unless `replace` is `true`. A
/// data directory without a vault accepts `replace: false`.
///
/// Returns the archive path that was restored, or `None` if the user
/// cancelled the dialog.
///
/// # Errors
///
/// Returns `path_not_granted` for a path the user did not pick in the backup
/// dialog, before the session is touched; `open_location_invalid` when the
/// dialog's answer is not a path; `backup_invalid` when the file is not a
/// backup archive;
/// `restore_would_overwrite` when a vault exists and `replace` is `false`;
/// and `io` or `vault_corrupt` when the vault files cannot be replaced.
/// Returns `task_failed` when a blocking task panics.
#[tauri::command]
pub(crate) async fn vault_restore<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    path: Option<String>,
    replace: bool,
) -> CommandResult<Option<String>> {
    // Refuse an ungranted path before touching the session.
    let granted = match path {
        Some(chosen) => {
            let grants = state.path_grants();
            let check = move || require_granted_path(&grants, GrantPurpose::Backup, &chosen);
            Some(run_blocking(check).await?)
        }
        None => None,
    };
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

/// Asks for a backup archive with a native open dialog and returns its path.
///
/// Works in every state and changes nothing: it does not restore, lock, or
/// write vault files. The chosen path is granted, so the frontend can confirm
/// with the user and then pass it to [`vault_restore`]. Returns `None` if the
/// user cancelled.
///
/// # Errors
///
/// Returns `open_location_invalid` when the dialog's answer is not a path,
/// and `task_failed` when the blocking task panics.
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

/// Asks for a `.oikonomia-backup` file with a native open dialog.
///
/// The chosen path is granted as a backup and nothing else, so that
/// [`vault_restore`] accepts it back. Returns `None` if the user cancelled.
///
/// # Errors
///
/// Returns `open_location_invalid` when the dialog's answer is not a path,
/// and `task_failed` when the blocking task panics.
async fn pick_backup_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
) -> CommandResult<Option<PathBuf>> {
    let app = app.clone();
    let data_dir = state.data_dir().to_path_buf();
    let grants = state.path_grants();

    run_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        // The preferences read, like the grant below, is file I/O, which
        // stays off the async workers.
        let filter_label = crate::tray::backup_filter_label(load_ui_prefs(&data_dir).locale);
        let dialog = app
            .dialog()
            .file()
            .add_filter(filter_label, &[BACKUP_EXTENSION]);
        let Some(picked) = dialog.blocking_pick_file() else {
            return Ok(None);
        };

        let path = dialog_path(picked, FileDialog::OpenBackup)?;
        grants.grant(GrantPurpose::Backup, [path.clone()]);
        Ok(Some(path))
    })
    .await
}

/// Locks the vault, and emits `vault-locked` if that ended an unlocked
/// session.
///
/// # Errors
///
/// Returns `task_failed` when the blocking task panics.
async fn lock_vault_session<R: Runtime>(
    app: &tauri::AppHandle<R>,
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

/// Returns `path` with the backup extension appended, unless it already ends
/// in exactly that extension.
///
/// A path with no file name becomes `oikonomia.oikonomia-backup`.
fn with_backup_extension(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension == BACKUP_EXTENSION => path,
        _ => {
            let mut name = path.file_name().map_or_else(
                || std::ffi::OsString::from("oikonomia"),
                std::ffi::OsString::from,
            );
            name.push(".");
            name.push(BACKUP_EXTENSION);
            match path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
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
