// Public API consumed by later tasks; allow dead_code until the call sites land.
#![allow(dead_code)]

use iced::{Point, Size, Vector};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Fixed zoom percentage steps available via step-zoom.
pub const ZOOM_STEPS: [u16; 10] = [25, 33, 50, 67, 100, 125, 150, 200, 300, 400];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomMode {
    FitWindow,
    FitWidth,
    FitHeight,
    ActualSize,
    /// Percentage, e.g. `Custom(100)` = 100%.
    Custom(u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportGeometry {
    pub scale: f32,
    pub rendered_size: Size,
    pub content_size: Size,
    pub image_origin: Point,
    pub max_scroll: Vector,
    pub horizontal_overflow: bool,
    pub vertical_overflow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportState {
    pub zoom: ZoomMode,
    pub scroll_offset: Vector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDirection {
    In,
    Out,
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Choose the default zoom mode based on image shape.
///
/// Rule (strict): long edge > 2× short edge is "long".
/// - Exactly 2.0 ratio → FitWindow (normal).
/// - Vertical long (h > 2×w) → FitWidth.
/// - Horizontal long (w > 2×h) → FitHeight.
/// - Otherwise → FitWindow.
pub fn default_zoom(image: Size) -> ZoomMode {
    let w = image.width;
    let h = image.height;
    if h > 2.0 * w {
        ZoomMode::FitWidth
    } else if w > 2.0 * h {
        ZoomMode::FitHeight
    } else {
        ZoomMode::FitWindow
    }
}

/// Compute the display scale for `mode` given `image` and `viewport` sizes.
///
/// - FitWidth  = viewport.w / image.w
/// - FitHeight = viewport.h / image.h
/// - FitWindow = min(FitWidth, FitHeight)
/// - ActualSize = 1.0
/// - Custom(p)  = p / 100
pub fn scale_for(mode: ZoomMode, image: Size, viewport: Size) -> f32 {
    match mode {
        ZoomMode::FitWidth => viewport.width / image.width,
        ZoomMode::FitHeight => viewport.height / image.height,
        ZoomMode::FitWindow => {
            let sw = viewport.width / image.width;
            let sh = viewport.height / image.height;
            sw.min(sh)
        }
        ZoomMode::ActualSize => 1.0,
        ZoomMode::Custom(p) => p as f32 / 100.0,
    }
}

/// Compute full viewport geometry for the given zoom mode, image, and viewport.
///
/// - `image_origin`: centered offset when rendered dimension < viewport on that
///   axis, zero otherwise (no centering when the image overflows the viewport).
/// - `max_scroll`: max(0, rendered - viewport) per axis.
/// - `horizontal_overflow` / `vertical_overflow`: rendered > viewport per axis.
pub fn geometry_for(mode: ZoomMode, image: Size, viewport: Size) -> ViewportGeometry {
    let scale = scale_for(mode, image, viewport);

    let rendered_size = Size::new(image.width * scale, image.height * scale);

    let h_overflow = rendered_size.width > viewport.width;
    let v_overflow = rendered_size.height > viewport.height;

    let origin_x = if h_overflow {
        0.0
    } else {
        (viewport.width - rendered_size.width) / 2.0
    };
    let origin_y = if v_overflow {
        0.0
    } else {
        (viewport.height - rendered_size.height) / 2.0
    };

    let max_scroll_x = (rendered_size.width - viewport.width).max(0.0);
    let max_scroll_y = (rendered_size.height - viewport.height).max(0.0);

    // content_size: at least the viewport on each axis
    let content_size = Size::new(
        rendered_size.width.max(viewport.width),
        rendered_size.height.max(viewport.height),
    );

    ViewportGeometry {
        scale,
        rendered_size,
        content_size,
        image_origin: Point::new(origin_x, origin_y),
        max_scroll: Vector::new(max_scroll_x, max_scroll_y),
        horizontal_overflow: h_overflow,
        vertical_overflow: v_overflow,
    }
}

/// Step zoom in or out through `ZOOM_STEPS`, clamped to [25, 400].
///
/// For `Custom(p)`: find the nearest step strictly in the requested direction.
/// If already at the boundary step, return the same value (clamped).
///
/// For `ActualSize`: treated as `Custom(100)` for step resolution.
///
/// For fit modes (`FitWindow`, `FitWidth`, `FitHeight`): no current percentage
/// is available without a viewport, so they are treated as `Custom(100)` as a
/// neutral baseline. The caller should switch to a `Custom` mode after stepping.
pub fn step_zoom(mode: ZoomMode, dir: ZoomDirection) -> ZoomMode {
    let current_pct: u16 = match mode {
        ZoomMode::Custom(p) => p,
        ZoomMode::ActualSize => 100,
        ZoomMode::FitWindow | ZoomMode::FitWidth | ZoomMode::FitHeight => 100,
    };

    let new_pct = match dir {
        ZoomDirection::In => ZOOM_STEPS
            .iter()
            .find(|&&s| s > current_pct)
            .copied()
            .unwrap_or(400),
        ZoomDirection::Out => ZOOM_STEPS
            .iter()
            .rev()
            .find(|&&s| s < current_pct)
            .copied()
            .unwrap_or(25),
    };

    ZoomMode::Custom(new_pct.clamp(25, 400))
}

/// Clamp a scroll offset so each axis stays within [0, max_scroll].
pub fn clamp_scroll(offset: Vector, max_scroll: Vector) -> Vector {
    Vector::new(
        offset.x.clamp(0.0, max_scroll.x),
        offset.y.clamp(0.0, max_scroll.y),
    )
}

/// Compute the new scroll offset that keeps the image point under `pointer`
/// stationary after a zoom change from `old` geometry to `new` geometry.
///
/// Formula:
/// 1. image_point = (old_offset + pointer - old.image_origin) / old.scale
/// 2. new_offset  = image_point * new.scale + new.image_origin - pointer
/// 3. Clamped to [0, new.max_scroll]
pub fn anchored_scroll(
    old_offset: Vector,
    pointer: Point,
    old: ViewportGeometry,
    new: ViewportGeometry,
) -> Vector {
    let ip_x = (old_offset.x + pointer.x - old.image_origin.x) / old.scale;
    let ip_y = (old_offset.y + pointer.y - old.image_origin.y) / old.scale;

    let new_x = ip_x * new.scale + new.image_origin.x - pointer.x;
    let new_y = ip_y * new.scale + new.image_origin.y - pointer.y;

    clamp_scroll(Vector::new(new_x, new_y), new.max_scroll)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Plan-required tests (verbatim from spec) ----------------------------

    #[test]
    fn default_modes_match_image_shape() {
        assert_eq!(default_zoom(Size::new(1200.0, 800.0)), ZoomMode::FitWindow);
        assert_eq!(default_zoom(Size::new(800.0, 2401.0)), ZoomMode::FitWidth);
        assert_eq!(default_zoom(Size::new(2401.0, 800.0)), ZoomMode::FitHeight);
    }

    #[test]
    fn fit_scales_use_the_requested_axis() {
        let image = Size::new(1000.0, 2000.0);
        let viewport = Size::new(500.0, 600.0);
        assert_eq!(scale_for(ZoomMode::FitWidth, image, viewport), 0.5);
        assert_eq!(scale_for(ZoomMode::FitHeight, image, viewport), 0.3);
        assert_eq!(scale_for(ZoomMode::FitWindow, image, viewport), 0.3);
    }

    #[test]
    fn fixed_steps_clamp_to_supported_range() {
        assert_eq!(
            step_zoom(ZoomMode::Custom(25), ZoomDirection::Out),
            ZoomMode::Custom(25)
        );
        assert_eq!(
            step_zoom(ZoomMode::Custom(100), ZoomDirection::In),
            ZoomMode::Custom(125)
        );
        assert_eq!(
            step_zoom(ZoomMode::Custom(400), ZoomDirection::In),
            ZoomMode::Custom(400)
        );
    }

    #[test]
    fn smaller_images_are_centered_without_overflow() {
        let geometry = geometry_for(
            ZoomMode::ActualSize,
            Size::new(300.0, 200.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(geometry.image_origin, Point::new(250.0, 200.0));
        assert_eq!(geometry.max_scroll, Vector::new(0.0, 0.0));
        assert!(!geometry.horizontal_overflow);
        assert!(!geometry.vertical_overflow);
    }

    #[test]
    fn pointer_anchor_preserves_the_image_point_when_possible() {
        let old = geometry_for(
            ZoomMode::Custom(100),
            Size::new(1000.0, 2000.0),
            Size::new(500.0, 500.0),
        );
        let new = geometry_for(
            ZoomMode::Custom(200),
            Size::new(1000.0, 2000.0),
            Size::new(500.0, 500.0),
        );
        assert_eq!(
            anchored_scroll(
                Vector::new(100.0, 300.0),
                Point::new(250.0, 250.0),
                old,
                new
            ),
            Vector::new(450.0, 850.0)
        );
    }

    // -- Spec §14.1 boundary tests -------------------------------------------

    /// Exactly 2.0 ratio (long edge = 2× short edge) is normal → FitWindow.
    #[test]
    fn boundary_ratio_exactly_two_is_normal() {
        // vertical: h = 2 * w (not strictly greater)
        assert_eq!(
            default_zoom(Size::new(500.0, 1000.0)),
            ZoomMode::FitWindow,
            "h == 2×w should be FitWindow (boundary is not long)"
        );
        // horizontal: w = 2 * h
        assert_eq!(
            default_zoom(Size::new(1000.0, 500.0)),
            ZoomMode::FitWindow,
            "w == 2×h should be FitWindow (boundary is not long)"
        );
    }

    /// Strictly greater than 2.0 ratio → long (FitWidth / FitHeight).
    #[test]
    fn boundary_ratio_just_over_two_is_long() {
        // vertical: h just over 2× w
        let w = 500.0_f32;
        let h = 500.0_f32 * 2.0 + 0.001; // 1000.001
        assert_eq!(
            default_zoom(Size::new(w, h)),
            ZoomMode::FitWidth,
            "h > 2×w by epsilon should be FitWidth"
        );
        // horizontal: w just over 2× h
        assert_eq!(
            default_zoom(Size::new(h, w)),
            ZoomMode::FitHeight,
            "w > 2×h by epsilon should be FitHeight"
        );
    }

    // -- step_zoom non-uniform gaps and non-Custom inputs --------------------

    /// Crossing the 67→100 gap (non-uniform step).
    #[test]
    fn step_zoom_crosses_non_uniform_gap() {
        assert_eq!(
            step_zoom(ZoomMode::Custom(67), ZoomDirection::In),
            ZoomMode::Custom(100)
        );
        assert_eq!(
            step_zoom(ZoomMode::Custom(33), ZoomDirection::Out),
            ZoomMode::Custom(25)
        );
        assert_eq!(
            step_zoom(ZoomMode::Custom(150), ZoomDirection::Out),
            ZoomMode::Custom(125)
        );
    }

    /// Stepping from ActualSize (treated as Custom(100) baseline).
    #[test]
    fn step_zoom_from_actual_size() {
        assert_eq!(
            step_zoom(ZoomMode::ActualSize, ZoomDirection::In),
            ZoomMode::Custom(125)
        );
        assert_eq!(
            step_zoom(ZoomMode::ActualSize, ZoomDirection::Out),
            ZoomMode::Custom(67)
        );
    }

    /// Stepping from fit modes (treated as Custom(100) baseline).
    #[test]
    fn step_zoom_from_fit_modes_uses_100_baseline() {
        for fit in [ZoomMode::FitWindow, ZoomMode::FitWidth, ZoomMode::FitHeight] {
            assert_eq!(
                step_zoom(fit, ZoomDirection::In),
                ZoomMode::Custom(125),
                "{fit:?} + In should map to Custom(125)"
            );
            assert_eq!(
                step_zoom(fit, ZoomDirection::Out),
                ZoomMode::Custom(67),
                "{fit:?} + Out should map to Custom(67)"
            );
        }
    }

    // -- clamp_scroll --------------------------------------------------------

    #[test]
    fn clamp_scroll_clamps_beyond_max_to_max() {
        let max = Vector::new(300.0, 500.0);
        let clamped = clamp_scroll(Vector::new(400.0, 600.0), max);
        assert_eq!(clamped, Vector::new(300.0, 500.0));
    }

    #[test]
    fn clamp_scroll_clamps_negative_to_zero() {
        let max = Vector::new(300.0, 500.0);
        let clamped = clamp_scroll(Vector::new(-10.0, -5.0), max);
        assert_eq!(clamped, Vector::new(0.0, 0.0));
    }

    #[test]
    fn clamp_scroll_passes_through_in_range() {
        let max = Vector::new(300.0, 500.0);
        let offset = Vector::new(150.0, 250.0);
        assert_eq!(clamp_scroll(offset, max), offset);
    }

    // -- geometry_for overflow -----------------------------------------------

    /// An image larger than the viewport overflows, has positive max_scroll,
    /// and image_origin is (0,0) on the overflowing axes.
    #[test]
    fn geometry_for_overflow_image() {
        let geometry = geometry_for(
            ZoomMode::ActualSize,
            Size::new(1200.0, 900.0),
            Size::new(800.0, 600.0),
        );
        // Both axes overflow
        assert!(geometry.horizontal_overflow);
        assert!(geometry.vertical_overflow);
        // No centering on overflowing axes
        assert_eq!(geometry.image_origin, Point::new(0.0, 0.0));
        // max_scroll = rendered - viewport = 1200-800, 900-600
        assert_eq!(geometry.max_scroll, Vector::new(400.0, 300.0));
    }

    /// When only one axis overflows, the non-overflowing axis is still centered.
    #[test]
    fn geometry_for_partial_overflow() {
        // image wider than viewport, shorter than viewport
        let geometry = geometry_for(
            ZoomMode::ActualSize,
            Size::new(1000.0, 400.0),
            Size::new(800.0, 600.0),
        );
        assert!(geometry.horizontal_overflow);
        assert!(!geometry.vertical_overflow);
        // x: overflows → origin_x = 0
        assert_eq!(geometry.image_origin.x, 0.0);
        // y: no overflow → centered: (600 - 400) / 2 = 100
        assert_eq!(geometry.image_origin.y, 100.0);
        assert_eq!(geometry.max_scroll.x, 200.0);
        assert_eq!(geometry.max_scroll.y, 0.0);
    }
}
