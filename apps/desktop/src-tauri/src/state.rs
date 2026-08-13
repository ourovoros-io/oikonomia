//! Process-wide application state: vault handle, OCR model paths, idle lock.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::DEFAULT_LOCK_TIMEOUT_SECS;
use oikonomia_core::vault::{Vault, VaultStatus, default_data_dir};

/// Shared state behind Tauri commands.
pub struct AppState {
    vault: Arc<Mutex<Vault>>,
    /// App data directory: vault files plus the plaintext UI prefs.
    data_dir: PathBuf,
    /// Directory containing bundled `text-detection.rten` + `text-recognition.rten`.
    ocr_model_dir: PathBuf,
    /// Seconds since `UNIX_EPOCH` of the last command touching the vault.
    last_activity: Arc<AtomicU64>,
    /// Cached idle timeout for the watchdog; the persisted value lives in the vault.
    lock_timeout_secs: Arc<AtomicU64>,
    /// Canonical paths from native drag-drop; path IPC commands accept only these.
    allowed_drop_paths: Mutex<HashSet<PathBuf>>,
    /// Serializes plaintext prefs load-mutate-save.
    prefs_lock: Mutex<()>,
}

impl AppState {
    /// Open the vault path; resolve OCR models next to the binary / resources.
    ///
    /// # Errors
    ///
    /// Propagates vault I/O errors from the default data directory.
    pub fn new(ocr_model_dir: PathBuf) -> Result<Self, CoreError> {
        let data_dir = default_data_dir()?;
        let vault = Vault::open_path(data_dir.clone())?;
        Ok(Self {
            vault: Arc::new(Mutex::new(vault)),
            data_dir,
            ocr_model_dir,
            last_activity: Arc::new(AtomicU64::new(now_secs())),
            lock_timeout_secs: Arc::new(AtomicU64::new(DEFAULT_LOCK_TIMEOUT_SECS)),
            allowed_drop_paths: Mutex::new(HashSet::new()),
            prefs_lock: Mutex::new(()),
        })
    }

    /// App data directory (UI prefs live here as plaintext).
    #[must_use]
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Path to bundled OCR models.
    #[must_use]
    pub fn ocr_model_dir(&self) -> &PathBuf {
        &self.ocr_model_dir
    }

    /// Shared vault handle for blocking work off the command thread.
    #[must_use]
    pub fn vault(&self) -> Arc<Mutex<Vault>> {
        Arc::clone(&self.vault)
    }

    /// Hold across a prefs load-mutate-save so theme and tray last-used cannot clobber.
    pub fn lock_prefs(&self) -> std::sync::MutexGuard<'_, ()> {
        match self.prefs_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.prefs_lock.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    /// Record native drop paths for later path-based analyze/post commands.
    pub fn remember_drop_paths(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut allowed = match self.allowed_drop_paths.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.allowed_drop_paths.clear_poison();
                poisoned.into_inner()
            }
        };
        for path in paths {
            if let Ok(canonical) = path.canonicalize() {
                allowed.insert(canonical);
            }
        }
    }

    /// True when `path` was recorded from a native drop (after canonicalize).
    #[must_use]
    pub fn drop_path_allowed(&self, path: &Path) -> bool {
        let allowed = match self.allowed_drop_paths.lock() {
            Ok(set) => set,
            Err(poisoned) => {
                self.allowed_drop_paths.clear_poison();
                poisoned.into_inner()
            }
        };
        path_is_allowed_set(&allowed, path)
    }

    /// Record command activity for the idle watchdog.
    pub fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }

    /// Cache the idle timeout so the watchdog needs no vault access.
    pub fn set_lock_timeout_cache(&self, secs: u64) {
        self.lock_timeout_secs.store(secs, Ordering::Relaxed);
    }

    /// Handles for the idle watchdog thread.
    #[must_use]
    pub fn watchdog_handles(&self) -> (Arc<Mutex<Vault>>, Arc<AtomicU64>, Arc<AtomicU64>) {
        (
            Arc::clone(&self.vault),
            Arc::clone(&self.last_activity),
            Arc::clone(&self.lock_timeout_secs),
        )
    }
}

/// Compare a candidate path against a set of already-canonicalized drop paths.
#[must_use]
pub fn path_is_allowed_set(allowed: &HashSet<PathBuf>, path: &Path) -> bool {
    path.canonicalize()
        .ok()
        .is_some_and(|canonical| allowed.contains(&canonical))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Lock the vault mutex, recovering from poisoning.
///
/// A panic inside a command closure must not brick the session or disable the
/// watchdog: recover the guard, force the vault into the safe locked state,
/// and clear the poison flag so later locks are clean.
pub fn lock_vault(vault: &Mutex<Vault>) -> std::sync::MutexGuard<'_, Vault> {
    match vault.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            vault.clear_poison();
            let mut guard = poisoned.into_inner();
            guard.lock();
            guard
        }
    }
}

/// Lock the vault from Rust when idle, regardless of webview state (F5).
///
/// The frontend timer is only a fast-path duplicate; this thread guarantees
/// the vault locks even if the webview throttles timers or stalls. Emits
/// `vault-locked` so the UI can drop to the unlock screen.
pub fn spawn_auto_lock(
    app: tauri::AppHandle,
    vault: Arc<Mutex<Vault>>,
    last_activity: Arc<AtomicU64>,
    lock_timeout_secs: Arc<AtomicU64>,
) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(5));

            let idle = now_secs().saturating_sub(last_activity.load(Ordering::Relaxed));
            if idle < lock_timeout_secs.load(Ordering::Relaxed) {
                continue;
            }

            let mut locked_now = false;
            {
                let mut vault = lock_vault(&vault);
                if vault.status() == VaultStatus::Unlocked {
                    vault.lock();
                    locked_now = true;
                }
            }

            if locked_now {
                use tauri::Emitter;
                let _ = app.emit("vault-locked", ());
            }
        }
    });
}

/// Resolve OCR model directory for dev and packaged builds.
#[must_use]
pub fn resolve_ocr_model_dir(resource_dir: Option<PathBuf>) -> PathBuf {
    // 1) Tauri resource dir (packaged): …/resources/ocr
    if let Some(dir) = resource_dir {
        let candidate = dir.join("ocr");
        if candidate.join("text-detection.rten").is_file() {
            return candidate;
        }
        // Sometimes resources land flat under resource_dir
        if dir.join("text-detection.rten").is_file() {
            return dir;
        }
    }

    // 2) Dev layout: apps/desktop/src-tauri/resources/ocr
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr");
    if dev.join("text-detection.rten").is_file() {
        return dev;
    }

    // 3) Fallback next to executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let near = parent.join("resources/ocr");
        if near.join("text-detection.rten").is_file() {
            return near;
        }
    }

    dev
}

#[cfg(test)]
mod tests {
    use super::path_is_allowed_set;
    use std::collections::HashSet;
    use std::fs;

    #[test]
    fn path_is_allowed_requires_canonical_membership() {
        let dir = std::env::temp_dir().join(format!("oiko-allow-{}", std::process::id()));
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let file = dir.join("drop.pdf");
        if fs::write(&file, b"%PDF").is_err() {
            return;
        }
        let Ok(canonical) = file.canonicalize() else {
            return;
        };
        let mut allowed = HashSet::new();
        allowed.insert(canonical);

        assert!(path_is_allowed_set(&allowed, &file));
        assert!(!path_is_allowed_set(&allowed, &dir.join("other.pdf")));
        let _ = fs::remove_dir_all(&dir);
    }
}
