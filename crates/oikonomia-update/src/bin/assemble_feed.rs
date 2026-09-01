//! Promote-lane CLI: assemble and verify `latest.json`.
//!
//! `assemble` scans an artifact directory; `verify` checks a manifest and its
//! detached minisign signature with the client's own verifier code.

use oikonomia_update::{FeedArtifact, assemble_manifest, parse_public_key};
use sha2::{Digest, Sha256};
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("assemble") => run_assemble(&args[1..]),
        Some("verify") => run_verify(&args[1..]),
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
    --out <latest.json> [--notes-file <path>] --platform <key>=<file> [--platform ...]
  assemble_feed verify --manifest <latest.json> --sig <latest.json.sig> --pubkey <minisign-pubkey>";

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
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

fn run_assemble(args: &[String]) -> Result<(), String> {
    let version = flag_value(args, "--version").ok_or(USAGE)?;
    let base_url = flag_value(args, "--base-url").ok_or(USAGE)?;
    let dir = PathBuf::from(flag_value(args, "--dir").ok_or(USAGE)?);
    let out = PathBuf::from(flag_value(args, "--out").ok_or(USAGE)?);
    let notes = match flag_value(args, "--notes-file") {
        Some(path) => std::fs::read_to_string(&path).map_err(|e| format!("notes: {e}"))?,
        None => String::new(),
    };

    let mut artifacts = Vec::new();
    for mapping in flag_values(args, "--platform") {
        let (platform, file_name) = mapping
            .split_once('=')
            .ok_or_else(|| format!("bad --platform mapping: {mapping}"))?;
        let file = dir.join(file_name);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let signature_path = dir.join(format!("{file_name}.sig"));
        let signature = std::fs::read_to_string(&signature_path)
            .map_err(|e| format!("{}: {e}", signature_path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest = hasher.finalize();
        let mut sha256_hex = String::with_capacity(64);
        for byte in digest {
            let _ = write!(sha256_hex, "{byte:02x}");
        }
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
