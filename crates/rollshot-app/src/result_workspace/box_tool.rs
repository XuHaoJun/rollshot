//! Pure creation, threshold, movement, crossing resize, minimum-size, and
//! Shift-resize geometry for box-shaped annotations (Rectangle, Ellipse).

use rollshot_image_document::{ImagePoint, ImageRect, ResizeHandle};

/// Minimum screen pixels for a creation gesture to be committed.
pub(crate) const MIN_BOX_SCREEN: f32 = 4.0;

/// Compute the normalized creation bounds from an anchor and raw pointer.
///
/// The raw pointer is clamped to source bounds first. With Shift held, both
/// absolute deltas are replaced by their minimum while preserving each sign.
/// The result is normalized via `ImageRect::from_corners`.
pub(crate) fn creation_bounds(
    anchor: ImagePoint,
    raw_pointer: ImagePoint,
    shift: bool,
    source_width: u32,
    source_height: u32,
) -> ImageRect {
    let p = raw_pointer.clamp_to(source_width, source_height);
    let dx = p.x - anchor.x;
    let dy = p.y - anchor.y;
    let (adj_dx, adj_dy) = if shift {
        let min_delta = dx.abs().min(dy.abs());
        (min_delta * dx.signum(), min_delta * dy.signum())
    } else {
        (dx, dy)
    };
    let end = ImagePoint::new(
        (anchor.x + adj_dx).clamp(0.0, source_width as f32),
        (anchor.y + adj_dy).clamp(0.0, source_height as f32),
    );
    ImageRect::from_corners(anchor, end)
}

/// Whether the creation bounds meet the 4-screen-pixel threshold on both axes.
pub(crate) fn meets_creation_threshold(bounds: ImageRect, scale: f32) -> bool {
    scale > 0.0
        && scale.is_finite()
        && bounds.width * scale >= MIN_BOX_SCREEN
        && bounds.height * scale >= MIN_BOX_SCREEN
}

/// Move a box-shaped annotation by pointer, preserving size and clamping to source.
pub(crate) fn moved_bounds(
    original: ImageRect,
    pointer: ImagePoint,
    grab_offset: ImagePoint,
    source_width: u32,
    source_height: u32,
) -> ImageRect {
    let new_x =
        (pointer.x - grab_offset.x).clamp(0.0, (source_width as f32 - original.width).max(0.0));
    let new_y =
        (pointer.y - grab_offset.y).clamp(0.0, (source_height as f32 - original.height).max(0.0));
    ImageRect {
        x: new_x,
        y: new_y,
        width: original.width,
        height: original.height,
    }
}

/// Eight-direction resize with crossing support, Shift aspect-ratio locking
/// for corners, minimum-size enforcement, and source-bounds clamping.
pub(crate) fn resized_bounds(
    original: ImageRect,
    handle: ResizeHandle,
    raw_pointer: ImagePoint,
    shift: bool,
    scale: f32,
    source_width: u32,
    source_height: u32,
) -> ImageRect {
    use ResizeHandle::*;

    let left = original.x;
    let top = original.y;
    let right = original.x + original.width;
    let bottom = original.y + original.height;

    let min = if scale > 0.0 && scale.is_finite() {
        MIN_BOX_SCREEN / scale
    } else {
        0.0
    };
    let p = raw_pointer.clamp_to(source_width, source_height);

    let (l, t, r, b) = match handle {
        // Edge handles: replace one coordinate, ignore Shift.
        Top => {
            let new_t = enforce_min(p.y, bottom, min, 0.0, source_height as f32);
            (left, new_t, right, bottom)
        }
        Right => {
            let new_r = enforce_min(p.x, left, min, 0.0, source_width as f32);
            (left, top, new_r, bottom)
        }
        Bottom => {
            let new_b = enforce_min(p.y, top, min, 0.0, source_height as f32);
            (left, top, right, new_b)
        }
        Left => {
            let new_l = enforce_min(p.x, right, min, 0.0, source_width as f32);
            (new_l, top, right, bottom)
        }
        // Corner handles: Shift locks aspect ratio.
        TopLeft => {
            if shift && original.width > 0.0 && original.height > 0.0 {
                let (mx, my) = shift_corner(p, right, bottom, original, min);
                (
                    mx.clamp(0.0, source_width as f32),
                    my.clamp(0.0, source_height as f32),
                    right,
                    bottom,
                )
            } else {
                (
                    enforce_min(p.x, right, min, 0.0, source_width as f32),
                    enforce_min(p.y, bottom, min, 0.0, source_height as f32),
                    right,
                    bottom,
                )
            }
        }
        TopRight => {
            if shift && original.width > 0.0 && original.height > 0.0 {
                let (mx, my) = shift_corner(p, left, bottom, original, min);
                (
                    left,
                    my.clamp(0.0, source_height as f32),
                    mx.clamp(0.0, source_width as f32),
                    bottom,
                )
            } else {
                (
                    left,
                    enforce_min(p.y, bottom, min, 0.0, source_height as f32),
                    enforce_min(p.x, left, min, 0.0, source_width as f32),
                    bottom,
                )
            }
        }
        BottomRight => {
            if shift && original.width > 0.0 && original.height > 0.0 {
                let (mx, my) = shift_corner(p, left, top, original, min);
                (
                    left,
                    top,
                    mx.clamp(0.0, source_width as f32),
                    my.clamp(0.0, source_height as f32),
                )
            } else {
                (
                    left,
                    top,
                    enforce_min(p.x, left, min, 0.0, source_width as f32),
                    enforce_min(p.y, top, min, 0.0, source_height as f32),
                )
            }
        }
        BottomLeft => {
            if shift && original.width > 0.0 && original.height > 0.0 {
                let (mx, my) = shift_corner(p, right, top, original, min);
                (
                    mx.clamp(0.0, source_width as f32),
                    top,
                    right,
                    my.clamp(0.0, source_height as f32),
                )
            } else {
                (
                    enforce_min(p.x, right, min, 0.0, source_width as f32),
                    top,
                    right,
                    enforce_min(p.y, top, min, 0.0, source_height as f32),
                )
            }
        }
    };

    ImageRect::from_corners(ImagePoint::new(l, t), ImagePoint::new(r, b))
}

/// Enforce minimum distance between `moving` and `fixed` on one axis.
/// `sign` is chosen from the raw-pointer direction relative to the fixed edge.
fn enforce_min(moving: f32, fixed: f32, min: f32, lo: f32, hi: f32) -> f32 {
    let d = moving - fixed;
    let pushed = if d.abs() < min {
        if d >= 0.0 {
            fixed + min
        } else {
            fixed - min
        }
    } else {
        moving
    };
    pushed.clamp(lo, hi)
}

/// Corner Shift: uniform scale from raw absolute deltas against original
/// dimensions, applying the smaller scale to preserve the original ratio.
fn shift_corner(
    p: ImagePoint,
    fixed_x: f32,
    fixed_y: f32,
    original: ImageRect,
    min: f32,
) -> (f32, f32) {
    let dx = (fixed_x - p.x).abs();
    let dy = (fixed_y - p.y).abs();
    let s = (dx / original.width).min(dy / original.height);

    let sign_x = if p.x <= fixed_x { 1.0 } else { -1.0 };
    let sign_y = if p.y <= fixed_y { 1.0 } else { -1.0 };

    let mx = fixed_x - sign_x * s * original.width;
    let my = fixed_y - sign_y * s * original.height;

    let mx = if (mx - fixed_x).abs() < min {
        if mx <= fixed_x {
            fixed_x - min
        } else {
            fixed_x + min
        }
    } else {
        mx
    };
    let my = if (my - fixed_y).abs() < min {
        if my <= fixed_y {
            fixed_y - min
        } else {
            fixed_y + min
        }
    } else {
        my
    };

    (mx, my)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- creation_bounds -----------------------------------------------------

    #[test]
    fn creation_all_four_quadrants() {
        let anchor = ImagePoint::new(50.0, 50.0);
        // NE
        let r = creation_bounds(anchor, ImagePoint::new(80.0, 30.0), false, 200, 200);
        assert_eq!(
            r,
            ImageRect::from_corners(anchor, ImagePoint::new(80.0, 30.0))
        );
        // SE
        let r = creation_bounds(anchor, ImagePoint::new(80.0, 70.0), false, 200, 200);
        assert_eq!(
            r,
            ImageRect::from_corners(anchor, ImagePoint::new(80.0, 70.0))
        );
        // SW
        let r = creation_bounds(anchor, ImagePoint::new(20.0, 70.0), false, 200, 200);
        assert_eq!(
            r,
            ImageRect::from_corners(anchor, ImagePoint::new(20.0, 70.0))
        );
        // NW
        let r = creation_bounds(anchor, ImagePoint::new(20.0, 30.0), false, 200, 200);
        assert_eq!(
            r,
            ImageRect::from_corners(anchor, ImagePoint::new(20.0, 30.0))
        );
    }

    #[test]
    fn creation_normalizes_inverted_drag() {
        let anchor = ImagePoint::new(50.0, 50.0);
        let r = creation_bounds(anchor, ImagePoint::new(30.0, 20.0), false, 200, 200);
        assert_eq!(r.x, 30.0);
        assert_eq!(r.y, 20.0);
        assert_eq!(r.width, 20.0);
        assert_eq!(r.height, 30.0);
    }

    #[test]
    fn creation_clamps_to_source_edges() {
        let anchor = ImagePoint::new(50.0, 50.0);
        let r = creation_bounds(anchor, ImagePoint::new(250.0, -10.0), false, 200, 200);
        // Clamped pointer: (200, 0)
        assert_eq!(
            r,
            ImageRect::from_corners(anchor, ImagePoint::new(200.0, 0.0))
        );
    }

    #[test]
    fn creation_shift_uses_smaller_axis_preserving_quadrant() {
        let anchor = ImagePoint::new(50.0, 50.0);
        // dx=30, dy=20 → min=20, both become 20 with original signs
        let r = creation_bounds(anchor, ImagePoint::new(80.0, 30.0), true, 200, 200);
        assert_eq!(
            r,
            ImageRect::from_corners(anchor, ImagePoint::new(70.0, 30.0))
        );
    }

    #[test]
    fn creation_shift_constrained_by_source_bounds() {
        let anchor = ImagePoint::new(90.0, 50.0);
        // dx=10, dy=60 → min=10; end = (100, 60) — clamped to source
        let r = creation_bounds(anchor, ImagePoint::new(100.0, 110.0), true, 100, 100);
        // raw clamped = (100, 100), dx=10, dy=50, min=10
        // end = (90+10, 90-10*sign) → need to check
        // Actually: anchor (90,50), clamped pointer (100,100)
        // dx=10, dy=50, min=10
        // adj: (10, 10) with signs (+, +)
        // end = (100, 60) clamped to (100, 60)
        assert_eq!(r.x, 90.0);
        assert_eq!(r.y, 50.0);
        assert_eq!(r.width, 10.0);
        assert_eq!(r.height, 10.0);
    }

    // -- meets_creation_threshold --------------------------------------------

    #[test]
    fn threshold_below_at_scale_1() {
        let bounds = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 3.9,
            height: 10.0,
        };
        assert!(!meets_creation_threshold(bounds, 1.0));
    }

    #[test]
    fn threshold_at_at_scale_1() {
        let bounds = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
        };
        assert!(meets_creation_threshold(bounds, 1.0));
    }

    #[test]
    fn threshold_above_at_scale_1() {
        let bounds = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(meets_creation_threshold(bounds, 1.0));
    }

    #[test]
    fn threshold_at_scale_025_needs_16_image_pixels() {
        let below = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 15.0,
            height: 16.0,
        };
        assert!(!meets_creation_threshold(below, 0.25));
        let at = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 16.0,
            height: 16.0,
        };
        assert!(meets_creation_threshold(at, 0.25));
    }

    #[test]
    fn threshold_at_scale_4_needs_1_image_pixel() {
        let below = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 0.9,
            height: 4.0,
        };
        assert!(!meets_creation_threshold(below, 4.0));
        let at = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        assert!(meets_creation_threshold(at, 4.0));
    }

    #[test]
    fn threshold_rejects_zero_or_negative_scale() {
        let bounds = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        assert!(!meets_creation_threshold(bounds, 0.0));
        assert!(!meets_creation_threshold(bounds, -1.0));
        assert!(!meets_creation_threshold(bounds, f32::NAN));
    }

    // -- moved_bounds --------------------------------------------------------

    #[test]
    fn move_preserves_body_size() {
        let orig = ImageRect {
            x: 10.0,
            y: 10.0,
            width: 30.0,
            height: 20.0,
        };
        let r = moved_bounds(
            orig,
            ImagePoint::new(60.0, 50.0),
            ImagePoint::new(5.0, 5.0),
            200,
            200,
        );
        assert_eq!(r.width, 30.0);
        assert_eq!(r.height, 20.0);
    }

    #[test]
    fn move_clamps_to_all_source_edges() {
        let orig = ImageRect {
            x: 10.0,
            y: 10.0,
            width: 30.0,
            height: 20.0,
        };
        // Push past right/bottom
        let r = moved_bounds(
            orig,
            ImagePoint::new(200.0, 200.0),
            ImagePoint::new(0.0, 0.0),
            100,
            100,
        );
        assert!(r.x + r.width <= 100.0);
        assert!(r.y + r.height <= 100.0);
        // Push past left/top
        let r = moved_bounds(
            orig,
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(30.0, 20.0),
            100,
            100,
        );
        assert!(r.x >= 0.0);
        assert!(r.y >= 0.0);
    }

    // -- resized_bounds: edge handles ----------------------------------------

    #[test]
    fn edge_handle_replaces_one_coordinate() {
        let orig = ImageRect {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 30.0,
        };
        let r = resized_bounds(
            orig,
            ResizeHandle::Right,
            ImagePoint::new(70.0, 99.0),
            false,
            1.0,
            200,
            200,
        );
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 10.0);
        assert_eq!(r.width, 60.0);
        assert_eq!(r.height, 30.0);
    }

    #[test]
    fn edge_handle_ignores_shift() {
        let orig = ImageRect {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 30.0,
        };
        let no_shift = resized_bounds(
            orig,
            ResizeHandle::Bottom,
            ImagePoint::new(99.0, 60.0),
            false,
            1.0,
            200,
            200,
        );
        let with_shift = resized_bounds(
            orig,
            ResizeHandle::Bottom,
            ImagePoint::new(99.0, 60.0),
            true,
            1.0,
            200,
            200,
        );
        assert_eq!(no_shift, with_shift);
    }

    // -- resized_bounds: all eight handles -----------------------------------

    #[test]
    fn each_handle_anchors_the_opposite_corner() {
        let orig = ImageRect {
            x: 20.0,
            y: 20.0,
            width: 60.0,
            height: 40.0,
        };
        // Use a point inside the rect so no crossing occurs.
        let p = ImagePoint::new(30.0, 30.0);
        let sw = 200u32;
        let sh = 200u32;

        // TopLeft → fixed (80, 60)
        let r = resized_bounds(orig, ResizeHandle::TopLeft, p, false, 1.0, sw, sh);
        assert_eq!(r.x + r.width, 80.0);
        assert_eq!(r.y + r.height, 60.0);

        // Top → fixed bottom=60
        let r = resized_bounds(orig, ResizeHandle::Top, p, false, 1.0, sw, sh);
        assert_eq!(r.y + r.height, 60.0);
        assert_eq!(r.x, 20.0);

        // TopRight → fixed (20, 60)
        let r = resized_bounds(orig, ResizeHandle::TopRight, p, false, 1.0, sw, sh);
        assert_eq!(r.x, 20.0);
        assert_eq!(r.y + r.height, 60.0);

        // Right → fixed left=20
        let r = resized_bounds(orig, ResizeHandle::Right, p, false, 1.0, sw, sh);
        assert_eq!(r.x, 20.0);
        assert_eq!(r.y, 20.0);

        // BottomRight → fixed (20, 20)
        let r = resized_bounds(orig, ResizeHandle::BottomRight, p, false, 1.0, sw, sh);
        assert_eq!(r.x, 20.0);
        assert_eq!(r.y, 20.0);

        // Bottom → fixed top=20
        let r = resized_bounds(orig, ResizeHandle::Bottom, p, false, 1.0, sw, sh);
        assert_eq!(r.y, 20.0);
        assert_eq!(r.x, 20.0);

        // BottomLeft → fixed (80, 20)
        let r = resized_bounds(orig, ResizeHandle::BottomLeft, p, false, 1.0, sw, sh);
        assert_eq!(r.x + r.width, 80.0);
        assert_eq!(r.y, 20.0);

        // Left → fixed right=80
        let r = resized_bounds(orig, ResizeHandle::Left, p, false, 1.0, sw, sh);
        assert_eq!(r.x + r.width, 80.0);
        assert_eq!(r.y, 20.0);
    }

    // -- resized_bounds: corner Shift aspect lock ----------------------------

    #[test]
    fn corner_shift_preserves_aspect_ratio() {
        let orig = ImageRect {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 40.0,
        };
        let p = ImagePoint::new(70.0, 20.0);
        let r = resized_bounds(orig, ResizeHandle::TopLeft, p, true, 1.0, 200, 200);
        // fixed (90, 50), dx=20, dy=30 → scale_x=20/80=0.25, scale_y=30/40=0.75 → s=0.25
        // new left = 90 - 0.25*80 = 70, new top = 50 - 0.25*40 = 40
        let new_w = (90.0 - r.x).abs();
        let new_h = (50.0 - r.y).abs();
        // ratio should match original
        assert!((new_w / new_h - 80.0 / 40.0).abs() < 0.01);
    }

    #[test]
    fn corner_shift_all_four_corners() {
        let orig = ImageRect {
            x: 20.0,
            y: 20.0,
            width: 60.0,
            height: 30.0,
        };
        let sw = 200u32;
        let sh = 200u32;

        for handle in [
            ResizeHandle::TopLeft,
            ResizeHandle::TopRight,
            ResizeHandle::BottomRight,
            ResizeHandle::BottomLeft,
        ] {
            let p = ImagePoint::new(60.0, 40.0);
            let r = resized_bounds(orig, handle, p, true, 1.0, sw, sh);
            let rw = r.width;
            let rh = r.height;
            if rw > 0.0 && rh > 0.0 {
                assert!(
                    (rw / rh - 60.0 / 30.0).abs() < 0.01,
                    "handle {:?}: ratio {} vs 2.0",
                    handle,
                    rw / rh
                );
            }
        }
    }

    // -- resized_bounds: crossing --------------------------------------------

    #[test]
    fn crossing_every_opposite_side() {
        let orig = ImageRect {
            x: 40.0,
            y: 40.0,
            width: 20.0,
            height: 20.0,
        };
        let sw = 200u32;
        let sh = 200u32;

        // Drag Top past Bottom → rect flips: origin at old bottom, extends to new top
        let r = resized_bounds(
            orig,
            ResizeHandle::Top,
            ImagePoint::new(50.0, 70.0),
            false,
            1.0,
            sw,
            sh,
        );
        assert!(
            r.width > 0.0 && r.height > 0.0,
            "positive dims after crossing"
        );
        assert_eq!(r.y, 60.0, "origin snaps to old bottom");

        // Drag Right past Left → rect flips: right edge at old left, extends leftward
        let r = resized_bounds(
            orig,
            ResizeHandle::Right,
            ImagePoint::new(30.0, 50.0),
            false,
            1.0,
            sw,
            sh,
        );
        assert!(r.width > 0.0 && r.height > 0.0);
        assert_eq!(r.x + r.width, 40.0, "right edge snaps to old left");

        // Drag Bottom past Top
        let r = resized_bounds(
            orig,
            ResizeHandle::Bottom,
            ImagePoint::new(50.0, 30.0),
            false,
            1.0,
            sw,
            sh,
        );
        assert!(r.width > 0.0 && r.height > 0.0);
        assert_eq!(r.y + r.height, 40.0, "bottom edge snaps to old top");

        // Drag Left past Right
        let r = resized_bounds(
            orig,
            ResizeHandle::Left,
            ImagePoint::new(70.0, 50.0),
            false,
            1.0,
            sw,
            sh,
        );
        assert!(r.width > 0.0 && r.height > 0.0);
        assert_eq!(r.x, 60.0, "origin snaps to old right");
    }

    #[test]
    fn crossing_every_opposite_corner() {
        let orig = ImageRect {
            x: 40.0,
            y: 40.0,
            width: 20.0,
            height: 20.0,
        };
        let sw = 200u32;
        let sh = 200u32;

        // TopLeft past BottomRight → rect flips, origin at old BottomRight
        let r = resized_bounds(
            orig,
            ResizeHandle::TopLeft,
            ImagePoint::new(70.0, 70.0),
            false,
            1.0,
            sw,
            sh,
        );
        assert!(r.width > 0.0 && r.height > 0.0);
        assert_eq!(r.x, 60.0);
        assert_eq!(r.y, 60.0);

        // BottomRight past TopLeft → rect flips, origin at old TopLeft
        let r = resized_bounds(
            orig,
            ResizeHandle::BottomRight,
            ImagePoint::new(30.0, 30.0),
            false,
            1.0,
            sw,
            sh,
        );
        assert!(r.width > 0.0 && r.height > 0.0);
        assert_eq!(r.x + r.width, 40.0);
        assert_eq!(r.y + r.height, 40.0);
    }

    // -- resized_bounds: minimum size 4/scale --------------------------------

    #[test]
    fn min_size_enforced_on_raw_pointer_side() {
        let orig = ImageRect {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 60.0,
        };
        let scale = 2.0;
        let min_img = MIN_BOX_SCREEN / scale; // 2.0

        // Drag Right very close to left edge
        let r = resized_bounds(
            orig,
            ResizeHandle::Right,
            ImagePoint::new(11.0, 50.0),
            false,
            scale,
            200,
            200,
        );
        assert!(r.width >= min_img - 0.01, "width {} < {}", r.width, min_img);
    }

    #[test]
    fn min_size_crossing_uses_correct_side() {
        let orig = ImageRect {
            x: 50.0,
            y: 50.0,
            width: 40.0,
            height: 30.0,
        };
        let scale = 1.0;
        let min_img = MIN_BOX_SCREEN / scale;

        // Drag Right past Left by a tiny amount
        let r = resized_bounds(
            orig,
            ResizeHandle::Right,
            ImagePoint::new(49.0, 65.0),
            false,
            scale,
            200,
            200,
        );
        // The right edge crossed left; new width should be >= min
        assert!(
            r.width >= min_img - 0.01,
            "crossing width {} < {}",
            r.width,
            min_img
        );
    }

    // -- resized_bounds: bounds containment ----------------------------------

    #[test]
    fn resize_stays_within_source_near_edges() {
        let orig = ImageRect {
            x: 90.0,
            y: 90.0,
            width: 5.0,
            height: 5.0,
        };
        let sw = 100u32;
        let sh = 100u32;

        for handle in [
            ResizeHandle::TopLeft,
            ResizeHandle::Top,
            ResizeHandle::TopRight,
            ResizeHandle::Right,
            ResizeHandle::BottomRight,
            ResizeHandle::Bottom,
            ResizeHandle::BottomLeft,
            ResizeHandle::Left,
        ] {
            let r = resized_bounds(
                orig,
                handle,
                ImagePoint::new(200.0, 200.0),
                false,
                1.0,
                sw,
                sh,
            );
            assert!(r.x >= 0.0, "{:?}: x={} < 0", handle, r.x);
            assert!(r.y >= 0.0, "{:?}: y={} < 0", handle, r.y);
            assert!(
                r.x + r.width <= sw as f32 + 0.01,
                "{:?}: x+w={} > {}",
                handle,
                r.x + r.width,
                sw
            );
            assert!(
                r.y + r.height <= sh as f32 + 0.01,
                "{:?}: y+h={} > {}",
                handle,
                r.y + r.height,
                sh
            );
        }
    }
}
