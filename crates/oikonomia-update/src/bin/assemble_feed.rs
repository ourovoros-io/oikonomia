//! Release-lane command line: builds and checks what a release publishes.
//!
//! The promote workflow runs this instead of doing the same work in shell, so
//! that the rules for a release live in tested Rust (`release_set`, `feed`,
//! `verify`) and signatures and digests are checked before publication by
//! the functions the application checks them with. Each subcommand does one
//! step:
//!
//! - `assemble` writes `latest.json` from the artifacts in a directory;
//! - `verify` checks a file against a detached minisign signature;
//! - `verify-feed` checks that installed copies would accept a feed, and
//!   every artifact it names against its hash and its minisign signature;
//! - `unpublished` lists the draft assets that must not be published;
//! - `fixed-copies` writes the version-free copies the website links to;
//! - `fixed-names` prints the version-free names a release must carry;
//! - `checksums` writes the release's `SHA256SUMS` file;
//! - `check-sizes` refuses an updater artifact larger than the client will
//!   download. `--dir` scans a bundle for the files a feed can name;
//!   `--manifest` with `--dir` checks exactly the files that feed points at.
//!
//! A subcommand prints its result to standard output and nothing else, so a
//! workflow can read it line by line. A failure prints one line to standard
//! error and exits with status 1.

use oikonomia_update::{
    ArtifactSizeError, FeedArtifact, FeedRefusal, ReleaseSetError, UpdateError, WindowsBuild,
    assemble_manifest, check_artifact_file, check_feed_as_client, checksum_line,
    checksummed_assets, feed_entries, feed_platform_keys, fixed_name_copies, fixed_names,
    is_published_asset, sha256_hex, updater_artifact_kinds, verify_signature,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use thiserror::Error;

/// What the tool prints when it is called with arguments it cannot use.
const USAGE: &str = "usage:
  assemble_feed assemble --version <v> --base-url <url> --dir <artifact-dir> \
    --out <latest.json> [--notes-file <path>] [--with-windows]
  assemble_feed verify --manifest <latest.json> --sig <latest.json.sig> --pubkey <minisign-pubkey>
  assemble_feed verify-feed --manifest <latest.json> --dir <artifact-dir> \
    --pubkey <minisign-pubkey> [--with-windows]
  assemble_feed unpublished [--with-windows] <asset-name>...
  assemble_feed checksums --dir <artifact-dir> --out <SHA256SUMS> [--with-windows]
  assemble_feed fixed-copies --dir <artifact-dir> [--with-windows]
  assemble_feed fixed-names [--with-windows]
  assemble_feed check-sizes --dir <bundle-or-artifact-dir>
  assemble_feed check-sizes --manifest <latest.json> --dir <artifact-dir>";

/// The flag that includes the Windows installer in the release.
const WITH_WINDOWS: &str = "--with-windows";

/// Why a subcommand failed.
///
/// A variant's message leaves out the cause it wraps; [`describe`] appends
/// the causes when the error is printed.
#[derive(Debug, Error)]
enum CliError {
    /// The subcommand is unknown, or a flag is missing or has no value.
    #[error("{}", USAGE)]
    Usage,

    /// A file or directory could not be read, written or listed.
    #[error("cannot {action} {}", path.display())]
    Io {
        /// What was being done, as a verb: `read`, `write`, `list`.
        action: &'static str,
        /// The file or directory it was done to.
        path: PathBuf,
        /// The error the system gave.
        #[source]
        source: std::io::Error,
    },

    /// A file could not be copied to its version-free name.
    #[error("cannot copy {} to {}", from.display(), to.display())]
    Copy {
        /// The versioned file.
        from: PathBuf,
        /// The version-free name it was to be copied to.
        to: PathBuf,
        /// The error the system gave.
        #[source]
        source: std::io::Error,
    },

    /// A result line could not be written to standard output.
    #[error("cannot write to standard output")]
    Output(#[source] std::io::Error),

    /// A file name in an artifact directory is not UTF-8. It could be neither
    /// matched against the release set nor written into a checksum file, so
    /// it is refused instead of being left out.
    #[error("file name in {} is not UTF-8: {}", directory.display(), name.to_string_lossy())]
    FileNameNotUtf8 {
        /// The directory that was being listed.
        directory: PathBuf,
        /// The name as the system gave it.
        name: OsString,
    },

    /// A feed file is not the JSON the client reads.
    #[error("cannot parse {}", path.display())]
    Feed {
        /// The feed file.
        path: PathBuf,
        /// What the parser objected to.
        #[source]
        source: serde_json::Error,
    },

    /// A feed holds other platforms than the release publishes.
    #[error("feed platforms are {found:?}, this release publishes {wanted:?}")]
    FeedPlatforms {
        /// The platform keys in the feed, sorted.
        found: Vec<String>,
        /// The platform keys the release publishes, sorted.
        wanted: Vec<&'static str>,
    },

    /// A feed entry's URL does not end in a usable file name.
    #[error("{platform}: no file name in {url}")]
    NoFileName {
        /// The platform key of the entry.
        platform: String,
        /// The URL the entry gives.
        url: String,
    },

    /// An artifact does not have the SHA-256 its feed entry states.
    #[error("{platform}: {file_name} does not match the sha256 in the feed")]
    DigestMismatch {
        /// The platform key of the entry.
        platform: String,
        /// The artifact that was hashed.
        file_name: String,
    },

    /// An artifact's signature in the feed does not verify with the key.
    #[error("{platform}: the signature of {file_name} does not verify with the public key")]
    SignatureMismatch {
        /// The platform key of the entry.
        platform: String,
        /// The artifact that was checked.
        file_name: String,
    },

    /// A directory holds no file the release publishes.
    #[error("{}: no published files to checksum", directory.display())]
    NothingToChecksum {
        /// The directory that was listed.
        directory: PathBuf,
    },

    /// Installed copies would refuse the feed.
    #[error(transparent)]
    Refused(#[from] FeedRefusal),

    /// A feed entry lacks a field `verify-feed` checks the artifact against.
    #[error("{platform}: the feed entry has no {field}")]
    IncompleteEntry {
        /// The platform key of the entry.
        platform: String,
        /// The field that is missing: `signature` or `sha256`.
        field: &'static str,
    },

    /// A bundle directory holds no file a feed could name.
    #[error("{}: no updater artifact matching {suffixes}", directory.display())]
    NoUpdaterArtifact {
        /// The directory that was scanned.
        directory: PathBuf,
        /// The suffixes that were looked for, comma-separated.
        suffixes: String,
    },

    /// A file named like an updater artifact is a symbolic link.
    #[error("{}: updater artifact is a symlink", path.display())]
    SymlinkedArtifact {
        /// The link.
        path: PathBuf,
    },

    /// A feed lists no platform, so there is nothing to check.
    #[error("{}: no platforms", path.display())]
    EmptyFeed {
        /// The feed file.
        path: PathBuf,
    },

    /// One or more updater artifacts are missing, are not regular files, or
    /// are larger than the update client will download.
    #[error("{0}")]
    ArtifactSizes(SizeFailures),

    /// The draft does not hold the files the release needs.
    #[error(transparent)]
    ReleaseSet(#[from] ReleaseSetError),

    /// The update library refused a key, a signature or a feed input.
    #[error(transparent)]
    Update(#[from] UpdateError),
}

impl CliError {
    /// Returns a closure that wraps an I/O error from doing `action` to `path`.
    fn io(action: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.to_path_buf();

        move |source| Self::Io {
            action,
            path,
            source,
        }
    }
}

/// The result of a subcommand.
type CliResult<T> = Result<T, CliError>;

/// The part of `latest.json` this tool reads to find and check the files a
/// feed names. `verify-feed` and `check-sizes` both read a feed through
/// [`read_feed`], so they agree on which file an entry means.
#[derive(Debug, Deserialize)]
struct Feed {
    /// The artifact of each platform, by platform key.
    platforms: BTreeMap<String, FeedEntry>,
}

/// One platform's artifact as the feed describes it.
///
/// Only the URL is needed to find the file, which is all `check-sizes` does
/// with an entry, so the other two fields may be absent here. `verify-feed`
/// requires them: before it reads an entry it asks the client's own parser,
/// which refuses a feed without them.
#[derive(Debug, Deserialize)]
struct FeedEntry {
    /// Where the app downloads the artifact from.
    url: String,
    /// The minisign signature over the artifact.
    #[serde(default)]
    signature: Option<String>,
    /// The hex SHA-256 of the artifact.
    #[serde(default)]
    sha256: Option<String>,
}

impl FeedEntry {
    /// Returns the name of the file this entry's URL ends in.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::NoFileName`], naming `platform`, when the URL does
    /// not end in a name that stays inside a directory it is joined to.
    fn file_name(&self, platform: &str) -> CliResult<&str> {
        artifact_file_name(&self.url).ok_or_else(|| CliError::NoFileName {
            platform: platform.to_owned(),
            url: self.url.clone(),
        })
    }
}

/// Returns the value of the entry field `field`.
///
/// # Errors
///
/// Returns [`CliError::IncompleteEntry`], naming `platform`, when the entry
/// does not have the field.
fn required_field<'a>(
    value: Option<&'a String>,
    field: &'static str,
    platform: &str,
) -> CliResult<&'a str> {
    value
        .map(String::as_str)
        .ok_or_else(|| CliError::IncompleteEntry {
            platform: platform.to_owned(),
            field,
        })
}

/// Reads the feed at `manifest` and returns its bytes with what this tool
/// reads of them.
///
/// # Errors
///
/// Returns [`CliError::Io`] when the file cannot be read and
/// [`CliError::Feed`] when it is not JSON with a `platforms` table whose
/// entries each have a `url`.
fn read_feed(manifest: &Path) -> CliResult<(Vec<u8>, Feed)> {
    let body = std::fs::read(manifest).map_err(CliError::io("read", manifest))?;
    let feed = serde_json::from_slice(&body).map_err(|source| CliError::Feed {
        path: manifest.to_path_buf(),
        source,
    })?;

    Ok((body, feed))
}

/// What one entry of a bundle directory is to the size check.
enum BundleEntry {
    /// A directory, to be scanned in turn.
    Directory,
    /// An updater artifact for the platform with this feed key.
    Artifact(&'static str),
    /// Anything else: a manual download, a signature, a link to either.
    Other,
}

/// One artifact the size check refused.
#[derive(Debug)]
struct SizeFailure {
    /// The feed key of the platform the artifact is for.
    platform: String,
    /// Why the artifact was refused.
    error: ArtifactSizeError,
}

/// Every artifact one run of the size check refused, printed one per line.
#[derive(Debug)]
struct SizeFailures(Vec<SizeFailure>);

impl std::fmt::Display for SizeFailures {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lines: Vec<String> = self
            .0
            .iter()
            .map(|failure| format!("{}: {}", failure.platform, failure.error))
            .collect();

        formatter.write_str(&lines.join("\n"))
    }
}

/// Runs the subcommand the arguments name and reports a failure on standard
/// error with exit status 1.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // `writeln!`, not `eprintln!`, which the workspace's `print_stderr`
            // lint forbids; a failed write to stderr has nowhere to be reported.
            let _ = writeln!(std::io::stderr(), "{}", describe(&error));
            ExitCode::FAILURE
        }
    }
}

/// Runs the subcommand named by the first argument.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for no subcommand or an unknown one, and
/// otherwise whatever the subcommand returns.
fn run(args: &[String]) -> CliResult<()> {
    let Some((command, rest)) = args.split_first() else {
        return Err(CliError::Usage);
    };

    match command.as_str() {
        "assemble" => run_assemble(rest),
        "verify" => run_verify(rest),
        "verify-feed" => run_verify_feed(rest),
        "unpublished" => run_unpublished(rest),
        "checksums" => run_checksums(rest),
        "fixed-copies" => run_fixed_copies(rest),
        "fixed-names" => run_fixed_names(rest),
        "check-sizes" => run_check_sizes(rest),
        _ => Err(CliError::Usage),
    }
}

/// Returns `error` and the causes behind it as one line, outermost first.
fn describe(error: &CliError) -> String {
    let mut line = error.to_string();
    let mut cause = std::error::Error::source(error);

    while let Some(error) = cause {
        line.push_str(": ");
        line.push_str(&error.to_string());
        cause = error.source();
    }

    line
}

/// Writes `latest.json` for the artifacts in `--dir` to `--out`.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] for a file
/// that cannot be read or written, [`CliError::FileNameNotUtf8`] for such a
/// name in the directory, [`CliError::ReleaseSet`] when a platform has no
/// artifact or several, [`CliError::Update`] when the version or an artifact
/// is not fit for a feed, and [`CliError::Output`] when the result line
/// cannot be printed.
fn run_assemble(args: &[String]) -> CliResult<()> {
    let version = required_flag(args, "--version")?;
    let base_url = required_flag(args, "--base-url")?;
    let directory = Path::new(required_flag(args, "--dir")?);
    let out = Path::new(required_flag(args, "--out")?);
    let notes = match optional_flag(args, "--notes-file")? {
        Some(path) => {
            let path = Path::new(path);
            std::fs::read_to_string(path).map_err(CliError::io("read", path))?
        }
        None => String::new(),
    };

    let names = file_names_in(directory)?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let artifacts = feed_entries(&names, windows_build(args))?
        .into_iter()
        .map(|(platform, file_name)| feed_artifact(directory, platform, file_name))
        .collect::<CliResult<Vec<_>>>()?;

    let manifest = assemble_manifest(version, notes.trim(), base_url, &artifacts)?;
    std::fs::write(out, manifest).map_err(CliError::io("write", out))?;
    print_line(format_args!("wrote {}", out.display()))
}

/// Reads the artifact `file_name` and the `.sig` beside it in `directory`
/// into the feed entry for `platform`.
///
/// # Errors
///
/// Returns [`CliError::Io`] when either file cannot be read.
fn feed_artifact(directory: &Path, platform: &str, file_name: &str) -> CliResult<FeedArtifact> {
    let artifact_path = directory.join(file_name);
    let bytes = std::fs::read(&artifact_path).map_err(CliError::io("read", &artifact_path))?;

    let signature_path = directory.join(format!("{file_name}.sig"));
    let signature =
        std::fs::read_to_string(&signature_path).map_err(CliError::io("read", &signature_path))?;

    Ok(FeedArtifact {
        platform: platform.to_owned(),
        file_name: file_name.to_owned(),
        signature: signature.trim().to_owned(),
        sha256_hex: sha256_hex(&bytes),
    })
}

/// Checks the file at `--manifest` against the detached signature at `--sig`
/// with the key `--pubkey`.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] for a file
/// that cannot be read, [`CliError::Update`] when the key is not a minisign
/// key or the signature does not verify, and [`CliError::Output`] when the
/// result line cannot be printed.
fn run_verify(args: &[String]) -> CliResult<()> {
    let manifest = Path::new(required_flag(args, "--manifest")?);
    let signature_path = Path::new(required_flag(args, "--sig")?);
    let public_key = required_flag(args, "--pubkey")?;

    let body = std::fs::read(manifest).map_err(CliError::io("read", manifest))?;
    let signature =
        std::fs::read_to_string(signature_path).map_err(CliError::io("read", signature_path))?;

    verify_signature(public_key, &body, &signature)?;
    print_line("manifest signature ok")
}

/// Checks the artifacts a feed names against the files in `--dir`.
///
/// The feed must hold exactly the platforms this release publishes. It must
/// pass the checks an installed copy makes before it offers an update: a
/// version it can read, and for each platform an artifact URL on the app's
/// allow-list. A feed assembled with a wrong `--base-url` fails here and not
/// in every installed copy. And every file the feed names must hash to its
/// `sha256` and carry a minisign signature that verifies with `--pubkey`,
/// the key baked into the app. All of it is checked with the functions the
/// app uses. Run before anything is public, so a release the app would
/// refuse never ships.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] or
/// [`CliError::Feed`] for a feed that cannot be read or parsed,
/// [`CliError::FeedPlatforms`] when its platforms are not the published
/// ones, [`CliError::Refused`] when installed copies would refuse it,
/// whatever [`verify_feed_entry`] returns for the first entry that fails,
/// and [`CliError::Output`] when a result line cannot be printed.
fn run_verify_feed(args: &[String]) -> CliResult<()> {
    let manifest = Path::new(required_flag(args, "--manifest")?);
    let directory = Path::new(required_flag(args, "--dir")?);
    let public_key = required_flag(args, "--pubkey")?;

    let (body, feed) = read_feed(manifest)?;

    let wanted = {
        let mut wanted = feed_platform_keys(windows_build(args));
        wanted.sort_unstable();
        wanted
    };
    // A `BTreeMap` yields its keys sorted, as `wanted` is.
    if !feed.platforms.keys().eq(wanted.iter()) {
        return Err(CliError::FeedPlatforms {
            found: feed.platforms.into_keys().collect(),
            wanted,
        });
    }

    check_feed_as_client(&body, &wanted)?;

    for (platform, entry) in &feed.platforms {
        let file_name = verify_feed_entry(platform, entry, directory, public_key)?;
        print_line(format_args!(
            "{platform}: {file_name} hash and signature ok"
        ))?;
    }

    Ok(())
}

/// Checks the artifact one feed entry names against the entry's digest and
/// signature, and returns the artifact's file name.
///
/// # Errors
///
/// Returns [`CliError::NoFileName`] when the entry's URL names no file,
/// [`CliError::IncompleteEntry`] when the entry has no digest or no
/// signature, [`CliError::Io`] when the file cannot be read,
/// [`CliError::DigestMismatch`] or [`CliError::SignatureMismatch`] when the
/// file is not the one the entry describes, and [`CliError::Update`] when
/// `public_key` is not a minisign key.
fn verify_feed_entry<'a>(
    platform: &str,
    entry: &'a FeedEntry,
    directory: &Path,
    public_key: &str,
) -> CliResult<&'a str> {
    let file_name = entry.file_name(platform)?;
    let sha256 = required_field(entry.sha256.as_ref(), "sha256", platform)?;
    let signature = required_field(entry.signature.as_ref(), "signature", platform)?;
    let artifact_path = directory.join(file_name);
    let bytes = std::fs::read(&artifact_path).map_err(CliError::io("read", &artifact_path))?;

    if !sha256_hex(&bytes).eq_ignore_ascii_case(sha256) {
        return Err(CliError::DigestMismatch {
            platform: platform.to_owned(),
            file_name: file_name.to_owned(),
        });
    }

    match verify_signature(public_key, &bytes, signature) {
        Ok(()) => Ok(file_name),
        // A key that is not a key is the caller's mistake, not this artifact's.
        Err(error @ UpdateError::MissingPublicKey) => Err(error.into()),
        Err(_) => Err(CliError::SignatureMismatch {
            platform: platform.to_owned(),
            file_name: file_name.to_owned(),
        }),
    }
}

/// Prints, one per line, the given asset names that the release must not keep.
///
/// # Errors
///
/// Returns [`CliError::Output`] when standard output cannot be written.
fn run_unpublished(args: &[String]) -> CliResult<()> {
    unpublished_assets(args)
        .into_iter()
        .try_for_each(print_line)
}

/// Returns the asset names among `args` that the release must not keep.
fn unpublished_assets(args: &[String]) -> Vec<&str> {
    let windows = windows_build(args);

    args.iter()
        .map(String::as_str)
        .filter(|argument| *argument != WITH_WINDOWS)
        .filter(|name| !is_published_asset(name, windows))
        .collect()
}

/// Writes the checksum file for the published files in `--dir` to `--out`.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] for a file
/// that cannot be read or written, [`CliError::FileNameNotUtf8`] for such a
/// name in the directory, [`CliError::NothingToChecksum`] when the directory
/// holds no published file, and [`CliError::Output`] when the result line
/// cannot be printed.
fn run_checksums(args: &[String]) -> CliResult<()> {
    let directory = Path::new(required_flag(args, "--dir")?);
    let out = Path::new(required_flag(args, "--out")?);

    let names = file_names_in(directory)?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let listed = checksummed_assets(&names, windows_build(args));
    if listed.is_empty() {
        return Err(CliError::NothingToChecksum {
            directory: directory.to_path_buf(),
        });
    }

    let contents = listed
        .into_iter()
        .map(|name| {
            let path = directory.join(name);
            let bytes = std::fs::read(&path).map_err(CliError::io("read", &path))?;
            Ok(checksum_line(&sha256_hex(&bytes), name))
        })
        .collect::<CliResult<String>>()?;

    std::fs::write(out, contents).map_err(CliError::io("write", out))?;
    print_line(format_args!("wrote {}", out.display()))
}

/// Copies each versioned download in `--dir` to its version-free name.
///
/// Prints the fixed names, one per line. Fails when any source is missing or
/// ambiguous, so a release never publishes without every site link.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] when the
/// directory cannot be listed, [`CliError::FileNameNotUtf8`] for such a name
/// in it, [`CliError::ReleaseSet`] when a download has no source or several,
/// [`CliError::Copy`] when a copy fails, and [`CliError::Output`] when a name
/// cannot be printed.
fn run_fixed_copies(args: &[String]) -> CliResult<()> {
    let directory = Path::new(required_flag(args, "--dir")?);

    let names = file_names_in(directory)?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();

    for (source, fixed) in fixed_name_copies(&names, windows_build(args))? {
        let from = directory.join(source);
        let to = directory.join(fixed);
        std::fs::copy(&from, &to).map_err(|source| CliError::Copy { from, to, source })?;
        print_line(fixed)?;
    }

    Ok(())
}

/// Prints, one per line, the version-free names the release must carry.
///
/// The workflow compares what it made and what the draft holds against this
/// list, so the names live in `release_set` only.
///
/// # Errors
///
/// Returns [`CliError::Output`] when standard output cannot be written.
fn run_fixed_names(args: &[String]) -> CliResult<()> {
    fixed_names(windows_build(args))
        .into_iter()
        .try_for_each(print_line)
}

/// Refuses updater artifacts larger than the update client will download.
///
/// `--dir` on its own scans a bundle for the files a feed can name.
/// `--manifest` with `--dir` checks the files that feed names, which is the
/// set an installed copy will download.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, and otherwise whatever
/// [`check_bundle_sizes`] or [`check_manifest_sizes`] returns.
fn run_check_sizes(args: &[String]) -> CliResult<()> {
    let directory = Path::new(required_flag(args, "--dir")?);

    match optional_flag(args, "--manifest")? {
        Some(manifest) => check_manifest_sizes(Path::new(manifest), directory),
        None => check_bundle_sizes(directory),
    }
}

/// Checks every updater artifact found under `directory`.
///
/// # Errors
///
/// Returns what [`updater_artifacts_in`] returns for a bundle that cannot be
/// scanned, [`CliError::NoUpdaterArtifact`] when the scan finds nothing, so
/// that a wrong directory does not pass as a bundle within the limit, and
/// what [`report_sizes`] returns for the files found.
fn check_bundle_sizes(directory: &Path) -> CliResult<()> {
    let found = updater_artifacts_in(directory)?;
    if found.is_empty() {
        let suffixes: Vec<&str> = updater_artifact_kinds()
            .iter()
            .map(|kind| kind.suffix)
            .collect();

        return Err(CliError::NoUpdaterArtifact {
            directory: directory.to_path_buf(),
            suffixes: suffixes.join(", "),
        });
    }

    report_sizes(found)
}

/// Checks the file in `directory` that each entry of the feed at `manifest`
/// names.
///
/// # Errors
///
/// Returns [`CliError::Io`] or [`CliError::Feed`] for a feed that cannot be
/// read or parsed, [`CliError::EmptyFeed`] when it lists no platform,
/// [`CliError::NoFileName`] when an entry's URL names no file, and what
/// [`report_sizes`] returns for the files named.
fn check_manifest_sizes(manifest: &Path, directory: &Path) -> CliResult<()> {
    let (_body, feed) = read_feed(manifest)?;
    if feed.platforms.is_empty() {
        return Err(CliError::EmptyFeed {
            path: manifest.to_path_buf(),
        });
    }

    // A `BTreeMap` yields the platforms sorted, so the report is stable.
    let files = feed
        .platforms
        .iter()
        .map(|(platform, entry)| {
            let file_name = entry.file_name(platform)?;
            Ok((platform.clone(), directory.join(file_name)))
        })
        .collect::<CliResult<Vec<_>>>()?;

    report_sizes(files)
}

/// Returns the updater artifacts under `directory`, nested bundle folders
/// included, as `(platform key, path)` sorted by both.
///
/// # Errors
///
/// Returns [`CliError::Io`] when a directory cannot be listed or an entry
/// cannot be examined, [`CliError::FileNameNotUtf8`] for such a name, and
/// [`CliError::SymlinkedArtifact`] for a symbolic link named like an updater
/// artifact.
fn updater_artifacts_in(directory: &Path) -> CliResult<Vec<(String, PathBuf)>> {
    let mut pending = vec![directory.to_path_buf()];
    let mut found = Vec::new();

    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current).map_err(CliError::io("list", &current))?;

        for entry in entries {
            let entry = entry.map_err(CliError::io("list", &current))?;
            match classify_bundle_entry(&entry, &current)? {
                BundleEntry::Directory => pending.push(entry.path()),
                BundleEntry::Artifact(platform) => found.push((platform.to_owned(), entry.path())),
                BundleEntry::Other => {}
            }
        }
    }
    found.sort();

    Ok(found)
}

/// Says what the entry `entry` of the bundle directory `directory` is to the
/// size check.
///
/// # Errors
///
/// Returns [`CliError::Io`] when the entry's type cannot be read,
/// [`CliError::FileNameNotUtf8`] for such a name, and
/// [`CliError::SymlinkedArtifact`] for a symbolic link named like an updater
/// artifact: the gate must see the file that will be uploaded, not a link.
fn classify_bundle_entry(entry: &std::fs::DirEntry, directory: &Path) -> CliResult<BundleEntry> {
    let path = entry.path();
    let file_type = entry.file_type().map_err(CliError::io("examine", &path))?;
    let name = utf8_file_name(entry.file_name(), directory)?;
    let kind = updater_artifact_kinds()
        .iter()
        .find(|kind| name.ends_with(kind.suffix));

    match kind {
        Some(_) if file_type.is_symlink() => Err(CliError::SymlinkedArtifact { path }),
        Some(kind) if file_type.is_file() => Ok(BundleEntry::Artifact(kind.platform)),
        _ if file_type.is_dir() => Ok(BundleEntry::Directory),
        _ => Ok(BundleEntry::Other),
    }
}

/// Prints each file within the limit and returns every refusal together, so
/// one run names every platform that is over the cap.
///
/// # Errors
///
/// Returns [`CliError::ArtifactSizes`] when any file is refused, and
/// [`CliError::Output`] when a result line cannot be printed.
fn report_sizes(files: Vec<(String, PathBuf)>) -> CliResult<()> {
    let mut failures = Vec::new();

    for (platform, path) in files {
        match check_artifact_file(&path) {
            Ok(length) => print_line(format_args!(
                "{platform}: {} is {length} bytes, within the update client's download limit",
                path.display()
            ))?,
            Err(error) => failures.push(SizeFailure { platform, error }),
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(CliError::ArtifactSizes(SizeFailures(failures)))
    }
}

/// Returns the last path segment of an artifact URL.
///
/// The name is joined to a directory, so it must stay inside it: an empty
/// name, `..` and a name that contains a backslash are refused, and a feed
/// cannot point a check at a path outside `--dir`.
fn artifact_file_name(url: &str) -> Option<&str> {
    url.rsplit('/')
        .next()
        .filter(|name| !name.is_empty() && *name != ".." && !name.contains('\\'))
}

/// Returns the Windows choice the arguments make: published when
/// [`WITH_WINDOWS`] is among them, withheld otherwise.
fn windows_build(args: &[String]) -> WindowsBuild {
    if args.iter().any(|argument| argument == WITH_WINDOWS) {
        WindowsBuild::Published
    } else {
        WindowsBuild::Withheld
    }
}

/// Returns the value that follows the flag `name`.
///
/// # Errors
///
/// Returns [`CliError::Usage`] when the flag is absent, is the last
/// argument, or is followed by another flag.
fn required_flag<'a>(args: &'a [String], name: &str) -> CliResult<&'a str> {
    optional_flag(args, name)?.ok_or(CliError::Usage)
}

/// Returns the value that follows the flag `name`, or `None` when the flag is
/// not given.
///
/// A value that begins with `--` is taken for the next flag: `--out
/// --with-windows` is a missing value, not a file named `--with-windows`.
///
/// # Errors
///
/// Returns [`CliError::Usage`] when the flag is the last argument or is
/// followed by another flag.
fn optional_flag<'a>(args: &'a [String], name: &str) -> CliResult<Option<&'a str>> {
    let Some(position) = args.iter().position(|argument| argument == name) else {
        return Ok(None);
    };

    match args.get(position + 1) {
        Some(value) if !value.starts_with("--") => Ok(Some(value)),
        Some(_) | None => Err(CliError::Usage),
    }
}

/// Returns the names of the entries directly inside `directory`, sorted so
/// the output is reproducible.
///
/// # Errors
///
/// Returns [`CliError::Io`] when the directory cannot be listed and
/// [`CliError::FileNameNotUtf8`] for a name that is not UTF-8.
fn file_names_in(directory: &Path) -> CliResult<Vec<String>> {
    let mut names = std::fs::read_dir(directory)
        .map_err(CliError::io("list", directory))?
        .map(|entry| {
            let entry = entry.map_err(CliError::io("list", directory))?;
            utf8_file_name(entry.file_name(), directory)
        })
        .collect::<CliResult<Vec<String>>>()?;
    names.sort();

    Ok(names)
}

/// Returns `name` as text.
///
/// # Errors
///
/// Returns [`CliError::FileNameNotUtf8`], naming `directory`, when `name` is
/// not UTF-8.
fn utf8_file_name(name: OsString, directory: &Path) -> CliResult<String> {
    name.into_string()
        .map_err(|name| CliError::FileNameNotUtf8 {
            directory: directory.to_path_buf(),
            name,
        })
}

/// Writes `line` and a newline to standard output.
///
/// # Errors
///
/// Returns [`CliError::Output`] when the write fails, as it does when the
/// reader of a pipe has gone away.
fn print_line(line: impl std::fmt::Display) -> CliResult<()> {
    writeln!(std::io::stdout(), "{line}").map_err(CliError::Output)
}

#[cfg(test)]
mod tests {
    use super::{CliError, USAGE, describe, run, unpublished_assets};
    use base64::Engine;
    use minisign::KeyPair;
    use oikonomia_update::sha256_hex;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    const DMG: &str = "Oikonomia_0.2.0_aarch64.dmg";
    const MAC: &str = "Oikonomia.app.tar.gz";
    const APPIMAGE: &str = "Oikonomia_0.2.0_amd64.AppImage";
    const DEB: &str = "Oikonomia_0.2.0_amd64.deb";
    const SETUP: &str = "Oikonomia_0.2.0_x64-setup.exe";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// Runs the tool and returns a failure as the line it would print.
    fn run_text(list: &[&str]) -> Result<(), String> {
        run(&args(list)).map_err(|error| describe(&error))
    }

    fn text(path: &Path) -> &str {
        path.to_str().expect("utf-8 temp path")
    }

    /// A draft's artifacts: each feed artifact with a `.sig` beside it.
    fn draft() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in [MAC, APPIMAGE, SETUP] {
            std::fs::write(dir.path().join(name), format!("bytes of {name}")).expect("artifact");
            std::fs::write(
                dir.path().join(format!("{name}.sig")),
                format!("  signature of {name}\n"),
            )
            .expect("signature");
        }
        for name in [DMG, DEB, "stray.msi"] {
            std::fs::write(dir.path().join(name), format!("bytes of {name}")).expect("download");
        }
        dir
    }

    /// A release download URL on a host the app may fetch from.
    const BASE_URL: &str = "https://github.com/o/r/releases/download/v0.2.0/";

    fn assemble(dir: &Path, out: &Path, extra: &[&str]) -> Result<(), String> {
        assemble_from(BASE_URL, dir, out, extra)
    }

    fn assemble_from(base_url: &str, dir: &Path, out: &Path, extra: &[&str]) -> Result<(), String> {
        let mut list = vec![
            "assemble",
            "--version",
            "v0.2.0",
            "--base-url",
            base_url,
            "--dir",
            text(dir),
            "--out",
            text(out),
        ];
        list.extend_from_slice(extra);
        run_text(&list)
    }

    fn manifest(out: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(out).expect("manifest written")).expect("json")
    }

    #[test]
    fn assemble_writes_a_feed_entry_per_published_platform() {
        let draft = draft();
        let out = draft.path().join("out").with_extension("json");

        assemble(draft.path(), &out, &[]).expect("assemble");

        let manifest = manifest(&out);
        assert_eq!(manifest["version"], "0.2.0");
        assert_eq!(manifest["notes"], "");
        let platforms = manifest["platforms"].as_object().expect("platforms");
        let keys: Vec<&String> = platforms.keys().collect();
        assert_eq!(keys, ["darwin-aarch64", "linux-x86_64"]);

        let linux = &platforms["linux-x86_64"];
        assert_eq!(
            linux["url"],
            format!("https://github.com/o/r/releases/download/v0.2.0/{APPIMAGE}")
        );
        assert_eq!(linux["signature"], format!("signature of {APPIMAGE}"));
        assert_eq!(
            linux["sha256"],
            sha256_hex(format!("bytes of {APPIMAGE}").as_bytes())
        );
    }

    #[test]
    fn assemble_adds_windows_and_notes_when_asked() {
        let draft = draft();
        let out = draft.path().join("latest-out.json");
        let notes = draft.path().join("notes.txt");
        std::fs::write(&notes, "  First release.\n").expect("notes");

        assemble(
            draft.path(),
            &out,
            &["--with-windows", "--notes-file", text(&notes)],
        )
        .expect("assemble");

        let manifest = manifest(&out);
        assert_eq!(manifest["notes"], "First release.");
        assert_eq!(
            manifest["platforms"]["windows-x86_64"]["url"],
            format!("https://github.com/o/r/releases/download/v0.2.0/{SETUP}")
        );
    }

    #[test]
    fn assemble_stops_when_an_artifact_or_its_signature_is_missing() {
        let draft = draft();
        let out = draft.path().join("latest-out.json");

        std::fs::remove_file(draft.path().join(format!("{APPIMAGE}.sig"))).expect("remove sig");
        let err = assemble(draft.path(), &out, &[]).expect_err("missing signature");
        assert!(err.contains(&format!("{APPIMAGE}.sig")), "{err}");

        std::fs::remove_file(draft.path().join(APPIMAGE)).expect("remove artifact");
        let err = assemble(draft.path(), &out, &[]).expect_err("missing artifact");
        assert_eq!(err, "no .AppImage file for linux-x86_64");

        assert!(!out.exists(), "a failed assemble must not write a feed");
    }

    #[test]
    fn assemble_reports_unreadable_inputs() {
        let draft = draft();
        let out = draft.path().join("latest-out.json");

        let err = assemble(&draft.path().join("no-such-dir"), &out, &[]).expect_err("no dir");
        assert!(err.contains("no-such-dir"), "{err}");

        let err = assemble(draft.path(), &out, &["--notes-file", "no-such-notes.txt"])
            .expect_err("no notes");
        assert!(err.starts_with("cannot read no-such-notes.txt: "), "{err}");
    }

    #[test]
    fn missing_flags_and_unknown_commands_print_the_usage() {
        let incomplete = [
            vec![],
            vec!["launch"],
            vec!["assemble", "--version", "v1"],
            vec!["assemble", "--version"],
            vec!["verify", "--manifest", "latest.json"],
            vec!["checksums", "--dir", "."],
            vec!["verify-feed", "--manifest", "latest.json", "--dir", "."],
            vec!["check-sizes"],
            vec!["check-sizes", "--dir"],
            vec!["check-sizes", "--manifest"],
            vec!["check-sizes", "--manifest", "latest.json"],
            // A flag is not a value: `--out` has none here.
            vec!["checksums", "--dir", ".", "--out", "--with-windows"],
            vec![
                "assemble",
                "--version",
                "--base-url",
                "u",
                "--dir",
                ".",
                "--out",
                "o",
            ],
            vec![
                "assemble",
                "--version",
                "v1",
                "--base-url",
                "u",
                "--dir",
                ".",
                "--out",
                "o",
                "--notes-file",
            ],
        ];

        for list in incomplete {
            assert_eq!(run_text(&list), Err(USAGE.to_owned()), "{list:?}");
        }
    }

    #[test]
    fn checksums_list_published_files_and_match_their_bytes() {
        let draft = draft();
        let out = draft.path().join("SHA256SUMS");

        run_text(&[
            "checksums",
            "--dir",
            text(draft.path()),
            "--out",
            text(&out),
        ])
        .expect("checksums");

        let written = std::fs::read_to_string(&out).expect("read");
        let names: Vec<&str> = written
            .lines()
            .map(|line| line.split_once("  ").expect("two-space separator").1)
            .collect();
        assert_eq!(
            names,
            [
                MAC,
                &format!("{MAC}.sig"),
                DMG,
                APPIMAGE,
                &format!("{APPIMAGE}.sig"),
                DEB,
            ]
        );
        for line in written.lines() {
            let (hash, name) = line.split_once("  ").expect("separator");
            let bytes = std::fs::read(draft.path().join(name)).expect("listed file exists");
            assert_eq!(hash, sha256_hex(&bytes), "{name}");
        }

        // Running again must not list the checksum file itself.
        run_text(&[
            "checksums",
            "--dir",
            text(draft.path()),
            "--out",
            text(&out),
        ])
        .expect("second run");
        assert_eq!(std::fs::read_to_string(&out).expect("read"), written);
    }

    #[test]
    fn checksums_refuse_a_directory_with_nothing_to_publish() {
        let empty = tempfile::tempdir().expect("temp dir");
        std::fs::write(empty.path().join("stray.msi"), b"x").expect("stray");
        let out = empty.path().join("SHA256SUMS");

        let err = run_text(&[
            "checksums",
            "--dir",
            text(empty.path()),
            "--out",
            text(&out),
        ])
        .expect_err("nothing to list");

        assert!(err.ends_with("no published files to checksum"), "{err}");
        assert!(!out.exists());
    }

    #[test]
    fn fixed_copies_match_their_sources_and_are_checksummed() {
        let draft = draft();
        let dir = text(draft.path());

        run_text(&["fixed-copies", "--dir", dir]).expect("fixed copies");

        for (fixed, source) in [
            ("Oikonomia-macos-arm64.dmg", DMG),
            ("Oikonomia-linux-x86_64.AppImage", APPIMAGE),
            ("Oikonomia-linux-amd64.deb", DEB),
        ] {
            assert_eq!(
                std::fs::read(draft.path().join(fixed)).expect("copy written"),
                std::fs::read(draft.path().join(source)).expect("source"),
                "{fixed}"
            );
        }
        assert!(
            !draft
                .path()
                .join("Oikonomia-windows-x64-setup.exe")
                .exists()
        );

        // A second run, as after an interrupted promotion, still works.
        run_text(&["fixed-copies", "--dir", dir, "--with-windows"]).expect("rerun");
        assert_eq!(
            std::fs::read(draft.path().join("Oikonomia-windows-x64-setup.exe")).expect("copy"),
            std::fs::read(draft.path().join(SETUP)).expect("source")
        );

        let out = draft.path().join("SHA256SUMS");
        run_text(&["checksums", "--dir", dir, "--out", text(&out)]).expect("checksums");
        let written = std::fs::read_to_string(&out).expect("read");
        assert!(
            written.contains("  Oikonomia-macos-arm64.dmg\n"),
            "{written}"
        );
        assert!(
            !written.contains("Oikonomia-windows-x64-setup.exe"),
            "{written}"
        );
    }

    #[test]
    fn fixed_copies_stop_when_a_source_is_missing() {
        let draft = draft();
        std::fs::remove_file(draft.path().join(DEB)).expect("remove deb");

        let err = run_text(&["fixed-copies", "--dir", text(draft.path())]).expect_err("no deb");

        assert_eq!(err, "no .deb file for linux-deb");
        assert!(!draft.path().join("Oikonomia-linux-amd64.deb").exists());
    }

    #[test]
    fn unpublished_names_what_the_release_must_drop() {
        let assets = args(&[MAC, SETUP, "stray.msi", "latest.json", DEB]);
        assert_eq!(unpublished_assets(&assets), [SETUP, "stray.msi"]);

        let with_windows = args(&["--with-windows", MAC, SETUP, "stray.msi"]);
        assert_eq!(unpublished_assets(&with_windows), ["stray.msi"]);
    }

    /// A manifest, its detached signature and the public key that made it.
    fn signed_manifest(dir: &Path) -> (PathBuf, PathBuf, String) {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let body = br#"{"version":"0.2.0"}"#;
        let signature =
            minisign::sign(None, &secret_key, Cursor::new(body), None, None).expect("sign");

        let manifest = dir.join("latest.json");
        let signature_path = dir.join("latest.json.sig");
        std::fs::write(&manifest, body).expect("manifest");
        std::fs::write(&signature_path, signature.into_string()).expect("signature");
        (
            manifest,
            signature_path,
            public_key.to_box().expect("public box").to_string(),
        )
    }

    fn verify(manifest: &Path, signature_path: &Path, public_key_text: &str) -> Result<(), String> {
        run_text(&[
            "verify",
            "--manifest",
            text(manifest),
            "--sig",
            text(signature_path),
            "--pubkey",
            public_key_text,
        ])
    }

    #[test]
    fn verify_accepts_a_manifest_signed_with_the_given_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (manifest, signature_path, public_key_text) = signed_manifest(dir.path());

        assert_eq!(verify(&manifest, &signature_path, &public_key_text), Ok(()));
    }

    #[test]
    fn verify_rejects_a_changed_manifest_another_key_and_bad_inputs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (manifest, signature_path, public_key_text) = signed_manifest(dir.path());
        let other = tempfile::tempdir().expect("temp dir");
        let (_, _, other_public_key_text) = signed_manifest(other.path());

        let err =
            verify(&manifest, &signature_path, &other_public_key_text).expect_err("another key");
        assert_eq!(err, "update manifest signature is invalid");

        let err = verify(&manifest, &signature_path, "not a key").expect_err("bad key");
        assert_eq!(err, "updater public key is missing or invalid");

        let err =
            verify(&manifest, &dir.path().join("gone.sig"), &public_key_text).expect_err("no sig");
        assert!(err.contains("gone.sig"), "{err}");

        std::fs::write(&manifest, br#"{"version":"9.9.9"}"#).expect("tamper");
        let err = verify(&manifest, &signature_path, &public_key_text).expect_err("tampered");
        assert_eq!(err, "update manifest signature is invalid");

        let err = verify(
            &dir.path().join("gone.json"),
            &signature_path,
            &public_key_text,
        )
        .expect_err("no manifest");
        assert!(err.contains("gone.json"), "{err}");
    }

    #[test]
    fn fixed_names_runs_with_and_without_windows() {
        assert_eq!(run_text(&["fixed-names"]), Ok(()));
        assert_eq!(run_text(&["fixed-names", "--with-windows"]), Ok(()));
    }

    /// A draft whose feed artifacts carry real signatures, written the way
    /// `tauri signer` writes a `.sig`: base64 of the minisign signature file.
    fn signed_draft(secret_key: &minisign::SecretKey) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in [MAC, APPIMAGE, SETUP] {
            let bytes = format!("bytes of {name}");
            std::fs::write(dir.path().join(name), &bytes).expect("artifact");
            sign_into(secret_key, dir.path(), name, bytes.as_bytes());
        }
        for name in [DMG, DEB] {
            std::fs::write(dir.path().join(name), format!("bytes of {name}")).expect("download");
        }
        dir
    }

    fn sign_into(secret_key: &minisign::SecretKey, dir: &Path, name: &str, bytes: &[u8]) {
        let signature =
            minisign::sign(None, secret_key, Cursor::new(bytes), None, None).expect("sign");
        let wrapped = base64::engine::general_purpose::STANDARD.encode(signature.into_string());
        std::fs::write(dir.join(format!("{name}.sig")), wrapped).expect("signature");
    }

    fn public_text(public_key: &minisign::PublicKey) -> String {
        let text = public_key.to_box().expect("public box").to_string();
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    fn verify_feed(
        dir: &Path,
        feed: &Path,
        public_key_text: &str,
        extra: &[&str],
    ) -> Result<(), String> {
        let mut list = vec![
            "verify-feed",
            "--manifest",
            text(feed),
            "--dir",
            text(dir),
            "--pubkey",
            public_key_text,
        ];
        list.extend_from_slice(extra);
        run_text(&list)
    }

    #[test]
    fn verify_feed_accepts_every_artifact_assemble_named() {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&secret_key);
        let public_key_text = public_text(&public_key);
        let feed = draft.path().join("latest.json");

        assemble(draft.path(), &feed, &[]).expect("assemble");
        assert_eq!(
            verify_feed(draft.path(), &feed, &public_key_text, &[]),
            Ok(())
        );

        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble with windows");
        assert_eq!(
            verify_feed(draft.path(), &feed, &public_key_text, &["--with-windows"]),
            Ok(())
        );
    }

    #[test]
    fn verify_feed_refuses_a_feed_without_the_published_platforms() {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&secret_key);
        let public_key_text = public_text(&public_key);
        let feed = draft.path().join("latest.json");

        // Windows is published but the feed has no entry for it.
        assemble(draft.path(), &feed, &[]).expect("assemble");
        let err = verify_feed(draft.path(), &feed, &public_key_text, &["--with-windows"])
            .expect_err("windows missing");
        assert!(err.contains("windows-x86_64"), "{err}");

        // Windows is withheld but the feed still offers it.
        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble");
        let err =
            verify_feed(draft.path(), &feed, &public_key_text, &[]).expect_err("windows extra");
        assert!(err.starts_with("feed platforms are"), "{err}");
    }

    #[test]
    fn verify_feed_refuses_a_changed_installer() {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&secret_key);
        let public_key_text = public_text(&public_key);
        let feed = draft.path().join("latest.json");

        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble");
        std::fs::write(draft.path().join(SETUP), b"another installer").expect("swap");

        let err = verify_feed(draft.path(), &feed, &public_key_text, &["--with-windows"])
            .expect_err("changed installer");
        assert_eq!(
            err,
            format!("windows-x86_64: {SETUP} does not match the sha256 in the feed")
        );
    }

    #[test]
    fn verify_feed_refuses_an_installer_signed_with_another_key() {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let other = KeyPair::generate_unencrypted_keypair().expect("other keypair");
        let draft = signed_draft(&secret_key);
        let public_key_text = public_text(&public_key);
        let feed = draft.path().join("latest.json");

        // Same bytes, so the hash still matches; only the signer differs.
        sign_into(
            &other.sk,
            draft.path(),
            SETUP,
            format!("bytes of {SETUP}").as_bytes(),
        );
        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble");

        let err = verify_feed(draft.path(), &feed, &public_key_text, &["--with-windows"])
            .expect_err("wrong signer");
        assert_eq!(
            err,
            format!("windows-x86_64: the signature of {SETUP} does not verify with the public key")
        );

        let err = verify_feed(draft.path(), &feed, "not a key", &["--with-windows"])
            .expect_err("bad key");
        assert_eq!(err, "updater public key is missing or invalid");
    }

    #[test]
    fn verify_feed_refuses_a_feed_whose_urls_the_app_would_not_fetch() {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&secret_key);
        let public_key_text = public_text(&public_key);
        let feed = draft.path().join("latest.json");

        // Digests and signatures are right; only the host is not one the
        // installed app will contact.
        let base_url = "https://downloads.example.com/v0.2.0";
        assemble_from(base_url, draft.path(), &feed, &[]).expect("assemble");

        let err = verify_feed(draft.path(), &feed, &public_key_text, &[]).expect_err("wrong host");
        assert_eq!(
            err,
            format!(
                "darwin-aarch64: installed copies refuse {base_url}/{MAC}: \
                 update url is not allow-listed"
            )
        );
    }

    #[test]
    fn verify_feed_refuses_a_version_the_app_could_not_read() {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&secret_key);
        let public_key_text = public_text(&public_key);
        let feed = draft.path().join("latest.json");

        assemble(draft.path(), &feed, &[]).expect("assemble");
        let written = std::fs::read_to_string(&feed).expect("feed");
        let edited = written.replace(r#""version": "0.2.0""#, r#""version": "0.2""#);
        assert_ne!(edited, written, "the feed must hold the version to edit");
        std::fs::write(&feed, edited).expect("edit");

        let err = verify_feed(draft.path(), &feed, &public_key_text, &[]).expect_err("version");
        assert_eq!(
            err,
            "installed copies cannot read the feed: version \"0.2\" is not semver"
        );
    }

    #[test]
    fn verify_feed_refuses_an_entry_that_only_says_where_the_file_is() {
        let KeyPair {
            pk: public_key,
            sk: secret_key,
        } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&secret_key);
        let public_key_text = public_text(&public_key);
        // Enough of a feed for `check-sizes`, which only finds the files.
        let feed = draft.path().join("latest.json");
        let body = format!(
            r#"{{"version":"0.2.0","platforms":{{
                "darwin-aarch64":{{"url":"{BASE_URL}{MAC}"}},
                "linux-x86_64":{{"url":"{BASE_URL}{APPIMAGE}"}}
            }}}}"#
        );
        std::fs::write(&feed, body).expect("feed");

        assert_eq!(
            run_text(&[
                "check-sizes",
                "--manifest",
                text(&feed),
                "--dir",
                text(draft.path())
            ]),
            Ok(())
        );
        let err = verify_feed(draft.path(), &feed, &public_key_text, &[]).expect_err("no digests");
        assert_eq!(
            err,
            "installed copies cannot read the feed: update manifest is not valid json"
        );
    }

    #[test]
    fn a_failure_is_printed_with_its_cause() {
        let missing = Path::new("no-such-file.json");
        let error = CliError::io("read", missing)(std::io::Error::other("disk on fire"));

        assert_eq!(error.to_string(), "cannot read no-such-file.json");
        assert_eq!(
            describe(&error),
            "cannot read no-such-file.json: disk on fire"
        );
    }

    #[test]
    fn verify_feed_refuses_a_feed_that_is_not_the_json_the_client_reads() {
        let draft = draft();
        let feed = draft.path().join("latest.json");
        std::fs::write(&feed, br#"{"version":"0.2.0"}"#).expect("feed");

        let err = verify_feed(draft.path(), &feed, "any key", &[]).expect_err("no platforms");

        assert!(
            err.starts_with(&format!("cannot parse {}: ", feed.display())),
            "{err}"
        );
        assert!(err.contains("platforms"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_that_is_not_utf8_is_an_error_not_a_skipped_file() {
        use super::utf8_file_name;
        use std::os::unix::ffi::OsStringExt;

        let name = std::ffi::OsString::from_vec(vec![b'a', 0xff, b'.', b'd', b'e', b'b']);

        let error = utf8_file_name(name, Path::new("artifacts")).expect_err("not UTF-8");

        assert_eq!(
            error.to_string(),
            "file name in artifacts is not UTF-8: a\u{fffd}.deb"
        );
        assert_eq!(
            utf8_file_name("Oikonomia.deb".into(), Path::new("artifacts")).expect("UTF-8"),
            "Oikonomia.deb"
        );
    }

    // macOS refuses to create a file whose name is not UTF-8, so the listing
    // itself can only be exercised on Linux.
    #[cfg(target_os = "linux")]
    #[test]
    fn checksums_refuse_a_directory_holding_a_name_that_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;

        let draft = draft();
        let name = std::ffi::OsString::from_vec(vec![b'a', 0xff, b'.', b'd', b'e', b'b']);
        std::fs::write(draft.path().join(name), b"x").expect("file");
        let out = draft.path().join("SHA256SUMS");

        let err = run_text(&[
            "checksums",
            "--dir",
            text(draft.path()),
            "--out",
            text(&out),
        ])
        .expect_err("a name that is not UTF-8");

        assert!(err.contains("is not UTF-8"), "{err}");
        assert!(!out.exists());
    }

    /// The update client's download cap as a file length.
    fn limit_bytes() -> u64 {
        u64::try_from(oikonomia_update::MAX_ARTIFACT_BYTES).expect("the cap fits in a file length")
    }

    /// Creates a sparse file of `len` bytes at `path`.
    fn file_of_length(path: &Path, len: u64) {
        let file = std::fs::File::create(path).expect("create");
        file.set_len(len).expect("set length");
    }

    /// Runs `check-sizes` with `extra` and returns a failure as printed.
    fn check_sizes(extra: &[&str]) -> Result<(), String> {
        let mut list = vec!["check-sizes"];
        list.extend_from_slice(extra);
        run_text(&list)
    }

    #[test]
    fn check_sizes_accepts_an_at_cap_bundle_and_rejects_one_byte_over() {
        let dir = tempfile::tempdir().expect("temp dir");
        let macos = dir.path().join("macos");
        std::fs::create_dir(&macos).expect("macos dir");
        let archive = macos.join("Oikonomia.app.tar.gz");
        file_of_length(&archive, limit_bytes());

        // A manual download is not an updater artifact, however large.
        let dmg_dir = dir.path().join("dmg");
        std::fs::create_dir(&dmg_dir).expect("dmg dir");
        file_of_length(&dmg_dir.join("Oikonomia.dmg"), limit_bytes() + 50);

        assert_eq!(check_sizes(&["--dir", text(dir.path())]), Ok(()));

        let nsis = dir.path().join("nsis");
        std::fs::create_dir(&nsis).expect("nsis dir");
        let setup = nsis.join("Oikonomia_0.2.0_x64-setup.exe");
        file_of_length(&setup, limit_bytes() + 1);

        let err = check_sizes(&["--dir", text(dir.path())]).expect_err("over the cap");
        assert!(err.contains("windows-x86_64"), "{err}");
        assert!(err.contains(&(limit_bytes() + 1).to_string()), "{err}");
        assert!(err.contains(&limit_bytes().to_string()), "{err}");
        assert!(err.contains("x64-setup.exe"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn check_sizes_refuses_a_symlinked_updater_artifact() {
        let dir = tempfile::tempdir().expect("temp dir");
        let payload = dir.path().join("payload");
        std::fs::write(&payload, b"small").expect("payload");
        std::os::unix::fs::symlink(&payload, dir.path().join(MAC)).expect("link");

        let err = check_sizes(&["--dir", text(dir.path())]).expect_err("symlink");
        assert!(err.contains("symlink"), "{err}");
    }

    #[test]
    fn check_sizes_fails_closed_when_a_bundle_has_no_updater_artifact() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("Oikonomia.deb"), b"deb").expect("deb");
        std::fs::write(dir.path().join("Oikonomia.app.tar.gz.sig"), b"sig").expect("sig");

        let err = check_sizes(&["--dir", text(dir.path())]).expect_err("nothing to check");
        assert!(err.contains("no updater artifact"), "{err}");

        let missing = dir.path().join("no-such-bundle");
        let err = check_sizes(&["--dir", text(&missing)]).expect_err("missing dir");
        assert!(err.contains("no-such-bundle"), "{err}");
    }

    #[test]
    fn check_sizes_checks_every_file_the_feed_names() {
        let dir = tempfile::tempdir().expect("temp dir");
        let appimage = "Oikonomia_0.2.0_amd64.AppImage";
        let setup = "Oikonomia_0.2.0_x64-setup.exe";
        file_of_length(&dir.path().join(appimage), limit_bytes());
        file_of_length(&dir.path().join(setup), 1);

        let manifest = dir.path().join("latest.json");
        let base = "https://github.com/o/r/releases/download/v0.2.0";
        let body = format!(
            r#"{{"platforms":{{
                "linux-x86_64":{{"url":"{base}/{appimage}"}},
                "windows-x86_64":{{"url":"{base}/{setup}"}}
            }}}}"#
        );
        std::fs::write(&manifest, body).expect("manifest");

        assert_eq!(
            check_sizes(&["--manifest", text(&manifest), "--dir", text(dir.path())]),
            Ok(())
        );

        file_of_length(&dir.path().join(setup), limit_bytes() + 1);
        let err = check_sizes(&["--manifest", text(&manifest), "--dir", text(dir.path())])
            .expect_err("feed artifact over the cap");
        assert!(err.contains("windows-x86_64"), "{err}");
        assert!(err.contains(&limit_bytes().to_string()), "{err}");

        let escaped = dir.path().join("bad.json");
        std::fs::write(
            &escaped,
            r#"{"platforms":{"linux-x86_64":{"url":"https://example.com/.."}}}"#,
        )
        .expect("bad manifest");
        let err = check_sizes(&["--manifest", text(&escaped), "--dir", text(dir.path())])
            .expect_err("path escape");
        assert!(err.contains("no file name"), "{err}");

        let empty = dir.path().join("empty.json");
        std::fs::write(&empty, r#"{"platforms":{}}"#).expect("empty platforms");
        let err = check_sizes(&["--manifest", text(&empty), "--dir", text(dir.path())])
            .expect_err("no platforms");
        assert!(err.contains("no platforms"), "{err}");
    }
}
