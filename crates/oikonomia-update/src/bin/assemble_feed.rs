//! Promote-lane CLI: assemble and verify `latest.json`.
//!
//! `assemble` scans an artifact directory; `verify` checks a manifest and its
//! detached minisign signature with the client's own verifier code;
//! `unpublished` lists the draft assets that must not be published;
//! `checksums` writes the release's `SHA256SUMS` file.
//!
//! The argument surface is parsed into [`Command`] first, and each subcommand
//! is a function of its typed arguments, so the promote lane's behaviour can be
//! tested without spawning the binary.

use oikonomia_update::{
    FeedArtifact, ReleaseSetError, UpdateError, WindowsBuild, assemble_manifest, checksum_line,
    checksummed_assets, feed_entries, is_published_asset, parse_public_key,
};
use sha2::{Digest, Sha256};
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage:
  assemble_feed assemble --version <v> --base-url <url> --dir <artifact-dir> \
    --out <latest.json> [--notes-file <path>] [--with-windows]
  assemble_feed verify --manifest <latest.json> --sig <latest.json.sig> --pubkey <minisign-pubkey>
  assemble_feed unpublished [--with-windows] <asset-name>...
  assemble_feed checksums --dir <artifact-dir> --out <SHA256SUMS> [--with-windows]";

const WITH_WINDOWS: &str = "--with-windows";

/// Everything that can stop the CLI. The `Display` text is what lands on
/// stderr, so the promote workflow log reads the same as it always has.
#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{USAGE}")]
    Usage,
    #[error("notes: {0}")]
    Notes(#[source] std::io::Error),
    #[error("{}: {source}", path.display())]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    ReleaseSet(#[source] ReleaseSetError),
    #[error("assemble: {0}")]
    Assemble(#[source] UpdateError),
    #[error("{}: no published files to checksum", dir.display())]
    NothingToChecksum { dir: PathBuf },
    #[error("pubkey: {0}")]
    PublicKey(#[source] UpdateError),
    #[error("verify: {0}")]
    Verify(#[source] UpdateError),
    #[error("{0}")]
    Stdout(#[source] std::io::Error),
}

impl CliError {
    fn file(path: &Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self::File {
            path: path.to_owned(),
            source,
        }
    }
}

/// A parsed invocation.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Assemble(AssembleArgs),
    Verify(VerifyArgs),
    Checksums(ChecksumsArgs),
    Unpublished(UnpublishedArgs),
}

impl Command {
    /// Parses the arguments after the program name.
    ///
    /// Only the presence of required flags is checked here. Files are read
    /// afterwards, and notes are read before the artifact directory, so a
    /// missing notes file is reported before a missing artifact.
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let Some((subcommand, rest)) = args.split_first() else {
            return Err(CliError::Usage);
        };
        match subcommand.as_str() {
            "assemble" => AssembleArgs::parse(rest).map(Self::Assemble),
            "verify" => VerifyArgs::parse(rest).map(Self::Verify),
            "checksums" => ChecksumsArgs::parse(rest).map(Self::Checksums),
            "unpublished" => Ok(Self::Unpublished(UnpublishedArgs::parse(rest))),
            _ => Err(CliError::Usage),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct AssembleArgs {
    version: String,
    base_url: String,
    dir: PathBuf,
    out: PathBuf,
    notes_file: Option<PathBuf>,
    windows: WindowsBuild,
}

impl AssembleArgs {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        Ok(Self {
            version: required_flag(args, "--version")?,
            base_url: required_flag(args, "--base-url")?,
            dir: required_flag(args, "--dir")?.into(),
            out: required_flag(args, "--out")?.into(),
            notes_file: flag_value(args, "--notes-file").map(PathBuf::from),
            windows: windows_build(args),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct VerifyArgs {
    manifest: PathBuf,
    sig: PathBuf,
    pubkey: String,
}

impl VerifyArgs {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        Ok(Self {
            manifest: required_flag(args, "--manifest")?.into(),
            sig: required_flag(args, "--sig")?.into(),
            pubkey: required_flag(args, "--pubkey")?,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ChecksumsArgs {
    dir: PathBuf,
    out: PathBuf,
    windows: WindowsBuild,
}

impl ChecksumsArgs {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        Ok(Self {
            dir: required_flag(args, "--dir")?.into(),
            out: required_flag(args, "--out")?.into(),
            windows: windows_build(args),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct UnpublishedArgs {
    windows: WindowsBuild,
    /// Asset names, in the order given. `--with-windows` is not a name.
    names: Vec<String>,
}

impl UnpublishedArgs {
    fn parse(args: &[String]) -> Self {
        Self {
            windows: windows_build(args),
            names: args
                .iter()
                .filter(|arg| *arg != WITH_WINDOWS)
                .cloned()
                .collect(),
        }
    }
}

/// Whether `--with-windows` is present anywhere in `args`.
fn windows_build(args: &[String]) -> WindowsBuild {
    if args.iter().any(|arg| arg == WITH_WINDOWS) {
        WindowsBuild::Published
    } else {
        WindowsBuild::Withheld
    }
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1).cloned())
}

fn required_flag(args: &[String], name: &str) -> Result<String, CliError> {
    flag_value(args, name).ok_or(CliError::Usage)
}

/// Parses and runs one invocation. Success text goes to stdout.
fn run(args: &[String]) -> Result<(), String> {
    execute(args).map_err(|error| error.to_string())
}

fn execute(args: &[String]) -> Result<(), CliError> {
    match Command::parse(args)? {
        Command::Assemble(args) => {
            let out = run_assemble(&args)?;
            writeln!(std::io::stdout(), "wrote {}", out.display()).map_err(CliError::Stdout)
        }
        Command::Verify(args) => {
            run_verify(&args)?;
            writeln!(std::io::stdout(), "manifest signature ok").map_err(CliError::Stdout)
        }
        Command::Checksums(args) => {
            let out = run_checksums(&args)?;
            writeln!(std::io::stdout(), "wrote {}", out.display()).map_err(CliError::Stdout)
        }
        Command::Unpublished(args) => run_unpublished(&args),
    }
}

/// Writes the assembled manifest to `args.out` and returns that path.
fn run_assemble(args: &AssembleArgs) -> Result<&Path, CliError> {
    let notes = read_notes(args.notes_file.as_deref())?;
    let names = file_names_in(&args.dir)?;
    let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
    let entries = feed_entries(&borrowed, args.windows).map_err(CliError::ReleaseSet)?;

    let mut artifacts = Vec::with_capacity(entries.len());
    for (platform, file_name) in entries {
        artifacts.push(read_feed_artifact(&args.dir, platform, file_name)?);
    }

    let manifest = assemble_manifest(&args.version, notes.trim(), &args.base_url, &artifacts)
        .map_err(CliError::Assemble)?;
    std::fs::write(&args.out, manifest).map_err(CliError::file(&args.out))?;
    Ok(&args.out)
}

fn read_notes(path: Option<&Path>) -> Result<String, CliError> {
    match path {
        Some(path) => std::fs::read_to_string(path).map_err(CliError::Notes),
        None => Ok(String::new()),
    }
}

/// File names directly inside `dir`, sorted so later output is reproducible.
fn file_names_in(dir: &Path) -> Result<Vec<String>, CliError> {
    let entries = std::fs::read_dir(dir).map_err(CliError::file(dir))?;

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(CliError::file(dir))?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }
    names.sort_unstable();

    Ok(names)
}

/// Reads one feed artifact and its detached `.sig` from `dir`.
fn read_feed_artifact(
    dir: &Path,
    platform: &str,
    file_name: &str,
) -> Result<FeedArtifact, CliError> {
    let file = dir.join(file_name);
    let bytes = std::fs::read(&file).map_err(CliError::file(&file))?;
    let signature_path = dir.join(format!("{file_name}.sig"));
    let signature =
        std::fs::read_to_string(&signature_path).map_err(CliError::file(&signature_path))?;
    Ok(FeedArtifact {
        platform: platform.to_owned(),
        file_name: file_name.to_owned(),
        signature: signature.trim().to_owned(),
        sha256_hex: sha256_hex(&bytes),
    })
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Writes the checksum file for the published files in `args.dir`.
fn run_checksums(args: &ChecksumsArgs) -> Result<&Path, CliError> {
    let names = file_names_in(&args.dir)?;
    let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
    let listed = checksummed_assets(&borrowed, args.windows);
    if listed.is_empty() {
        return Err(CliError::NothingToChecksum {
            dir: args.dir.clone(),
        });
    }

    let mut contents = String::new();
    for name in listed {
        let file = args.dir.join(name);
        let bytes = std::fs::read(&file).map_err(CliError::file(&file))?;
        contents.push_str(&checksum_line(&sha256_hex(&bytes), name));
    }

    std::fs::write(&args.out, contents).map_err(CliError::file(&args.out))?;
    Ok(&args.out)
}

/// Prints, one per line, the asset names the release must not keep.
fn run_unpublished(args: &UnpublishedArgs) -> Result<(), CliError> {
    let mut stdout = std::io::stdout();
    for name in &args.names {
        if !is_published_asset(name, args.windows) {
            writeln!(stdout, "{name}").map_err(CliError::Stdout)?;
        }
    }
    Ok(())
}

/// The asset names among `args` that the release must not keep.
#[cfg(test)]
fn unpublished_assets(args: &[String]) -> Vec<&str> {
    let windows = windows_build(args);
    args.iter()
        .map(String::as_str)
        .filter(|arg| *arg != WITH_WINDOWS)
        .filter(|name| !is_published_asset(name, windows))
        .collect()
}

fn run_verify(args: &VerifyArgs) -> Result<(), CliError> {
    let body = std::fs::read(&args.manifest).map_err(CliError::file(&args.manifest))?;
    let signature = std::fs::read_to_string(&args.sig).map_err(CliError::file(&args.sig))?;
    let key = parse_public_key(&args.pubkey).map_err(CliError::PublicKey)?;
    oikonomia_update::verify_manifest_bytes(&key, &body, &signature).map_err(CliError::Verify)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod typed_command {
    use super::*;
    use minisign::{KeyPair, SecretKey};
    use std::io::Cursor;
    use tempfile::TempDir;

    const ARTIFACT: &[u8] = b"not a real bundle, only bytes to hash";
    const BASE_URL: &str = "https://example.invalid/releases/download/v1.2.3";
    const MAC_NAME: &str = "Oikonomia.app.tar.gz";
    const LINUX_NAME: &str = "Oikonomia.AppImage";

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    /// A throwaway keypair made fresh for each test; never a release key.
    fn throwaway_keys() -> (String, SecretKey) {
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
        (pk.to_box().expect("public box").to_string(), sk)
    }

    fn sign(secret: &SecretKey, data: &[u8]) -> String {
        minisign::sign(None, secret, Cursor::new(data), None, None)
            .expect("sign")
            .into_string()
    }

    fn write(path: &Path, contents: &[u8]) {
        std::fs::write(path, contents).expect("write fixture");
    }

    /// An artifact directory holding the macOS and Linux feed files.
    fn feed_dir() -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        write(&dir.path().join(MAC_NAME), ARTIFACT);
        write(&dir.path().join(format!("{MAC_NAME}.sig")), b"  MAC-SIG\n");
        write(&dir.path().join(LINUX_NAME), ARTIFACT);
        write(
            &dir.path().join(format!("{LINUX_NAME}.sig")),
            b"  LINUX-SIG\n",
        );
        dir
    }

    fn assemble_args(dir: &Path) -> AssembleArgs {
        AssembleArgs {
            version: "v1.2.3".to_owned(),
            base_url: BASE_URL.to_owned(),
            dir: dir.to_owned(),
            out: dir.join("latest.json"),
            notes_file: None,
            windows: WindowsBuild::Withheld,
        }
    }

    fn message(error: &CliError) -> String {
        error.to_string()
    }

    #[test]
    fn parses_a_full_assemble_invocation() {
        let parsed = Command::parse(&args(&[
            "assemble",
            "--version",
            "v1.2.3",
            "--base-url",
            BASE_URL,
            "--dir",
            "artifacts",
            "--out",
            "artifacts/latest.json",
            "--notes-file",
            "notes.txt",
            "--with-windows",
        ]))
        .expect("parse");
        assert_eq!(
            parsed,
            Command::Assemble(AssembleArgs {
                version: "v1.2.3".to_owned(),
                base_url: BASE_URL.to_owned(),
                dir: PathBuf::from("artifacts"),
                out: PathBuf::from("artifacts/latest.json"),
                notes_file: Some(PathBuf::from("notes.txt")),
                windows: WindowsBuild::Published,
            })
        );

        let withheld = Command::parse(&args(&[
            "assemble",
            "--version",
            "v1.2.3",
            "--base-url",
            BASE_URL,
            "--dir",
            "artifacts",
            "--out",
            "artifacts/latest.json",
        ]))
        .expect("parse");
        assert!(matches!(
            withheld,
            Command::Assemble(AssembleArgs {
                windows: WindowsBuild::Withheld,
                notes_file: None,
                ..
            })
        ));
    }

    #[test]
    fn parses_a_verify_invocation() {
        let parsed = Command::parse(&args(&[
            "verify",
            "--manifest",
            "latest.json",
            "--sig",
            "latest.json.sig",
            "--pubkey",
            "KEY",
        ]))
        .expect("parse");
        assert_eq!(
            parsed,
            Command::Verify(VerifyArgs {
                manifest: PathBuf::from("latest.json"),
                sig: PathBuf::from("latest.json.sig"),
                pubkey: "KEY".to_owned(),
            })
        );
    }

    #[test]
    fn parses_a_checksums_invocation() {
        let parsed = Command::parse(&args(&[
            "checksums",
            "--dir",
            "artifacts",
            "--out",
            "SHA256SUMS",
            "--with-windows",
        ]))
        .expect("parse");
        assert_eq!(
            parsed,
            Command::Checksums(ChecksumsArgs {
                dir: PathBuf::from("artifacts"),
                out: PathBuf::from("SHA256SUMS"),
                windows: WindowsBuild::Published,
            })
        );
    }

    #[test]
    fn parses_an_unpublished_invocation() {
        let parsed = Command::parse(&args(&[
            "unpublished",
            "Oikonomia.app.tar.gz",
            "--with-windows",
            "stray.msi",
        ]))
        .expect("parse");
        assert_eq!(
            parsed,
            Command::Unpublished(UnpublishedArgs {
                windows: WindowsBuild::Published,
                names: vec!["Oikonomia.app.tar.gz".to_owned(), "stray.msi".to_owned(),],
            })
        );

        let withheld = Command::parse(&args(&["unpublished"])).expect("parse");
        assert_eq!(
            withheld,
            Command::Unpublished(UnpublishedArgs {
                windows: WindowsBuild::Withheld,
                names: Vec::new(),
            })
        );
    }

    #[test]
    fn no_subcommand_or_an_unknown_one_is_usage() {
        for invocation in [args(&[]), args(&["publish"]), args(&["--version", "1"])] {
            let error = Command::parse(&invocation).expect_err("usage");
            assert!(matches!(error, CliError::Usage), "{invocation:?}");
            assert_eq!(message(&error), USAGE);
        }
    }

    #[test]
    fn each_missing_required_flag_is_usage() {
        let full = [
            "--version",
            "1",
            "--base-url",
            "u",
            "--dir",
            "d",
            "--out",
            "o",
        ];
        for skipped in (0..full.len()).step_by(2) {
            let mut partial: Vec<&str> = vec!["assemble"];
            for (index, value) in full.iter().enumerate() {
                if index != skipped && index != skipped + 1 {
                    partial.push(value);
                }
            }
            let error = Command::parse(&args(&partial)).expect_err("usage");
            assert!(matches!(error, CliError::Usage), "{partial:?}");
        }

        let error =
            Command::parse(&args(&["verify", "--manifest", "m", "--sig", "s"])).expect_err("usage");
        assert!(matches!(error, CliError::Usage));

        let error = Command::parse(&args(&["checksums", "--dir", "d"])).expect_err("usage");
        assert!(matches!(error, CliError::Usage));
    }

    #[test]
    fn a_flag_with_no_value_is_missing() {
        let error = Command::parse(&args(&[
            "verify",
            "--manifest",
            "m",
            "--sig",
            "s",
            "--pubkey",
        ]))
        .expect_err("usage");
        assert!(matches!(error, CliError::Usage));

        let error =
            Command::parse(&args(&["checksums", "--dir", "d", "--out"])).expect_err("usage");
        assert!(matches!(error, CliError::Usage));
    }

    #[test]
    fn sha256_hex_is_lowercase_hex_of_the_bytes() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn assemble_writes_the_manifest_the_library_builds() {
        let dir = feed_dir();
        let notes = dir.path().join("notes.txt");
        write(&notes, b"\n  Oikonomia v1.2.3  \n");
        let mut args = assemble_args(dir.path());
        args.notes_file = Some(notes);

        let out = run_assemble(&args).expect("assemble");
        assert_eq!(out, dir.path().join("latest.json"));

        let expected = assemble_manifest(
            "v1.2.3",
            "Oikonomia v1.2.3",
            BASE_URL,
            &[
                FeedArtifact {
                    platform: "darwin-aarch64".to_owned(),
                    file_name: MAC_NAME.to_owned(),
                    signature: "MAC-SIG".to_owned(),
                    sha256_hex: sha256_hex(ARTIFACT),
                },
                FeedArtifact {
                    platform: "linux-x86_64".to_owned(),
                    file_name: LINUX_NAME.to_owned(),
                    signature: "LINUX-SIG".to_owned(),
                    sha256_hex: sha256_hex(ARTIFACT),
                },
            ],
        )
        .expect("expected manifest");
        let written = std::fs::read_to_string(out).expect("read manifest");
        assert_eq!(written, expected);
    }

    #[test]
    fn assemble_without_notes_writes_empty_notes() {
        let dir = feed_dir();
        let args = assemble_args(dir.path());
        let out = run_assemble(&args).expect("assemble");
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out).expect("read")).expect("json");
        assert_eq!(written["notes"], "");
        assert_eq!(written["version"], "1.2.3");
    }

    #[test]
    fn assemble_without_feed_artifacts_writes_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let args = assemble_args(dir.path());
        let error = run_assemble(&args).expect_err("no artifacts");
        assert!(matches!(error, CliError::ReleaseSet(_)));
        assert_eq!(message(&error), "no .app.tar.gz file for darwin-aarch64");
        assert!(!args.out.exists());
    }

    #[test]
    fn assemble_rejects_a_blank_version_and_writes_nothing() {
        let dir = feed_dir();
        let mut args = assemble_args(dir.path());
        args.version = " ".to_owned();
        let error = run_assemble(&args).expect_err("blank version");
        assert!(matches!(
            error,
            CliError::Assemble(UpdateError::ManifestParse)
        ));
        assert!(message(&error).starts_with("assemble: "));
        assert!(!args.out.exists());
    }

    #[test]
    fn assemble_reports_a_missing_platform_artifact() {
        let dir = feed_dir();
        std::fs::remove_file(dir.path().join(LINUX_NAME)).expect("remove artifact");
        let args = assemble_args(dir.path());
        let error = run_assemble(&args).expect_err("missing artifact");
        assert_eq!(message(&error), "no .AppImage file for linux-x86_64");
        assert!(!args.out.exists());
    }

    #[test]
    fn assemble_requires_the_detached_signature() {
        let dir = feed_dir();
        let sig = dir.path().join(format!("{LINUX_NAME}.sig"));
        std::fs::remove_file(&sig).expect("remove sig");
        let args = assemble_args(dir.path());
        let error = run_assemble(&args).expect_err("missing sig");
        assert!(matches!(&error, CliError::File { path, .. } if *path == sig));
    }

    #[test]
    fn unreadable_notes_stop_before_any_artifact_is_read() {
        let dir = TempDir::new().expect("tempdir");
        let mut args = assemble_args(dir.path());
        args.notes_file = Some(dir.path().join("absent-notes.txt"));
        let error = run_assemble(&args).expect_err("notes");
        assert!(matches!(error, CliError::Notes(_)));
        assert!(message(&error).starts_with("notes: "));
    }

    #[test]
    fn feed_set_errors_precede_signature_reads() {
        let dir = feed_dir();
        std::fs::remove_file(dir.path().join(format!("{MAC_NAME}.sig"))).expect("remove sig");
        std::fs::remove_file(dir.path().join(LINUX_NAME)).expect("remove artifact");
        let args = assemble_args(dir.path());
        let error = run_assemble(&args).expect_err("feed set");
        assert!(matches!(error, CliError::ReleaseSet(_)));
        assert_eq!(message(&error), "no .AppImage file for linux-x86_64");
    }

    #[test]
    fn unwritable_output_is_reported_by_path() {
        let dir = feed_dir();
        let mut args = assemble_args(dir.path());
        args.out = dir.path().join("no-such-dir").join("latest.json");
        let error = run_assemble(&args).expect_err("unwritable");
        assert!(matches!(&error, CliError::File { path, .. } if *path == args.out));
    }

    /// Writes `body` as a manifest with a detached signature from `secret`.
    fn signed_manifest(dir: &Path, body: &[u8], secret: &SecretKey) -> VerifyArgs {
        let manifest = dir.join("latest.json");
        let sig = dir.join("latest.json.sig");
        write(&manifest, body);
        write(&sig, sign(secret, body).as_bytes());
        VerifyArgs {
            manifest,
            sig,
            pubkey: String::new(),
        }
    }

    #[test]
    fn verify_accepts_a_manifest_signed_by_the_given_key() {
        let dir = TempDir::new().expect("tempdir");
        let (public, secret) = throwaway_keys();
        let mut args = signed_manifest(dir.path(), b"{\"version\":\"1.2.3\"}", &secret);
        args.pubkey = public;
        run_verify(&args).expect("good signature verifies");
    }

    #[test]
    fn verify_rejects_a_tampered_manifest() {
        let dir = TempDir::new().expect("tempdir");
        let (public, secret) = throwaway_keys();
        let mut args = signed_manifest(dir.path(), b"{\"version\":\"1.2.3\"}", &secret);
        args.pubkey = public;
        write(&args.manifest, b"{\"version\":\"9.9.9\"}");
        let error = run_verify(&args).expect_err("tampered body");
        assert!(matches!(
            error,
            CliError::Verify(UpdateError::ManifestSignature)
        ));
        assert!(message(&error).starts_with("verify: "));
    }

    #[test]
    fn verify_rejects_a_tampered_signature() {
        let dir = TempDir::new().expect("tempdir");
        let (public, secret) = throwaway_keys();
        let body = b"{\"version\":\"1.2.3\"}";
        let mut args = signed_manifest(dir.path(), body, &secret);
        args.pubkey = public;
        let (_other_public, other_secret) = throwaway_keys();
        write(&args.sig, sign(&other_secret, body).as_bytes());
        let error = run_verify(&args).expect_err("signature from another key");
        assert!(matches!(
            error,
            CliError::Verify(UpdateError::ManifestSignature)
        ));
    }

    #[test]
    fn verify_rejects_an_unparseable_public_key() {
        let dir = TempDir::new().expect("tempdir");
        let (_public, secret) = throwaway_keys();
        let mut args = signed_manifest(dir.path(), b"{}", &secret);
        args.pubkey = "not a key".to_owned();
        let error = run_verify(&args).expect_err("bad key");
        assert!(matches!(
            error,
            CliError::PublicKey(UpdateError::MissingPublicKey)
        ));
        assert!(message(&error).starts_with("pubkey: "));
    }

    #[test]
    fn verify_reports_missing_files_by_path() {
        let dir = TempDir::new().expect("tempdir");
        let (public, secret) = throwaway_keys();
        let mut args = signed_manifest(dir.path(), b"{}", &secret);
        args.pubkey = public;

        std::fs::remove_file(&args.sig).expect("remove sig");
        let error = run_verify(&args).expect_err("missing sig");
        assert!(matches!(&error, CliError::File { path, .. } if *path == args.sig));

        std::fs::remove_file(&args.manifest).expect("remove manifest");
        let error = run_verify(&args).expect_err("missing manifest");
        assert!(matches!(&error, CliError::File { path, .. } if *path == args.manifest));
    }

    #[test]
    fn run_dispatches_every_subcommand_and_rejects_usage() {
        let dir = feed_dir();
        let out = dir.path().join("latest.json");
        let dir_arg = dir.path().to_str().expect("utf-8 path");
        let out_arg = out.to_str().expect("utf-8 path");
        run(&args(&[
            "assemble",
            "--version",
            "1.2.3",
            "--base-url",
            BASE_URL,
            "--dir",
            dir_arg,
            "--out",
            out_arg,
        ]))
        .expect("assemble via run");

        let (public, secret) = throwaway_keys();
        let body = std::fs::read(&out).expect("manifest");
        let sig = dir.path().join("latest.json.sig");
        write(&sig, sign(&secret, &body).as_bytes());
        let sig_arg = sig.to_str().expect("utf-8 path");
        run(&args(&[
            "verify",
            "--manifest",
            out_arg,
            "--sig",
            sig_arg,
            "--pubkey",
            &public,
        ]))
        .expect("verify via run");

        let sums = dir.path().join("SHA256SUMS");
        let sums_arg = sums.to_str().expect("utf-8 path");
        run(&args(&["checksums", "--dir", dir_arg, "--out", sums_arg])).expect("checksums via run");

        run(&args(&["unpublished", "stray.msi"])).expect("unpublished via run");

        assert_eq!(run(&args(&["help"])).expect_err("usage"), USAGE);
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{USAGE, run, sha256_hex, unpublished_assets};
    use minisign::KeyPair;
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
        run(&args(&list))
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
        assert!(err.starts_with("notes:"), "{err}");
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
        ];

        for list in incomplete {
            assert_eq!(run(&args(&list)), Err(USAGE.to_owned()), "{list:?}");
        }
    }

    #[test]
    fn checksums_list_published_files_and_match_their_bytes() {
        let draft = draft();
        let out = draft.path().join("SHA256SUMS");

        run(&args(&[
            "checksums",
            "--dir",
            text(draft.path()),
            "--out",
            text(&out),
        ]))
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
        run(&args(&[
            "checksums",
            "--dir",
            text(draft.path()),
            "--out",
            text(&out),
        ]))
        .expect("second run");
        assert_eq!(std::fs::read_to_string(&out).expect("read"), written);
    }

    #[test]
    fn checksums_refuse_a_directory_with_nothing_to_publish() {
        let empty = tempfile::tempdir().expect("temp dir");
        std::fs::write(empty.path().join("stray.msi"), b"x").expect("stray");
        let out = empty.path().join("SHA256SUMS");

        let err = run(&args(&[
            "checksums",
            "--dir",
            text(empty.path()),
            "--out",
            text(&out),
        ]))
        .expect_err("nothing to list");

        assert!(err.ends_with("no published files to checksum"), "{err}");
        assert!(!out.exists());
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
        run(&args(&[
            "verify",
            "--manifest",
            text(manifest),
            "--sig",
            text(sig),
            "--pubkey",
            pubkey,
        ]))
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
        assert!(err.starts_with("verify:"), "{err}");

        let err = verify(&manifest, &sig, "not a key").expect_err("bad key");
        assert!(err.starts_with("pubkey:"), "{err}");

        let err = verify(&manifest, &dir.path().join("gone.sig"), &pubkey).expect_err("no sig");
        assert!(err.contains("gone.sig"), "{err}");

        std::fs::write(&manifest, br#"{"version":"9.9.9"}"#).expect("tamper");
        let err = verify(&manifest, &sig, &pubkey).expect_err("tampered");
        assert!(err.starts_with("verify:"), "{err}");

        let err = verify(&dir.path().join("gone.json"), &sig, &pubkey).expect_err("no manifest");
        assert!(err.contains("gone.json"), "{err}");
    }
}
