//! Process-wide application state (vault handle + OCR model paths).

use std::path::PathBuf;
use std::sync::Mutex;

use oikonomia_core::error::Error as CoreError;
use oikonomia_core::vault::{Vault, VaultStatus, default_data_dir};

/// Shared state behind Tauri commands.
pub struct AppState {
    vault: Mutex<Vault>,
    /// Directory containing bundled `text-detection.rten` + `text-recognition.rten`.
    ocr_model_dir: PathBuf,
}

impl AppState {
    /// Open the vault path; resolve OCR models next to the binary / resources.
    ///
    /// # Errors
    ///
    /// Propagates vault I/O errors from the default data directory.
    pub fn new(ocr_model_dir: PathBuf) -> Result<Self, CoreError> {
        let data_dir = default_data_dir()?;
        let vault = Vault::open_path(data_dir)?;
        Ok(Self {
            vault: Mutex::new(vault),
            ocr_model_dir,
        })
    }

    /// Path to bundled OCR models.
    #[must_use]
    pub fn ocr_model_dir(&self) -> &PathBuf {
        &self.ocr_model_dir
    }

    /// Run a closure with exclusive access to the vault.
    pub fn with_vault<T>(
        &self,
        f: impl FnOnce(&mut Vault) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut guard = self.vault.lock().map_err(|_| CoreError::VaultLocked)?;
        f(&mut guard)
    }

    /// Current vault status.
    pub fn status(&self) -> Result<VaultStatus, CoreError> {
        self.with_vault(|vault| Ok(vault.status()))
    }
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
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let near = parent.join("resources/ocr");
            if near.join("text-detection.rten").is_file() {
                return near;
            }
        }
    }

    dev
}
