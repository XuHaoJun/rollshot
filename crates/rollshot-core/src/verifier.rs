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
            tracing::trace!(
                target: crate::diagnostics::TARGET_VERIFIER,
                dx = candidate.dx,
                dy = candidate.dy,
                overlap_area = region.area(),
                outcome = "InsufficientOverlap",
                "verify result"
            );
            return VerifierOutcome::InsufficientOverlap;
        }

        let downsample_mad = downsampled_mad(prev, curr, region, self.config.downsample_step);
        let downsample_ok =
            downsample_mad.is_finite() && downsample_mad <= self.config.downsample_max_mad;
        // Only the full-res band when the cheap gate passed (lazy — the band
        // scan is wasted when downsample already disagrees).
        let full_mad = if downsample_ok {
            sample_band_mad(prev, curr, region, self.config.sample_band)
        } else {
            f32::NAN
        };

        // Legacy strict-mean acceptance (preserved exactly → monotonic superset).
        let legacy_pass =
            downsample_ok && full_mad.is_finite() && full_mad <= self.config.full_res_max_mad;
        if legacy_pass {
            let score = full_mad.clamp(0.0, 1.0);
            tracing::trace!(
                target: crate::diagnostics::TARGET_VERIFIER,
                dx = candidate.dx,
                dy = candidate.dy,
                overlap_area = region.area(),
                downsample_mad = downsample_mad,
                full_mad = full_mad,
                outcome = "Pass",
                "verify result"
            );
            return VerifierOutcome::Pass {
                overlap: region,
                score,
            };
        }

        // Robust tile-vote acceptance: tolerate a localized minority of
        // disagreeing tiles, gated by how strongly the offset is supported.
        //
        // Cost note: this is a full-overlap scan reached ONLY on the reject
        // path (the legacy mean failed above), and `rank_verified_candidates`
        // calls `verify` once per candidate — so a reject-heavy frame pays a
        // tile scan for *every* candidate. That is the verifier-stage cost the
        // `bad_frame` benchmark shows (reject-dominated → large %). Clean
        // frames whose winning candidate passes the legacy mean return above
        // and never reach here, so steady-state scrolling is unaffected.
        let agree = tile_agreement(
            prev,
            curr,
            region,
            self.config.robust_tile_px,
            self.config.robust_tile_tol,
        );
        if agree >= required_agreement(candidate, self.config) {
            let score = if full_mad.is_finite() {
                full_mad.clamp(0.0, 1.0)
            } else {
                1.0 - agree
            };
            tracing::trace!(
                target: crate::diagnostics::TARGET_VERIFIER,
                dx = candidate.dx,
                dy = candidate.dy,
                overlap_area = region.area(),
                tile_agreement = agree,
                full_mad = full_mad,
                outcome = "Pass",
                "verify result"
            );
            return VerifierOutcome::Pass {
                overlap: region,
                score,
            };
        }

        tracing::trace!(
            target: crate::diagnostics::TARGET_VERIFIER,
            dx = candidate.dx,
            dy = candidate.dy,
            overlap_area = region.area(),
            downsample_mad = downsample_mad,
            full_mad = full_mad,
            tile_agreement = agree,
            outcome = "OverlapDisagreement",
            "verify result"
        );
        VerifierOutcome::OverlapDisagreement {
            downsample_mad,
            full_mad,
        }
    }
}

/// Required agreeing-tile fraction for `candidate`. A strongly-supported offset
/// (high NCC confidence — low score — or high feature inlier ratio) may drop to
/// the misfire floor; a weakly-supported offset must meet the strict ratio.
fn required_agreement(candidate: &MotionCandidate, config: &VerifierConfig) -> f32 {
    const STRONG_SCORE: f32 = 0.06;
    const STRONG_INLIER_RATIO: f32 = 0.5;
    let strong_ncc = candidate.score <= STRONG_SCORE;
    let strong_feature = matches!(
        (candidate.inliers, candidate.raw_matches),
        (Some(i), Some(r)) if r > 0 && (i as f32 / r as f32) >= STRONG_INLIER_RATIO
    );
    if strong_ncc || strong_feature {
        config.robust_accept_ratio_floor
    } else {
        config.robust_accept_ratio
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

/// Fraction of `tile_px`×`tile_px` tiles over the overlap whose mean absolute
/// difference is below `tile_tol`. Partial edge tiles count by their pixels.
/// Scans every overlap pixel once (O(overlap area)); only invoked on the
/// verifier reject path — see the cost note in `verify`.
fn tile_agreement(
    prev: &RgbaImage,
    curr: &RgbaImage,
    r: OverlapRegion,
    tile_px: u32,
    tile_tol: f32,
) -> f32 {
    let tile = tile_px.max(1);
    let mut total_tiles = 0u32;
    let mut agree_tiles = 0u32;
    let mut ty = 0u32;
    while ty < r.height {
        let th = tile.min(r.height - ty);
        let mut tx = 0u32;
        while tx < r.width {
            let tw = tile.min(r.width - tx);
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for row in 0..th {
                for col in 0..tw {
                    let p = pixel_gray(prev, r.prev_x + tx + col, r.prev_y + ty + row);
                    let c = pixel_gray(curr, r.curr_x + tx + col, r.curr_y + ty + row);
                    sum += (p - c).abs();
                    count += 1;
                }
            }
            let mad = if count == 0 {
                f32::INFINITY
            } else {
                sum / (count as f32 * 255.0)
            };
            total_tiles += 1;
            if mad <= tile_tol {
                agree_tiles += 1;
            }
            tx += tile;
        }
        ty += tile;
    }
    if total_tiles == 0 {
        return 0.0;
    }
    agree_tiles as f32 / total_tiles as f32
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

    #[test]
    fn tile_agreement_full_on_identical_overlap() {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let curr = crop(&canvas, 0, 40, 160, 160);
        let r = compute_overlap(160, 160, 160, 160, 0, 40).unwrap();
        let ratio = tile_agreement(&prev, &curr, r, 32, 24.0 / 255.0);
        assert!(
            ratio > 0.99,
            "identical overlap should fully agree, got {ratio}"
        );
    }

    #[test]
    fn tile_agreement_localized_change_is_majority_agree() {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let mut curr = crop(&canvas, 0, 40, 160, 160);
        for y in 100..160 {
            for x in 0..40 {
                curr.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            }
        }
        let r = compute_overlap(160, 160, 160, 160, 0, 40).unwrap();
        let ratio = tile_agreement(&prev, &curr, r, 32, 24.0 / 255.0);
        assert!(
            (0.6..0.99).contains(&ratio),
            "expected majority agree, got {ratio}"
        );
    }

    #[test]
    fn tile_agreement_low_on_global_mismatch() {
        let prev = textured(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));
        let r = compute_overlap(160, 160, 160, 160, 0, 20).unwrap();
        let ratio = tile_agreement(&prev, &curr, r, 32, 24.0 / 255.0);
        assert!(
            ratio < 0.4,
            "global mismatch should mostly disagree, got {ratio}"
        );
    }

    // Paint a localized block onto `curr` large enough that the legacy strict
    // mean band FAILS (so the tile-vote path is what decides), while leaving a
    // tile agreement (~0.67) above the misfire floor but below the strict
    // ratio — so confidence gating is the deciding factor between the two
    // tests below.
    fn localized_change_fixture() -> (RgbaImage, RgbaImage) {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let mut curr = crop(&canvas, 0, 40, 160, 160);
        for y in 64..160 {
            for x in 0..96 {
                curr.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            }
        }
        (prev, curr)
    }

    #[test]
    fn localized_change_accepted_via_tile_vote_when_confident() {
        let (prev, curr) = localized_change_fixture();
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        // strong support: low score (≈ high NCC confidence)
        let mut cand = candidate(0, 40);
        cand.score = 0.02;
        match verifier.verify(&prev, &curr, &cand) {
            VerifierOutcome::Pass { .. } => {}
            other => panic!("expected tile-vote Pass, got {other:?}"),
        }
    }

    #[test]
    fn localized_change_rejected_when_weakly_supported() {
        let (prev, curr) = localized_change_fixture();
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        let mut cand = candidate(0, 40);
        cand.score = 0.5; // weak support → strict accept_ratio applies
        assert!(matches!(
            verifier.verify(&prev, &curr, &cand),
            VerifierOutcome::OverlapDisagreement { .. }
        ));
    }
}
