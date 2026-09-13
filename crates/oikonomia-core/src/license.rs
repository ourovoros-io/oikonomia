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
    "54ef40437de3579cfd3f94abf5760d9ef3bb53b1e57aa9c02126ae0b1d654133";

/// Product string required inside a `.lic` payload.
pub const PRODUCT: &str = "oikonomia";

/// File name next to `ui-prefs.json` in the app data directory.
pub const LICENSE_FILE_NAME: &str = "license.lic";

/// Length of the trial window after `trial_started_at`.
pub const TRIAL_DAYS: i64 = 30;

const PRODUCTION_PUBLIC_KEY: [u8; 32] = decode_hex32(PRODUCTION_PUBLIC_KEY_HEX);

/// License / trial lifecycle shown to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseState {
    /// `trial_started_at` is absent.
    None,
    /// Inside the 30-day window after the trial stamp, and no valid unexpired license.
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
    /// No license file and no recorded trial.
    fn none() -> Self {
        Self {
            state: LicenseState::None,
            days_remaining: None,
            licensed_until: None,
        }
    }

    /// License or trial is past its end date.
    fn expired() -> Self {
        Self {
            state: LicenseState::Expired,
            days_remaining: None,
            licensed_until: None,
        }
    }

    /// Active trial with `days_remaining` until expiry.
    fn trial(days_remaining: u32) -> Self {
        Self {
            state: LicenseState::Trial,
            days_remaining: Some(days_remaining),
            licensed_until: None,
        }
    }

    /// Verified paid license valid through `expiry` (`YYYY-MM-DD`).
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

/// Secondary, best-effort location for the trial-start stamp.
///
/// The primary stamp lives in `ui-prefs.json`; a platform can additionally
/// mirror it somewhere that survives deleting the app-data directory (on
/// macOS, the login Keychain). Reading is used to recover the earliest known
/// stamp; writing is best-effort and must never fail the caller, so
/// implementations swallow their own errors (logging is appropriate).
pub trait TrialStampStore {
    /// The previously stored stamp (RFC3339, UTC), if any.
    fn read_stamp(&self) -> Option<String>;

    /// Best-effort write of `rfc3339`. Implementations must not panic and
    /// must not propagate failure; a write that cannot be performed (for
    /// example, an unavailable Keychain) is silently dropped.
    fn write_stamp(&self, rfc3339: &str);
}

/// A [`TrialStampStore`] with no secondary location: reads are always
/// absent, writes are always no-ops.
///
/// Used by every caller that has not opted into a secondary stamp (all
/// pre-Task-16 call sites, and non-macOS desktop builds), so the trial
/// behaves exactly as it did before the secondary store existed.
pub struct NoTrialStampStore;

impl TrialStampStore for NoTrialStampStore {
    fn read_stamp(&self) -> Option<String> {
        None
    }

    fn write_stamp(&self, _rfc3339: &str) {}
}

/// Whether mutating ledger / vault-password commands may proceed.
///
/// Only [`LicenseState::Expired`] is blocked. `none` (trial not yet stamped),
/// `trial`, and `licensed` are writable.
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
    require_writes_allowed_with(data_dir, verifier, &NoTrialStampStore)
}

/// Same as [`require_writes_allowed`], additionally consulting `store` (the
/// secondary trial stamp) so a deleted app-data directory alone cannot
/// resurrect writes that a still-expired trial should block.
///
/// # Errors
///
/// [`Error::LicenseExpired`] when the current status is expired;
/// [`Error::Crypto`] if the verifier cannot be used (caller supplies it).
pub fn require_writes_allowed_with(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    store: &dyn TrialStampStore,
) -> Result<()> {
    let status = license_status_with(data_dir, verifier, store)?;
    if writes_allowed(&status) {
        Ok(())
    } else {
        Err(Error::LicenseExpired)
    }
}

/// Run `write` only when [`writes_allowed`]. Document attach/delete use this
/// so expired status returns [`Error::LicenseExpired`] before mutating.
///
/// # Errors
///
/// [`Error::LicenseExpired`], or whatever `write` returns.
pub fn when_writes_allowed<T>(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    write: impl FnOnce() -> Result<T>,
) -> Result<T> {
    require_writes_allowed(data_dir, verifier)?;
    write()
}

/// Whether a new entity may be created under the current license.
///
/// Unlicensed states (`none` / `trial` / `expired`) may have **one** entity
/// per vault. [`LicenseState::Licensed`] may create more. Expired writes are
/// still [`Error::LicenseExpired`], not [`Error::LicenseEntityLimit`].
///
/// # Errors
///
/// [`Error::LicenseExpired`] when writes are blocked; [`Error::LicenseEntityLimit`]
/// when an unlicensed vault already has an entity.
pub fn require_entity_create_allowed(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    existing_entity_count: u64,
) -> Result<()> {
    require_writes_allowed(data_dir, verifier)?;
    let status = license_status(data_dir, verifier)?;
    if status.state == LicenseState::Licensed || existing_entity_count == 0 {
        Ok(())
    } else {
        Err(Error::LicenseEntityLimit)
    }
}

/// Current license / trial status using the system UTC clock.
///
/// # Errors
///
/// Does not fail on a missing or unreadable `.lic` (that is treated as no
/// license). Prefs load is infallible.
pub fn license_status(data_dir: &Path, verifier: &LicenseVerifier) -> Result<LicenseStatus> {
    license_status_with(data_dir, verifier, &NoTrialStampStore)
}

/// Same as [`license_status`], additionally consulting `store` (the
/// secondary trial stamp) when no valid license file is installed.
///
/// # Errors
///
/// Does not fail on a missing or unreadable `.lic` (that is treated as no
/// license). Prefs load is infallible.
pub fn license_status_with(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    store: &dyn TrialStampStore,
) -> Result<LicenseStatus> {
    Ok(license_status_at_with(
        data_dir,
        verifier,
        OffsetDateTime::now_utc(),
        store,
    ))
}

/// Status at an explicit UTC instant (tests pin `now`).
#[must_use]
pub fn license_status_at(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    now: OffsetDateTime,
) -> LicenseStatus {
    license_status_at_with(data_dir, verifier, now, &NoTrialStampStore)
}

/// Same as [`license_status_at`], additionally consulting `store` (the
/// secondary trial stamp).
///
/// The trial branch uses the *earliest* of the prefs stamp and the store
/// stamp: whichever the two records the older date wins, so a still-running
/// trial cannot be reset by deleting one of the two locations. A stamp that
/// is present in either location but fails to parse as RFC3339 is treated as
/// expired, matching the historical single-stamp behavior for corrupt data.
#[must_use]
pub fn license_status_at_with(
    data_dir: &Path,
    verifier: &LicenseVerifier,
    now: OffsetDateTime,
    store: &dyn TrialStampStore,
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
    let store_raw = store.read_stamp();
    let start = match earliest_trial_start(prefs.trial_started_at.as_deref(), store_raw.as_deref())
    {
        TrialStartLookup::NotStarted => return LicenseStatus::none(),
        TrialStartLookup::Corrupt => return LicenseStatus::expired(),
        TrialStartLookup::Started(start) => start,
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

/// Outcome of resolving the trial-start instant from the primary (prefs)
/// and secondary (store) raw stamps. See [`earliest_trial_start`].
enum TrialStartLookup {
    /// Neither location has a stamp: no trial recorded yet.
    NotStarted,
    /// A stamp is present in at least one location but fails to parse as
    /// RFC3339 (matches the historical single-stamp "corrupt = expired"
    /// behavior).
    Corrupt,
    /// The earliest of the stamps that did parse.
    Started(OffsetDateTime),
}

/// Earliest parsed trial-start instant across the primary (`prefs`) and
/// secondary (`store`) raw stamps. See [`TrialStartLookup`] for the three
/// possible outcomes.
fn earliest_trial_start(prefs: Option<&str>, store: Option<&str>) -> TrialStartLookup {
    if prefs.is_none() && store.is_none() {
        return TrialStartLookup::NotStarted;
    }
    let mut earliest: Option<OffsetDateTime> = None;
    for raw in [prefs, store].into_iter().flatten() {
        match parse_rfc3339(raw) {
            Some(parsed) => earliest = Some(earliest.map_or(parsed, |current| current.min(parsed))),
            None => return TrialStartLookup::Corrupt,
        }
    }
    earliest.map_or(TrialStartLookup::NotStarted, TrialStartLookup::Started)
}

/// Parse `raw` as an RFC3339 timestamp, or `None` if it is not one.
fn parse_rfc3339(raw: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(raw, &Rfc3339).ok()
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
    if let Err(err) = replace_license_file(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    license_status(data_dir, verifier)
}

/// Rename `tmp` onto `dest`. Windows `rename` fails when `dest` exists, so
/// drop `dest` first there; Unix rename still replaces atomically.
fn replace_license_file(tmp: &Path, dest: &Path) -> Result<()> {
    #[cfg(windows)]
    if dest.exists() {
        fs::remove_file(dest)
            .map_err(|err| Error::Io(format!("could not replace license: {err}")))?;
    }
    fs::rename(tmp, dest).map_err(|err| Error::Io(format!("could not install license: {err}")))
}

/// Set `trial_started_at` once. Never resets an existing stamp.
///
/// Uses the system clock. No NTP, no network.
///
/// # Errors
///
/// [`Error::Io`] when prefs cannot be written.
pub fn record_trial_start(data_dir: &Path) -> Result<()> {
    record_trial_start_with(data_dir, &NoTrialStampStore)
}

/// Same as [`record_trial_start`], additionally reading and writing a
/// secondary stamp via `store`.
///
/// The canonical stamp is the earliest parseable RFC3339 timestamp between
/// the prefs stamp and the store stamp (or now, if neither is present), and
/// is written to both: prefs unconditionally, the store on a best-effort
/// basis (a write failure there is swallowed by [`TrialStampStore`]'s
/// contract). This is what lets the trial survive deleting `ui-prefs.json`
/// alone: the secondary store still has the original date to restore.
///
/// A stamp that is present but fails to parse is preserved verbatim rather
/// than being replaced by "now", matching [`record_trial_start`]'s
/// historical "never resets an existing stamp" contract even for corrupt
/// data.
///
/// # Errors
///
/// [`Error::Io`] when prefs cannot be written.
pub fn record_trial_start_with(data_dir: &Path, store: &dyn TrialStampStore) -> Result<()> {
    let mut prefs = load_ui_prefs(data_dir);
    let store_raw = store.read_stamp();
    let canonical = earliest_present_stamp(prefs.trial_started_at.as_deref(), store_raw.as_deref());
    prefs.trial_started_at = Some(canonical.clone());
    store.write_stamp(&canonical);
    save_ui_prefs(data_dir, &prefs)
}

/// Pick the canonical trial-start stamp from the primary (`prefs`) and
/// secondary (`store`) raw stamps.
///
/// When only one side is present, the other is adopted outright: this is
/// what lets the secondary store restore the original date after the
/// primary prefs file is deleted, and what keeps the primary stamp in place
/// when the secondary store has nothing (for example, [`NoTrialStampStore`]
/// or an unavailable Keychain). When both are present and parse, the
/// earlier of the two wins. A stamp that fails to parse is never preferred
/// over one that does, and a lone unparseable stamp is kept as-is rather
/// than replaced. Neither present: "now".
fn earliest_present_stamp(prefs: Option<&str>, store: Option<&str>) -> String {
    match (prefs, store) {
        (None, None) => rfc3339_now(),
        (Some(p), None) => p.to_owned(),
        (None, Some(s)) => s.to_owned(),
        (Some(p), Some(s)) => match (parse_rfc3339(p), parse_rfc3339(s)) {
            (Some(pp), Some(ss)) if ss < pp => s.to_owned(),
            (None, Some(_)) => s.to_owned(),
            _ => p.to_owned(),
        },
    }
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
    use rand::Rng;
    use serde_json::json;
    use tempfile::tempdir;

    struct Ephemeral {
        verifier: LicenseVerifier,
        signing: SigningKey,
    }

    fn ephemeral() -> Ephemeral {
        let mut seed = [0u8; 32];
        rand::rng().fill_bytes(&mut seed);
        let signing = SigningKey::from_bytes(&seed);
        let verifier = LicenseVerifier::from_public_key_bytes(&signing.verifying_key().to_bytes())
            .expect("ephemeral verifying key");
        Ephemeral { verifier, signing }
    }

    /// Sign a v1 `.lic` JSON payload with the ephemeral test key.
    fn sign_license(
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

    fn write_license_file(dir: &Path, body: &str) -> PathBuf {
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
    fn workspace_pins_ed25519_dalek_v3() {
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
            version.starts_with("3."),
            "ed25519-dalek must be 3.x (1.x is RUSTSEC-2022-0093), got {version:?}"
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
        let body = sign_license(
            &keys.signing,
            PRODUCT,
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        let src = write_license_file(dir.path(), &body);
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
        let good = sign_license(
            &keys.signing,
            PRODUCT,
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        let good_src = write_license_file(dir.path(), &good);
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
        let bad_src = write_license_file(&bad_dir, &bad_sig);
        assert_eq!(
            install_license(dir.path(), &bad_src, &keys.verifier),
            Err(Error::LicenseInvalid)
        );

        let wrong = sign_license(
            &keys.signing,
            "other-product",
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        let wrong_dir = dir.path().join("wrong");
        fs::create_dir_all(&wrong_dir).expect("wrong dir");
        let wrong_src = write_license_file(&wrong_dir, &wrong);
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
        let body = sign_license(
            &keys.signing,
            PRODUCT,
            "2020-01-01",
            "buyer@example.com",
            "2020-01-01T00:00:00Z",
        );
        let src = write_license_file(dir.path(), &body);
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
    fn unparseable_trial_started_at_is_expired() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let mut prefs = load_ui_prefs(dir.path());
        prefs.trial_started_at = Some("not-a-date".into());
        save_ui_prefs(dir.path(), &prefs).expect("save");
        let status = license_status(dir.path(), &keys.verifier).expect("status");
        assert_eq!(status.state, LicenseState::Expired);
        assert!(!writes_allowed(&status));
        assert_eq!(
            require_writes_allowed(dir.path(), &keys.verifier),
            Err(Error::LicenseExpired)
        );
        record_trial_start(dir.path()).expect("must not restamp garbage");
        assert_eq!(
            load_ui_prefs(dir.path()).trial_started_at.as_deref(),
            Some("not-a-date")
        );
    }

    #[test]
    fn install_license_replaces_existing_file() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let first = sign_license(
            &keys.signing,
            PRODUCT,
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        let second = sign_license(
            &keys.signing,
            PRODUCT,
            "2098-06-15",
            "renew@example.com",
            "2026-08-20T13:00:00Z",
        );
        let first_src = write_license_file(dir.path(), &first);
        install_license(dir.path(), &first_src, &keys.verifier).expect("first");
        let renew_dir = dir.path().join("renew");
        fs::create_dir_all(&renew_dir).expect("renew dir");
        let second_src = write_license_file(&renew_dir, &second);
        let status = install_license(dir.path(), &second_src, &keys.verifier).expect("replace");
        assert_eq!(status.state, LicenseState::Licensed);
        assert_eq!(status.licensed_until.as_deref(), Some("2098-06-15"));
        let stored = fs::read_to_string(license_path(dir.path())).expect("stored");
        assert_eq!(stored, second, "second signed body replaces the first");
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
        let body = sign_license(
            &keys.signing,
            PRODUCT,
            "2020-06-01",
            "buyer@example.com",
            "2020-01-01T00:00:00Z",
        );
        let src = write_license_file(dir.path(), &body);
        let status = install_license(dir.path(), &src, &keys.verifier).expect("install");
        assert_eq!(status.state, LicenseState::Expired);
        assert!(!writes_allowed(&status));
    }

    /// In-memory stand-in for the desktop's macOS Keychain trial stamp.
    struct FakeStampStore(std::sync::Mutex<Option<String>>);

    impl TrialStampStore for FakeStampStore {
        fn read_stamp(&self) -> Option<String> {
            self.0.lock().ok().and_then(|guard| guard.clone())
        }

        fn write_stamp(&self, rfc3339: &str) {
            if let Ok(mut guard) = self.0.lock() {
                *guard = Some(rfc3339.to_owned());
            }
        }
    }

    #[test]
    fn deleting_the_prefs_stamp_does_not_reset_the_trial() {
        let dir = tempdir().expect("tempdir");
        let store = FakeStampStore(std::sync::Mutex::new(None));
        record_trial_start_with(dir.path(), &store).expect("stamp");
        let stamped = store.read_stamp().expect("secondary stamp written");

        // Casual reset: user deletes ui-prefs.json.
        std::fs::remove_file(crate::prefs::ui_prefs_path(dir.path())).expect("delete prefs");

        // Re-stamping must restore the ORIGINAL date from the secondary store.
        record_trial_start_with(dir.path(), &store).expect("re-stamp");
        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.trial_started_at.as_deref(), Some(stamped.as_str()));
    }

    #[test]
    fn earliest_stamp_wins_for_status() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();
        let old = "2020-01-01T00:00:00Z";
        let store = FakeStampStore(std::sync::Mutex::new(Some(old.to_owned())));

        // Prefs say "today", keychain says 2020: trial is long over.
        record_trial_start_with(dir.path(), &NoTrialStampStore).expect("prefs stamp");
        let now = OffsetDateTime::now_utc();
        let status = license_status_at_with(dir.path(), &keys.verifier, now, &store);
        assert_eq!(status.state, LicenseState::Expired);
    }

    #[test]
    fn entity_create_limit_is_not_license_expired() {
        let dir = tempdir().expect("tempdir");
        let keys = ephemeral();

        assert!(require_entity_create_allowed(dir.path(), &keys.verifier, 0).is_ok());
        assert_eq!(
            require_entity_create_allowed(dir.path(), &keys.verifier, 1),
            Err(Error::LicenseEntityLimit)
        );

        record_trial_start(dir.path()).expect("trial");
        assert!(require_entity_create_allowed(dir.path(), &keys.verifier, 0).is_ok());
        assert_eq!(
            require_entity_create_allowed(dir.path(), &keys.verifier, 1),
            Err(Error::LicenseEntityLimit)
        );

        let expired = sign_license(
            &keys.signing,
            PRODUCT,
            "2020-01-01",
            "buyer@example.com",
            "2020-01-01T00:00:00Z",
        );
        install_license(
            dir.path(),
            &write_license_file(dir.path(), &expired),
            &keys.verifier,
        )
        .expect("expired");
        assert_eq!(
            require_entity_create_allowed(dir.path(), &keys.verifier, 0),
            Err(Error::LicenseExpired)
        );
        assert_eq!(
            require_entity_create_allowed(dir.path(), &keys.verifier, 1),
            Err(Error::LicenseExpired)
        );

        let valid = sign_license(
            &keys.signing,
            PRODUCT,
            "2099-12-31",
            "buyer@example.com",
            "2026-08-20T12:00:00Z",
        );
        install_license(
            dir.path(),
            &write_license_file(dir.path(), &valid),
            &keys.verifier,
        )
        .expect("lic");
        assert!(require_entity_create_allowed(dir.path(), &keys.verifier, 3).is_ok());
    }
}
