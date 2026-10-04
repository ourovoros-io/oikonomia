//! Promote-lane CLI: assemble and verify `latest.json`.
//!
//! `assemble` scans an artifact directory; `verify` checks a manifest and its
//! detached minisign signature with the client's own verifier code;
//! `unpublished` lists the draft assets that must not be published;
//! `checksums` writes the release's `SHA256SUMS` file.

use oikonomia_update::{
    FeedArtifact, WindowsBuild, assemble_manifest, checksum_line, checksummed_assets, feed_entries,
    is_published_asset, parse_public_key,
};
use sha2::{Digest, Sha256};
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("assemble") => run_assemble(&args[1..]),
        Some("verify") => run_verify(&args[1..]),
        Some("unpublished") => run_unpublished(&args[1..]),
        Some("checksums") => run_checksums(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match result {
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

fn windows_build(args: &[String]) -> WindowsBuild {
    if args.iter().any(|arg| arg == WITH_WINDOWS) {
        WindowsBuild::Published
    } else {
        WindowsBuild::Withheld
    }
}

/// File names directly inside `dir`, sorted so the output is reproducible.
fn file_names_in(dir: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }
    names.sort();

    Ok(names)
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn run_assemble(args: &[String]) -> Result<(), String> {
    let version = flag_value(args, "--version").ok_or(USAGE)?;
    let base_url = flag_value(args, "--base-url").ok_or(USAGE)?;
    let dir = PathBuf::from(flag_value(args, "--dir").ok_or(USAGE)?);
    let out = PathBuf::from(flag_value(args, "--out").ok_or(USAGE)?);
    let notes = match flag_value(args, "--notes-file") {
        Some(path) => std::fs::read_to_string(&path).map_err(|e| format!("notes: {e}"))?,
        None => String::new(),
    };

    let names = file_names_in(&dir)?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let entries = feed_entries(&names, windows_build(args)).map_err(|e| e.to_string())?;

    let mut artifacts = Vec::new();
    for (platform, file_name) in entries {
        let file = dir.join(file_name);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let signature_path = dir.join(format!("{file_name}.sig"));
        let signature = std::fs::read_to_string(&signature_path)
            .map_err(|e| format!("{}: {e}", signature_path.display()))?;
        let sha256_hex = sha256_hex(&bytes);
        artifacts.push(FeedArtifact {
            platform: platform.to_owned(),
            file_name: file_name.to_owned(),
            signature: signature.trim().to_owned(),
            sha256_hex,
        });
    }

    let manifest = assemble_manifest(&version, notes.trim(), &base_url, &artifacts)
        .map_err(|e| format!("assemble: {e}"))?;
    std::fs::write(&out, manifest).map_err(|e| format!("{}: {e}", out.display()))?;
    writeln!(std::io::stdout(), "wrote {}", out.display()).map_err(|e| e.to_string())
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();

    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Writes the checksum file for the published files in `--dir`.
fn run_checksums(args: &[String]) -> Result<(), String> {
    let dir = PathBuf::from(flag_value(args, "--dir").ok_or(USAGE)?);
    let out = PathBuf::from(flag_value(args, "--out").ok_or(USAGE)?);

    let names = file_names_in(&dir)?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let listed = checksummed_assets(&names, windows_build(args));
    if listed.is_empty() {
        return Err(format!("{}: no published files to checksum", dir.display()));
    }

    let mut contents = String::new();
    for name in listed {
        let file = dir.join(name);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        contents.push_str(&checksum_line(&sha256_hex(&bytes), name));
    }

    std::fs::write(&out, contents).map_err(|e| format!("{}: {e}", out.display()))?;
    writeln!(std::io::stdout(), "wrote {}", out.display()).map_err(|e| e.to_string())
}

/// Prints, one per line, the given asset names that the release must not keep.
fn run_unpublished(args: &[String]) -> Result<(), String> {
    let windows = windows_build(args);
    let mut stdout = std::io::stdout();

    for name in args.iter().filter(|arg| *arg != WITH_WINDOWS) {
        if !is_published_asset(name, windows) {
            writeln!(stdout, "{name}").map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

fn run_verify(args: &[String]) -> Result<(), String> {
    let manifest = flag_value(args, "--manifest").ok_or(USAGE)?;
    let sig = flag_value(args, "--sig").ok_or(USAGE)?;
    let pubkey = flag_value(args, "--pubkey").ok_or(USAGE)?;

    let body = std::fs::read(&manifest).map_err(|e| format!("{manifest}: {e}"))?;
    let signature = std::fs::read_to_string(&sig).map_err(|e| format!("{sig}: {e}"))?;
    let key = parse_public_key(&pubkey).map_err(|e| format!("pubkey: {e}"))?;
    oikonomia_update::verify_manifest_bytes(&key, &body, &signature)
        .map_err(|e| format!("verify: {e}"))?;
    writeln!(std::io::stdout(), "manifest signature ok").map_err(|e| e.to_string())
}
