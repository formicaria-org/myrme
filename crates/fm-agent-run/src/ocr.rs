//! **The OCR tool of Myrme light** (2026-10-20): PP-OCRv6 small, through `oar-ocr`, on ONNX Runtime.
//!
//! The one tool the light version kept (`docs/research/2026-10-19-myrme-light.md`): typed pages read
//! at 0.8% word error in about a second on a laptop CPU, against the small vision model's 2.3% in
//! many seconds. It does not read handwriting well (61%), and says so through its confidence:
//! typed pages read at a mean of 0.99–1.00, handwritten ones at 0.63–0.95. So a page is the OCR's
//! when it reads at [`CONFIDENT`] or above, and the vision model's otherwise ([`Agent`]'s reading
//! turn decides; this module only reads).
//!
//! **Where its pieces live.** The three model files (detector, recogniser, dictionary) are in
//! `<tools dir>/ocr/`, fetched with checksums like everything else (`host.rs`). ONNX Runtime is opened
//! at run time: on a computer from the same folder (fetched beside the model runtime), on a phone
//! from the APK's native-library dir, where formicaria's packaging puts `libonnxruntime.so`
//! ([`BUNDLED_ONNXRUNTIME`]). Anything missing means no OCR, never an error: the vision model reads.
//!
//! [`Agent`]: crate::Agent

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The subfolder of the tools dir that holds the OCR's files (and, on a computer, ONNX Runtime).
pub const DIR: &str = "ocr";

/// ONNX Runtime's file name in a phone's native-library dir: formicaria stages it into the APK from
/// the catalogue's `runtime_url_onnxruntime_android_arm64`, the AAR Microsoft publishes (contract 1.6).
pub const BUNDLED_ONNXRUNTIME: &str = "libonnxruntime.so";

/// A page is the OCR's at this mean confidence or above (measured 2026-10-19: every typed page of
/// the eval at 0.99–1.00, handwriting at 0.63–0.95).
pub const CONFIDENT: f32 = 0.97;

/// What the OCR read on one page: its text in reading order, and the mean confidence of its lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub text: String,
    pub confidence: f32,
}

impl Page {
    /// Whether this reading stands on its own (see [`CONFIDENT`]).
    pub fn confident(&self) -> bool {
        !self.text.trim().is_empty() && self.confidence >= CONFIDENT
    }
}

/// Something that reads a page image into text with a confidence — the OCR, or a test's fake.
pub trait ReadPage: Send + Sync {
    /// `image` is the encoded image (JPEG, PNG or WebP), as stored in the vault.
    fn read_page(&self, image: &[u8]) -> Result<Page, String>;
    /// What read it, for the provenance line under the text (e.g. `PP-OCRv6 small`).
    fn model(&self) -> &str;
}

/// One recognised region: where it is and what it says.
#[derive(Debug, Clone)]
pub struct Piece {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub text: String,
    pub conf: f32,
}

/// **Reading order**: regions whose vertical centres fall within half a typical line height of a
/// line's are one line, read left to right; lines top to bottom. Returns the page's text and the mean
/// confidence of its regions (0 for a page with none).
pub fn reading_order(mut pieces: Vec<Piece>) -> Page {
    if pieces.is_empty() {
        return Page { text: String::new(), confidence: 0.0 };
    }
    let confidence = pieces.iter().map(|p| p.conf).sum::<f32>() / pieces.len() as f32;
    let mut heights: Vec<f32> = pieces.iter().map(|p| p.bottom - p.top).collect();
    heights.sort_by(f32::total_cmp);
    let tol = heights[heights.len() / 2] * 0.5;
    let mid = |p: &Piece| (p.top + p.bottom) / 2.0;
    pieces.sort_by(|a, b| mid(a).total_cmp(&mid(b)));
    let mut lines: Vec<Vec<Piece>> = Vec::new();
    for p in pieces {
        let joins = lines.last().is_some_and(|line| {
            let c = line.iter().map(mid).sum::<f32>() / line.len() as f32;
            (mid(&p) - c).abs() <= tol
        });
        match lines.last_mut() {
            Some(line) if joins => line.push(p),
            _ => lines.push(vec![p]),
        }
    }
    let text = lines
        .into_iter()
        .map(|mut l| {
            l.sort_by(|a, b| a.left.total_cmp(&b.left));
            l.into_iter().map(|p| p.text).collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Page { text, confidence }
}

/// Where this device's ONNX Runtime is, if it has one: in a phone's `bundle` (its native-library
/// dir), else the library the catalogue pins for this computer, inside `<dir>/ocr/`.
pub fn onnxruntime_at(
    dir: &Path,
    manifest: &crate::manifest::Manifest,
    bundle: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(native) = bundle {
        let p = native.join(BUNDLED_ONNXRUNTIME);
        return p.exists().then_some(p);
    }
    let lib = manifest.onnxruntime_lib(crate::manifest::Manifest::platform_key()?)?;
    let p = dir.join(DIR).join(lib);
    p.exists().then_some(p)
}

/// **Open the OCR if this device has all of it** — the three files the catalogue pins, in
/// `<dir>/ocr/`, and ONNX Runtime (`bundle`: a phone's native-library dir). `Err` says what is
/// missing or what failed, for the log; the caller goes on without the OCR either way.
pub fn open(
    dir: &Path,
    manifest: &crate::manifest::Manifest,
    bundle: Option<&Path>,
) -> Result<Arc<dyn ReadPage>, String> {
    let files = manifest.ocr_files();
    let [det, rec, dict] = files.as_slice() else {
        return Err("the catalogue pins no OCR".into());
    };
    let path = |f: &crate::manifest::OcrFile| dir.join(DIR).join(&f.name);
    for f in [det, rec, dict] {
        if !path(f).exists() {
            return Err(format!("{} is not downloaded", f.name));
        }
    }
    let lib = onnxruntime_at(dir, manifest, bundle).ok_or("ONNX Runtime is not on this device")?;
    open_files(&lib, &path(det), &path(rec), &path(dict))
}

#[cfg(feature = "ocr")]
fn open_files(
    lib: &Path,
    det: &Path,
    rec: &Path,
    dict: &Path,
) -> Result<Arc<dyn ReadPage>, String> {
    Ok(Arc::new(PpOcr::open(lib, det, rec, dict)?))
}

#[cfg(not(feature = "ocr"))]
fn open_files(_: &Path, _: &Path, _: &Path, _: &Path) -> Result<Arc<dyn ReadPage>, String> {
    Err("this build has no OCR (feature `ocr` off)".into())
}

/// PP-OCRv6 small through `oar-ocr`.
#[cfg(feature = "ocr")]
pub struct PpOcr {
    // One page at a time: a reading turn reads its pages in turn anyway, and a lock keeps the
    // engine's thread-safety out of the question.
    engine: std::sync::Mutex<oar_ocr::oarocr::OAROCR>,
}

#[cfg(feature = "ocr")]
impl PpOcr {
    /// Load ONNX Runtime from `lib` (once per process), then the detector, recogniser and
    /// dictionary.
    pub fn open(lib: &Path, det: &Path, rec: &Path, dict: &Path) -> Result<Self, String> {
        static LOADED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        LOADED
            .get_or_init(|| {
                ort::init_from(lib)
                    .map(|b| {
                        b.commit();
                    })
                    .map_err(|e| format!("cannot load ONNX Runtime from {}: {e}", lib.display()))
            })
            .clone()?;
        let config = oar_ocr::core::config::OrtSessionConfig::new().with_intra_threads(4);
        let engine =
            oar_ocr::oarocr::OAROCRBuilder::new(det.to_path_buf(), rec.to_path_buf(), dict)
                .ort_session(config)
                .build()
                .map_err(|e| format!("cannot open the OCR models: {e}"))?;
        Ok(Self { engine: std::sync::Mutex::new(engine) })
    }
}

#[cfg(feature = "ocr")]
impl ReadPage for PpOcr {
    fn read_page(&self, image: &[u8]) -> Result<Page, String> {
        let page = image::load_from_memory(image)
            .map_err(|e| format!("cannot decode the image: {e}"))?
            .to_rgb8();
        let engine = self.engine.lock().map_err(|_| "the OCR failed earlier".to_string())?;
        let result = engine.predict(vec![page]).map_err(|e| format!("the OCR failed: {e}"))?;
        let mut pieces = Vec::new();
        for r in result.first().map(|r| r.text_regions.as_slice()).unwrap_or_default() {
            if let Some((text, conf)) = r.text_with_confidence() {
                let ys = r.bounding_box.points.iter().map(|p| p.y);
                let top = ys.clone().fold(f32::MAX, f32::min);
                let bottom = ys.fold(f32::MIN, f32::max);
                let left = r.bounding_box.points.iter().map(|p| p.x).fold(f32::MAX, f32::min);
                pieces.push(Piece { top, bottom, left, text: text.to_string(), conf });
            }
        }
        Ok(reading_order(pieces))
    }

    fn model(&self) -> &str {
        "PP-OCRv6 small"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(top: f32, left: f32, text: &str, conf: f32) -> Piece {
        Piece { top, bottom: top + 20.0, left, text: text.into(), conf }
    }

    #[test]
    fn regions_are_read_in_lines_top_to_bottom_and_left_to_right() {
        let page = reading_order(vec![
            piece(52.0, 200.0, "world", 1.0),
            piece(10.0, 300.0, "page", 0.98),
            piece(50.0, 10.0, "hello", 1.0),
            piece(12.0, 10.0, "A", 0.96),
        ]);
        assert_eq!(page.text, "A page\nhello world");
        assert!((page.confidence - 0.985).abs() < 1e-4);
    }

    #[test]
    fn only_a_confident_page_with_text_stands_on_its_own() {
        assert!(Page { text: "typed".into(), confidence: 0.99 }.confident());
        assert!(!Page { text: "scrawl".into(), confidence: 0.9 }.confident());
        assert!(!Page { text: "  ".into(), confidence: 1.0 }.confident(), "nothing read");
        assert!(!reading_order(Vec::new()).confident());
    }
}
