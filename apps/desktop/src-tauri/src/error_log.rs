//! The local error log of a release build.
//!
//! A release build that kept no log left nothing to go on after a failed
//! start or a failed update install on someone else's machine. It now keeps a
//! small one, and the app's promise that nothing leaves the machine and that
//! the ledger is only ever on disk encrypted shapes every part of it.
//!
//! # What is written
//!
//! Warnings and errors from this workspace's crates, one line each:
//! the time in UTC, the level, the module that logged, and the message.
//! Everything else is dropped by [`is_logged`]: lower levels, and every
//! record from a third-party crate, whose messages nobody here has audited.
//!
//! A message holds no ledger data. That is decided where the message is
//! formatted, not here: a log line interpolates fixed phrases, numbers, the
//! application's own paths, operating-system error text, errors of Tauri and
//! of the update crate, and core errors through
//! `oikonomia_core::Error::log_text`, which keeps the code and the
//! operation and drops the detail. Only a debug build asks core for the
//! detail (`oikonomia_core::error::enable_log_detail`), and a debug build
//! does not write this file. The module `oikonomia_core::error::log_text`
//! describes the reduced form and tests it.
//!
//! The webview cannot write to the file. This logger is not a Tauri plugin
//! and registers no command, so there is no IPC path to it.
//!
//! # Where, and how much
//!
//! [`FILE_NAME`] in the directory Tauri names for the app's logs
//! (`PathResolver::app_log_dir`, Tauri 2.12):
//!
//! | Platform | Directory |
//! |----------|-----------|
//! | macOS | `~/Library/Logs/io.ourovoros.oikonomia` |
//! | Linux | `$XDG_DATA_HOME/io.ourovoros.oikonomia/logs` |
//! | Windows | `%LOCALAPPDATA%\io.ourovoros.oikonomia\logs` |
//!
//! On Linux `$XDG_DATA_HOME` is `~/.local/share` unless the user set it.
//!
//! The file is capped at [`MAX_FILE_BYTES`]. A line that would take it past
//! the cap first renames it to [`PREVIOUS_FILE_NAME`], replacing the file of
//! that name, and starts a new one. So there are at most two files and at
//! most twice the cap on disk, and the newest lines are always kept. A
//! message is cut at [`MAX_MESSAGE_BYTES`], so one runaway message cannot
//! fill the file.
//!
//! On Unix the directory is created `0700` and each file `0600`, the modes of
//! the vault's own directory and files, and an existing directory or file is
//! set to that mode before it is used. The modes are set here directly:
//! core's helpers for them (`vault/permissions.rs`) are private to core. If
//! the mode cannot be set, nothing is logged. Windows relies on the per-user
//! ACL of `AppData`, as the vault does.
//!
//! # Failures of the log itself
//!
//! A log that cannot be opened is not a reason to refuse to start, and a line
//! that cannot be written has nowhere to be reported. Both are dropped: the
//! app runs without a log, or without that line.
//!
//! # Debug builds
//!
//! A debug build does not use this module. It registers `tauri-plugin-log`
//! at the info level, as it always has (`register_logger` in the crate root).

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Name of the log file.
pub(crate) const FILE_NAME: &str = "errors.log";

/// Name the log file is given when it reaches the cap. At most one such file
/// exists.
pub(crate) const PREVIOUS_FILE_NAME: &str = "errors.previous.log";

/// Largest size of one log file, in bytes: 1 MiB.
pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Longest message written, in bytes. The rest of a longer one is cut.
const MAX_MESSAGE_BYTES: usize = 2048;

/// The least severe level that is written.
const MAX_LEVEL: log::LevelFilter = log::LevelFilter::Warn;

/// The crates whose records are written: the first segment of a record's
/// target, which is the module path of the `log::` call unless the call
/// names another.
///
/// `oikonomia_lib` is this crate's library and `oikonomia` its binary.
const LOGGED_CRATES: [&str; 4] = [
    "oikonomia",
    "oikonomia_lib",
    "oikonomia_core",
    "oikonomia_update",
];

/// Mode of the log directory: only the owner may list, enter and write.
#[cfg(unix)]
const DIRECTORY_MODE: u32 = 0o700;

/// Mode of a log file: only the owner may read and write.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

/// Why the error log could not be installed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InstallError {
    /// The log directory or the log file could not be created, opened or
    /// restricted to its owner.
    #[error("cannot open the error log")]
    Open(#[source] io::Error),
    /// The process already has a logger.
    #[error("a logger is already installed")]
    AlreadyInstalled(#[source] log::SetLoggerError),
}

/// Opens the log file in `directory` and makes it the process's logger.
///
/// # Errors
///
/// Returns [`InstallError::Open`] when the directory or the file cannot be
/// created or made owner-only, and [`InstallError::AlreadyInstalled`] when a
/// logger was installed before this call. In both cases the process is left
/// without this log.
pub(crate) fn install(directory: &Path) -> Result<(), InstallError> {
    let error_log = ErrorLog::open(directory).map_err(InstallError::Open)?;

    log::set_boxed_logger(Box::new(error_log)).map_err(InstallError::AlreadyInstalled)?;
    // Lets `log::info!` and below return before they format anything.
    log::set_max_level(MAX_LEVEL);
    Ok(())
}

/// Returns whether a record of `level` from `target` is written to the log:
/// a warning or an error, from one of [`LOGGED_CRATES`].
fn is_logged(level: log::Level, target: &str) -> bool {
    let crate_name = target.split("::").next().unwrap_or(target);

    level <= MAX_LEVEL && LOGGED_CRATES.contains(&crate_name)
}

/// The logger of a release build: writes the records [`is_logged`] accepts
/// to a [`CappedFile`].
#[derive(Debug)]
struct ErrorLog {
    /// The file, behind a lock because any thread may log.
    file: Mutex<CappedFile>,
}

impl ErrorLog {
    /// Opens the log in `directory` with the cap of [`MAX_FILE_BYTES`].
    ///
    /// # Errors
    ///
    /// Returns the error of [`CappedFile::open`].
    fn open(directory: &Path) -> io::Result<Self> {
        let file = CappedFile::open(directory, MAX_FILE_BYTES)?;

        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl log::Log for ErrorLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        is_logged(metadata.level(), metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = render_line(
            &utc_timestamp(SystemTime::now()),
            record.level(),
            record.target(),
            &record.args().to_string(),
        );

        // A thread that panicked while holding the lock had finished or not
        // started its one write, so the file is still usable.
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);

        // The log is the only place a failed write could be reported.
        file.append(&line).ok();
    }

    /// Does nothing: every line is written to the file as it is logged.
    fn flush(&self) {}
}

/// Returns one line of the log, ending in a newline.
///
/// Characters of `message` that break a line or reorder the text around
/// them ([`breaks_or_reorders_a_line`]) become spaces, so a record is always
/// one line and a message cannot forge another record. A message longer than
/// [`MAX_MESSAGE_BYTES`] is cut at a character boundary and marked.
fn render_line(timestamp: &str, level: log::Level, target: &str, message: &str) -> String {
    let mut line = format!("{timestamp} {level} {target}: ");
    let mut message_bytes = 0;

    for character in message.chars() {
        message_bytes += character.len_utf8();
        if message_bytes > MAX_MESSAGE_BYTES {
            line.push_str(" [cut]");
            break;
        }
        line.push(if breaks_or_reorders_a_line(character) {
            ' '
        } else {
            character
        });
    }
    line.push('\n');
    line
}

/// Returns whether `character` would end a line or change the order in which
/// a viewer shows the text around it: a control character, the Unicode line
/// and paragraph separators, or a bidirectional embedding, override or
/// isolate.
fn breaks_or_reorders_a_line(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// Returns `now` in UTC as `2026-10-07T09:41:05Z`.
///
/// A clock set before 1970 gives the first second of 1970.
fn utc_timestamp(now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs());

    let (year, month, day) = civil_date(seconds / 86_400);
    let second_of_day = seconds % 86_400;
    let (hour, minute, second) = (
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60,
    );

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Returns the year, month and day of the proleptic Gregorian calendar that
/// is `days` days after 1970-01-01.
///
/// The algorithm is `civil_from_days` from Howard Hinnant's "chrono-Compatible
/// Low-Level Date Algorithms", restricted to days on or after the epoch. It
/// counts in 400-year eras that start on 1 March, so the leap day is the last
/// day of a year and no month table is needed.
fn civil_date(days: u64) -> (u64, u64, u64) {
    // Days from 0000-03-01 to 1970-01-01.
    let days = days + 719_468;

    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);

    // Months counted from March, so that January and February belong to the
    // year that started the March before.
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);

    (year, month, day)
}

/// An append-only file that is set aside before a line would take it past a
/// cap, with one earlier file kept beside it.
///
/// The file stays within the cap unless a single line is longer than the
/// cap, which [`MAX_MESSAGE_BYTES`] rules out for [`MAX_FILE_BYTES`].
#[derive(Debug)]
struct CappedFile {
    /// Path of the file being written.
    path: PathBuf,
    /// Path the file is renamed to when it reaches the cap.
    previous_path: PathBuf,
    /// The file at `path`, open for appending.
    file: File,
    /// Size of the file at `path`, in bytes.
    size: u64,
    /// The cap, in bytes.
    max_bytes: u64,
}

impl CappedFile {
    /// Creates `directory` if it is missing and opens [`FILE_NAME`] in it
    /// for appending, both owner-only on Unix.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error when the directory or the file
    /// cannot be created or opened, or, on Unix, restricted to its owner.
    fn open(directory: &Path, max_bytes: u64) -> io::Result<Self> {
        create_private_directory(directory)?;

        let path = directory.join(FILE_NAME);
        let file = open_private_file(&path)?;
        let size = file.metadata()?.len();

        Ok(Self {
            path,
            previous_path: directory.join(PREVIOUS_FILE_NAME),
            file,
            size,
            max_bytes,
        })
    }

    /// Appends `line`, first setting the file aside when the line would take
    /// it past the cap.
    ///
    /// An empty file takes a line of any length, so a cap smaller than one
    /// line cannot make every write rotate.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error when the file cannot be set
    /// aside, reopened or written. The line is then not in the log.
    fn append(&mut self, line: &str) -> io::Result<()> {
        let length = line.len() as u64;

        if self.size > 0 && self.size + length > self.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(line.as_bytes())?;
        self.size += length;
        Ok(())
    }

    /// Renames the file to the previous file's name, replacing that file,
    /// and starts an empty one.
    ///
    /// A file that is no longer at its path counts as set aside already: the
    /// user deleted it while the app ran, or an earlier call renamed it and
    /// then could not open the new one. Returning that error here would stop
    /// the log for the rest of the session.
    fn rotate(&mut self) -> io::Result<()> {
        match std::fs::rename(&self.path, &self.previous_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        self.file = open_private_file(&self.path)?;
        self.size = 0;
        Ok(())
    }
}

/// Creates `directory` and any missing parents and, on Unix, sets it to
/// `DIRECTORY_MODE` whether or not it existed.
fn create_private_directory(directory: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(DIRECTORY_MODE);
    }
    builder.create(directory)?;

    // The creation mode does not apply to a directory that was already there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(DIRECTORY_MODE))?;
    }
    Ok(())
}

/// Opens `path` for appending, creating it if it is missing, and on Unix
/// sets it to `FILE_MODE` whether or not it existed.
fn open_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(FILE_MODE);
    }
    let file = options.open(path)?;

    // The creation mode does not apply to a file that was already there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(FILE_MODE))?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::{
        CappedFile, ErrorLog, FILE_NAME, LOGGED_CRATES, MAX_FILE_BYTES, MAX_LEVEL,
        MAX_MESSAGE_BYTES, PREVIOUS_FILE_NAME, is_logged, render_line, utc_timestamp,
    };
    use log::{Level, Log};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, UNIX_EPOCH};

    /// Creates a fresh directory for one test and returns the path of a log
    /// directory inside it that does not exist yet.
    fn log_directory(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "oiko-error-log-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since_epoch| since_epoch.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("tmpdir");
        root.join("logs")
    }

    /// Removes the directory `log_directory` created.
    fn remove(directory: &Path) {
        let root = directory.parent().expect("the log directory has a parent");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Returns the size of `path` in bytes.
    fn size_of(path: &Path) -> u64 {
        std::fs::metadata(path).expect("the file exists").len()
    }

    /// Sends one record through `error_log` the way the `log` macros do.
    fn log_record(error_log: &ErrorLog, level: Level, target: &str, message: &str) {
        error_log.log(
            &log::Record::builder()
                .level(level)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    #[test]
    fn only_warnings_and_errors_of_this_workspace_are_logged() {
        for target in [
            "oikonomia_lib",
            "oikonomia_lib::update_exec",
            "oikonomia_core::vault::store",
            "oikonomia_update::client",
            "oikonomia",
        ] {
            assert!(is_logged(Level::Error, target), "{target}");
            assert!(is_logged(Level::Warn, target), "{target}");
            assert!(!is_logged(Level::Info, target), "{target}");
            assert!(!is_logged(Level::Debug, target), "{target}");
            assert!(!is_logged(Level::Trace, target), "{target}");
        }
    }

    #[test]
    fn no_record_of_another_crate_is_logged_at_any_level() {
        for target in [
            "tauri",
            "tauri::manager",
            "ureq::unit",
            "rusqlite",
            "tao::platform_impl",
            // The target `tauri-plugin-log` gives a record from the webview.
            "webview",
            "webview::http://localhost/src/main.tsx",
            // A crate whose name only starts like one of ours.
            "oikonomia_core_extras::module",
            "oikonomia_lib_other",
            "",
        ] {
            for level in [Level::Error, Level::Warn, Level::Info] {
                assert!(!is_logged(level, target), "{level} {target}");
            }
        }
    }

    /// The names in `LOGGED_CRATES` are checked against what the compiler
    /// calls the crates, so a renamed crate does not silently stop logging.
    #[test]
    fn the_logged_crates_are_the_workspace_crates_that_log() {
        let this_crate = module_path!().split("::").next().expect("a crate name");
        let core = std::any::type_name::<oikonomia_core::Error>();
        let update = std::any::type_name::<oikonomia_update::UpdateError>();

        assert!(LOGGED_CRATES.contains(&this_crate), "{this_crate}");
        for type_name in [core, update] {
            let crate_name = type_name.split("::").next().expect("a crate name");
            assert!(LOGGED_CRATES.contains(&crate_name), "{type_name}");
        }
        assert!(LOGGED_CRATES.contains(&env!("CARGO_PKG_NAME")));
    }

    #[test]
    fn the_cap_is_one_mebibyte_and_the_level_is_warn() {
        let directory = log_directory("cap");

        let error_log = ErrorLog::open(&directory).expect("open");
        let max_bytes = error_log.file.lock().expect("lock").max_bytes;

        assert_eq!(MAX_FILE_BYTES, 1_048_576);
        assert_eq!(max_bytes, MAX_FILE_BYTES);
        assert_eq!(MAX_LEVEL, log::LevelFilter::Warn);
        remove(&directory);
    }

    #[test]
    fn a_logged_record_is_one_line_with_time_level_and_module() {
        let directory = log_directory("line");
        let error_log = ErrorLog::open(&directory).expect("open");

        log_record(
            &error_log,
            Level::Warn,
            "oikonomia_lib::update_exec",
            "macos app replace failed: denied",
        );

        let written = std::fs::read_to_string(directory.join(FILE_NAME)).expect("read");
        let (timestamp, rest) = written.split_once(' ').expect("a timestamp");
        assert_eq!(timestamp.len(), "2026-10-07T09:41:05Z".len(), "{written}");
        assert_eq!(
            rest,
            "WARN oikonomia_lib::update_exec: macos app replace failed: denied\n"
        );
        remove(&directory);
    }

    #[test]
    fn records_that_are_not_logged_leave_the_file_empty() {
        let directory = log_directory("dropped");
        let error_log = ErrorLog::open(&directory).expect("open");

        log_record(&error_log, Level::Info, "oikonomia_lib", "OCR model dir");
        log_record(&error_log, Level::Error, "tauri::manager", "third party");
        log_record(&error_log, Level::Error, "webview", "from the webview");

        assert_eq!(size_of(&directory.join(FILE_NAME)), 0);
        remove(&directory);
    }

    #[test]
    fn a_message_cannot_start_a_second_line() {
        let line = render_line(
            "2026-10-07T09:41:05Z",
            Level::Error,
            "oikonomia_lib",
            "first\n2026-10-07T09:41:06Z ERROR oikonomia_lib: forged\r\u{1b}[2J\
             \u{2028}separator\u{2029}paragraph\u{202e}reversed\u{2066}isolated",
        );

        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
        for forbidden in [
            '\r', '\u{1b}', '\u{2028}', '\u{2029}', '\u{202e}', '\u{2066}',
        ] {
            assert!(!line.contains(forbidden), "{forbidden:?}");
        }
        assert!(line.contains("separator paragraph reversed isolated"));
    }

    #[test]
    fn a_long_message_is_cut_at_a_character_boundary() {
        // Two bytes per character, and an odd limit would fall inside one.
        let message = "é".repeat(MAX_MESSAGE_BYTES);

        let line = render_line("t", Level::Warn, "oikonomia_lib", &message);

        assert!(line.ends_with(" [cut]\n"));
        assert_eq!(line.matches('é').count(), MAX_MESSAGE_BYTES / 2);
        assert!(
            !render_line("t", Level::Warn, "oikonomia_lib", "short").contains("[cut]"),
            "a short message is not marked"
        );
    }

    #[test]
    fn the_file_is_set_aside_at_the_cap_and_only_one_earlier_file_is_kept() {
        let directory = log_directory("rotation");
        let cap = 100;
        let mut file = CappedFile::open(&directory, cap).expect("open");
        let current = directory.join(FILE_NAME);
        let previous = directory.join(PREVIOUS_FILE_NAME);

        // Forty bytes a line: two fit under the cap, the third does not.
        let line = |number: u32| format!("line {number:02} {}\n", "x".repeat(31));
        for number in 0..7 {
            file.append(&line(number)).expect("append");

            assert!(size_of(&current) <= cap, "after line {number}");
            if previous.exists() {
                assert!(size_of(&previous) <= cap, "after line {number}");
            }
        }

        // Lines 0-1, 2-3 and 4-5 each filled a file; only the last of those
        // is kept, beside the file line 6 started.
        assert_eq!(
            std::fs::read_to_string(&previous).expect("previous"),
            format!("{}{}", line(4), line(5))
        );
        assert_eq!(std::fs::read_to_string(&current).expect("current"), line(6));
        let files = std::fs::read_dir(&directory).expect("list").count();
        assert_eq!(files, 2, "exactly the log and the previous log");
        remove(&directory);
    }

    #[test]
    fn a_reopened_log_counts_what_is_already_in_the_file() {
        let directory = log_directory("reopen");
        let cap = 100;
        let line = "x".repeat(59) + "\n";

        let mut first = CappedFile::open(&directory, cap).expect("open");
        first.append(&line).expect("append");
        drop(first);
        let mut second = CappedFile::open(&directory, cap).expect("reopen");
        second.append(&line).expect("append");

        // 60 + 60 is over the cap, so the second session's line rotated.
        assert_eq!(size_of(&directory.join(PREVIOUS_FILE_NAME)), 60);
        assert_eq!(size_of(&directory.join(FILE_NAME)), 60);
        remove(&directory);
    }

    #[test]
    fn a_log_deleted_while_open_starts_again_at_the_next_rotation() {
        let directory = log_directory("deleted");
        let current = directory.join(FILE_NAME);
        let line = "x".repeat(59) + "\n";
        let mut file = CappedFile::open(&directory, 100).expect("open");
        file.append(&line).expect("append");

        std::fs::remove_file(&current).expect("delete");
        // Over the cap, so this rotates, with nothing left to rename.
        file.append(&line)
            .expect("append after the file was deleted");
        file.append(&line)
            .expect("and rotation works again afterwards");

        assert_eq!(size_of(&current), 60);
        assert_eq!(size_of(&directory.join(PREVIOUS_FILE_NAME)), 60);
        remove(&directory);
    }

    #[test]
    fn a_line_longer_than_the_cap_is_written_to_an_empty_file() {
        let directory = log_directory("oversize");
        let mut file = CappedFile::open(&directory, 10).expect("open");

        file.append("a line of more than ten bytes\n")
            .expect("append");

        assert!(!directory.join(PREVIOUS_FILE_NAME).exists());
        assert_eq!(size_of(&directory.join(FILE_NAME)), 30);
        remove(&directory);
    }

    #[test]
    fn the_timestamp_is_utc_in_iso_8601() {
        let at = |seconds: u64| utc_timestamp(UNIX_EPOCH + Duration::from_secs(seconds));

        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        // A leap day in a year divisible by 400, and the day after.
        assert_eq!(at(951_782_399), "2000-02-28T23:59:59Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(951_868_800), "2000-03-01T00:00:00Z");
        // No leap day in 2100.
        assert_eq!(at(4_107_542_400), "2100-03-01T00:00:00Z");
        assert_eq!(at(1_791_366_065), "2026-10-07T09:41:05Z");
        assert_eq!(at(1_798_761_599), "2026-12-31T23:59:59Z");
        assert_eq!(
            utc_timestamp(UNIX_EPOCH - Duration::from_secs(5)),
            "1970-01-01T00:00:00Z"
        );
    }

    #[cfg(unix)]
    mod modes {
        use crate::error_log::tests::{log_directory, remove};
        use crate::error_log::{CappedFile, FILE_NAME, PREVIOUS_FILE_NAME};
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;

        /// Returns the permission bits of `path`.
        fn mode_of(path: &Path) -> u32 {
            let metadata = std::fs::metadata(path).expect("the file exists");
            metadata.permissions().mode() & 0o777
        }

        #[test]
        fn the_directory_and_both_files_are_owner_only() {
            let directory = log_directory("modes");
            let mut file = CappedFile::open(&directory, 10).expect("open");

            file.append("first line\n").expect("append");
            file.append("second line\n").expect("append");

            assert_eq!(mode_of(&directory), 0o700);
            assert_eq!(mode_of(&directory.join(FILE_NAME)), 0o600);
            assert_eq!(mode_of(&directory.join(PREVIOUS_FILE_NAME)), 0o600);
            remove(&directory);
        }

        #[test]
        fn a_directory_and_a_file_that_were_readable_by_others_are_tightened() {
            let directory = log_directory("loose");
            let path = directory.join(FILE_NAME);
            std::fs::create_dir_all(&directory).expect("directory");
            std::fs::write(&path, b"from an earlier build\n").expect("file");
            std::fs::set_permissions(&directory, Permissions::from_mode(0o755)).expect("loosen");
            std::fs::set_permissions(&path, Permissions::from_mode(0o644)).expect("loosen");

            let file = CappedFile::open(&directory, 1000).expect("open");

            assert_eq!(mode_of(&directory), 0o700);
            assert_eq!(mode_of(&path), 0o600);
            assert_eq!(file.size, 22, "what was there is kept and counted");
            remove(&directory);
        }
    }
}
