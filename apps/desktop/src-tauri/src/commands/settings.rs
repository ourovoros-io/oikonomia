//! Settings commands: the idle lock timeout and the plaintext preferences.
//!
//! The two kinds of setting live in different places. The lock timeout is
//! stored inside the vault, so its commands require the unlocked vault. The
//! language and the quick-add window's last choices are stored in a plaintext
//! preferences file beside the vault, so that the tray and the unlock screen
//! can use them before a password is entered; their commands work in every
//! vault state and go through [`with_prefs_blocking`].
//!
//! # Preferences save errors
//!
//! A command that saves the preferences file returns what core's save
//! returns ([`oikonomia_core::prefs::save_ui_prefs`]):
//!
//! - `io` when the file cannot be written;
//! - `prefs_unreadable` when a file is there and cannot be read or does not
//!   decode.
//!
//! In the second case core has refused to replace the file and it is left as
//! it was: the command changed nothing. The commands that only read the
//! preferences answer with the defaults for such a file and never fail on
//! it; [`settings_get_ui_prefs`] also says so, in its `unreadable` field,
//! which is how the Settings screen knows to offer
//! [`settings_reset_ui_prefs`].

use crate::commands::support::{run_blocking, with_connection, with_vault_blocking};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use oikonomia_core::domain::EntityId;
use oikonomia_core::ledger::{SimpleEntryKind, get_lock_timeout_secs, set_lock_timeout_secs};
use oikonomia_core::prefs::{
    LastRoleAccounts, Locale, UiPrefsView, load_ui_prefs, load_ui_prefs_view, remember_last_entity,
    remember_quick_add, reset_unreadable_ui_prefs, resolve_locale, store_locale,
};
use tauri::{Manager, Runtime, State};

/// Returns the idle time, in seconds, after which the vault locks itself.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn settings_get_lock_timeout(state: State<'_, AppState>) -> CommandResult<u64> {
    with_connection(&state, get_lock_timeout_secs).await
}

/// Stores the idle time, in seconds, after which the vault locks itself, and
/// hands the new value to the idle watchdog.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `lock_timeout_too_short` (with the minimum as `min_secs`) for a value
/// under the minimum, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn settings_set_lock_timeout(
    state: State<'_, AppState>,
    secs: u64,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        set_lock_timeout_secs(vault.connection()?, secs)?;

        // Under the same guard as the stored value, so that two changes
        // cannot leave the watchdog's copy and the vault disagreeing.
        vault.set_lock_timeout_cache(secs);
        Ok(())
    })
    .await
}

/// Returns the stored app language.
///
/// Works in every vault state: the preference is plaintext so that the tray
/// and native dialogs are in the user's language before a password is
/// entered. A missing or unreadable preferences file yields the default
/// language.
///
/// # Errors
///
/// Returns `app_state_unavailable` when the application state was never set
/// up, and `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn settings_get_locale(app: tauri::AppHandle) -> CommandResult<Locale> {
    with_prefs_blocking(app, |_app, state| {
        Ok(load_ui_prefs(state.data_dir()).locale())
    })
    .await
}

/// Stores the app language, then rebuilds the tray menu and retitles the
/// quick-add window in it.
///
/// Works in every vault state.
///
/// # Errors
///
/// Returns the [preferences save errors](self#preferences-save-errors), in
/// which case the tray and the window keep the language they had,
/// `app_state_unavailable` when the application state was never set up, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn settings_set_locale(
    app: tauri::AppHandle,
    locale: Locale,
) -> CommandResult<()> {
    with_prefs_blocking(app, move |app, state| {
        let prefs_guard = state.lock_prefs();
        store_locale(state.data_dir(), locale)?;
        drop(prefs_guard);

        crate::tray::apply_locale(app, locale);
        Ok(())
    })
    .await
}

/// Returns the app language, choosing it from the system's languages on the
/// very first run.
///
/// `system_languages` is the webview's report of the system's preferred
/// languages (`navigator.languages`); Rust decides everything else. When a
/// language is already stored it is returned unchanged and nothing is
/// written, so the system is consulted once per installation. Otherwise the
/// first supported language is stored and the native strings are refreshed
/// exactly as after a change in Settings. Works in every vault state and
/// before a vault exists, and is safe to call on every launch.
///
/// # Errors
///
/// Returns the [preferences save errors](self#preferences-save-errors) when
/// a language is chosen and cannot be stored, which includes every call made
/// while the preferences file is there and cannot be read or is not a JSON
/// object, `app_state_unavailable` when the application state was never set up, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn settings_resolve_locale(
    app: tauri::AppHandle,
    system_languages: Vec<String>,
) -> CommandResult<Locale> {
    with_prefs_blocking(app, move |app, state| {
        let prefs_guard = state.lock_prefs();
        let resolution = resolve_locale(state.data_dir(), &system_languages)?;
        drop(prefs_guard);

        // The tray was built at startup from the stored language, English on
        // a first run, so it only needs a rebuild when this call stored a new
        // one.
        if resolution.newly_stored {
            crate::tray::apply_locale(app, resolution.locale);
        }

        Ok(resolution.locale)
    })
    .await
}

/// Returns the whole plaintext preferences: the language and what the
/// quick-add window last used.
///
/// Works in every vault state. A missing or unreadable preferences file
/// yields the defaults; `unreadable` is `true` for a file that is there and
/// could not be read or decoded, and `false` otherwise.
///
/// The locale sent is the one in effect, so the web UI never sees a stored
/// value this build does not know.
///
/// # Errors
///
/// Returns `app_state_unavailable` when the application state was never set
/// up, and `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn settings_get_ui_prefs<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> CommandResult<UiPrefsView> {
    with_prefs_blocking(app, |_app, state| Ok(load_ui_prefs_view(state.data_dir()))).await
}

/// Moves a damaged preferences file aside, to `ui-prefs.damaged.json` in the
/// same folder, and returns the preferences as they are afterwards.
///
/// Works in every vault state. A file is damaged when it is not UTF-8 or
/// does not decode. It is moved, not deleted, and replaces the file an
/// earlier reset left under that name. No preferences file is left, so the
/// next save writes a new one. A preferences file that is missing, blank or
/// readable is left as it is and the call succeeds.
///
/// The tray and the window titles are not rebuilt: the reset stores no
/// preference, so the language in effect is the default before it and after
/// it.
///
/// # Errors
///
/// Returns `io` when the file cannot be moved, or cannot be read for a
/// reason other than what it holds (its permissions, a directory at its
/// path). In both cases it is still where it was and the next
/// [`settings_get_ui_prefs`] still reports it. Also returns
/// `app_state_unavailable` when the application state was never set up, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn settings_reset_ui_prefs<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> CommandResult<UiPrefsView> {
    with_prefs_blocking(app, |_app, state| {
        // Under the lock every save holds, so the file a save has just
        // written is not the one judged and moved, and the view returned is
        // of the state this call left.
        let _prefs_guard = state.lock_prefs();

        reset_unreadable_ui_prefs(state.data_dir())?;
        Ok(load_ui_prefs_view(state.data_dir()))
    })
    .await
}

/// Remembers the entity, and the accounts chosen for an entry kind, that the
/// quick-add window last posted with.
///
/// Works in every vault state. The values go to the plaintext preferences
/// file; they are identifiers, with no amount and no name among them.
///
/// # Errors
///
/// Returns the [preferences save errors](self#preferences-save-errors),
/// `app_state_unavailable` when the application state was never set up, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn settings_remember_quick_add<R: Runtime>(
    app: tauri::AppHandle<R>,
    entity_id: EntityId,
    kind: SimpleEntryKind,
    accounts: LastRoleAccounts,
) -> CommandResult<()> {
    with_prefs_blocking(app, move |_app, state| {
        // Core's function is a load-change-save, so it runs under the lock
        // that keeps a language change from being written over.
        let _prefs_guard = state.lock_prefs();

        remember_quick_add(state.data_dir(), entity_id, kind, accounts)?;
        Ok(())
    })
    .await
}

/// Remembers the book that is open, so the next unlock selects it again.
///
/// Works in every vault state. Only the book's identifier goes to the
/// plaintext preferences file, never its name. The id is not checked against
/// the vault; a book that no longer exists is ignored when it is read back.
///
/// # Errors
///
/// Returns the [preferences save errors](self#preferences-save-errors),
/// `app_state_unavailable` when the application state was never set up, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn settings_remember_last_entity<R: Runtime>(
    app: tauri::AppHandle<R>,
    entity_id: EntityId,
) -> CommandResult<()> {
    with_prefs_blocking(app, move |_app, state| {
        // A load-change-save, so it runs under the lock that keeps a
        // language change from being written over.
        let _prefs_guard = state.lock_prefs();

        remember_last_entity(state.data_dir(), entity_id)?;
        Ok(())
    })
    .await
}

/// Runs preferences work on the blocking pool with the shared state.
///
/// The plaintext preferences file is read and written with blocking I/O, and
/// a save ends in an fsync. A synchronous command would do that on the main
/// thread, which also runs the event loop, and an async one on a runtime
/// worker, so every preferences command goes through here.
///
/// # Errors
///
/// Returns the error `work` returns, `app_state_unavailable` when the state
/// was never set up, and `task_failed` when the blocking task panics.
async fn with_prefs_blocking<R, T, F>(app: tauri::AppHandle<R>, work: F) -> CommandResult<T>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce(&tauri::AppHandle<R>, &AppState) -> CommandResult<T> + Send + 'static,
{
    run_blocking(move || {
        // `Manager::state` panics when the state is not managed, which is the
        // case after a failed start (`crate::startup`) while the hidden
        // webview is still running.
        let Some(state) = app.try_state::<AppState>() else {
            return Err(CommandError::desktop(
                DesktopError::AppStateUnavailable,
                "application state is not set up",
            ));
        };
        work(&app, &state)
    })
    .await
}

/// The preferences commands invoked through the mock IPC, the way the
/// webview invokes them: `settings_remember_quick_add` read back through
/// `settings_get_ui_prefs`, the `unreadable` flag of that answer, and
/// `settings_reset_ui_prefs`.
///
/// Not built on Windows, where the mock runtime keeps a test executable from
/// starting; `commands::support::ipc_test_support` says why.
#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use crate::commands::settings::{
        settings_get_ui_prefs, settings_remember_last_entity, settings_remember_quick_add,
        settings_reset_ui_prefs, with_prefs_blocking,
    };
    use crate::commands::support::ipc_test_support::MockApp;

    /// An entity id as the webview sends it.
    const ENTITY: &str = "11111111-1111-4111-8111-111111111111";
    /// Another entity id.
    const OTHER_ENTITY: &str = "33333333-3333-4333-8333-333333333333";
    /// An account id as the webview sends it.
    const CATEGORY: &str = "22222222-2222-4222-8222-222222222222";
    /// Another account id.
    const WALLET: &str = "44444444-4444-4444-8444-444444444444";
    /// A preferences file that does not decode: an object cut off part way.
    const CUT_OFF: &[u8] = b"{\"locale\": \"el\", \"last_entity_id\": ";

    /// Starts the mock app with the preferences commands registered. The
    /// preferences need nothing in the vault.
    fn mock_app(label: &str) -> MockApp {
        let (app, ()) = MockApp::start(
            label,
            tauri::generate_handler![
                settings_remember_quick_add,
                settings_remember_last_entity,
                settings_get_ui_prefs,
                settings_reset_ui_prefs
            ],
            |_conn| (),
        );
        app
    }

    /// The accounts object `rememberQuickAdd` in `web/src/lib/api.ts` sends:
    /// every part named, `null` for a part the entry kind does not use.
    fn roles(category: Option<&str>, wallet: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "category_account_id": category,
            "wallet_account_id": wallet,
            "payable_account_id": null,
            "from_account_id": null,
            "to_account_id": null,
        })
    }

    /// Invokes the command with the payload the frontend sends.
    fn remember(app: &MockApp, entity: &str, kind: &str, accounts: &serde_json::Value) {
        let answer = app
            .invoke(
                "settings_remember_quick_add",
                serde_json::json!({ "entityId": entity, "kind": kind, "accounts": accounts }),
            )
            .unwrap();

        assert_eq!(answer, serde_json::Value::Null);
    }

    #[test]
    fn the_ipc_call_stores_the_entity_and_the_accounts_under_entity_and_kind() {
        let app = mock_app("remember-quick-add");
        let accounts = roles(Some(CATEGORY), Some(WALLET));

        remember(&app, ENTITY, "expense", &accounts);

        let prefs = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();
        assert_eq!(prefs["last_entity_id"], ENTITY);
        assert_eq!(
            prefs["last_accounts_by_entity_kind"],
            serde_json::json!({ format!("{ENTITY}:expense"): accounts })
        );
        // The language is not part of the call and keeps its value.
        assert_eq!(prefs["locale"], "en");
    }

    #[test]
    fn a_later_call_replaces_its_own_key_and_keeps_the_others() {
        let app = mock_app("remember-quick-add-twice");
        let first = roles(Some(CATEGORY), Some(WALLET));
        let income = roles(Some(WALLET), None);
        let replaced = roles(Some(CATEGORY), None);

        remember(&app, ENTITY, "expense", &first);
        remember(&app, ENTITY, "income", &income);
        remember(&app, OTHER_ENTITY, "expense", &first);
        remember(&app, ENTITY, "expense", &replaced);

        let prefs = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();
        assert_eq!(prefs["last_entity_id"], ENTITY);
        assert_eq!(
            prefs["last_accounts_by_entity_kind"],
            serde_json::json!({
                format!("{ENTITY}:expense"): replaced,
                format!("{ENTITY}:income"): income,
                format!("{OTHER_ENTITY}:expense"): first,
            })
        );
    }

    #[test]
    fn the_last_book_is_remembered_through_the_ipc_call() {
        let app = mock_app("remember-last-entity");

        let answer = app
            .invoke(
                "settings_remember_last_entity",
                serde_json::json!({ "entityId": OTHER_ENTITY }),
            )
            .unwrap();

        assert_eq!(answer, serde_json::Value::Null);
        let prefs = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();
        assert_eq!(prefs["last_entity_id"], OTHER_ENTITY);
    }

    /// After a failed start the hidden webview still runs and calls the
    /// preferences commands, with no state for them to read.
    #[test]
    fn a_preferences_command_with_no_application_state_says_so_and_does_no_work() {
        let app = tauri::test::mock_app();
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let work_ran = std::sync::Arc::clone(&ran);

        let refused = tauri::async_runtime::block_on(with_prefs_blocking(
            app.handle().clone(),
            move |_app, _state| {
                work_ran.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        ))
        .unwrap_err();

        assert_eq!(refused.code, "app_state_unavailable");
        assert_ne!(refused.code, "task_failed", "no task failed");
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Core refuses to save over a preferences file it could not decode. The
    /// command answers with the code of that refusal, and the file stays as
    /// it was.
    #[test]
    fn a_preferences_file_that_does_not_decode_is_reported_and_left_as_it_is() {
        let app = mock_app("remember-quick-add-corrupt");
        let corrupt = CUT_OFF;
        let path = app.write_file("ui-prefs.json", corrupt);

        let refused = app
            .invoke(
                "settings_remember_quick_add",
                serde_json::json!({
                    "entityId": ENTITY,
                    "kind": "expense",
                    "accounts": roles(Some(CATEGORY), Some(WALLET)),
                }),
            )
            .unwrap_err();

        assert_eq!(refused["code"], "prefs_unreadable", "{refused}");
        assert_eq!(
            refused["params"],
            serde_json::json!({ "operation": "replace a preferences file that does not decode" })
        );
        assert_eq!(std::fs::read(path).unwrap(), corrupt);

        // Reading still works, on the defaults, and says what it found.
        let prefs = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();
        assert_eq!(prefs["locale"], "en");
        assert_eq!(prefs["last_entity_id"], serde_json::Value::Null);
        assert_eq!(prefs["unreadable"], true);
    }

    #[test]
    fn the_preferences_say_whether_the_file_could_be_read() {
        let app = mock_app("prefs-unreadable-flag");
        let read = || {
            app.invoke("settings_get_ui_prefs", serde_json::json!({}))
                .unwrap()
        };

        assert_eq!(read()["unreadable"], false, "no file");

        remember(&app, ENTITY, "expense", &roles(Some(CATEGORY), None));
        assert_eq!(read()["unreadable"], false, "a file this build wrote");

        app.write_file("ui-prefs.json", CUT_OFF);
        assert_eq!(read()["unreadable"], true, "a file that does not decode");
    }

    #[test]
    fn a_reset_moves_the_unreadable_file_aside_and_the_next_save_goes_through() {
        let app = mock_app("prefs-reset");
        app.write_file("ui-prefs.json", CUT_OFF);
        let path = app.path("ui-prefs.json");
        let aside = app.path("ui-prefs.damaged.json");

        let prefs = app
            .invoke("settings_reset_ui_prefs", serde_json::json!({}))
            .unwrap();

        assert_eq!(
            prefs,
            serde_json::json!({
                "locale": "en",
                "last_entity_id": null,
                "last_accounts_by_entity_kind": {},
                "unreadable": false,
            })
        );
        assert!(!path.exists(), "the preferences file is still there");
        assert_eq!(std::fs::read(&aside).unwrap(), CUT_OFF);

        remember(
            &app,
            ENTITY,
            "expense",
            &roles(Some(CATEGORY), Some(WALLET)),
        );
        let prefs = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();
        assert_eq!(prefs["last_entity_id"], ENTITY);
        assert_eq!(prefs["unreadable"], false);
        assert_eq!(std::fs::read(&aside).unwrap(), CUT_OFF);
    }

    #[test]
    fn a_reset_leaves_readable_preferences_as_they_are() {
        let app = mock_app("prefs-reset-good");
        remember(
            &app,
            ENTITY,
            "expense",
            &roles(Some(CATEGORY), Some(WALLET)),
        );
        let before = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();

        let after = app
            .invoke("settings_reset_ui_prefs", serde_json::json!({}))
            .unwrap();

        assert_eq!(after, before);
        assert_eq!(after["last_entity_id"], ENTITY);
        assert!(app.path("ui-prefs.json").exists());
        assert!(!app.path("ui-prefs.damaged.json").exists());
    }

    #[test]
    fn a_reset_with_no_preferences_file_succeeds_and_creates_nothing() {
        let app = mock_app("prefs-reset-missing");

        let prefs = app
            .invoke("settings_reset_ui_prefs", serde_json::json!({}))
            .unwrap();

        assert_eq!(prefs["unreadable"], false);
        assert!(!app.path("ui-prefs.json").exists());
        assert!(!app.path("ui-prefs.damaged.json").exists());
    }

    #[test]
    fn a_reset_that_cannot_move_the_file_answers_io_and_the_file_stays() {
        let app = mock_app("prefs-reset-blocked");
        app.write_file("ui-prefs.json", CUT_OFF);
        let path = app.path("ui-prefs.json");
        // A directory that is not empty cannot be replaced by a rename.
        let aside = app.path("ui-prefs.damaged.json");
        std::fs::create_dir(&aside).unwrap();
        std::fs::write(aside.join("kept"), b"x").unwrap();

        let refused = app
            .invoke("settings_reset_ui_prefs", serde_json::json!({}))
            .unwrap_err();

        assert_eq!(refused["code"], "io", "{refused}");
        assert_eq!(
            refused["params"]["operation"],
            "move the damaged preferences file aside"
        );
        assert_eq!(std::fs::read(&path).unwrap(), CUT_OFF);
        let prefs = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();
        assert_eq!(prefs["unreadable"], true);
    }

    /// A directory at the path cannot be read, but it is not a damaged file,
    /// so it is reported and left where it is.
    #[test]
    fn a_reset_does_not_move_what_is_unreadable_for_another_reason_than_its_contents() {
        let app = mock_app("prefs-reset-directory");
        std::fs::create_dir(app.path("ui-prefs.json")).unwrap();

        let refused = app
            .invoke("settings_reset_ui_prefs", serde_json::json!({}))
            .unwrap_err();

        assert_eq!(refused["code"], "io", "{refused}");
        assert_eq!(
            refused["params"]["operation"],
            "read the preferences file before moving it aside"
        );
        assert!(app.path("ui-prefs.json").is_dir());
        assert!(!app.path("ui-prefs.damaged.json").exists());
    }

    #[test]
    fn an_entry_kind_the_ledger_does_not_have_is_refused_before_anything_is_stored() {
        let app = mock_app("remember-quick-add-kind");

        let refused = app.invoke(
            "settings_remember_quick_add",
            serde_json::json!({
                "entityId": ENTITY,
                "kind": "refund",
                "accounts": roles(None, None),
            }),
        );

        assert!(refused.is_err(), "{refused:?}");
        let prefs = app
            .invoke("settings_get_ui_prefs", serde_json::json!({}))
            .unwrap();
        assert_eq!(prefs["last_entity_id"], serde_json::Value::Null);
    }
}
