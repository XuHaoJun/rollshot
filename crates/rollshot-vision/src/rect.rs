//! Pixel-space rectangle helpers shared by template matching and (later)
//! region features. `ImageRect` is f32 pixel-space; `PixelRect` is the u32
//! integer grid used by the `image` crate.

use rollshot_automation::{CapabilityError, Region};
use rollshot_image_document::ImageRect;

/// Default cap on template search area (pixels). Bounds naive-NCC cost.
pub const MAX_SEARCH_AREA: u64 = 8_000_000; // ~ 4000x2000

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Convert an f32 `ImageRect` to an integer `PixelRect` covering it.
///
/// Rounding: floor on the min edges, ceil on the max edges (smallest integer
/// rect that fully covers the f32 rect), then clamp to the image. Rejects
/// non-finite, empty (before or after clamp), and oversized regions.
pub fn to_pixel_rect(
    rect: ImageRect,
    image_w: u32,
    image_h: u32,
    max_area: u64,
) -> Result<PixelRect, CapabilityError> {
    if !rect.is_finite() {
        return Err(CapabilityError::InvalidInput {
            code: "non_finite_region",
        });
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(CapabilityError::InvalidInput {
            code: "empty_region",
        });
    }
    let x0 = rect.x.floor();
    let y0 = rect.y.floor();
    let x1 = (rect.x + rect.width).ceil();
    let y1 = (rect.y + rect.height).ceil();
    if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
        return Err(CapabilityError::InvalidInput {
            code: "non_finite_region",
        });
    }

    // Clamp to [0, image]. Values are finite here.
    let cx0 = x0.max(0.0).min(image_w as f32) as u32;
    let cy0 = y0.max(0.0).min(image_h as f32) as u32;
    let cx1 = x1.max(0.0).min(image_w as f32) as u32;
    let cy1 = y1.max(0.0).min(image_h as f32) as u32;

    if cx1 <= cx0 || cy1 <= cy0 {
        return Err(CapabilityError::InvalidInput {
            code: "empty_region",
        });
    }
    let width = cx1 - cx0;
    let height = cy1 - cy0;
    if (width as u64) * (height as u64) > max_area {
        return Err(CapabilityError::InvalidInput {
            code: "region_too_large",
        });
    }
    Ok(PixelRect {
        x: cx0,
        y: cy0,
        width,
        height,
    })
}

/// Resolve a capability `Region` to a `PixelRect` against the image.
pub fn region_to_pixel_rect(
    region: &Region,
    image_w: u32,
    image_h: u32,
    max_area: u64,
) -> Result<PixelRect, CapabilityError> {
    match region {
        Region::Full => to_pixel_rect(
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: image_w as f32,
                height: image_h as f32,
            },
            image_w,
            image_h,
            max_area,
        ),
        Region::Rect { bounds } => to_pixel_rect(*bounds, image_w, image_h, max_area),
    }
}

/// Intersection-over-union of two rects in image space.
pub fn iou(a: ImageRect, b: ImageRect) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let ix = (ax2.min(bx2) - a.x.max(b.x)).max(0.0);
    let iy = (ay2.min(by2) - a.y.max(b.y)).max(0.0);
    let inter = ix * iy;
    let union = a.width * a.height + b.width * b.height - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Bounding union of two rects.
pub fn union(a: ImageRect, b: ImageRect) -> ImageRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let x2 = (a.x + a.width).max(b.x + b.width);
    let y2 = (a.y + a.height).max(b.y + b.height);
    ImageRect {
        x,
        y,
        width: x2 - x,
        height: y2 - y,
    }
}

/// Expand a rect by `pad` on every side, clamped to the image.
pub fn pad_and_clip(rect: ImageRect, pad: f32, image_w: u32, image_h: u32) -> ImageRect {
    rect.expanded(pad).clamp_to(image_w, image_h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_automation::CapabilityError;
    use rollshot_image_document::ImageRect;

    fn r(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn pixel_rect_uses_floor_min_ceil_max() {
        // x in [10.2, 10.2+5.3=15.5] -> floor(10.2)=10 .. ceil(15.5)=16 -> w=6
        let p = to_pixel_rect(r(10.2, 4.9, 5.3, 2.2), 100, 100, MAX_SEARCH_AREA).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (10, 4, 6, 4));
    }

    #[test]
    fn pixel_rect_clamps_to_image() {
        let p = to_pixel_rect(r(-5.0, -5.0, 20.0, 20.0), 10, 10, MAX_SEARCH_AREA).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (0, 0, 10, 10));
    }

    #[test]
    fn pixel_rect_rejects_non_finite() {
        let e = to_pixel_rect(r(f32::NAN, 0.0, 1.0, 1.0), 10, 10, MAX_SEARCH_AREA).unwrap_err();
        assert_eq!(
            e,
            CapabilityError::InvalidInput {
                code: "non_finite_region"
            }
        );
    }

    #[test]
    fn pixel_rect_rejects_non_finite_endpoints() {
        let e =
            to_pixel_rect(r(f32::MAX, 0.0, f32::MAX, 1.0), 10, 10, MAX_SEARCH_AREA).unwrap_err();
        assert_eq!(
            e,
            CapabilityError::InvalidInput {
                code: "non_finite_region"
            }
        );
    }

    #[test]
    fn pixel_rect_rejects_empty() {
        let e = to_pixel_rect(r(5.0, 5.0, 0.0, 3.0), 10, 10, MAX_SEARCH_AREA).unwrap_err();
        assert_eq!(
            e,
            CapabilityError::InvalidInput {
                code: "empty_region"
            }
        );
    }

    #[test]
    fn pixel_rect_rejects_oversized() {
        let e = to_pixel_rect(r(0.0, 0.0, 100.0, 100.0), 100, 100, 100).unwrap_err();
        assert_eq!(
            e,
            CapabilityError::InvalidInput {
                code: "region_too_large"
            }
        );
    }

    #[test]
    fn iou_of_identical_is_one() {
        assert!((iou(r(0.0, 0.0, 10.0, 10.0), r(0.0, 0.0, 10.0, 10.0)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_of_disjoint_is_zero() {
        assert_eq!(iou(r(0.0, 0.0, 5.0, 5.0), r(20.0, 20.0, 5.0, 5.0)), 0.0);
    }
}
