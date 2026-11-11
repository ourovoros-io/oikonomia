//! The updater trail of real checks and installs against local servers:
//! which steps are recorded, in which order, and that no line holds a URL.

use crate::client::{CheckOutcome, ClientConfig, perform_check};
use crate::hosts::HostPolicy;
use crate::machine::UpdateMachine;
use crate::status::UpdateStatus;
use crate::tests::support::{
    available_offer, cache_dir, captured_log, config, install, machine_with_an_offer,
    serve_signed_manifest, server_url, sign, signed_manifest, spy, static_manifest, test_keys,
};
use crate::trail::Event;
use crate::verify::sha256_hex;
use httptest::matchers::request;
use httptest::responders::status_code;
use httptest::{Expectation, Server};
use std::net::TcpListener;
use std::time::Duration;
use url::Url;

/// Fails unless no line of `trail` holds a part of a URL other than its
/// host: a scheme, a path, the identity query, or a port.
fn assert_no_url_in(trail: &[String], port: u16) {
    for line in trail {
        for leaked in [
            "http",
            "/",
            "?",
            "version=",
            "os=",
            "arch=",
            &format!(":{port}"),
        ] {
            assert!(!line.contains(leaked), "{leaked:?} in {line:?}");
        }
    }
}

/// Returns the port of `server`.
fn port_of(server: &Server) -> u16 {
    server_url(server, "/").port().expect("a port")
}

/// Returns the host of `server` as the trail writes it: `127.0.0.1` or
/// `[::1]`, whichever loopback address it bound.
fn host_of(server: &Server) -> String {
    server_url(server, "/")
        .host_str()
        .expect("a host")
        .to_owned()
}

#[test]
fn a_refused_connection_records_the_host_and_the_transport_failure() {
    let log = captured_log();
    let (public_key, _secret_key) = test_keys();
    let cache = cache_dir();
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
        HostPolicy::with_http_hosts(["127.0.0.1"]),
        Duration::from_secs(2),
    )
    .expect("config");

    let _outcome = perform_check(&config);

    let trail = log.trail_of_this_thread();
    assert_eq!(trail.len(), 1, "{trail:?}");
    // Only the I/O kind after the prefix is left unchecked: Linux and macOS
    // refuse a closed loopback port, but Windows retries the SYN after the
    // reset and may report a timeout instead. Any error from the connect is
    // `Connection Failed` in ureq, whatever its kind.
    assert!(
        trail[0].starts_with(
            "fetch feed: host 127.0.0.1, 0 redirects, no response: Connection Failed: "
        ),
        "{trail:?}"
    );
    assert_no_url_in(&trail, address.port());
}

#[test]
fn a_failed_request_logs_no_warning_that_holds_its_url() {
    let log = captured_log();
    let (public_key, _secret_key) = test_keys();
    let cache = cache_dir();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let refused_port = listener.local_addr().expect("address").port();
    drop(listener);
    let refused = Url::parse(&format!(
        "http://127.0.0.1:{refused_port}/refused-path/latest.json?token=refused-secret"
    ))
    .expect("url");
    let refused_config = ClientConfig::for_test(
        refused,
        &public_key,
        "0.1.0",
        "linux-x86_64",
        cache.path().to_path_buf(),
        HostPolicy::with_http_hosts(["127.0.0.1"]),
        Duration::from_secs(2),
    )
    .expect("config");
    let _refused = perform_check(&refused_config);
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/missing-path/latest.json"))
            .respond_with(status_code(404)),
    );
    let missing_config = config(
        &server,
        "/missing-path/latest.json?token=missing-secret",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let _missing = perform_check(&missing_config);

    for leaked in [
        "refused-path",
        "refused-secret",
        &format!(":{refused_port}"),
        "missing-path",
        "missing-secret",
        &format!(":{}", port_of(&server)),
    ] {
        assert_eq!(log.containing(leaked), Vec::<String>::new(), "{leaked}");
    }
}

#[test]
fn a_redirected_check_records_each_request_and_the_signature_verdict() {
    let log = captured_log();
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &secret_key, "0.2.0");
    let moved = server_url(&server, "/assets/latest.json?token=private");
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(302).append_header("Location", moved.to_string())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/assets/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
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

    let _outcome = perform_check(&config);

    let trail = log.trail_of_this_thread();
    let host = host_of(&server);
    assert_eq!(
        trail,
        [
            format!("fetch feed: host {host}, 1 redirect, ok"),
            format!("fetch feed signature: host {host}, 0 redirects, ok"),
            "verify feed signature: passed".to_owned(),
        ]
    );
    assert_no_url_in(&trail, port_of(&server));
    assert!(
        trail.iter().all(|line| !line.contains("token")),
        "{trail:?}"
    );
}

#[test]
fn a_redirect_off_the_allow_list_records_the_host_it_refused() {
    let log = captured_log();
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json")).respond_with(
            status_code(302).append_header("Location", "https://evil.example/x?version=0.1.0"),
        ),
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

    let _outcome = perform_check(&config);

    let trail = log.trail_of_this_thread();
    assert_eq!(
        trail,
        ["fetch feed: host evil.example, 1 redirect, refused: host not on the allow-list"]
    );
    assert_no_url_in(&trail, port_of(&server));
}

#[test]
fn a_forged_feed_records_that_its_signature_does_not_verify() {
    let log = captured_log();
    let (public_key, _secret_key) = test_keys();
    let (_other_public_key, other_secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &other_secret_key, "0.2.0");
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

    let _outcome = perform_check(&config);

    assert_eq!(
        log.trail_of_this_thread().last().map(String::as_str),
        Some("verify feed signature: signature does not verify")
    );
}

#[test]
fn an_install_records_the_download_its_size_the_verdict_and_the_handoff() {
    let log = captured_log();
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (mut machine, config) =
        machine_with_an_offer(&server, &public_key, &secret_key, cache.path());
    let (installer, _calls) = spy(false);
    let checked = log.trail_of_this_thread().len();

    let _outcome = install(&mut machine, &config, &installer).expect("an offer");

    let trail = log.trail_of_this_thread();
    assert_eq!(
        trail[checked..],
        [
            format!("fetch artifact: host {}, 0 redirects, ok", host_of(&server)),
            "artifact size: 8 bytes, downloaded".to_owned(),
            "verify artifact: passed".to_owned(),
            "install handoff: verified artifact handed to the installer".to_owned(),
        ]
    );
    assert_no_url_in(&trail, port_of(&server));
}

#[test]
fn a_tampered_artifact_records_its_digest_mismatch_and_reaches_no_installer() {
    let log = captured_log();
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(b"different"),
    );
    serve_signed_manifest(&server, &body, &sign(&secret_key, body.as_bytes()));
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(payload.as_slice())),
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
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let (installer, _calls) = spy(false);

    let _outcome = install(&mut machine, &config, &installer).expect("an offer");

    let trail = log.trail_of_this_thread();
    assert_eq!(
        trail[trail.len() - 2..],
        [
            "artifact size: 10 bytes, downloaded",
            "verify artifact: digest mismatch",
        ]
    );
    assert!(
        !trail.iter().any(|line| line.starts_with("install handoff")),
        "{trail:?}"
    );
}

#[test]
fn the_debug_of_a_finished_check_holds_no_artifact_url() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let artifact = server_url(
        &server,
        "/releases/download/v0.2.0/Oikonomia.AppImage?token=artifact-secret",
    );
    let payload = b"artifact-bytes";
    let body = static_manifest(
        "0.2.0",
        "notes",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    serve_signed_manifest(&server, &body, &sign(&secret_key, body.as_bytes()));
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let outcome = CheckOutcome::Available(available_offer(&config));

    let debug = format!("{:?}", Event::CheckFinished(&outcome));

    assert_eq!(debug, "Event(check finished: version 0.2.0 available)");
    for leaked in [
        artifact.as_str(),
        "releases/download",
        "Oikonomia.AppImage",
        "token",
        "artifact-secret",
    ] {
        assert!(!debug.contains(leaked), "{leaked:?} in {debug:?}");
    }
}
