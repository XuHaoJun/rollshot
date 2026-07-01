//! # rollshot-ocr
//!
//! Unsafe-isolation crate wrapping RapidOCR (`paddle-ocr-rs`) + ONNX Runtime
//! (`ort`). The public API is safe and returns primitives only (no rollshot
//! deps), so `rollshot-vision` stays `forbid(unsafe_code)`.
//!
//! Coordinate convention (eng-review D6/D15): `detect` upscales small input by
//! `min_scale`, runs OCR, then divides coordinates back, so `OcrDetection` is
//! always in the INPUT image's native pixel space.

use std::sync::OnceLock;

use image::RgbImage;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use paddle_ocr_rs::{ocr_lite::OcrLite, ocr_result::Point};
use sha2::{Digest, Sha256};
use thiserror::Error;

const DET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ch_PP-OCRv4_det_infer.onnx"));
const CLS: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/ch_ppocr_mobile_v2.0_cls_infer.onnx"
));
const REC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ch_PP-OCRv4_rec_infer.onnx"));

const DET_SHA256: &str = "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9";
const CLS_SHA256: &str = "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c";
const REC_SHA256: &str = "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b";
const MAX_UPSCALED_PIXELS: u64 = 16_000_000;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("ocr session init failed")]
    SessionInit,
    #[error("ocr detection failed")]
    Detect,
    #[error("invalid image")]
    InvalidImage,
    #[error("bundled model hash mismatch")]
    ModelHashMismatch,
}

/// Detection in the INPUT image's native pixel space (upscale already inverted).
#[derive(Debug, Clone, PartialEq)]
pub struct OcrDetection {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub quad: [(f32, f32); 4],
    pub text: String,
    pub confidence: f32,
}

/// Detector knobs. `Default` encodes the snow-shot-validated screenshot params
/// (spec §4.4). `max_side_len == 0` ⇒ paddle uses the image's own longest side
/// (no downscale — the tall-capture fix, eng-review D1).
#[derive(Debug, Clone, Copy)]
pub struct OcrRegionQuery {
    pub padding: u32,
    pub max_side_len: u32,
    pub min_scale: f32,
    pub box_score_thresh: f32,
    pub box_thresh: f32,
    pub unclip_ratio: f32,
    pub do_angle: bool,
}

impl Default for OcrRegionQuery {
    fn default() -> Self {
        Self {
            padding: 50,
            max_side_len: 0,
            min_scale: 1.5,
            box_score_thresh: 0.5,
            box_thresh: 0.3,
            unclip_ratio: 1.6,
            do_angle: false,
        }
    }
}

pub struct OcrEngine {
    ocr: OcrLite,
}

impl std::fmt::Debug for OcrEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OcrEngine { .. }")
    }
}

fn verify_bundled_hashes_once() -> Result<(), OcrError> {
    static OK: OnceLock<bool> = OnceLock::new();
    // include_bytes! constants can't change at runtime, so hash once per process
    // (eng-review D12); guards against binary corruption.
    let ok = *OK.get_or_init(|| {
        let check = |bytes: &[u8], want: &str| {
            let got: String = Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            got == want
        };
        check(DET, DET_SHA256) && check(CLS, CLS_SHA256) && check(REC, REC_SHA256)
    });
    if ok {
        Ok(())
    } else {
        Err(OcrError::ModelHashMismatch)
    }
}

fn build_session(builder: SessionBuilder) -> Result<SessionBuilder, ort::Error> {
    let threads = num_cpus::get_physical().max(1);
    builder
        .with_inter_threads(threads)?
        .with_intra_threads(threads)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
}

/// Upscale factor for the small-text trick. Only ever **upscales** (never below
/// `1.0`): a requested `min_scale` is capped so the working image stays within
/// `MAX_UPSCALED_PIXELS`, and an input already at/over that budget is used at
/// native size — never downscaled. The host `MAX_OCR_AREA` bounds the input, so
/// an accepted region's working image stays under the budget.
fn effective_scale(width: u32, height: u32, min_scale: f32) -> f32 {
    let requested = min_scale.max(1.0);
    let pixels = (width as u64).saturating_mul(height as u64).max(1);
    let cap_scale = ((MAX_UPSCALED_PIXELS as f64) / (pixels as f64)).sqrt() as f32;
    requested.min(cap_scale).max(1.0)
}

impl OcrEngine {
    pub fn new() -> Result<Self, OcrError> {
        verify_bundled_hashes_once()?;
        let mut ocr = OcrLite::new();
        // Match Snow Shot's validated RapidOCR/ORT shape: explicit session
        // thread counts and Level3 graph optimizations instead of relying on
        // default builder behavior.
        ocr.init_models_from_memory_custom(DET, CLS, REC, build_session)
            .map_err(|_| OcrError::SessionInit)?;
        Ok(Self { ocr })
    }

    /// Detect text in `img`. Returns bounds in `img`'s native pixel space.
    pub fn detect(
        &mut self,
        img: &RgbImage,
        query: &OcrRegionQuery,
    ) -> Result<Vec<OcrDetection>, OcrError> {
        if img.width() == 0 || img.height() == 0 {
            return Err(OcrError::InvalidImage);
        }
        let scale = effective_scale(img.width(), img.height(), query.min_scale);
        let result = if (scale - 1.0).abs() > f32::EPSILON {
            let nw = ((img.width() as f32) * scale).round().max(1.0) as u32;
            let nh = ((img.height() as f32) * scale).round().max(1.0) as u32;
            let work = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Lanczos3);
            self.ocr.detect(
                &work,
                query.padding,
                query.max_side_len,
                query.box_score_thresh,
                query.box_thresh,
                query.unclip_ratio,
                query.do_angle,
                false,
            )
        } else {
            self.ocr.detect(
                img,
                query.padding,
                query.max_side_len,
                query.box_score_thresh,
                query.box_thresh,
                query.unclip_ratio,
                query.do_angle,
                false,
            )
        }
        .map_err(|_| OcrError::Detect)?;

        let mut out = Vec::with_capacity(result.text_blocks.len());
        for block in &result.text_blocks {
            if block.box_points.len() != 4 {
                continue;
            }
            let (x, y, w, h) = aabb(&block.box_points);
            let quad = [
                (
                    block.box_points[0].x as f32 / scale,
                    block.box_points[0].y as f32 / scale,
                ),
                (
                    block.box_points[1].x as f32 / scale,
                    block.box_points[1].y as f32 / scale,
                ),
                (
                    block.box_points[2].x as f32 / scale,
                    block.box_points[2].y as f32 / scale,
                ),
                (
                    block.box_points[3].x as f32 / scale,
                    block.box_points[3].y as f32 / scale,
                ),
            ];
            // Invert the upscale → input-native coordinates (eng-review D6/D15).
            let (x, y, w, h) = (x / scale, y / scale, w / scale, h / scale);
            if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) {
                continue;
            }
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            out.push(OcrDetection {
                x,
                y,
                w,
                h,
                quad,
                text: block.text.clone(),
                confidence: block.text_score,
            });
        }
        Ok(out)
    }
}

/// Axis-aligned bounding box of a paddle 4-point quad.
fn aabb(points: &[Point]) -> (f32, f32, f32, f32) {
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in points {
        let (px, py) = (p.x as f32, p.y as f32);
        min_x = min_x.min(px);
        min_y = min_y.min(py);
        max_x = max_x.max(px);
        max_y = max_y.max(py);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::FontRef;
    use image::{Rgb, RgbImage};
    use imageproc::drawing::{draw_text_mut, text_size};

    // Vendored deterministic font (workspace asset), read at test time.
    const FONT: &[u8] = include_bytes!("../../rollshot-image-document/assets/fonts/DejaVuSans.ttf");

    /// White image with black `text` at (x,y); returns the rendered text box.
    fn text_image(
        w: u32,
        h: u32,
        x: i32,
        y: i32,
        px: f32,
        text: &str,
    ) -> (RgbImage, (u32, u32, u32, u32)) {
        let font = FontRef::try_from_slice(FONT).unwrap();
        let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
        draw_text_mut(&mut img, Rgb([0, 0, 0]), x, y, px, &font, text);
        let (tw, th) = text_size(px, &font, text);
        (img, (x as u32, y as u32, tw, th))
    }

    #[test]
    fn engine_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<OcrEngine>();
    }

    #[test]
    fn default_query_matches_screenshot_params() {
        let q = OcrRegionQuery::default();
        assert_eq!(q.padding, 50);
        assert_eq!(q.max_side_len, 0);
        assert_eq!(q.min_scale, 1.5);
        assert_eq!(q.box_score_thresh, 0.5);
        assert_eq!(q.box_thresh, 0.3);
        assert_eq!(q.unclip_ratio, 1.6);
        assert!(!q.do_angle);
    }

    #[test]
    fn bundled_model_hashes_match() {
        verify_bundled_hashes_once().unwrap();
    }

    #[test]
    fn new_succeeds() {
        OcrEngine::new().unwrap();
    }

    #[test]
    fn detect_reads_text_with_valid_shape() {
        let (img, _) = text_image(640, 160, 20, 50, 48.0, "Hello OCR");
        let mut engine = OcrEngine::new().unwrap();
        let dets = engine.detect(&img, &OcrRegionQuery::default()).unwrap();
        assert!(!dets.is_empty(), "expected ≥1 detection");
        for d in &dets {
            assert!(d.w.is_finite() && d.h.is_finite() && d.w > 0.0 && d.h > 0.0);
            assert!((0.0..=1.0).contains(&d.confidence));
        }
    }

    #[test]
    fn detect_on_blank_returns_zero() {
        let img = RgbImage::from_pixel(320, 120, Rgb([255, 255, 255]));
        let mut engine = OcrEngine::new().unwrap();
        assert_eq!(
            engine
                .detect(&img, &OcrRegionQuery::default())
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn detect_rejects_zero_dim() {
        let img = RgbImage::new(0, 0);
        let mut engine = OcrEngine::new().unwrap();
        assert!(matches!(
            engine.detect(&img, &OcrRegionQuery::default()),
            Err(OcrError::InvalidImage)
        ));
    }

    #[test]
    fn effective_scale_upscales_small_never_downscales_large() {
        // Small input: full requested upscale.
        assert_eq!(effective_scale(300, 90, 1.5), 1.5);
        // Input where 1.5× would exceed the working-image budget: the upscale is
        // capped between 1.0 and 1.5 so the working image stays within budget.
        let capped = effective_scale(3600, 3000, 1.5);
        assert!((1.0..1.5).contains(&capped));
        let working = (3600.0 * capped) * (3000.0 * capped);
        assert!(working <= MAX_UPSCALED_PIXELS as f32 + 1024.0);
        // Input already over the budget is used at native size — never downscaled.
        assert_eq!(effective_scale(8000, 4000, 1.5), 1.0);
    }

    #[test]
    fn detection_preserves_four_point_quad_after_upscale_inversion() {
        let mut engine = OcrEngine::new().unwrap();
        let (img, _) = text_image(300, 90, 12, 28, 32.0, "acct 12345");
        let detections = engine.detect(&img, &OcrRegionQuery::default()).unwrap();
        let first = detections.first().expect("expected OCR text");

        assert_eq!(first.quad.len(), 4);
        for p in first.quad {
            assert!(p.0.is_finite());
            assert!(p.1.is_finite());
            assert!(p.0 >= 0.0);
            assert!(p.1 >= 0.0);
            assert!(p.0 <= img.width() as f32);
            assert!(p.1 <= img.height() as f32);
        }
    }

    #[test]
    fn upscale_inversion_keeps_native_coords() {
        // A small image: default min_scale=1.5 upscales internally; bounds must
        // come back in the SMALL image's coordinate space and overlap the text.
        let (img, (tx, ty, tw, th)) = text_image(300, 90, 12, 28, 32.0, "INVOICE");
        let mut engine = OcrEngine::new().unwrap();
        let dets = engine.detect(&img, &OcrRegionQuery::default()).unwrap();
        assert!(!dets.is_empty());
        let d = &dets[0];
        // Detected box is within the small (300x90) image, not 1.5x larger.
        assert!(d.x + d.w <= 300.0 + 4.0 && d.y + d.h <= 90.0 + 4.0);
        // ...and overlaps the rendered text box.
        let overlap = (d.x < (tx + tw) as f32)
            && ((d.x + d.w) > tx as f32)
            && (d.y < (ty + th) as f32)
            && ((d.y + d.h) > ty as f32);
        assert!(
            overlap,
            "detection {d:?} should overlap text box {:?}",
            (tx, ty, tw, th)
        );
    }
}
