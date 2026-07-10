//! Deterministic placement for agent-suggested Number Callout bubbles.
//!
//! Rollshot owns bubble layout, not the agent. Given the tip the agent
//! suggested, the image dimensions, and the bounds of already-committed
//! annotations, [`place_number_callout_bubble`] returns the bubble
//! center that:
//!
//! 1. Sits far enough from the tip to read the leader (offset
//!    `NUMBER_BUBBLE_RADIUS * 2.5`).
//! 2. Avoids overlapping a `2 * NUMBER_BUBBLE_RADIUS` square centered on
//!    the tip (the protected tip area).
//! 3. Avoids overlapping the axis-aligned bounds of committed annotations
//!    ([`annotation_bounds`]).
//! 4. Falls back to a clamped upper-right placement when no candidate's
//!    bubble can fit in the image.
//!
//! Candidate order is upper-right, upper-left, lower-right, lower-left.
//! Ties resolve to the first candidate in iteration order (upper-right),
//! so placement is stable across runs.

use crate::annotation::Annotation;
use crate::geometry::{ImagePoint, ImageRect};
use crate::shapes::annotation_bounds;
use crate::style::{NUMBER_BUBBLE_OUTLINE_WIDTH, NUMBER_BUBBLE_RADIUS};

/// Return the bubble center for an agent-suggested Number Callout tip.
///
/// The returned point is the bubble's center (matching
/// `Annotation::NumberCallout`'s `bubble` field and
/// `ImageDocument::add_number_callout`'s `bubble` parameter). The chosen
/// center is clamped to the image bounds so the bubble center stays
/// inside the frame even when no candidate's full bubble fits.
pub fn place_number_callout_bubble(
    tip: ImagePoint,
    image_width: u32,
    image_height: u32,
    annotations: &[Annotation],
) -> ImagePoint {
    let offset = NUMBER_BUBBLE_RADIUS * 2.5;
    let extent = NUMBER_BUBBLE_RADIUS + NUMBER_BUBBLE_OUTLINE_WIDTH;
    let tip_protection = NUMBER_BUBBLE_RADIUS;

    let protected = ImageRect {
        x: tip.x - tip_protection,
        y: tip.y - tip_protection,
        width: tip_protection * 2.0,
        height: tip_protection * 2.0,
    };

    let candidates = [
        ImagePoint::new(tip.x + offset, tip.y - offset),
        ImagePoint::new(tip.x - offset, tip.y - offset),
        ImagePoint::new(tip.x + offset, tip.y + offset),
        ImagePoint::new(tip.x - offset, tip.y + offset),
    ];

    let mut best_index = 0_usize;
    let mut best_score = f32::INFINITY;
    for (index, center) in candidates.iter().enumerate() {
        let bounds = bubble_bounds(*center, extent);
        let mut score = rect_overlap_area(bounds, protected);
        for annotation in annotations {
            score += rect_overlap_area(bounds, annotation_bounds(annotation));
        }
        if score < best_score {
            best_score = score;
            best_index = index;
        }
    }

    clamp_center_to_image(candidates[best_index], extent, image_width, image_height)
}

fn bubble_bounds(center: ImagePoint, extent: f32) -> ImageRect {
    ImageRect {
        x: center.x - extent,
        y: center.y - extent,
        width: extent * 2.0,
        height: extent * 2.0,
    }
}

fn rect_overlap_area(a: ImageRect, b: ImageRect) -> f32 {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    if x1 > x0 && y1 > y0 {
        (x1 - x0) * (y1 - y0)
    } else {
        0.0
    }
}

fn clamp_center_to_image(center: ImagePoint, extent: f32, width: u32, height: u32) -> ImagePoint {
    let w = width as f32;
    let h = height as f32;
    let min_x = extent.min(w * 0.5);
    let max_x = (w - extent).max(w * 0.5);
    let min_y = extent.min(h * 0.5);
    let max_y = (h - extent).max(h * 0.5);
    ImagePoint {
        x: center.x.clamp(min_x, max_x),
        y: center.y.clamp(min_y, max_y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId};
    use crate::style;

    fn offset() -> f32 {
        style::NUMBER_BUBBLE_RADIUS * 2.5
    }

    fn extent() -> f32 {
        style::NUMBER_BUBBLE_RADIUS + style::NUMBER_BUBBLE_OUTLINE_WIDTH
    }

    fn number_callout(tip: ImagePoint, bubble: ImagePoint) -> Annotation {
        Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip,
            bubble,
        }
    }

    #[test]
    fn center_in_empty_image_places_upper_right() {
        let tip = ImagePoint::new(500.0, 500.0);
        let result = place_number_callout_bubble(tip, 1000, 1000, &[]);
        let off = offset();
        // UR candidate center; no clamping needed because the bubble fits.
        assert!(
            result.x > tip.x,
            "bubble center should be right of the tip, got {result:?}"
        );
        assert!(
            result.y < tip.y,
            "bubble center should be above the tip, got {result:?}"
        );
        assert_eq!(result, ImagePoint::new(tip.x + off, tip.y - off));
    }

    #[test]
    fn tip_near_top_left_clamps_to_upper_right_within_image() {
        let tip = ImagePoint::new(50.0, 50.0);
        let result = place_number_callout_bubble(tip, 1000, 1000, &[]);
        let off = offset();
        let ext = extent();
        // UR wins ties; the bubble is offset upper-right of the tip and
        // clamped so its center sits at least `extent` inside the image.
        assert_eq!(result.x, tip.x + off);
        assert!((result.y - ext).abs() < f32::EPSILON);
    }

    #[test]
    fn tip_near_top_right_keeps_upper_right_quadrant() {
        let tip = ImagePoint::new(950.0, 50.0);
        let result = place_number_callout_bubble(tip, 1000, 1000, &[]);
        assert!(
            result.x > tip.x,
            "bubble center should be right of the tip, got {result:?}"
        );
        assert!(
            result.y < tip.y,
            "bubble center should be above the tip, got {result:?}"
        );
        let ext = extent();
        assert!(result.x <= 1000.0 - ext);
        assert!(result.y >= ext);
    }

    #[test]
    fn tip_near_bottom_left_keeps_upper_right_quadrant() {
        let tip = ImagePoint::new(50.0, 950.0);
        let result = place_number_callout_bubble(tip, 1000, 1000, &[]);
        assert!(result.x > tip.x, "got {result:?}");
        assert!(result.y < tip.y, "got {result:?}");
    }

    #[test]
    fn tip_near_bottom_right_keeps_upper_right_quadrant() {
        let tip = ImagePoint::new(950.0, 950.0);
        let result = place_number_callout_bubble(tip, 1000, 1000, &[]);
        assert!(result.x > tip.x, "got {result:?}");
        assert!(result.y < tip.y, "got {result:?}");
    }

    #[test]
    fn existing_upper_right_callout_pushes_new_bubble_to_upper_left() {
        let tip = ImagePoint::new(500.0, 500.0);
        // Existing callout placed in the upper-right area relative to the
        // new tip, close enough that the UR candidate's bubble overlaps
        // it. The other three candidates (UL, LR, LL) have zero overlap,
        // so UL wins the tie by being first in iteration order.
        let existing = number_callout(ImagePoint::new(530.0, 470.0), ImagePoint::new(550.0, 450.0));
        let result = place_number_callout_bubble(tip, 1000, 1000, &[existing]);
        assert!(
            result.x < tip.x,
            "bubble should be left of the tip, got {result:?}"
        );
        assert!(
            result.y < tip.y,
            "bubble should be above the tip, got {result:?}"
        );
    }

    #[test]
    fn symmetric_overlap_picks_upper_right_by_tiebreak() {
        let tip = ImagePoint::new(500.0, 500.0);
        // Two existing callouts, mirrored across the vertical axis, so
        // UR and UL overlap each by the same area.
        let left = number_callout(ImagePoint::new(200.0, 200.0), ImagePoint::new(220.0, 180.0));
        let right = number_callout(ImagePoint::new(800.0, 200.0), ImagePoint::new(780.0, 180.0));
        let result = place_number_callout_bubble(tip, 1000, 1000, &[left, right]);
        assert!(result.x > tip.x, "UR should win the tie, got {result:?}");
        assert!(result.y < tip.y, "UR should win the tie, got {result:?}");
    }

    #[test]
    fn tiny_image_clamps_center_inside_bounds() {
        // Image smaller than 2 * bubble extent: no candidate's bubble
        // fits, so the upper-right candidate is clamped to the image
        // bounds and the returned center stays inside the frame.
        let tip = ImagePoint::new(5.0, 5.0);
        let result = place_number_callout_bubble(tip, 10, 10, &[]);
        assert!(result.x >= 0.0 && result.x <= 10.0, "got {result:?}");
        assert!(result.y >= 0.0 && result.y <= 10.0, "got {result:?}");
    }
}
