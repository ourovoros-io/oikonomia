//! Fixtures the client tests share: signing keys, a local feed server, a
//! spy installer and a capture of the warnings the crate logs.

use crate::client::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, VerifiedOffer,
    install_offer, perform_check, perform_check_inner,
};
use crate::error::UpdateError;
use crate::hosts::HostPolicy;
use crate::machine::UpdateMachine;
use crate::status::UpdateStatus;
use crate::verify::sha256_hex;
use httptest::matchers::request;
use httptest::responders::status_code;
use httptest::{Expectation, Server};
use minisign::{KeyPair, SecretKey};
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use url::Url;

/// An installer that records how it was called and installs nothing.
pub(super) struct SpyInstaller {
    /// How many times `install` was called.
    pub(super) calls: Arc<AtomicUsize>,
    /// The artifact path of the last call.
    pub(super) last_path: Mutex<Option<PathBuf>>,
    /// Whether `install` reports a failure.
    fail: bool,
    /// What a successful `install` reports.
    handoff: InstallHandoff,
}

impl ArtifactInstaller for SpyInstaller {
    fn install(&self, artifact: &Path) -> crate::Result<InstallHandoff> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut last = self.last_path.lock().expect("spy path");
        *last = Some(artifact.to_path_buf());
        if self.fail {
            return Err(UpdateError::ArtifactIntegrity);
        }
        Ok(self.handoff)
    }
}

/// Returns a spy that reports the app replaced, or a failure when `fail`
/// is set, and a handle on its call count.
pub(super) fn spy(fail: bool) -> (SpyInstaller, Arc<AtomicUsize>) {
    spy_with_handoff(fail, InstallHandoff::Replaced)
}

/// Returns a spy that reports `handoff`, or a failure when `fail` is set,
/// and a handle on its call count.
pub(super) fn spy_with_handoff(
    fail: bool,
    handoff: InstallHandoff,
) -> (SpyInstaller, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        SpyInstaller {
            calls: Arc::clone(&calls),
            last_path: Mutex::new(None),
            fail,
            handoff,
        },
        calls,
    )
}

/// Runs an install the way the desktop does: begin on the machine, install
/// without it, then finish on it.
pub(super) fn install(
    machine: &mut UpdateMachine,
    config: &ClientConfig,
    installer: &SpyInstaller,
) -> crate::Result<InstallOutcome> {
    let offer = machine.begin_install()?;
    let outcome = install_offer(config, &offer, installer);
    machine.finish_install(&outcome);
    Ok(outcome)
}

/// Returns the status of a check or an install that failed with the error
/// whose code is `code`.
pub(super) fn failed_with(code: &str) -> UpdateStatus {
    UpdateStatus::Failed {
        code: Some(code.to_owned()),
    }
}

/// Serves a signed manifest offering `payload` at `artifact_path` as 0.2.0
/// and returns a config for an app at 0.1.0.
#[expect(
    clippy::too_many_arguments,
    reason = "a test helper taking the keys, the artifact and the cache; tracked for the API pass"
)]
pub(super) fn serve_newer_release(
    server: &Server,
    public_key: &str,
    secret_key: &SecretKey,
    artifact_path: &'static str,
    payload: &'static [u8],
    cache: &Path,
) -> ClientConfig {
    let artifact = server_url(server, artifact_path);
    let body = static_manifest(
        "0.2.0",
        "ok",
        artifact.as_str(),
        &sign(secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(secret_key, body.as_bytes());
    serve_signed_manifest(server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", artifact_path))
            .times(0..)
            .respond_with(status_code(200).body(payload)),
    );
    config(
        server,
        "/latest.json",
        public_key,
        "0.1.0",
        cache,
        Duration::from_secs(2),
    )
}

/// Generates a minisign key pair and returns the public key as the text of
/// a key file, with the secret key to sign fixtures with.
pub(super) fn test_keys() -> (String, SecretKey) {
    let KeyPair {
        pk: public_key,
        sk: secret_key,
    } = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let boxed = public_key.to_box().expect("public box");
    (boxed.to_string(), secret_key)
}

/// Returns the minisign signature file for `data`, signed with `secret_key`.
pub(super) fn sign(secret_key: &SecretKey, data: &[u8]) -> String {
    let signature = minisign::sign(None, secret_key, Cursor::new(data), None, None).expect("sign");
    signature.into_string()
}

/// Returns a fresh cache directory. The caller holds the handle for the whole
/// test: dropping it removes the directory.
pub(super) fn cache_dir() -> TempDir {
    tempfile::tempdir().expect("temporary directory")
}

/// Returns the URL of `path` on `server`.
pub(super) fn server_url(server: &Server, path: &str) -> Url {
    Url::parse(&server.url_str(path)).expect("server url")
}

/// Returns a policy that allows plain `http` to `server` and nothing else.
pub(super) fn policy_for(server: &Server) -> HostPolicy {
    let url = server_url(server, "/");
    let host = url.host_str().expect("host").to_owned();
    HostPolicy::test_http_hosts([host])
}

/// Returns a config for a `linux-x86_64` app at `current_version` whose
/// feed is `feed_path` on `server`.
#[expect(
    clippy::too_many_arguments,
    reason = "a test helper forwarding most of `ClientConfig::for_test`; tracked for the API pass"
)]
pub(super) fn config(
    server: &Server,
    feed_path: &str,
    public_key: &str,
    current_version: &str,
    cache: &Path,
    timeout: Duration,
) -> ClientConfig {
    let feed_url = server_url(server, feed_path);
    ClientConfig::for_test(
        feed_url,
        public_key,
        current_version,
        "linux-x86_64",
        cache.to_path_buf(),
        policy_for(server),
        timeout,
    )
    .expect("config")
}

/// Returns a manifest written by hand, not by `assemble_manifest`, with
/// one `linux-x86_64` entry. Tests use it to state fields the release lane
/// would never write.
pub(super) fn static_manifest(
    version: &str,
    notes: &str,
    artifact_url: &str,
    signature: &str,
    sha256: &str,
) -> String {
    format!(
        r#"{{
  "version": "{version}",
  "notes": {notes_json},
  "platforms": {{
    "linux-x86_64": {{
      "url": "{artifact_url}",
      "signature": {signature_json},
      "sha256": "{sha256}"
    }}
  }}
}}"#,
        notes_json = serde_json::to_string(notes).expect("notes json"),
        signature_json = serde_json::to_string(signature).expect("signature json"),
    )
}

/// Serves `body` at `/latest.json` and `signature` at `/latest.json.sig`,
/// each at least once.
pub(super) fn serve_signed_manifest(server: &Server, body: &str, signature: &str) {
    serve_signed_manifest_times(server, body, signature, 1..);
}

/// Serves `body` at `/latest.json` and `signature` at `/latest.json.sig`,
/// each the number of times `times` allows.
pub(super) fn serve_signed_manifest_times(
    server: &Server,
    body: &str,
    signature: &str,
    times: std::ops::RangeFrom<usize>,
) {
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(times.clone())
            .respond_with(status_code(200).body(body.to_owned())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .times(times)
            .respond_with(status_code(200).body(signature.to_owned())),
    );
}

/// Serves `body` at `/latest.json` and `signature` at `/latest.json.sig`,
/// each exactly once, so a second check fails the test.
pub(super) fn serve_signed_manifest_once(server: &Server, body: &str, signature: &str) {
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(1)
            .respond_with(status_code(200).body(body.to_owned())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .times(1)
            .respond_with(status_code(200).body(signature.to_owned())),
    );
}

/// Returns the entries directly inside `dir`, or none when it does not
/// exist.
pub(super) fn leftover_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        found.push(entry.path());
    }
    found
}

/// Returns a manifest offering `version` at `/Oikonomia.AppImage` on `server`
/// and the detached signature over it.
pub(super) fn signed_manifest(
    server: &Server,
    secret_key: &SecretKey,
    version: &str,
) -> (String, String) {
    let payload = b"artifact-bytes";
    let body = static_manifest(
        version,
        "notes",
        server_url(server, "/Oikonomia.AppImage").as_str(),
        &sign(secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(secret_key, body.as_bytes());
    (body, signature)
}

/// Returns the code of the error the check ends with.
pub(super) fn check_error_code(config: &ClientConfig) -> &'static str {
    match perform_check_inner(config) {
        Ok(outcome) => panic!("the check must fail, got {outcome:?}"),
        Err(err) => err.code(),
    }
}

/// Reads from `stream` up to the blank line that ends an HTTP request head.
pub(super) fn read_request_head(stream: &mut TcpStream) {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).expect("request head");
        head.push(byte[0]);
    }
}

/// Answers the first request on a fresh loopback port with `total_bytes` zero
/// bytes, written `piece_bytes` at a time with `pause` before each write.
///
/// httptest sends a body in one piece; this server exists for the tests that
/// need a body to arrive slowly or to be larger than is worth holding twice.
pub(super) fn serve_zero_bytes(
    total_bytes: usize,
    piece_bytes: usize,
    pause: Duration,
) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");

    std::thread::spawn(move || {
        let (mut stream, _peer) = listener.accept().expect("accept");
        read_request_head(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {total_bytes}\r\nConnection: close\r\n\r\n"
        )
        .expect("response head");

        let piece = vec![0_u8; piece_bytes];
        let mut remaining = total_bytes;
        while remaining > 0 {
            std::thread::sleep(pause);
            let length = remaining.min(piece_bytes);
            // A client that stops at its size cap closes the connection with
            // bytes still unsent; that ends this server, it is not a failure.
            if stream.write_all(&piece[..length]).is_err() {
                return;
            }
            remaining -= length;
        }
    });

    address
}

/// Serves a signed 0.2.0 manifest on `server` whose artifact is `payload` at
/// `artifact_address`, and returns a config for an app at 0.1.0 that may
/// fetch from both.
pub(super) fn config_for_artifact_at(
    server: &Server,
    keys: (&str, &SecretKey),
    artifact_address: SocketAddr,
    payload: &[u8],
    cache: &Path,
) -> ClientConfig {
    let (public_key, secret_key) = keys;
    let body = static_manifest(
        "0.2.0",
        "notes",
        &format!("http://{artifact_address}/Oikonomia.AppImage"),
        &sign(secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(secret_key, body.as_bytes());
    serve_signed_manifest(server, &body, &signature);

    let feed_host = server_url(server, "/").host_str().expect("host").to_owned();
    ClientConfig::for_test(
        server_url(server, "/latest.json"),
        public_key,
        "0.1.0",
        "linux-x86_64",
        cache.to_path_buf(),
        HostPolicy::test_http_hosts([feed_host, artifact_address.ip().to_string()]),
        Duration::from_millis(500),
    )
    .expect("config")
}

/// Runs a check with `config` and returns the offer it finds.
pub(super) fn available_offer(config: &ClientConfig) -> VerifiedOffer {
    match perform_check(config) {
        CheckOutcome::Available(offer) => offer,
        outcome => panic!("expected an offer, got {outcome:?}"),
    }
}

/// Keeps every warning the crate logs while the tests run.
///
/// `log` takes one logger per process, so all tests share this one and each
/// looks only for messages that name a path of its own.
pub(super) struct CapturedWarnings {
    /// Every warning logged so far, oldest first.
    messages: Mutex<Vec<String>>,
}

impl CapturedWarnings {
    /// Returns the captured warnings whose text contains `path`.
    pub(super) fn mentioning(&self, path: &Path) -> Vec<String> {
        let needle = path.display().to_string();
        let messages = self.messages.lock().expect("captured warnings");
        messages
            .iter()
            .filter(|message| message.contains(&needle))
            .cloned()
            .collect()
    }
}

impl log::Log for CapturedWarnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let mut messages = self.messages.lock().expect("captured warnings");
            messages.push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

/// Returns the process-wide warning capture, installing it as the logger
/// on first use.
pub(super) fn captured_warnings() -> &'static CapturedWarnings {
    static WARNINGS: CapturedWarnings = CapturedWarnings {
        messages: Mutex::new(Vec::new()),
    };
    static INSTALL: std::sync::Once = std::sync::Once::new();

    INSTALL.call_once(|| {
        log::set_logger(&WARNINGS).expect("this test binary installs no other logger");
        log::set_max_level(log::LevelFilter::Warn);
    });
    &WARNINGS
}

/// Returns a machine that found the 0.2.0 release served on `server`, and
/// the config it was checked with.
pub(super) fn machine_with_an_offer(
    server: &Server,
    public_key: &str,
    secret_key: &SecretKey,
    cache: &Path,
) -> (UpdateMachine, ClientConfig) {
    let config = serve_newer_release(
        server,
        public_key,
        secret_key,
        "/Oikonomia.AppImage",
        b"appimage",
        cache,
    );
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    (machine, config)
}
