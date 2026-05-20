use image::{Rgba, RgbaImage};

use crate::types::{OffsetEstimate, StitchConfig};

const TOP_IGNORE_RATIO: f32 = 0.12;
const BOTTOM_IGNORE_RATIO: f32 = 0.08;
const SIDE_IGNORE_RATIO: f32 = 0.04;
const MIN_IGNORE_PX: u32 = 24;
const TEMPLATE_MIN_HEIGHT: u32 = 48;
const SECOND_BEST_MIN_MARGIN: f32 = 0.015;
const VERIFY_MAX_NORMALIZED_DIFF: f32 = 18.0 / 255.0;

#[derive(Clone, Copy)]
struct Region {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Estimates the vertical offset that takes `prev` onto `curr`.
///
/// Confidence follows the rollshot convention: lower is better, and an
/// estimate is acceptable when `confidence <= StitchConfig::accept_diff`.
///
/// Returns an estimate with `confidence = f32::INFINITY` when the frames
/// are too small to match, the content ROI is empty, the best score is
/// indistinguishable from the second-best, or the verification step
/// reports too much pixel disagreement.
pub fn estimate_offset(
    prev: &RgbaImage,
    curr: &RgbaImage,
    last_offset: i32,
    config: &StitchConfig,
) -> OffsetEstimate {
    let no_match = OffsetEstimate {
        dy: 0,
        confidence: f32::INFINITY,
        method: config.algorithm,
    };

    if prev.dimensions() != curr.dimensions() {
        return no_match;
    }

    let width = prev.width();
    let height = prev.height();
    if height < 100 || width < 50 {
        return no_match;
    }

    let roi = content_roi(width, height);
    let match_region = match_width_region(roi, config.match_width);
    if roi.h < TEMPLATE_MIN_HEIGHT * 2 || match_region.w < 40 {
        return no_match;
    }

    let template_h = (roi.h / 3).max(TEMPLATE_MIN_HEIGHT).min(roi.h - 1);
    let search_start = roi.y as i32;
    let search_end = (roi.y + roi.h - template_h) as i32;
    if search_end <= search_start {
        return no_match;
    }

    let prev_gray = to_grayscale(prev);
    let curr_gray = to_grayscale(curr);

    let max_offset = (height as i32 - config.min_overlap as i32)
        .max(0)
        .min(search_end - search_start);
    let predict = last_offset.clamp(0, max_offset);

    let mut best_offset = 0i32;
    let mut best_score = f32::MIN;
    let mut second_score = f32::MIN;

    for offset in predict_iter(max_offset, predict) {
        let search_y = search_start + offset;

        let curr_template = Region {
            y: roi.y,
            h: template_h,
            ..match_region
        };
        let prev_template = Region {
            y: search_y as u32,
            ..curr_template
        };
        let score = ncc_score_region(&prev_gray, &curr_gray, width, prev_template, curr_template);

        if score > best_score {
            second_score = best_score;
            best_score = score;
            best_offset = offset;
        } else if score > second_score {
            second_score = score;
        }
    }

    if !best_score.is_finite() || best_score <= 0.0 {
        return no_match;
    }

    if second_score.is_finite() && best_score - second_score < SECOND_BEST_MIN_MARGIN {
        return no_match;
    }

    let overlap_h = height.saturating_sub(best_offset as u32);
    let overlap_region = Region {
        y: 0,
        h: overlap_h,
        ..match_region
    };
    let verify = overlap_mean_abs_diff(
        &prev_gray,
        &curr_gray,
        width,
        overlap_region,
        best_offset as u32,
    );

    if !verify.is_finite() || verify > VERIFY_MAX_NORMALIZED_DIFF {
        return no_match;
    }

    let confidence = (1.0 - best_score.clamp(0.0, 1.0)) + verify * 0.5;
    OffsetEstimate {
        dy: best_offset,
        confidence,
        method: config.algorithm,
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
    use super::{content_roi, estimate_offset};
    use crate::types::{MatchAlgorithm, StitchConfig};
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

    #[test]
    fn content_roi_skips_borders() {
        let roi = content_roi(320, 320);

        assert!(roi.x >= 24);
        assert!(roi.y >= 24);
        assert!(roi.w < 320);
        assert!(roi.h < 320);
    }

    #[test]
    fn estimate_offset_respects_min_overlap() {
        let canvas = make_textured_canvas(320, 800);
        let prev = crop(&canvas, 0, 320);
        let curr = crop(&canvas, 120, 320);
        let config = StitchConfig {
            min_overlap: 280,
            ..StitchConfig::default()
        };

        let estimate = estimate_offset(&prev, &curr, 0, &config);

        assert!(
            estimate.dy <= 40,
            "dy = {} exceeds configured max offset",
            estimate.dy
        );
    }

    #[test]
    fn estimate_offset_finds_known_scroll() {
        let canvas = make_textured_canvas(160, 600);
        let prev = crop(&canvas, 0, 160);
        let curr = crop(&canvas, 40, 160);

        let estimate = estimate_offset(&prev, &curr, 0, &StitchConfig::default());

        assert_eq!(estimate.method, MatchAlgorithm::Template);
        assert!(
            (estimate.dy - 40).abs() <= 2,
            "dy = {} (expected ~40)",
            estimate.dy
        );
        assert!(
            estimate.confidence < StitchConfig::default().accept_diff,
            "confidence = {} (expected < {})",
            estimate.confidence,
            StitchConfig::default().accept_diff
        );
    }

    #[test]
    fn estimate_offset_rejects_unrelated_frames() {
        let prev = make_textured_canvas(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));

        let estimate = estimate_offset(&prev, &curr, 0, &StitchConfig::default());

        assert!(
            estimate.confidence > StitchConfig::default().accept_diff,
            "confidence = {} (expected > accept_diff)",
            estimate.confidence
        );
    }

    #[test]
    fn estimate_offset_rejects_dimension_mismatch() {
        let prev = make_textured_canvas(160, 160);
        let curr = make_textured_canvas(160, 200);

        let estimate = estimate_offset(&prev, &curr, 0, &StitchConfig::default());

        assert!(!estimate.confidence.is_finite());
    }
}
