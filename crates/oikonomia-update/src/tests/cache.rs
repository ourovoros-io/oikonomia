//! The cache directory: file names, permissions, and cleaning up.

use crate::client::{
    InstallHandoff, InstallOutcome, delete_artifact, download_and_verify, purge_cache,
};
use crate::machine::UpdateMachine;
use crate::tests::support::{
    available_offer, cache_dir, captured_warnings, install, leftover_files, serve_newer_release,
    spy_with_handoff, test_keys,
};
use httptest::Server;
use std::path::PathBuf;

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
fn a_running_installer_keeps_its_artifact_and_the_next_download_clears_it() {
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

    assert_eq!(
        outcome,
        InstallOutcome::Installed(InstallHandoff::InstallerStarted)
    );
    assert_eq!(leftover_files(cache.path()).len(), 1);

    let stale = cache.path().join("stale-from-an-earlier-install.exe");
    std::fs::write(&stale, b"old").expect("stale file");
    let offer = available_offer(&config);
    let fresh = download_and_verify(&config, &offer).expect("download");
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

    purge_cache(cache.path());

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

    purge_cache(cache.path());

    assert_eq!(leftover_files(cache.path()), vec![kept.clone()]);
    assert_eq!(warnings.mentioning(&leftover), Vec::<String>::new());
    assert_eq!(warnings.mentioning(&kept), Vec::<String>::new());
}
