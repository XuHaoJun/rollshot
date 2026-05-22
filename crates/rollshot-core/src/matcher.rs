use image::{Rgba, RgbaImage};
use rayon::prelude::*;

use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::overlap::compute_overlap;
use crate::types::{MatchMethod, MotionCandidate, NoMatchReason, ScrollAxis, StitchConfig};
use crate::verifier::{PixelOverlapVerifier, VerifierOutcome};

const TOP_IGNORE_RATIO: f32 = 0.12;
const BOTTOM_IGNORE_RATIO: f32 = 0.08;
const SIDE_IGNORE_RATIO: f32 = 0.04;
const MIN_IGNORE_PX: u32 = 24;
const COARSE_DOWNSAMPLE_STEP: u32 = 4;
const COARSE_AXIS_STRIDE: i32 = 8;
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MotionSearchOutcome {
    Candidate(MotionCandidate),
    NoMatch {
        reason: NoMatchReason,
        best_candidate: Option<MotionCandidate>,
    },
}

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SearchBudget {
    coarse_score_calls: u64,
    full_res_ncc_calls: u64,
    full_res_ncc_pixel_visits: u64,
    verifier_calls: u64,
}

#[cfg(test)]
static ACTIVE_SEARCH_BUDGET: std::sync::Mutex<Option<SearchBudget>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn estimate_motion_with_budget(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    budget: &mut SearchBudget,
) -> MotionSearchOutcome {
    let result =
        with_search_budget(|| estimate_motion(prev, curr, locked_axis, last_motion, config));
    *budget = take_search_budget();
    result
}

#[cfg(test)]
fn with_search_budget<R>(f: impl FnOnce() -> R) -> R {
    {
        let mut active = ACTIVE_SEARCH_BUDGET
            .lock()
            .expect("search budget mutex poisoned");
        assert!(active.is_none(), "nested search budgets are not supported");
        *active = Some(SearchBudget::default());
    }
    f()
}

#[cfg(test)]
fn take_search_budget() -> SearchBudget {
    ACTIVE_SEARCH_BUDGET
        .lock()
        .expect("search budget mutex poisoned")
        .take()
        .unwrap_or_default()
}

#[cfg(test)]
fn with_active_search_budget(f: impl FnOnce(&mut SearchBudget)) {
    let mut active = ACTIVE_SEARCH_BUDGET
        .lock()
        .expect("search budget mutex poisoned");
    if let Some(budget) = active.as_mut() {
        f(budget);
    }
}

pub(crate) fn estimate_motion(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
) -> MotionSearchOutcome {
    if prev.dimensions() != curr.dimensions() {
        return MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::DimensionMismatch,
            best_candidate: None,
        };
    }

    let width = prev.width();
    let height = prev.height();
    let prev_gray = to_grayscale(prev);
    let curr_gray = to_grayscale(curr);

    let mut candidates = Vec::new();
    let coarse = coarse_candidates(&prev_gray, &curr_gray, width, height, locked_axis, config);
    candidates.extend(coarse.iter().copied());
    candidates.extend(template_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        last_motion,
        &coarse,
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

    if let Some(candidate) = rank_verified_candidates(prev, curr, locked_axis, candidates, config) {
        return MotionSearchOutcome::Candidate(candidate);
    }

    match akaze_candidates(prev, curr, &config.akaze) {
        AkazeCandidateOutcome::Disabled => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::AkazeDisabled,
            best_candidate: None,
        },
        AkazeCandidateOutcome::NotEnoughFeatures { .. } => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::NotEnoughFeatures,
            best_candidate: None,
        },
        AkazeCandidateOutcome::NotEnoughMatches { .. } => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::AkazeLowInliers,
            best_candidate: None,
        },
        AkazeCandidateOutcome::Candidates(akaze_candidates) => {
            let best = akaze_candidates.first().copied();
            match rank_verified_candidates(prev, curr, locked_axis, akaze_candidates, config) {
                Some(candidate) => MotionSearchOutcome::Candidate(candidate),
                None => MotionSearchOutcome::NoMatch {
                    reason: NoMatchReason::AkazeLowInliers,
                    best_candidate: best,
                },
            }
        }
    }
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

        #[cfg(test)]
        with_active_search_budget(|budget| budget.verifier_calls += 1);

        let verifier_score = match verifier.verify(prev, curr, &candidate) {
            VerifierOutcome::Pass { score, .. } => score,
            VerifierOutcome::InsufficientOverlap | VerifierOutcome::OverlapDisagreement { .. } => {
                continue
            }
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
        Some(axis) => {
            if dx == 0 && dy == 0 {
                return false;
            }

            matches!(
                validate_with_lock(axis, dx, dy, config.max_cross_axis_px),
                AxisValidation::OnAxis { .. } | AxisValidation::AxisChanged { .. }
            )
        }
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

fn search_axes(locked_axis: Option<ScrollAxis>) -> &'static [SearchAxis] {
    match locked_axis {
        Some(ScrollAxis::Vertical) | Some(ScrollAxis::Horizontal) => {
            &[SearchAxis::Vertical, SearchAxis::Horizontal]
        }
        None => &[SearchAxis::Vertical, SearchAxis::Horizontal],
    }
}

fn predicted_offset(axis: SearchAxis, last_motion: (i32, i32)) -> i32 {
    match axis {
        SearchAxis::Vertical => last_motion.1,
        SearchAxis::Horizontal => last_motion.0,
    }
}

fn template_refine_radius(_config: &StitchConfig) -> i32 {
    COARSE_DOWNSAMPLE_STEP as i32 * COARSE_AXIS_STRIDE * 2 + 16
}

fn template_seed(axis: SearchAxis, last_motion: (i32, i32), coarse: &[MotionCandidate]) -> i32 {
    let predicted = predicted_offset(axis, last_motion);
    if predicted != 0 {
        return predicted;
    }
    coarse
        .iter()
        .find_map(|candidate| match axis {
            SearchAxis::Vertical if candidate.dx == 0 => Some(candidate.dy),
            SearchAxis::Horizontal if candidate.dy == 0 => Some(candidate.dx),
            _ => None,
        })
        .unwrap_or(predicted)
}

#[allow(clippy::too_many_arguments)]
fn template_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let mut out = Vec::new();
    let roi = content_roi(width, height);
    let match_region = match_width_region(roi, config.match_width);

    for axis in search_axes(locked_axis) {
        let seed = template_seed(*axis, last_motion, coarse);
        if let Some(candidate) = search_template_axis(
            prev_gray,
            curr_gray,
            width,
            height,
            *axis,
            match_region,
            seed,
            config,
        ) {
            out.push(candidate);
        }
    }

    out
}

#[allow(clippy::too_many_arguments)]
fn search_template_axis(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    axis: SearchAxis,
    region: Region,
    last_offset: i32,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    if width < 50 || height < 50 {
        return None;
    }

    let max_offset = match axis {
        SearchAxis::Vertical => (height as i32 - config.min_overlap as i32).max(0),
        SearchAxis::Horizontal => (width as i32 - config.min_overlap as i32).max(0),
    };
    let max_offset = max_offset.min(match axis {
        SearchAxis::Vertical => (height as f32 * config.max_search_ratio) as i32,
        SearchAxis::Horizontal => (width as f32 * config.max_search_ratio) as i32,
    });
    if max_offset <= 0 {
        return None;
    }

    let offsets = refinement_offsets(last_offset, max_offset, template_refine_radius(config));
    let scored: Vec<_> = offsets
        .into_par_iter()
        .filter_map(|offset| {
            let score = match axis {
                SearchAxis::Vertical => {
                    ncc_score_shifted(prev_gray, curr_gray, width, height, region, 0, offset)
                }
                SearchAxis::Horizontal => {
                    ncc_score_shifted(prev_gray, curr_gray, width, height, region, offset, 0)
                }
            };
            score.is_finite().then_some((score, offset))
        })
        .collect();

    let mut scored: Vec<(f32, i32)> = scored.into_iter().collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let (best_score, best_offset) = scored.first().copied()?;
    let second_score = scored.get(1).map(|(score, _)| *score).unwrap_or(f32::MIN);

    if !best_score.is_finite() || best_score <= 0.0 {
        return None;
    }

    let confidence = 1.0 - best_score.clamp(0.0, 1.0);
    let second_confidence = if second_score.is_finite() {
        Some(1.0 - second_score.clamp(0.0, 1.0))
    } else {
        None
    };

    let (dx, dy) = match axis {
        SearchAxis::Vertical => (0, best_offset),
        SearchAxis::Horizontal => (best_offset, 0),
    };

    Some(candidate(
        dx,
        dy,
        MatchMethod::Template,
        confidence,
        second_confidence,
    ))
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

fn coarse_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let step = COARSE_DOWNSAMPLE_STEP as i32;
    let (sample_w, sample_h) = coarse_sample_dimensions(width, height, COARSE_DOWNSAMPLE_STEP);
    let prev_samples = coarse_samples(prev_gray, width, height, COARSE_DOWNSAMPLE_STEP);
    let curr_samples = coarse_samples(curr_gray, width, height, COARSE_DOWNSAMPLE_STEP);
    let max_dx = ((width as f32 * config.max_search_ratio) as i32 / step).max(0);
    let max_dy = ((height as f32 * config.max_search_ratio) as i32 / step).max(0);

    let mut out = Vec::new();
    for axis in search_axes(locked_axis) {
        let max_offset = match axis {
            SearchAxis::Vertical => max_dy,
            SearchAxis::Horizontal => max_dx,
        };
        if let Some(candidate) = coarse_axis_candidate(
            &prev_samples,
            &curr_samples,
            sample_w,
            sample_h,
            *axis,
            max_offset,
            0,
        ) {
            out.push(candidate);
        }
    }

    out.into_iter()
        .map(|mut candidate| {
            candidate.dx *= step;
            candidate.dy *= step;
            candidate
        })
        .filter(|candidate| candidate_matches_axis(candidate.dx, candidate.dy, locked_axis, config))
        .collect()
}

fn coarse_axis_candidate(
    prev_samples: &[f32],
    curr_samples: &[f32],
    sample_w: u32,
    sample_h: u32,
    axis: SearchAxis,
    max_offset: i32,
    predicted: i32,
) -> Option<MotionCandidate> {
    let min_dim = sample_w.min(sample_h) as i32;
    let stride = if min_dim < 60 { 2 } else { COARSE_AXIS_STRIDE };
    let offsets: Vec<i32> = coarse_axis_offsets(max_offset, predicted, stride)
        .into_iter()
        .filter(|offset| *offset != 0)
        .collect();
    #[cfg(test)]
    with_active_search_budget(|budget| budget.coarse_score_calls += offsets.len() as u64);

    let mut scored: Vec<_> = offsets
        .into_par_iter()
        .filter_map(|offset| {
            let (dx, dy) = match axis {
                SearchAxis::Vertical => (0, offset),
                SearchAxis::Horizontal => (offset, 0),
            };
            let diff = coarse_mad(prev_samples, curr_samples, sample_w, sample_h, dx, dy, 1);
            diff.is_finite().then_some((diff, dx, dy))
        })
        .collect();

    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let (best_score, best_dx, best_dy) = *scored.first()?;
    let second = scored.get(1).map(|(score, _, _)| *score);
    Some(candidate(
        best_dx,
        best_dy,
        MatchMethod::Coarse,
        best_score,
        second,
    ))
}

fn coarse_sample_dimensions(width: u32, height: u32, step: u32) -> (u32, u32) {
    let step = step.max(1);
    (width.div_ceil(step).max(1), height.div_ceil(step).max(1))
}

fn coarse_samples(gray: &[f32], width: u32, height: u32, step: u32) -> Vec<f32> {
    let step = step.max(1);
    let (sample_w, sample_h) = coarse_sample_dimensions(width, height, step);
    let mut out = Vec::with_capacity((sample_w * sample_h) as usize);
    let mut y = 0;
    while y < height {
        let mut x = 0;
        while x < width {
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for yy in y..(y + step).min(height) {
                for xx in x..(x + step).min(width) {
                    sum += gray[(yy * width + xx) as usize];
                    count += 1;
                }
            }
            out.push(sum / count.max(1) as f32);
            x += step;
        }
        y += step;
    }
    out
}

fn coarse_mad(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    dx: i32,
    dy: i32,
    step: u32,
) -> f32 {
    let overlap = match compute_overlap(width, height, width, height, dx, dy) {
        Some(overlap) => overlap,
        None => return f32::INFINITY,
    };

    let mut sum = 0.0f32;
    let mut count = 0u32;
    let mut y = 0;
    while y < overlap.height {
        let mut x = 0;
        while x < overlap.width {
            let prev_idx = ((overlap.prev_y + y) * width + overlap.prev_x + x) as usize;
            let curr_idx = ((overlap.curr_y + y) * width + overlap.curr_x + x) as usize;
            sum += (prev_gray[prev_idx] - curr_gray[curr_idx]).abs();
            count += 1;
            x += step.max(1);
        }
        y += step.max(1);
    }
    if count == 0 {
        return f32::INFINITY;
    }
    sum / (count as f32 * 255.0)
}

fn edge_projection_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let mut out = Vec::new();

    for axis in search_axes(locked_axis) {
        if let Some(candidate) =
            edge_projection_axis(prev_gray, curr_gray, width, height, *axis, config)
        {
            out.push(candidate);
        }
    }

    out
}

fn edge_projection_axis(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    axis: SearchAxis,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    let max_offset = match axis {
        SearchAxis::Vertical => (height as f32 * config.max_search_ratio) as i32,
        SearchAxis::Horizontal => (width as f32 * config.max_search_ratio) as i32,
    };
    if max_offset <= 0 {
        return None;
    }

    let prev_proj = edge_projection(prev_gray, width, height, axis);
    let curr_proj = edge_projection(curr_gray, width, height, axis);
    let mut scored = Vec::new();
    for offset in signed_predict_iter(max_offset, 0) {
        let score = projection_mad(
            &prev_proj,
            &curr_proj,
            offset,
            EDGE_PROJECTION_STEP as usize,
        );
        if score.is_finite() {
            scored.push((score, offset));
        }
    }

    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (best, offset) = *scored.first()?;
    let second = scored.get(1).map(|(score, _)| *score);
    let (dx, dy) = match axis {
        SearchAxis::Vertical => (0, offset),
        SearchAxis::Horizontal => (offset, 0),
    };

    Some(candidate(dx, dy, MatchMethod::Edge, best, second))
}

fn edge_projection(gray: &[f32], width: u32, height: u32, axis: SearchAxis) -> Vec<f32> {
    match axis {
        SearchAxis::Vertical => {
            let mut rows = vec![0.0; height as usize];
            for y in 1..height {
                let mut sum = 0.0;
                for x in 0..width {
                    let idx = (y * width + x) as usize;
                    let prev = ((y - 1) * width + x) as usize;
                    sum += (gray[idx] - gray[prev]).abs();
                }
                rows[y as usize] = sum / width.max(1) as f32 / 255.0;
            }
            rows
        }
        SearchAxis::Horizontal => {
            let mut cols = vec![0.0; width as usize];
            for x in 1..width {
                let mut sum = 0.0;
                for y in 0..height {
                    let idx = (y * width + x) as usize;
                    let prev = (y * width + x - 1) as usize;
                    sum += (gray[idx] - gray[prev]).abs();
                }
                cols[x as usize] = sum / height.max(1) as f32 / 255.0;
            }
            cols
        }
    }
}

fn projection_mad(prev: &[f32], curr: &[f32], offset: i32, step: usize) -> f32 {
    let prev_start = offset.max(0) as usize;
    let curr_start = (-offset).max(0) as usize;
    let overlap = prev
        .len()
        .min(curr.len())
        .saturating_sub(offset.unsigned_abs() as usize);
    if overlap == 0 {
        return f32::INFINITY;
    }

    let step = step.max(1);
    let mut sum = 0.0f32;
    let mut count = 0usize;
    for i in (0..overlap).step_by(step) {
        sum += (prev[prev_start + i] - curr[curr_start + i]).abs();
        count += 1;
    }
    if count == 0 {
        return f32::INFINITY;
    }
    sum / count as f32
}

fn to_grayscale(img: &RgbaImage) -> Vec<f32> {
    img.pixels()
        .map(|Rgba([r, g, b, _])| 0.299 * *r as f32 + 0.587 * *g as f32 + 0.114 * *b as f32)
        .collect()
}

fn coarse_axis_offsets(max_abs: i32, predict: i32, step: i32) -> Vec<i32> {
    let max_abs = max_abs.max(0);
    let step = step.max(1);
    let predict = predict.clamp(-max_abs, max_abs);
    let mut out = Vec::new();
    out.push(predict);

    let mut delta = step;
    while delta <= max_abs {
        if predict + delta <= max_abs {
            out.push(predict + delta);
        }
        if predict - delta >= -max_abs {
            out.push(predict - delta);
        }
        delta += step;
    }

    if !out.contains(&max_abs) {
        out.push(max_abs);
    }
    if max_abs != 0 && !out.contains(&-max_abs) {
        out.push(-max_abs);
    }

    out
}

fn refinement_offsets(seed: i32, max_abs: i32, radius: i32) -> Vec<i32> {
    let seed = seed.clamp(-max_abs, max_abs);
    let radius = radius.max(0);
    let start = (seed - radius).max(-max_abs);
    let end = (seed + radius).min(max_abs);
    let mut out = Vec::with_capacity((end - start + 1).max(0) as usize);
    out.push(seed);
    for delta in 1..=radius {
        if seed + delta <= end {
            out.push(seed + delta);
        }
        if seed - delta >= start {
            out.push(seed - delta);
        }
    }
    out
}

fn signed_predict_iter(max_abs: i32, predict: i32) -> Vec<i32> {
    let p = predict.clamp(-max_abs, max_abs);
    let mut out = Vec::with_capacity((max_abs as usize).saturating_mul(2) + 1);
    out.push(p);
    for delta in 1..=max_abs {
        if p + delta <= max_abs {
            out.push(p + delta);
        }
        if p - delta >= -max_abs {
            out.push(p - delta);
        }
    }
    out
}

fn ncc_score_shifted(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    region: Region,
    dx: i32,
    dy: i32,
) -> f32 {
    let overlap = match compute_overlap(width, height, width, height, dx, dy) {
        Some(overlap) => overlap,
        None => return f32::MIN,
    };
    let x0 = region.x.max(overlap.prev_x);
    let y0 = region.y.max(overlap.prev_y);
    let x1 = (region.x + region.w).min(overlap.prev_x + overlap.width);
    let y1 = (region.y + region.h).min(overlap.prev_y + overlap.height);
    if x1 <= x0 || y1 <= y0 {
        return f32::MIN;
    }

    #[cfg(test)]
    with_active_search_budget(|budget| {
        budget.full_res_ncc_calls += 1;
        budget.full_res_ncc_pixel_visits += u64::from(x1 - x0) * u64::from(y1 - y0) * 2;
    });

    let mut prev_sum = 0.0f32;
    let mut curr_sum = 0.0f32;
    let mut count = 0usize;
    for prev_y in y0..y1 {
        for prev_x in x0..x1 {
            let curr_x = (prev_x as i32 - dx) as u32;
            let curr_y = (prev_y as i32 - dy) as u32;
            let prev_idx = (prev_y * width + prev_x) as usize;
            let curr_idx = (curr_y * width + curr_x) as usize;
            prev_sum += prev_gray[prev_idx];
            curr_sum += curr_gray[curr_idx];
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
    for prev_y in y0..y1 {
        for prev_x in x0..x1 {
            let curr_x = (prev_x as i32 - dx) as u32;
            let curr_y = (prev_y as i32 - dy) as u32;
            let prev_idx = (prev_y * width + prev_x) as usize;
            let curr_idx = (curr_y * width + curr_x) as usize;
            let p = prev_gray[prev_idx] - prev_mean;
            let c = curr_gray[curr_idx] - curr_mean;
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

#[cfg(test)]
mod tests {
    use super::{
        coarse_axis_offsets, coarse_sample_dimensions, content_roi, estimate_motion,
        estimate_motion_with_budget, refinement_offsets, template_refine_radius,
        MotionSearchOutcome, SearchBudget, COARSE_AXIS_STRIDE, COARSE_DOWNSAMPLE_STEP,
    };
    #[cfg(feature = "akaze")]
    use crate::types::AkazeConfig;
    #[cfg(feature = "akaze")]
    use crate::types::{MatchMethod, NoMatchReason};
    use crate::types::{MotionCandidate, ScrollAxis, StitchConfig};
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

    fn unwrap_candidate(outcome: MotionSearchOutcome) -> MotionCandidate {
        match outcome {
            MotionSearchOutcome::Candidate(candidate) => candidate,
            other => panic!("expected candidate, got {other:?}"),
        }
    }

    #[cfg(feature = "akaze")]
    fn make_sparse_feature_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = make_repeated_grid(width, height);
        for i in 0..64u32 {
            let x = 18 + ((i * 41) % width.saturating_sub(36).max(1));
            let y = 18 + ((i * 67) % height.saturating_sub(36).max(1));
            for yy in y..(y + 7).min(height) {
                for xx in x..(x + 7).min(width) {
                    if xx == x || yy == y || xx == x + yy - y {
                        img.put_pixel(xx, yy, Rgba([15, 15, 15, 255]));
                    }
                }
            }
        }
        img
    }

    #[cfg(feature = "akaze")]
    fn fallback_config() -> StitchConfig {
        StitchConfig {
            second_best_margin: 0.25,
            akaze: AkazeConfig {
                enabled: true,
                max_features: 1200,
                detector_threshold: 0.0005,
                min_raw_matches: 8,
                min_inliers: 6,
                min_inlier_ratio: 0.25,
            },
            ..StitchConfig::default()
        }
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
        let candidate = unwrap_candidate(estimate_motion(&prev, &curr, None, (0, 0), &config));
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
        let candidate = unwrap_candidate(estimate_motion(
            &prev,
            &curr,
            None,
            (0, 0),
            &StitchConfig::default(),
        ));
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
        assert!(matches!(
            estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default()),
            MotionSearchOutcome::NoMatch { .. }
        ));
    }

    #[test]
    fn estimate_motion_returns_none_for_dimension_mismatch() {
        let prev = make_textured_canvas(160, 160);
        let curr = make_textured_canvas(160, 200);
        assert!(matches!(
            estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default()),
            MotionSearchOutcome::NoMatch { .. }
        ));
    }

    #[test]
    fn estimate_motion_finds_vertical_up_scroll() {
        let canvas = make_textured_canvas(160, 700);
        let prev = crop(&canvas, 220, 160);
        let curr = crop(&canvas, 180, 160);

        let candidate = unwrap_candidate(estimate_motion(
            &prev,
            &curr,
            None,
            (0, 0),
            &StitchConfig::default(),
        ));

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

        let candidate = unwrap_candidate(estimate_motion(
            &prev,
            &curr,
            None,
            (0, 0),
            &StitchConfig::default(),
        ));

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

        let candidate = unwrap_candidate(estimate_motion(
            &prev,
            &curr,
            Some(ScrollAxis::Horizontal),
            (40, 0),
            &StitchConfig::default(),
        ));

        assert_eq!(candidate.dy, 0);
        assert!(
            (candidate.dx + 40).abs() <= 2,
            "dx = {} (expected ~-40)",
            candidate.dx
        );
    }

    #[test]
    fn locked_vertical_hint_rejects_unrelated_frame() {
        let canvas = make_wide_canvas(700, 160);
        let prev = crop_xy(&canvas, 0, 0, 160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));

        let candidate = estimate_motion(
            &prev,
            &curr,
            Some(ScrollAxis::Vertical),
            (0, 40),
            &StitchConfig::default(),
        );

        assert!(matches!(candidate, MotionSearchOutcome::NoMatch { .. }));
    }

    #[test]
    fn repeated_grid_is_rejected_by_second_best_margin() {
        let canvas = make_repeated_grid(240, 560);
        let prev = crop_xy(&canvas, 0, 0, 160, 160);
        let curr = crop_xy(&canvas, 0, 32, 160, 160);

        let candidate = estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default());

        assert!(matches!(candidate, MotionSearchOutcome::NoMatch { .. }));
    }

    #[test]
    fn locked_vertical_still_returns_reliable_axis_change_candidate() {
        let canvas = make_wide_canvas(700, 160);
        let prev = crop_xy(&canvas, 0, 0, 160, 160);
        let curr = crop_xy(&canvas, 40, 0, 160, 160);

        let candidate = unwrap_candidate(estimate_motion(
            &prev,
            &curr,
            Some(ScrollAxis::Vertical),
            (0, 40),
            &StitchConfig::default(),
        ));

        assert_eq!(candidate.dy, 0);
        assert!(
            (candidate.dx - 40).abs() <= 2,
            "dx = {} (expected ~40)",
            candidate.dx
        );
    }

    #[test]
    fn coarse_matching_uses_subsampled_dimensions() {
        assert_eq!(
            coarse_sample_dimensions(1920, 1080, COARSE_DOWNSAMPLE_STEP),
            (480, 270)
        );
        assert_eq!(
            coarse_sample_dimensions(3, 2, COARSE_DOWNSAMPLE_STEP),
            (1, 1)
        );
    }

    #[test]
    fn coarse_axis_offsets_are_bounded_for_large_frames() {
        let offsets = coarse_axis_offsets(2205, 0, 32);
        assert_eq!(offsets.first().copied(), Some(0));
        assert!(offsets.contains(&2205));
        assert!(offsets.contains(&-2205));
        assert!(
            offsets.len() <= 141,
            "offset count should stay bounded, got {}",
            offsets.len()
        );
    }

    #[cfg(feature = "akaze")]
    #[test]
    fn akaze_fallback_recovers_repeated_grid_with_sparse_features() {
        let canvas = make_sparse_feature_canvas(360, 760);
        let prev = crop_xy(&canvas, 0, 0, 240, 240);
        let curr = crop_xy(&canvas, 0, 72, 240, 240);

        let outcome = estimate_motion(&prev, &curr, None, (0, 0), &fallback_config());
        let candidate = match outcome {
            MotionSearchOutcome::Candidate(candidate) => candidate,
            other => panic!("expected AKAZE candidate, got {other:?}"),
        };

        assert_eq!(candidate.method, MatchMethod::Akaze);
        assert_eq!(candidate.dx, 0);
        assert!((candidate.dy - 72).abs() <= 3, "dy = {}", candidate.dy);
        assert!(candidate.inliers.unwrap_or(0) >= 6);
    }

    #[cfg(feature = "akaze")]
    #[test]
    fn akaze_attempt_with_blank_frames_reports_not_enough_features() {
        let prev = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));
        let curr = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));

        let outcome = estimate_motion(&prev, &curr, None, (0, 0), &fallback_config());

        assert!(matches!(
            outcome,
            MotionSearchOutcome::NoMatch {
                reason: NoMatchReason::NotEnoughFeatures,
                best_candidate: None,
            }
        ));
    }

    #[test]
    fn large_pair_stays_within_structural_search_budget() {
        let canvas = make_textured_canvas(1470, 900);
        let prev = crop(&canvas, 0, 660);
        let curr = crop(&canvas, 110, 660);
        let config = StitchConfig::default();

        let mut budget = SearchBudget::default();
        let candidate = unwrap_candidate(estimate_motion_with_budget(
            &prev,
            &curr,
            None,
            (0, 0),
            &config,
            &mut budget,
        ));

        assert_eq!(candidate.dx, 0);
        assert!(
            (candidate.dy - 110).abs() <= 3,
            "dy = {} (expected ~110)",
            candidate.dy
        );
        assert!(
            budget.coarse_score_calls <= 4096,
            "coarse_score_calls = {}",
            budget.coarse_score_calls
        );
        assert!(
            budget.full_res_ncc_calls <= 384,
            "full_res_ncc_calls = {}",
            budget.full_res_ncc_calls
        );
        assert!(
            budget.full_res_ncc_pixel_visits <= 200_000_000,
            "full_res_ncc_pixel_visits = {}",
            budget.full_res_ncc_pixel_visits
        );
    }

    #[test]
    fn refinement_offsets_stay_near_seed() {
        let radius = template_refine_radius(&StitchConfig::default());
        assert!(
            radius >= COARSE_DOWNSAMPLE_STEP as i32 * COARSE_AXIS_STRIDE,
            "radius = {radius}"
        );

        let offsets = refinement_offsets(220, 990, radius);
        assert_eq!(offsets.first().copied(), Some(220));
        assert!(offsets.contains(&(220 - radius)));
        assert!(offsets.contains(&(220 + radius)));
        assert!(!offsets.contains(&0));
        assert!(
            offsets.len() <= (radius * 2 + 1) as usize,
            "len = {}",
            offsets.len()
        );
    }

    #[cfg(feature = "akaze")]
    #[test]
    fn akaze_candidate_rejected_by_verifier_preserves_best_estimate() {
        let canvas = make_sparse_feature_canvas(360, 760);
        let prev = crop_xy(&canvas, 0, 0, 240, 240);
        let mut curr = crop_xy(&canvas, 0, 72, 240, 240);
        for y in 120..240 {
            for x in 0..240 {
                let v = ((x * 41 + y * 67) % 255) as u8;
                curr.put_pixel(x, y, Rgba([v, 255 - v, v / 2, 255]));
            }
        }

        let outcome = estimate_motion(&prev, &curr, None, (0, 0), &fallback_config());

        let best = match outcome {
            MotionSearchOutcome::NoMatch {
                reason: NoMatchReason::AkazeLowInliers,
                best_candidate: Some(candidate),
            } => candidate,
            other => panic!("expected AkazeLowInliers with best_candidate, got {other:?}"),
        };
        assert_eq!(best.method, MatchMethod::Akaze);
        assert!((best.dy - 72).abs() <= 8, "best dy = {}", best.dy);
    }

    #[test]
    #[ignore = "release-mode perf smoke; run manually with --ignored --nocapture"]
    fn large_retina_pair_perf_smoke() {
        let canvas = make_textured_canvas(2940, 1800);
        let prev = crop(&canvas, 0, 1320);
        let curr = crop(&canvas, 220, 1320);
        let config = StitchConfig::default();

        let started = std::time::Instant::now();
        let outcome = estimate_motion(&prev, &curr, None, (0, 0), &config);
        let elapsed = started.elapsed();
        let candidate = unwrap_candidate(outcome);

        let mut budget = SearchBudget::default();
        let budget_candidate = unwrap_candidate(estimate_motion_with_budget(
            &prev,
            &curr,
            None,
            (0, 0),
            &config,
            &mut budget,
        ));

        println!(
            "large_retina_pair_perf_smoke: elapsed={:.3}s parallelism={} candidate={:?} budget_candidate={:?} budget={:?}",
            elapsed.as_secs_f64(),
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            candidate,
            budget_candidate,
            budget
        );

        assert!(
            candidate.dy != 0,
            "matcher should find some vertical motion, got dy=0"
        );
        assert_eq!(budget_candidate.dx, candidate.dx);
        assert_eq!(budget_candidate.dy, candidate.dy);

        if std::env::var_os("ROLLSHOT_PERF_STRICT").is_some() {
            assert!(
                elapsed.as_secs_f64() < 1.0,
                "release perf smoke exceeded 1.0s: elapsed={elapsed:?}, budget={budget:?}"
            );
        }
    }
}
