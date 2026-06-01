use image::RgbaImage;

use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::canvas::{CanvasAppendError, StripCanvas};
use crate::duplicate;
use crate::matcher::{estimate_motion, MotionSearchOutcome, PreparedFrame};
use crate::metrics::{StitchMetrics, StitchOutcomeKind};
use crate::overlap::compute_overlap;
use crate::types::{
    AppendDirection, MotionCandidate, MotionEstimate, NoMatchReason, ScrollAxis, StitchConfig,
    StitchOutcome, StitchStats,
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
}

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
        }
    }

    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        let total_start = std::time::Instant::now();
        self.last_metrics = StitchMetrics::default();
        self.last_metrics.frame_index = self.frame_counter;
        self.frame_counter += 1;
        let outcome = self.push_frame_inner(frame);
        // Snapshot canvas state on every return path so the per-frame record
        // reflects the canvas at the moment this frame was processed, not just
        // for FirstFrame/Appended outcomes.
        self.snapshot_canvas_state();
        self.last_metrics.total_us = total_start.elapsed().as_micros() as u64;
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

        let candidate = match estimate_motion(
            anchor,
            &curr,
            self.locked_axis,
            self.last_motion,
            &self.config,
            &mut self.last_metrics,
        ) {
            MotionSearchOutcome::Candidate(c) => c,
            MotionSearchOutcome::NoMatch {
                reason,
                best_candidate,
            } => {
                let best_estimate = best_candidate.and_then(|candidate| {
                    build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    )
                });
                self.last_metrics.set_no_match(reason);
                return StitchOutcome::NoMatch {
                    reason,
                    best_estimate,
                };
            }
        };

        self.last_metrics.best_dx = candidate.dx;
        self.last_metrics.best_dy = candidate.dy;
        self.last_metrics.best_score = candidate.score;
        self.last_metrics.second_best_score = candidate.second_best_score;
        self.last_metrics.match_method = Some(candidate.method);

        if candidate.score > self.config.accept_confidence {
            self.last_metrics.set_no_match(NoMatchReason::LowConfidence);
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::LowConfidence,
                best_estimate: build_estimate(
                    anchor.rgba(),
                    curr.rgba(),
                    &candidate,
                    self.config.axis_ratio_threshold,
                ),
            };
        }

        let direction = match self.classify_direction(&candidate) {
            DirectionResult::Direction(dir) => dir,
            DirectionResult::Ambiguous => {
                self.last_metrics.set_no_match(NoMatchReason::AmbiguousAxis);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::AmbiguousAxis,
                    best_estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::CrossAxisTooLarge => {
                self.last_metrics
                    .set_no_match(NoMatchReason::CrossAxisTooLarge);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::CrossAxisTooLarge,
                    best_estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::AxisChanged { new_axis, locked } => {
                let estimate = build_estimate(
                    anchor.rgba(),
                    curr.rgba(),
                    &candidate,
                    self.config.axis_ratio_threshold,
                )
                .expect("axis-change estimate must compute overlap");
                self.last_metrics.outcome = StitchOutcomeKind::AxisChanged;
                return StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis,
                    estimate,
                };
            }
        };

        if let Some(locked_dir) = self.locked_direction {
            if direction != locked_dir {
                self.last_metrics
                    .set_no_match(NoMatchReason::ReverseDirection);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::ReverseDirection,
                    best_estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
        }

        let slice_px = match direction {
            AppendDirection::Bottom | AppendDirection::Top => candidate.dy.unsigned_abs(),
            AppendDirection::Right | AppendDirection::Left => candidate.dx.unsigned_abs(),
        };
        if slice_px < self.config.min_append {
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

        let verifier = PixelOverlapVerifier::new(&self.config.verifier, self.config.min_overlap);
        let (overlap_region, _verifier_score) = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.verifier_us);
            match verifier.verify(anchor.rgba(), curr.rgba(), &candidate) {
                VerifierOutcome::Pass { overlap, score } => (overlap, score),
                VerifierOutcome::InsufficientOverlap => {
                    drop(_t);
                    self.last_metrics
                        .set_no_match(NoMatchReason::InsufficientOverlap);
                    return StitchOutcome::NoMatch {
                        reason: NoMatchReason::InsufficientOverlap,
                        best_estimate: build_estimate(
                            anchor.rgba(),
                            curr.rgba(),
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
                VerifierOutcome::OverlapDisagreement { .. } => {
                    drop(_t);
                    self.last_metrics
                        .set_no_match(NoMatchReason::OverlapVerificationFailed);
                    return StitchOutcome::NoMatch {
                        reason: NoMatchReason::OverlapVerificationFailed,
                        best_estimate: build_estimate(
                            anchor.rgba(),
                            curr.rgba(),
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
            }
        };

        // Run the append in its own scope so the canvas borrow (and the append
        // timer) are released before the error match. That lets the error arms
        // borrow `anchor`/`self.last_metrics` again and compute `build_estimate`
        // lazily — only on the rejection paths that actually need it.
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
                    overlap: overlap_region,
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
            overlap: overlap_region,
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
        StitchOutcome::FirstFrame
    }

    fn snapshot_canvas_state(&mut self) {
        if let Some(canvas) = &self.canvas {
            self.last_metrics.canvas_logical_pixels = canvas.logical_pixels();
            self.last_metrics.canvas_allocated_bytes = canvas.allocated_bytes();
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
