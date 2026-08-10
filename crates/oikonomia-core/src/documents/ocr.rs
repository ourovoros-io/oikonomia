//! Bundled offline OCR (ocrs neural models shipped with the app).
//!
//! Models are small (~12 MB total) and run entirely on-device. No network,
//! no Ollama, no external services.
//!
//! Receipts: best-effort preprocessing (upscale, contrast) before OCR.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageReader, RgbImage};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use once_cell::sync::Lazy;
use rten::Model;
use tracing::info;

use crate::error::{Error, Result};

static ENGINE: Lazy<Mutex<Option<OcrEngine>>> = Lazy::new(|| Mutex::new(None));

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
    if let Ok(guard) = ENGINE.lock() {
        if guard.is_some() {
            return true;
        }
    }
    paths.available()
}

/// Ensure the global OCR engine is loaded (lazy, once).
///
/// # Errors
///
/// Missing model files or engine init failure.
pub fn ensure_engine(paths: &OcrModelPaths) -> Result<()> {
    let mut guard = ENGINE
        .lock()
        .map_err(|_| Error::Analysis("OCR engine lock poisoned".into()))?;

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

    let prepared = preprocess_for_receipt(dyn_img);
    run_ocr_on_rgb(paths, &prepared)
}

fn preprocess_for_receipt(img: DynamicImage) -> RgbImage {
    // Convert to luma then back to RGB — OCR is greyscale; colour UIs (screenshots)
    // confuse detection less after this.
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();

    let mut luma = gray;

    // Phone photos / desktop screenshots: upscale small images; cap huge ones.
    let min_side = w.min(h);
    let max_side = w.max(h);
    if min_side > 0 && min_side < 1200 {
        let scale = 1200.0 / f64::from(min_side);
        let nw = ((f64::from(w) * scale).round() as u32).max(1);
        let nh = ((f64::from(h) * scale).round() as u32).max(1);
        luma = imageops::resize(&luma, nw, nh, FilterType::Lanczos3);
    } else if max_side > 2800 {
        let scale = 2400.0 / f64::from(max_side);
        let nw = ((f64::from(w) * scale).round() as u32).max(1);
        let nh = ((f64::from(h) * scale).round() as u32).max(1);
        luma = imageops::resize(&luma, nw, nh, FilterType::Triangle);
    }

    // Contrast + mild unsharp-ish stretch helps faded thermal receipts and UI screenshots.
    // Also lift dark-mode UIs (near-black bg, light text) by auto-inverting when mean is low.
    let mut sum: u64 = 0;
    for p in luma.pixels() {
        sum += u64::from(p[0]);
    }
    let n = u64::from(luma.width()) * u64::from(luma.height());
    let mean = if n == 0 { 128 } else { (sum / n) as u8 };
    let invert = mean < 90; // dark-mode screenshot → invert for black-on-white OCR

    let mut rgb = RgbImage::new(luma.width(), luma.height());
    let contrast = 1.35_f32;
    for (x, y, pixel) in luma.enumerate_pixels() {
        let mut v = f32::from(pixel[0]) / 255.0;
        if invert {
            v = 1.0 - v;
        }
        // Contrast around mid-grey
        v = ((v - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
        // Soft gamma to open midtones
        v = v.powf(0.92);
        let u = (v * 255.0).round() as u8;
        rgb.put_pixel(x, y, image::Rgb([u, u, u]));
    }

    rgb
}

fn run_ocr_on_rgb(paths: &OcrModelPaths, img: &RgbImage) -> Result<String> {
    let _ = paths; // engine already loaded
    let img_source = ImageSource::from_bytes(img.as_raw(), img.dimensions())
        .map_err(|e| Error::Analysis(format!("image source: {e}")))?;

    let mut guard = ENGINE
        .lock()
        .map_err(|_| Error::Analysis("OCR engine lock poisoned".into()))?;
    let engine = guard
        .as_mut()
        .ok_or_else(|| Error::Analysis("OCR engine not loaded".into()))?;

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
