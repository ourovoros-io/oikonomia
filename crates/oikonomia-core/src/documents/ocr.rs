//! OCR with the `ocrs` models that ship with the application.
//!
//! The two models, text detection and text recognition (about 12 MB
//! together), are read from disk and run on the device. Nothing here opens a
//! socket.
//!
//! # One engine per process
//!
//! [`ENGINE`] holds the loaded engine behind a mutex. [`ensure_engine`] loads
//! it on first use from the model paths of that call; once an engine is
//! loaded, later calls do not look at their paths. Inference keeps the lock
//! for a whole run, so two images are never read at once.
//! [`ocr_available`] does not take the lock: it reads [`ENGINE_LOADED`] and
//! the model paths.
//!
//! A panic inside the engine is caught in [`run_ocr_on_rgb`] and reported as
//! an error. The engine is dropped there, because its state after an unwind
//! is unknown, and the next call loads a new one.
//!
//! # Preparing an image
//!
//! [`prepare_image`] turns the bytes of a PNG, JPEG or WebP file into what
//! the engine reads:
//!
//! 1. The header is read under the decoder limits ([`MAX_DECODED_SIDE`],
//!    [`MAX_DECODE_BYTES`]), and a shape that cannot be read (a zero side, or
//!    more elongated than [`MAX_ASPECT_RATIO`]) is refused before any pixel
//!    is decoded.
//! 2. The image is decoded and reduced to 8-bit grey. Orientation is taken as
//!    stored: an EXIF rotation tag is not applied.
//! 3. It is resampled by the factor [`ocr_scale`] gives: small images are
//!    enlarged a little, huge ones shrunk, so the long side never exceeds
//!    [`MAX_LONG_SIDE`].
//! 4. [`contrast_stretched`] inverts a dark image and raises the contrast.
//!
//! # Reading
//!
//! [`infer_text`] returns the recognized lines joined with newlines, without
//! the lines of a single character.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageReader, Limits, RgbImage};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;

use crate::error::{Error, Result};

/// The one OCR engine of the process, or `None` until [`ensure_engine`] has
/// loaded it and again after it panicked.
///
/// The lock is held for the whole of an inference run.
static ENGINE: LazyLock<Mutex<Option<OcrEngine>>> = LazyLock::new(|| Mutex::new(None));

/// Whether [`ENGINE`] holds a loaded engine, readable without its lock.
///
/// Inference keeps the lock for a whole run, which can take seconds, and
/// [`ocr_available`] must not wait for that. Every store is made with the
/// lock held ([`set_engine`], [`lock_engine`]), so the flag agrees with the
/// engine whenever the lock is free. It carries no data, only a yes or no
/// that the next [`ensure_engine`] call checks again under the lock, so
/// `Relaxed` is enough.
static ENGINE_LOADED: AtomicBool = AtomicBool::new(false);

/// Where the two `.rten` model files are.
#[derive(Debug, Clone)]
pub struct OcrModelPaths {
    /// The text detection model, which finds where the words are.
    pub detection: PathBuf,
    /// The text recognition model, which reads a line of text.
    pub recognition: PathBuf,
}

impl OcrModelPaths {
    /// The paths of the two models in `dir`, under the names they ship with:
    /// `text-detection.rten` and `text-recognition.rten`.
    ///
    /// Nothing is read; [`available`](Self::available) says whether the files
    /// exist.
    #[must_use]
    pub fn from_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            detection: dir.join("text-detection.rten"),
            recognition: dir.join("text-recognition.rten"),
        }
    }

    /// Whether both model files exist. Their content is not checked.
    #[must_use]
    pub fn available(&self) -> bool {
        self.detection.is_file() && self.recognition.is_file()
    }
}

/// Whether OCR can run: an engine is already loaded, or both model files
/// exist at `paths`.
///
/// Once an engine is loaded this is true whatever `paths` says. Never waits
/// for a running OCR: it reads [`ENGINE_LOADED`] instead of taking the engine
/// lock.
pub(super) fn ocr_available(paths: &OcrModelPaths) -> bool {
    ENGINE_LOADED.load(Ordering::Relaxed) || paths.available()
}

/// Locks [`ENGINE`], recovering from a poisoned lock, and keeps
/// [`ENGINE_LOADED`] in step when the slot comes back empty.
fn lock_engine() -> std::sync::MutexGuard<'static, Option<OcrEngine>> {
    let guard = recover_option_mutex(&ENGINE);
    // Recovery from a poisoned lock empties the slot without going through
    // `set_engine`.
    if guard.is_none() {
        ENGINE_LOADED.store(false, Ordering::Relaxed);
    }
    guard
}

/// Puts `engine` in the locked `slot` and records whether one is loaded.
fn set_engine(slot: &mut Option<OcrEngine>, engine: Option<OcrEngine>) {
    ENGINE_LOADED.store(engine.is_some(), Ordering::Relaxed);
    *slot = engine;
}

/// Locks `mutex`. When a panic poisoned it, clears the poison and empties the
/// slot.
///
/// The slot is emptied because the value was in use when its holder
/// panicked, so it may be half-updated; `None` makes the next user build a
/// new one. For the engine that means reloading the models.
fn recover_option_mutex<T>(mutex: &Mutex<Option<T>>) -> std::sync::MutexGuard<'_, Option<T>> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            mutex.clear_poison();
            let mut guard = poisoned.into_inner();
            *guard = None;
            guard
        }
    }
}

/// Loads the engine into [`ENGINE`] unless one is loaded already.
///
/// Loading reads both model files, so the first call is slow and the lock is
/// held while it runs.
///
/// # Errors
///
/// [`Error::Analysis`] when a model file is missing, when a model does not
/// load, or when the engine cannot be built from the two models.
pub(super) fn ensure_engine(paths: &OcrModelPaths) -> Result<()> {
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

    log::info!(
        "loading bundled OCR models from {} and {}",
        paths.detection.display(),
        paths.recognition.display()
    );

    let detection = Model::load_file(&paths.detection)
        .map_err(|err| Error::Analysis(format!("load detection model: {err}")))?;
    let recognition = Model::load_file(&paths.recognition)
        .map_err(|err| Error::Analysis(format!("load recognition model: {err}")))?;

    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection),
        recognition_model: Some(recognition),
        ..Default::default()
    })
    .map_err(|err| Error::Analysis(format!("init OCR engine: {err}")))?;

    set_engine(&mut guard, Some(engine));
    Ok(())
}

/// Reads the text of an image file (PNG, JPEG or WebP bytes).
///
/// The image is prepared first ([`prepare_image`]). The text is the
/// recognized lines joined with newlines, and is empty when none was found.
///
/// # Errors
///
/// [`Error::Analysis`] in every case:
///
/// - the models are missing or do not load ([`ensure_engine`]);
/// - the bytes are not an image the decoder knows, or the image is over the
///   decoding limits;
/// - the image has a zero side or is more elongated than
///   [`MAX_ASPECT_RATIO`];
/// - the engine fails or panics.
pub(super) fn ocr_image_bytes(paths: &OcrModelPaths, data: &[u8]) -> Result<String> {
    ensure_engine(paths)?;

    let prepared = prepare_image(data)?;
    run_ocr_on_rgb(&prepared)
}

/// Short side an image is scaled up towards, so a small photo or screenshot
/// gives recognition more pixels per line of text.
const UPSCALE_SHORT_SIDE: u32 = 1200;

/// The most an image is enlarged by.
///
/// Enlarging adds no detail, and the recognizer misreads text that gets too
/// large. At this factor the corpus image `english_total.jpg` (768 x 104) is
/// read correctly, which `jpeg_ocr_smoke` in `tests/document_corpus.rs`
/// checks.
const MAX_UPSCALE: f64 = 1.5;

/// Longest side a prepared image may have. Together with the shape of an
/// image it bounds the work and memory of every later step: no prepared
/// image is larger than 2800 x 2800.
const MAX_LONG_SIDE: u32 = 2800;

/// The long side an image over [`MAX_LONG_SIDE`] is scaled down to.
const CAPPED_LONG_SIDE: u32 = 2400;

/// Largest ratio of long side to short side that is read. At
/// [`CAPPED_LONG_SIDE`] a narrower image has a short side under 120 px
/// (2400 / 20), too narrow to hold a readable line of receipt text.
const MAX_ASPECT_RATIO: u32 = 20;

/// Pixel data the decoder may allocate: 256 MiB holds an RGB image of 89
/// megapixels. Set here so the bound does not depend on the defaults of the
/// `image` crate. The greyscale copy made from the decoded image adds at
/// most half as much again, and nothing when the image is 8-bit grey.
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

/// Longest side the decoder accepts. An image that fits
/// [`MAX_DECODE_BYTES`] at 4:3 has a long side under 11,000 px; this leaves
/// room for wider shapes.
const MAX_DECODED_SIDE: u32 = 16_384;

/// Decodes image bytes and prepares the picture for OCR, by the four steps in
/// the module documentation.
///
/// # Errors
///
/// [`Error::Analysis`] when the format is not recognized, the header or the
/// pixels do not decode within the limits, or [`ocr_scale`] refuses the
/// shape.
fn prepare_image(data: &[u8]) -> Result<RgbImage> {
    // The header is read first, so a shape that would be refused after
    // decoding is refused before any pixel is decoded.
    let (width, height) = image_reader(data)?
        .into_dimensions()
        .map_err(|err| Error::Analysis(format!("image dimensions: {err}")))?;
    ocr_scale(width, height)?;

    let decoded = image_reader(data)?
        .decode()
        .map_err(|err| Error::Analysis(format!("decode image: {err}")))?;

    preprocess_for_receipt(decoded)
}

/// A reader over `data` with its format guessed from the content and the
/// decoding limits set.
///
/// # Errors
///
/// [`Error::Analysis`] when the format cannot be guessed. Reading from memory
/// does not fail otherwise.
fn image_reader(data: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);

    let mut reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|err| Error::Analysis(format!("image format: {err}")))?;
    reader.limits(limits);
    Ok(reader)
}

/// `side` multiplied by `scale` and rounded, at least 1.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "input is an image dimension; the scaled result is clamped to at least 1"
)]
fn scaled_side(side: u32, scale: f64) -> u32 {
    ((f64::from(side) * scale).round() as u32).max(1)
}

/// The factor an image of this size is resampled by before OCR.
///
/// - Long side over [`MAX_LONG_SIDE`]: scaled down to [`CAPPED_LONG_SIDE`].
/// - Otherwise, short side under [`UPSCALE_SHORT_SIDE`]: scaled up towards
///   it, by at most [`MAX_UPSCALE`] and stopping where the long side reaches
///   [`MAX_LONG_SIDE`].
/// - Otherwise 1: the image is kept as it is.
///
/// So the long side of the result never exceeds [`MAX_LONG_SIDE`].
///
/// # Errors
///
/// A side of zero, or a shape more elongated than [`MAX_ASPECT_RATIO`].
fn ocr_scale(width: u32, height: u32) -> Result<f64> {
    let short_side = width.min(height);
    let long_side = width.max(height);

    if short_side == 0 {
        return Err(Error::Analysis(format!(
            "image has a zero side: {width}x{height}"
        )));
    }
    if u64::from(long_side) > u64::from(short_side) * u64::from(MAX_ASPECT_RATIO) {
        return Err(Error::Analysis(format!(
            "image is too elongated to read: {width}x{height}"
        )));
    }

    let scale = if long_side > MAX_LONG_SIDE {
        f64::from(CAPPED_LONG_SIDE) / f64::from(long_side)
    } else if short_side < UPSCALE_SHORT_SIDE {
        let to_short_target = f64::from(UPSCALE_SHORT_SIDE) / f64::from(short_side);
        let to_long_limit = f64::from(MAX_LONG_SIDE) / f64::from(long_side);
        to_short_target.min(to_long_limit).min(MAX_UPSCALE)
    } else {
        1.0
    };
    Ok(scale)
}

/// Reduces a decoded image to grey, resamples it by [`ocr_scale`] and raises
/// its contrast.
///
/// Grey because the models read luminance, and a coloured ground (a
/// screenshot of a user interface) gets in the way of detection less once it
/// is grey. An 8-bit grey image is taken as it is, without a copy.
///
/// # Errors
///
/// [`Error::Analysis`] when [`ocr_scale`] refuses the shape.
fn preprocess_for_receipt(image: DynamicImage) -> Result<RgbImage> {
    let mut luma = image.into_luma8();
    let (width, height) = luma.dimensions();

    let scale = ocr_scale(width, height)?;
    let target = (scaled_side(width, scale), scaled_side(height, scale));
    if target != (width, height) {
        // Lanczos keeps text edges sharp when enlarging; a triangle filter
        // is enough, and cheaper, when shrinking.
        let filter = if scale > 1.0 {
            FilterType::Lanczos3
        } else {
            FilterType::Triangle
        };
        luma = imageops::resize(&luma, target.0, target.1, filter);
    }

    Ok(contrast_stretched(&luma))
}

/// The grey level halfway between black and white, and the mean assumed for
/// an image without pixels.
const MID_GREY: u8 = 128;

/// Mean grey level under which an image is taken as light text on a dark
/// ground and inverted.
const DARK_IMAGE_MEAN: u8 = 90;

/// How much the distance of a pixel from mid-grey is multiplied by.
const CONTRAST_GAIN: f32 = 1.35;

/// Exponent applied after the contrast stretch. Under 1, so midtones get
/// lighter.
const MIDTONE_GAMMA: f32 = 0.92;

/// Raises the contrast of a greyscale image and returns it as RGB.
///
/// Helps faded thermal receipts and UI screenshots. A dark image (dark-mode
/// screenshot: near-black background, light text) is inverted first, so OCR
/// always sees dark text on a light ground.
fn contrast_stretched(luma: &image::GrayImage) -> RgbImage {
    let mut sum: u64 = 0;
    for pixel in luma.pixels() {
        sum += u64::from(pixel[0]);
    }
    let count = u64::from(luma.width()) * u64::from(luma.height());
    // A mean of u8 pixels always fits u8; try_from guards the impossible case.
    let mean = sum
        .checked_div(count)
        .map_or(MID_GREY, |mean| u8::try_from(mean).unwrap_or(u8::MAX));
    let invert = mean < DARK_IMAGE_MEAN;

    let mut rgb = RgbImage::new(luma.width(), luma.height());
    for (x, y, pixel) in luma.enumerate_pixels() {
        let mut value = f32::from(pixel[0]) / 255.0;
        if invert {
            value = 1.0 - value;
        }
        // Contrast around mid-grey, then a soft gamma to open midtones.
        value = ((value - 0.5) * CONTRAST_GAIN + 0.5).clamp(0.0, 1.0);
        value = value.powf(MIDTONE_GAMMA);
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

/// Runs the loaded engine on a prepared image, holding the engine lock for
/// the whole run.
///
/// # Errors
///
/// [`Error::Analysis`] when no engine is loaded, when the engine reports an
/// error, or when it panics. After a panic the engine is dropped and
/// [`ensure_engine`] loads a new one on the next call.
fn run_ocr_on_rgb(image: &RgbImage) -> Result<String> {
    let image_source = ImageSource::from_bytes(image.as_raw(), image.dimensions())
        .map_err(|err| Error::Analysis(format!("image source: {err}")))?;

    let mut guard = lock_engine();
    let engine = guard
        .as_mut()
        .ok_or_else(|| Error::Analysis("OCR engine not loaded".into()))?;

    let inferred = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        infer_text(engine, image_source)
    }));

    if let Ok(result) = inferred {
        result
    } else {
        set_engine(&mut guard, None);
        Err(Error::Analysis("OCR engine panicked".into()))
    }
}

/// Most characters of a recognized line that is dropped as noise: a line of
/// one character is a speck or a rule, not text.
const NOISE_LINE_CHARS: usize = 1;

/// Most characters of a first-pass reading that is not returned as it is.
///
/// The same number as `MIN_PDF_TEXT_CHARS` in the analyzer.
const SPARSE_TEXT_CHARS: usize = 8;

/// Reads the text of a prepared image.
///
/// The first pass is `OcrEngine::get_text`. When it fails, or yields
/// [`SPARSE_TEXT_CHARS`] characters or fewer, a second pass calls the three
/// steps `get_text` is made of (`detect_words`, `find_text_lines`,
/// `recognize_text`; `ocrs` 0.13 `src/lib.rs`) one by one. The second pass
/// therefore recognizes the same text. What it changes is that a failure
/// names the step that failed, and that a short reading is returned
/// untrimmed.
///
/// Both passes drop lines of [`NOISE_LINE_CHARS`] or fewer. Returns an empty
/// string when nothing was recognized.
///
/// # Errors
///
/// [`Error::Analysis`] naming the step that failed: preparing the input,
/// detecting words or recognizing text.
fn infer_text(engine: &mut OcrEngine, image_source: ImageSource<'_>) -> Result<String> {
    let ocr_input = engine
        .prepare_input(image_source)
        .map_err(|err| Error::Analysis(format!("OCR prepare: {err}")))?;

    if let Ok(blob) = engine.get_text(&ocr_input) {
        let cleaned = blob
            .lines()
            .map(str::trim)
            .filter(|line| line.chars().count() > NOISE_LINE_CHARS)
            .collect::<Vec<_>>()
            .join("\n");
        if cleaned.chars().count() > SPARSE_TEXT_CHARS {
            return Ok(cleaned);
        }
    }

    let word_rects = engine
        .detect_words(&ocr_input)
        .map_err(|err| Error::Analysis(format!("OCR detect: {err}")))?;
    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
    let line_texts = engine
        .recognize_text(&ocr_input, &line_rects)
        .map_err(|err| Error::Analysis(format!("OCR recognize: {err}")))?;

    let mut lines = Vec::new();
    for line in line_texts.iter().flatten() {
        let text = line.to_string();
        if text.chars().count() > NOISE_LINE_CHARS {
            lines.push(text);
        }
    }

    Ok(lines.join("\n"))
}

#[cfg(test)]
#[expect(clippy::panic, reason = "test poisons a local mutex on purpose")]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{ENGINE, OcrModelPaths, ocr_available, prepare_image, recover_option_mutex};

    /// A white PNG of the given size.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::GrayImage::from_pixel(width, height, image::Luma([255]));
        let mut bytes = Cursor::new(Vec::new());

        assert!(
            image::DynamicImage::ImageLuma8(image)
                .write_to(&mut bytes, image::ImageFormat::Png)
                .is_ok(),
            "the test image must encode"
        );
        bytes.into_inner()
    }

    /// The prepared size of a PNG of the given size, or the error message.
    fn prepared_size(width: u32, height: u32) -> Result<(u32, u32), String> {
        prepare_image(&png(width, height))
            .map(|image| image.dimensions())
            .map_err(|err| err.to_string())
    }

    /// Whether preparing a PNG of the given size fails with `reason`.
    fn is_refused_as(width: u32, height: u32, reason: &str) -> bool {
        prepared_size(width, height).is_err_and(|message| message.contains(reason))
    }

    #[test]
    fn a_slightly_small_image_is_scaled_up_to_the_short_side_target() {
        assert_eq!(prepared_size(1000, 1500), Ok((1200, 1800)));
        assert_eq!(prepared_size(1500, 1000), Ok((1800, 1200)));
    }

    #[test]
    fn a_small_image_is_enlarged_by_the_largest_factor_only() {
        assert_eq!(prepared_size(600, 800), Ok((900, 1200)));
        assert_eq!(prepared_size(768, 104), Ok((1152, 156)));
    }

    #[test]
    fn an_image_already_large_enough_keeps_its_size() {
        assert_eq!(prepared_size(1200, 1600), Ok((1200, 1600)));
        assert_eq!(prepared_size(2000, 2800), Ok((2000, 2800)));
    }

    #[test]
    fn a_huge_image_is_scaled_down_to_the_capped_long_side() {
        assert_eq!(prepared_size(1500, 3000), Ok((1200, 2400)));
    }

    #[test]
    fn upscaling_a_narrow_image_stops_at_the_long_side_limit() {
        // Scaling 1000 x 2500 to a 1200 short side would make it 3000 long.
        assert_eq!(prepared_size(1000, 2500), Ok((1120, 2800)));
        assert_eq!(prepared_size(2500, 1000), Ok((2800, 1120)));
    }

    #[test]
    fn a_narrow_image_that_is_already_too_long_is_scaled_down() {
        assert_eq!(prepared_size(400, 4000), Ok((240, 2400)));
    }

    #[test]
    fn an_extremely_elongated_image_is_refused() {
        assert!(is_refused_as(10, 300, "too elongated"));
        assert!(is_refused_as(300, 10, "too elongated"));
        // The ratio limit itself is still accepted.
        assert_eq!(prepared_size(100, 2000), Ok((140, 2800)));
    }

    #[test]
    fn an_image_with_a_side_over_the_decoder_limit_is_refused() {
        // The limit applies when the header is read, before any pixel.
        assert!(is_refused_as(17_000, 1_000, "image dimensions"));
        assert!(is_refused_as(1_000, 17_000, "image dimensions"));
        assert!(is_refused_as(1, 60_000, "image dimensions"));
    }

    #[test]
    fn availability_is_answered_while_the_engine_is_in_use() {
        // Inference holds this lock for its whole run.
        let in_use = recover_option_mutex(&ENGINE);
        let (answer, answered) = mpsc::channel();

        let asker = std::thread::spawn(move || {
            let paths = OcrModelPaths::from_dir("no-such-model-directory");
            let _ = answer.send(ocr_available(&paths));
        });

        // An answer that needs the lock never comes while it is held here;
        // the wait only bounds how long that failure takes to show.
        let available = answered.recv_timeout(Duration::from_secs(30));
        drop(in_use);
        assert!(asker.join().is_ok());

        assert_eq!(
            available,
            Ok(false),
            "the answer must not wait for the engine"
        );
    }

    #[test]
    fn recover_option_mutex_clears_poison() {
        let mutex = Mutex::new(Some(7_i32));
        let _ = std::panic::catch_unwind(|| {
            let _guard = mutex.lock();
            panic!("poison the lock");
        });
        assert!(mutex.is_poisoned());
        let guard = recover_option_mutex(&mutex);
        assert!(guard.is_none());
        drop(guard);
        assert!(!mutex.is_poisoned());
        assert!(mutex.lock().is_ok_and(|guard| guard.is_none()));
    }
}
