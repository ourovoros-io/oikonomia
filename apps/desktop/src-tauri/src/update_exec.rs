//! Exec the already-verified updater artifact. No HTTP. No plugin `check`.

use oikonomia_update::{ArtifactInstaller, Result, UpdateError};
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;

/// Installs by execing the local verified file. Never fetches `latest.json`.
pub(crate) struct VerifiedPathInstaller;

impl ArtifactInstaller for VerifiedPathInstaller {
    fn install(&self, artifact: &Path) -> Result<()> {
        exec_verified_artifact(artifact)
    }
}

/// Runs the platform installer on `artifact`. The path must already be verified.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactUrl`] for `.deb` and
/// [`UpdateError::ArtifactIntegrity`] when the file is missing or exec fails.
pub(crate) fn exec_verified_artifact(artifact: &Path) -> Result<()> {
    if path_ends_with_ignore_ascii_case(artifact, ".deb") {
        return Err(UpdateError::ArtifactUrl);
    }
    if !artifact.is_file() {
        return Err(UpdateError::ArtifactIntegrity);
    }
    exec_verified_artifact_on_target(artifact, &current_install_target()?)
}

/// Same as [`exec_verified_artifact`] with an explicit current-install path (tests).
pub(crate) fn exec_verified_artifact_on_target(artifact: &Path, current: &Path) -> Result<()> {
    if path_ends_with_ignore_ascii_case(artifact, ".deb") {
        return Err(UpdateError::ArtifactUrl);
    }
    if !artifact.is_file() {
        return Err(UpdateError::ArtifactIntegrity);
    }

    #[cfg(target_os = "linux")]
    {
        replace_linux_appimage(artifact, current)
    }

    #[cfg(target_os = "windows")]
    {
        let _ = current;
        spawn_windows_installer(artifact)
    }

    #[cfg(target_os = "macos")]
    {
        install_macos_from_path(artifact, current)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = current;
        Err(UpdateError::ArtifactIntegrity)
    }
}

fn current_install_target() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            let path = PathBuf::from(appimage);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    std::env::current_exe().map_err(|err| {
        log::warn!("current exe path failed: {err}");
        UpdateError::ArtifactIntegrity
    })
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

#[cfg(target_os = "windows")]
fn spawn_windows_installer(artifact: &Path) -> Result<()> {
    let extension = artifact
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    let mut command = if extension.eq_ignore_ascii_case("msi") {
        let mut msiexec = Command::new("msiexec");
        msiexec.arg("/i").arg(artifact);
        msiexec
    } else {
        Command::new(artifact)
    };
    command.spawn().map_err(|err| {
        log::warn!("windows installer spawn failed: {err}");
        UpdateError::ArtifactIntegrity
    })?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos_from_path(artifact: &Path, current: &Path) -> Result<()> {
    let name = artifact
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if name.ends_with(".tar.gz")
        || artifact
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tgz"))
    {
        return extract_macos_app_archive(artifact, current);
    }
    let status = Command::new("open").arg(artifact).status().map_err(|err| {
        log::warn!("macos open failed: {err}");
        UpdateError::ArtifactIntegrity
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(UpdateError::ArtifactIntegrity)
    }
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
    #[cfg(target_os = "linux")]
    use super::exec_verified_artifact_on_target;
    use super::{VerifiedPathInstaller, exec_verified_artifact};
    use oikonomia_update::ArtifactInstaller;

    fn temp_dir() -> std::path::PathBuf {
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

    #[test]
    fn exec_rejects_deb_without_running_it() {
        let dir = temp_dir();
        let deb = dir.join("oikonomia.deb");
        std::fs::write(&deb, b"not-an-installer").expect("write");
        let err = VerifiedPathInstaller
            .install(&deb)
            .expect_err("deb must be rejected");
        assert_eq!(err.code(), "update_artifact_url");
        assert_eq!(std::fs::read(&deb).expect("kept"), b"not-an-installer");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exec_rejects_missing_file() {
        let missing = std::env::temp_dir().join("oiko-missing-artifact-no-such-file");
        let err = exec_verified_artifact(&missing).expect_err("missing");
        assert_eq!(err.code(), "update_artifact_integrity");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_replace_writes_verified_bytes_onto_current() {
        let dir = temp_dir();
        let current = dir.join("Oikonomia.AppImage");
        let verified = dir.join("verified.AppImage");
        std::fs::write(&current, b"old-appimage").expect("current");
        std::fs::write(&verified, b"new-verified").expect("verified");
        exec_verified_artifact_on_target(&verified, &current).expect("replace");
        assert_eq!(std::fs::read(&current).expect("read"), b"new-verified");
        assert!(verified.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
