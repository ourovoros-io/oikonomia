//! The update check: what a feed, its signature and its version lead to,
//! and what the machine does with the outcome.

use crate::client::{
    CheckOutcome, ClientConfig, InstallRoute, MAX_MANIFEST_BYTES, MAX_SIGNATURE_BYTES,
    UPDATE_FEED_URL, download_and_verify, perform_check,
};
use crate::error::UpdateError;
use crate::feed::{FeedArtifact, assemble_manifest};
use crate::hosts::HostPolicy;
use crate::machine::{CheckStart, UpdateMachine};
use crate::notes::sanitize_notes;
use crate::status::UpdateStatus;
use crate::tests::support::{
    available_offer, cache_dir, check_error_code, config, failed_with, install, leftover_files,
    machine_with_an_offer, serve_signed_manifest, server_url, sign, signed_manifest, spy,
    static_manifest, test_keys,
};
use crate::verify::sha256_hex;
use httptest::matchers::request;
use httptest::responders::status_code;
use httptest::{Expectation, Server};
use minisign::SecretKey;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;
use url::Url;

#[test]
fn production_feed_url_is_https_github() {
    let url = Url::parse(UPDATE_FEED_URL).expect("feed");
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("github.com"));
    assert!(url.path().ends_with("/latest.json"));
}

#[test]
fn production_feed_is_the_latest_release_of_this_source_repository() {
    // Releases are cut in the repository the code lives in. Deriving the
    // expectation from the manifest's `repository` means a repo move cannot
    // leave the updater pointing at a feed nobody publishes to.
    let expected = format!(
        "{}/releases/latest/download/latest.json",
        env!("CARGO_PKG_REPOSITORY")
    );

    assert_eq!(UPDATE_FEED_URL, expected);
}

#[test]
fn a_refused_connection_is_a_failed_check_and_writes_nothing() {
    let (public_key, _secret_key) = test_keys();
    let cache = cache_dir();
    // A loopback port that was just bound and released: nothing listens on
    // it, so the connection is refused at once, with no DNS lookup and no
    // dependence on the network the test runs in.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    drop(listener);
    let feed = Url::parse(&format!("http://{address}/latest.json")).expect("url");
    let config = ClientConfig::for_test(
        feed,
        &public_key,
        "0.1.0",
        "linux-x86_64",
        cache.path().to_path_buf(),
        HostPolicy::test_http_hosts(["127.0.0.1"]),
        Duration::from_secs(2),
    )
    .expect("config");

    assert_eq!(check_error_code(&config), "update_network");
    assert_eq!(
        UpdateMachine::new().check(&config),
        failed_with("update_network")
    );
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn timeout_check_is_failed_no_file_no_exec() {
    let (public_key, _secret_key) = test_keys();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    std::thread::spawn(move || {
        let _accepted = listener.accept();
        std::thread::sleep(Duration::from_secs(30));
    });
    let cache = cache_dir();
    let feed = Url::parse(&format!("http://{address}/latest.json")).expect("url");
    let config = ClientConfig::for_test(
        feed,
        &public_key,
        "0.1.0",
        "linux-x86_64",
        cache.path().to_path_buf(),
        HostPolicy::test_http_hosts(["127.0.0.1"]),
        Duration::from_millis(200),
    )
    .expect("config");
    let mut machine = UpdateMachine::new();
    let status = machine.check(&config);
    assert_eq!(status, failed_with("update_network"));
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn same_version_is_up_to_date_no_download() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let payload = b"artifact-bytes";
    let body = static_manifest(
        "0.1.0",
        "nothing new",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(0)
            .respond_with(status_code(500)),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn http_204_is_up_to_date_no_download() {
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(204)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn signed_update_is_available_without_install() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"newer-artifact";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "plain notes",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
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
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn missing_manifest_sig_is_failed() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"x";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(2)
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .times(2)
            .respond_with(status_code(404)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    // A feed published without its signature cannot be trusted; that is
    // not the same finding as a server that could not be reached, and the
    // status says which it was.
    assert_eq!(
        machine.check(&config),
        failed_with("update_manifest_signature")
    );
    assert_eq!(check_error_code(&config), "update_manifest_signature");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn bad_manifest_sig_is_failed() {
    let (public_key, secret_key) = test_keys();
    let (other_public_key, other_secret_key) = test_keys();
    let _ = other_public_key;
    let server = Server::run();
    let payload = b"x";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let wrong = sign(&other_secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &wrong);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(
        machine.check(&config),
        failed_with("update_manifest_signature")
    );
    assert_eq!(check_error_code(&config), "update_manifest_signature");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn truncated_json_after_valid_sig_is_failed() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let truncated = b"{\"version\":\"0.2.0\"";
    let signature = sign(&secret_key, truncated);
    serve_signed_manifest(
        &server,
        std::str::from_utf8(truncated).expect("utf8"),
        &signature,
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), failed_with("update_manifest_parse"));
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn host_not_allow_listed_is_failed_no_file() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"payload";
    let body = static_manifest(
        "0.2.0",
        "n",
        "https://evil.example/payload",
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), failed_with("update_artifact_url"));
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn file_url_artifact_is_failed() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"payload";
    let body = static_manifest(
        "0.2.0",
        "n",
        "file:///tmp/evil",
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), failed_with("update_artifact_url"));
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn html_notes_are_plain_text() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"newer";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "Read <a href=\"https://evil.example/nav\">here</a>",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
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
    assert!(!notes.contains(['<', '>', '"']), "{notes}");
}

/// Proves the promote lane (`assemble_manifest`) and the client (`perform_check`,
/// `download_and_verify`) agree on the wire shape: a manifest built the same way
/// `assemble_feed` builds it for a published release, served over httptest,
/// must check as `Available` and its artifact must download and verify.
#[test]
fn promoted_feed_round_trips_through_check_and_download() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();

    let artifact_bytes = b"new-app-bytes".to_vec();
    let artifact_signature = sign(&secret_key, &artifact_bytes);
    let artifact_sha256_hex = sha256_hex(&artifact_bytes);
    let base_url = server_url(&server, "/").to_string();

    let manifest_body = assemble_manifest(
        "9.9.9",
        "Promoted release.",
        &base_url,
        &[FeedArtifact {
            platform: "linux-x86_64".to_owned(),
            file_name: "Oikonomia_test.app.tar.gz".to_owned(),
            signature: artifact_signature,
            sha256_hex: artifact_sha256_hex,
        }],
    )
    .expect("assemble");
    let manifest_signature = sign(&secret_key, manifest_body.as_bytes());

    serve_signed_manifest(&server, &manifest_body, &manifest_signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia_test.app.tar.gz"))
            .respond_with(status_code(200).body(artifact_bytes.clone())),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let outcome = perform_check(&config);
    let CheckOutcome::Available(offer) = outcome else {
        panic!("promoted feed must yield Available, got {outcome:?}");
    };
    let path = download_and_verify(&config, &offer).expect("download");
    assert!(path.exists());
    assert_eq!(leftover_files(cache.path()).len(), 1);
}

#[test]
fn artifact_fields_outside_the_platform_table_are_not_an_offer() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"artifact-bytes";
    // No entry for this platform; the artifact sits at the top level, a
    // shape `assemble_manifest` never writes.
    let body = serde_json::json!({
        "version": "0.2.0",
        "notes": "notes",
        "platforms": {},
        "url": server_url(&server, "/Oikonomia.AppImage").as_str(),
        "signature": sign(&secret_key, payload),
        "sha256": sha256_hex(payload),
    })
    .to_string();
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_missing_platform");
}

/// Serves a signed manifest for `version` that lists an artifact for
/// `darwin-aarch64` only, and returns a config for a `linux-x86_64` app at
/// 0.1.0.
fn serve_release_without_this_platform(
    server: &Server,
    public_key: &str,
    secret_key: &SecretKey,
    version: &str,
    cache: &Path,
) -> ClientConfig {
    let payload = b"artifact-bytes";
    let body = assemble_manifest(
        version,
        "notes",
        server_url(server, "/").as_str(),
        &[FeedArtifact {
            platform: "darwin-aarch64".to_owned(),
            file_name: "Oikonomia.app.tar.gz".to_owned(),
            signature: sign(secret_key, payload),
            sha256_hex: sha256_hex(payload),
        }],
    )
    .expect("assemble");
    let signature = sign(secret_key, body.as_bytes());
    serve_signed_manifest(server, &body, &signature);

    config(
        server,
        "/latest.json",
        public_key,
        "0.1.0",
        cache,
        Duration::from_secs(2),
    )
}

#[test]
fn current_version_is_up_to_date_even_when_the_feed_omits_this_platform() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_release_without_this_platform(
        &server,
        &public_key,
        &secret_key,
        "0.1.0",
        cache.path(),
    );

    assert_eq!(UpdateMachine::new().check(&config), UpdateStatus::UpToDate);
}

#[test]
fn newer_version_without_this_platform_fails_as_a_missing_platform() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_release_without_this_platform(
        &server,
        &public_key,
        &secret_key,
        "0.2.0",
        cache.path(),
    );

    assert_eq!(check_error_code(&config), "update_missing_platform");
}

#[test]
fn feed_and_signature_requests_each_name_this_copy_once() {
    const IDENTITY_QUERY: &str = "version=0.1.0&os=linux&arch=x86_64";

    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &secret_key, "0.2.0");
    server.expect(
        Expectation::matching(httptest::all_of![
            request::method_path("GET", "/latest.json"),
            request::query(IDENTITY_QUERY),
        ])
        .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(httptest::all_of![
            request::method_path("GET", "/latest.json.sig"),
            request::query(IDENTITY_QUERY),
        ])
        .respond_with(status_code(200).body(signature)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let status = UpdateMachine::new().check(&config);

    assert!(
        matches!(status, UpdateStatus::Available { .. }),
        "got {status:?}"
    );
}

/// Returns a manifest offering 0.2.0, padded with trailing whitespace (which
/// JSON allows) to exactly `total_bytes`, and the signature over it.
fn signed_manifest_of_size(
    server: &Server,
    secret_key: &SecretKey,
    total_bytes: usize,
) -> (String, String) {
    let (mut body, _signature) = signed_manifest(server, secret_key, "0.2.0");
    let padding = total_bytes
        .checked_sub(body.len())
        .expect("the manifest is smaller than the requested size");
    body.push_str(&" ".repeat(padding));

    let signature = sign(secret_key, body.as_bytes());
    (body, signature)
}

#[test]
fn manifest_of_exactly_the_size_cap_is_accepted() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest_of_size(&server, &secret_key, MAX_MANIFEST_BYTES);
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let status = UpdateMachine::new().check(&config);

    assert!(
        matches!(status, UpdateStatus::Available { .. }),
        "got {status:?}"
    );
}

#[test]
fn manifest_one_byte_over_the_size_cap_is_refused() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    // Validly signed, so only its size stands between it and an offer.
    let (body, _signature) = signed_manifest_of_size(&server, &secret_key, MAX_MANIFEST_BYTES + 1);
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}

#[test]
fn manifest_signature_over_the_size_cap_is_refused() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, mut signature) = signed_manifest(&server, &secret_key, "0.2.0");
    // `verify_minisign` trims its input, so read in full the padded signature
    // would verify and the check would end in an offer.
    let padding = MAX_SIGNATURE_BYTES + 1 - signature.len();
    signature.push_str(&"\n".repeat(padding));
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}

#[test]
fn older_remote_version_is_up_to_date_and_never_downloaded() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &secret_key, "0.0.9");
    serve_signed_manifest(&server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(0)
            .respond_with(status_code(500)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();

    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);

    let err = install(&mut machine, &config, &installer)
        .expect_err("an older version must not be installable");
    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn http_204_on_the_manifest_signature_is_a_signature_failure() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, _signature) = signed_manifest(&server, &secret_key, "0.2.0");
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .respond_with(status_code(204)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_manifest_signature");
}

#[test]
fn a_check_during_an_install_is_refused_and_the_status_stays_installing() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (mut machine, config) =
        machine_with_an_offer(&server, &public_key, &secret_key, cache.path());
    let _offer = machine.begin_install().expect("available");
    assert_eq!(machine.status(), UpdateStatus::Installing);

    assert_eq!(machine.begin_check(), CheckStart::InstallInProgress);
    assert_eq!(machine.status(), UpdateStatus::Installing);

    assert_eq!(machine.check(&config), UpdateStatus::Installing);
}

#[test]
fn a_check_that_ends_after_an_install_began_does_not_replace_installing() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (mut machine, _config) =
        machine_with_an_offer(&server, &public_key, &secret_key, cache.path());
    let _offer = machine.begin_install().expect("available");

    // A second check that was already in flight when the first ended.
    machine.finish_check(CheckOutcome::UpToDate);
    assert_eq!(machine.status(), UpdateStatus::Installing);

    machine.finish_check(CheckOutcome::Failed(UpdateError::Network));
    assert_eq!(machine.status(), UpdateStatus::Installing);

    machine.abandon_check();
    assert_eq!(machine.status(), UpdateStatus::Installing);
}

#[test]
fn an_abandoned_check_fails_without_a_code_and_a_new_check_may_start() {
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.begin_check(), CheckStart::Started);

    // What the caller's drop guard does when the check never returns.
    machine.abandon_check();

    assert_eq!(machine.status(), UpdateStatus::Failed { code: None });
    assert_eq!(machine.begin_check(), CheckStart::Started);
}

#[test]
fn abandoning_a_check_that_was_never_begun_changes_nothing() {
    let mut machine = UpdateMachine::new();

    machine.abandon_check();

    assert_eq!(machine.status(), UpdateStatus::Idle);
}

#[test]
fn a_leading_v_in_the_feed_version_does_not_reach_the_status() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &secret_key, " v0.2.0 ");
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let offer = available_offer(&config);
    assert_eq!(offer.version().to_string(), "0.2.0");

    assert_eq!(
        UpdateMachine::new().check(&config),
        UpdateStatus::Available {
            version: "0.2.0".into(),
            notes: "notes".into(),
        }
    );
    assert_eq!(
        UpdateMachine::new().check(&config.with_install_route(InstallRoute::PackageManager)),
        UpdateStatus::AvailableManually {
            version: "0.2.0".into(),
            notes: "notes".into(),
        }
    );
}

#[test]
fn a_running_version_that_is_not_semver_is_refused_as_an_invalid_version() {
    let (public_key, _secret_key) = test_keys();
    let cache = cache_dir();

    let err = ClientConfig::for_test(
        Url::parse("http://127.0.0.1/latest.json").expect("url"),
        &public_key,
        "nightly",
        "linux-x86_64",
        cache.path().to_path_buf(),
        HostPolicy::test_http_hosts(["127.0.0.1"]),
        Duration::from_secs(1),
    )
    .expect_err("not semver");

    assert_eq!(err.code(), "update_invalid_version");
}

#[test]
fn a_signed_manifest_whose_version_is_not_semver_fails_as_an_invalid_version() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &secret_key, "next");
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_invalid_version");
}

#[test]
fn a_server_error_on_the_manifest_signature_is_a_network_failure() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, _signature) = signed_manifest(&server, &secret_key, "0.2.0");
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .respond_with(status_code(503)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}

#[test]
fn a_missing_manifest_is_a_network_failure_not_a_signature_one() {
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(404)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}
