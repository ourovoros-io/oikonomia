//! Helpers shared by the command modules.
//!
//! Four kinds of helper live here: the ones that take work to the blocking
//! pool ([`run_blocking`], [`with_vault_blocking`], [`with_connection`],
//! [`with_localized_connection`]), the ones that check what the webview sent
//! ([`require_granted_path`], [`decode_capped_base64`]), the native dialogs
//! more than one module opens ([`save_with_dialog`], [`dialog_path`]), and
//! [`Arguments`], which reads the arguments of a command as one struct.
//!
//! Nothing here is a command. The rules these helpers implement are stated in
//! the [module above](crate::commands).

use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::{AppState, GrantPurpose, PathGrants, VaultGuard};
use base64::Engine;
use oikonomia_core::error::{Error as CoreError, ValidationError};
use oikonomia_core::prefs::{Locale, load_ui_prefs};
use oikonomia_core::vault::Connection;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tauri::ipc::{CommandArg, CommandItem, InvokeBody, InvokeError};
use tauri::{Runtime, State};
use tauri_plugin_dialog::FilePath;

/// The named arguments of a command, read together as one `T`.
///
/// Tauri reads each parameter of a command from the payload key of the same
/// name. A command with many arguments would need as many parameters, so it
/// takes one `Arguments<T>` instead: `T` is deserialized from the whole
/// payload object, whose keys the webview sends in camelCase. The name of the
/// parameter itself is not a key of the payload.
///
/// What crosses IPC is the same either way, and so is a missing optional
/// key: serde reads a missing `Option` field of `T` as `None`.
///
/// The wrapper is needed because Tauri already takes every type that
/// implements `Deserialize` as an argument read from its own key, so `T`
/// cannot be given another reading directly.
#[derive(Debug)]
pub(crate) struct Arguments<T>(pub(crate) T);

impl<'de, T, R> CommandArg<'de, R> for Arguments<T>
where
    T: Deserialize<'de>,
    R: Runtime,
{
    /// Reads `T` from the whole JSON payload of the invocation.
    ///
    /// # Errors
    ///
    /// Returns an [`InvokeError`] carrying the reason as text when the
    /// payload is raw bytes, or when it does not deserialize as a `T`. Like
    /// every argument Tauri refuses, it reaches the webview without a code.
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        match command.message.payload() {
            InvokeBody::Json(payload) => T::deserialize(payload)
                .map(Self)
                .map_err(InvokeError::from_error),
            InvokeBody::Raw(_) => Err(InvokeError::from(format!(
                "command {} expects named arguments but the IPC call used a bytes payload",
                command.name
            ))),
        }
    }
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

/// Runs `work` on the blocking pool with exclusive access to the vault.
///
/// No command waits for the vault mutex on the main thread or on an async
/// worker; a password change holds it for seconds. The call counts as
/// activity for the idle watchdog.
///
/// Whatever status `work` leaves the vault in, the guard brings the idle
/// watchdog in line with it before the vault mutex is released
/// ([`crate::state::VaultGuard`]), so no command updates the watchdog itself.
///
/// # Errors
///
/// Returns the error `work` returns, and `task_failed` when the blocking task
/// panics. After such a panic the next command finds the vault locked
/// ([`GatedVault::acquire`](crate::state::GatedVault::acquire)).
pub(super) async fn with_vault_blocking<T, F>(
    state: &State<'_, AppState>,
    work: F,
) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut VaultGuard<'_>) -> Result<T, CoreError> + Send + 'static,
{
    let vault = state.vault();
    state.touch();

    run_blocking(move || {
        let mut guard = vault.acquire();
        work(&mut guard).map_err(CommandError::from)
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

/// Runs `work` on the blocking pool with the unlocked vault's connection and
/// the language ledger text is written in ([`stored_text_locale`]).
///
/// For the commands whose core call writes text into the books: seeded
/// account names, generated entry descriptions.
///
/// # Errors
///
/// Returns `vault_locked` when the vault is not unlocked, the error `work`
/// returns, and `task_failed` when the blocking task panics.
pub(super) async fn with_localized_connection<T, F>(
    state: &State<'_, AppState>,
    work: F,
) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection, Locale) -> Result<T, CoreError> + Send + 'static,
{
    let vault = state.vault();
    let data_dir = state.data_dir().to_path_buf();
    state.touch();

    run_blocking(move || {
        // Read before the vault is taken; see `stored_text_locale`.
        let locale = stored_text_locale(&data_dir);

        let guard = vault.acquire();
        Ok(work(guard.connection()?, locale)?)
    })
    .await
}

/// Returns the language that text written into the user's books must be in.
///
/// Read from the preferences file in `data_dir`, never taken from the
/// webview: the UI cannot choose the language of ledger text.
///
/// This reads a file, so it runs on the blocking pool, and before the vault
/// is taken, so the plaintext preferences file is not touched while the
/// vault mutex is held.
///
/// The read takes no lock. [`save_ui_prefs`](oikonomia_core::prefs::save_ui_prefs)
/// writes a temporary file and renames it over the preferences file, so a
/// read during a language change sees the old or the new file, complete.
/// [`AppState::lock_prefs`] is for a load-change-save, which this is not, and
/// taking it here would make every caller wait behind a save's fsync.
pub(super) fn stored_text_locale(data_dir: &Path) -> Locale {
    load_ui_prefs(data_dir).locale
}

/// Accepts a webview-supplied path only if the user handed it to the app for
/// `purpose`, through a native drop or dialog ([`AppState::grant_paths`]).
///
/// Returns the resolved path that was checked, which is the one to open
/// ([`PathGrants::resolve`]). Resolving reads the filesystem, so this runs on
/// the blocking pool.
///
/// # Errors
///
/// Returns `path_not_granted` when the path resolves to nothing the user
/// handed over for `purpose`. That includes a path that does not exist, and
/// one handed over for another purpose only.
pub(super) fn require_granted_path(
    grants: &PathGrants,
    purpose: GrantPurpose,
    path: &str,
) -> CommandResult<PathBuf> {
    grants.resolve(purpose, Path::new(path)).ok_or_else(|| {
        CommandError::desktop(
            DesktopError::PathNotGranted,
            "file path was not chosen through the app for this use",
        )
    })
}

/// Returns the name a dropped document is stored and typed under: the last
/// component of the path as the user dropped it.
///
/// Taken before the path is resolved, because a dropped link keeps its own
/// name and extension while the file it points to may be named anything. A
/// path with no last component, or one that is not UTF-8, gets `document`.
pub(super) fn dropped_file_name(dropped_path: &str) -> String {
    Path::new(dropped_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document")
        .to_owned()
}

/// Decodes a document the webview picked, up to the size core stores
/// ([`oikonomia_core::documents::MAX_DOCUMENT_BYTES`]).
///
/// The drop path applies the same cap from the file's metadata, before
/// reading it.
///
/// # Errors
///
/// Returns `file_too_large` for a payload over the cap and
/// `file_data_invalid` for one that is not base64.
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
///
/// # Errors
///
/// Returns `file_too_large`, with the cap in mebibytes as `max_mb`, for a
/// payload over the cap, and `file_data_invalid` for one that is not base64.
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
        .map_err(|err| {
            CommandError::desktop(
                DesktopError::FileDataInvalid,
                format!("invalid file data: {err}"),
            )
        })?;

    if data.len() > max_decoded {
        return Err(too_large_error(max_decoded));
    }
    Ok(data)
}

/// Builds the error for a payload over a cap of `max_decoded` bytes, with the
/// code and parameter core uses for a stored document that is too large.
fn too_large_error(max_decoded: usize) -> CommandError {
    let max_megabytes = max_decoded / (1024 * 1024);

    CommandError::from(CoreError::from(ValidationError::FileTooLarge {
        max_mb: u64::try_from(max_megabytes).unwrap_or(u64::MAX),
    }))
}

/// What a native save dialog offers, and how the chosen path is completed.
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
///
/// # Errors
///
/// Returns `save_location_invalid` when the dialog's answer is not a path
/// ([`dialog_path`]), `save_failed` when the file cannot be written, and
/// `task_failed` when the blocking task panics.
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

        let destination = (target.complete_path)(dialog_path(picked, FileDialog::Save)?);

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

/// A native file dialog the app opens, which decides the code for an answer
/// that is not a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FileDialog {
    /// A save dialog: an export, or a backup to write.
    Save,
    /// The open dialog for a backup archive to restore.
    OpenBackup,
    /// The open dialog for a bank statement to import.
    OpenCsv,
}

impl FileDialog {
    /// Returns the error an answer of this dialog that is not a path is
    /// reported as: one code for the save dialogs, another for the open
    /// ones.
    const fn not_a_path(self) -> DesktopError {
        match self {
            Self::Save => DesktopError::SaveLocationInvalid,
            Self::OpenBackup | Self::OpenCsv => DesktopError::OpenLocationInvalid,
        }
    }

    /// Returns the words that name the dialog in the diagnostic message.
    const fn label(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::OpenBackup => "backup",
            Self::OpenCsv => "CSV",
        }
    }
}

/// Converts the location the native file dialog `dialog` returned into a
/// filesystem path.
///
/// # Errors
///
/// Returns `save_location_invalid` for a save dialog and
/// `open_location_invalid` for an open dialog when the location is not a
/// path. A desktop dialog returns paths; the plugin's other form is a URI,
/// which mobile systems hand out.
pub(super) fn dialog_path(picked: FilePath, dialog: FileDialog) -> CommandResult<PathBuf> {
    picked.into_path().map_err(|err| {
        CommandError::desktop(
            dialog.not_a_path(),
            format!("invalid {} location: {err}", dialog.label()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oikonomia_core::documents::MAX_DOCUMENT_BYTES;
    use oikonomia_core::prefs::{UiPrefs, save_ui_prefs};

    /// Creates a fresh directory for one test.
    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oiko-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since_epoch| since_epoch.as_nanos())
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

        let grants = state.path_grants();
        let backup = GrantPurpose::Backup;

        let refused = require_granted_path(&grants, backup, text).expect_err("ungranted path");
        assert_eq!(refused.code, "path_not_granted");

        state.grant_paths(backup, [archive.clone()]);
        assert_eq!(
            require_granted_path(&grants, backup, text).expect("granted path"),
            archive.canonicalize().expect("canonical")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_handed_over_for_another_purpose_is_refused_as_not_granted() {
        let dir = temp_dir("path-purpose");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");
        let statement = dir.join("statement.csv");
        std::fs::write(&statement, b"date,amount\n").expect("write");
        let text = statement.to_str().expect("utf-8 path");
        let grants = state.path_grants();

        // What a CSV pick and a file drop record.
        state.grant_paths(GrantPurpose::Csv, [statement.clone()]);
        state.grant_paths(GrantPurpose::Document, [statement.clone()]);

        let refused =
            require_granted_path(&grants, GrantPurpose::Backup, text).expect_err("other purpose");
        assert_eq!(refused.code, "path_not_granted");
        assert_eq!(refused.params, std::collections::BTreeMap::new());
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
        state.grant_paths(GrantPurpose::Document, [link.clone()]);
        let named = link.to_str().expect("utf-8 path");

        let accepted = require_granted_path(&state.path_grants(), GrantPurpose::Document, named)
            .expect("granted");

        assert_eq!(accepted, real.canonicalize().expect("canonical"));
        // The document keeps the name it was dropped under.
        assert_eq!(dropped_file_name(named), "link.csv");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dialog_answer_that_is_not_a_path_is_coded_by_the_kind_of_dialog() {
        // What a mobile system hands out in place of a path.
        let not_a_path = || {
            let uri = "content://media/external/file/42".parse::<tauri::Url>();
            FilePath::Url(uri.expect("a uri"))
        };
        let cases = [
            (FileDialog::Save, "save_location_invalid"),
            (FileDialog::OpenBackup, "open_location_invalid"),
            (FileDialog::OpenCsv, "open_location_invalid"),
        ];

        for (dialog, code) in cases {
            let refused = dialog_path(not_a_path(), dialog).expect_err("not a path");

            assert_eq!(refused.code, code, "{dialog:?}");
            assert!(refused.message.contains(dialog.label()), "{refused}");
        }
    }

    #[test]
    fn a_dialog_answer_that_is_a_path_is_returned_as_it_is() {
        let picked = std::path::PathBuf::from("books").join("2026.csv");

        for dialog in [
            FileDialog::Save,
            FileDialog::OpenBackup,
            FileDialog::OpenCsv,
        ] {
            assert_eq!(
                dialog_path(FilePath::Path(picked.clone()), dialog).expect("a path"),
                picked,
                "{dialog:?}"
            );
        }
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
                let _ = sender.send(stored_text_locale(state.data_dir()));
            });

            // The bound only turns a blocked reader into a failure instead of
            // a hung test; a reader that does not lock answers at once.
            let read = receiver.recv_timeout(std::time::Duration::from_secs(5));
            drop(writer);

            assert_eq!(read.ok(), Some(Locale::En));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Exercises the helper every ledger-text command reads the language with.
    ///
    /// The commands themselves take a Tauri `State`, which cannot be constructed
    /// in a unit test, so that each of them goes through
    /// `with_localized_connection` or calls this helper inside its blocking
    /// closure is covered by reading the code.
    #[test]
    fn ledger_text_takes_its_language_from_the_stored_preference() {
        let dir = temp_dir("stored-text-locale");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");

        assert_eq!(stored_text_locale(state.data_dir()), Locale::En);

        for locale in [Locale::El, Locale::Fr, Locale::De, Locale::En] {
            let prefs = UiPrefs {
                locale,
                ..UiPrefs::default()
            };
            save_ui_prefs(&dir, &prefs).expect("save prefs");

            assert_eq!(stored_text_locale(state.data_dir()), locale);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Returns the base64 encoding of `length` bytes.
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

/// Scaffolding for tests that invoke a command through Tauri's mock IPC, the
/// way the webview invokes it.
///
/// Such a test covers what a unit test of the command's body cannot: how
/// Tauri binds the payload the frontend sends to the command's parameters,
/// and how the answer and the error are serialized back.
///
/// A test starts a [`MockApp`] with the commands under test registered and a
/// closure that fills the vault, then calls [`MockApp::invoke`] with the
/// object the frontend passes to `invoke`:
///
/// ```ignore
/// let (app, ids) = MockApp::start("label", tauri::generate_handler![entry_list], seed);
/// let listed = app.invoke("entry_list", serde_json::json!({ "entityId": ids.entity }));
/// ```
///
/// # Not on Windows
///
/// The module, and every test module that uses it, is gated `#[cfg(test)]`
/// and `#[cfg(not(windows))]`, and the `tauri` `test` feature is a
/// dev-dependency of the other targets only. The gate is two attributes
/// because clippy allows `unwrap` in tests only under a plain `#[cfg(test)]`. A test executable carries no
/// application manifest, and with the mock runtime linked in it imports
/// webview and common-controls entry points that do not resolve without
/// one: the binary fails to start (`0xc0000139`,
/// `STATUS_ENTRYPOINT_NOT_FOUND`) and takes every desktop test with it. The
/// binding these tests cover is serde over the IPC payload, the same code on
/// every platform, so the Linux and macOS runs cover it.
#[cfg(test)]
#[cfg(not(windows))]
pub(crate) mod ipc_test_support {
    use crate::state::{AppState, GrantPurpose};
    use oikonomia_core::vault::Connection;
    use std::path::PathBuf;
    use tauri::Manager;
    use tauri::ipc::{CallbackFn, Invoke, InvokeBody};
    use tauri::test::{
        INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
    };
    use tauri::webview::InvokeRequest;

    /// The master password of the vault a [`MockApp`] runs over.
    pub(crate) const PASSWORD: &str = "correct horse battery staple";

    /// A mock application over an unlocked vault in a temporary directory,
    /// with one webview to make IPC calls from.
    ///
    /// The directory is removed when the value is dropped.
    pub(crate) struct MockApp {
        /// The app; it owns the [`AppState`] the commands read.
        app: tauri::App<MockRuntime>,
        /// The webview the IPC calls are made from.
        webview: tauri::WebviewWindow<MockRuntime>,
        /// The vault's directory.
        data_dir: PathBuf,
    }

    impl MockApp {
        /// Starts a mock app whose invoke handler is `handler`, normally
        /// `tauri::generate_handler![...]` naming the commands under test.
        ///
        /// A new vault is created and unlocked with [`PASSWORD`], and `seed`
        /// is run on its connection before the app is built; what `seed`
        /// returns, such as the ids it created, is returned beside the app.
        /// `label` goes into the directory name, to tell tests apart.
        ///
        /// # Panics
        ///
        /// Panics when the directory, the vault or the mock app cannot be
        /// set up.
        pub(crate) fn start<S>(
            label: &str,
            handler: impl Fn(Invoke<MockRuntime>) -> bool + Send + Sync + 'static,
            seed: impl FnOnce(&Connection) -> S,
        ) -> (Self, S) {
            let data_dir = std::env::temp_dir().join(format!(
                "oiko-ipc-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since_epoch| since_epoch.as_nanos())
            ));
            std::fs::create_dir_all(&data_dir).unwrap();
            let state = AppState::open_path(data_dir.clone(), data_dir.clone()).unwrap();

            let seeded = {
                let vault = state.vault();
                let mut guard = vault.acquire();
                guard.init(PASSWORD).unwrap();
                seed(guard.connection().unwrap())
            };

            let app = mock_builder()
                .manage(state)
                .invoke_handler(handler)
                .build(mock_context(noop_assets()))
                .unwrap();
            let webview =
                tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
                    .build()
                    .unwrap();

            let app = Self {
                app,
                webview,
                data_dir,
            };
            (app, seeded)
        }

        /// Writes `bytes` to a new file named `name` in the app's temporary
        /// directory and returns its path as the webview would name it.
        ///
        /// # Panics
        ///
        /// Panics when the file cannot be written or its path is not UTF-8.
        pub(crate) fn write_file(&self, name: &str, bytes: &[u8]) -> String {
            let path = self.data_dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path.to_str().unwrap().to_owned()
        }

        /// Grants `path` for `purpose`, as the native dialog or drop that
        /// stands behind that purpose does.
        pub(crate) fn grant(&self, purpose: GrantPurpose, path: &str) {
            self.app
                .state::<AppState>()
                .grant_paths(purpose, [PathBuf::from(path)]);
        }

        /// Invokes `command` with `arguments` as the JSON payload, as
        /// `invoke(command, arguments)` does in the webview.
        ///
        /// # Errors
        ///
        /// Returns what the webview's promise would be rejected with: the
        /// serialized command error, or the text of Tauri's own refusal.
        ///
        /// # Panics
        ///
        /// Panics when the answer is not JSON.
        pub(crate) fn invoke(
            &self,
            command: &str,
            arguments: serde_json::Value,
        ) -> Result<serde_json::Value, serde_json::Value> {
            let origin = if cfg!(target_os = "android") {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            };
            let request = InvokeRequest {
                cmd: command.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: origin.parse().unwrap(),
                body: InvokeBody::Json(arguments),
                headers: tauri::http::HeaderMap::default(),
                invoke_key: INVOKE_KEY.to_owned(),
            };

            get_ipc_response(&self.webview, request).map(|body| body.deserialize().unwrap())
        }
    }

    impl Drop for MockApp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }
}
