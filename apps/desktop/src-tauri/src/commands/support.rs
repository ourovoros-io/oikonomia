//! Helpers shared by the command modules.

use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::{AppState, VaultGuard};
use base64::Engine;
use oikonomia_core::error::{Error as CoreError, ValidationError};
use oikonomia_core::prefs::{Locale, load_ui_prefs};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tauri::State;
use tauri_plugin_dialog::FilePath;

/// Decodes a document the webview picked, up to the size core stores
/// ([`oikonomia_core::documents::MAX_DOCUMENT_BYTES`]). The drop path applies
/// the same cap from the file's metadata, before reading it.
pub(super) fn decode_document_base64(data_base64: &str) -> CommandResult<Vec<u8>> {
    decode_capped_base64(data_base64, oikonomia_core::documents::MAX_DOCUMENT_BYTES)
}

/// Decodes base64 from the webview into at most `max_decoded` bytes.
///
/// Surrounding whitespace is ignored for the size check and the decode alike.
/// The encoded length is checked first, so a huge payload is refused before
/// it is decoded into memory. That check alone lets through up to three
/// bytes over the cap, because it allows one more base64 group than the cap
/// needs, so the decoded length is checked as well.
pub(super) fn decode_capped_base64(
    bytes_base64: &str,
    max_decoded: usize,
) -> CommandResult<Vec<u8>> {
    let trimmed = bytes_base64.trim();
    let max_base64_len = max_decoded / 3 * 4 + 4;
    if trimmed.len() > max_base64_len {
        return Err(too_large_error(max_decoded));
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|e| {
            CommandError::desktop(
                DesktopError::FileDataInvalid,
                format!("invalid file data: {e}"),
            )
        })?;

    if data.len() > max_decoded {
        return Err(too_large_error(max_decoded));
    }
    Ok(data)
}

/// The error for a payload over a cap of `max_decoded` bytes, with the code
/// and parameter core uses for a stored document that is too large.
pub(super) fn too_large_error(max_decoded: usize) -> CommandError {
    let max_megabytes = max_decoded / (1024 * 1024);

    CommandError::from(CoreError::Validation(ValidationError::FileTooLarge {
        max_mb: u64::try_from(max_megabytes).unwrap_or(u64::MAX),
    }))
}

/// The language that text written into the user's books must be in.
///
/// Read from the stored preference on the desktop side, never from the
/// webview: the UI cannot choose the language of ledger text, and it is read
/// before the vault closure so the plaintext preferences file is not touched
/// while the vault lock is held.
///
/// The read takes no lock. [`save_ui_prefs`](oikonomia_core::prefs::save_ui_prefs) writes a temporary file and
/// renames it over the preferences file, so a read during a language change
/// sees the old or the new file, complete. [`AppState::lock_prefs`] is for a
/// load-change-save, which this is not, and taking it here would make every
/// caller wait behind a save's fsync.
pub(super) fn stored_text_locale(state: &AppState) -> Locale {
    load_ui_prefs(state.data_dir()).locale
}

/// Accept a webview-supplied path only if the user handed it to the app
/// through a native drop or dialog ([`AppState::grant_paths`]).
///
/// Returns the resolved path that was checked, which is the one to open
/// ([`AppState::granted_path`]).
pub(super) fn require_granted_path(state: &AppState, path: &str) -> CommandResult<PathBuf> {
    state.granted_path(Path::new(path)).ok_or_else(|| {
        CommandError::desktop(
            DesktopError::PathNotGranted,
            "file path was not chosen through the app",
        )
    })
}

/// The name a dropped document is stored and typed under: the last component
/// of the path as the user dropped it.
///
/// Taken before the path is resolved, because a dropped link keeps its own
/// name and extension while the file it points to may be named anything.
pub(super) fn dropped_file_name(dropped_path: &str) -> String {
    Path::new(dropped_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document")
        .to_owned()
}

/// Run vault work on the blocking pool: no command ever waits for the vault
/// mutex on the main thread or an async runtime worker (e.g. while a rekey
/// holds it for seconds).
///
/// Whatever status `f` leaves the vault in, the guard brings the idle
/// watchdog in line with it before the vault mutex is released
/// ([`crate::state::VaultGuard`]), so no command updates the watchdog itself.
pub(super) async fn with_vault_blocking<T, F>(state: &State<'_, AppState>, f: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut VaultGuard<'_>) -> Result<T, CoreError> + Send + 'static,
{
    let vault = state.vault();
    state.touch();

    run_blocking(move || {
        let mut guard = vault.acquire();
        f(&mut guard).map_err(CommandError::from)
    })
    .await
}

/// Runs `work` on the blocking pool with the unlocked vault's connection.
///
/// This is [`with_vault_blocking`] for the common command that only queries
/// or writes the ledger and never changes the vault's own state.
///
/// # Errors
///
/// Returns `vault_locked` when the vault is not unlocked, the error `work`
/// returns, and `task_failed` when the blocking task panics.
pub(super) async fn with_connection<T, F>(state: &State<'_, AppState>, work: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, CoreError> + Send + 'static,
{
    with_vault_blocking(state, move |vault| work(vault.connection()?)).await
}

/// Runs `work` on the runtime's blocking pool and returns what it returns.
///
/// For work that may block a thread: waiting for the vault mutex, a native
/// dialog, file I/O. An async worker that blocks stalls every other command
/// scheduled on it.
///
/// # Errors
///
/// Returns the error `work` returns, and `task_failed` when the task panics
/// or the runtime drops it.
///
/// # Cancel safety
///
/// Dropping the returned future does not stop `work`: once the task is
/// spawned, which happens when the future is first polled, it runs to the
/// end and its result is discarded.
pub(super) async fn run_blocking<T, F>(work: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(result) => result,
        Err(err) => Err(CommandError::desktop(
            DesktopError::TaskFailed,
            format!("background task failed: {err}"),
        )),
    }
}

/// Converts the location a native file dialog returned into a filesystem path.
///
/// `purpose` names the dialog in the diagnostic message: `"save"`,
/// `"backup"`, `"CSV"`.
///
/// # Errors
///
/// Returns `save_location_invalid`, for open dialogs as well as save dialogs,
/// when the location is not a path. A desktop dialog returns paths; the
/// plugin's other form is a URI, which mobile systems hand out.
pub(super) fn dialog_path(picked: FilePath, purpose: &str) -> CommandResult<PathBuf> {
    picked.into_path().map_err(|err| {
        CommandError::desktop(
            DesktopError::SaveLocationInvalid,
            format!("invalid {purpose} location: {err}"),
        )
    })
}

/// What a native Save dialog offers, and how the chosen path is completed.
pub(super) struct SaveTarget {
    /// File type filter as a label and its extensions; `None` offers every file.
    pub(super) filter: Option<(&'static str, &'static [&'static str])>,
    /// File name the dialog suggests.
    pub(super) file_name: String,
    /// Applied to the chosen path before writing, to add a missing extension.
    pub(super) complete_path: fn(PathBuf) -> PathBuf,
}

/// Asks where to save with a native dialog, then writes `bytes` there.
///
/// Both steps run in one task on the blocking pool. The dialog blocks until
/// the user answers, and the write is file I/O of up to tens of megabytes;
/// neither may hold an async worker.
///
/// Returns the path written, or `None` if the user cancelled.
pub(super) async fn save_with_dialog(
    app: &tauri::AppHandle,
    target: SaveTarget,
    bytes: Vec<u8>,
) -> CommandResult<Option<String>> {
    let app = app.clone();

    run_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        let mut dialog = app.dialog().file().set_file_name(&target.file_name);
        if let Some((label, extensions)) = target.filter {
            dialog = dialog.add_filter(label, extensions);
        }
        let Some(picked) = dialog.blocking_save_file() else {
            return Ok(None);
        };

        let destination = (target.complete_path)(dialog_path(picked, "save")?);

        std::fs::write(&destination, &bytes).map_err(|err| {
            CommandError::desktop(
                DesktopError::SaveFailed,
                format!("could not save file: {err}"),
            )
        })?;
        Ok(Some(destination.display().to_string()))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use oikonomia_core::documents::MAX_DOCUMENT_BYTES;
    use oikonomia_core::prefs::{UiPrefs, save_ui_prefs};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oiko-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    #[test]
    fn webview_supplied_paths_need_a_native_grant() {
        let dir = temp_dir("path-grant");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");
        let archive = dir.join("books.oikonomia-backup");
        std::fs::write(&archive, b"OIKOBACK").expect("write");
        let text = archive.to_str().expect("utf-8 path");

        let refused = require_granted_path(&state, text).expect_err("ungranted path");
        assert_eq!(refused.code, "path_not_granted");

        state.grant_paths([archive.clone()]);
        assert_eq!(
            require_granted_path(&state, text).expect("granted path"),
            archive.canonicalize().expect("canonical")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_granted_path_comes_back_with_its_links_resolved() {
        let dir = temp_dir("path-resolved");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");
        let real = dir.join("statement-2026.csv");
        std::fs::write(&real, b"date,amount\n").expect("write");
        let link = dir.join("link.csv");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        state.grant_paths([link.clone()]);
        let named = link.to_str().expect("utf-8 path");

        let accepted = require_granted_path(&state, named).expect("granted");

        assert_eq!(accepted, real.canonicalize().expect("canonical"));
        // The document keeps the name it was dropped under.
        assert_eq!(dropped_file_name(named), "link.csv");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dropped_document_is_named_by_the_last_component_of_its_path() {
        let dropped = std::path::Path::new("inbox").join("bill.pdf");

        assert_eq!(
            dropped_file_name(dropped.to_str().expect("utf-8 path")),
            "bill.pdf"
        );
        assert_eq!(dropped_file_name("bill.pdf"), "bill.pdf");
        // A path with no file name still gets one.
        assert_eq!(dropped_file_name(""), "document");
    }

    #[test]
    fn reading_the_ledger_language_does_not_wait_for_a_preferences_write() {
        let dir = temp_dir("locale-no-lock");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");
        let (sender, receiver) = std::sync::mpsc::channel();

        std::thread::scope(|scope| {
            let writer = state.lock_prefs();
            scope.spawn(|| {
                let _ = sender.send(stored_text_locale(&state));
            });

            // The bound only turns a blocked reader into a failure instead of
            // a hung test; a reader that does not lock answers at once.
            let read = receiver.recv_timeout(std::time::Duration::from_secs(5));
            drop(writer);

            assert_eq!(read.ok(), Some(Locale::En));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Exercises the locale helper every ledger-text command calls. The
    /// commands themselves take a Tauri `State`, which cannot be constructed
    /// in a unit test, so that each of them reads the language through this
    /// helper before entering the vault closure is covered by reading the code.
    #[test]
    fn ledger_text_takes_its_language_from_the_stored_preference() {
        let dir = temp_dir("stored-text-locale");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");

        assert_eq!(stored_text_locale(&state), Locale::En);

        for locale in [Locale::El, Locale::Fr, Locale::De, Locale::En] {
            let prefs = UiPrefs {
                locale,
                ..UiPrefs::default()
            };
            save_ui_prefs(&dir, &prefs).expect("save prefs");

            assert_eq!(stored_text_locale(&state), locale);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn encoded(length: usize) -> String {
        base64::engine::general_purpose::STANDARD.encode(vec![7_u8; length])
    }

    #[test]
    fn capped_decode_accepts_exactly_the_cap_and_refuses_one_byte_more() {
        // Caps of every length modulo three, since base64 works in threes.
        for cap in [3_usize, 4, 5, 6] {
            let at_cap = decode_capped_base64(&encoded(cap), cap).expect("at the cap");
            assert_eq!(at_cap.len(), cap, "cap {cap}");

            let over = decode_capped_base64(&encoded(cap + 1), cap).expect_err("one over");
            assert_eq!(over.code, "file_too_large", "cap {cap}");
        }
    }

    #[test]
    fn capped_decode_ignores_surrounding_whitespace_at_the_cap() {
        let padded = format!("\n  {}  \r\n", encoded(4));

        assert_eq!(decode_capped_base64(&padded, 4).expect("padded").len(), 4);

        let padded_over = format!("\n  {}  \r\n", encoded(5));
        let over = decode_capped_base64(&padded_over, 4).expect_err("one over");
        assert_eq!(over.code, "file_too_large");
    }

    #[test]
    fn a_picked_document_is_capped_at_the_size_core_stores() {
        let at_cap = format!("  {}\n", encoded(MAX_DOCUMENT_BYTES));
        assert_eq!(
            decode_document_base64(&at_cap).expect("at the cap").len(),
            MAX_DOCUMENT_BYTES
        );

        let over = decode_document_base64(&encoded(MAX_DOCUMENT_BYTES + 1)).expect_err("over");
        assert_eq!(over.code, "file_too_large");
        assert_eq!(over.params.get("max_mb").map(String::as_str), Some("8"));

        let invalid = decode_document_base64("not-valid-base64!!!").expect_err("invalid");
        assert_eq!(invalid.code, "file_data_invalid");
    }

    #[test]
    fn decode_pdf_export_rejects_oversized_before_decode() {
        // max_decoded=2 → max_base64 = 4; six chars must fail before decode.
        let err = decode_capped_base64("AAAAAA", 2).expect_err("cap");
        assert_eq!(err.code, "file_too_large");
        assert!(err.message.contains("large"), "{}", err.message);
    }

    #[test]
    fn decode_pdf_export_rejects_oversized_after_decode() {
        // max_decoded=3 → max_base64 = 8; 4 decoded bytes encode to 8 chars.
        let encoded = base64::engine::general_purpose::STANDARD.encode([1_u8, 2, 3, 4]);
        assert_eq!(encoded.len(), 8);
        let err = decode_capped_base64(&encoded, 3).expect_err("after decode");
        assert_eq!(err.code, "file_too_large");
    }
}
