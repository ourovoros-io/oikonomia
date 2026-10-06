//! The cache directory: file names, permissions, and cleaning up.

use crate::client::{
    ClientConfig, InstallHandoff, InstallOutcome, delete_artifact, download_and_verify,
    purge_cache, write_new_private_file,
};
use crate::machine::UpdateMachine;
use crate::tests::support::{
    available_offer, cache_dir, captured_warnings, config, install, leftover_files,
    serve_newer_release, serve_signed_manifest, server_url, sign, spy, spy_with_handoff,
    static_manifest, test_keys,
};
use crate::verify::sha256_hex;
use httptest::matchers::request;
use httptest::responders::status_code;
use httptest::{Expectation, Server};
use minisign::SecretKey;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// The bytes of the release [`serve_release_downloaded`] offers.
const PAYLOAD: &[u8] = b"appimage";

/// Serves a signed 0.2.0 manifest offering [`PAYLOAD`] at
/// `/Oikonomia.AppImage`, lets that artifact be downloaded exactly
/// `downloads` times, and returns a config for an app at 0.1.0 and the path
/// the artifact has in `cache`.
fn serve_release_downloaded(
    server: &Server,
    keys: (&str, &SecretKey),
    cache: &Path,
    downloads: usize,
) -> (ClientConfig, PathBuf) {
    let (public_key, secret_key) = keys;
    let body = static_manifest(
        "0.2.0",
        "ok",
        server_url(server, "/Oikonomia.AppImage").as_str(),
        &sign(secret_key, PAYLOAD),
        &sha256_hex(PAYLOAD),
    );
    serve_signed_manifest(server, &body, &sign(secret_key, body.as_bytes()));
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(downloads)
            .respond_with(status_code(200).body(PAYLOAD)),
    );

    let config = config(
        server,
        "/latest.json",
        public_key,
        "0.1.0",
        cache,
        Duration::from_secs(2),
    );
    let destination = cache.join(format!("{}-Oikonomia.AppImage", sha256_hex(PAYLOAD)));
    (config, destination)
}

#[test]
fn a_leftover_holding_the_offered_bytes_is_installed_without_a_second_download() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (config, destination) =
        serve_release_downloaded(&server, (&public_key, &secret_key), cache.path(), 0);
    // What an installer that is still running, or one that crashed, leaves.
    write_new_private_file(&destination, PAYLOAD).expect("leftover");
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    machine.check(&config);

    let outcome = install(&mut machine, &config, &installer).expect("available");

    assert!(
        matches!(outcome, InstallOutcome::Installed(InstallHandoff::Replaced)),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let installed_from = installer.last_path.lock().expect("path").clone();
    assert_eq!(installed_from, Some(destination));
}

#[test]
fn a_leftover_holding_other_bytes_is_replaced_by_the_verified_download() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (config, destination) =
        serve_release_downloaded(&server, (&public_key, &secret_key), cache.path(), 1);
    write_new_private_file(&destination, b"not the artifact").expect("leftover");
    let offer = available_offer(&config);

    let path = download_and_verify(&config, &offer).expect("download");

    assert_eq!(path, destination);
    assert_eq!(std::fs::read(&path).expect("read"), PAYLOAD);
    assert_eq!(leftover_files(cache.path()), vec![destination]);
}

#[test]
fn a_leftover_that_is_not_the_artifact_is_removed_when_the_download_fails() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let body = static_manifest(
        "0.2.0",
        "ok",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&secret_key, PAYLOAD),
        &sha256_hex(PAYLOAD),
    );
    serve_signed_manifest(&server, &body, &sign(&secret_key, body.as_bytes()));
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(503)),
    );
    let config = config(
        &server,
        "/latest.json",
        &public_key,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let destination = cache
        .path()
        .join(format!("{}-Oikonomia.AppImage", sha256_hex(PAYLOAD)));
    write_new_private_file(&destination, b"not the artifact").expect("leftover");
    let offer = available_offer(&config);

    let err = download_and_verify(&config, &offer).expect_err("no download");

    assert_eq!(err.code(), "update_network");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn a_directory_under_the_name_of_the_artifact_fails_the_install_as_a_cache_error() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (config, destination) =
        serve_release_downloaded(&server, (&public_key, &secret_key), cache.path(), 1);
    std::fs::create_dir(&destination).expect("directory");
    std::fs::write(destination.join("inside"), b"x").expect("file inside");
    let offer = available_offer(&config);

    let err = download_and_verify(&config, &offer).expect_err("a directory is in the way");

    assert_eq!(err.code(), "update_cache_io");
    assert_eq!(leftover_files(cache.path()), vec![destination]);
}

// A leftover owned by another account is refused as well, by the owner
// comparison in `is_private_to_owner_of`. No test plants one: changing a
// file's owner takes root.
#[cfg(unix)]
#[test]
fn a_leftover_that_group_or_others_may_write_is_not_reused() {
    use std::os::unix::fs::PermissionsExt;

    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (config, destination) =
        serve_release_downloaded(&server, (&public_key, &secret_key), cache.path(), 1);
    std::fs::write(&destination, PAYLOAD).expect("leftover");
    std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o666)).expect("loosen");
    let offer = available_offer(&config);

    let path = download_and_verify(&config, &offer).expect("download");

    let mode = std::fs::symlink_metadata(&path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(std::fs::read(&path).expect("read"), PAYLOAD);
}

#[cfg(unix)]
#[test]
fn a_link_to_the_offered_bytes_is_replaced_and_never_handed_to_the_installer() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let (config, destination) =
        serve_release_downloaded(&server, (&public_key, &secret_key), cache.path(), 1);
    // The right bytes, but in a file outside the cache that the link's
    // owner can change after they have been verified.
    let outside_dir = cache_dir();
    let outside = outside_dir.path().join("looks-verified");
    std::fs::write(&outside, PAYLOAD).expect("outside file");
    std::os::unix::fs::symlink(&outside, &destination).expect("plant link");
    let offer = available_offer(&config);

    let path = download_and_verify(&config, &offer).expect("download");

    let metadata = std::fs::symlink_metadata(&path).expect("metadata");
    assert!(metadata.file_type().is_file(), "the link was kept");
    assert_eq!(std::fs::read(&path).expect("read"), PAYLOAD);
    assert_eq!(std::fs::read(&outside).expect("outside"), PAYLOAD);
}

#[test]
fn artifact_name_from_a_hostile_url_cannot_leave_the_cache_directory() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    // The last URL segment decodes to `..\..\evil name.exe`.
    let config = serve_newer_release(
        &server,
        &public_key,
        &secret_key,
        "/dir/..%5C..%5Cevil%20name.exe",
        b"installer",
        cache.path(),
    );
    let offer = available_offer(&config);
    let path = download_and_verify(&config, &offer).expect("download");

    assert_eq!(path.parent(), Some(cache.path()));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("file name");
    assert!(
        name.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_'),
        "unexpected artifact name {name}"
    );
    assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("exe"));
}

#[test]
fn a_running_installer_keeps_its_artifact_and_the_next_install_clears_the_rest() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_newer_release(
        &server,
        &public_key,
        &secret_key,
        "/Oikonomia-setup.exe",
        b"installer",
        cache.path(),
    );
    let (installer, _calls) = spy_with_handoff(false, InstallHandoff::InstallerStarted);
    let mut machine = UpdateMachine::new();
    machine.check(&config);

    let outcome = install(&mut machine, &config, &installer).expect("legal");

    assert!(
        matches!(
            outcome,
            InstallOutcome::Installed(InstallHandoff::InstallerStarted)
        ),
        "{outcome:?}"
    );
    assert_eq!(leftover_files(cache.path()).len(), 1);

    let stale = cache.path().join("stale-from-an-earlier-install.exe");
    std::fs::write(&stale, b"old").expect("stale file");
    let offer = available_offer(&config);
    let kept = leftover_files(cache.path())
        .into_iter()
        .find(|path| *path != stale)
        .expect("the installer's artifact");
    let fresh = download_and_verify(&config, &offer).expect("download");
    assert_eq!(fresh, kept);
    assert_eq!(leftover_files(cache.path()), vec![fresh]);
}

#[cfg(unix)]
#[test]
fn cache_is_private_and_a_planted_link_cannot_redirect_the_download() {
    use std::os::unix::fs::PermissionsExt;

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
    );
    let offer = available_offer(&config);

    // Someone planted a link where the artifact will be written, pointing
    // at a file outside the cache, and loosened the directory.
    let outside_dir = cache_dir();
    let outside = outside_dir.path().join("victim");
    let first = download_and_verify(&config, &offer).expect("first download");
    let planted = first.clone();
    std::fs::remove_file(&first).expect("clear");
    std::os::unix::fs::symlink(&outside, &planted).expect("plant link");
    std::fs::set_permissions(cache.path(), std::fs::Permissions::from_mode(0o777)).expect("loosen");

    let path = download_and_verify(&config, &offer).expect("download");

    assert!(!outside.exists(), "the download followed the planted link");
    let metadata = std::fs::symlink_metadata(&path).expect("metadata");
    assert!(metadata.file_type().is_file());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read(&path).expect("read"), b"appimage");
    let cache_mode = std::fs::metadata(cache.path())
        .expect("cache")
        .permissions()
        .mode();
    assert_eq!(cache_mode & 0o777, 0o700);
}

#[test]
fn delete_artifact_removes_the_file() {
    let cache = cache_dir();
    let artifact = cache.path().join("Oikonomia.AppImage");
    std::fs::write(&artifact, b"verified").expect("artifact");

    delete_artifact(&artifact);

    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn delete_artifact_says_nothing_about_a_file_that_is_already_gone() {
    let warnings = captured_warnings();
    let cache = cache_dir();
    let missing = cache.path().join("never-written.AppImage");

    delete_artifact(&missing);

    assert_eq!(warnings.mentioning(&missing), Vec::<String>::new());
}

#[test]
fn delete_artifact_logs_a_removal_that_fails() {
    let warnings = captured_warnings();
    let cache = cache_dir();
    // `remove_file` refuses a directory, with an error other than `NotFound`.
    let directory = cache.path().join("not-a-file");
    std::fs::create_dir(&directory).expect("directory");

    delete_artifact(&directory);

    assert_eq!(warnings.mentioning(&directory).len(), 1);
}

#[test]
fn a_cache_directory_that_cannot_be_created_fails_as_a_cache_error() {
    let (public_key, secret_key) = test_keys();
    let server = Server::run();
    let parent = cache_dir();
    // A file where the cache directory should be: `create_dir_all` refuses.
    let blocked = parent.path().join("updater");
    std::fs::write(&blocked, b"not a directory").expect("file");
    let config = serve_newer_release(
        &server,
        &public_key,
        &secret_key,
        "/Oikonomia.AppImage",
        b"appimage",
        &blocked,
    );
    let offer = available_offer(&config);

    let err = download_and_verify(&config, &offer).expect_err("no cache directory");

    assert_eq!(err.code(), "update_cache_io");
    assert!(std::error::Error::source(&err).is_some());
}

#[cfg(unix)]
#[test]
fn purging_the_cache_logs_a_file_it_cannot_remove() {
    use std::os::unix::fs::PermissionsExt;

    let warnings = captured_warnings();
    let cache = cache_dir();
    let stuck = cache.path().join("stuck-installer.AppImage");
    std::fs::write(&stuck, b"old").expect("leftover file");
    // Without write permission on the directory nothing in it can be removed.
    std::fs::set_permissions(cache.path(), std::fs::Permissions::from_mode(0o500))
        .expect("read-only directory");

    purge_cache(cache.path(), &cache.path().join("artifact-in-flight"));

    std::fs::set_permissions(cache.path(), std::fs::Permissions::from_mode(0o700))
        .expect("restore the directory");
    if !stuck.exists() {
        // Root ignores directory permissions, so the removal succeeded and
        // there is no failure to observe.
        return;
    }
    assert_eq!(warnings.mentioning(&stuck).len(), 1);
}

#[test]
fn purging_the_cache_removes_files_and_says_nothing() {
    let warnings = captured_warnings();
    let cache = cache_dir();
    let leftover = cache.path().join("leftover.AppImage");
    std::fs::write(&leftover, b"old").expect("leftover file");
    let kept = cache.path().join("a-directory");
    std::fs::create_dir(&kept).expect("directory");

    purge_cache(cache.path(), &cache.path().join("artifact-in-flight"));

    assert_eq!(leftover_files(cache.path()), vec![kept.clone()]);
    assert_eq!(warnings.mentioning(&leftover), Vec::<String>::new());
    assert_eq!(warnings.mentioning(&kept), Vec::<String>::new());
}

#[test]
fn purging_the_cache_spares_the_file_under_the_name_of_the_artifact_in_flight() {
    let cache = cache_dir();
    let in_flight = cache.path().join("digest-Oikonomia.AppImage");
    std::fs::write(&in_flight, b"left by an earlier attempt").expect("same release");
    let other = cache.path().join("other-digest-Oikonomia.AppImage");
    std::fs::write(&other, b"old").expect("another release");

    purge_cache(cache.path(), &in_flight);

    assert_eq!(leftover_files(cache.path()), vec![in_flight]);
}
