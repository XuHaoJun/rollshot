use image::RgbaImage;

use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::canvas::{CanvasAppendError, StripCanvas};
use crate::duplicate;
use crate::matcher::{estimate_motion, MotionSearchOutcome, PreparedFrame};
use crate::metrics::{StitchMetrics, StitchOutcomeKind};
use crate::overlap::compute_overlap;
use crate::types::{
    AppendDirection, MotionCandidate, MotionEstimate, NoMatchReason, OverlapRegion,
    RecoveryProbeResult, ScrollAxis, StitchConfig, StitchOutcome, StitchStats,
};
use crate::verifier::{PixelOverlapVerifier, VerifierOutcome};

pub struct Stitcher {
    config: StitchConfig,
    canvas: Option<StripCanvas>,
    last_good: Option<PreparedFrame>,
    last_motion: (i32, i32),
    locked_axis: Option<ScrollAxis>,
    locked_direction: Option<AppendDirection>,
    stats: StitchStats,
    last_metrics: StitchMetrics,
    frame_counter: usize,
    first_frame_misses: u32,
}

/// Consecutive `NoMatch` results, while still stuck on the first frame, after
/// which the stitcher discards that frame and re-anchors to the latest one. A
/// stale/bad first frame (e.g. a lazy-loaded image that had not painted yet
/// when it was grabbed) otherwise blocks the capture forever, because the
/// anchor only advances on a successful append.
const REANCHOR_MISS_THRESHOLD: u32 = 2;

impl Stitcher {
    pub fn new(config: StitchConfig) -> Self {
        Self {
            config,
            canvas: None,
            last_good: None,
            last_motion: (0, 0),
            locked_axis: None,
            locked_direction: None,
            stats: StitchStats::default(),
            last_metrics: StitchMetrics::default(),
            frame_counter: 0,
            first_frame_misses: 0,
        }
    }

    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        self.push_frame_with_reanchor(frame, true)
    }

    pub fn push_frame_preserving_anchor(&mut self, frame: RgbaImage) -> StitchOutcome {
        self.push_frame_with_reanchor(frame, false)
    }

    fn push_frame_with_reanchor(
        &mut self,
        frame: RgbaImage,
        allow_reanchor: bool,
    ) -> StitchOutcome {
        let total_start = std::time::Instant::now();
        self.last_metrics = StitchMetrics::default();
        self.last_metrics.frame_index = self.frame_counter;
        self.frame_counter += 1;

        // Keep a copy only while at risk of re-anchoring (mid miss-streak), so
        // clean steady-state frames pay no clone cost. A stale/bad anchor
        // (e.g. a lazy-load image not yet painted) must not block forever.
        let reanchor_candidate = if allow_reanchor
            && self.canvas.is_some()
            && self.first_frame_misses + 1 >= REANCHOR_MISS_THRESHOLD
        {
            Some(frame.clone())
        } else {
            None
        };

        let outcome = self.push_frame_inner(frame);

        if allow_reanchor {
            // Only a genuine content disagreement counts toward the re-anchor floor.
            // `ReverseDirection` is a deliberate, valid rejection (the user scrolled
            // back the way they came); the overlapping frame is not a stall, so it
            // must not erode the monotonic-direction guard by triggering a re-anchor.
            let counts_as_miss = matches!(
                outcome,
                StitchOutcome::NoMatch { reason, .. }
                    if reason != NoMatchReason::ReverseDirection
            );
            if counts_as_miss {
                if self.canvas.is_some() {
                    self.first_frame_misses += 1;
                    if self.first_frame_misses >= REANCHOR_MISS_THRESHOLD {
                        if let Some(candidate) = reanchor_candidate {
                            if self.stats.frame_count == 1 {
                                // Stale first frame: nothing committed, rebuild.
                                self.reanchor_to(candidate);
                            } else {
                                // Mid-capture: PRESERVE the committed canvas.
                                self.reanchor_mid_capture(candidate);
                            }
                        }
                    }
                }
            } else if self.canvas.is_some() {
                self.first_frame_misses = 0;
            }
        }

        // Snapshot canvas state on every return path so the per-frame record
        // reflects the canvas at the moment this frame was processed, not just
        // for FirstFrame/Appended outcomes.
        self.snapshot_canvas_state();
        self.last_metrics.total_us = total_start.elapsed().as_micros() as u64;

        self.log_frame_outcome(&outcome);

        outcome
    }

    fn push_frame_inner(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.canvas.is_none() {
            let outcome = self.accept_first_frame(frame);
            self.last_metrics.outcome = StitchOutcomeKind::FirstFrame;
            return outcome;
        }

        let anchor = self
            .last_good
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            self.last_metrics
                .set_no_match(NoMatchReason::DimensionMismatch);
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::DimensionMismatch,
                best_estimate: None,
            };
        }

        let signature = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.duplicate_us);
            duplicate::signature(&frame)
        };
        if duplicate::is_duplicate(
            anchor.signature(),
            &signature,
            self.config.duplicate_threshold,
        ) {
            self.last_metrics.outcome = StitchOutcomeKind::Duplicate;
            return StitchOutcome::Duplicate;
        }

        let curr = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.prepare_frame_us);
            PreparedFrame::from_parts(frame, signature)
        };

        let mut metrics = std::mem::take(&mut self.last_metrics);
        let evaluation = self.evaluate_frame(anchor, &curr, &mut metrics, true);
        self.last_metrics = metrics;

        match evaluation {
            FrameEvaluation::Append {
                candidate,
                direction,
                overlap,
            } => {
                let slice_px = match direction {
                    AppendDirection::Bottom | AppendDirection::Top => candidate.dy.unsigned_abs(),
                    AppendDirection::Right | AppendDirection::Left => candidate.dx.unsigned_abs(),
                };
                let append_result = {
                    let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.append_us);
                    let canvas = self
                        .canvas
                        .as_mut()
                        .expect("canvas present after first frame");
                    canvas.append(direction, curr.rgba(), slice_px)
                };
                let added = match append_result {
                    Ok(n) => n,
                    Err(CanvasAppendError::AxisMismatch { locked, attempted }) => {
                        let estimate = MotionEstimate {
                            dx: candidate.dx,
                            dy: candidate.dy,
                            axis: attempted,
                            direction,
                            confidence: candidate.score,
                            method: candidate.method,
                            overlap,
                            inliers: candidate.inliers,
                            raw_matches: candidate.raw_matches,
                        };
                        self.last_metrics.outcome = StitchOutcomeKind::AxisChanged;
                        return StitchOutcome::AxisChanged {
                            previous_axis: locked,
                            new_axis: attempted,
                            estimate,
                        };
                    }
                    Err(CanvasAppendError::DimensionMismatch { .. }) => {
                        self.last_metrics
                            .set_no_match(NoMatchReason::DimensionMismatch);
                        return StitchOutcome::NoMatch {
                            reason: NoMatchReason::DimensionMismatch,
                            best_estimate: build_estimate(
                                anchor.rgba(),
                                curr.rgba(),
                                &candidate,
                                self.config.axis_ratio_threshold,
                            ),
                        };
                    }
                    Err(CanvasAppendError::EmptyAppend) => {
                        self.last_metrics.outcome = StitchOutcomeKind::NoProgress;
                        return StitchOutcome::NoProgress {
                            estimate: build_estimate(
                                anchor.rgba(),
                                curr.rgba(),
                                &candidate,
                                self.config.axis_ratio_threshold,
                            ),
                        };
                    }
                };
                let (canvas_height, canvas_width, append_copied_bytes) = {
                    let canvas = self
                        .canvas
                        .as_ref()
                        .expect("canvas present after first frame");
                    (
                        canvas.height(),
                        canvas.width(),
                        canvas.last_append_copied_bytes(),
                    )
                };
                self.last_metrics.append_copied_bytes = append_copied_bytes;

                self.locked_axis = Some(direction.axis());
                self.locked_direction = Some(direction);
                self.last_motion = (candidate.dx, candidate.dy);

                let estimate = MotionEstimate {
                    dx: candidate.dx,
                    dy: candidate.dy,
                    axis: direction.axis(),
                    direction,
                    confidence: candidate.score,
                    method: candidate.method,
                    overlap,
                    inliers: candidate.inliers,
                    raw_matches: candidate.raw_matches,
                };

                self.last_good = Some(curr);
                self.stats.frame_count += 1;
                self.stats.total_height = canvas_height;
                self.stats.total_width = canvas_width;
                self.stats.last_append = added;

                self.last_metrics.outcome = StitchOutcomeKind::Appended;

                StitchOutcome::Appended {
                    direction,
                    added,
                    estimate,
                }
            }
            FrameEvaluation::NoProgress { candidate } => {
                self.last_metrics.outcome = StitchOutcomeKind::NoProgress;
                StitchOutcome::NoProgress {
                    estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                }
            }
            FrameEvaluation::AxisChanged {
                candidate,
                new_axis,
                locked,
            } => {
                self.last_metrics.outcome = StitchOutcomeKind::AxisChanged;
                StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis,
                    estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    )
                    .expect("axis-change estimate must compute overlap"),
                }
            }
            FrameEvaluation::Reject { reason, candidate } => {
                self.last_metrics.set_no_match(reason);
                StitchOutcome::NoMatch {
                    reason,
                    best_estimate: candidate.and_then(|c| {
                        build_estimate(
                            anchor.rgba(),
                            curr.rgba(),
                            &c,
                            self.config.axis_ratio_threshold,
                        )
                    }),
                }
            }
        }
    }

    pub fn probe_recovery(&self, frame: &RgbaImage) -> RecoveryProbeResult {
        if self.canvas.is_none() || self.last_good.is_none() {
            return RecoveryProbeResult::Missed;
        }
        let anchor = self.last_good.as_ref().unwrap();

        if anchor.dimensions() != frame.dimensions() {
            return RecoveryProbeResult::Missed;
        }

        let signature = duplicate::signature(frame);
        if duplicate::is_duplicate(
            anchor.signature(),
            &signature,
            self.config.duplicate_threshold,
        ) {
            return RecoveryProbeResult::Recovered;
        }

        let curr = PreparedFrame::from_parts(frame.clone(), signature);

        let mut metrics = StitchMetrics::default();
        match self.evaluate_frame(anchor, &curr, &mut metrics, false) {
            FrameEvaluation::Append { .. } | FrameEvaluation::NoProgress { .. } => {
                RecoveryProbeResult::Recovered
            }
            FrameEvaluation::AxisChanged { .. } | FrameEvaluation::Reject { .. } => {
                RecoveryProbeResult::Missed
            }
        }
    }

    fn evaluate_frame(
        &self,
        anchor: &PreparedFrame,
        curr: &PreparedFrame,
        metrics: &mut StitchMetrics,
        enforce_direction_lock: bool,
    ) -> FrameEvaluation {
        let candidate = match estimate_motion(
            anchor,
            curr,
            self.locked_axis,
            self.last_motion,
            &self.config,
            metrics,
        ) {
            MotionSearchOutcome::Candidate(c) => c,
            MotionSearchOutcome::NoMatch {
                reason,
                best_candidate,
            } => {
                return FrameEvaluation::Reject {
                    reason,
                    candidate: best_candidate,
                };
            }
        };

        metrics.best_dx = candidate.dx;
        metrics.best_dy = candidate.dy;
        metrics.best_score = candidate.score;
        metrics.second_best_score = candidate.second_best_score;
        metrics.match_method = Some(candidate.method);

        if candidate.score > self.config.accept_confidence {
            return FrameEvaluation::Reject {
                reason: NoMatchReason::LowConfidence,
                candidate: Some(candidate),
            };
        }

        let direction = match self.classify_direction(&candidate) {
            DirectionResult::Direction(dir) => dir,
            DirectionResult::Ambiguous => {
                return FrameEvaluation::Reject {
                    reason: NoMatchReason::AmbiguousAxis,
                    candidate: Some(candidate),
                };
            }
            DirectionResult::CrossAxisTooLarge => {
                return FrameEvaluation::Reject {
                    reason: NoMatchReason::CrossAxisTooLarge,
                    candidate: Some(candidate),
                };
            }
            DirectionResult::AxisChanged { new_axis, locked } => {
                return FrameEvaluation::AxisChanged {
                    candidate,
                    new_axis,
                    locked,
                };
            }
        };

        if enforce_direction_lock {
            if let Some(locked_dir) = self.locked_direction {
                if direction != locked_dir {
                    return FrameEvaluation::Reject {
                        reason: NoMatchReason::ReverseDirection,
                        candidate: Some(candidate),
                    };
                }
            }
        }

        let slice_px = match direction {
            AppendDirection::Bottom | AppendDirection::Top => candidate.dy.unsigned_abs(),
            AppendDirection::Right | AppendDirection::Left => candidate.dx.unsigned_abs(),
        };
        if slice_px < self.config.min_append {
            return FrameEvaluation::NoProgress { candidate };
        }

        let verifier = PixelOverlapVerifier::new(&self.config.verifier, self.config.min_overlap);
        let overlap_region = {
            let _t = crate::metrics::ScopedTimer::new(&mut metrics.verifier_us);
            match verifier.verify(anchor.rgba(), curr.rgba(), &candidate) {
                VerifierOutcome::Pass { overlap, .. } => overlap,
                VerifierOutcome::InsufficientOverlap => {
                    return FrameEvaluation::Reject {
                        reason: NoMatchReason::InsufficientOverlap,
                        candidate: Some(candidate),
                    };
                }
                VerifierOutcome::OverlapDisagreement { .. } => {
                    return FrameEvaluation::Reject {
                        reason: NoMatchReason::OverlapVerificationFailed,
                        candidate: Some(candidate),
                    };
                }
            }
        };

        FrameEvaluation::Append {
            candidate,
            direction,
            overlap: overlap_region,
        }
    }

    pub fn full_image(&mut self) -> Option<&RgbaImage> {
        self.canvas.as_mut().map(|c| c.image())
    }

    pub fn canvas_viewport(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Option<crate::canvas::CanvasViewport> {
        self.canvas
            .as_ref()
            .and_then(|canvas| canvas.viewport(x, y, width, height))
    }

    pub fn stats(&self) -> StitchStats {
        self.stats
    }

    /// Per-frame instrumentation snapshot from the most recent push_frame call.
    /// Reset to defaults at the start of each push_frame; populated as stages run.
    pub fn last_metrics(&self) -> &StitchMetrics {
        &self.last_metrics
    }

    fn accept_first_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        let height = frame.height();
        let width = frame.width();
        self.stats = StitchStats {
            frame_count: 1,
            total_height: height,
            total_width: width,
            last_append: height,
        };
        self.last_good = Some(PreparedFrame::new(frame.clone()));
        self.canvas = Some(StripCanvas::new(frame));
        self.first_frame_misses = 0;
        StitchOutcome::FirstFrame
    }

    /// Discard the stale first frame and adopt `frame` as a fresh anchor. Only
    /// reached while still stuck on the first frame (no successful append yet),
    /// so this resets the match state without losing committed canvas content.
    fn reanchor_to(&mut self, frame: RgbaImage) {
        self.accept_first_frame(frame);
        self.last_motion = (0, 0);
        self.locked_axis = None;
        self.locked_direction = None;
    }

    /// Mid-capture re-anchor: the committed canvas is real content and MUST be
    /// kept. Only reset the match anchor (`last_good`) to `frame` and clear the
    /// motion/axis lock so the next frame matches fresh content. A content gap
    /// is accepted and logged. Last-resort floor beneath the robust verifier
    /// and feature consensus.
    fn reanchor_mid_capture(&mut self, frame: RgbaImage) {
        tracing::warn!(
            target: crate::diagnostics::TARGET_STITCH,
            miss_count = self.first_frame_misses,
            canvas_height = self.stats.total_height,
            "mid-capture re-anchor; a content gap may appear"
        );
        self.last_good = Some(PreparedFrame::new(frame));
        self.last_motion = (0, 0);
        self.locked_axis = None;
        self.locked_direction = None;
        self.first_frame_misses = 0;
    }

    fn snapshot_canvas_state(&mut self) {
        if let Some(canvas) = &self.canvas {
            self.last_metrics.canvas_logical_pixels = canvas.logical_pixels();
            self.last_metrics.canvas_allocated_bytes = canvas.allocated_bytes();
        }
    }

    fn log_frame_outcome(&self, outcome: &StitchOutcome) {
        let metrics = &self.last_metrics;
        tracing::trace!(
            target: crate::diagnostics::TARGET_STITCH,
            frame_index = metrics.frame_index,
            outcome = ?metrics.outcome,
            no_match_reason = ?metrics.no_match_reason,
            total_us = metrics.total_us,
            best_dx = metrics.best_dx,
            best_dy = metrics.best_dy,
            best_score = metrics.best_score,
            second_best_score = ?metrics.second_best_score,
            match_method = ?metrics.match_method,
            canvas_logical_pixels = metrics.canvas_logical_pixels,
            canvas_allocated_bytes = metrics.canvas_allocated_bytes,
            "processed stitch frame"
        );

        if matches!(outcome, StitchOutcome::NoMatch { .. }) {
            tracing::debug!(
                target: crate::diagnostics::TARGET_STITCH,
                frame_index = metrics.frame_index,
                reason = ?metrics.no_match_reason,
                total_us = metrics.total_us,
                "frame rejected"
            );
        } else if matches!(outcome, StitchOutcome::AxisChanged { .. }) {
            tracing::debug!(
                target: crate::diagnostics::TARGET_STITCH,
                frame_index = metrics.frame_index,
                outcome = ?metrics.outcome,
                total_us = metrics.total_us,
                "axis changed"
            );
        }
    }

    fn classify_direction(&self, candidate: &MotionCandidate) -> DirectionResult {
        match self.locked_axis {
            None => {
                match classify_axis(candidate.dx, candidate.dy, self.config.axis_ratio_threshold) {
                    AxisClassification::Vertical { direction }
                    | AxisClassification::Horizontal { direction } => {
                        DirectionResult::Direction(direction)
                    }
                    AxisClassification::Ambiguous => DirectionResult::Ambiguous,
                }
            }
            Some(locked) => match validate_with_lock(
                locked,
                candidate.dx,
                candidate.dy,
                self.config.max_cross_axis_px,
            ) {
                AxisValidation::OnAxis { direction } => DirectionResult::Direction(direction),
                AxisValidation::CrossAxisTooLarge => DirectionResult::CrossAxisTooLarge,
                AxisValidation::AxisChanged { new_axis } => {
                    DirectionResult::AxisChanged { new_axis, locked }
                }
            },
        }
    }
}

enum DirectionResult {
    Direction(AppendDirection),
    Ambiguous,
    CrossAxisTooLarge,
    AxisChanged {
        new_axis: ScrollAxis,
        locked: ScrollAxis,
    },
}

enum FrameEvaluation {
    Append {
        candidate: MotionCandidate,
        direction: AppendDirection,
        overlap: OverlapRegion,
    },
    NoProgress {
        candidate: MotionCandidate,
    },
    AxisChanged {
        candidate: MotionCandidate,
        new_axis: ScrollAxis,
        locked: ScrollAxis,
    },
    Reject {
        reason: NoMatchReason,
        candidate: Option<MotionCandidate>,
    },
}

fn build_estimate(
    prev: &RgbaImage,
    curr: &RgbaImage,
    candidate: &MotionCandidate,
    axis_ratio_threshold: f32,
) -> Option<MotionEstimate> {
    let overlap = compute_overlap(
        prev.width(),
        prev.height(),
        curr.width(),
        curr.height(),
        candidate.dx,
        candidate.dy,
    )?;
    let direction = match classify_axis(candidate.dx, candidate.dy, axis_ratio_threshold) {
        AxisClassification::Vertical { direction }
        | AxisClassification::Horizontal { direction } => direction,
        AxisClassification::Ambiguous => {
            if candidate.dy >= 0 {
                AppendDirection::Bottom
            } else {
                AppendDirection::Top
            }
        }
    };
    Some(MotionEstimate {
        dx: candidate.dx,
        dy: candidate.dy,
        axis: direction.axis(),
        direction,
        confidence: candidate.score,
        method: candidate.method,
        overlap,
        inliers: candidate.inliers,
        raw_matches: candidate.raw_matches,
    })
}
