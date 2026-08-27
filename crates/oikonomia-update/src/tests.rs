//! Local httptest fixtures. Never contacts production GitHub.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]
#![expect(clippy::panic, reason = "tests fail loudly by design")]

use crate::UPDATE_FEED_URL;
use crate::client::{ArtifactInstaller, ClientConfig, download_and_verify};
use crate::error::UpdateError;
use crate::hosts::HostPolicy;
use crate::machine::UpdateMachine;
use crate::notes::sanitize_notes;
use crate::status::UpdateStatus;
use crate::verify::{parse_public_key, to_hex};
use httptest::responders::status_code;
use httptest::{Expectation, Server, matchers::request};
use minisign::{KeyPair, SecretKey};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

struct SpyInstaller {
    calls: Arc<AtomicUsize>,
    last_path: Mutex<Option<PathBuf>>,
    fail: bool,
}

impl ArtifactInstaller for SpyInstaller {
    fn install(&self, artifact: &Path) -> crate::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut last = self.last_path.lock().expect("spy path");
        *last = Some(artifact.to_path_buf());
        if self.fail {
            return Err(UpdateError::ArtifactIntegrity);
        }
        Ok(())
    }
}

fn spy(fail: bool) -> (SpyInstaller, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        SpyInstaller {
            calls: Arc::clone(&calls),
            last_path: Mutex::new(None),
            fail,
        },
        calls,
    )
}

fn test_keys() -> (String, SecretKey) {
    let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let boxed = pk.to_box().expect("public box");
    (boxed.to_string(), sk)
}

fn sign(sk: &SecretKey, data: &[u8]) -> String {
    let signature = minisign::sign(None, sk, Cursor::new(data), None, None).expect("sign");
    signature.into_string()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    to_hex(&hasher.finalize())
}

fn cache_dir() -> PathBuf {
    tempfile::tempdir().expect("tmpdir").keep()
}

fn server_url(server: &Server, path: &str) -> Url {
    Url::parse(&server.url_str(path)).expect("server url")
}

fn policy_for(server: &Server) -> HostPolicy {
    let url = server_url(server, "/");
    let host = url.host_str().expect("host").to_owned();
    HostPolicy::test_http_hosts([host])
}

fn config(
    server: &Server,
    feed_path: &str,
    public_key: &str,
    current_version: &str,
    cache: PathBuf,
    timeout: Duration,
) -> ClientConfig {
    let feed_url = server_url(server, feed_path);
    ClientConfig::for_test(
        feed_url,
        public_key,
        current_version,
        "linux-x86_64",
        cache,
        policy_for(server),
        timeout,
    )
    .expect("config")
}

fn static_manifest(
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
      "signature": {sig_json},
      "sha256": "{sha256}"
    }}
  }}
}}"#,
        notes_json = serde_json::to_string(notes).expect("notes json"),
        sig_json = serde_json::to_string(signature).expect("sig json"),
    )
}

fn serve_signed_manifest(server: &Server, body: &str, sig: &str) {
    serve_signed_manifest_times(server, body, sig, 1..);
}

fn serve_signed_manifest_times(
    server: &Server,
    body: &str,
    sig: &str,
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
            .respond_with(status_code(200).body(sig.to_owned())),
    );
}

fn serve_signed_manifest_once(server: &Server, body: &str, sig: &str) {
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(1)
            .respond_with(status_code(200).body(body.to_owned())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .times(1)
            .respond_with(status_code(200).body(sig.to_owned())),
    );
}

fn leftover_files(dir: &Path) -> Vec<PathBuf> {
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

#[test]
fn empty_public_key_is_rejected() {
    let err = parse_public_key("").expect_err("empty");
    assert_eq!(err.code(), "update_missing_public_key");
    let err = parse_public_key("   ").expect_err("whitespace");
    assert_eq!(err.code(), "update_missing_public_key");
}

#[test]
fn production_feed_url_is_https_github() {
    let url = Url::parse(UPDATE_FEED_URL).expect("feed");
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("github.com"));
    assert!(url.path().ends_with("/latest.json"));
}

#[test]
fn offline_dns_check_is_failed_no_file_no_exec() {
    let (pk, _sk) = test_keys();
    let cache = cache_dir();
    let feed = Url::parse("http://no-such-host.invalid/latest.json").expect("url");
    let config = ClientConfig::for_test(
        feed,
        &pk,
        "0.1.0",
        "linux-x86_64",
        cache.clone(),
        HostPolicy::test_http_hosts(["no-such-host.invalid"]),
        Duration::from_millis(400),
    )
    .expect("config");
    let (installer, _calls) = spy(false);
    let mut machine = UpdateMachine::new();
    let status = machine.check(&config);
    assert_eq!(status, UpdateStatus::Failed);
    assert!(leftover_files(&cache).is_empty());
    assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn timeout_check_is_failed_no_file_no_exec() {
    let (pk, _sk) = test_keys();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        let _accepted = listener.accept();
        std::thread::sleep(Duration::from_secs(30));
    });
    let cache = cache_dir();
    let feed = Url::parse(&format!("http://{addr}/latest.json")).expect("url");
    let config = ClientConfig::for_test(
        feed,
        &pk,
        "0.1.0",
        "linux-x86_64",
        cache.clone(),
        HostPolicy::test_http_hosts(["127.0.0.1"]),
        Duration::from_millis(200),
    )
    .expect("config");
    let mut machine = UpdateMachine::new();
    let status = machine.check(&config);
    assert_eq!(status, UpdateStatus::Failed);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn same_version_is_up_to_date_no_download() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let payload = b"artifact-bytes";
    let body = static_manifest(
        "0.1.0",
        "nothing new",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(0)
            .respond_with(status_code(500)),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn http_204_is_up_to_date_no_download() {
    let (pk, _sk) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(204)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn signed_update_is_available_without_install() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"newer-artifact";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "plain notes",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let (installer, _calls) = spy(false);
    let mut machine = UpdateMachine::new();
    let status = machine.check(&config);
    assert_eq!(
        status,
        UpdateStatus::Available {
            version: "0.2.0".into(),
            notes: "plain notes".into(),
        }
    );
    assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn missing_manifest_sig_is_failed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"x";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .respond_with(status_code(404)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn bad_manifest_sig_is_failed() {
    let (pk, sk) = test_keys();
    let (other_pk, other_sk) = test_keys();
    let _ = other_pk;
    let server = Server::run();
    let payload = b"x";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let wrong = sign(&other_sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &wrong);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn truncated_json_after_valid_sig_is_failed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let truncated = b"{\"version\":\"0.2.0\"";
    let sig = sign(&sk, truncated);
    serve_signed_manifest(&server, std::str::from_utf8(truncated).expect("utf8"), &sig);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn host_not_allow_listed_is_failed_no_file() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"payload";
    let body = static_manifest(
        "0.2.0",
        "n",
        "https://evil.example/payload",
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn file_url_artifact_is_failed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"payload";
    let body = static_manifest(
        "0.2.0",
        "n",
        "file:///tmp/evil",
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn artifact_hash_mismatch_deletes_partial_and_does_not_exec() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(b"different"),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(payload.as_slice())),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let status = machine.install(&config, &installer).expect("legal");
    assert_eq!(status, UpdateStatus::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn artifact_sig_mismatch_deletes_partial_and_does_not_exec() {
    let (pk, sk) = test_keys();
    let (_other_pk, other_sk) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&other_sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(payload.as_slice())),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let status = machine.install(&config, &installer).expect("legal");
    assert_eq!(status, UpdateStatus::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn install_from_idle_is_hard_error() {
    let mut machine = UpdateMachine::new();
    let (pk, _sk) = test_keys();
    let server = Server::run();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache_dir(),
        Duration::from_secs(1),
    );
    let (installer, _calls) = spy(false);
    let err = machine.install(&config, &installer).expect_err("idle");
    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(machine.status(), UpdateStatus::Idle);
}

#[test]
fn install_from_failed_is_hard_error() {
    let (pk, _sk) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(500)),
    );
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache_dir(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    let (installer, _calls) = spy(false);
    let err = machine.install(&config, &installer).expect_err("failed");
    assert_eq!(err.code(), "update_install_not_allowed");
}

#[test]
fn install_from_checking_is_hard_error() {
    let mut machine = UpdateMachine::new();
    machine.begin_check();
    let (pk, _sk) = test_keys();
    let server = Server::run();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache_dir(),
        Duration::from_secs(1),
    );
    let (installer, _calls) = spy(false);
    let err = machine.install(&config, &installer).expect_err("checking");
    assert_eq!(err.code(), "update_install_not_allowed");
}

#[test]
fn html_notes_are_plain_text() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"newer";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "Read <a href=\"https://evil.example/nav\">here</a>",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache_dir(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    let UpdateStatus::Available { notes, version } = machine.check(&config) else {
        panic!("expected available");
    };
    assert_eq!(version, "0.2.0");
    assert_eq!(
        notes,
        sanitize_notes("Read <a href=\"https://evil.example/nav\">here</a>")
    );
    assert!(!notes.contains('<') || notes.contains("&lt;"));
    assert!(!notes.contains("href"));
}

#[test]
fn successful_install_calls_exec_once() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"install-me";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "ok",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest_once(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(1)
            .respond_with(status_code(200).body(payload.as_slice())),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    let status = machine.install(&config, &installer).expect("legal");
    assert!(matches!(status, UpdateStatus::Available { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let exec_path = installer
        .last_path
        .lock()
        .expect("path")
        .clone()
        .expect("installer received a path");
    assert!(
        exec_path.starts_with(&cache),
        "exec path must be the wrapper-verified cache file, got {}",
        exec_path.display()
    );
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn download_and_verify_rejects_mismatched_hash_and_leaves_no_file() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(b"nope"),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(payload.as_slice())),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.clone(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    let offer = machine.require_available().expect("offer").clone();
    let err = download_and_verify(&config, &offer).expect_err("hash");
    assert_eq!(err.code(), "update_artifact_integrity");
    assert!(leftover_files(&cache).is_empty());
}

#[test]
fn license_hex_is_not_a_minisign_public_key() {
    const LICENSE_HEX: &str = "7d5b038e9ab30eef536cc559baac20e44070adedcdf548af48744029804ec671";
    let err = parse_public_key(LICENSE_HEX).expect_err("license hex");
    assert_eq!(err.code(), "update_missing_public_key");
}
