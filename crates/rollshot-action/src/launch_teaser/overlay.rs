//! Text overlay rasterization for launch teaser rendering.
//!
//! All text is rasterized into Rollshot-owned PNG overlays before FFmpeg
//! invocation. User/model strings never enter FFmpeg arguments, filter
//! names, expressions, codecs, or paths.

use std::path::Path;

use image::{Rgba, RgbaImage};
use rollshot_image_document::draw_text_block;
use rollshot_image_document::ImagePoint;
use rollshot_image_document::Rgba8;

use super::error::LaunchTeaserRenderError;
use super::graph::RenderProfile;
use super::plan::{ValidatedLaunchTeaserPlan, OUTRO_DURATION_MS};

/// A rasterized overlay asset ready for FFmpeg consumption.
#[derive(Debug, Clone)]
pub struct OverlayAsset {
    /// Index of the shot this overlay belongs to.
    pub shot_index: usize,
    /// Absolute path to the generated PNG file.
    pub path: std::path::PathBuf,
    /// Start time in milliseconds when this overlay appears.
    pub start_ms: u64,
    /// End time in milliseconds when this overlay disappears.
    pub end_ms: u64,
}

/// Fixed font size for overlays.
const FONT_SIZE: f32 = 48.0;

/// Fixed margin from edges.
const MARGIN: u32 = 48;

/// Background plate color (semi-transparent black).
const PLATE_BG: Rgba<u8> = Rgba([0, 0, 0, 180]);

/// Text color (white).
const TEXT_COLOR: Rgba8 = Rgba8::new(255, 255, 255, 255);

/// Rasterize text overlays for all shots in the plan.
///
/// Creates transparent RGBA images at the output profile dimensions,
/// renders black plates with white text, and saves as generated PNG filenames.
/// Hook appears on the first shot, each caption on its shot, and outro on
/// the final 1,500 ms.
pub fn prepare_overlay_assets(
    plan: &ValidatedLaunchTeaserPlan,
    scratch: &Path,
    profile: RenderProfile,
) -> Result<Vec<OverlayAsset>, LaunchTeaserRenderError> {
    let (width, height) = match profile {
        RenderProfile::Final => (super::plan::FINAL_WIDTH, super::plan::FINAL_HEIGHT),
        RenderProfile::Preview => (super::plan::PREVIEW_WIDTH, super::plan::PREVIEW_HEIGHT),
    };

    let mut assets = Vec::new();
    let plan_inner = plan.plan();
    let total_duration = plan.duration_ms();

    for (i, shot) in plan_inner.shots.iter().enumerate() {
        // Caption overlay for every shot.
        if !shot.caption.is_empty() {
            let start_ms = plan_inner.cumulative_start_ms(i);
            let end_ms = start_ms + shot.displayed_ms();
            let path = scratch.join(format!("overlay-{i:03}-caption.png"));
            rasterize_overlay(width, height, &shot.caption, &path)
                .map_err(|_| LaunchTeaserRenderError::OverlayFailed)?;
            assets.push(OverlayAsset {
                shot_index: i,
                path,
                start_ms,
                end_ms,
            });
        }

        // Hook overlay on the first shot.
        if i == 0 && !plan_inner.hook.is_empty() {
            let path = scratch.join("overlay-000-hook.png");
            rasterize_overlay(width, height, &plan_inner.hook, &path)
                .map_err(|_| LaunchTeaserRenderError::OverlayFailed)?;
            assets.push(OverlayAsset {
                shot_index: 0,
                path,
                start_ms: 0,
                end_ms: plan_inner.shots[0].displayed_ms(),
            });
        }

        // Outro overlay on the last shot.
        if i == plan_inner.shots.len() - 1 && !plan_inner.outro_text.is_empty() {
            let outro_start = total_duration.saturating_sub(OUTRO_DURATION_MS);
            let path = scratch.join("overlay-outro.png");
            rasterize_overlay(width, height, &plan_inner.outro_text, &path)
                .map_err(|_| LaunchTeaserRenderError::OverlayFailed)?;
            assets.push(OverlayAsset {
                shot_index: i,
                path,
                start_ms: outro_start,
                end_ms: total_duration,
            });
        }
    }

    Ok(assets)
}

/// Rasterize text onto a transparent image and save as PNG.
fn rasterize_overlay(
    width: u32,
    height: u32,
    text: &str,
    path: &Path,
) -> Result<(), image::ImageError> {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));

    // Draw a semi-transparent background plate at the bottom.
    let plate_height = (FONT_SIZE as u32) + MARGIN * 2;
    let plate_y = height.saturating_sub(plate_height);
    for y in plate_y..height {
        for x in 0..width {
            img.put_pixel(x, y, PLATE_BG);
        }
    }

    // Draw text.
    let text_x = MARGIN;
    let text_y = plate_y + MARGIN;
    draw_text_block(
        &mut img,
        ImagePoint::new(text_x as f32, text_y as f32),
        text,
        FONT_SIZE,
        false,
        TEXT_COLOR,
    );

    img.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch_teaser::plan::*;
    use crate::project::ProjectStepId;

    fn valid_plan() -> LaunchTeaserPlanV1 {
        LaunchTeaserPlanV1 {
            schema_version: LAUNCH_TEASER_SCHEMA_VERSION,
            source: LaunchTeaserSourceV1 {
                project_revision: 1,
                projection_digest: "a".repeat(64),
                motion_sha256: "b".repeat(64),
                motion_duration_ms: 30_000,
                motion_width: 1920,
                motion_height: 1080,
            },
            hook: "Test Hook".into(),
            shots: vec![
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(1),
                    source_start_ms: 0,
                    source_end_ms: 5_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "First step".into(),
                    transition: TransitionV1::Cut,
                },
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(2),
                    source_start_ms: 5_000,
                    source_end_ms: 10_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "Second step".into(),
                    transition: TransitionV1::Cut,
                },
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(3),
                    source_start_ms: 10_000,
                    source_end_ms: 15_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "Third step".into(),
                    transition: TransitionV1::Cut,
                },
            ],
            outro_text: "Made with Rollshot".into(),
            provenance: LaunchTeaserProvenanceV1 {
                deterministic_seed_version: 1,
                agent: None,
                repository_reads: Vec::new(),
                accepted_user_edits: Vec::new(),
            },
        }
    }

    #[test]
    fn overlay_assets_created_for_all_shots() {
        let plan = valid_plan().validate().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let assets = prepare_overlay_assets(&plan, scratch.path(), RenderProfile::Final).unwrap();
        // 3 captions + 1 hook + 1 outro = 5 overlays
        assert_eq!(assets.len(), 5);
    }

    #[test]
    fn overlay_files_are_unique() {
        let plan = valid_plan().validate().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let assets = prepare_overlay_assets(&plan, scratch.path(), RenderProfile::Final).unwrap();
        let paths: Vec<_> = assets.iter().map(|a| &a.path).collect();
        let unique_len = paths.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(unique_len, paths.len());
    }

    #[test]
    fn overlay_files_exist_on_disk() {
        let plan = valid_plan().validate().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let assets = prepare_overlay_assets(&plan, scratch.path(), RenderProfile::Final).unwrap();
        for asset in &assets {
            assert!(asset.path.is_file(), "missing overlay: {:?}", asset.path);
        }
    }

    #[test]
    fn preview_overlays_use_smaller_dimensions() {
        let plan = valid_plan().validate().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let assets = prepare_overlay_assets(&plan, scratch.path(), RenderProfile::Preview).unwrap();
        // Verify at least one file is created and readable.
        let img = image::open(&assets[0].path).unwrap();
        assert_eq!(img.width(), PREVIEW_WIDTH);
        assert_eq!(img.height(), PREVIEW_HEIGHT);
    }

    #[test]
    fn final_overlays_use_full_dimensions() {
        let plan = valid_plan().validate().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let assets = prepare_overlay_assets(&plan, scratch.path(), RenderProfile::Final).unwrap();
        let img = image::open(&assets[0].path).unwrap();
        assert_eq!(img.width(), FINAL_WIDTH);
        assert_eq!(img.height(), FINAL_HEIGHT);
    }

    #[test]
    fn outro_overlay_appears_at_correct_time() {
        let plan = valid_plan().validate().unwrap();
        let total = plan.duration_ms();
        let scratch = tempfile::tempdir().unwrap();
        let assets = prepare_overlay_assets(&plan, scratch.path(), RenderProfile::Final).unwrap();
        let outro = assets
            .iter()
            .find(|a| a.path.to_string_lossy().contains("outro"))
            .unwrap();
        assert_eq!(outro.start_ms, total - OUTRO_DURATION_MS);
        assert_eq!(outro.end_ms, total);
    }

    #[test]
    fn hostile_text_never_enters_path() {
        let mut plan = valid_plan();
        plan.shots[0].caption = "x'];movie=/etc/passwd['y".into();
        let validated = plan.validate().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let assets =
            prepare_overlay_assets(&validated, scratch.path(), RenderProfile::Final).unwrap();
        for asset in &assets {
            assert!(!asset.path.to_string_lossy().contains("passwd"));
            assert!(!asset.path.to_string_lossy().contains("/etc"));
        }
    }

    #[test]
    fn cumulative_start_ms_correct() {
        let plan = valid_plan();
        assert_eq!(plan.cumulative_start_ms(0), 0);
        assert_eq!(plan.cumulative_start_ms(1), 5_000);
        assert_eq!(plan.cumulative_start_ms(2), 10_000);
    }

    #[test]
    fn shot_displayed_ms_uses_speed() {
        let mut plan = valid_plan();
        plan.shots[0].speed = SpeedV1::P2000;
        // source_dur = 5_000, displayed = 5_000 * 1000 / 2000 = 2_500
        assert_eq!(plan.shots[0].displayed_ms(), 2_500);
    }
}
