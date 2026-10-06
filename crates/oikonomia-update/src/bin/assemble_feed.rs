//! Release-lane command line: builds and checks what a release publishes.
//!
//! The promote workflow runs this instead of doing the same work in shell, so
//! that the rules for a release live in tested Rust (`release_set`, `feed`,
//! `verify`) and the feed is checked before publication by the code that
//! will read it in the application. Each subcommand does one step:
//!
//! - `assemble` writes `latest.json` from the artifacts in a directory;
//! - `verify` checks a file against a detached minisign signature;
//! - `verify-feed` checks every artifact a feed names against its hash and
//!   its minisign signature, as the app does before installing;
//! - `unpublished` lists the draft assets that must not be published;
//! - `fixed-copies` writes the version-free copies the website links to;
//! - `fixed-names` prints the version-free names a release must carry;
//! - `checksums` writes the release's `SHA256SUMS` file.
//!
//! A subcommand prints its result to standard output and nothing else, so a
//! workflow can read it line by line. A failure prints one line to standard
//! error and exits with status 1.

use oikonomia_update::{
    FeedArtifact, ReleaseSetError, UpdateError, WindowsBuild, assemble_manifest, checksum_line,
    checksummed_assets, feed_entries, feed_platform_keys, fixed_name_copies, fixed_names,
    is_published_asset, sha256_hex, verify_signature,
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
  assemble_feed verify-feed --manifest <latest.json> --dir <artifact-dir> --pubkey <minisign-pubkey> \
    [--with-windows]
  assemble_feed unpublished [--with-windows] <asset-name>...
  assemble_feed checksums --dir <artifact-dir> --out <SHA256SUMS> [--with-windows]
  assemble_feed fixed-copies --dir <artifact-dir> [--with-windows]
  assemble_feed fixed-names [--with-windows]";

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

    /// A file or directory could not be read, written or copied.
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

/// The part of `latest.json` that `verify-feed` reads.
#[derive(Debug, Deserialize)]
struct Feed {
    /// The artifact of each platform, by platform key.
    platforms: BTreeMap<String, FeedEntry>,
}

/// One platform's artifact as the feed describes it.
#[derive(Debug, Deserialize)]
struct FeedEntry {
    /// Where the app downloads the artifact from.
    url: String,
    /// The minisign signature over the artifact.
    signature: String,
    /// The hex SHA-256 of the artifact.
    sha256: String,
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

/// Writes `feed` entries for the artifacts in `--dir` to `--out`.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] for a file
/// that cannot be read or written, [`CliError::ReleaseSet`] when a platform
/// has no artifact or several, and [`CliError::Update`] when the version or
/// an artifact is not fit for a feed.
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
/// that cannot be read, and [`CliError::Update`] when the key is not a
/// minisign key or the signature does not verify.
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

/// Checks a feed against the files in `--dir` the way the app will: the feed
/// holds exactly the platforms this release publishes, and every file it
/// names hashes to its `sha256` and carries a minisign signature that
/// verifies with `--pubkey`, the key baked into the app. Run before anything
/// is public, so a feed whose artifact the app would refuse never ships.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] or
/// [`CliError::Feed`] for a feed that cannot be read or parsed,
/// [`CliError::FeedPlatforms`] when its platforms are not the published
/// ones, and whatever [`verify_feed_entry`] returns for the first entry that
/// fails.
fn run_verify_feed(args: &[String]) -> CliResult<()> {
    let manifest = Path::new(required_flag(args, "--manifest")?);
    let directory = Path::new(required_flag(args, "--dir")?);
    let public_key = required_flag(args, "--pubkey")?;

    let body = std::fs::read(manifest).map_err(CliError::io("read", manifest))?;
    let feed: Feed = serde_json::from_slice(&body).map_err(|source| CliError::Feed {
        path: manifest.to_path_buf(),
        source,
    })?;

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
/// [`CliError::Io`] when the file cannot be read,
/// [`CliError::DigestMismatch`] or [`CliError::SignatureMismatch`] when the
/// file is not the one the entry describes, and [`CliError::Update`] when
/// `public_key` is not a minisign key.
fn verify_feed_entry<'a>(
    platform: &str,
    entry: &'a FeedEntry,
    directory: &Path,
    public_key: &str,
) -> CliResult<&'a str> {
    // The name is joined to `directory`, so it must stay inside it.
    let file_name = entry
        .url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty() && *name != ".." && !name.contains('\\'))
        .ok_or_else(|| CliError::NoFileName {
            platform: platform.to_owned(),
            url: entry.url.clone(),
        })?;
    let artifact_path = directory.join(file_name);
    let bytes = std::fs::read(&artifact_path).map_err(CliError::io("read", &artifact_path))?;

    if !sha256_hex(&bytes).eq_ignore_ascii_case(&entry.sha256) {
        return Err(CliError::DigestMismatch {
            platform: platform.to_owned(),
            file_name: file_name.to_owned(),
        });
    }

    match verify_signature(public_key, &bytes, &entry.signature) {
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
/// that cannot be read or written, and [`CliError::NothingToChecksum`] when
/// the directory holds no published file.
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

/// Copies each versioned download in `--dir` to its version-free name and
/// prints the fixed names, one per line. Fails when any source is missing or
/// ambiguous, so a release never publishes without every site link.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for a missing flag, [`CliError::Io`] when the
/// directory cannot be listed, [`CliError::ReleaseSet`] when a download has
/// no source or several, and [`CliError::Copy`] when a copy fails.
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

/// Prints, one per line, the version-free names the release must carry. The
/// workflow compares what it made and what the draft holds against this list,
/// so the names live in `release_set` only.
///
/// # Errors
///
/// Returns [`CliError::Output`] when standard output cannot be written.
fn run_fixed_names(args: &[String]) -> CliResult<()> {
    fixed_names(windows_build(args))
        .into_iter()
        .try_for_each(print_line)
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
    use super::{CliError, USAGE, describe, run, unpublished_assets, utf8_file_name};
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

    fn assemble(dir: &Path, out: &Path, extra: &[&str]) -> Result<(), String> {
        let mut list = vec![
            "assemble",
            "--version",
            "v0.2.0",
            "--base-url",
            "https://github.com/o/r/releases/download/v0.2.0/",
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
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let body = br#"{"version":"0.2.0"}"#;
        let signature = minisign::sign(None, &sk, Cursor::new(body), None, None).expect("sign");

        let manifest = dir.join("latest.json");
        let sig = dir.join("latest.json.sig");
        std::fs::write(&manifest, body).expect("manifest");
        std::fs::write(&sig, signature.into_string()).expect("signature");
        (manifest, sig, pk.to_box().expect("public box").to_string())
    }

    fn verify(manifest: &Path, sig: &Path, pubkey: &str) -> Result<(), String> {
        run_text(&[
            "verify",
            "--manifest",
            text(manifest),
            "--sig",
            text(sig),
            "--pubkey",
            pubkey,
        ])
    }

    #[test]
    fn verify_accepts_a_manifest_signed_with_the_given_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (manifest, sig, pubkey) = signed_manifest(dir.path());

        assert_eq!(verify(&manifest, &sig, &pubkey), Ok(()));
    }

    #[test]
    fn verify_rejects_a_changed_manifest_another_key_and_bad_inputs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (manifest, sig, pubkey) = signed_manifest(dir.path());
        let other = tempfile::tempdir().expect("temp dir");
        let (_, _, other_pubkey) = signed_manifest(other.path());

        let err = verify(&manifest, &sig, &other_pubkey).expect_err("another key");
        assert_eq!(err, "update manifest signature is invalid");

        let err = verify(&manifest, &sig, "not a key").expect_err("bad key");
        assert_eq!(err, "updater public key is missing or invalid");

        let err = verify(&manifest, &dir.path().join("gone.sig"), &pubkey).expect_err("no sig");
        assert!(err.contains("gone.sig"), "{err}");

        std::fs::write(&manifest, br#"{"version":"9.9.9"}"#).expect("tamper");
        let err = verify(&manifest, &sig, &pubkey).expect_err("tampered");
        assert_eq!(err, "update manifest signature is invalid");

        let err = verify(&dir.path().join("gone.json"), &sig, &pubkey).expect_err("no manifest");
        assert!(err.contains("gone.json"), "{err}");
    }

    #[test]
    fn fixed_names_runs_with_and_without_windows() {
        assert_eq!(run_text(&["fixed-names"]), Ok(()));
        assert_eq!(run_text(&["fixed-names", "--with-windows"]), Ok(()));
    }

    /// A draft whose feed artifacts carry real signatures, written the way
    /// `tauri signer` writes a `.sig`: base64 of the minisign signature file.
    fn signed_draft(sk: &minisign::SecretKey) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in [MAC, APPIMAGE, SETUP] {
            let bytes = format!("bytes of {name}");
            std::fs::write(dir.path().join(name), &bytes).expect("artifact");
            sign_into(sk, dir.path(), name, bytes.as_bytes());
        }
        for name in [DMG, DEB] {
            std::fs::write(dir.path().join(name), format!("bytes of {name}")).expect("download");
        }
        dir
    }

    fn sign_into(sk: &minisign::SecretKey, dir: &Path, name: &str, bytes: &[u8]) {
        let signature = minisign::sign(None, sk, Cursor::new(bytes), None, None).expect("sign");
        let wrapped = base64::engine::general_purpose::STANDARD.encode(signature.into_string());
        std::fs::write(dir.join(format!("{name}.sig")), wrapped).expect("signature");
    }

    fn public_text(pk: &minisign::PublicKey) -> String {
        let text = pk.to_box().expect("public box").to_string();
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    fn verify_feed(dir: &Path, feed: &Path, pubkey: &str, extra: &[&str]) -> Result<(), String> {
        let mut list = vec![
            "verify-feed",
            "--manifest",
            text(feed),
            "--dir",
            text(dir),
            "--pubkey",
            pubkey,
        ];
        list.extend_from_slice(extra);
        run_text(&list)
    }

    #[test]
    fn verify_feed_accepts_every_artifact_assemble_named() {
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&sk);
        let pubkey = public_text(&pk);
        let feed = draft.path().join("latest.json");

        assemble(draft.path(), &feed, &[]).expect("assemble");
        assert_eq!(verify_feed(draft.path(), &feed, &pubkey, &[]), Ok(()));

        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble with windows");
        assert_eq!(
            verify_feed(draft.path(), &feed, &pubkey, &["--with-windows"]),
            Ok(())
        );
    }

    #[test]
    fn verify_feed_refuses_a_feed_without_the_published_platforms() {
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&sk);
        let pubkey = public_text(&pk);
        let feed = draft.path().join("latest.json");

        // Windows is published but the feed has no entry for it.
        assemble(draft.path(), &feed, &[]).expect("assemble");
        let err = verify_feed(draft.path(), &feed, &pubkey, &["--with-windows"])
            .expect_err("windows missing");
        assert!(err.contains("windows-x86_64"), "{err}");

        // Windows is withheld but the feed still offers it.
        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble");
        let err = verify_feed(draft.path(), &feed, &pubkey, &[]).expect_err("windows extra");
        assert!(err.starts_with("feed platforms are"), "{err}");
    }

    #[test]
    fn verify_feed_refuses_a_changed_installer() {
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let draft = signed_draft(&sk);
        let pubkey = public_text(&pk);
        let feed = draft.path().join("latest.json");

        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble");
        std::fs::write(draft.path().join(SETUP), b"another installer").expect("swap");

        let err = verify_feed(draft.path(), &feed, &pubkey, &["--with-windows"])
            .expect_err("changed installer");
        assert_eq!(
            err,
            format!("windows-x86_64: {SETUP} does not match the sha256 in the feed")
        );
    }

    #[test]
    fn verify_feed_refuses_an_installer_signed_with_another_key() {
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        let other = KeyPair::generate_unencrypted_keypair().expect("other keypair");
        let draft = signed_draft(&sk);
        let pubkey = public_text(&pk);
        let feed = draft.path().join("latest.json");

        // Same bytes, so the hash still matches; only the signer differs.
        sign_into(
            &other.sk,
            draft.path(),
            SETUP,
            format!("bytes of {SETUP}").as_bytes(),
        );
        assemble(draft.path(), &feed, &["--with-windows"]).expect("assemble");

        let err = verify_feed(draft.path(), &feed, &pubkey, &["--with-windows"])
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
}
