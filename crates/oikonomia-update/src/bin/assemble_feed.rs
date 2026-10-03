//! Promote-lane CLI: assemble and verify `latest.json`.
//!
//! `assemble` scans an artifact directory; `verify` checks a manifest and its
//! detached minisign signature with the client's own verifier code.
//!
//! The argument surface is parsed into [`Command`] first, and each subcommand
//! is a function of its typed arguments, so the promote lane's behaviour can be
//! tested without spawning the binary.

use oikonomia_update::{FeedArtifact, UpdateError, assemble_manifest, parse_public_key};
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
    --out <latest.json> [--notes-file <path>] --platform <key>=<file> [--platform ...]
  assemble_feed verify --manifest <latest.json> --sig <latest.json.sig> --pubkey <minisign-pubkey>";

/// Everything that can stop the CLI. The `Display` text is what lands on
/// stderr, so the promote workflow log reads the same as it always has.
#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{USAGE}")]
    Usage,
    #[error("notes: {0}")]
    Notes(#[source] std::io::Error),
    #[error("bad --platform mapping: {0}")]
    PlatformMapping(String),
    #[error("{}: {source}", path.display())]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("assemble: {0}")]
    Assemble(#[source] UpdateError),
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
}

impl Command {
    /// Parses the arguments after the program name.
    ///
    /// Only the presence of required flags is checked here; `--platform`
    /// mappings are validated during assembly, in argument order, so error
    /// precedence matches the order the files are read in.
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let Some((subcommand, rest)) = args.split_first() else {
            return Err(CliError::Usage);
        };
        match subcommand.as_str() {
            "assemble" => AssembleArgs::parse(rest).map(Self::Assemble),
            "verify" => VerifyArgs::parse(rest).map(Self::Verify),
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
    /// Raw `<platform>=<file>` values, in the order given.
    platforms: Vec<String>,
}

impl AssembleArgs {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        Ok(Self {
            version: required_flag(args, "--version")?,
            base_url: required_flag(args, "--base-url")?,
            dir: required_flag(args, "--dir")?.into(),
            out: required_flag(args, "--out")?.into(),
            notes_file: flag_value(args, "--notes-file").map(PathBuf::from),
            platforms: flag_values(args, "--platform"),
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

/// One `--platform <key>=<file>` mapping.
#[derive(Debug, PartialEq, Eq)]
struct PlatformMapping<'a> {
    platform: &'a str,
    file_name: &'a str,
}

impl<'a> PlatformMapping<'a> {
    fn parse(mapping: &'a str) -> Result<Self, CliError> {
        let (platform, file_name) = mapping
            .split_once('=')
            .ok_or_else(|| CliError::PlatformMapping(mapping.to_owned()))?;
        Ok(Self {
            platform,
            file_name,
        })
    }
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn required_flag(args: &[String], name: &str) -> Result<String, CliError> {
    flag_value(args, name).ok_or(CliError::Usage)
}

fn flag_values(args: &[String], name: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == name && index + 1 < args.len() {
            values.push(args[index + 1].clone());
            index += 2;
        } else {
            index += 1;
        }
    }
    values
}

/// Parses and runs one invocation, writing the success line to stdout.
fn run(args: &[String]) -> Result<(), CliError> {
    let success = match Command::parse(args)? {
        Command::Assemble(args) => {
            let out = run_assemble(&args)?;
            format!("wrote {}", out.display())
        }
        Command::Verify(args) => {
            run_verify(&args)?;
            "manifest signature ok".to_owned()
        }
    };
    writeln!(std::io::stdout(), "{success}").map_err(CliError::Stdout)
}

/// Writes the assembled manifest to `args.out` and returns that path.
fn run_assemble(args: &AssembleArgs) -> Result<&Path, CliError> {
    let notes = read_notes(args.notes_file.as_deref())?;
    let mut artifacts = Vec::with_capacity(args.platforms.len());
    for mapping in &args.platforms {
        let mapping = PlatformMapping::parse(mapping)?;
        artifacts.push(read_artifact(&args.dir, &mapping)?);
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

/// Reads an artifact and its detached `.sig` from `dir` into a feed entry.
fn read_artifact(dir: &Path, mapping: &PlatformMapping<'_>) -> Result<FeedArtifact, CliError> {
    let file = dir.join(mapping.file_name);
    let bytes = std::fs::read(&file).map_err(CliError::file(&file))?;
    let signature_path = dir.join(format!("{}.sig", mapping.file_name));
    let signature =
        std::fs::read_to_string(&signature_path).map_err(CliError::file(&signature_path))?;
    Ok(FeedArtifact {
        platform: mapping.platform.to_owned(),
        file_name: mapping.file_name.to_owned(),
        signature: signature.trim().to_owned(),
        sha256_hex: sha256_hex(&bytes),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn run_verify(args: &VerifyArgs) -> Result<(), CliError> {
    let body = std::fs::read(&args.manifest).map_err(CliError::file(&args.manifest))?;
    let signature = std::fs::read_to_string(&args.sig).map_err(CliError::file(&args.sig))?;
    let key = parse_public_key(&args.pubkey).map_err(CliError::PublicKey)?;
    oikonomia_update::verify_manifest_bytes(&key, &body, &signature).map_err(CliError::Verify)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
#[expect(clippy::panic, reason = "tests fail loudly by design")]
mod tests {
    use super::*;
    use minisign::{KeyPair, SecretKey};
    use std::io::Cursor;
    use tempfile::TempDir;

    const ARTIFACT: &[u8] = b"not a real bundle, only bytes to hash";
    const BASE_URL: &str = "https://example.invalid/releases/download/v1.2.3";

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

    /// An artifact directory holding `app.tar.gz` and its `.sig`.
    fn artifact_dir() -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        write(&dir.path().join("app.tar.gz"), ARTIFACT);
        write(&dir.path().join("app.tar.gz.sig"), b"  ARTIFACT-SIG\n");
        dir
    }

    fn assemble_args(dir: &Path, platforms: &[&str]) -> AssembleArgs {
        AssembleArgs {
            version: "v1.2.3".to_owned(),
            base_url: BASE_URL.to_owned(),
            dir: dir.to_owned(),
            out: dir.join("latest.json"),
            notes_file: None,
            platforms: platforms.iter().map(|value| (*value).to_owned()).collect(),
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
            "--platform",
            "darwin-aarch64=a.tar.gz",
            "--platform",
            "linux-x86_64=b.tar.gz",
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
                platforms: vec![
                    "darwin-aarch64=a.tar.gz".to_owned(),
                    "linux-x86_64=b.tar.gz".to_owned(),
                ],
            })
        );
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
        assert_eq!(
            flag_values(&args(&["--platform"]), "--platform"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn platform_mapping_splits_on_the_first_equals_sign() {
        assert_eq!(
            PlatformMapping::parse("darwin-aarch64=a=b.tar.gz").expect("mapping"),
            PlatformMapping {
                platform: "darwin-aarch64",
                file_name: "a=b.tar.gz",
            }
        );
        let error = PlatformMapping::parse("darwin-aarch64").expect_err("no equals");
        assert_eq!(message(&error), "bad --platform mapping: darwin-aarch64");
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
        let dir = artifact_dir();
        let notes = dir.path().join("notes.txt");
        write(&notes, b"\n  Oikonomia v1.2.3  \n");
        let mut args = assemble_args(dir.path(), &["darwin-aarch64=app.tar.gz"]);
        args.notes_file = Some(notes);

        let out = run_assemble(&args).expect("assemble");
        assert_eq!(out, dir.path().join("latest.json"));

        let expected = assemble_manifest(
            "v1.2.3",
            "Oikonomia v1.2.3",
            BASE_URL,
            &[FeedArtifact {
                platform: "darwin-aarch64".to_owned(),
                file_name: "app.tar.gz".to_owned(),
                signature: "ARTIFACT-SIG".to_owned(),
                sha256_hex: sha256_hex(ARTIFACT),
            }],
        )
        .expect("expected manifest");
        let written = std::fs::read_to_string(out).expect("read manifest");
        assert_eq!(written, expected);
    }

    #[test]
    fn assemble_without_notes_writes_empty_notes() {
        let dir = artifact_dir();
        let args = assemble_args(dir.path(), &["darwin-aarch64=app.tar.gz"]);
        let out = run_assemble(&args).expect("assemble");
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out).expect("read")).expect("json");
        assert_eq!(written["notes"], "");
        assert_eq!(written["version"], "1.2.3");
    }

    #[test]
    fn assemble_with_no_platforms_is_an_assemble_error_and_writes_nothing() {
        let dir = artifact_dir();
        let args = assemble_args(dir.path(), &[]);
        let error = run_assemble(&args).expect_err("no artifacts");
        assert!(matches!(error, CliError::Assemble(_)));
        assert!(message(&error).starts_with("assemble: "));
        assert!(!args.out.exists());
    }

    #[test]
    fn assemble_reports_the_missing_file_by_path() {
        let dir = artifact_dir();
        let args = assemble_args(dir.path(), &["darwin-aarch64=missing.tar.gz"]);
        let error = run_assemble(&args).expect_err("missing artifact");
        let missing = dir.path().join("missing.tar.gz");
        assert!(matches!(&error, CliError::File { path, .. } if *path == missing));
        assert!(message(&error).starts_with(&format!("{}: ", missing.display())));
    }

    #[test]
    fn assemble_requires_the_detached_signature() {
        let dir = artifact_dir();
        std::fs::remove_file(dir.path().join("app.tar.gz.sig")).expect("remove sig");
        let args = assemble_args(dir.path(), &["darwin-aarch64=app.tar.gz"]);
        let error = run_assemble(&args).expect_err("missing sig");
        let sig = dir.path().join("app.tar.gz.sig");
        assert!(matches!(&error, CliError::File { path, .. } if *path == sig));
    }

    #[test]
    fn unreadable_notes_stop_before_any_artifact_is_read() {
        let dir = artifact_dir();
        let mut args = assemble_args(dir.path(), &["no-equals-sign"]);
        args.notes_file = Some(dir.path().join("absent-notes.txt"));
        let error = run_assemble(&args).expect_err("notes");
        assert!(matches!(error, CliError::Notes(_)));
        assert!(message(&error).starts_with("notes: "));
    }

    #[test]
    fn platform_errors_surface_in_argument_order() {
        let dir = artifact_dir();
        let args = assemble_args(dir.path(), &["darwin-aarch64=missing.tar.gz", "bad"]);
        let error = run_assemble(&args).expect_err("first fault wins");
        assert!(matches!(error, CliError::File { .. }));

        let args = assemble_args(dir.path(), &["bad", "darwin-aarch64=missing.tar.gz"]);
        let error = run_assemble(&args).expect_err("first fault wins");
        assert!(matches!(error, CliError::PlatformMapping(mapping) if mapping == "bad"));
    }

    #[test]
    fn unwritable_output_is_reported_by_path() {
        let dir = artifact_dir();
        let mut args = assemble_args(dir.path(), &["darwin-aarch64=app.tar.gz"]);
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
    fn run_dispatches_both_subcommands_and_rejects_usage() {
        let dir = artifact_dir();
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
            "--platform",
            "darwin-aarch64=app.tar.gz",
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

        let Err(CliError::Usage) = run(&args(&["help"])) else {
            panic!("unknown subcommand must be usage");
        };
    }
}
