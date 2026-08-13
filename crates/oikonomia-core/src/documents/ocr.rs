//! Bundled offline OCR (ocrs neural models shipped with the app).
//!
//! Models are small (~12 MB total) and run entirely on-device. No network,
//! no Ollama, no external services.
//!
//! Receipts: best-effort preprocessing (upscale, contrast) before OCR.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageReader, RgbImage};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use tracing::info;

use crate::error::{Error, Result};

static ENGINE: LazyLock<Mutex<Option<OcrEngine>>> = LazyLock::new(|| Mutex::new(None));

/// Paths to the two shipped `.rten` models.
#[derive(Debug, Clone)]
pub struct OcrModelPaths {
    /// Text detection network.
    pub detection: PathBuf,
    /// Text recognition network.
    pub recognition: PathBuf,
}

impl OcrModelPaths {
    /// Resolve models under a directory that contains the two files.
    #[must_use]
    pub fn from_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            detection: dir.join("text-detection.rten"),
            recognition: dir.join("text-recognition.rten"),
        }
    }

    /// Whether both model files exist on disk.
    #[must_use]
    pub fn available(&self) -> bool {
        self.detection.is_file() && self.recognition.is_file()
    }
}

/// True if the OCR engine can be (or already has been) loaded.
pub fn ocr_available(paths: &OcrModelPaths) -> bool {
    if lock_engine().is_some() {
        return true;
    }
    paths.available()
}

fn lock_engine() -> std::sync::MutexGuard<'static, Option<OcrEngine>> {
    match ENGINE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            ENGINE.clear_poison();
            let mut guard = poisoned.into_inner();
            *guard = None;
            guard
        }
    }
}

/// Ensure the global OCR engine is loaded (lazy, once).
///
/// # Errors
///
/// Missing model files or engine init failure.
pub fn ensure_engine(paths: &OcrModelPaths) -> Result<()> {
    let mut guard = lock_engine();

    if guard.is_some() {
        return Ok(());
    }

    if !paths.available() {
        return Err(Error::Analysis(format!(
            "bundled OCR models not found (expected {} and {})",
            paths.detection.display(),
            paths.recognition.display()
        )));
    }

    info!(
        detection = %paths.detection.display(),
        recognition = %paths.recognition.display(),
        "loading bundled OCR models"
    );

    let detection = Model::load_file(&paths.detection)
        .map_err(|e| Error::Analysis(format!("load detection model: {e}")))?;
    let recognition = Model::load_file(&paths.recognition)
        .map_err(|e| Error::Analysis(format!("load recognition model: {e}")))?;

    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection),
        recognition_model: Some(recognition),
        ..Default::default()
    })
    .map_err(|e| Error::Analysis(format!("init OCR engine: {e}")))?;

    *guard = Some(engine);
    Ok(())
}

/// Run OCR on an image (PNG/JPEG/WebP bytes) and return plain text lines joined.
///
/// Applies best-effort preprocessing for phone photos / receipts: orientation is
/// left as stored; small images are upscaled; mild contrast boost.
///
/// # Errors
///
/// Decode failure or OCR runtime error.
pub fn ocr_image_bytes(paths: &OcrModelPaths, data: &[u8]) -> Result<String> {
    ensure_engine(paths)?;

    let dyn_img = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| Error::Analysis(format!("image format: {e}")))?
        .decode()
        .map_err(|e| Error::Analysis(format!("decode image: {e}")))?;

    let prepared = preprocess_for_receipt(&dyn_img);
    run_ocr_on_rgb(paths, &prepared)
}

/// Scale a dimension, clamped into the valid non-zero range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "input is an image dimension; the scaled result is clamped to at least 1"
)]
fn scaled_side(side: u32, scale: f64) -> u32 {
    ((f64::from(side) * scale).round() as u32).max(1)
}

fn preprocess_for_receipt(img: &DynamicImage) -> RgbImage {
    // Convert to luma then back to RGB — OCR is greyscale; colour UIs (screenshots)
    // confuse detection less after this.
    let mut luma = img.to_luma8();
    let (width, height) = luma.dimensions();

    // Phone photos / desktop screenshots: upscale small images; cap huge ones.
    let min_side = width.min(height);
    let max_side = width.max(height);
    if min_side > 0 && min_side < 1200 {
        let scale = 1200.0 / f64::from(min_side);
        luma = imageops::resize(
            &luma,
            scaled_side(width, scale),
            scaled_side(height, scale),
            FilterType::Lanczos3,
        );
    } else if max_side > 2800 {
        let scale = 2400.0 / f64::from(max_side);
        luma = imageops::resize(
            &luma,
            scaled_side(width, scale),
            scaled_side(height, scale),
            FilterType::Triangle,
        );
    }

    // Contrast + mild unsharp-ish stretch helps faded thermal receipts and UI screenshots.
    // Also lift dark-mode UIs (near-black bg, light text) by auto-inverting when mean is low.
    let mut sum: u64 = 0;
    for pixel in luma.pixels() {
        sum += u64::from(pixel[0]);
    }
    let count = u64::from(luma.width()) * u64::from(luma.height());
    // A mean of u8 pixels always fits u8; try_from guards the impossible case.
    let mean = sum
        .checked_div(count)
        .map_or(128, |m| u8::try_from(m).unwrap_or(u8::MAX));
    let invert = mean < 90; // dark-mode screenshot → invert for black-on-white OCR

    let mut rgb = RgbImage::new(luma.width(), luma.height());
    let contrast = 1.35_f32;
    for (x, y, pixel) in luma.enumerate_pixels() {
        let mut value = f32::from(pixel[0]) / 255.0;
        if invert {
            value = 1.0 - value;
        }
        // Contrast around mid-grey, then a soft gamma to open midtones.
        value = ((value - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
        value = value.powf(0.92);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value is clamped to [0, 1] before scaling into u8 range"
        )]
        let level = (value * 255.0).round() as u8;
        rgb.put_pixel(x, y, image::Rgb([level, level, level]));
    }

    rgb
}

fn run_ocr_on_rgb(paths: &OcrModelPaths, img: &RgbImage) -> Result<String> {
    let _ = paths; // engine already loaded
    let img_source = ImageSource::from_bytes(img.as_raw(), img.dimensions())
        .map_err(|e| Error::Analysis(format!("image source: {e}")))?;

    let mut guard = lock_engine();
    let engine = guard
        .as_mut()
        .ok_or_else(|| Error::Analysis("OCR engine not loaded".into()))?;

    let inferred = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        infer_text(engine, img_source)
    }));

    if let Ok(result) = inferred {
        result
    } else {
        *guard = None;
        Err(Error::Analysis("OCR engine panicked".into()))
    }
}

fn infer_text(engine: &mut OcrEngine, img_source: ImageSource<'_>) -> Result<String> {
    let ocr_input = engine
        .prepare_input(img_source)
        .map_err(|e| Error::Analysis(format!("OCR prepare: {e}")))?;

    // Prefer the high-level API when possible for denser text recovery.
    if let Ok(blob) = engine.get_text(&ocr_input) {
        let cleaned = blob
            .lines()
            .map(str::trim)
            .filter(|l| l.chars().count() > 1)
            .collect::<Vec<_>>()
            .join("\n");
        if cleaned.chars().count() > 8 {
            return Ok(cleaned);
        }
    }

    let word_rects = engine
        .detect_words(&ocr_input)
        .map_err(|e| Error::Analysis(format!("OCR detect: {e}")))?;
    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
    let line_texts = engine
        .recognize_text(&ocr_input, &line_rects)
        .map_err(|e| Error::Analysis(format!("OCR recognize: {e}")))?;

    let mut lines = Vec::new();
    for line in line_texts.iter().flatten() {
        let s = line.to_string();
        if s.chars().count() > 1 {
            lines.push(s);
        }
    }

    Ok(lines.join("\n"))
}
