//! Storyboard PNG export: assemble the final guide's reviewed keyframes into a
//! single vertical, chat-friendly image. This is a static workflow summary, not
//! a raw frame dump.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use image::{ImageFormat, Rgba, RgbaImage};
use rollshot_image_document::{draw_text_block, measure_block, ImagePoint, Rgba8};

use crate::error::StoryboardError;
use crate::export::model::ReviewedGuideExportJob;
use crate::frame_store::FrameStore;
use crate::guide::Guide;
use crate::project::PublishCancellation;

const LABEL_FONT_PX: f32 = 26.0;
const LABEL_GAP: u32 = 10;
const CAPTION_FONT_PX: f32 = 20.0;
const CAPTION_GAP: u32 = 8;
const CAPTION_COLOR: Rgba8 = Rgba8::new(71, 79, 92, 255);
const BORDER: Rgba<u8> = Rgba([218, 223, 232, 255]);
const CARD_BACKGROUND: Rgba<u8> = Rgba([250, 251, 253, 255]);
const TEXT_COLOR: Rgba8 = Rgba8::new(20, 24, 31, 255);
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

#[derive(Debug, Clone)]
pub struct StoryboardOptions {
    pub max_width: u32,
    pub max_canvas_pixels: u64,
    pub outer_padding: u32,
    pub card_spacing: u32,
    pub card_padding: u32,
    pub show_titles: bool,
}

impl Default for StoryboardOptions {
    fn default() -> Self {
        Self {
            max_width: 1200,
            max_canvas_pixels: 24_000_000,
            outer_padding: 24,
            card_spacing: 20,
            card_padding: 16,
            show_titles: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryboardExportResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct StoryboardStep<'a> {
    pub index: usize,
    pub title: &'a str,
    pub caption: Option<&'a str>,
    pub image: &'a RgbaImage,
}

#[derive(Debug, Clone)]
pub struct StoryboardRenderResult {
    pub image: RgbaImage,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}

pub fn export_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
    out_path: &Path,
) -> Result<StoryboardExportResult, StoryboardError> {
    let rendered = render_storyboard(guide, store, opts)?;
    write_png_atomic(out_path, &rendered.image)?;
    Ok(StoryboardExportResult {
        path: out_path.to_path_buf(),
        width: rendered.width,
        height: rendered.height,
        step_count: rendered.step_count,
    })
}

pub fn render_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError> {
    if guide.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let mut steps = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let retained = store
            .retained(step.keyframe)
            .ok_or(StoryboardError::KeyframeMissing { index: i + 1 })?;
        steps.push(StoryboardStep {
            index: step.index,
            title: &step.title,
            caption: non_empty_caption(&step.caption),
            image: &retained.image,
        });
    }

    render_storyboard_steps(&steps, opts)
}

pub fn render_reviewed_storyboard(
    job: &crate::export::model::ReviewedGuideExportJob,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError> {
    if job.steps.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let mut owned = Vec::with_capacity(job.steps.len());
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for step in &job.steps {
        step.image
            .with_flattened_image(&cancel, |image| {
                owned.push(downscale(image, opts.max_width));
                Ok(())
            })
            .map_err(|error| StoryboardError::Io {
                path: String::new(),
                source: std::io::Error::other(error.to_string()),
            })?;
    }

    let steps: Vec<StoryboardStep<'_>> = job
        .steps
        .iter()
        .zip(owned.iter())
        .map(|(step, image)| StoryboardStep {
            index: step.index,
            title: &step.title,
            caption: step.caption.as_deref().and_then(non_empty_caption),
            image,
        })
        .collect();

    render_storyboard_steps(&steps, opts)
}

pub fn render_reviewed_storyboard_cancellable(
    job: &ReviewedGuideExportJob,
    opts: StoryboardOptions,
    cancel: &PublishCancellation,
) -> Result<StoryboardRenderResult, StoryboardError> {
    if job.steps.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let canvas_width = opts.max_width;
    let card_width = canvas_width
        .checked_sub(opts.outer_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;
    let content_width = card_width
        .checked_sub(opts.card_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;

    let flag = cancel.flag();
    let mut cards = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        if flag.load(Ordering::Relaxed) {
            return Err(StoryboardError::Cancelled);
        }

        let mut card_result = None;
        step.image
            .with_flattened_image(flag, |image| {
                let scaled = downscale(image, content_width);
                let label = step_label(step.index, &step.title, opts.show_titles);
                let label = fit_label(&label, content_width as f32);
                let (_, label_height) = measure_block(&label, LABEL_FONT_PX, true);
                let label_height = label_height.ceil() as u32;

                let caption = step
                    .caption
                    .as_deref()
                    .and_then(non_empty_caption)
                    .map(|caption| fit_caption(caption, content_width as f32));
                let caption_height = caption
                    .as_ref()
                    .map(|caption| measure_block(caption, CAPTION_FONT_PX, false).1.ceil() as u32)
                    .unwrap_or(0);
                let text_height = if caption.is_some() {
                    label_height
                        .checked_add(CAPTION_GAP)
                        .and_then(|height| height.checked_add(caption_height))
                        .ok_or(crate::error::ExportError::Io {
                            path: String::new(),
                            source: std::io::Error::other("canvas too large"),
                        })?
                } else {
                    label_height
                };
                let card_height = opts
                    .card_padding
                    .checked_mul(2)
                    .and_then(|height| height.checked_add(text_height))
                    .and_then(|height| height.checked_add(LABEL_GAP))
                    .and_then(|height| height.checked_add(scaled.height()))
                    .ok_or(crate::error::ExportError::Io {
                        path: String::new(),
                        source: std::io::Error::other("canvas too large"),
                    })?;
                card_result = Some(Card {
                    label,
                    caption,
                    image: scaled,
                    height: card_height,
                });
                Ok(())
            })
            .map_err(|error| match error {
                crate::error::ExportError::Cancelled => StoryboardError::Cancelled,
                crate::error::ExportError::Io { source, .. } => StoryboardError::Io {
                    path: String::new(),
                    source,
                },
                other => StoryboardError::Io {
                    path: String::new(),
                    source: std::io::Error::other(other.to_string()),
                },
            })?;

        if let Some(card) = card_result {
            cards.push(card);
        }
    }

    if flag.load(Ordering::Relaxed) {
        return Err(StoryboardError::Cancelled);
    }

    render_cards(cards, opts, canvas_width, card_width)
}

pub fn export_reviewed_storyboard_cancellable(
    job: &ReviewedGuideExportJob,
    opts: StoryboardOptions,
    cancel: &PublishCancellation,
    out_path: &Path,
) -> Result<StoryboardExportResult, StoryboardError> {
    let rendered = render_reviewed_storyboard_cancellable(job, opts, cancel)?;
    write_png_atomic(out_path, &rendered.image)?;
    Ok(StoryboardExportResult {
        path: out_path.to_path_buf(),
        width: rendered.width,
        height: rendered.height,
        step_count: rendered.step_count,
    })
}

pub fn render_storyboard_steps(
    steps: &[StoryboardStep<'_>],
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError> {
    if steps.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let canvas_width = opts.max_width;
    let card_width = canvas_width
        .checked_sub(opts.outer_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;
    let content_width = card_width
        .checked_sub(opts.card_padding.saturating_mul(2))
        .ok_or(StoryboardError::CanvasTooLarge)?;

    let mut cards = Vec::with_capacity(steps.len());
    for step in steps {
        let image = downscale(step.image, content_width);
        let label = step_label(step.index, step.title, opts.show_titles);
        let label = fit_label(&label, content_width as f32);
        let (_, label_height) = measure_block(&label, LABEL_FONT_PX, true);
        let label_height = label_height.ceil() as u32;

        let caption = step
            .caption
            .and_then(non_empty_caption)
            .map(|caption| fit_caption(caption, content_width as f32));
        let caption_height = caption
            .as_ref()
            .map(|caption| measure_block(caption, CAPTION_FONT_PX, false).1.ceil() as u32)
            .unwrap_or(0);
        let text_height = if caption.is_some() {
            label_height
                .checked_add(CAPTION_GAP)
                .and_then(|height| height.checked_add(caption_height))
                .ok_or(StoryboardError::CanvasTooLarge)?
        } else {
            label_height
        };
        let card_height = opts
            .card_padding
            .checked_mul(2)
            .and_then(|height| height.checked_add(text_height))
            .and_then(|height| height.checked_add(LABEL_GAP))
            .and_then(|height| height.checked_add(image.height()))
            .ok_or(StoryboardError::CanvasTooLarge)?;
        cards.push(Card {
            label,
            caption,
            image,
            height: card_height,
        });
    }

    render_cards(cards, opts, canvas_width, card_width)
}

fn render_cards(
    cards: Vec<Card>,
    opts: StoryboardOptions,
    canvas_width: u32,
    card_width: u32,
) -> Result<StoryboardRenderResult, StoryboardError> {
    let mut canvas_height = opts
        .outer_padding
        .checked_mul(2)
        .ok_or(StoryboardError::CanvasTooLarge)?;
    for (i, card) in cards.iter().enumerate() {
        if i > 0 {
            canvas_height = canvas_height
                .checked_add(opts.card_spacing)
                .ok_or(StoryboardError::CanvasTooLarge)?;
        }
        canvas_height = canvas_height
            .checked_add(card.height)
            .ok_or(StoryboardError::CanvasTooLarge)?;
    }
    let canvas_pixels = (canvas_width as u64)
        .checked_mul(canvas_height as u64)
        .ok_or(StoryboardError::CanvasTooLarge)?;
    if canvas_pixels > opts.max_canvas_pixels {
        return Err(StoryboardError::CanvasTooLarge);
    }

    let mut canvas = RgbaImage::from_pixel(canvas_width, canvas_height, WHITE);
    let mut y = opts.outer_padding;
    for (i, card) in cards.iter().enumerate() {
        draw_card(&mut canvas, opts.outer_padding, y, card_width, card.height);
        draw_card_content(&mut canvas, &opts, y, card);

        y += card.height;
        if i + 1 < cards.len() {
            y += opts.card_spacing;
        }
    }

    Ok(StoryboardRenderResult {
        image: canvas,
        width: canvas_width,
        height: canvas_height,
        step_count: cards.len(),
    })
}

fn draw_card_content(canvas: &mut RgbaImage, opts: &StoryboardOptions, y: u32, card: &Card) {
    let content_x = opts.outer_padding + opts.card_padding;
    let mut content_y = y + opts.card_padding;
    draw_text_block(
        canvas,
        ImagePoint::new(content_x as f32, content_y as f32),
        &card.label,
        LABEL_FONT_PX,
        true,
        TEXT_COLOR,
    );
    let (_, label_height) = measure_block(&card.label, LABEL_FONT_PX, true);
    content_y += label_height.ceil() as u32;
    if let Some(caption) = &card.caption {
        content_y += CAPTION_GAP;
        draw_text_block(
            canvas,
            ImagePoint::new(content_x as f32, content_y as f32),
            caption,
            CAPTION_FONT_PX,
            false,
            CAPTION_COLOR,
        );
        let (_, caption_height) = measure_block(caption, CAPTION_FONT_PX, false);
        content_y += caption_height.ceil() as u32;
    }
    content_y += LABEL_GAP;
    image::imageops::replace(
        canvas,
        &card.image,
        i64::from(content_x),
        i64::from(content_y),
    );
}

struct Card {
    label: String,
    caption: Option<String>,
    image: RgbaImage,
    height: u32,
}

fn step_label(index: usize, title: &str, show_titles: bool) -> String {
    if show_titles && !title.trim().is_empty() {
        format!("Step {index} - {title}")
    } else {
        format!("Step {index}")
    }
}

fn downscale(image: &RgbaImage, max_width: u32) -> RgbaImage {
    let width = image.width();
    if width == 0 || max_width == 0 || width <= max_width {
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

fn draw_card(canvas: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32) {
    for yy in y..y + height {
        for xx in x..x + width {
            let is_border = xx == x || yy == y || xx == x + width - 1 || yy == y + height - 1;
            canvas.put_pixel(xx, yy, if is_border { BORDER } else { CARD_BACKGROUND });
        }
    }
}

fn fit_label(label: &str, max_width: f32) -> String {
    fit_text(label, max_width, LABEL_FONT_PX, true)
}

fn fit_caption(caption: &str, max_width: f32) -> String {
    fit_text(caption.trim(), max_width, CAPTION_FONT_PX, false)
}

fn fit_text(text: &str, max_width: f32, px: f32, bold: bool) -> String {
    if measure_block(text, px, bold).0 <= max_width {
        return text.to_string();
    }

    let ellipsis = "...";
    let text = text.trim_end();
    let mut fitted = String::new();
    for ch in text.chars() {
        let candidate = format!("{fitted}{ch}{ellipsis}");
        if measure_block(&candidate, px, bold).0 > max_width {
            break;
        }
        fitted.push(ch);
    }
    if fitted.is_empty() {
        ellipsis.to_string()
    } else {
        format!("{fitted}{ellipsis}")
    }
}

fn non_empty_caption(caption: &str) -> Option<&str> {
    let caption = caption.trim();
    (!caption.is_empty()).then_some(caption)
}

fn write_png_atomic(path: &Path, image: &RgbaImage) -> Result<(), StoryboardError> {
    let tmp = path.with_extension("png.tmp");
    image
        .save_with_format(&tmp, ImageFormat::Png)
        .map_err(|source| {
            let _ = std::fs::remove_file(&tmp);
            StoryboardError::Encode {
                path: tmp.display().to_string(),
                source,
            }
        })?;
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        StoryboardError::Io {
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
    use image::{ImageReader, Rgba, RgbaImage};
    use rollshot_image_document::measure_block;

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

    fn guide_with_steps(keyframe: crate::models::FrameId, count: usize) -> Guide {
        let candidates = (0..count)
            .map(|i| CandidateStep {
                id: i as u64,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: (i as u64) * 100,
                keyframe,
                nearby: vec![keyframe],
            })
            .collect();
        Guide::from_candidates(candidates)
    }

    fn temp_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "rollshot-storyboard-{label}-{nanos}-{}.png",
            std::process::id()
        ))
    }

    #[test]
    fn exports_single_png_with_one_card_per_step() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 2);
        assert!(guide.rename(1, "Open settings".to_string()));
        assert!(guide.rename(2, "Save changes".to_string()));
        let path = temp_path("ok");

        let result = export_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
            &path,
        )
        .expect("export storyboard");

        assert_eq!(result.path, path);
        assert_eq!(result.width, 320);
        assert_eq!(result.step_count, 2);
        assert!(path.exists(), "PNG should be written");
        assert!(
            !path.with_extension("png.tmp").exists(),
            "temporary file should be removed"
        );

        let decoded = ImageReader::open(&path)
            .unwrap()
            .decode()
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.width(), 320);
        assert_eq!(decoded.height(), result.height);
        assert!(
            decoded.height() > 80,
            "storyboard should include labels, padding, and both images"
        );
        let non_white = decoded
            .pixels()
            .filter(|pixel| pixel.0 != [255, 255, 255, 255])
            .count();
        assert!(
            non_white > 100,
            "expected labels/cards/images, got {non_white}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_guide_is_rejected_and_writes_nothing() {
        let recording = recording();
        let guide = Guide::from_candidates(Vec::new());
        let path = temp_path("empty");

        let result = export_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions::default(),
            &path,
        );

        assert!(matches!(result, Err(StoryboardError::Empty)));
        assert!(!path.exists());
    }

    #[test]
    fn missing_keyframe_is_rejected_and_writes_nothing() {
        let store = FrameStore::new(StoreConfig::default());
        let guide = guide_with_steps(999, 1);
        let path = temp_path("missing");

        let result = export_storyboard(&guide, &store, StoryboardOptions::default(), &path);

        assert!(matches!(
            result,
            Err(StoryboardError::KeyframeMissing { index: 1 })
        ));
        assert!(!path.exists());
    }

    #[test]
    fn long_titles_are_elided_to_fit_card_width() {
        let label = fit_label(
            "Step 1 - This title is intentionally far longer than a narrow storyboard card",
            120.0,
        );

        assert!(label.ends_with("..."), "label should be elided: {label}");
        let (width, _) = measure_block(&label, LABEL_FONT_PX, true);
        assert!(width <= 120.0, "label width {width} exceeded limit");
    }

    #[test]
    fn canvas_pixel_limit_rejects_too_large_output() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let guide = guide_with_steps(keyframe, 2);
        let path = temp_path("too-large");

        let result = export_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 10,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
            &path,
        );

        assert!(matches!(result, Err(StoryboardError::CanvasTooLarge)));
        assert!(!path.exists());
    }

    #[test]
    fn renders_storyboard_in_memory_without_writing_a_file() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 2);
        assert!(guide.rename(1, "Open settings".to_string()));
        assert!(guide.rename(2, "Save changes".to_string()));

        let result = render_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render storyboard");

        assert_eq!(result.width, 320);
        assert_eq!(result.image.width(), result.width);
        assert_eq!(result.image.height(), result.height);
        assert_eq!(result.step_count, 2);
        assert!(
            result
                .image
                .pixels()
                .any(|pixel| pixel.0 != [255, 255, 255, 255]),
            "render should contain labels/cards/images"
        );
    }

    #[test]
    fn render_empty_guide_is_rejected() {
        let recording = recording();
        let guide = Guide::from_candidates(Vec::new());

        let result = render_storyboard(&guide, &recording.store, StoryboardOptions::default());

        assert!(matches!(result, Err(StoryboardError::Empty)));
    }

    #[test]
    fn render_missing_keyframe_is_rejected() {
        let store = FrameStore::new(StoreConfig::default());
        let guide = guide_with_steps(999, 1);

        let result = render_storyboard(&guide, &store, StoryboardOptions::default());

        assert!(matches!(
            result,
            Err(StoryboardError::KeyframeMissing { index: 1 })
        ));
    }

    #[test]
    fn captions_increase_storyboard_card_height() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 1);

        let without_caption = render_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render without caption");

        assert!(guide.set_caption(
            1,
            "The Save button closes the dialog without persisting the change.".to_string()
        ));
        let with_caption = render_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render with caption");

        assert!(
            with_caption.height > without_caption.height,
            "caption should add text height"
        );
        assert_eq!(with_caption.step_count, 1);
    }

    #[test]
    fn long_captions_are_elided_to_fit_card_width() {
        let caption = fit_caption(
            "The settings dialog closes but the saved preference is not present after reopening the same panel",
            180.0,
        );

        assert!(caption.ends_with("..."), "caption = {caption}");
        assert!(
            measure_block(&caption, CAPTION_FONT_PX, false).0 <= 180.0,
            "caption should fit measured width: {caption}"
        );
    }

    #[test]
    fn renders_storyboard_from_explicit_steps() {
        let image = quadrant();
        let steps = vec![StoryboardStep {
            index: 1,
            title: "Click Save",
            caption: Some("The dialog closes but the value is not persisted."),
            image: &image,
        }];

        let result = render_storyboard_steps(
            &steps,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render explicit steps");

        assert_eq!(result.width, 320);
        assert_eq!(result.step_count, 1);
        assert_eq!(result.image.width(), result.width);
        assert_eq!(result.image.height(), result.height);
        assert!(
            result
                .image
                .pixels()
                .any(|pixel| pixel.0 != [255, 255, 255, 255]),
            "render should contain card, text, and image pixels"
        );
    }

    #[test]
    fn explicit_step_render_rejects_empty_steps() {
        let result = render_storyboard_steps(&[], StoryboardOptions::default());

        assert!(matches!(result, Err(StoryboardError::Empty)));
    }

    #[test]
    fn whitespace_only_captions_are_omitted_from_storyboard_layout() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 1);
        let opts = StoryboardOptions {
            max_width: 320,
            max_canvas_pixels: 1_000_000,
            outer_padding: 12,
            card_spacing: 10,
            card_padding: 8,
            show_titles: true,
        };

        let without_caption = render_storyboard(&guide, &recording.store, opts.clone())
            .expect("render without caption");
        assert!(guide.set_caption(1, "    ".to_string()));
        let whitespace_caption = render_storyboard(&guide, &recording.store, opts)
            .expect("render with whitespace caption");

        assert_eq!(whitespace_caption.height, without_caption.height);
    }

    #[test]
    fn cancellable_stops_on_cancel_and_returns_error() {
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
        };

        let result =
            render_reviewed_storyboard_cancellable(&job, StoryboardOptions::default(), &cancel);
        assert!(matches!(result, Err(StoryboardError::Cancelled)));
    }

    #[test]
    fn cancellable_observes_at_most_one_live_image() {
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
        };

        let cancel = PublishCancellation::new();
        let result = render_reviewed_storyboard_cancellable(
            &job,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 10_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
            &cancel,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap().step_count, 5);
    }

    #[test]
    fn cancellable_export_writes_file_on_success() {
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
            steps: vec![crate::export::model::ReviewedGuideStep {
                index: 1,
                title: "Step 1".into(),
                caption: None,
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::VisualChange,
                at_ms: 100,
                image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                    image,
                )),
                hotspots: Vec::new(),
            }],
        };

        let path = temp_path("cancellable-ok");
        let result = export_reviewed_storyboard_cancellable(
            &job,
            StoryboardOptions::default(),
            &cancel,
            &path,
        );
        assert!(result.is_ok());
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cancellable_export_leaves_no_file_on_cancel() {
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
            steps: vec![crate::export::model::ReviewedGuideStep {
                index: 1,
                title: "Step 1".into(),
                caption: None,
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::VisualChange,
                at_ms: 100,
                image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                    image,
                )),
                hotspots: Vec::new(),
            }],
        };

        let path = temp_path("cancellable-cancel");
        let result = export_reviewed_storyboard_cancellable(
            &job,
            StoryboardOptions::default(),
            &cancel,
            &path,
        );
        assert!(matches!(result, Err(StoryboardError::Cancelled)));
        assert!(!path.exists());
    }
}
