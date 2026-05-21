use image::{Rgba, RgbaImage};

use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::overlap::compute_overlap;
use crate::types::{MatchMethod, MotionCandidate, ScrollAxis, StitchConfig};
use crate::verifier::{PixelOverlapVerifier, VerifierOutcome};

const TOP_IGNORE_RATIO: f32 = 0.12;
const BOTTOM_IGNORE_RATIO: f32 = 0.08;
const SIDE_IGNORE_RATIO: f32 = 0.04;
const MIN_IGNORE_PX: u32 = 24;
const TEMPLATE_MIN_HEIGHT: u32 = 48;
const COARSE_DOWNSAMPLE_STEP: u32 = 4;
const EDGE_PROJECTION_STEP: u32 = 2;

#[derive(Clone, Copy)]
struct Region {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

#[derive(Debug, Clone, Copy)]
struct CandidateScore {
    candidate: MotionCandidate,
    verifier_score: f32,
}

#[derive(Debug, Clone, Copy)]
enum SearchAxis {
    Vertical,
    Horizontal,
}

pub fn estimate_motion(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    if prev.dimensions() != curr.dimensions() {
        return None;
    }

    let width = prev.width();
    let height = prev.height();
    let prev_gray = to_grayscale(prev);
    let curr_gray = to_grayscale(curr);

    let mut candidates = Vec::new();
    candidates.extend(coarse_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        config,
    ));
    candidates.extend(template_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        last_motion,
        config,
    ));
    candidates.extend(edge_projection_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        config,
    ));

    rank_verified_candidates(prev, curr, locked_axis, candidates, config)
}

fn rank_verified_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    candidates: Vec<MotionCandidate>,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    let verifier = PixelOverlapVerifier::new(&config.verifier, config.min_overlap);
    let mut scored = Vec::new();

    for mut candidate in candidates {
        if candidate.score > config.accept_confidence {
            continue;
        }
        if !passes_second_best_margin(&candidate, config.second_best_margin) {
            continue;
        }
        if !candidate_matches_axis(candidate.dx, candidate.dy, locked_axis, config) {
            continue;
        }

        let verifier_score = match verifier.verify(prev, curr, &candidate) {
            VerifierOutcome::Pass { score, .. } => score,
            VerifierOutcome::InsufficientOverlap
            | VerifierOutcome::OverlapDisagreement { .. } => continue,
        };

        candidate.score = (candidate.score + verifier_score * 0.5).clamp(0.0, 1.0);
        scored.push(CandidateScore {
            candidate,
            verifier_score,
        });
    }

    scored.sort_by(|a, b| {
        a.candidate
            .score
            .total_cmp(&b.candidate.score)
            .then(a.verifier_score.total_cmp(&b.verifier_score))
    });

    scored.first().map(|s| s.candidate)
}

fn passes_second_best_margin(candidate: &MotionCandidate, margin: f32) -> bool {
    match candidate.second_best_score {
        Some(second) => second - candidate.score >= margin,
        None => true,
    }
}

fn candidate_matches_axis(
    dx: i32,
    dy: i32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> bool {
    match locked_axis {
        None => !matches!(
            classify_axis(dx, dy, config.axis_ratio_threshold),
            AxisClassification::Ambiguous
        ),
        Some(axis) => matches!(
            validate_with_lock(axis, dx, dy, config.max_cross_axis_px),
            AxisValidation::OnAxis { .. }
        ),
    }
}

fn candidate(
    dx: i32,
    dy: i32,
    method: MatchMethod,
    score: f32,
    second_best_score: Option<f32>,
) -> MotionCandidate {
    MotionCandidate {
        dx,
        dy,
        method,
        score,
        second_best_score,
        inliers: None,
        raw_matches: None,
    }
}

fn content_roi(width: u32, height: u32) -> Region {
    let side = ((width as f32 * SIDE_IGNORE_RATIO) as u32).max(MIN_IGNORE_PX);
    let top = ((height as f32 * TOP_IGNORE_RATIO) as u32).max(MIN_IGNORE_PX);
    let bottom = ((height as f32 * BOTTOM_IGNORE_RATIO) as u32).max(MIN_IGNORE_PX);
    let x = side.min(width.saturating_sub(1));
    let y = top.min(height.saturating_sub(1));
    let w = width.saturating_sub(x.saturating_mul(2)).max(1);
    let h = height.saturating_sub(y).saturating_sub(bottom).max(1);
    Region { x, y, w, h }
}

fn match_width_region(region: Region, match_width: u32) -> Region {
    if match_width == 0 || match_width >= region.w {
        return region;
    }

    let w = match_width.max(1);
    let x = region.x + (region.w - w) / 2;
    Region { x, w, ..region }
}

fn to_grayscale(img: &RgbaImage) -> Vec<f32> {
    img.pixels()
        .map(|Rgba([r, g, b, _])| 0.299 * *r as f32 + 0.587 * *g as f32 + 0.114 * *b as f32)
        .collect()
}

fn predict_iter(max: i32, predict: i32) -> Vec<i32> {
    let p = predict.clamp(0, max);
    let mut out = Vec::with_capacity((max as usize).saturating_mul(2) + 1);
    out.push(p);
    for delta in 1..=max {
        if p + delta <= max {
            out.push(p + delta);
        }
        if p - delta >= 0 {
            out.push(p - delta);
        }
    }
    out
}

fn ncc_score_region(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    prev_region: Region,
    curr_region: Region,
) -> f32 {
    if prev_region.w == 0 || prev_region.h == 0 || width == 0 {
        return f32::MIN;
    }

    let mut prev_sum = 0.0f32;
    let mut curr_sum = 0.0f32;
    let mut count = 0usize;

    for row in 0..prev_region.h {
        let prev_base = ((prev_region.y + row) * width + prev_region.x) as usize;
        let curr_base = ((curr_region.y + row) * width + curr_region.x) as usize;
        for col in 0..prev_region.w as usize {
            prev_sum += prev_gray[prev_base + col];
            curr_sum += curr_gray[curr_base + col];
            count += 1;
        }
    }

    if count == 0 {
        return f32::MIN;
    }

    let prev_mean = prev_sum / count as f32;
    let curr_mean = curr_sum / count as f32;
    let mut num = 0.0f32;
    let mut prev_var = 0.0f32;
    let mut curr_var = 0.0f32;

    for row in 0..prev_region.h {
        let prev_base = ((prev_region.y + row) * width + prev_region.x) as usize;
        let curr_base = ((curr_region.y + row) * width + curr_region.x) as usize;
        for col in 0..prev_region.w as usize {
            let p = prev_gray[prev_base + col] - prev_mean;
            let c = curr_gray[curr_base + col] - curr_mean;
            num += p * c;
            prev_var += p * p;
            curr_var += c * c;
        }
    }

    if prev_var <= 1.0 || curr_var <= 1.0 {
        return f32::MIN;
    }

    num / (prev_var.sqrt() * curr_var.sqrt())
}

fn overlap_mean_abs_diff(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    region: Region,
    offset: u32,
) -> f32 {
    if region.w == 0 || region.h == 0 {
        return f32::INFINITY;
    }

    let sample_h = region.h.min(160);
    let prev_start_y = offset + region.h.saturating_sub(sample_h);
    let curr_start_y = region.h.saturating_sub(sample_h);

    let mut sum = 0.0f32;
    let mut count = 0usize;
    for row in 0..sample_h {
        let prev_base = ((prev_start_y + row) * width + region.x) as usize;
        let curr_base = ((curr_start_y + row) * width + region.x) as usize;
        for col in 0..region.w as usize {
            sum += (prev_gray[prev_base + col] - curr_gray[curr_base + col]).abs();
            count += 1;
        }
    }

    if count == 0 {
        return f32::INFINITY;
    }

    sum / (count as f32 * 255.0)
}

#[cfg(test)]
mod tests {
    use super::{content_roi, estimate_motion};
    use crate::types::{MatchMethod, ScrollAxis, StitchConfig};
    use image::{imageops, Rgba, RgbaImage};

    fn make_textured_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
        for y in (0..height).step_by(11) {
            let accent = ((y / 3) % 180) as u8;
            for x in 8..width.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
                img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
                if y + 1 < height {
                    img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }
        for col in [21, 47, 73, 99, 125] {
            if col >= width {
                continue;
            }
            for y in 12..height.saturating_sub(12) {
                if (y / 13) % 3 != 0 {
                    img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
                }
            }
        }
        img
    }

    fn crop(canvas: &RgbaImage, y: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, 0, y, canvas.width(), h).to_image()
    }

    fn make_wide_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
        for x in (0..width).step_by(11) {
            let accent = ((x / 3) % 180) as u8;
            for y in 8..height.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
                img.put_pixel(x, y, Rgba([stripe, accent, 80, 255]));
                if x + 1 < width {
                    img.put_pixel(x + 1, y, Rgba([30, 30, 30, 255]));
                }
            }
        }
        for row in [21u32, 47, 73, 99, 125] {
            if row >= height {
                continue;
            }
            for x in 12..width.saturating_sub(12) {
                if (x / 13) % 3 != 0 {
                    img.put_pixel(x, row, Rgba([20, 20, 20, 255]));
                }
            }
        }
        img
    }

    fn make_repeated_grid(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
        for y in 0..height {
            for x in 0..width {
                let v = if (x / 16 + y / 16) % 2 == 0 { 48 } else { 208 };
                img.put_pixel(x, y, Rgba([v, v, v, 255]));
            }
        }
        img
    }

    fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, x, y, w, h).to_image()
    }

    #[test]
    fn content_roi_skips_borders() {
        let roi = content_roi(320, 320);
        assert!(roi.x >= 24);
        assert!(roi.y >= 24);
        assert!(roi.w < 320);
        assert!(roi.h < 320);
    }

    #[test]
    fn estimate_motion_respects_min_overlap() {
        let canvas = make_textured_canvas(320, 800);
        let prev = crop(&canvas, 0, 320);
        let curr = crop(&canvas, 120, 320);
        let config = StitchConfig {
            min_overlap: 280,
            ..StitchConfig::default()
        };
        let candidate = estimate_motion(&prev, &curr, None, (0, 0), &config).expect("template candidate");
        assert!(
            candidate.dy <= 40,
            "dy = {} exceeds bounded search",
            candidate.dy
        );
    }

    #[test]
    fn estimate_motion_finds_known_scroll() {
        let canvas = make_textured_canvas(160, 600);
        let prev = crop(&canvas, 0, 160);
        let curr = crop(&canvas, 40, 160);
        let candidate =
            estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default()).expect("template candidate");
        assert_eq!(candidate.method, MatchMethod::Template);
        assert_eq!(candidate.dx, 0);
        assert!(
            (candidate.dy - 40).abs() <= 2,
            "dy = {} (expected ~40)",
            candidate.dy
        );
    }

    #[test]
    fn estimate_motion_returns_none_for_unrelated_frames() {
        let prev = make_textured_canvas(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));
        assert!(estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default()).is_none());
    }

    #[test]
    fn estimate_motion_returns_none_for_dimension_mismatch() {
        let prev = make_textured_canvas(160, 160);
        let curr = make_textured_canvas(160, 200);
        assert!(estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default()).is_none());
    }

    #[test]
    fn estimate_motion_finds_vertical_up_scroll() {
        let canvas = make_textured_canvas(160, 700);
        let prev = crop(&canvas, 220, 160);
        let curr = crop(&canvas, 180, 160);

        let candidate = estimate_motion(
            &prev,
            &curr,
            None,
            (0, 0),
            &StitchConfig::default(),
        )
        .expect("template candidate");

        assert_eq!(candidate.method, MatchMethod::Template);
        assert_eq!(candidate.dx, 0);
        assert!(
            (candidate.dy + 40).abs() <= 2,
            "dy = {} (expected ~-40)",
            candidate.dy
        );
    }

    #[test]
    fn estimate_motion_finds_horizontal_right_scroll() {
        let canvas = make_wide_canvas(700, 160);
        let prev = crop_xy(&canvas, 0, 0, 160, 160);
        let curr = crop_xy(&canvas, 40, 0, 160, 160);

        let candidate = estimate_motion(
            &prev,
            &curr,
            None,
            (0, 0),
            &StitchConfig::default(),
        )
        .expect("horizontal candidate");

        assert_eq!(candidate.dy, 0);
        assert!(
            (candidate.dx - 40).abs() <= 2,
            "dx = {} (expected ~40)",
            candidate.dx
        );
    }

    #[test]
    fn estimate_motion_finds_horizontal_left_scroll() {
        let canvas = make_wide_canvas(700, 160);
        let prev = crop_xy(&canvas, 220, 0, 160, 160);
        let curr = crop_xy(&canvas, 180, 0, 160, 160);

        let candidate = estimate_motion(
            &prev,
            &curr,
            Some(ScrollAxis::Horizontal),
            (40, 0),
            &StitchConfig::default(),
        )
        .expect("horizontal candidate");

        assert_eq!(candidate.dy, 0);
        assert!(
            (candidate.dx + 40).abs() <= 2,
            "dx = {} (expected ~-40)",
            candidate.dx
        );
    }

    #[test]
    fn locked_vertical_hint_rejects_horizontal_candidate() {
        let canvas = make_wide_canvas(700, 160);
        let prev = crop_xy(&canvas, 0, 0, 160, 160);
        let curr = crop_xy(&canvas, 40, 0, 160, 160);

        let candidate = estimate_motion(
            &prev,
            &curr,
            Some(ScrollAxis::Vertical),
            (0, 40),
            &StitchConfig::default(),
        );

        assert!(candidate.is_none());
    }

    #[test]
    fn repeated_grid_is_rejected_by_second_best_margin() {
        let canvas = make_repeated_grid(240, 560);
        let prev = crop_xy(&canvas, 0, 0, 160, 160);
        let curr = crop_xy(&canvas, 0, 32, 160, 160);

        let candidate = estimate_motion(
            &prev,
            &curr,
            None,
            (0, 0),
            &StitchConfig::default(),
        );

        assert!(candidate.is_none());
    }
}
