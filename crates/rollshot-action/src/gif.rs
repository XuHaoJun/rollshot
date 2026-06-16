//! Basic summary-GIF export: assemble the final guide's keyframes into one
//! infinitely-looping GIF. A visual companion to `steps.md`, built from the
//! reviewed/edited keyframes only — never from the raw frame stream. One frame
//! per step, fixed dwell, downscaled to a max width for predictable size.

use std::path::Path;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};

use crate::error::GifError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;

/// Tunables for summary-GIF assembly. `Default` is the P0.5 "basic" profile.
#[derive(Debug, Clone)]
pub struct GifOptions {
    /// Per-frame display time, milliseconds.
    pub frame_dwell_ms: u32,
    /// Frames wider than this are downscaled (aspect preserved); never upscaled.
    pub max_width: u32,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self {
            frame_dwell_ms: 1500,
            max_width: 800,
        }
    }
}

/// Encode the guide's keyframes into an infinitely-looping GIF at `out_path`.
/// One frame per guide step, in order, using each step's current keyframe.
/// Writes atomically (temp sibling + rename); on any error nothing is left at
/// `out_path` and the editable guide/store are untouched.
pub fn export_gif(
    guide: &Guide,
    store: &FrameStore,
    opts: GifOptions,
    out_path: &Path,
) -> Result<(), GifError> {
    if guide.is_empty() {
        return Err(GifError::Empty);
    }

    // One (possibly downscaled) RGBA frame per step, in order.
    let mut images = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let retained = store
            .retained(step.keyframe)
            .ok_or(GifError::KeyframeMissing { index: i + 1 })?;
        images.push(downscale(&retained.image, opts.max_width));
    }

    // Encode into an in-memory buffer. The encoder is scoped so it is dropped
    // (and the GIF trailer flushed) before the buffer is read.
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut buf);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|source| GifError::Encode { source })?;
        for image in images {
            let delay = Delay::from_numer_denom_ms(opts.frame_dwell_ms, 1);
            encoder
                .encode_frame(Frame::from_parts(image, 0, 0, delay))
                .map_err(|source| GifError::Encode { source })?;
        }
    }

    write_atomic(out_path, &buf)
}

/// Downscale `image` so its width is at most `max_width`, preserving aspect
/// ratio. Never upscales.
fn downscale(image: &RgbaImage, max_width: u32) -> RgbaImage {
    let width = image.width();
    if width == 0 || width <= max_width {
        return image.clone();
    }
    let height = (image.height() as u64 * max_width as u64 / width as u64).max(1) as u32;
    image::imageops::resize(
        image,
        max_width,
        height,
        image::imageops::FilterType::Triangle,
    )
}

/// Write `bytes` to `path` atomically: a temp sibling first, then rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), GifError> {
    let tmp = path.with_extension("gif.tmp");
    std::fs::write(&tmp, bytes).map_err(|source| GifError::Io {
        path: tmp.display().to_string(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        GifError::Io {
            path: path.display().to_string(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::StoreConfig;
    use crate::models::{CandidateKind, CandidateStep, CaptureRegion, DetectReason};
    use crate::recorder::{ActionRecorder, Recording};
    use image::codecs::gif::GifDecoder;
    use image::{AnimationDecoder, Rgba};
    use std::path::PathBuf;

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }
    }
    fn black() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))
    }
    fn quadrant() -> RgbaImage {
        let mut img = black();
        for y in 0..4 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }
    fn temp_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "rollshot-gif-{label}-{nanos}-{}.gif",
            std::process::id()
        ))
    }

    /// A real recording with retained frames (mirrors the export.rs fixture).
    fn recording() -> Recording {
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut rec = ActionRecorder::new(region(), StoreConfig::default(), det);
        rec.ingest_frame(black(), 0);
        for i in 1..=6 {
            rec.ingest_frame(quadrant(), i * 100);
        }
        let recording = rec.finish();
        assert!(!recording.candidates.is_empty());
        recording
    }

    fn one_step_guide(kf: crate::models::FrameId) -> Guide {
        Guide::from_candidates(vec![CandidateStep {
            id: 0,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 0,
            keyframe: kf,
            nearby: vec![kf],
        }])
    }

    fn decode_frames(path: &PathBuf) -> Vec<image::Frame> {
        let file = std::fs::File::open(path).expect("open gif");
        GifDecoder::new(std::io::BufReader::new(file))
            .expect("gif decoder")
            .into_frames()
            .collect_frames()
            .expect("collect frames")
    }

    #[test]
    fn exports_one_frame_per_step() {
        let store = recording().store;
        let kf = store.retained_ids_for_test()[0];
        let guide = Guide::from_candidates(vec![
            CandidateStep {
                id: 0,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 0,
                keyframe: kf,
                nearby: vec![kf],
            },
            CandidateStep {
                id: 1,
                kind: CandidateKind::Scroll,
                reason: DetectReason::ScrollSettled,
                at_ms: 100,
                keyframe: kf,
                nearby: vec![kf],
            },
        ]);
        let path = temp_path("two-steps");
        export_gif(&guide, &store, GifOptions::default(), &path).expect("export");
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        assert_eq!(decode_frames(&path).len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn downscales_frames_wider_than_max_width() {
        let store = recording().store;
        let guide = one_step_guide(store.retained_ids_for_test()[0]);
        let path = temp_path("downscale");
        export_gif(
            &guide,
            &store,
            GifOptions {
                frame_dwell_ms: 100,
                max_width: 4,
            },
            &path,
        )
        .expect("export");
        assert_eq!(decode_frames(&path)[0].buffer().width(), 4);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn keeps_frames_narrower_than_max_width() {
        let store = recording().store;
        let guide = one_step_guide(store.retained_ids_for_test()[0]);
        let path = temp_path("native");
        export_gif(
            &guide,
            &store,
            GifOptions {
                frame_dwell_ms: 100,
                max_width: 100,
            },
            &path,
        )
        .expect("export");
        assert_eq!(decode_frames(&path)[0].buffer().width(), 8);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_guide_is_an_error() {
        let store = FrameStore::new(StoreConfig::default());
        let guide = Guide::from_candidates(vec![]);
        let path = temp_path("empty");
        let result = export_gif(&guide, &store, GifOptions::default(), &path);
        assert!(matches!(result, Err(GifError::Empty)));
        assert!(!path.exists());
    }

    #[test]
    fn missing_keyframe_errors_and_leaves_no_file() {
        let store = recording().store;
        let kf = store.retained_ids_for_test()[0];
        // Step 1 is exportable; step 2's keyframe is not retained -> fails before
        // anything is written, so no partial GIF is left at the target path.
        let guide = Guide::from_candidates(vec![
            CandidateStep {
                id: 0,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 0,
                keyframe: kf,
                nearby: vec![kf],
            },
            CandidateStep {
                id: 1,
                kind: CandidateKind::UiChanged,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                keyframe: 999_999, // not retained -> injected failure
                nearby: vec![999_999],
            },
        ]);
        let path = temp_path("missing-keyframe");
        let result = export_gif(&guide, &store, GifOptions::default(), &path);
        assert!(matches!(
            result,
            Err(GifError::KeyframeMissing { index: 2 })
        ));
        assert!(!path.exists(), "no partial GIF on error");
    }
}
