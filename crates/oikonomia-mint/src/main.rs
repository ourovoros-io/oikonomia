//! Offline license minting. The Ed25519 secret key lives ONLY on the
//! operator's machine; this binary is never shipped, never in CI.

use ed25519_dalek::{Signer, SigningKey};
use oikonomia_core::license::{LicenseVerifier, install_license, signed_payload};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use zeroize::Zeroizing;

/// Random 32-byte Ed25519 seed as lowercase hex, zeroized on drop.
fn generate_secret_key_hex() -> Zeroizing<String> {
    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    Zeroizing::new(hex_encode(key.as_bytes()))
}

/// Hex verifying key for a hex secret key (what gets baked into the app).
fn verifying_key_hex(secret_hex: &str) -> Result<String, String> {
    let key = signing_key_from_hex(secret_hex)?;
    Ok(hex_encode(key.verifying_key().as_bytes()))
}

/// Build the signed `.lic` JSON for one buyer.
fn mint_license_json(secret_hex: &str, email: &str, expiry: &str) -> Result<String, String> {
    if email.trim().is_empty() || !email.contains('@') {
        return Err("email must be a real address".to_owned());
    }
    let date_format = time::macros::format_description!("[year]-[month]-[day]");
    if expiry.len() != 10 || time::Date::parse(expiry, &date_format).is_err() {
        return Err(format!("expiry must be YYYY-MM-DD, got {expiry}"));
    }

    let issued_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| err.to_string())?;
    let key = signing_key_from_hex(secret_hex)?;
    let payload = signed_payload(oikonomia_core::license::PRODUCT, expiry, email, &issued_at);
    let signature = key.sign(payload.as_bytes());

    let json = serde_json::json!({
        "v": 1,
        "product": oikonomia_core::license::PRODUCT,
        "expiry": expiry,
        "email": email,
        "issued_at": issued_at,
        "sig": hex_encode(&signature.to_bytes()),
    });
    serde_json::to_string_pretty(&json).map_err(|err| err.to_string())
}

/// Ed25519 signing key from a 64-hex-character secret seed. The decoded
/// seed bytes are held in a zeroizing buffer and wiped as soon as the
/// `SigningKey` is constructed from them.
fn signing_key_from_hex(secret_hex: &str) -> Result<SigningKey, String> {
    let seed = Zeroizing::new(decode_hex32(secret_hex, "secret key")?);
    Ok(SigningKey::from_bytes(&seed))
}

/// Parse exactly 32 bytes of hex. `what` names the field in error messages.
fn decode_hex32(hex: &str, what: &str) -> Result<[u8; 32], String> {
    let trimmed = hex.trim();
    if trimmed.len() != 64 {
        return Err(format!("{what} must be 64 hex characters"));
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&trimmed[i * 2..i * 2 + 2], 16)
            .map_err(|_| format!("{what} is not hex"))?;
    }
    Ok(out)
}

/// Lowercase hex encoding of `bytes`.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

const USAGE: &str = "usage:
  oikonomia-mint keygen --out <secret-key-file>
  oikonomia-mint issue --key <secret-key-file> --email <buyer-email> --expiry <YYYY-MM-DD> --out <license.lic>
  oikonomia-mint verify --lic <license.lic> [--pubkey-hex <64-hex>]";

/// Value that follows flag `name` in `args`, if present.
fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") => run_keygen(&args[1..]),
        Some("issue") => run_issue(&args[1..]),
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

/// Generate a fresh keypair; write the secret to `--out` (0600 from the
/// first instant on unix; refuses to overwrite an existing file on any
/// platform) and print the public key hex as the last stdout line, for
/// scripted capture.
fn run_keygen(args: &[String]) -> Result<(), String> {
    let out = PathBuf::from(flag_value(args, "--out").ok_or(USAGE)?);
    let secret_hex = generate_secret_key_hex();
    let public_hex = verifying_key_hex(&secret_hex)?;

    let mut file = open_secret_key_file(&out)?;
    file.write_all(secret_hex.as_bytes())
        .map_err(|err| format!("{}: {err}", out.display()))?;

    writeln!(
        std::io::stdout(),
        "wrote secret key to {} -- bake this into license.rs PRODUCTION_PUBLIC_KEY_HEX:",
        out.display()
    )
    .map_err(|err| err.to_string())?;
    writeln!(std::io::stdout(), "{public_hex}").map_err(|err| err.to_string())
}

/// Create the secret key file with no window where it is readable beyond
/// the owner: on unix the mode is set in the same `open` syscall that
/// creates the file (not a `write` followed by a separate `chmod`, which
/// briefly leaves the file at the process umask, e.g. 0644). Refuses to
/// overwrite an existing file on every platform, so a re-run never
/// silently clobbers a production signing key.
fn open_secret_key_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            format!(
                "{} already exists; move the existing key aside before running keygen again",
                path.display()
            )
        } else {
            format!("{}: {err}", path.display())
        }
    })
}

/// Mint a license for one buyer from a secret key file and write it out.
fn run_issue(args: &[String]) -> Result<(), String> {
    let key_path = flag_value(args, "--key").ok_or(USAGE)?;
    let email = flag_value(args, "--email").ok_or(USAGE)?;
    let expiry = flag_value(args, "--expiry").ok_or(USAGE)?;
    let out = PathBuf::from(flag_value(args, "--out").ok_or(USAGE)?);

    let secret_hex = Zeroizing::new(
        std::fs::read_to_string(&key_path).map_err(|err| format!("{key_path}: {err}"))?,
    );
    let json = mint_license_json(secret_hex.trim(), &email, &expiry)?;
    std::fs::write(&out, json).map_err(|err| format!("{}: {err}", out.display()))?;
    writeln!(std::io::stdout(), "wrote {}", out.display()).map_err(|err| err.to_string())
}

/// Verify a `.lic` file against a public key (default: the baked production
/// key), the same way the app installs one, via a scratch data directory.
fn run_verify(args: &[String]) -> Result<(), String> {
    let lic = PathBuf::from(flag_value(args, "--lic").ok_or(USAGE)?);
    let pubkey_hex = flag_value(args, "--pubkey-hex")
        .unwrap_or_else(|| oikonomia_core::license::PRODUCTION_PUBLIC_KEY_HEX.to_owned());
    let pubkey = decode_hex32(&pubkey_hex, "public key")?;
    let verifier =
        LicenseVerifier::from_public_key_bytes(&pubkey).map_err(|err| err.to_string())?;

    let scratch = ScratchDir::new()?;
    let status = install_license(&scratch.path, &lic, &verifier).map_err(|err| err.to_string())?;
    writeln!(
        std::io::stdout(),
        "license ok: state={:?} licensed_until={}",
        status.state,
        status.licensed_until.as_deref().unwrap_or("-")
    )
    .map_err(|err| err.to_string())
}

/// A directory under the OS temp root, removed on drop. `install_license`
/// needs a data directory to copy the verified license into; this crate
/// keeps no `tempfile` runtime dependency, so it makes its own (unique per
/// process + timestamp, which is enough for a single-shot CLI run).
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new() -> Result<Self, String> {
        let unique = format!(
            "oikonomia-mint-verify-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{generate_secret_key_hex, mint_license_json, run_keygen, verifying_key_hex};
    use oikonomia_core::license::{LicenseState, LicenseVerifier, install_license, license_status};

    fn hex_to_key(hex: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex");
        }
        out
    }

    #[test]
    fn minted_license_installs_and_reports_licensed() {
        let secret_hex = generate_secret_key_hex();
        let public_hex = verifying_key_hex(&secret_hex).expect("pubkey");
        let json = mint_license_json(&secret_hex, "buyer@example.com", "2999-12-31").expect("mint");

        let dir = tempfile::tempdir().expect("tempdir");
        let lic_path = dir.path().join("bought.lic");
        std::fs::write(&lic_path, &json).expect("write");

        let verifier =
            LicenseVerifier::from_public_key_bytes(&hex_to_key(&public_hex)).expect("verifier");
        let status = install_license(dir.path(), &lic_path, &verifier).expect("install");
        assert_eq!(status.state, LicenseState::Licensed);
        assert_eq!(status.licensed_until.as_deref(), Some("2999-12-31"));
        let again = license_status(dir.path(), &verifier).expect("status");
        assert_eq!(again.state, LicenseState::Licensed);
    }

    #[test]
    fn tampered_license_is_rejected() {
        let secret_hex = generate_secret_key_hex();
        let public_hex = verifying_key_hex(&secret_hex).expect("pubkey");
        let json = mint_license_json(&secret_hex, "buyer@example.com", "2999-12-31").expect("mint");
        let tampered = json.replace("buyer@example.com", "thief@example.com");

        let dir = tempfile::tempdir().expect("tempdir");
        let lic_path = dir.path().join("tampered.lic");
        std::fs::write(&lic_path, tampered).expect("write");
        let verifier =
            LicenseVerifier::from_public_key_bytes(&hex_to_key(&public_hex)).expect("verifier");
        assert!(install_license(dir.path(), &lic_path, &verifier).is_err());
    }

    #[test]
    fn expiry_must_be_a_calendar_date() {
        let secret_hex = generate_secret_key_hex();
        assert!(mint_license_json(&secret_hex, "b@example.com", "not-a-date").is_err());
        assert!(mint_license_json(&secret_hex, "", "2999-12-31").is_err());
    }

    #[test]
    fn keygen_writes_0600_from_creation_and_refuses_to_overwrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key_path = dir.path().join("mint.key");
        let args = vec!["--out".to_owned(), key_path.to_string_lossy().into_owned()];

        run_keygen(&args).expect("first keygen");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                mode, 0o600,
                "secret key file must be owner-only from creation"
            );
        }

        let err = run_keygen(&args).expect_err("second keygen onto the same path must refuse");
        assert!(
            err.contains("already exists"),
            "error should tell the operator the key already exists, got: {err}"
        );
    }
}
