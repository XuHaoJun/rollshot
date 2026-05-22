use image::RgbaImage;

use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::canvas::{CanvasAppendError, LinearCanvas};
use crate::duplicate;
use crate::matcher::{estimate_motion, MotionSearchOutcome};
use crate::overlap::compute_overlap;
use crate::types::{
    AppendDirection, MotionCandidate, MotionEstimate, NoMatchReason, ScrollAxis, StitchConfig,
    StitchOutcome, StitchStats,
};
use crate::static_region::StaticRegionDetector;
use crate::verifier::{PixelOverlapVerifier, VerifierOutcome};

pub struct Stitcher {
    config: StitchConfig,
    canvas: Option<LinearCanvas>,
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
    last_motion: (i32, i32),
    locked_axis: Option<ScrollAxis>,
    stats: StitchStats,
    static_detector: StaticRegionDetector,
}

impl Stitcher {
    pub fn new(config: StitchConfig) -> Self {
        let static_detector = StaticRegionDetector::new(config.static_region.clone());
        Self {
            config,
            canvas: None,
            last_good_frame: None,
            last_good_signature: None,
            last_motion: (0, 0),
            locked_axis: None,
            stats: StitchStats::default(),
            static_detector,
        }
    }

    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.canvas.is_none() {
            return self.accept_first_frame(frame);
        }

        let anchor = self
            .last_good_frame
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::DimensionMismatch,
                best_estimate: None,
            };
        }

        let signature = duplicate::signature(&frame);
        if let Some(prev_sig) = self.last_good_signature.as_ref() {
            if duplicate::is_duplicate(prev_sig, &signature, self.config.duplicate_threshold) {
                return StitchOutcome::Duplicate;
            }
        }

        let candidate = match estimate_motion(
            anchor,
            &frame,
            self.locked_axis,
            self.last_motion,
            &self.config,
        ) {
            MotionSearchOutcome::Candidate(c) => c,
            MotionSearchOutcome::NoMatch {
                reason,
                best_candidate,
            } => {
                let best_estimate = best_candidate.and_then(|candidate| {
                    build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold)
                });
                return StitchOutcome::NoMatch {
                    reason,
                    best_estimate,
                };
            }
        };

        if candidate.score > self.config.accept_confidence {
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::LowConfidence,
                best_estimate: build_estimate(
                    anchor,
                    &frame,
                    &candidate,
                    self.config.axis_ratio_threshold,
                ),
            };
        }

        let direction = match self.classify_direction(&candidate) {
            DirectionResult::Direction(dir) => dir,
            DirectionResult::Ambiguous => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::AmbiguousAxis,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::CrossAxisTooLarge => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::CrossAxisTooLarge,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::AxisChanged { new_axis, locked } => {
                let estimate =
                    build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold)
                        .expect("axis-change estimate must compute overlap");
                return StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis,
                    estimate,
                };
            }
        };

        let slice_px = match direction {
            AppendDirection::Bottom | AppendDirection::Top => candidate.dy.unsigned_abs(),
            AppendDirection::Right | AppendDirection::Left => candidate.dx.unsigned_abs(),
        };
        if slice_px < self.config.min_append {
            return StitchOutcome::NoProgress {
                estimate: build_estimate(
                    anchor,
                    &frame,
                    &candidate,
                    self.config.axis_ratio_threshold,
                ),
            };
        }

        let verifier = PixelOverlapVerifier::new(&self.config.verifier, self.config.min_overlap);
        let (overlap_region, _verifier_score) = match verifier.verify(anchor, &frame, &candidate) {
            VerifierOutcome::Pass { overlap, score } => (overlap, score),
            VerifierOutcome::InsufficientOverlap => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::InsufficientOverlap,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            VerifierOutcome::OverlapDisagreement { .. } => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::OverlapVerificationFailed,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
        };

        if self.config.static_region.enabled {
            self.static_detector.observe(anchor, &frame, candidate.dx, candidate.dy);
        }
        let mask = if self.config.static_region.enabled {
            self.static_detector.mask()
        } else {
            None
        };

        let canvas = self
            .canvas
            .as_mut()
            .expect("canvas present after first frame");
        let added = match canvas.append(direction, &frame, slice_px, mask) {
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
                return StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis: attempted,
                    estimate,
                };
            }
            Err(CanvasAppendError::DimensionMismatch { .. }) => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::DimensionMismatch,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            Err(CanvasAppendError::EmptyAppend) => {
                return StitchOutcome::NoProgress {
                    estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
        };

        self.locked_axis = Some(direction.axis());
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

        self.last_good_signature = Some(signature);
        self.last_good_frame = Some(frame);
        self.stats.frame_count += 1;
        self.stats.total_height = canvas.height();
        self.stats.total_width = canvas.width();
        self.stats.last_append = added;

        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        }
    }

    pub fn full_image(&self) -> Option<&RgbaImage> {
        self.canvas.as_ref().map(LinearCanvas::image)
    }

    pub fn stats(&self) -> StitchStats {
        self.stats
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
        self.last_good_signature = Some(duplicate::signature(&frame));
        self.last_good_frame = Some(frame.clone());
        self.canvas = Some(LinearCanvas::new(frame));
        StitchOutcome::FirstFrame
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
