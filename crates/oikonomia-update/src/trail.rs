//! The updater trail: one line per step of a check or an install, so that a
//! failed update leaves a record of where it stopped and why.
//!
//! The webview is told only a code ([`UpdateStatus`]). Before the trail, a
//! user's report of a failed update said nothing about which request failed,
//! which host it went to, or which check refused the artifact.
//!
//! # How a step is recorded
//!
//! As a `log` record at the info level with the target [`TARGET`]. Nothing
//! else may write to that target, and the API is built so that nothing
//! else needs to: [`record`] takes an [`Event`], and the steps only this
//! crate sees go through the crate-private `record_step`. Both take typed
//! values, not text, and their `Display` writes the line: the fixed parts
//! are phrases written here at compile time, around an error code, a parsed
//! version, a number or a host. The one free-form runtime text a line can
//! hold is a failure's message and the message of each error in its source
//! chain, such as the `io::Error` under [`UpdateError::CacheIo`] or the text
//! an [`UpdateError::InvalidVersion`] did not parse. That text always goes
//! through `write_redacted`, which cuts it at its first path.
//!
//! [`TARGET`] is not a Rust module path, so no `log::` call writes to it
//! unless it names it, and it names no crate of the workspace. The desktop
//! crate's release logger writes this target, and only this target, to its
//! own capped file (`error_log` there).
//!
//! # What a line says
//!
//! | Step | Recorded by | Line |
//! |------|-------------|------|
//! | A check begins | the caller, [`Event::CheckStarted`] | `check started` |
//! | A request ends | [`perform_check`], [`install_offer`] | `fetch feed: host release-assets.githubusercontent.com, 2 redirects, ok` |
//! | The feed's signature is checked | [`perform_check`] | `verify feed signature: passed` |
//! | A check ends | the caller, [`Event::CheckFinished`] | `check finished: version 0.2.0 available` |
//! | A check stops before it runs | the caller, [`Event::CheckStopped`] | `check finished: failed: cache_dir_unavailable` |
//! | An install begins | the caller, [`Event::InstallStarted`] | `install started` |
//! | The artifact's bytes are in hand | [`install_offer`] | `artifact size: 4096 bytes, downloaded` |
//! | The artifact is checked | [`install_offer`] | `verify artifact: digest mismatch` |
//! | The artifact goes to the installer | [`install_offer`] | `install handoff: verified artifact handed to the installer` |
//! | An install ends | the caller, [`Event::InstallFinished`] | `install finished: failed: update_cache_io: ...` |
//! | An install stops before it downloads | the caller, [`Event::InstallStopped`] | `install finished: failed: update_install_not_allowed` |
//!
//! An install installs the offer of the last check that found one, so its
//! version is the one that check's line names.
//!
//! A request is recorded with the host of the last URL it requested or
//! refused, and the number of redirects followed: the v0.1.0 check failed
//! on the redirect `releases/latest` answers with. That host answered only
//! when the request ended in a status or a body: for one refused by the
//! allow-list it was never asked, and for one with no response it never
//! answered. A failure is recorded
//! as its [`UpdateError::code`], its message, and the message of each
//! error in its source chain.
//!
//! # What a line never says
//!
//! - **A URL.** A request names only its host (`ReleaseHost`): never the
//!   scheme, the port, credentials, the path, or the query, which carries the
//!   running version and platform.
//! - **A path.** Each message of a failure's source chain is cut at its
//!   first word that looks like a path (`text_before_path` says which do).
//!   The text std gives an operating-system error names no path
//!   (`Permission denied (os error 13)`, `Input/output error (os error 5)`)
//!   and is kept whole; a custom error's text may name one. The paths the
//!   app handles are absolute (its own files come from Tauri's path
//!   resolver, a file the user picked from a native dialog), and every
//!   absolute form is caught: Unix, home-relative, drive, UNC, and a
//!   `file:` URL. What is not caught is a relative path (`Documents/books`,
//!   `./books`), a bare file name, and an absolute path joined to the text
//!   before it in the same word by anything but `=`, `:` or, for a `/`
//!   path, a quote (`x,/home/alice`, `open(/home/alice)`,
//!   `open"\\server\share`). Opening quotes and brackets at the start of a
//!   word are skipped, so `('/home/alice')` is caught.
//! - **Ledger data or amounts.** This crate has none: it never opens the
//!   vault, and a check or install reads nothing the user typed.
//! - **The release notes**, which are the feed's to say and can be long.
//!
//! Nothing here opens a file or a socket. Where the record goes is the
//! logger's decision, and no logger is installed by this crate.
//!
//! [`UpdateStatus`]: crate::UpdateStatus
//! [`perform_check`]: crate::perform_check
//! [`install_offer`]: crate::install_offer

use crate::client::{CheckOutcome, InstallHandoff, InstallOutcome, MAX_REDIRECTS, Resource};
use crate::error::UpdateError;
use std::error::Error;
use std::fmt;
use url::Url;

/// The `log` target of every record of the trail.
///
/// Not a module path, and the name of no crate, so a `log::` call reaches
/// it only by naming it, and a logger that filters on crate names never
/// takes it for one of them.
pub const TARGET: &str = "updater-trail";

/// A step of a check or an install that the caller records, because the
/// caller is where it begins or ends.
#[derive(Debug)]
pub enum Event<'a> {
    /// The user asked for a check, and it is about to run.
    CheckStarted,
    /// A check ended with this outcome.
    CheckFinished(&'a CheckOutcome),
    /// A check the user asked for ended before it ran, with the code of the
    /// error the caller returned.
    CheckStopped(&'static str),
    /// The user asked for an install, and it is about to be set up.
    InstallStarted,
    /// An install ended with this outcome.
    InstallFinished(&'a InstallOutcome),
    /// An install the user asked for ended before its download, with the
    /// code of the error the caller returned.
    InstallStopped(&'static str),
}

/// Records `event` on the trail.
///
/// Does not block on anything but the logger, opens no file and formats
/// nothing unless the logger takes the record.
pub fn record(event: &Event<'_>) {
    write(event);
}

/// Records `step`, a step only this crate sees, on the trail.
pub(crate) fn record_step(step: &Step) {
    write(step);
}

/// Logs `line` under [`TARGET`]. The one `log::` call of the trail.
fn write(line: &dyn fmt::Display) {
    log::info!(target: TARGET, "{line}");
}

impl fmt::Display for Event<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CheckStarted => formatter.write_str("check started"),
            Self::CheckFinished(outcome) => {
                formatter.write_str("check finished: ")?;
                write_check_outcome(formatter, outcome)
            }
            Self::CheckStopped(code) => write!(formatter, "check finished: failed: {code}"),
            Self::InstallStarted => formatter.write_str("install started"),
            Self::InstallFinished(outcome) => {
                formatter.write_str("install finished: ")?;
                write_install_outcome(formatter, outcome)
            }
            Self::InstallStopped(code) => write!(formatter, "install finished: failed: {code}"),
        }
    }
}

/// Writes how a check ended. The notes of an offer are left out.
fn write_check_outcome(formatter: &mut fmt::Formatter<'_>, outcome: &CheckOutcome) -> fmt::Result {
    match outcome {
        CheckOutcome::UpToDate => formatter.write_str("up to date"),
        CheckOutcome::Available(offer) => {
            write!(formatter, "version {} available", offer.version())
        }
        CheckOutcome::AvailableManually { version, .. } => {
            write!(
                formatter,
                "version {version} available through the package manager"
            )
        }
        CheckOutcome::Failed(error) => write_failure(formatter, error),
    }
}

/// Writes how an install ended.
fn write_install_outcome(
    formatter: &mut fmt::Formatter<'_>,
    outcome: &InstallOutcome,
) -> fmt::Result {
    match outcome {
        InstallOutcome::Installed(InstallHandoff::Replaced) => {
            formatter.write_str("replaced the running copy")
        }
        InstallOutcome::Installed(InstallHandoff::InstallerStarted) => {
            formatter.write_str("installer started")
        }
        InstallOutcome::Cancelled => formatter.write_str("cancelled"),
        InstallOutcome::Failed(error) => write_failure(formatter, error),
    }
}

/// Writes `failed: `, the code of `error`, its message and the message of
/// each error in its source chain, each one cut at its first path.
fn write_failure(formatter: &mut fmt::Formatter<'_>, error: &UpdateError) -> fmt::Result {
    write!(formatter, "failed: {}: ", error.code())?;
    write_redacted(formatter, &error.to_string())?;

    let mut cause = error.source();
    while let Some(current) = cause {
        formatter.write_str(": ")?;
        write_redacted(formatter, &current.to_string())?;
        cause = current.source();
    }
    Ok(())
}

/// Writes `text` up to its first word that looks like a path, and `<path>`
/// in place of that word and everything after it.
///
/// Everything after is dropped, not only the word: a path can hold spaces
/// (`/home/alice/My Books/2026.csv`), and the words that follow its first
/// one cannot be told apart from the rest of the message.
fn write_redacted(formatter: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    match text_before_path(text) {
        Some(before) => {
            formatter.write_str(before)?;
            formatter.write_str("<path>")
        }
        None => formatter.write_str(text),
    }
}

/// Returns the text before the first word of `text` that looks like a path,
/// or `None` when no word does.
///
/// A word looks like a path when, after any opening quote or bracket, it
/// starts with `/`, `\` (a UNC path too), `~` or `file:`, or holds one of
/// [`PATH_JOINS`], which also catches a drive (`C:\`, `C:/`) and any URL. A
/// separator inside a word is not enough: std's text for `EIO` is
/// `Input/output error`.
fn text_before_path(text: &str) -> Option<&str> {
    let mut before = 0;

    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece
            .trim_end_matches(char::is_whitespace)
            .trim_start_matches(['"', '\'', '`', '(', '[', '<']);
        if word.starts_with(['/', '\\', '~'])
            || word.starts_with("file:")
            || PATH_JOINS.iter().any(|join| word.contains(join))
        {
            // `before` is the summed length of whole pieces, so a character
            // boundary, and `get` returns the text. Should it not, nothing
            // of the message is kept rather than all of it.
            return Some(text.get(..before).unwrap_or(""));
        }
        before += piece.len();
    }
    None
}

/// What joins a path to the word before it: `path=/home/alice`,
/// `at:/home/alice`, `C:\Users`, `open"/home/alice"`.
const PATH_JOINS: [&str; 6] = ["=/", "=\\", ":/", ":\\", "\"/", "'/"];

/// A step only this crate sees.
#[derive(Debug)]
pub(crate) enum Step {
    /// A request, with its redirects, ended.
    Fetched(FetchRecord),
    /// The artifact's bytes are in hand and about to be checked.
    ArtifactSize {
        /// How many bytes there are.
        size: ArtifactSize,
        /// Where they came from.
        source: ArtifactSource,
    },
    /// A signature, or the artifact's digest and signature, were checked.
    Verified {
        /// What was checked.
        subject: Signed,
        /// What the check found.
        verdict: Verdict,
    },
    /// The verified artifact is being handed to the caller's installer.
    HandedToInstaller,
}

impl fmt::Display for Step {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetched(fetch) => fetch.fmt(formatter),
            Self::ArtifactSize { size, source } => {
                write!(formatter, "artifact size: {size}, {source}")
            }
            Self::Verified { subject, verdict } => write!(formatter, "verify {subject}: {verdict}"),
            Self::HandedToInstaller => {
                formatter.write_str("install handoff: verified artifact handed to the installer")
            }
        }
    }
}

/// The host of a URL the updater requested, and nothing else of the URL.
///
/// Built only from a parsed [`Url`], through [`Url::host_str`], so it never
/// holds a scheme, credentials, a port, a path, a query or a fragment.
#[derive(Debug)]
pub(crate) struct ReleaseHost(String);

impl ReleaseHost {
    /// Returns the host of `url`, or `None` for a URL without one, such as
    /// a `file:` URL a redirect pointed at.
    pub(crate) fn of(url: &Url) -> Option<Self> {
        url.host_str().map(|host| Self(host.to_owned()))
    }
}

impl fmt::Display for ReleaseHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// How one request, with the redirects it followed, ended.
#[derive(Debug)]
pub(crate) struct FetchRecord {
    /// What was requested.
    resource: Resource,
    /// The host of the last URL requested or refused, when it had one.
    host: Option<ReleaseHost>,
    /// How many redirects were followed before the end.
    redirects: u8,
    /// How it ended.
    end: FetchEnd,
}

impl FetchRecord {
    /// Returns the record of a fetch of `resource` that ended in `end` at
    /// `url`, after `redirects` redirects. Only the host of `url` is kept.
    pub(crate) fn new(resource: Resource, url: &Url, redirects: u8, end: FetchEnd) -> Self {
        Self {
            resource,
            host: ReleaseHost::of(url),
            redirects,
            end,
        }
    }
}

impl fmt::Display for FetchRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let resource = match self.resource {
            Resource::Manifest => "feed",
            Resource::ManifestSignature => "feed signature",
            Resource::Artifact => "artifact",
        };
        write!(formatter, "fetch {resource}: ")?;
        match &self.host {
            Some(host) => write!(formatter, "host {host}, ")?,
            None => formatter.write_str("no host, ")?,
        }
        let plural = if self.redirects == 1 { "" } else { "s" };
        write!(
            formatter,
            "{} redirect{plural}, {}",
            self.redirects, self.end
        )
    }
}

/// How a request ended, in the words of the trail.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FetchEnd {
    /// HTTP 200, with the whole body within its cap.
    Body,
    /// HTTP 204.
    NoContent,
    /// The last hop answered with this status, which is not 200, 204 or a
    /// redirect.
    Status(u16),
    /// The URL of a hop was off the allow-list and was not requested.
    Refused,
    /// The body was larger than the cap of the resource.
    TooLarge,
    /// No usable answer arrived.
    Network(NetworkFailure),
    /// The install was cancelled during the download.
    Cancelled,
}

impl fmt::Display for FetchEnd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Body => formatter.write_str("ok"),
            Self::NoContent => formatter.write_str("no content (204)"),
            Self::Status(status) => write!(formatter, "status {status}"),
            Self::Refused => formatter.write_str("refused: host not on the allow-list"),
            Self::TooLarge => formatter.write_str("refused: body over the size limit"),
            Self::Network(failure) => failure.fmt(formatter),
            Self::Cancelled => formatter.write_str("cancelled"),
        }
    }
}

/// Why a request got no usable answer.
///
/// The transport error itself is not kept: its message starts with the URL
/// it failed on, query included. Its kind and the kind of the I/O error
/// under it are fixed phrases.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NetworkFailure {
    /// No response arrived.
    NoResponse {
        /// What the HTTP library says failed.
        transport: ureq::ErrorKind,
        /// The kind of the I/O error under it, when there is one.
        io: Option<std::io::ErrorKind>,
    },
    /// A redirect named no location, or one that does not parse.
    UnusableRedirect,
    /// One more redirect than [`MAX_REDIRECTS`] was asked for.
    RedirectLimit,
    /// The body stopped arriving before its end, with the kind of the I/O
    /// error the read failed with.
    BodyInterrupted(std::io::ErrorKind),
}

impl fmt::Display for NetworkFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoResponse {
                transport,
                io: Some(io),
            } => write!(formatter, "no response: {transport}: {io}"),
            Self::NoResponse {
                transport,
                io: None,
            } => write!(formatter, "no response: {transport}"),
            Self::UnusableRedirect => formatter.write_str("redirect without a usable location"),
            Self::RedirectLimit => write!(formatter, "more than {MAX_REDIRECTS} redirects"),
            Self::BodyInterrupted(io) => write!(formatter, "body interrupted: {io}"),
        }
    }
}

/// The length of an artifact, in bytes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArtifactSize(u64);

impl ArtifactSize {
    /// Returns the size of `bytes`.
    pub(crate) fn of(bytes: &[u8]) -> Self {
        // A `usize` fits in a `u64` on every target this crate builds for.
        Self(bytes.len() as u64)
    }
}

impl fmt::Display for ArtifactSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} bytes", self.0)
    }
}

/// Where the artifact's bytes came from.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArtifactSource {
    /// A download that just ended.
    Downloaded,
    /// A file an earlier attempt at the same release left in the cache.
    Cached,
}

impl fmt::Display for ArtifactSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Downloaded => "downloaded",
            Self::Cached => "left in the cache by an earlier attempt",
        })
    }
}

/// What a verification checked.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Signed {
    /// The detached signature over the feed.
    Feed,
    /// The artifact's digest and signature.
    Artifact,
}

impl fmt::Display for Signed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Feed => "feed signature",
            Self::Artifact => "artifact",
        })
    }
}

/// What a verification found.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Verdict {
    /// Every check passed.
    Passed,
    /// The SHA-256 of the bytes is not the one the signed feed states.
    DigestMismatch,
    /// The signature is malformed, or does not verify the bytes with the
    /// updater key.
    SignatureInvalid,
}

impl fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Passed => "passed",
            Self::DigestMismatch => "digest mismatch",
            Self::SignatureInvalid => "signature does not verify",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactSize, ArtifactSource, Event, FetchEnd, FetchRecord, NetworkFailure, ReleaseHost,
        Signed, Step, Verdict,
    };
    use crate::client::{CheckOutcome, InstallHandoff, InstallOutcome, Resource};
    use crate::error::{InstallStep, UpdateError};
    use semver::Version;
    use url::Url;

    /// Returns the line `outcome` of a check is recorded as.
    fn check_line(outcome: &CheckOutcome) -> String {
        Event::CheckFinished(outcome).to_string()
    }

    /// Returns the line `outcome` of an install is recorded as.
    fn install_line(outcome: &InstallOutcome) -> String {
        Event::InstallFinished(outcome).to_string()
    }

    #[test]
    fn a_release_host_is_the_host_and_nothing_else_of_the_url() {
        let url = Url::parse(
            "https://alice:secret@objects.githubusercontent.com:8443/github-production-release-asset/\
             Oikonomia.AppImage?version=0.1.0&os=linux&arch=x86_64#fragment",
        )
        .expect("url");

        let host = ReleaseHost::of(&url).expect("a host").to_string();

        assert_eq!(host, "objects.githubusercontent.com");
        for leaked in [
            "alice", "secret", "8443", "/", "?", "version", "linux", "#", "https",
        ] {
            assert!(!host.contains(leaked), "{leaked} in {host}");
        }
    }

    #[test]
    fn a_release_host_of_an_address_or_a_hostless_url() {
        let ipv6 = Url::parse("http://[::1]:8080/latest.json").expect("url");
        let file = Url::parse("file:///home/alice/latest.json").expect("url");
        let international = Url::parse("https://bücher.example/latest.json").expect("url");

        assert_eq!(ReleaseHost::of(&ipv6).expect("host").to_string(), "[::1]");
        assert!(ReleaseHost::of(&file).is_none());
        assert_eq!(
            ReleaseHost::of(&international).expect("host").to_string(),
            "xn--bcher-kva.example"
        );
    }

    #[test]
    fn a_fetch_names_the_resource_the_final_host_and_the_redirects() {
        let after_redirects = Url::parse(
            "https://release-assets.githubusercontent.com/path/latest.json?version=0.1.0&os=linux",
        )
        .expect("url");

        let line = Step::Fetched(FetchRecord::new(
            Resource::Manifest,
            &after_redirects,
            2,
            FetchEnd::Body,
        ))
        .to_string();
        let one = FetchRecord::new(
            Resource::ManifestSignature,
            &after_redirects,
            1,
            FetchEnd::Status(404),
        )
        .to_string();

        assert_eq!(
            line,
            "fetch feed: host release-assets.githubusercontent.com, 2 redirects, ok"
        );
        assert_eq!(
            one,
            "fetch feed signature: host release-assets.githubusercontent.com, 1 redirect, \
             status 404"
        );
    }

    #[test]
    fn every_end_of_a_fetch_reads_differently() {
        let ends = [
            FetchEnd::Body,
            FetchEnd::NoContent,
            FetchEnd::Status(503),
            FetchEnd::Refused,
            FetchEnd::TooLarge,
            FetchEnd::Network(NetworkFailure::NoResponse {
                transport: ureq::ErrorKind::ConnectionFailed,
                io: Some(std::io::ErrorKind::ConnectionRefused),
            }),
            FetchEnd::Network(NetworkFailure::NoResponse {
                transport: ureq::ErrorKind::Dns,
                io: None,
            }),
            FetchEnd::Network(NetworkFailure::UnusableRedirect),
            FetchEnd::Network(NetworkFailure::RedirectLimit),
            FetchEnd::Network(NetworkFailure::BodyInterrupted(
                std::io::ErrorKind::TimedOut,
            )),
            FetchEnd::Cancelled,
        ];

        let mut lines: Vec<String> = ends.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines[5],
            "no response: Connection Failed: connection refused"
        );
        assert_eq!(lines[8], "more than 5 redirects");
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), ends.len(), "{lines:?}");
    }

    #[test]
    fn a_check_records_how_it_ended_with_the_code_of_a_failure() {
        assert_eq!(Event::CheckStarted.to_string(), "check started");
        assert_eq!(
            check_line(&CheckOutcome::UpToDate),
            "check finished: up to date"
        );
        assert_eq!(
            check_line(&CheckOutcome::AvailableManually {
                version: Version::new(0, 2, 0),
                notes: "notes the trail leaves out".to_owned(),
            }),
            "check finished: version 0.2.0 available through the package manager"
        );
        assert_eq!(
            check_line(&CheckOutcome::Failed(UpdateError::Network)),
            "check finished: failed: update_network: \
             update server could not be reached or gave an unusable answer"
        );
        assert_eq!(
            Event::CheckStopped("cache_dir_unavailable").to_string(),
            "check finished: failed: cache_dir_unavailable"
        );
    }

    #[test]
    fn an_install_records_how_it_ended_and_the_step_it_failed_at() {
        assert_eq!(Event::InstallStarted.to_string(), "install started");
        assert_eq!(
            Event::InstallStopped("update_install_not_allowed").to_string(),
            "install finished: failed: update_install_not_allowed"
        );
        assert_eq!(
            install_line(&InstallOutcome::Installed(InstallHandoff::Replaced)),
            "install finished: replaced the running copy"
        );
        assert_eq!(
            install_line(&InstallOutcome::Installed(InstallHandoff::InstallerStarted)),
            "install finished: installer started"
        );
        assert_eq!(
            install_line(&InstallOutcome::Cancelled),
            "install finished: cancelled"
        );
        assert_eq!(
            install_line(&InstallOutcome::Failed(UpdateError::InstallFailed {
                step: InstallStep::Replace,
            })),
            "install finished: failed: update_install_failed: \
             update could not be installed: cannot move the new copy into place"
        );
    }

    #[test]
    fn the_cause_chain_of_a_failure_is_recorded_without_its_unix_paths() {
        let cause = std::io::Error::other(
            "cannot write /home/alice/My Books/2026 ledger.csv: permission denied",
        );

        let line = install_line(&InstallOutcome::Failed(UpdateError::CacheIo(cause)));

        assert_eq!(
            line,
            "install finished: failed: update_cache_io: \
             update cache could not be written: cannot write <path>"
        );
    }

    #[test]
    fn the_cause_chain_of_a_failure_is_recorded_without_its_other_path_forms() {
        for (message, kept) in [
            (r"C:\Users\Alice Smith\Documents\books.oiko is locked", ""),
            (r"denied: \\fileserver\share\books.oiko", "denied: "),
            ("open \"D:/Backups/vault 2026.oiko\" failed", "open "),
            ("no such file ~alice/books (os error 2)", "no such file "),
            ("bad path=/home/alice/books.oiko", "bad "),
            ("failed at:/home/alice/books.oiko", "failed "),
            ("cannot open file:///home/alice/books.oiko", "cannot open "),
            ("cannot open file:books.oiko", "cannot open "),
            (r"lock held, path=C:\Users\alice", "lock held, "),
            ("open('/home/alice/books.oiko')", ""),
        ] {
            let error = UpdateError::CacheIo(std::io::Error::other(message));

            let line = check_line(&CheckOutcome::Failed(error));

            assert_eq!(
                line,
                format!(
                    "check finished: failed: update_cache_io: \
                     update cache could not be written: {kept}<path>"
                )
            );
        }
    }

    #[test]
    fn operating_system_error_text_is_kept_whole() {
        // EACCES, and EIO, whose text is `Input/output error` on Linux and
        // macOS.
        for code in [13, 5] {
            let cause = std::io::Error::from_raw_os_error(code);
            let expected = cause.to_string();

            let line = check_line(&CheckOutcome::Failed(UpdateError::CacheIo(cause)));

            assert!(line.ends_with(&format!(": {expected}")), "{line}");
            assert!(!line.contains("<path>"), "{line}");
        }
        for kept in [
            "Input/output error (os error 5)",
            "read/write lock poisoned",
            "TCP/IP stack: 3:00 timeout",
        ] {
            let error = UpdateError::CacheIo(std::io::Error::other(kept));

            let line = check_line(&CheckOutcome::Failed(error));

            assert!(line.ends_with(&format!(": {kept}")), "{line}");
        }
    }

    #[test]
    fn the_artifact_steps_name_the_size_the_source_and_the_verdict() {
        let size = Step::ArtifactSize {
            size: ArtifactSize::of(&[0; 4096]),
            source: ArtifactSource::Downloaded,
        };
        let cached = Step::ArtifactSize {
            size: ArtifactSize::of(b"abc"),
            source: ArtifactSource::Cached,
        };
        let tampered = Step::Verified {
            subject: Signed::Artifact,
            verdict: Verdict::DigestMismatch,
        };
        let feed = Step::Verified {
            subject: Signed::Feed,
            verdict: Verdict::SignatureInvalid,
        };

        assert_eq!(size.to_string(), "artifact size: 4096 bytes, downloaded");
        assert_eq!(
            cached.to_string(),
            "artifact size: 3 bytes, left in the cache by an earlier attempt"
        );
        assert_eq!(tampered.to_string(), "verify artifact: digest mismatch");
        assert_eq!(
            feed.to_string(),
            "verify feed signature: signature does not verify"
        );
        assert_eq!(
            Step::HandedToInstaller.to_string(),
            "install handoff: verified artifact handed to the installer"
        );
    }
}
