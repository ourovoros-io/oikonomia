//! Offline license file + 30-day trial. No network I/O on this path.
//!
//! A `.lic` is UTF-8 JSON signed with Ed25519. The production public key is
//! baked into the binary; tests inject an ephemeral [`LicenseVerifier`].
//! The app never phones home.

use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::error::{Error, Result};
use crate::prefs::{load_ui_prefs, save_ui_prefs};
use crate::util::parse_date;

/// Baked production verifying key (32-byte hex). The matching secret is not
/// in this repository, the app, the vault, CI, or tests.
pub const PRODUCTION_PUBLIC_KEY_HEX: &str =
    "7d5b038e9ab30eef536cc559baac20e44070adedcdf548af48744029804ec671";

/// Product string required inside a `.lic` payload.
pub const PRODUCT: &str = "oikonomia";

/// File name next to `ui-prefs.json` in the app data directory.
pub const LICENSE_FILE_NAME: &str = "license.lic";

/// Length of the trial window after the first successful unlock.
pub const TRIAL_DAYS: i64 = 30;

const PRODUCTION_PUBLIC_KEY: [u8; 32] = decode_hex32(PRODUCTION_PUBLIC_KEY_HEX);

/// License / trial lifecycle shown to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseState {
    /// Vault has never been unlocked; `trial_started_at` is absent.
    None,
    /// Inside the 30-day window after first unlock, and no valid unexpired license.
    Trial,
    /// A signature-valid `.lic` whose expiry is today or later (UTC).
    Licensed,
    /// Trial elapsed, or a signature-valid `.lic` whose expiry is in the past.
    Expired,
}

/// IPC payload for `license_status` / `license_install`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseStatus {
    /// Current lifecycle state.
    pub state: LicenseState,
    /// Whole days left on the trial, or days until license expiry.
    /// Omitted when [`LicenseState::None`] or [`LicenseState::Expired`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_remaining: Option<u32>,
    /// License `expiry` (`YYYY-MM-DD`) when [`LicenseState::Licensed`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub licensed_until: Option<String>,
}

impl LicenseStatus {
    fn none() -> Self {
        Self {
            state: LicenseState::None,
            days_remaining: None,
            licensed_until: None,
        }
    }

    fn expired() -> Self {
        Self {
            state: LicenseState::Expired,
            days_remaining: None,
            licensed_until: None,
        }
    }

    fn trial(days_remaining: u32) -> Self {
        Self {
            state: LicenseState::Trial,
            days_remaining: Some(days_remaining),
            licensed_until: None,
        }
    }

    fn licensed(expiry: String, days_remaining: u32) -> Self {
        Self {
            state: LicenseState::Licensed,
            days_remaining: Some(days_remaining),
            licensed_until: Some(expiry),
        }
    }
}

/// Ed25519 verifier for `.lic` files.
///
/// Production uses [`LicenseVerifier::production`]. Tests construct one from
/// an ephemeral keypair via [`LicenseVerifier::from_public_key_bytes`].
#[derive(Clone)]
pub struct LicenseVerifier {
    key: VerifyingKey,
}

impl LicenseVerifier {
    /// Verifier for the baked production public key.
    ///
    /// # Errors
    ///
    /// [`Error::Crypto`] if the baked bytes are not a valid Edwards point
    /// (a programming error; covered by a unit test).
    pub fn production() -> Result<Self> {
        Self::from_public_key_bytes(&PRODUCTION_PUBLIC_KEY)
    }

    /// Inject a verifying key. Tests sign with the matching ephemeral secret.
    ///
    /// # Errors
    ///
    /// [`Error::Crypto`] when `bytes` is not a valid compressed Edwards-Y point.
    pub fn from_public_key_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let key = VerifyingKey::from_bytes(bytes)
            .map_err(|err| Error::Crypto(format!("license public key: {err}")))?;
        Ok(Self { key })
    }
}

/// Path of `license.lic` inside the app data directory.
#[must_use]
pub fn license_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LICENSE_FILE_NAME)
}

/// Whether mutating ledger / vault-password commands may proceed.
///
/// Only [`LicenseState::Expired`] is blocked. `none` is the pre-first-unlock
/// window (including the session after `vault_init`); `trial` and `licensed`
/// are writable.
#[must_use]
pub fn writes_allowed(status: &LicenseStatus) -> bool {
    !matches!(status.state, LicenseState::Expired)
}

/// [`Error::LicenseExpired`] when [`writes_allowed`] is false.
///
/// # Errors
///
/// [`Error::LicenseExpired`] when the current status is expired;
/// [`Error::Crypto`] if the verifier cannot be used (caller supplies it).
pub fn require_writes_allowed(data_dir: &Path, verifier: &LicenseVerifier) -> Result<()> {
    let status = license_status(data_dir, verifier)?;
    if writes_allowed(&status) {
        Ok(())
    } else {
        Err(Error::LicenseExpired)
    }
}

/// Current license / trial status using the system UTC clock.
///
/// # Errors
///
/// Does not fail on a missing or unreadable `.lic` (that is treated as no
/// license). Prefs load is infallible.
pub fn license_status(data_dir: &Path, verifier: &LicenseVerifier) -> Result<LicenseStatus> {
    Ok(license_status_at(
        data_dir,
        verifier,
        OffsetDateTime::now_utc(),
    ))
}

/// Status at an explicit UTC instant (tests pin `now`).
#[must_use]
pub fn license_status_at(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    now: OffsetDateTime,
) -> LicenseStatus {
    let today = now.date();
    if let Some(file) = load_verified_license(data_dir, verifier) {
        if file.expiry >= today {
            let days = u32::try_from((file.expiry - today).whole_days()).unwrap_or(0);
            return LicenseStatus::licensed(crate::util::format_date(file.expiry), days);
        }
        return LicenseStatus::expired();
    }

    let prefs = load_ui_prefs(data_dir);
    let Some(raw) = prefs.trial_started_at.as_deref() else {
        return LicenseStatus::none();
    };
    let Some(start) = OffsetDateTime::parse(raw, &Rfc3339).ok() else {
        return LicenseStatus::none();
    };
    let Some(end) = start.checked_add(Duration::days(TRIAL_DAYS)) else {
        return LicenseStatus::expired();
    };
    if now < end {
        let days = u32::try_from((end.date() - today).whole_days()).unwrap_or(0);
        LicenseStatus::trial(days)
    } else {
        LicenseStatus::expired()
    }
}

/// Verify `source` and atomically copy the original bytes to `license.lic`.
///
/// The stored file is the signed `.lic`, not an unsigned cache. An
/// expired-but-valid-signature file is kept; status becomes
/// [`LicenseState::Expired`].
///
/// # Errors
///
/// [`Error::LicenseInvalid`] for unreadable JSON, wrong product, missing or
/// bad signature. [`Error::Io`] if the verified copy cannot be written.
pub fn install_license(
    data_dir: &Path,
    source: &Path,
    verifier: &LicenseVerifier,
) -> Result<LicenseStatus> {
    let bytes = fs::read(source).map_err(|_| Error::LicenseInvalid)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::LicenseInvalid)?;
    let _verified = verify_license_text(text, verifier)?;

    fs::create_dir_all(data_dir)
        .map_err(|err| Error::Io(format!("could not create data directory: {err}")))?;

    let dest = license_path(data_dir);
    let tmp = data_dir.join("license.lic.tmp");
    fs::write(&tmp, &bytes).map_err(|err| Error::Io(format!("could not stage license: {err}")))?;
    if let Err(err) = fs::rename(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(Error::Io(format!("could not install license: {err}")));
    }

    license_status(data_dir, verifier)
}

/// Set `trial_started_at` once on first successful unlock. Never resets.
///
/// Uses the system clock. No NTP, no network.
///
/// # Errors
///
/// [`Error::Io`] when prefs cannot be written.
pub fn record_trial_start(data_dir: &Path) -> Result<()> {
    let mut prefs = load_ui_prefs(data_dir);
    if prefs.trial_started_at.is_some() {
        return Ok(());
    }
    prefs.trial_started_at = Some(rfc3339_now());
    save_ui_prefs(data_dir, &prefs)
}

/// Canonical bytes that a `.lic` signature covers.
#[must_use]
pub fn signed_payload(product: &str, expiry: &str, email: &str, issued_at: &str) -> String {
    format!("v1\n{product}\n{expiry}\n{email}\n{issued_at}\n")
}

#[derive(Debug, Deserialize)]
struct LicenseJson {
    v: u32,
    product: String,
    expiry: String,
    email: String,
    issued_at: String,
    sig: String,
}

struct VerifiedLicense {
    expiry: time::Date,
}

fn load_verified_license(data_dir: &Path, verifier: &LicenseVerifier) -> Option<VerifiedLicense> {
    let bytes = fs::read(license_path(data_dir)).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    verify_license_text(text, verifier).ok()
}

fn verify_license_text(text: &str, verifier: &LicenseVerifier) -> Result<VerifiedLicense> {
    let parsed: LicenseJson = serde_json::from_str(text).map_err(|_| Error::LicenseInvalid)?;
    if parsed.v != 1 {
        return Err(Error::LicenseInvalid);
    }
    if parsed.product != PRODUCT {
        return Err(Error::LicenseInvalid);
    }
    let expiry = parse_date(&parsed.expiry).map_err(|_| Error::LicenseInvalid)?;
    if parsed.email.is_empty() || parsed.issued_at.is_empty() {
        return Err(Error::LicenseInvalid);
    }

    let sig_bytes = decode_hex64(parsed.sig.trim())?;
    let signature = Signature::from_bytes(&sig_bytes);
    let payload = signed_payload(
        &parsed.product,
        &parsed.expiry,
        &parsed.email,
        &parsed.issued_at,
    );
    verifier
        .key
        .verify(payload.as_bytes(), &signature)
        .map_err(|_| Error::LicenseInvalid)?;

    Ok(VerifiedLicense { expiry })
}

fn rfc3339_now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

const fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

const fn decode_hex32(s: &str) -> [u8; 32] {
    let bytes = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (hex_nibble(bytes[i * 2]) << 4) | hex_nibble(bytes[i * 2 + 1]);
        i += 1;
    }
    out
}

fn decode_hex64(s: &str) -> Result<[u8; 64]> {
    if s.len() != 128 {
        return Err(Error::LicenseInvalid);
    }
    let bytes = s.as_bytes();
    let mut out = [0u8; 64];
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = hex_nibble_checked(bytes[i * 2])?;
        let lo = hex_nibble_checked(bytes[i * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble_checked(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Error::LicenseInvalid),
    }
}

#[cfg(test)]
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::RngCore;
    use serde_json::json;
    use tempfile::tempdir;

    struct Ephemeral {
        verifier: LicenseVerifier,
        signing: SigningKey,
    }

    fn ephemeral() -> Ephemeral {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let signing = SigningKey::from_bytes(&seed);
        let verifier = LicenseVerifier::from_public_key_bytes(&signing.verifying_key().to_bytes())
            .expect("ephemeral verifying key");
        Ephemeral { verifier, signing }
    }

    fn sign_lic(
        signing: &SigningKey,
        product: &str,
        expiry: &str,
        email: &str,
        issued_at: &str,
    ) -> String {
        let payload = signed_payload(product, expiry, email, issued_at);
        let sig = signing.sign(payload.as_bytes());
        json!({
            "v": 1,
            "product": product,
            "expiry": expiry,
            "email": email,
            "issued_at": issued_at,
            "sig": encode_hex(&sig.to_bytes()),
        })
        .to_string()
    }

    fn write_lic(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("incoming.lic");
        fs::write(&path, body).expect("write incoming");
        path
    }

    #[test]
    fn production_verifier_constructs() {
        assert!(LicenseVerifier::production().is_ok());
        assert_eq!(PRODUCTION_PUBLIC_KEY_HEX.len(), 64);
    }

    #[test]
    fn signed_payload_is_exact_v1_format() {
        assert_eq!(
            signed_payload(
                "oikonomia",
                "2027-12-31",
                "buyer@example.com",
                "2026-08-20T12:00:00Z",
            ),
            "v1\noikonomia\n2027-12-31\nbuyer@example.com\n2026-08-20T12:00:00Z\n"
        );
    }

    #[test]
    fn license_module_source_has_no_network_crates() {
        let src = include_str!("license.rs");
        let product = src.split("#[cfg(test)]").next().unwrap_or(src);
        for needle in ["reqwest", "ureq", "ntp", "tokio::net", "hyper"] {
            assert!(
                !product.contains(needle),
                "license module must not reference {needle}"
            );
        }
    }

    #[test]
    fn workspace_pins_ed25519_dalek_v2() {
        let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"));
        let after = manifest.split("ed25519-dalek = ").nth(1);
        assert!(
            after.is_some(),
            "ed25519-dalek missing from workspace Cargo.toml"
        );
        let version = after
            .unwrap_or("")
            .trim_start_matches('"')
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
            .unwrap_or("");
        assert!(
            version.starts_with("2."),
            "ed25519-dalek must be 2.x (RUSTSEC-2022-0093), got {version:?}"
        );
    }

    #[test]
    fn missing_prefs_and_license_is_none() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let status = license_status(dir.path(), &keys.verifier).expect("status");
        assert_eq!(status.state, LicenseState::None);
        assert_eq!(status.days_remaining, None);
        assert!(writes_allowed(&status));
    }

    #[test]
    fn valid_lic_is_licensed() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let body = sign_lic(
            &keys.signing,
            PRODUCT,
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        let src = write_lic(dir.path(), &body);
        let status = install_license(dir.path(), &src, &keys.verifier).expect("install");
        assert_eq!(status.state, LicenseState::Licensed);
        assert_eq!(status.licensed_until.as_deref(), Some("2099-12-31"));
        assert!(status.days_remaining.is_some());
        assert!(writes_allowed(&status));
        let stored = fs::read_to_string(license_path(dir.path())).expect("stored");
        assert_eq!(stored, body, "installed file is the signed original");
    }

    #[test]
    fn bad_sig_wrong_product_junk_leave_previous_license() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let good = sign_lic(
            &keys.signing,
            PRODUCT,
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        let good_src = write_lic(dir.path(), &good);
        install_license(dir.path(), &good_src, &keys.verifier).expect("install good");

        let junk = dir.path().join("junk.lic");
        fs::write(&junk, "not json").expect("junk");
        assert_eq!(
            install_license(dir.path(), &junk, &keys.verifier),
            Err(Error::LicenseInvalid)
        );

        let mut parsed: LicenseJson = serde_json::from_str(&good).expect("parse good");
        let mut sig: Vec<u8> = decode_hex64(&parsed.sig).expect("sig bytes").to_vec();
        sig[0] ^= 0xff;
        parsed.sig = encode_hex(&sig);
        let bad_sig = serde_json::to_string(&json!({
            "v": parsed.v,
            "product": parsed.product,
            "expiry": parsed.expiry,
            "email": parsed.email,
            "issued_at": parsed.issued_at,
            "sig": parsed.sig,
        }))
        .expect("bad sig json");
        let bad_dir = dir.path().join("bad");
        fs::create_dir_all(&bad_dir).expect("bad dir");
        let bad_src = write_lic(&bad_dir, &bad_sig);
        assert_eq!(
            install_license(dir.path(), &bad_src, &keys.verifier),
            Err(Error::LicenseInvalid)
        );

        let wrong = sign_lic(
            &keys.signing,
            "other-product",
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        let wrong_dir = dir.path().join("wrong");
        fs::create_dir_all(&wrong_dir).expect("wrong dir");
        let wrong_src = write_lic(&wrong_dir, &wrong);
        assert_eq!(
            install_license(dir.path(), &wrong_src, &keys.verifier),
            Err(Error::LicenseInvalid)
        );

        let status = license_status(dir.path(), &keys.verifier).expect("status");
        assert_eq!(status.state, LicenseState::Licensed);
        assert_eq!(
            fs::read_to_string(license_path(dir.path())).expect("stored"),
            good
        );
    }

    #[test]
    fn expired_valid_signature_lic_is_expired_and_kept() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let body = sign_lic(
            &keys.signing,
            PRODUCT,
            "2020-01-01",
            "buyer@example.com",
            "2020-01-01T00:00:00Z",
        );
        let src = write_lic(dir.path(), &body);
        let status = install_license(dir.path(), &src, &keys.verifier).expect("install");
        assert_eq!(status.state, LicenseState::Expired);
        assert_eq!(status.days_remaining, None);
        assert!(!writes_allowed(&status));
        assert_eq!(
            require_writes_allowed(dir.path(), &keys.verifier),
            Err(Error::LicenseExpired)
        );
        assert_eq!(
            fs::read_to_string(license_path(dir.path())).expect("kept"),
            body
        );
    }

    #[test]
    fn trial_inside_30_days_is_writable() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        record_trial_start(dir.path()).expect("start");
        let status = license_status(dir.path(), &keys.verifier).expect("status");
        assert_eq!(status.state, LicenseState::Trial);
        assert!(writes_allowed(&status));
        assert!(require_writes_allowed(dir.path(), &keys.verifier).is_ok());
        assert!(status.days_remaining.is_some());
    }

    #[test]
    fn trial_started_at_set_once() {
        let dir = tempdir().expect("tempdir");
        record_trial_start(dir.path()).expect("first");
        let first = load_ui_prefs(dir.path()).trial_started_at;
        assert!(first.is_some());
        std::thread::sleep(std::time::Duration::from_millis(15));
        record_trial_start(dir.path()).expect("second");
        assert_eq!(load_ui_prefs(dir.path()).trial_started_at, first);
    }

    #[test]
    fn old_prefs_without_trial_are_none() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        fs::write(
            crate::prefs::ui_prefs_path(dir.path()),
            r#"{ "theme": "dark" }"#,
        )
        .expect("old prefs");
        let status = license_status(dir.path(), &keys.verifier).expect("status");
        assert_eq!(status.state, LicenseState::None);
        assert_eq!(load_ui_prefs(dir.path()).trial_started_at, None);
    }

    #[test]
    fn trial_past_30_days_is_expired() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let mut prefs = load_ui_prefs(dir.path());
        prefs.trial_started_at = Some("2020-01-01T00:00:00Z".into());
        save_ui_prefs(dir.path(), &prefs).expect("save");
        let status = license_status(dir.path(), &keys.verifier).expect("status");
        assert_eq!(status.state, LicenseState::Expired);
        assert!(!writes_allowed(&status));
    }

    #[test]
    fn expired_license_wins_over_active_trial() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        record_trial_start(dir.path()).expect("trial");
        let body = sign_lic(
            &keys.signing,
            PRODUCT,
            "2020-06-01",
            "buyer@example.com",
            "2020-01-01T00:00:00Z",
        );
        let src = write_lic(dir.path(), &body);
        let status = install_license(dir.path(), &src, &keys.verifier).expect("install");
        assert_eq!(status.state, LicenseState::Expired);
        assert!(!writes_allowed(&status));
    }
}
