//! Reporting a start that cannot go on.
//!
//! The vault header and the data directory are user data, and either can be
//! damaged or unreadable when the app starts. That must not be a panic: on a
//! Windows release build a panic closes the app with nothing on screen, on
//! every launch. Tauri itself panics on an error returned from the `setup`
//! hook, so a failure is not returned from there either. It ends here: the
//! user gets a native message that names the problem and the data directory,
//! and the app exits with a failure code when the message is dismissed.
//!
//! The message is shown through the dialog plugin's callback API. Its
//! blocking calls must not be used on the main thread, which is where the
//! setup hook runs.

use crate::state::AppState;
use oikonomia_core::error::Error as CoreError;
use oikonomia_core::prefs::{Locale, load_ui_prefs};
use oikonomia_core::vault::default_data_dir;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// Process exit code after a failed start.
const STARTUP_FAILURE_EXIT_CODE: i32 = 1;

/// Why the app could not start.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StartupError {
    /// The system named no directory for the app's data.
    #[error("cannot find the application data directory")]
    NoDataDir(#[source] CoreError),
    /// The data directory or the vault header in it could not be used.
    #[error("cannot open the vault in {}", .data_dir.display())]
    Vault {
        /// The data directory that was tried.
        data_dir: PathBuf,
        /// What core reported.
        #[source]
        source: CoreError,
    },
    /// The idle auto-lock thread could not be started.
    #[error("cannot start the auto-lock thread")]
    Watchdog(#[source] std::io::Error),
    /// The tray or a plugin could not be set up.
    #[error("cannot set up the tray or a plugin")]
    Shell(#[source] tauri::Error),
}

impl StartupError {
    /// Returns the data directory the failure is about, when there is one.
    fn data_dir(&self) -> Option<&Path> {
        match self {
            Self::Vault { data_dir, .. } => Some(data_dir),
            Self::NoDataDir(_) | Self::Watchdog(_) | Self::Shell(_) => None,
        }
    }

    /// Returns what the user is told went wrong.
    fn problem(&self) -> StartupProblem {
        match self {
            Self::Vault { source, .. } => vault_problem(source),
            Self::NoDataDir(_) | Self::Watchdog(_) | Self::Shell(_) => StartupProblem::Other,
        }
    }
}

/// Returns what the user is told when opening the vault failed with `source`.
///
/// Opening works on the files of the data directory and never opens the
/// database, so only a corrupt vault, a header from a newer build and a file
/// failure are expected here. Every other variant is listed, without a
/// wildcard arm, so that a new core error has to be given a sentence here
/// before this compiles.
fn vault_problem(source: &CoreError) -> StartupProblem {
    match source {
        CoreError::VaultCorrupt(_) => StartupProblem::VaultDamaged,
        CoreError::VaultTooNew { .. } => StartupProblem::VaultTooNew,
        CoreError::VaultUninitialized
        | CoreError::VaultLocked
        | CoreError::InvalidPassword
        | CoreError::UnbalancedEntry { .. }
        | CoreError::TooFewLines
        | CoreError::InvalidLineAmounts
        | CoreError::AccountWrongEntity
        | CoreError::MoneyOverflow
        | CoreError::NegativeMoney
        | CoreError::Validation(_)
        | CoreError::Database { .. }
        | CoreError::Io { .. }
        | CoreError::Serialization { .. }
        | CoreError::Crypto { .. }
        | CoreError::BackupInvalid(_)
        | CoreError::RestoreWouldOverwrite
        | CoreError::NotFound(_)
        | CoreError::Analysis { .. }
        | CoreError::PrefsUnreadable { .. }
        | CoreError::Csv(_) => StartupProblem::DataFolderUnreadable,
    }
}

/// The startup problems the user gets a sentence for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupProblem {
    /// The vault header cannot be parsed, or the database has no header.
    VaultDamaged,
    /// The vault was written by a newer build than this one.
    VaultTooNew,
    /// The data directory or the header file cannot be read.
    DataFolderUnreadable,
    /// Anything else; the user is told only that the app cannot start.
    Other,
}

/// Opens the vault in the default data directory.
///
/// # Errors
///
/// Returns [`StartupError::NoDataDir`] when the system names no data
/// directory, and [`StartupError::Vault`] when the directory or the vault
/// header in it cannot be used.
pub(crate) fn open_app_state(ocr_model_dir: PathBuf) -> Result<AppState, StartupError> {
    let data_dir = default_data_dir().map_err(StartupError::NoDataDir)?;
    open_app_state_in(data_dir, ocr_model_dir)
}

/// Opens the vault in `data_dir`.
///
/// # Errors
///
/// Returns [`StartupError::Vault`], naming `data_dir`, when the directory or
/// the vault header in it cannot be used.
fn open_app_state_in(data_dir: PathBuf, ocr_model_dir: PathBuf) -> Result<AppState, StartupError> {
    AppState::open_path(data_dir.clone(), ocr_model_dir)
        .map_err(|source| StartupError::Vault { data_dir, source })
}

/// Tells the user the app cannot start, and exits once they have read it.
///
/// Returns at once: the message is on screen while the event loop runs, and
/// its callback asks the app to exit.
pub(crate) fn report_and_exit(app: &tauri::App, failure: &StartupError) {
    log::error!("startup failed: {}", log_text(failure));

    // The windows from the configuration already exist. Their UI would call
    // commands whose state was never set up, so they stay out of sight.
    for window in app.webview_windows().values() {
        if let Err(err) = window.hide() {
            log::warn!("failed to hide a window after a failed start: {err}");
        }
    }

    // The preferences live in the data directory. If that is the part that
    // failed, the read falls back to English.
    let locale = failure
        .data_dir()
        .map(|data_dir| load_ui_prefs(data_dir).locale())
        .unwrap_or_default();
    let message = failure_message(locale, failure.problem(), failure.data_dir());

    let handle = app.handle().clone();
    app.dialog()
        .message(message)
        .title("Oikonomia")
        .kind(MessageDialogKind::Error)
        .show(move |_acknowledged| handle.exit(STARTUP_FAILURE_EXIT_CODE));
}

/// Returns `failure` and its cause as the log shows them.
///
/// The failure's own text names at most the data directory, which is the
/// application's. A cause from core goes through
/// [`CoreError::log_text`], so that a release build's log gets its code and
/// operation and not its detail: the detail of a header that does not parse
/// quotes the header. A cause from the operating system or from Tauri is
/// written as it is; neither has seen the ledger.
fn log_text(failure: &StartupError) -> String {
    match failure {
        StartupError::NoDataDir(source) | StartupError::Vault { source, .. } => {
            format!("{failure}: {}", source.log_text())
        }
        StartupError::Watchdog(source) => format!("{failure}: {source}"),
        StartupError::Shell(source) => format!("{failure}: {source}"),
    }
}

/// Returns the text of the failure message: that the app cannot start, what
/// is wrong when that is known, and the data directory on a line of its own.
///
/// The cause is worded here per language. Core's error text is English and
/// may hold operating-system text, so it never goes to the user; the log gets
/// the reduced form [`log_text`] describes.
fn failure_message(locale: Locale, problem: StartupProblem, data_dir: Option<&Path>) -> String {
    let mut message = cannot_start_label(locale).to_owned();

    if let Some(detail) = problem_label(locale, problem) {
        message.push(' ');
        message.push_str(detail);
    }
    if let Some(data_dir) = data_dir {
        message.push_str("\n\n");
        message.push_str(&data_dir.display().to_string());
    }
    message
}

/// Returns the sentence that says the app cannot start.
fn cannot_start_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Oikonomia cannot start.",
        Locale::El => "Το Oikonomia δεν μπορεί να ξεκινήσει.",
        Locale::Fr => "Oikonomia ne peut pas démarrer.",
        Locale::De => "Oikonomia kann nicht starten.",
    }
}

/// Returns the sentence that says what is wrong, for the problems that have
/// one.
///
/// The damaged-vault and newer-vault sentences are the ones the web catalog
/// shows for `error.vaultCorrupt` and `error.vaultTooNew`, so the app names
/// each problem one way.
fn problem_label(locale: Locale, problem: StartupProblem) -> Option<&'static str> {
    let label = match (problem, locale) {
        (StartupProblem::VaultDamaged, Locale::En) => "The vault file is corrupt.",
        (StartupProblem::VaultDamaged, Locale::El) => "Το αρχείο θυρίδας είναι κατεστραμμένο.",
        (StartupProblem::VaultDamaged, Locale::Fr) => "Le fichier du coffre est corrompu.",
        (StartupProblem::VaultDamaged, Locale::De) => "Die Tresordatei ist beschädigt.",

        (StartupProblem::VaultTooNew, Locale::En) => {
            "This vault was saved by a newer version of Oikonomia. \
             Update the app to open it; nothing has been lost."
        }
        (StartupProblem::VaultTooNew, Locale::El) => {
            "Αυτή η θυρίδα αποθηκεύτηκε από νεότερη έκδοση του Oikonomia. \
             Ενημερώστε την εφαρμογή για να την ανοίξετε· δεν έχει χαθεί τίποτα."
        }
        (StartupProblem::VaultTooNew, Locale::Fr) => {
            "Ce coffre a été enregistré par une version plus récente d’Oikonomia. \
             Mettez à jour l’application pour l’ouvrir\u{202f}; rien n’a été perdu."
        }
        (StartupProblem::VaultTooNew, Locale::De) => {
            "Dieser Tresor wurde mit einer neueren Version von Oikonomia gespeichert. \
             Aktualisieren Sie die App, um ihn zu öffnen; es ist nichts verloren gegangen."
        }

        (StartupProblem::DataFolderUnreadable, Locale::En) => "Its data folder cannot be read.",
        (StartupProblem::DataFolderUnreadable, Locale::El) => {
            "Δεν είναι δυνατή η ανάγνωση του φακέλου δεδομένων του."
        }
        (StartupProblem::DataFolderUnreadable, Locale::Fr) => {
            "Son dossier de données est illisible."
        }
        (StartupProblem::DataFolderUnreadable, Locale::De) => {
            "Der Datenordner lässt sich nicht lesen."
        }

        (StartupProblem::Other, _) => return None,
    };
    Some(label)
}

#[cfg(test)]
mod tests {
    use super::{
        StartupError, StartupProblem, failure_message, log_text, open_app_state_in, problem_label,
    };
    use oikonomia_core::error::{Error as CoreError, VaultCorruption};
    use oikonomia_core::prefs::Locale;
    use oikonomia_core::vault::vault_header_path;
    use std::path::{Path, PathBuf};

    /// Creates a fresh directory for one test.
    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oiko-startup-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since_epoch| since_epoch.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    #[test]
    fn a_damaged_vault_header_is_an_error_that_names_the_data_directory() {
        let dir = temp_dir("damaged");
        std::fs::write(vault_header_path(&dir), b"{ not a header").expect("header");

        let Err(failure) = open_app_state_in(dir.clone(), dir.clone()) else {
            unreachable!("a header that is not JSON cannot open");
        };

        assert_eq!(failure.problem(), StartupProblem::VaultDamaged);
        assert_eq!(failure.data_dir(), Some(dir.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_data_directory_that_cannot_be_created_is_reported_as_unreadable() {
        let dir = temp_dir("unreadable");
        // A file where the data directory should be.
        let blocked = dir.join("data");
        std::fs::write(&blocked, b"in the way").expect("file");

        let Err(failure) = open_app_state_in(blocked.clone(), dir.clone()) else {
            unreachable!("a file cannot serve as the data directory");
        };

        assert_eq!(failure.problem(), StartupProblem::DataFolderUnreadable);
        assert_eq!(failure.data_dir(), Some(blocked.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Text that stands for something the user wrote, placed where a core
    /// error carries lower-level detail.
    const SENTINEL: &str = "Acme-Payroll-7731";

    /// No test of this crate enables core's log detail, so this is the line
    /// a release build writes.
    #[test]
    fn the_log_line_of_a_failed_start_has_the_cause_without_its_detail() {
        let unreadable_header = StartupError::Vault {
            data_dir: PathBuf::from("/data/oikonomia"),
            source: CoreError::VaultCorrupt(VaultCorruption::HeaderUnreadable {
                detail: format!("invalid type: string \"{SENTINEL}\""),
            }),
        };
        let unreadable_directory = StartupError::Vault {
            data_dir: PathBuf::from("/data/oikonomia"),
            source: CoreError::Io {
                operation: "create private directory",
                detail: format!("/Users/someone/{SENTINEL}: denied"),
            },
        };
        let no_directory = StartupError::NoDataDir(CoreError::Io {
            operation: "resolve application data directory",
            detail: SENTINEL.to_owned(),
        });

        assert_eq!(
            log_text(&unreadable_header),
            "cannot open the vault in /data/oikonomia: \
             vault_corrupt: vault header does not parse"
        );
        assert_eq!(
            log_text(&unreadable_directory),
            "cannot open the vault in /data/oikonomia: io: create private directory"
        );
        assert_eq!(
            log_text(&no_directory),
            "cannot find the application data directory: \
             io: resolve application data directory"
        );
    }

    #[test]
    fn the_log_line_of_a_failed_start_has_an_operating_system_cause_in_full() {
        let failure = StartupError::Watchdog(std::io::Error::other("no threads left"));

        assert_eq!(
            log_text(&failure),
            "cannot start the auto-lock thread: no threads left"
        );
    }

    #[test]
    fn failures_outside_the_data_directory_get_the_general_message() {
        let failure = StartupError::Watchdog(std::io::Error::other("no threads left"));

        assert_eq!(failure.problem(), StartupProblem::Other);
        assert_eq!(failure.data_dir(), None);
        assert_eq!(
            failure_message(Locale::En, failure.problem(), failure.data_dir()),
            "Oikonomia cannot start."
        );
    }

    #[test]
    fn the_message_names_the_problem_and_the_directory_in_every_language() {
        let data_dir = Path::new("/home/someone/.local/share/oikonomia");

        for locale in Locale::ALL.iter().copied() {
            for problem in [
                StartupProblem::VaultDamaged,
                StartupProblem::DataFolderUnreadable,
            ] {
                let detail = problem_label(locale, problem).expect("worded");
                let message = failure_message(locale, problem, Some(data_dir));

                assert!(message.contains(detail), "{locale:?} {problem:?}");
                assert!(
                    message.ends_with("\n\n/home/someone/.local/share/oikonomia"),
                    "{locale:?} {problem:?}: {message}"
                );
            }
        }
        assert_eq!(
            failure_message(Locale::En, StartupProblem::VaultDamaged, Some(data_dir)),
            "Oikonomia cannot start. The vault file is corrupt.\n\n\
             /home/someone/.local/share/oikonomia"
        );
    }

    #[test]
    fn the_vault_problem_wording_is_the_web_catalogs() {
        let catalogs = [
            (
                Locale::En,
                include_str!("../../../../web/src/locales/en.json"),
            ),
            (
                Locale::El,
                include_str!("../../../../web/src/locales/el.json"),
            ),
            (
                Locale::Fr,
                include_str!("../../../../web/src/locales/fr.json"),
            ),
            (
                Locale::De,
                include_str!("../../../../web/src/locales/de.json"),
            ),
        ];

        for (locale, catalog) in catalogs {
            for problem in [StartupProblem::VaultDamaged, StartupProblem::VaultTooNew] {
                let label = problem_label(locale, problem).expect("worded");
                assert!(catalog.contains(label), "{locale:?}: {label}");
            }
        }
    }
}
