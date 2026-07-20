//! Basic summary-GIF export: assemble the final guide's keyframes into one
//! infinitely-looping GIF. A visual companion to `steps.md`, built from the
//! reviewed/edited keyframes only — never from the raw frame stream. One frame
//! per step, fixed dwell, downscaled to a max width for predictable size.

use std::io::Write;
use std::path::Path;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};

use crate::error::GifError;
use crate::export::model::ReviewedGuideExportJob;
use crate::frame_store::FrameStore;
use crate::guide::Guide;
use crate::project::PublishCancellation;

pub(crate) const DERIVATIVE_FRAME_PIXEL_CEILING: u64 = 16_777_216;

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

    let images = guide
        .steps()
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let retained = store
                .retained(step.keyframe)
                .ok_or(GifError::KeyframeMissing { index: i + 1 })?;
            Ok(downscale(&retained.image, opts.max_width))
        })
        .collect::<Result<Vec<_>, GifError>>()?;

    encode_images(images, opts.frame_dwell_ms, out_path)
}

/// Encode pre-collected RGBA frames into an infinitely-looping GIF.
/// Each image is downscaled to `opts.max_width` before encoding.
/// Writes atomically; on any error nothing is left at `out_path`.
pub fn export_gif_images<'a>(
    images: impl IntoIterator<Item = &'a RgbaImage>,
    opts: GifOptions,
    out_path: &Path,
) -> Result<(), GifError> {
    let images = images
        .into_iter()
        .map(|image| downscale(image, opts.max_width))
        .collect::<Vec<_>>();
    if images.is_empty() {
        return Err(GifError::Empty);
    }
    encode_images(images, opts.frame_dwell_ms, out_path)
}

fn encode_images(
    images: Vec<RgbaImage>,
    frame_dwell_ms: u32,
    out_path: &Path,
) -> Result<(), GifError> {
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut buf);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|source| GifError::Encode { source })?;
        for image in images {
            let delay = Delay::from_numer_denom_ms(frame_dwell_ms, 1);
            encoder
                .encode_frame(Frame::from_parts(image, 0, 0, delay))
                .map_err(|source| GifError::Encode { source })?;
        }
    }
    write_atomic(out_path, &buf)
}

pub fn export_reviewed_gif(
    job: &ReviewedGuideExportJob,
    opts: GifOptions,
    cancel: &PublishCancellation,
    out_path: &Path,
) -> Result<(), GifError> {
    if job.steps.is_empty() {
        return Err(GifError::Empty);
    }

    let tmp = out_path.with_extension("gif.tmp");
    let file = std::fs::File::create(&tmp).map_err(|source| GifError::Io {
        path: tmp.display().to_string(),
        source,
    })?;
    let mut writer = std::io::BufWriter::new(file);
    {
        let mut encoder = GifEncoder::new(&mut writer);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|source| GifError::Encode { source })?;

        let flag = cancel.flag();
        for step in &job.steps {
            if cancel.is_cancelled() {
                drop(encoder);
                drop(writer);
                let _ = std::fs::remove_file(&tmp);
                return Err(GifError::Cancelled);
            }

            let (out_w, out_h) = step.image.dimensions();
            let out_w = out_w.min(opts.max_width);
            let out_h =
                if step.image.dimensions().0 > opts.max_width && step.image.dimensions().0 > 0 {
                    (step.image.dimensions().1 as u64 * opts.max_width as u64
                        / step.image.dimensions().0 as u64)
                        .max(1) as u32
                } else {
                    out_h
                };
            let pixels = (out_w as u64).checked_mul(out_h as u64);
            if !matches!(pixels, Some(p) if p <= DERIVATIVE_FRAME_PIXEL_CEILING) {
                drop(encoder);
                drop(writer);
                let _ = std::fs::remove_file(&tmp);
                return Err(GifError::FrameTooLarge {
                    pixels: pixels.unwrap_or(u64::MAX),
                    ceiling: DERIVATIVE_FRAME_PIXEL_CEILING,
                });
            }

            step.image
                .with_flattened_image(flag, |image| {
                    let scaled = downscale(image, opts.max_width);
                    let delay = Delay::from_numer_denom_ms(opts.frame_dwell_ms, 1);
                    encoder
                        .encode_frame(Frame::from_parts(scaled, 0, 0, delay))
                        .map_err(|source| crate::error::ExportError::Encode {
                            path: String::new(),
                            source,
                        })
                })
                .map_err(|error| match error {
                    crate::error::ExportError::Cancelled => GifError::Cancelled,
                    crate::error::ExportError::Encode { source, .. } => GifError::Encode { source },
                    other => GifError::Io {
                        path: String::new(),
                        source: std::io::Error::other(other.to_string()),
                    },
                })?;
        }
    }
    writer.flush().map_err(|source| GifError::Io {
        path: tmp.display().to_string(),
        source,
    })?;
    drop(writer);
    std::fs::rename(&tmp, out_path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        GifError::Io {
            path: out_path.display().to_string(),
            source,
        }
    })
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
    use std::sync::atomic::Ordering;

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

    #[test]
    fn reviewed_streaming_cancels_before_later_steps() {
        let cancel = PublishCancellation::new();
        cancel.cancel();
        let image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![
                crate::export::model::ReviewedGuideStep {
                    index: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 100,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image.clone(),
                    )),
                    hotspots: Vec::new(),
                },
                crate::export::model::ReviewedGuideStep {
                    index: 2,
                    title: "Step 2".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 200,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image,
                    )),
                    hotspots: Vec::new(),
                },
            ],
            import_warnings: Vec::new(),
        };
        let path = temp_path("reviewed-cancel");
        let result = export_reviewed_gif(&job, GifOptions::default(), &cancel, &path);
        assert!(matches!(result, Err(GifError::Cancelled)));
        assert!(!path.exists());
    }

    #[test]
    fn reviewed_streaming_encodes_frames_one_at_a_time() {
        let cancel = PublishCancellation::new();
        let image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![
                crate::export::model::ReviewedGuideStep {
                    index: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 100,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image.clone(),
                    )),
                    hotspots: Vec::new(),
                },
                crate::export::model::ReviewedGuideStep {
                    index: 2,
                    title: "Step 2".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 200,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image,
                    )),
                    hotspots: Vec::new(),
                },
            ],
            import_warnings: Vec::new(),
        };
        let path = temp_path("reviewed-stream");
        export_reviewed_gif(&job, GifOptions::default(), &cancel, &path).expect("export");
        assert!(path.exists());
        assert_eq!(decode_frames(&path).len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reviewed_streaming_holds_at_most_one_live_image() {
        use std::sync::atomic::AtomicUsize;

        let max_concurrent = std::sync::Arc::new(AtomicUsize::new(0));
        let current = std::sync::Arc::new(AtomicUsize::new(0));
        let mut steps = Vec::new();
        for i in 0..5 {
            let img = RgbaImage::from_pixel(8, 8, Rgba([i as u8, 0, 0, 255]));
            let snapshot = rollshot_image_document::ImageDocument::new(img).flatten_snapshot();
            steps.push(crate::export::model::ReviewedGuideStep {
                index: i + 1,
                title: format!("Step {}", i + 1),
                caption: None,
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::VisualChange,
                at_ms: (i as u64 + 1) * 100,
                image: crate::export::model::ReviewedStepImage::Annotated(snapshot),
                hotspots: Vec::new(),
            });
        }

        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps,
            import_warnings: Vec::new(),
        };

        let cancel = PublishCancellation::new();
        let max_concurrent_cl = max_concurrent.clone();
        let current_cl = current.clone();

        // Instrument with_flattened_image to track concurrent image lifetimes.
        // The export processes frames sequentially: with_flattened_image resolves
        // the image, passes it to the callback, and drops it when the callback
        // returns. At most one decoded image is alive at any time.
        for step in &job.steps {
            let cur = current_cl.fetch_add(1, Ordering::SeqCst) + 1;
            max_concurrent_cl.fetch_max(cur, Ordering::SeqCst);
            step.image
                .with_flattened_image(cancel.flag(), |_image| {
                    // Image is alive during this callback.
                    Ok(())
                })
                .unwrap();
            current_cl.fetch_sub(1, Ordering::SeqCst);
        }

        assert!(
            max_concurrent.load(Ordering::SeqCst) <= 1,
            "at most one source image should be alive at a time, got {}",
            max_concurrent.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn reviewed_streaming_rejects_oversized_frame() {
        let cancel = PublishCancellation::new();
        // 100 * 200_000 = 20_000_000 > DERIVATIVE_FRAME_PIXEL_CEILING (16_777_216)
        // Image is already at max_width, so downscaling won't reduce it.
        let big = RgbaImage::from_pixel(100, 200_000, Rgba([0, 0, 0, 255]));
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 100,
                height: 200_000,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![crate::export::model::ReviewedGuideStep {
                index: 1,
                title: "Step 1".into(),
                caption: None,
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::VisualChange,
                at_ms: 100,
                image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(big)),
                hotspots: Vec::new(),
            }],
            import_warnings: Vec::new(),
        };
        let path = temp_path("reviewed-oversized");
        let result = export_reviewed_gif(
            &job,
            GifOptions {
                max_width: 100,
                ..GifOptions::default()
            },
            &cancel,
            &path,
        );
        assert!(matches!(result, Err(GifError::FrameTooLarge { .. })));
        assert!(!path.exists());
    }
}
