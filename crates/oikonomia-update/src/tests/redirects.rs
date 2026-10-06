//! Redirects: each hop is checked against the allow-list, and a chain has a
//! limit.

use crate::client::{
    CheckOutcome, ClientConfig, MAX_REDIRECTS, VerifiedOffer, download_and_verify, perform_check,
};
use crate::machine::UpdateMachine;
use crate::status::UpdateStatus;
use crate::tests::support::{
    cache_dir, check_error_code, config, leftover_files, serve_signed_manifest, server_url,
    signed_manifest, test_keys,
};
use httptest::matchers::request;
use httptest::responders::status_code;
use httptest::{Expectation, Server};
use minisign::SecretKey;
use std::path::PathBuf;
use std::time::Duration;

/// Returns a 302 response that points at `location`.
fn redirect_to(location: &str) -> impl httptest::responders::Responder + use<> {
    status_code(302).append_header("Location", location.to_owned())
}

#[test]
fn feed_redirect_to_an_allowed_host_is_followed() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &secret_key, "0.2.0");
    let moved = server_url(&server, "/assets/latest.json");
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(redirect_to(moved.as_str())),
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

    let status = UpdateMachine::new().check(&config);

    assert_eq!(
        status,
        UpdateStatus::Available {
            version: "0.2.0".into(),
            notes: "notes".into(),
        }
    );
}

#[test]
fn feed_redirect_to_a_host_off_the_allow_list_is_refused() {
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(redirect_to("http://evil.example/latest.json")),
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

    assert_eq!(check_error_code(&config), "update_artifact_url");
}

#[test]
fn a_chain_of_exactly_the_redirect_limit_is_followed() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &secret_key, "0.2.0");
    // Relative locations: each hop is resolved against the URL that sent it.
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(redirect_to("/hop/1")),
    );
    for hop in 1..MAX_REDIRECTS {
        server.expect(
            Expectation::matching(request::method_path("GET", format!("/hop/{hop}")))
                .respond_with(redirect_to(&format!("/hop/{}", hop + 1))),
        );
    }
    server.expect(
        Expectation::matching(request::method_path("GET", format!("/hop/{MAX_REDIRECTS}")))
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

    let status = UpdateMachine::new().check(&config);

    assert!(
        matches!(status, UpdateStatus::Available { .. }),
        "got {status:?}"
    );
}

#[test]
fn one_redirect_past_the_limit_fails_without_another_request() {
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    // The first request plus one per followed redirect; the redirect that
    // answers the last of them is the one past the limit.
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(usize::from(MAX_REDIRECTS) + 1)
            .respond_with(redirect_to("/latest.json")),
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
fn redirect_without_a_location_fails() {
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(302)),
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

/// Serves a signed 0.2.0 manifest whose artifact URL answers with a redirect
/// to `location`, and returns the offer a check finds there.
fn offer_with_redirected_artifact(
    server: &Server,
    secret_key: &SecretKey,
    config: &ClientConfig,
    location: &str,
) -> VerifiedOffer {
    let (body, signature) = signed_manifest(server, secret_key, "0.2.0");
    serve_signed_manifest(server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(redirect_to(location)),
    );

    match perform_check(config) {
        CheckOutcome::Available(offer) => offer,
        outcome => panic!("expected an offer, got {outcome:?}"),
    }
}

#[test]
fn artifact_redirect_to_an_allowed_host_is_downloaded_and_verified() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let moved = server_url(&server, "/cdn/Oikonomia.AppImage");
    let offer = offer_with_redirected_artifact(&server, &secret_key, &config, moved.as_str());
    server.expect(
        Expectation::matching(request::method_path("GET", "/cdn/Oikonomia.AppImage"))
            .respond_with(status_code(200).body("artifact-bytes")),
    );

    let path = download_and_verify(&config, &offer).expect("download");

    assert_eq!(std::fs::read(&path).expect("read"), b"artifact-bytes");
}

#[test]
fn artifact_redirect_to_a_host_off_the_allow_list_is_refused() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let offer = offer_with_redirected_artifact(
        &server,
        &secret_key,
        &config,
        "http://evil.example/payload",
    );

    let err = download_and_verify(&config, &offer).expect_err("redirect off the allow-list");

    assert_eq!(err.code(), "update_artifact_url");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}
