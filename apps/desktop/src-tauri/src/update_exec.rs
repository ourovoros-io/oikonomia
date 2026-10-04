//! Exec the already-verified updater artifact. No HTTP. No plugin `check`.

use oikonomia_update::{ArtifactInstaller, InstallHandoff, InstallRoute, Result, UpdateError};
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;

/// How this copy of the app got onto the machine, which decides how (and
/// whether) it may replace itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallKind {
    /// A macOS `.app` bundle, replaced from a `.app.tar.gz`.
    MacApp,
    /// A Linux `AppImage` at this path, replaced by the new `AppImage`.
    AppImage(PathBuf),
    /// A Windows per-user install, replaced by running the new installer.
    WindowsInstaller,
    /// Files owned by the system package manager (a `.deb`), or a build run
    /// straight from a source tree. Never replaced by the app.
    PackageManaged,
}

impl InstallKind {
    /// The kind of the running copy.
    pub(crate) fn detect() -> Self {
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        Self::classify(std::env::consts::OS, appimage)
    }

    /// `appimage` is the `APPIMAGE` variable the `AppImage` runtime sets to
    /// the path of the mounted image. A Linux copy without it was installed
    /// from a package (or built locally) and does not own its own files.
    fn classify(os: &str, appimage: Option<PathBuf>) -> Self {
        match os {
            "macos" => Self::MacApp,
            "windows" => Self::WindowsInstaller,
            "linux" => match appimage {
                Some(path) if path.is_file() => Self::AppImage(path),
                Some(_) | None => Self::PackageManaged,
            },
            _ => Self::PackageManaged,
        }
    }

    /// Whether the update machine may offer an in-app install.
    pub(crate) fn route(&self) -> InstallRoute {
        match self {
            Self::MacApp | Self::AppImage(_) | Self::WindowsInstaller => InstallRoute::InApp,
            Self::PackageManaged => InstallRoute::PackageManager,
        }
    }
}

/// Installs by execing the local verified file. Never fetches `latest.json`.
pub(crate) struct VerifiedPathInstaller {
    kind: InstallKind,
}

impl VerifiedPathInstaller {
    /// Installer for a copy of the given kind.
    pub(crate) fn new(kind: InstallKind) -> Self {
        Self { kind }
    }
}

impl ArtifactInstaller for VerifiedPathInstaller {
    fn install(&self, artifact: &Path) -> Result<InstallHandoff> {
        install_verified_artifact(&self.kind, artifact)
    }
}

/// Runs the installer that matches `kind` on `artifact`. The path must
/// already be verified.
///
/// # Errors
///
/// Returns [`UpdateError::InstallNotAvailable`] for a package-managed copy,
/// [`UpdateError::ArtifactUrl`] when the artifact is not the file type this
/// kind installs, and [`UpdateError::ArtifactIntegrity`] when the file is
/// missing or the installer fails.
pub(crate) fn install_verified_artifact(
    kind: &InstallKind,
    artifact: &Path,
) -> Result<InstallHandoff> {
    let expected_suffix = match kind {
        InstallKind::MacApp => ".app.tar.gz",
        InstallKind::AppImage(_) => ".appimage",
        InstallKind::WindowsInstaller => ".exe",
        InstallKind::PackageManaged => return Err(UpdateError::InstallNotAvailable),
    };
    if !path_ends_with_ignore_ascii_case(artifact, expected_suffix) {
        return Err(UpdateError::ArtifactUrl);
    }
    if !artifact.is_file() {
        return Err(UpdateError::ArtifactIntegrity);
    }

    run_platform_installer(kind, artifact)
}

#[cfg(target_os = "linux")]
fn run_platform_installer(kind: &InstallKind, artifact: &Path) -> Result<InstallHandoff> {
    let InstallKind::AppImage(current) = kind else {
        return Err(UpdateError::InstallNotAvailable);
    };
    replace_linux_appimage(artifact, current)?;
    Ok(InstallHandoff::Replaced)
}

#[cfg(target_os = "windows")]
fn run_platform_installer(kind: &InstallKind, artifact: &Path) -> Result<InstallHandoff> {
    let InstallKind::WindowsInstaller = kind else {
        return Err(UpdateError::InstallNotAvailable);
    };
    spawn_windows_installer(artifact)?;
    Ok(InstallHandoff::InstallerStarted)
}

#[cfg(target_os = "macos")]
fn run_platform_installer(kind: &InstallKind, artifact: &Path) -> Result<InstallHandoff> {
    let InstallKind::MacApp = kind else {
        return Err(UpdateError::InstallNotAvailable);
    };
    let current = std::env::current_exe().map_err(|err| {
        log::warn!("current exe path failed: {err}");
        UpdateError::ArtifactIntegrity
    })?;
    extract_macos_app_archive(artifact, &current)?;
    Ok(InstallHandoff::Replaced)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn run_platform_installer(_kind: &InstallKind, _artifact: &Path) -> Result<InstallHandoff> {
    Err(UpdateError::InstallNotAvailable)
}

#[cfg(target_os = "linux")]
fn replace_linux_appimage(verified: &Path, current: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let parent = current.parent().ok_or(UpdateError::ArtifactIntegrity)?;
    let file_name = current
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(UpdateError::ArtifactIntegrity)?;
    let staging = parent.join(format!(".{file_name}.staging"));
    let backup = parent.join(format!(".{file_name}.previous"));

    if let Err(err) = std::fs::copy(verified, &staging) {
        log::warn!("verified appimage copy failed: {err}");
        return Err(UpdateError::ArtifactIntegrity);
    }
    let mut permissions = std::fs::metadata(&staging)
        .map_err(|_| UpdateError::ArtifactIntegrity)?
        .permissions();
    permissions.set_mode(0o755);
    if let Err(err) = std::fs::set_permissions(&staging, permissions) {
        let _ = std::fs::remove_file(&staging);
        log::warn!("verified appimage chmod failed: {err}");
        return Err(UpdateError::ArtifactIntegrity);
    }

    if current.exists()
        && let Err(err) = std::fs::rename(current, &backup)
    {
        let _ = std::fs::remove_file(&staging);
        log::warn!("current appimage backup failed: {err}");
        return Err(UpdateError::ArtifactIntegrity);
    }
    if let Err(err) = std::fs::rename(&staging, current) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, current);
        }
        let _ = std::fs::remove_file(&staging);
        log::warn!("appimage replace failed: {err}");
        return Err(UpdateError::ArtifactIntegrity);
    }
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

/// Flags for the NSIS installer Tauri builds: `/P` shows progress only and
/// asks nothing, `/UPDATE` keeps the existing install's choices, and `/R`
/// starts the new version when the install finishes.
#[cfg(target_os = "windows")]
const NSIS_UPDATE_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];

/// Starts the installer and returns at once. The installer replaces files the
/// running app holds open, so the caller must exit, not restart. The install
/// is per user (`bundle.windows.nsis.installMode`), so it needs no elevation
/// and a plain process spawn is enough.
#[cfg(target_os = "windows")]
fn spawn_windows_installer(artifact: &Path) -> Result<()> {
    Command::new(artifact)
        .args(NSIS_UPDATE_ARGS)
        .spawn()
        .map_err(|err| {
            log::warn!("windows installer spawn failed: {err}");
            UpdateError::ArtifactIntegrity
        })?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn extract_macos_app_archive(artifact: &Path, current: &Path) -> Result<()> {
    let extract_root = current
        .parent()
        .ok_or(UpdateError::ArtifactIntegrity)?
        .join(".oikonomia-update-extract");
    let _ = std::fs::remove_dir_all(&extract_root);
    std::fs::create_dir_all(&extract_root).map_err(|_| UpdateError::ArtifactIntegrity)?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(artifact)
        .arg("-C")
        .arg(&extract_root)
        .status()
        .map_err(|err| {
            log::warn!("macos tar extract failed: {err}");
            UpdateError::ArtifactIntegrity
        })?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&extract_root);
        return Err(UpdateError::ArtifactIntegrity);
    }
    let new_app = first_app_bundle(&extract_root).ok_or(UpdateError::ArtifactIntegrity)?;
    let dest = macos_app_bundle_path(current)?;
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|_| UpdateError::ArtifactIntegrity)?;
    }
    let move_result = std::fs::rename(&new_app, &dest);
    let _ = std::fs::remove_dir_all(&extract_root);
    move_result.map_err(|err| {
        log::warn!("macos app replace failed: {err}");
        UpdateError::ArtifactIntegrity
    })
}

#[cfg(target_os = "macos")]
fn first_app_bundle(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("app") {
            return Some(path);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_path(executable: &Path) -> Result<PathBuf> {
    let mut path = executable.to_path_buf();
    for _ in 0..3 {
        if path.extension().and_then(|ext| ext.to_str()) == Some("app") {
            return Ok(path);
        }
        match path.parent() {
            Some(parent) => path = parent.to_path_buf(),
            None => break,
        }
    }
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or(UpdateError::ArtifactIntegrity)
}

fn path_ends_with_ignore_ascii_case(path: &Path, suffix: &str) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.as_bytes();
    let suffix = suffix.as_bytes();
    if name.len() < suffix.len() {
        return false;
    }
    let start = name.len() - suffix.len();
    name[start..].eq_ignore_ascii_case(suffix)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{InstallKind, VerifiedPathInstaller, install_verified_artifact};
    use oikonomia_update::{ArtifactInstaller, InstallRoute};
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oiko-update-exec-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn installable_kinds() -> [InstallKind; 3] {
        [
            InstallKind::MacApp,
            InstallKind::AppImage(PathBuf::from("Oikonomia.AppImage")),
            InstallKind::WindowsInstaller,
        ]
    }

    #[test]
    fn linux_without_an_appimage_is_package_managed() {
        let dir = temp_dir();
        let appimage = dir.join("Oikonomia.AppImage");
        std::fs::write(&appimage, b"image").expect("appimage");

        assert_eq!(
            InstallKind::classify("linux", None),
            InstallKind::PackageManaged
        );
        // A stale variable naming a file that is gone must not enable installs.
        assert_eq!(
            InstallKind::classify("linux", Some(dir.join("gone.AppImage"))),
            InstallKind::PackageManaged
        );
        assert_eq!(
            InstallKind::classify("linux", Some(appimage.clone())),
            InstallKind::AppImage(appimage)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn macos_and_windows_ignore_the_appimage_variable() {
        let stray = Some(PathBuf::from("Oikonomia.AppImage"));

        assert_eq!(
            InstallKind::classify("macos", stray.clone()),
            InstallKind::MacApp
        );
        assert_eq!(
            InstallKind::classify("windows", stray),
            InstallKind::WindowsInstaller
        );
        assert_eq!(
            InstallKind::classify("freebsd", None),
            InstallKind::PackageManaged
        );
    }

    #[test]
    fn only_a_package_managed_copy_is_routed_to_the_package_manager() {
        for kind in installable_kinds() {
            assert_eq!(kind.route(), InstallRoute::InApp, "{kind:?}");
        }
        assert_eq!(
            InstallKind::PackageManaged.route(),
            InstallRoute::PackageManager
        );
    }

    #[test]
    fn package_managed_copy_never_runs_an_artifact() {
        let dir = temp_dir();
        for name in ["Oikonomia.AppImage", "oikonomia.deb", "setup.exe"] {
            let artifact = dir.join(name);
            std::fs::write(&artifact, b"payload").expect("write");

            let err = VerifiedPathInstaller::new(InstallKind::PackageManaged)
                .install(&artifact)
                .expect_err("package-managed copies do not self-install");

            assert_eq!(err.code(), "update_install_not_allowed");
            assert_eq!(std::fs::read(&artifact).expect("kept"), b"payload");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_kind_rejects_a_deb_without_running_it() {
        let dir = temp_dir();
        let deb = dir.join("oikonomia.deb");
        std::fs::write(&deb, b"not-an-installer").expect("write");

        for kind in installable_kinds() {
            let err = install_verified_artifact(&kind, &deb).expect_err("deb must be rejected");
            assert_eq!(err.code(), "update_artifact_url", "{kind:?}");
        }

        assert_eq!(std::fs::read(&deb).expect("kept"), b"not-an-installer");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_kind_accepts_only_its_own_file_type() {
        let dir = temp_dir();
        let wrong_for = [
            (InstallKind::MacApp, "Oikonomia-setup.exe"),
            (
                InstallKind::AppImage(dir.join("current.AppImage")),
                "Oikonomia.app.tar.gz",
            ),
            (InstallKind::WindowsInstaller, "Oikonomia.AppImage"),
        ];

        for (kind, name) in wrong_for {
            let artifact = dir.join(name);
            std::fs::write(&artifact, b"payload").expect("write");

            let err = install_verified_artifact(&kind, &artifact).expect_err("wrong file type");

            assert_eq!(err.code(), "update_artifact_url", "{kind:?} given {name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_artifact_is_an_integrity_failure() {
        let dir = temp_dir();
        let cases = [
            (InstallKind::MacApp, "gone.app.tar.gz"),
            (
                InstallKind::AppImage(dir.join("current.AppImage")),
                "gone.AppImage",
            ),
            (InstallKind::WindowsInstaller, "gone.exe"),
        ];

        for (kind, name) in cases {
            let err = install_verified_artifact(&kind, &dir.join(name)).expect_err("missing");
            assert_eq!(err.code(), "update_artifact_integrity", "{kind:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_replace_writes_verified_bytes_onto_current() {
        use oikonomia_update::InstallHandoff;
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir();
        let current = dir.join("Oikonomia.AppImage");
        let verified = dir.join("abc-Oikonomia_0.2.0_amd64.AppImage");
        std::fs::write(&current, b"old-appimage").expect("current");
        std::fs::write(&verified, b"new-verified").expect("verified");

        let handoff = install_verified_artifact(&InstallKind::AppImage(current.clone()), &verified)
            .expect("replace");

        assert_eq!(handoff, InstallHandoff::Replaced);
        assert_eq!(std::fs::read(&current).expect("read"), b"new-verified");
        let mode = std::fs::metadata(&current)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the new AppImage must be executable");
        assert!(verified.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_installer_runs_unattended_and_relaunches_the_app() {
        assert_eq!(super::NSIS_UPDATE_ARGS, ["/P", "/UPDATE", "/R"]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_install_hands_off_to_the_installer_process() {
        use oikonomia_update::InstallHandoff;

        // `whoami.exe` ignores the installer flags and exits; it stands in
        // for the installer so the spawn itself is exercised.
        let dir = temp_dir();
        let system_root = std::env::var("SYSTEMROOT").expect("SYSTEMROOT");
        let stand_in = dir.join("abc-Oikonomia_0.2.0_x64-setup.exe");
        std::fs::copy(
            PathBuf::from(system_root)
                .join("System32")
                .join("whoami.exe"),
            &stand_in,
        )
        .expect("copy stand-in");

        let handoff =
            install_verified_artifact(&InstallKind::WindowsInstaller, &stand_in).expect("spawn");

        assert_eq!(handoff, InstallHandoff::InstallerStarted);
    }
}
