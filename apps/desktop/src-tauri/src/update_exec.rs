//! Exec the already-verified updater artifact. No HTTP. No plugin `check`.

use oikonomia_update::{ArtifactInstaller, InstallHandoff, InstallRoute, Result, UpdateError};
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;

/// How this copy of the app got onto the machine, which decides how (and
/// whether) it may replace itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallKind {
    /// Any copy on macOS. One that runs from an `.app` bundle is replaced
    /// from a `.app.tar.gz`; for one that does not, such as a build run from
    /// a source tree, the install fails (`macos_app_bundle_path`).
    MacApp,
    /// A Linux `AppImage` at this path, replaced by the new `AppImage`.
    AppImage(PathBuf),
    /// Any copy on Windows, updated by running the new per-user installer.
    WindowsInstaller,
    /// A copy the app never replaces. On Linux that is every copy that does
    /// not run from inside an `AppImage`: files owned by the system package
    /// manager (a `.deb`), or a build run from a source tree. On a system
    /// other than macOS, Windows and Linux it is every copy.
    PackageManaged,
}

impl InstallKind {
    /// The kind of the running copy. Only Linux looks at how the copy runs;
    /// macOS and Windows are classified by the system alone.
    pub(crate) fn detect() -> Self {
        let runtime = AppImageRuntime {
            image: std::env::var_os("APPIMAGE").map(PathBuf::from),
            mount: std::env::var_os("APPDIR").map(PathBuf::from),
            executable: std::env::current_exe().ok(),
        };
        Self::classify(std::env::consts::OS, &runtime)
    }

    /// A Linux copy is an `AppImage` only when it runs from inside one.
    /// Otherwise it was installed from a package (or built locally) and does
    /// not own its own files.
    fn classify(os: &str, runtime: &AppImageRuntime) -> Self {
        match os {
            "macos" => Self::MacApp,
            "windows" => Self::WindowsInstaller,
            "linux" => match runtime.own_image() {
                Some(image) => Self::AppImage(image),
                None => Self::PackageManaged,
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

/// What the `AppImage` runtime tells a process it started.
struct AppImageRuntime {
    /// `APPIMAGE`: path of the image file.
    image: Option<PathBuf>,
    /// `APPDIR`: directory the image is mounted or unpacked at.
    mount: Option<PathBuf>,
    /// The running executable.
    executable: Option<PathBuf>,
}

impl AppImageRuntime {
    /// The image this process runs from, if any.
    ///
    /// `APPIMAGE` alone proves nothing: a program started from another
    /// `AppImage` (a terminal, a launcher) inherits that one's variables, and
    /// trusting them would make an update overwrite the other program. The
    /// running executable must sit inside the mounted image.
    ///
    /// Both paths are resolved first: the executable path the system reports
    /// has its links resolved, the mount variable may not, and comparing the
    /// two unresolved would turn updates off wherever `/tmp` is a link. A
    /// mount that is empty or the filesystem root would contain every
    /// executable, so it proves nothing and is refused.
    fn own_image(&self) -> Option<PathBuf> {
        let image = self.image.as_ref()?;
        let mount = self.mount.as_ref()?.canonicalize().ok()?;
        let executable = self.executable.as_ref()?.canonicalize().ok()?;

        let mount_is_a_real_directory = mount.parent().is_some();

        if image.is_file() && mount_is_a_real_directory && executable.starts_with(&mount) {
            Some(image.clone())
        } else {
            None
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

/// Replaces the image at `current` with the verified download.
///
/// The new image is written to a staging file next to `current` and moved
/// over it with a single `rename`, which replaces the target atomically: the
/// launch path names the old image or the complete new one, never nothing.
/// Setting the old image aside first would open a gap in which a crash leaves
/// no image there at all.
///
/// Compiled for tests on every Unix so the replacement is exercised on a
/// development machine; only a Linux `AppImage` copy reaches it otherwise.
#[cfg(any(target_os = "linux", all(test, unix)))]
fn replace_linux_appimage(verified: &Path, current: &Path) -> Result<()> {
    let parent = current.parent().ok_or(UpdateError::ArtifactIntegrity)?;
    let file_name = current
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(UpdateError::ArtifactIntegrity)?;
    let staging = parent.join(format!(".{file_name}.staging"));

    stage_and_swap_appimage(verified, &staging, current).map_err(|err| {
        log::warn!("appimage replace failed: {err}");
        UpdateError::ArtifactIntegrity
    })
}

/// Copies `verified` into a new file at `staging`, then renames it over
/// `current`. On any failure the staging file is removed again.
#[cfg(any(target_os = "linux", all(test, unix)))]
fn stage_and_swap_appimage(verified: &Path, staging: &Path, current: &Path) -> std::io::Result<()> {
    let mut source = std::fs::File::open(verified)?;
    let mut staged = StagingFile::create(staging)?;

    std::io::copy(&mut source, &mut staged.file)?;
    // Without this a power cut shortly after the rename could leave a
    // truncated image under the launch path.
    staged.file.sync_all()?;

    staged.rename_over(current)
}

/// The file a new image is staged in, removed on drop unless it was renamed
/// into place, so no error path leaves a partial image behind.
#[cfg(any(target_os = "linux", all(test, unix)))]
struct StagingFile<'a> {
    /// Where the file was created.
    path: &'a Path,
    /// The open file, created by this process.
    file: std::fs::File,
    /// Whether the file has been moved to its final name.
    renamed: bool,
}

#[cfg(any(target_os = "linux", all(test, unix)))]
impl<'a> StagingFile<'a> {
    /// Creates an empty executable file at `path`.
    ///
    /// The name is fixed, so whatever sits there is a leftover of an
    /// interrupted update or was planted. It is removed, and the file is then
    /// created with `create_new`, which fails on an existing name and never
    /// follows a link, so the bytes cannot be written through one.
    fn create(path: &'a Path) -> std::io::Result<Self> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        remove_file_if_present(path)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(path)?;
        let staged = Self {
            path,
            file,
            renamed: false,
        };

        // The process umask may have cleared bits of the mode asked for
        // above. Set on the handle, so the path is not resolved again.
        staged
            .file
            .set_permissions(std::fs::Permissions::from_mode(0o755))?;
        Ok(staged)
    }

    /// Moves the file over `target`. After that it is no longer a staging
    /// file, and dropping this removes nothing.
    fn rename_over(mut self, target: &Path) -> std::io::Result<()> {
        std::fs::rename(self.path, target)?;
        self.renamed = true;
        Ok(())
    }
}

#[cfg(any(target_os = "linux", all(test, unix)))]
impl Drop for StagingFile<'_> {
    fn drop(&mut self) {
        if self.renamed {
            return;
        }
        if let Err(err) = remove_file_if_present(self.path) {
            log::warn!("could not remove the staged appimage: {err}");
        }
    }
}

/// Removes the file or link at `path`; a missing one is not an error.
#[cfg(any(target_os = "linux", all(test, unix)))]
fn remove_file_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        Ok(()) | Err(_) => Ok(()),
    }
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

/// Name the old bundle is set aside under while the new one moves in.
#[cfg(target_os = "macos")]
const PREVIOUS_BUNDLE: &str = ".oikonomia-update-previous.app";

/// The `tar` that ships with macOS, named by its full path.
///
/// A bare `tar` is looked up through the `PATH` the app inherited, so
/// whichever directory comes first there would choose the program that
/// unpacks the update over the installed app.
#[cfg(target_os = "macos")]
const MACOS_TAR: &str = "/usr/bin/tar";

/// Replaces the app bundle that holds `current` with the one in `artifact`.
///
/// The archive is unpacked next to the bundle, never inside it: the bundle
/// is about to be moved away. The old bundle is set aside first and put back
/// if the new one cannot take its place, so a failed update leaves the
/// installed app as it was.
#[cfg(target_os = "macos")]
fn extract_macos_app_archive(artifact: &Path, current: &Path) -> Result<()> {
    // Resolve links first: an `Oikonomia.app` that is a link must have the
    // directory it points at replaced, not the link.
    let current = current
        .canonicalize()
        .map_err(|_| UpdateError::ArtifactIntegrity)?;
    let bundle = macos_app_bundle_path(&current)?;
    let applications = bundle.parent().ok_or(UpdateError::ArtifactIntegrity)?;
    let extract_root = applications.join(".oikonomia-update-extract");
    let previous = applications.join(PREVIOUS_BUNDLE);

    // After an update that died half-way the only copy may be the one set
    // aside, and it may be the one running now. Clearing it would delete
    // the app; the user has to move it back first.
    if bundle == previous {
        return Err(UpdateError::InstallNotAvailable);
    }

    let executable = current
        .strip_prefix(&bundle)
        .map_err(|_| UpdateError::ArtifactIntegrity)?;
    let _ = std::fs::remove_dir_all(&extract_root);

    let result = unpack_app_bundle(artifact, &extract_root, executable).and_then(|new_app| {
        // `bundle` exists (this process runs from it), so anything at
        // `previous` is a stale leftover.
        let _ = std::fs::remove_dir_all(&previous);
        swap_app_bundle(&new_app, &bundle, &previous)
    });

    let _ = std::fs::remove_dir_all(&extract_root);
    result
}

/// Unpacks `artifact` into `extract_root` and returns the app bundle in it.
///
/// The archive must hold exactly one bundle, and that bundle must have the
/// running app's executable at the same place (`Contents/MacOS/<name>`).
/// Anything else is refused before the installed app is touched.
#[cfg(target_os = "macos")]
fn unpack_app_bundle(artifact: &Path, extract_root: &Path, executable: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(extract_root).map_err(|_| UpdateError::ArtifactIntegrity)?;

    let status = Command::new(MACOS_TAR)
        .arg("-xzf")
        .arg(artifact)
        .arg("-C")
        .arg(extract_root)
        .status()
        .map_err(|err| {
            log::warn!("macos tar extract failed: {err}");
            UpdateError::ArtifactIntegrity
        })?;
    if !status.success() {
        return Err(UpdateError::ArtifactIntegrity);
    }

    let new_app = only_app_bundle(extract_root).ok_or(UpdateError::ArtifactIntegrity)?;
    if !new_app.join(executable).is_file() {
        log::warn!("macos update archive holds no {}", executable.display());
        return Err(UpdateError::ArtifactIntegrity);
    }
    Ok(new_app)
}

/// Moves `bundle` aside to `previous`, moves `new_app` into its place, and
/// deletes the old one. If the new bundle cannot be moved in, the old one is
/// moved back.
#[cfg(target_os = "macos")]
fn swap_app_bundle(new_app: &Path, bundle: &Path, previous: &Path) -> Result<()> {
    std::fs::rename(bundle, previous).map_err(|err| {
        log::warn!("macos app set-aside failed: {err}");
        UpdateError::ArtifactIntegrity
    })?;

    if let Err(err) = std::fs::rename(new_app, bundle) {
        log::warn!("macos app replace failed: {err}");
        if let Err(err) = std::fs::rename(previous, bundle) {
            log::error!("macos app restore failed: {err}");
        }
        return Err(UpdateError::ArtifactIntegrity);
    }

    let _ = std::fs::remove_dir_all(previous);
    Ok(())
}

/// The one `.app` directory directly inside `root`; none or several is an
/// archive this code does not know how to install.
#[cfg(target_os = "macos")]
fn only_app_bundle(root: &Path) -> Option<PathBuf> {
    let mut bundles = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "app") && path.is_dir()
        });

    let bundle = bundles.next()?;
    bundles.next().is_none().then_some(bundle)
}

/// The `.app` bundle whose `Contents/MacOS` directory holds `executable`.
///
/// Only that exact layout counts. A binary anywhere else (a development
/// build, even one under a directory that happens to end in `.app`) has
/// nothing that may be replaced, so that is an error, not a guess.
#[cfg(target_os = "macos")]
fn macos_app_bundle_path(executable: &Path) -> Result<PathBuf> {
    let macos_dir = executable.parent().ok_or(UpdateError::ArtifactIntegrity)?;
    let contents = macos_dir.parent().ok_or(UpdateError::ArtifactIntegrity)?;
    let bundle = contents.parent().ok_or(UpdateError::ArtifactIntegrity)?;

    let is_bundle_layout = macos_dir.ends_with("Contents/MacOS")
        && bundle
            .extension()
            .is_some_and(|extension| extension == "app");
    if is_bundle_layout {
        Ok(bundle.to_path_buf())
    } else {
        Err(UpdateError::ArtifactIntegrity)
    }
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
mod tests {
    use super::{AppImageRuntime, InstallKind, VerifiedPathInstaller, install_verified_artifact};
    use oikonomia_update::{ArtifactInstaller, InstallRoute};
    use std::path::PathBuf;

    /// A fresh directory per call. The counter matters: tests run in
    /// parallel and the clock is too coarse to tell two calls apart, so two
    /// tests once shared a directory and one deleted it under the other.
    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT: AtomicU64 = AtomicU64::new(0);

        let dir = std::env::temp_dir().join(format!(
            "oiko-update-exec-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
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

    const NO_APPIMAGE: AppImageRuntime = AppImageRuntime {
        image: None,
        mount: None,
        executable: None,
    };

    /// A directory laid out like a mounted image, with the app's executable
    /// in it, and the image file next to it.
    struct MountedImage {
        dir: PathBuf,
        image: PathBuf,
        mount: PathBuf,
        executable: PathBuf,
    }

    fn mounted_image() -> MountedImage {
        let dir = temp_dir();
        let image = dir.join("Oikonomia.AppImage");
        let mount = dir.join(".mount_Oikonoabc");
        let executable = mount.join("usr").join("bin").join("oikonomia");
        std::fs::write(&image, b"image").expect("image");
        std::fs::create_dir_all(executable.parent().expect("parent")).expect("mount");
        std::fs::write(&executable, b"exe").expect("executable");
        MountedImage {
            dir,
            image,
            mount,
            executable,
        }
    }

    #[test]
    fn linux_copy_running_from_inside_its_image_is_an_appimage() {
        let mounted = mounted_image();
        let runtime = AppImageRuntime {
            image: Some(mounted.image.clone()),
            mount: Some(mounted.mount.clone()),
            executable: Some(mounted.executable.clone()),
        };

        assert_eq!(
            InstallKind::classify("linux", &runtime),
            InstallKind::AppImage(mounted.image.clone())
        );
        let _ = std::fs::remove_dir_all(&mounted.dir);
    }

    #[cfg(unix)]
    #[test]
    fn appimage_is_recognised_when_the_mount_is_named_through_a_link() {
        // `/tmp` is a link on some systems: the runtime names the mount by
        // the link, the system reports the executable by its real path.
        let mounted = mounted_image();
        let link = mounted.dir.join("link-to-mount");
        std::os::unix::fs::symlink(&mounted.mount, &link).expect("link");
        let runtime = AppImageRuntime {
            image: Some(mounted.image.clone()),
            mount: Some(link),
            executable: Some(mounted.executable.clone()),
        };

        assert_eq!(
            InstallKind::classify("linux", &runtime),
            InstallKind::AppImage(mounted.image.clone())
        );
        let _ = std::fs::remove_dir_all(&mounted.dir);
    }

    #[test]
    fn linux_copy_with_inherited_appimage_variables_is_package_managed() {
        // A .deb copy started from a terminal that is itself an AppImage
        // inherits that terminal's variables. Updating must not overwrite it.
        let terminal = mounted_image();
        let installed = temp_dir().join("oikonomia");
        std::fs::write(&installed, b"exe").expect("installed copy");
        let runtime = AppImageRuntime {
            image: Some(terminal.image.clone()),
            mount: Some(terminal.mount.clone()),
            executable: Some(installed),
        };

        assert_eq!(
            InstallKind::classify("linux", &runtime),
            InstallKind::PackageManaged
        );
        assert_eq!(std::fs::read(&terminal.image).expect("kept"), b"image");
        let _ = std::fs::remove_dir_all(&terminal.dir);
    }

    #[test]
    fn a_mount_that_would_contain_every_executable_proves_nothing() {
        let mounted = mounted_image();
        let root = mounted
            .executable
            .ancestors()
            .last()
            .expect("root")
            .to_path_buf();

        for mount in [root, PathBuf::new()] {
            let runtime = AppImageRuntime {
                image: Some(mounted.image.clone()),
                mount: Some(mount.clone()),
                executable: Some(mounted.executable.clone()),
            };

            assert_eq!(
                InstallKind::classify("linux", &runtime),
                InstallKind::PackageManaged,
                "mount {mount:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&mounted.dir);
    }

    #[test]
    fn linux_copy_without_a_usable_image_is_package_managed() {
        let mounted = mounted_image();
        let image_is_gone = AppImageRuntime {
            image: Some(mounted.dir.join("gone.AppImage")),
            mount: Some(mounted.mount.clone()),
            executable: Some(mounted.executable.clone()),
        };

        assert_eq!(
            InstallKind::classify("linux", &NO_APPIMAGE),
            InstallKind::PackageManaged
        );
        assert_eq!(
            InstallKind::classify("linux", &image_is_gone),
            InstallKind::PackageManaged
        );
        let _ = std::fs::remove_dir_all(&mounted.dir);
    }

    #[test]
    fn other_systems_ignore_the_appimage_variables() {
        let mounted = mounted_image();
        let stray = AppImageRuntime {
            image: Some(mounted.image.clone()),
            mount: Some(mounted.mount.clone()),
            executable: Some(mounted.executable.clone()),
        };

        assert_eq!(InstallKind::classify("macos", &stray), InstallKind::MacApp);
        assert_eq!(
            InstallKind::classify("windows", &stray),
            InstallKind::WindowsInstaller
        );
        assert_eq!(
            InstallKind::classify("freebsd", &stray),
            InstallKind::PackageManaged
        );
        let _ = std::fs::remove_dir_all(&mounted.dir);
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

    /// An installed app bundle whose executable holds `marker`, and the
    /// path of that executable.
    #[cfg(target_os = "macos")]
    fn installed_app(root: &std::path::Path, marker: &[u8]) -> PathBuf {
        let executable = root
            .join("Oikonomia.app")
            .join("Contents")
            .join("MacOS")
            .join("oikonomia");
        std::fs::create_dir_all(executable.parent().expect("parent")).expect("bundle dirs");
        std::fs::write(&executable, marker).expect("executable");
        executable
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_replaces_the_installed_bundle_with_the_archived_one() {
        use oikonomia_update::InstallHandoff;

        let dir = temp_dir();
        let applications = dir.join("Applications");
        let current = installed_app(&applications, b"old version");
        std::fs::write(
            applications
                .join("Oikonomia.app")
                .join("Contents")
                .join("stale.txt"),
            b"left by the old version",
        )
        .expect("stale file");

        let staging = dir.join("staging");
        installed_app(&staging, b"new version");
        let archive = dir.join("abc-Oikonomia.app.tar.gz");
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&staging)
            .arg("Oikonomia.app")
            .status()
            .expect("tar");
        assert!(status.success());

        let handoff =
            super::extract_macos_app_archive(&archive, &current).map(|()| InstallHandoff::Replaced);

        assert_eq!(handoff.ok(), Some(InstallHandoff::Replaced));
        assert_eq!(std::fs::read(&current).expect("installed"), b"new version");
        assert!(
            !applications
                .join("Oikonomia.app")
                .join("Contents")
                .join("stale.txt")
                .exists(),
            "files of the old version must not survive"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&applications)
            .expect("read")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(leftovers, ["Oikonomia.app"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_keeps_the_installed_app_when_the_archive_is_bad() {
        let dir = temp_dir();
        let applications = dir.join("Applications");
        let current = installed_app(&applications, b"old version");

        let not_an_archive = dir.join("abc-Oikonomia.app.tar.gz");
        std::fs::write(&not_an_archive, b"garbage").expect("write");
        assert!(super::extract_macos_app_archive(&not_an_archive, &current).is_err());

        // A valid archive that holds no app bundle.
        let staging = dir.join("staging");
        std::fs::create_dir_all(&staging).expect("staging");
        std::fs::write(staging.join("readme.txt"), b"no app here").expect("file");
        let no_app = dir.join("def-Oikonomia.app.tar.gz");
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&no_app)
            .arg("-C")
            .arg(&staging)
            .arg("readme.txt")
            .status()
            .expect("tar");
        assert!(status.success());
        assert!(super::extract_macos_app_archive(&no_app, &current).is_err());

        assert_eq!(
            std::fs::read(&current).expect("still installed"),
            b"old version"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    fn archive_of(staging: &std::path::Path, entries: &[&str], archive: &std::path::Path) {
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(archive)
            .arg("-C")
            .arg(staging)
            .args(entries)
            .status()
            .expect("tar");
        assert!(status.success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_refuses_an_archive_that_is_not_one_whole_app() {
        let dir = temp_dir();
        let applications = dir.join("Applications");
        let current = installed_app(&applications, b"old version");
        let staging = dir.join("staging");
        std::fs::create_dir_all(&staging).expect("staging");

        // A regular file named like a bundle.
        std::fs::write(staging.join("File.app"), b"not a directory").expect("file");
        // A bundle without the executable the installed app runs.
        std::fs::create_dir_all(staging.join("Empty.app").join("Contents")).expect("empty");
        // Two complete bundles.
        installed_app(&staging.join("one"), b"one");
        installed_app(&staging.join("two"), b"two");
        std::fs::rename(
            staging.join("two").join("Oikonomia.app"),
            staging.join("one").join("Other.app"),
        )
        .expect("second bundle");

        let cases: [(&std::path::Path, &[&str]); 3] = [
            (&staging, &["File.app"]),
            (&staging, &["Empty.app"]),
            (&staging.join("one"), &["Oikonomia.app", "Other.app"]),
        ];
        for (index, (root, entries)) in cases.into_iter().enumerate() {
            let archive = dir.join(format!("{index}-Oikonomia.app.tar.gz"));
            archive_of(root, entries, &archive);

            assert!(
                super::extract_macos_app_archive(&archive, &current).is_err(),
                "{entries:?} was installed"
            );
            assert_eq!(std::fs::read(&current).expect("kept"), b"old version");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_never_deletes_the_set_aside_copy_it_is_running_from() {
        // An update that died half-way can leave the app only under its
        // set-aside name. Launched from there, it must not clear itself.
        let dir = temp_dir();
        let applications = dir.join("Applications");
        installed_app(&applications, b"only copy");
        let set_aside = applications.join(super::PREVIOUS_BUNDLE);
        std::fs::rename(applications.join("Oikonomia.app"), &set_aside).expect("set aside");
        let current = set_aside.join("Contents").join("MacOS").join("oikonomia");

        let staging = dir.join("staging");
        installed_app(&staging, b"new version");
        let archive = dir.join("abc-Oikonomia.app.tar.gz");
        archive_of(&staging, &["Oikonomia.app"], &archive);

        let err = super::extract_macos_app_archive(&archive, &current).expect_err("refused");

        assert_eq!(err.code(), "update_install_not_allowed");
        assert_eq!(std::fs::read(&current).expect("still there"), b"only copy");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_through_a_linked_bundle_replaces_the_real_one() {
        let dir = temp_dir();
        let real_home = dir.join("real");
        installed_app(&real_home, b"old version");
        let applications = dir.join("Applications");
        std::fs::create_dir_all(&applications).expect("applications");
        let link = applications.join("Oikonomia.app");
        std::os::unix::fs::symlink(real_home.join("Oikonomia.app"), &link).expect("link");
        let through_link = link.join("Contents").join("MacOS").join("oikonomia");

        let staging = dir.join("staging");
        installed_app(&staging, b"new version");
        let archive = dir.join("abc-Oikonomia.app.tar.gz");
        archive_of(&staging, &["Oikonomia.app"], &archive);

        super::extract_macos_app_archive(&archive, &through_link).expect("update");

        assert!(link.is_symlink(), "the link itself must stay a link");
        assert_eq!(
            std::fs::read(&through_link).expect("via link"),
            b"new version"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_clears_a_stale_set_aside_bundle() {
        let dir = temp_dir();
        let applications = dir.join("Applications");
        let current = installed_app(&applications, b"old version");
        let stale = applications.join(super::PREVIOUS_BUNDLE);
        std::fs::create_dir_all(&stale).expect("stale");

        let staging = dir.join("staging");
        installed_app(&staging, b"new version");
        let archive = dir.join("abc-Oikonomia.app.tar.gz");
        archive_of(&staging, &["Oikonomia.app"], &archive);

        super::extract_macos_app_archive(&archive, &current).expect("update");

        assert_eq!(std::fs::read(&current).expect("installed"), b"new version");
        assert!(!stale.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_outside_a_bundle_replaces_nothing() {
        let dir = temp_dir();
        let bare = dir.join("target").join("debug").join("oikonomia");
        std::fs::create_dir_all(bare.parent().expect("parent")).expect("dirs");
        std::fs::write(&bare, b"dev build").expect("binary");
        let archive = dir.join("abc-Oikonomia.app.tar.gz");
        std::fs::write(&archive, b"irrelevant").expect("archive");

        assert!(super::extract_macos_app_archive(&archive, &bare).is_err());

        assert_eq!(std::fs::read(&bare).expect("untouched"), b"dev build");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_bundle_path_is_found_from_the_executable_inside_it() {
        let inside = PathBuf::from("/Applications/Oikonomia.app/Contents/MacOS/oikonomia");
        assert_eq!(
            super::macos_app_bundle_path(&inside).ok(),
            Some(PathBuf::from("/Applications/Oikonomia.app"))
        );

        // A bare binary (cargo run) has no bundle; no directory may be replaced.
        let bare = PathBuf::from("/work/target/debug/oikonomia");
        assert!(super::macos_app_bundle_path(&bare).is_err());

        // Only `<bundle>.app/Contents/MacOS/<binary>` counts. A source tree
        // under a directory that ends in `.app` must never be replaced.
        for outside in [
            "/work/tool.app",
            "/work/project.app/src-tauri/target/debug/oikonomia",
            "/work/project.app/Contents/Resources/oikonomia",
            "/work/project.app/MacOS/oikonomia",
            "/Contents/MacOS/oikonomia",
        ] {
            assert!(
                super::macos_app_bundle_path(&PathBuf::from(outside)).is_err(),
                "{outside}"
            );
        }
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

    /// An installed image and a verified download next to it, as
    /// `(directory, current, verified)`.
    #[cfg(unix)]
    fn appimage_and_download() -> (PathBuf, PathBuf, PathBuf) {
        let dir = temp_dir();
        let current = dir.join("Oikonomia.AppImage");
        let verified = dir.join("abc-Oikonomia_0.2.0_amd64.AppImage");
        std::fs::write(&current, b"old-appimage").expect("current");
        std::fs::write(&verified, b"new-verified").expect("verified");
        (dir, current, verified)
    }

    #[cfg(unix)]
    #[test]
    fn appimage_replace_does_not_write_through_a_link_planted_at_the_staging_name() {
        let (dir, current, verified) = appimage_and_download();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"untouched").expect("victim");
        let planted = dir.join(".Oikonomia.AppImage.staging");
        std::os::unix::fs::symlink(&victim, &planted).expect("link");

        super::replace_linux_appimage(&verified, &current).expect("replace");

        assert_eq!(std::fs::read(&victim).expect("victim"), b"untouched");
        assert_eq!(std::fs::read(&current).expect("current"), b"new-verified");
        assert!(!current.is_symlink(), "the image must be a real file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Names of the entries in `dir`, sorted.
    #[cfg(unix)]
    fn entries_of(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    #[test]
    fn appimage_replace_leaves_an_executable_image_and_nothing_else() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, current, verified) = appimage_and_download();

        super::replace_linux_appimage(&verified, &current).expect("replace");

        assert_eq!(std::fs::read(&current).expect("current"), b"new-verified");
        let mode = std::fs::metadata(&current)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            entries_of(&dir),
            ["Oikonomia.AppImage", "abc-Oikonomia_0.2.0_amd64.AppImage"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn appimage_replace_starts_over_a_staging_file_left_by_an_interrupted_update() {
        let (dir, current, verified) = appimage_and_download();
        let stale = dir.join(".Oikonomia.AppImage.staging");
        std::fs::write(
            &stale,
            b"half of an older download, longer than the new one",
        )
        .expect("stale");

        super::replace_linux_appimage(&verified, &current).expect("replace");

        assert_eq!(std::fs::read(&current).expect("current"), b"new-verified");
        assert!(!stale.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn appimage_replace_that_fails_removes_its_staging_file_and_keeps_the_installed_image() {
        let (dir, _current, verified) = appimage_and_download();
        // A directory cannot be replaced by renaming a file over it, so the
        // update fails after the staging file was written.
        let installed = dir.join("Installed.AppImage");
        std::fs::create_dir(&installed).expect("directory");
        std::fs::write(installed.join("kept"), b"old").expect("content");

        let err = super::replace_linux_appimage(&verified, &installed).expect_err("refused");

        assert_eq!(err.code(), "update_artifact_integrity");
        assert_eq!(std::fs::read(installed.join("kept")).expect("kept"), b"old");
        assert!(!dir.join(".Installed.AppImage.staging").exists());
        let _ = std::fs::remove_dir_all(&dir);
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
