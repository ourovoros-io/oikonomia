//! Promote-lane CLI: assemble and verify `latest.json`.
//!
//! `assemble` scans an artifact directory; `verify` checks a manifest and its
//! detached minisign signature with the client's own verifier code;
//! `verify-feed` checks every artifact a feed names against its hash and
//! its minisign signature, as the app does before installing;
//! `unpublished` lists the draft assets that must not be published;
//! `fixed-copies` writes the version-free copies the website links to;
//! `fixed-names` prints the version-free names a release must carry;
//! `checksums` writes the release's `SHA256SUMS` file.

use oikonomia_update::{
    FeedArtifact, WindowsBuild, assemble_manifest, checksum_line, checksummed_assets, feed_entries,
    feed_platform_keys, fixed_name_copies, fixed_names, is_published_asset, parse_public_key,
    verify_minisign,
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

/// Runs the subcommand named by the first argument.
fn run(args: &[String]) -> Result<(), String> {
    let Some((command, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };

    match command.as_str() {
        "assemble" => run_assemble(rest),
        "verify" => run_verify(rest),
        "verify-feed" => run_verify_feed(rest),
        "unpublished" => run_unpublished(rest),
        "checksums" => run_checksums(rest),
        "fixed-copies" => run_fixed_copies(rest),
        "fixed-names" => run_fixed_names(rest),
        _ => Err(USAGE.to_owned()),
    }
}

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

/// Copies each versioned download in `--dir` to its version-free name and
/// prints the fixed names, one per line. Fails when any source is missing or
/// ambiguous, so a release never publishes without every site link.
fn run_fixed_copies(args: &[String]) -> Result<(), String> {
    let dir = PathBuf::from(flag_value(args, "--dir").ok_or(USAGE)?);

    let names = file_names_in(&dir)?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let copies = fixed_name_copies(&names, windows_build(args)).map_err(|e| e.to_string())?;

    let mut stdout = std::io::stdout();
    for (source, fixed) in copies {
        let from = dir.join(source);
        let to = dir.join(fixed);
        std::fs::copy(&from, &to)
            .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
        writeln!(stdout, "{fixed}").map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Prints, one per line, the version-free names the release must carry. The
/// workflow compares what it made and what the draft holds against this list,
/// so the names live in `release_set` only.
fn run_fixed_names(args: &[String]) -> Result<(), String> {
    let mut stdout = std::io::stdout();
    for name in fixed_names(windows_build(args)) {
        writeln!(stdout, "{name}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Prints, one per line, the given asset names that the release must not keep.
fn run_unpublished(args: &[String]) -> Result<(), String> {
    let mut stdout = std::io::stdout();

    for name in unpublished_assets(args) {
        writeln!(stdout, "{name}").map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// The asset names among `args` that the release must not keep.
fn unpublished_assets(args: &[String]) -> Vec<&str> {
    let windows = windows_build(args);

    args.iter()
        .map(String::as_str)
        .filter(|arg| *arg != WITH_WINDOWS)
        .filter(|name| !is_published_asset(name, windows))
        .collect()
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

/// Checks a feed against the files in `--dir` the way the app will: the feed
/// holds exactly the platforms this release publishes, and every file it
/// names hashes to its `sha256` and carries a minisign signature that
/// verifies with `--pubkey`, the key baked into the app. Run before anything
/// is public, so a feed whose artifact the app would refuse never ships.
fn run_verify_feed(args: &[String]) -> Result<(), String> {
    let manifest = PathBuf::from(flag_value(args, "--manifest").ok_or(USAGE)?);
    let dir = PathBuf::from(flag_value(args, "--dir").ok_or(USAGE)?);
    let pubkey = flag_value(args, "--pubkey").ok_or(USAGE)?;
    let key = parse_public_key(&pubkey).map_err(|e| format!("pubkey: {e}"))?;

    let body = std::fs::read(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let feed: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let platforms = feed
        .get("platforms")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("{}: no platforms object", manifest.display()))?;

    let expected = feed_platform_keys(windows_build(args));
    let mut found: Vec<&str> = platforms.keys().map(String::as_str).collect();
    found.sort_unstable();
    let mut wanted = expected.clone();
    wanted.sort_unstable();
    if found != wanted {
        return Err(format!(
            "feed platforms are {found:?}, this release publishes {wanted:?}"
        ));
    }

    let mut stdout = std::io::stdout();
    for platform in expected {
        let entry = platforms
            .get(platform)
            .ok_or_else(|| format!("{platform}: missing from the feed"))?;
        let field = |name: &str| {
            entry
                .get(name)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("{platform}: no {name}"))
        };
        let url = field("url")?;
        let signature = field("signature")?;
        let sha256 = field("sha256")?;

        let file_name = url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && *name != ".." && !name.contains('\\'))
            .ok_or_else(|| format!("{platform}: no file name in {url}"))?;
        let file = dir.join(file_name);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;

        if !sha256_hex(&bytes).eq_ignore_ascii_case(sha256) {
            return Err(format!(
                "{platform}: {file_name} does not match the sha256 in the feed"
            ));
        }
        verify_minisign(&key, &bytes, signature).map_err(|_| {
            format!("{platform}: the signature of {file_name} does not verify with the public key")
        })?;
        writeln!(stdout, "{platform}: {file_name} hash and signature ok")
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{USAGE, run, sha256_hex, unpublished_assets};
    use base64::Engine;
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
            vec!["verify-feed", "--manifest", "latest.json", "--dir", "."],
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
    fn fixed_copies_match_their_sources_and_are_checksummed() {
        let draft = draft();
        let dir = text(draft.path());

        run(&args(&["fixed-copies", "--dir", dir])).expect("fixed copies");

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
        run(&args(&["fixed-copies", "--dir", dir, "--with-windows"])).expect("rerun");
        assert_eq!(
            std::fs::read(draft.path().join("Oikonomia-windows-x64-setup.exe")).expect("copy"),
            std::fs::read(draft.path().join(SETUP)).expect("source")
        );

        let out = draft.path().join("SHA256SUMS");
        run(&args(&["checksums", "--dir", dir, "--out", text(&out)])).expect("checksums");
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

        let err = run(&args(&["fixed-copies", "--dir", text(draft.path())])).expect_err("no deb");

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

    #[test]
    fn fixed_names_runs_with_and_without_windows() {
        assert_eq!(run(&args(&["fixed-names"])), Ok(()));
        assert_eq!(run(&args(&["fixed-names", "--with-windows"])), Ok(()));
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
        run(&args(&list))
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

        let err = verify_feed(draft.path(), &feed, "not a key", &[]).expect_err("bad key");
        assert!(err.starts_with("pubkey:"), "{err}");
    }
}
