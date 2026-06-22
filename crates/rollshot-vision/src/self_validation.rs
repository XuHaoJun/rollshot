//! Author-time template self-validation. Pure and deterministic. The caller
//! (SP3 author pipeline) supplies a candidate region; this module crops it from
//! the source image, matches it back, and measures whether it is a reliable
//! template. Confidence is measured here, NOT taken from any LLM.

use rollshot_image_document::ImageRect;

use crate::index::VisualIndex;
use crate::rect::iou;
use crate::template::{match_template_image, MAX_TEMPLATE_AREA};
use crate::VisionError;
use rollshot_automation::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedCount {
    Unique,
    Repeating,
    AtLeast(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateDecision {
    Pass,
    NeedsConfirm,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfValidationConfig {
    pub expected_count: ExpectedCount,
    pub target_bounds: Option<ImageRect>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemplateSelfValidation {
    pub self_score: f32,
    pub second_best_score: Option<f32>,
    pub peak_margin: f32,
    pub false_positive_count: u32,
    pub edge_density: f32,
    pub entropy: f32,
    pub stable_under_jitter: bool,
    pub decision: TemplateDecision,
}

// Tunable floors (SP1 constants; config-ize later).
const EDGE_DENSITY_FLOOR: f32 = 0.05;
const ENTROPY_FLOOR: f32 = 1.5;
const FALSE_POSITIVE_SCORE: f32 = 0.7;
const CLEAN_PEAK_MARGIN: f32 = 0.15;
const SELF_SCORE_FLOOR: f32 = 0.9;
const JITTER_SCORE_DROP: f32 = 0.2;
const MIN_SELF_VALIDATION_AREA: u64 = 16;

pub fn self_validate(
    index: &VisualIndex,
    candidate_bounds: ImageRect,
    cfg: &SelfValidationConfig,
) -> Result<TemplateSelfValidation, VisionError> {
    let (iw, ih) = (index.width(), index.height());
    // Candidate must lie fully inside the image.
    if !candidate_bounds.is_finite()
        || candidate_bounds.x < 0.0
        || candidate_bounds.y < 0.0
        || candidate_bounds.width <= 0.0
        || candidate_bounds.height <= 0.0
        || candidate_bounds.x + candidate_bounds.width > iw as f32
        || candidate_bounds.y + candidate_bounds.height > ih as f32
    {
        return Err(VisionError::CandidateOutOfBounds);
    }

    let cx = candidate_bounds.x.floor() as u32;
    let cy = candidate_bounds.y.floor() as u32;
    let cw = candidate_bounds.width.round().max(1.0) as u32;
    let ch = candidate_bounds.height.round().max(1.0) as u32;
    let candidate_area = u64::from(cw) * u64::from(ch);
    let area_ok =
        (MIN_SELF_VALIDATION_AREA..=MAX_TEMPLATE_AREA).contains(&candidate_area);

    let candidate_rgba =
        image::imageops::crop_imm(index.image(), cx, cy, cw, ch).to_image();
    let candidate_gray = image::imageops::grayscale(&candidate_rgba);

    let edge_density = edge_density(&candidate_gray);
    let entropy = entropy(&candidate_gray);

    // Match the candidate back against the full image. A low-information
    // candidate is rejected by match_template_image; treat that as Reject.
    let matches = match match_template_image(index, &candidate_gray, &Region::Full, 32) {
        Ok(m) => m,
        Err(_) => {
            return Ok(TemplateSelfValidation {
                self_score: 0.0,
                second_best_score: None,
                peak_margin: 0.0,
                false_positive_count: 0,
                edge_density,
                entropy,
                stable_under_jitter: false,
                decision: TemplateDecision::Reject,
            });
        }
    };

    let self_match_index = matches
        .iter()
        .enumerate()
        .filter(|(_, m)| iou(m.bounds, candidate_bounds) >= 0.5)
        .max_by(|(_, a), (_, b)| a.score.total_cmp(&b.score))
        .map(|(index, _)| index);
    let self_score = self_match_index
        .and_then(|index| matches.get(index))
        .map(|m| m.score)
        .unwrap_or(0.0);
    let second_best_score = matches
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != self_match_index)
        .map(|(_, m)| m.score)
        .max_by(f32::total_cmp);

    let k = match cfg.expected_count {
        ExpectedCount::Unique => 1usize,
        ExpectedCount::Repeating => 2,
        ExpectedCount::AtLeast(n) => n.max(1) as usize,
    };
    // peak_margin: gap between the k-th accepted match and the next one.
    let peak_margin = match (matches.get(k - 1), matches.get(k)) {
        (Some(a), Some(b)) => a.score - b.score,
        (Some(_), None) => 1.0, // clean cliff: nothing beyond the expected set
        _ => 0.0,
    };
    let false_positive_count = matches
        .iter()
        .skip(k)
        .filter(|m| m.score >= FALSE_POSITIVE_SCORE)
        .count() as u32;

    let stable_under_jitter =
        jitter_stable(index, &candidate_rgba, candidate_bounds, self_score);

    let count_ok = match cfg.expected_count {
        ExpectedCount::Unique => true,
        ExpectedCount::Repeating => matches.iter().filter(|m| m.score >= FALSE_POSITIVE_SCORE).count() >= 2,
        ExpectedCount::AtLeast(n) => {
            matches.iter().filter(|m| m.score >= FALSE_POSITIVE_SCORE).count()
                >= n.max(1) as usize
        }
    };

    let coverage_ok = match cfg.target_bounds {
        None => true,
        Some(t) => matches.iter().any(|m| iou(m.bounds, t) >= 0.3),
    };

    let decision = decide(
        self_score,
        edge_density,
        entropy,
        peak_margin,
        false_positive_count,
        stable_under_jitter,
        area_ok,
        count_ok,
        coverage_ok,
    );

    Ok(TemplateSelfValidation {
        self_score,
        second_best_score,
        peak_margin,
        false_positive_count,
        edge_density,
        entropy,
        stable_under_jitter,
        decision,
    })
}

#[allow(clippy::too_many_arguments)]
fn decide(
    self_score: f32,
    edge_density: f32,
    entropy: f32,
    peak_margin: f32,
    false_positive_count: u32,
    stable: bool,
    area_ok: bool,
    count_ok: bool,
    coverage_ok: bool,
) -> TemplateDecision {
    let structural_floor = edge_density >= EDGE_DENSITY_FLOOR && entropy >= ENTROPY_FLOOR;
    if self_score < SELF_SCORE_FLOOR
        || !structural_floor
        || false_positive_count > 0
        || !stable
        || !area_ok
    {
        return TemplateDecision::Reject;
    }
    if peak_margin >= CLEAN_PEAK_MARGIN && count_ok && coverage_ok {
        return TemplateDecision::Pass;
    }
    TemplateDecision::NeedsConfirm
}

/// Fraction of pixels whose local gradient magnitude exceeds a threshold.
fn edge_density(gray: &image::GrayImage) -> f32 {
    let (w, h) = gray.dimensions();
    if w < 2 || h < 2 {
        return 0.0;
    }
    let mut edges = 0u32;
    let mut total = 0u32;
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let c = gray.get_pixel(x, y).0[0] as i32;
            let gx = (gray.get_pixel(x + 1, y).0[0] as i32 - c).abs();
            let gy = (gray.get_pixel(x, y + 1).0[0] as i32 - c).abs();
            if gx + gy > 30 {
                edges += 1;
            }
            total += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        edges as f32 / total as f32
    }
}

/// Shannon entropy of the 256-bin intensity histogram, in bits.
fn entropy(gray: &image::GrayImage) -> f32 {
    let mut hist = [0u32; 256];
    let mut n = 0u32;
    for p in gray.pixels() {
        hist[p.0[0] as usize] += 1;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    let nf = n as f32;
    let mut e = 0.0f32;
    for &c in hist.iter() {
        if c > 0 {
            let p = c as f32 / nf;
            e -= p * p.log2();
        }
    }
    e
}

/// Re-match brightness and ±1 px crop/padding variants. Every available
/// variant must return near its expected source location with bounded score
/// loss; this is author-time validation, so conservative rejection is correct.
fn jitter_stable(
    index: &VisualIndex,
    candidate_rgba: &image::RgbaImage,
    candidate_bounds: ImageRect,
    base_score: f32,
) -> bool {
    let mut variants: Vec<(image::GrayImage, ImageRect)> = Vec::new();

    let mut jittered = candidate_rgba.clone();
    for p in jittered.pixels_mut() {
        for c in 0..3 {
            p.0[c] = ((p.0[c] as f32) * 1.05).min(255.0) as u8;
        }
    }
    variants.push((image::imageops::grayscale(&jittered), candidate_bounds));

    let mut darkened = candidate_rgba.clone();
    for p in darkened.pixels_mut() {
        for c in 0..3 {
            p.0[c] = ((p.0[c] as f32) * 0.95).min(255.0) as u8;
        }
    }
    variants.push((image::imageops::grayscale(&darkened), candidate_bounds));

    let x = candidate_bounds.x.floor() as u32;
    let y = candidate_bounds.y.floor() as u32;
    let w = candidate_bounds.width.round() as u32;
    let h = candidate_bounds.height.round() as u32;
    if w > 4 && h > 4 {
        let inward = image::imageops::crop_imm(index.image(), x + 1, y + 1, w - 2, h - 2)
            .to_image();
        variants.push((
            image::imageops::grayscale(&inward),
            ImageRect {
                x: (x + 1) as f32,
                y: (y + 1) as f32,
                width: (w - 2) as f32,
                height: (h - 2) as f32,
            },
        ));
    }
    if x > 0 && y > 0 && x + w < index.width() && y + h < index.height() {
        let outward =
            image::imageops::crop_imm(index.image(), x - 1, y - 1, w + 2, h + 2).to_image();
        variants.push((
            image::imageops::grayscale(&outward),
            ImageRect {
                x: (x - 1) as f32,
                y: (y - 1) as f32,
                width: (w + 2) as f32,
                height: (h + 2) as f32,
            },
        ));
    }

    variants.into_iter().all(|(gray, expected)| {
        let matches = match match_template_image(index, &gray, &Region::Full, 4) {
            Ok(matches) => matches,
            Err(_) => return false,
        };
        matches.iter().any(|m| {
            iou(m.bounds, expected) >= 0.5 && base_score - m.score <= JITTER_SCORE_DROP
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::VisualIndex;
    use rollshot_image_document::ImageRect;

    fn cfg(expected: ExpectedCount) -> SelfValidationConfig {
        SelfValidationConfig { expected_count: expected, target_bounds: None }
    }

    // Scene with one distinctive non-periodic glyph at (10,12), size 8x8.
    fn distinctive_scene() -> image::RgbaImage {
        let mut scene = image::RgbaImage::from_fn(40, 40, |x, y| {
            let v = 120 + ((x * 3 + y * 5) % 23) as u8;
            image::Rgba([v, v, v, 255])
        });
        for dy in 0..8 {
            for dx in 0..8 {
                let v = ((dx * 31 + dy * 17 + dx * dy * 7) % 220) as u8;
                scene.put_pixel(10 + dx, 12 + dy, image::Rgba([v, v, v, 255]));
            }
        }
        scene
    }

    #[test]
    fn distinctive_candidate_passes() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Pass);
        assert!(v.self_score > 0.9);
        assert!(v.stable_under_jitter);
    }

    #[test]
    fn flat_candidate_is_rejected() {
        // Crop a uniform patch -> low edge/entropy.
        let index = VisualIndex::build(image::RgbaImage::from_pixel(
            40, 40, image::Rgba([180, 180, 180, 255]),
        ))
        .unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 5.0, y: 5.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Reject);
    }

    #[test]
    fn out_of_bounds_candidate_errors() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let e = self_validate(
            &index,
            ImageRect { x: 38.0, y: 38.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap_err();
        assert_eq!(e, crate::VisionError::CandidateOutOfBounds);
    }

    #[test]
    fn repeating_pattern_rejects_unique_expectation() {
        let mut scene = distinctive_scene();
        let glyph = image::imageops::crop_imm(&scene, 10, 12, 8, 8).to_image();
        image::imageops::replace(&mut scene, &glyph, 26, 12);
        let index = VisualIndex::build(scene).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Reject);
        assert!(v.false_positive_count >= 1);
    }

    #[test]
    fn candidate_area_gate_rejects_tiny_crop() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 1.0, height: 1.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Reject);
    }

    #[test]
    fn target_coverage_miss_needs_confirmation() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 8.0, height: 8.0 },
            &SelfValidationConfig {
                expected_count: ExpectedCount::Unique,
                target_bounds: Some(ImageRect {
                    x: 30.0,
                    y: 30.0,
                    width: 5.0,
                    height: 5.0,
                }),
            },
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::NeedsConfirm);
    }
}
