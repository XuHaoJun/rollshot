//! Axis detection and single-axis lock validation.

use crate::types::{AppendDirection, ScrollAxis};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisClassification {
    Vertical { direction: AppendDirection },
    Horizontal { direction: AppendDirection },
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisValidation {
    /// Candidate matches the locked axis and stays within `max_cross_axis_px`.
    OnAxis { direction: AppendDirection },
    /// Cross-axis movement exceeded `max_cross_axis_px` while a locked axis was set.
    CrossAxisTooLarge,
    /// Candidate is reliable on the opposite axis from the lock.
    AxisChanged { new_axis: ScrollAxis },
}

/// Classifies a `(dx, dy)` candidate using the rollshot axis-ratio rule.
pub fn classify_axis(dx: i32, dy: i32, ratio_threshold: f32) -> AxisClassification {
    let adx = dx.unsigned_abs() as f32;
    let ady = dy.unsigned_abs() as f32;

    if adx == 0.0 && ady == 0.0 {
        return AxisClassification::Ambiguous;
    }

    if ady > adx * ratio_threshold {
        let direction = if dy >= 0 {
            AppendDirection::Bottom
        } else {
            AppendDirection::Top
        };
        return AxisClassification::Vertical { direction };
    }

    if adx > ady * ratio_threshold {
        let direction = if dx >= 0 {
            AppendDirection::Right
        } else {
            AppendDirection::Left
        };
        return AxisClassification::Horizontal { direction };
    }

    AxisClassification::Ambiguous
}

/// Validates a candidate against a locked axis. Cross-axis movement above the
/// tolerance is treated as a real axis change rather than noise.
pub fn validate_with_lock(
    locked: ScrollAxis,
    dx: i32,
    dy: i32,
    max_cross_axis_px: i32,
) -> AxisValidation {
    let cross = match locked {
        ScrollAxis::Vertical => dx.abs(),
        ScrollAxis::Horizontal => dy.abs(),
    };

    if cross <= max_cross_axis_px {
        let direction = match locked {
            ScrollAxis::Vertical => {
                if dy >= 0 {
                    AppendDirection::Bottom
                } else {
                    AppendDirection::Top
                }
            }
            ScrollAxis::Horizontal => {
                if dx >= 0 {
                    AppendDirection::Right
                } else {
                    AppendDirection::Left
                }
            }
        };
        return AxisValidation::OnAxis { direction };
    }

    let main = match locked {
        ScrollAxis::Vertical => dy.abs(),
        ScrollAxis::Horizontal => dx.abs(),
    };

    if cross > main {
        let new_axis = match locked {
            ScrollAxis::Vertical => ScrollAxis::Horizontal,
            ScrollAxis::Horizontal => ScrollAxis::Vertical,
        };
        AxisValidation::AxisChanged { new_axis }
    } else {
        AxisValidation::CrossAxisTooLarge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_down_motion_classifies_bottom() {
        assert_eq!(
            classify_axis(0, 30, 1.5),
            AxisClassification::Vertical {
                direction: AppendDirection::Bottom
            }
        );
    }

    #[test]
    fn vertical_up_motion_classifies_top() {
        assert_eq!(
            classify_axis(0, -30, 1.5),
            AxisClassification::Vertical {
                direction: AppendDirection::Top
            }
        );
    }

    #[test]
    fn horizontal_right_motion_classifies_right() {
        assert_eq!(
            classify_axis(40, 0, 1.5),
            AxisClassification::Horizontal {
                direction: AppendDirection::Right
            }
        );
    }

    #[test]
    fn horizontal_left_motion_classifies_left() {
        assert_eq!(
            classify_axis(-40, 0, 1.5),
            AxisClassification::Horizontal {
                direction: AppendDirection::Left
            }
        );
    }

    #[test]
    fn diagonal_motion_within_ratio_is_ambiguous() {
        assert_eq!(
            classify_axis(20, 25, 1.5),
            AxisClassification::Ambiguous
        );
    }

    #[test]
    fn zero_motion_is_ambiguous() {
        assert_eq!(classify_axis(0, 0, 1.5), AxisClassification::Ambiguous);
    }

    #[test]
    fn vertical_lock_accepts_small_cross_axis() {
        let v = validate_with_lock(ScrollAxis::Vertical, 3, 40, 6);
        assert_eq!(
            v,
            AxisValidation::OnAxis {
                direction: AppendDirection::Bottom
            }
        );
    }

    #[test]
    fn vertical_lock_flags_too_large_cross_axis_as_noise() {
        let v = validate_with_lock(ScrollAxis::Vertical, 12, 40, 6);
        assert_eq!(v, AxisValidation::CrossAxisTooLarge);
    }

    #[test]
    fn vertical_lock_reports_axis_change_when_horizontal_dominates() {
        let v = validate_with_lock(ScrollAxis::Vertical, 60, 4, 6);
        assert_eq!(
            v,
            AxisValidation::AxisChanged {
                new_axis: ScrollAxis::Horizontal
            }
        );
    }

    #[test]
    fn horizontal_lock_accepts_left_motion() {
        let v = validate_with_lock(ScrollAxis::Horizontal, -30, 2, 6);
        assert_eq!(
            v,
            AxisValidation::OnAxis {
                direction: AppendDirection::Left
            }
        );
    }
}
