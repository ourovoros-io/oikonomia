//! The install: download, verification and hand-off to the installer, and
//! the machine's installing state around them.

use crate::artifact_limit::MAX_ARTIFACT_BYTES;
use crate::client::{InstallHandoff, InstallOutcome, InstallRoute, download_and_verify};
use crate::error::{InstallStep, UpdateError};
use crate::machine::{CheckStart, UpdateMachine};
use crate::status::UpdateStatus;
use crate::tests::support::{
    available_offer, cache_dir, config, config_for_artifact_at, failed_with, install,
    leftover_files, machine_with_an_offer, serve_newer_release, serve_signed_manifest,
    serve_signed_manifest_once, serve_zero_bytes, server_url, sign, spy, static_manifest,
    test_keys,
};
use crate::verify::sha256_hex;
use httptest::matchers::request;
use httptest::responders::status_code;
use httptest::{Expectation, Server};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn artifact_hash_mismatch_leaves_no_file_and_does_not_exec() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(b"different"),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
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
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let outcome = install(&mut machine, &config, &installer).expect("legal");
    assert!(
        matches!(
            outcome,
            InstallOutcome::Failed(UpdateError::ArtifactIntegrity)
        ),
        "{outcome:?}"
    );
    assert_eq!(machine.status(), failed_with("update_artifact_integrity"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn artifact_sig_mismatch_leaves_no_file_and_does_not_exec() {
    let (public_key, secret_key) = test_keys();
    let (_other_public_key, other_secret_key) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&other_secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
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
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let outcome = install(&mut machine, &config, &installer).expect("legal");
    assert!(
        matches!(
            outcome,
            InstallOutcome::Failed(UpdateError::ArtifactIntegrity)
        ),
        "{outcome:?}"
    );
    assert_eq!(machine.status(), failed_with("update_artifact_integrity"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn install_from_idle_is_hard_error() {
    let mut machine = UpdateMachine::new();
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(1),
    );
    let (installer, _calls) = spy(false);
    let err = install(&mut machine, &config, &installer).expect_err("idle");
    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(machine.status(), UpdateStatus::Idle);
}

#[test]
fn install_from_failed_is_hard_error() {
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
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
    assert_eq!(machine.check(&config), failed_with("update_network"));
    let (installer, _calls) = spy(false);
    let err = install(&mut machine, &config, &installer).expect_err("failed");
    assert_eq!(err.code(), "update_install_not_allowed");
}

#[test]
fn install_from_checking_is_hard_error() {
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.begin_check(), CheckStart::Started);
    let (public_key, _secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(1),
    );
    let (installer, _calls) = spy(false);
    let err = install(&mut machine, &config, &installer).expect_err("checking");
    assert_eq!(err.code(), "update_install_not_allowed");
}

#[test]
fn successful_install_calls_exec_once() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"install-me";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "ok",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest_once(&server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(1)
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
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    let outcome = install(&mut machine, &config, &installer).expect("legal");
    assert!(
        matches!(outcome, InstallOutcome::Installed(InstallHandoff::Replaced)),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let exec_path = installer
        .last_path
        .lock()
        .expect("path")
        .clone()
        .expect("installer received a path");
    assert!(
        exec_path.starts_with(cache.path()),
        "exec path must be the wrapper-verified cache file, got {}",
        exec_path.display()
    );
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn downloaded_artifact_keeps_the_file_extension_of_its_url() {
    // The installers pick their action from the extension, and Windows will
    // not start a program whose file name has none.
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_newer_release(
        &server,
        &public_key,
        &secret_key,
        "/Oikonomia_0.2.0_x64-setup.exe",
        b"installer",
        cache.path(),
    );
    let (installer, _calls) = spy(false);
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    install(&mut machine, &config, &installer).expect("legal");

    let exec_path = installer
        .last_path
        .lock()
        .expect("path")
        .clone()
        .expect("installer received a path");
    let name = exec_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("file name");
    assert!(
        name.ends_with("-Oikonomia_0.2.0_x64-setup.exe"),
        "unexpected artifact name {name}"
    );
    assert_eq!(exec_path.parent(), Some(cache.path()));
}

#[test]
fn package_managed_copy_reports_the_version_and_refuses_to_install() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_newer_release(
        &server,
        &public_key,
        &secret_key,
        "/Oikonomia.AppImage",
        b"appimage",
        cache.path(),
    )
    .with_install_route(InstallRoute::PackageManager);
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();

    let status = machine.check(&config);

    assert_eq!(
        status,
        UpdateStatus::AvailableManually {
            version: "0.2.0".into(),
            notes: "ok".into(),
        }
    );
    let err = install(&mut machine, &config, &installer)
        .expect_err("a package-managed copy must not install");
    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn download_and_verify_rejects_mismatched_hash_and_leaves_no_file() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(b"nope"),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
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
    let offer = available_offer(&config);
    let err = download_and_verify(&config, &offer).expect_err("hash");
    assert_eq!(err.code(), "update_artifact_integrity");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

/// The feed's sha256 names the tampered bytes, so only the artifact's
/// signature stands between them and the installer.
#[test]
fn artifact_matching_its_hash_but_not_its_signature_is_refused() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let tampered = b"real-bytez";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(tampered),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(tampered.as_slice())),
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
    let offer = available_offer(&config);
    let err = download_and_verify(&config, &offer).expect_err("tampered artifact");
    assert_eq!(err.code(), "update_artifact_integrity");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn artifact_download_may_outlast_the_feed_deadline_while_bytes_keep_arriving() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    // Four writes 300 ms apart: 1.2 s in all, against a 500 ms feed deadline.
    let payload = [0_u8; 4];
    let artifact_address = serve_zero_bytes(payload.len(), 1, Duration::from_millis(300));
    let config = config_for_artifact_at(
        &server,
        (&public_key, &secret_key),
        artifact_address,
        &payload,
        cache.path(),
    );
    let offer = available_offer(&config);

    let path = download_and_verify(&config, &offer).expect("slow download");

    assert_eq!(std::fs::read(&path).expect("read"), payload);
}

#[test]
fn artifact_over_the_size_cap_is_refused_and_leaves_no_file() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let artifact_address = serve_zero_bytes(MAX_ARTIFACT_BYTES + 1, 1 << 20, Duration::ZERO);
    // The manifest describes other bytes: read in full, the download would
    // end as an integrity error, not a size one.
    let config = config_for_artifact_at(
        &server,
        (&public_key, &secret_key),
        artifact_address,
        b"artifact-bytes",
        cache.path(),
    );
    let offer = available_offer(&config);

    let err = download_and_verify(&config, &offer).expect_err("over the cap");

    assert_eq!(err.code(), "update_artifact_too_large");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn a_second_install_during_an_install_is_refused() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (mut machine, _config) =
        machine_with_an_offer(&server, &public_key, &secret_key, cache.path());
    let _offer = machine.begin_install().expect("available");

    let err = machine.begin_install().expect_err("already installing");

    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(machine.status(), UpdateStatus::Installing);
}

#[test]
fn an_install_that_dies_leaves_a_usable_machine() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (mut machine, config) =
        machine_with_an_offer(&server, &public_key, &secret_key, cache.path());
    let offer = machine.begin_install().expect("available");

    // What the caller's drop guard does when the install never returns.
    drop(offer);
    machine.abandon_install();

    assert_eq!(machine.status(), UpdateStatus::Failed { code: None });
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let (installer, calls) = spy(false);
    let outcome = install(&mut machine, &config, &installer).expect("available again");
    assert!(
        matches!(outcome, InstallOutcome::Installed(InstallHandoff::Replaced)),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_finished_install_is_ignored_by_a_machine_that_is_not_installing() {
    let mut machine = UpdateMachine::new();

    machine.finish_install(&InstallOutcome::Failed(UpdateError::Network));
    machine.abandon_install();

    assert_eq!(machine.status(), UpdateStatus::Idle);
}

#[test]
fn an_installer_that_fails_reaches_the_status_as_its_own_code() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (mut machine, config) =
        machine_with_an_offer(&server, &public_key, &secret_key, cache.path());
    // The spy fails at an install step, as the desktop's installer does.
    let (installer, calls) = spy(true);

    let outcome = install(&mut machine, &config, &installer).expect("available");

    assert!(
        matches!(
            outcome,
            InstallOutcome::Failed(UpdateError::InstallFailed {
                step: InstallStep::Replace
            })
        ),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The artifact passed verification, and the code does not say otherwise.
    assert_eq!(machine.status(), failed_with("update_install_failed"));
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn a_download_that_cannot_be_fetched_reaches_the_status_as_a_network_failure() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let payload = b"never-served";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(&secret_key, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
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
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    machine.check(&config);

    let outcome = install(&mut machine, &config, &installer).expect("available");

    assert!(
        matches!(outcome, InstallOutcome::Failed(UpdateError::Network)),
        "{outcome:?}"
    );
    assert_eq!(machine.status(), failed_with("update_network"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn a_machine_stays_installing_after_a_successful_install() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (mut machine, config) =
        machine_with_an_offer(&server, &public_key, &secret_key, cache.path());
    let (installer, _calls) = spy(false);

    let outcome = install(&mut machine, &config, &installer).expect("available");

    assert!(
        matches!(outcome, InstallOutcome::Installed(InstallHandoff::Replaced)),
        "{outcome:?}"
    );
    assert_eq!(machine.status(), UpdateStatus::Installing);
    assert_eq!(machine.begin_check(), CheckStart::InstallInProgress);
}
