use image::{Rgba, RgbaImage};

use crate::overlap::compute_overlap;
use crate::types::{MotionCandidate, OverlapRegion, VerifierConfig};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerifierOutcome {
    Pass { overlap: OverlapRegion, score: f32 },
    InsufficientOverlap,
    OverlapDisagreement { downsample_mad: f32, full_mad: f32 },
}

pub struct PixelOverlapVerifier<'a> {
    config: &'a VerifierConfig,
    min_overlap_area: u64,
}

impl<'a> PixelOverlapVerifier<'a> {
    pub fn new(config: &'a VerifierConfig, min_overlap: u32) -> Self {
        Self {
            config,
            min_overlap_area: u64::from(min_overlap) * u64::from(min_overlap),
        }
    }

    pub fn verify(
        &self,
        prev: &RgbaImage,
        curr: &RgbaImage,
        candidate: &MotionCandidate,
    ) -> VerifierOutcome {
        let region = match compute_overlap(
            prev.width(),
            prev.height(),
            curr.width(),
            curr.height(),
            candidate.dx,
            candidate.dy,
        ) {
            Some(r) => r,
            None => return VerifierOutcome::InsufficientOverlap,
        };

        if region.area() < self.min_overlap_area {
            return VerifierOutcome::InsufficientOverlap;
        }

        let downsample_mad = downsampled_mad(prev, curr, region, self.config.downsample_step);
        if !downsample_mad.is_finite() || downsample_mad > self.config.downsample_max_mad {
            return VerifierOutcome::OverlapDisagreement {
                downsample_mad,
                full_mad: f32::NAN,
            };
        }

        let full_mad = sample_band_mad(prev, curr, region, self.config.sample_band);
        if !full_mad.is_finite() || full_mad > self.config.full_res_max_mad {
            return VerifierOutcome::OverlapDisagreement {
                downsample_mad,
                full_mad,
            };
        }

        let score = full_mad.clamp(0.0, 1.0);
        VerifierOutcome::Pass {
            overlap: region,
            score,
        }
    }
}

fn pixel_gray(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let Rgba([r, g, b, _]) = *img.get_pixel(x, y);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

fn downsampled_mad(prev: &RgbaImage, curr: &RgbaImage, r: OverlapRegion, step: u32) -> f32 {
    let step = step.max(1);
    let mut sum = 0.0f32;
    let mut count = 0u32;
    let mut row = 0u32;
    while row < r.height {
        let mut col = 0u32;
        while col < r.width {
            let p = pixel_gray(prev, r.prev_x + col, r.prev_y + row);
            let c = pixel_gray(curr, r.curr_x + col, r.curr_y + row);
            sum += (p - c).abs();
            count += 1;
            col += step;
        }
        row += step;
    }
    if count == 0 {
        return f32::INFINITY;
    }
    sum / (count as f32 * 255.0)
}

fn sample_band_mad(prev: &RgbaImage, curr: &RgbaImage, r: OverlapRegion, sample_band: u32) -> f32 {
    if r.width == 0 || r.height == 0 {
        return f32::INFINITY;
    }
    let band_h = sample_band.min(r.height).max(1);
    let band_w = sample_band.min(r.width).max(1);
    let (use_h, use_w) = if r.height >= r.width {
        (band_h, r.width)
    } else {
        (r.height, band_w)
    };
    let row_start = r.height - use_h;
    let col_start = r.width - use_w;

    let mut sum = 0.0f32;
    let mut count = 0u32;
    for row in 0..use_h {
        for col in 0..use_w {
            let p = pixel_gray(prev, r.prev_x + col_start + col, r.prev_y + row_start + row);
            let c = pixel_gray(curr, r.curr_x + col_start + col, r.curr_y + row_start + row);
            sum += (p - c).abs();
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
    use super::*;
    use image::{imageops, Rgba, RgbaImage};

    fn textured(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
        for y in 0..height {
            for x in 0..width {
                if (x / 4 + y / 6) % 2 == 0 {
                    img.put_pixel(
                        x,
                        y,
                        Rgba([30, ((x * 7) % 200) as u8, ((y * 11) % 200) as u8, 255]),
                    );
                }
            }
        }
        img
    }

    fn crop(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, x, y, w, h).to_image()
    }

    fn candidate(dx: i32, dy: i32) -> MotionCandidate {
        MotionCandidate {
            dx,
            dy,
            method: MatchMethod::Template,
            score: 0.0,
            second_best_score: None,
            inliers: None,
            raw_matches: None,
        }
    }

    use crate::types::MatchMethod;

    #[test]
    fn matching_frames_with_known_motion_pass() {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let curr = crop(&canvas, 0, 40, 160, 160);
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        match verifier.verify(&prev, &curr, &candidate(0, 40)) {
            VerifierOutcome::Pass { overlap, score } => {
                assert_eq!(overlap.height, 120);
                assert!(score < cfg.full_res_max_mad);
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn unrelated_frames_fail_verification() {
        let prev = textured(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        let outcome = verifier.verify(&prev, &curr, &candidate(0, 20));
        assert!(matches!(
            outcome,
            VerifierOutcome::OverlapDisagreement { .. }
        ));
    }

    #[test]
    fn motion_with_no_overlap_returns_insufficient_overlap() {
        let prev = textured(64, 64);
        let curr = textured(64, 64);
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 32);
        let outcome = verifier.verify(&prev, &curr, &candidate(0, 200));
        assert_eq!(outcome, VerifierOutcome::InsufficientOverlap);
    }

    #[test]
    fn overlap_below_min_overlap_area_returns_insufficient_overlap() {
        let canvas = textured(120, 240);
        let prev = crop(&canvas, 0, 0, 120, 120);
        let curr = crop(&canvas, 0, 115, 120, 120);
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 80);
        let outcome = verifier.verify(&prev, &curr, &candidate(0, 115));
        assert_eq!(outcome, VerifierOutcome::InsufficientOverlap);
    }

    #[test]
    fn horizontal_right_motion_passes_with_aligned_crops() {
        let canvas = textured(320, 160);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let curr = crop(&canvas, 40, 0, 160, 160);
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        match verifier.verify(&prev, &curr, &candidate(40, 0)) {
            VerifierOutcome::Pass { overlap, .. } => {
                assert_eq!(overlap.width, 120);
                assert_eq!(overlap.height, 160);
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }
}
