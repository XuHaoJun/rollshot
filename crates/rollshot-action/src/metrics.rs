//! Deterministic, allocation-light visual metrics over `image::RgbaImage`.
//! Used by the detector on downsampled luma planes. BT.601 luma weights.

use image::RgbaImage;

/// A rectangle in downsampled-plane sample coordinates (used as a cursor mask).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A downsampled luma plane: row-major `f32` samples in `[0, 255]`.
#[derive(Debug, Clone, PartialEq)]
pub struct LumaPlane {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<f32>,
}

#[inline]
fn luma(r: u8, g: u8, b: u8) -> f32 {
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// Block-average downsample to luma. The plane width is at most `target_width`
/// (block size `ceil(src_width / target_width)`, min 1); aspect ratio is
/// preserved. If `target_width >= src_width`, the block size is 1 (no
/// downsample), so small fixtures map 1:1 to luma samples.
pub fn downsample_luma(image: &RgbaImage, target_width: u32) -> LumaPlane {
    let sw = image.width();
    let sh = image.height();
    if sw == 0 || sh == 0 || target_width == 0 {
        return LumaPlane {
            width: 0,
            height: 0,
            samples: Vec::new(),
        };
    }
    let block = sw.div_ceil(target_width).max(1);
    let width = sw.div_ceil(block);
    let height = sh.div_ceil(block);
    let mut samples = Vec::with_capacity((width * height) as usize);
    for by in 0..height {
        for bx in 0..width {
            let x0 = bx * block;
            let y0 = by * block;
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for y in y0..(y0 + block).min(sh) {
                for x in x0..(x0 + block).min(sw) {
                    let p = image.get_pixel(x, y).0;
                    sum += luma(p[0], p[1], p[2]);
                    count += 1;
                }
            }
            samples.push(if count > 0 { sum / count as f32 } else { 0.0 });
        }
    }
    LumaPlane {
        width,
        height,
        samples,
    }
}

#[inline]
fn in_mask(mask: Option<Rect>, x: u32, y: u32) -> bool {
    match mask {
        Some(r) => x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height,
        None => false,
    }
}

/// Mean absolute luma difference over unmasked samples, normalized to `[0, 1]`.
/// Returns `0.0` on dimension mismatch or empty planes.
pub fn masked_luma_diff(a: &LumaPlane, b: &LumaPlane, mask: Option<Rect>) -> f32 {
    if a.width != b.width || a.height != b.height || a.samples.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0f32;
    let mut count = 0u32;
    for y in 0..a.height {
        for x in 0..a.width {
            if in_mask(mask, x, y) {
                continue;
            }
            let i = (y * a.width + x) as usize;
            sum += (a.samples[i] - b.samples[i]).abs();
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f32) / 255.0
    }
}

/// Fraction of unmasked samples whose absolute luma delta exceeds
/// `per_sample` (in `[0, 255]` units). Result in `[0, 1]`.
pub fn changed_area_ratio(
    a: &LumaPlane,
    b: &LumaPlane,
    mask: Option<Rect>,
    per_sample: f32,
) -> f32 {
    if a.width != b.width || a.height != b.height || a.samples.is_empty() {
        return 0.0;
    }
    let mut changed = 0u32;
    let mut count = 0u32;
    for y in 0..a.height {
        for x in 0..a.width {
            if in_mask(mask, x, y) {
                continue;
            }
            let i = (y * a.width + x) as usize;
            if (a.samples[i] - b.samples[i]).abs() > per_sample {
                changed += 1;
            }
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        changed as f32 / count as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    #[test]
    fn downsample_keeps_dims_when_target_exceeds_source() {
        let img = solid(8, 6, [255, 255, 255, 255]);
        let plane = downsample_luma(&img, 384);
        assert_eq!((plane.width, plane.height), (8, 6));
        // White luma ≈ 255.
        assert!((plane.samples[0] - 255.0).abs() < 0.5);
    }

    #[test]
    fn identical_planes_have_zero_diff_and_zero_changed_area() {
        let a = downsample_luma(&solid(8, 8, [10, 20, 30, 255]), 384);
        let b = a.clone();
        assert_eq!(masked_luma_diff(&a, &b, None), 0.0);
        assert_eq!(changed_area_ratio(&a, &b, None, 12.0), 0.0);
    }

    #[test]
    fn changed_quadrant_yields_expected_area_ratio() {
        // 8x8 black; flip the top-left 4x4 quadrant to white.
        let base = solid(8, 8, [0, 0, 0, 255]);
        let mut changed = base.clone();
        for y in 0..4 {
            for x in 0..4 {
                changed.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let a = downsample_luma(&base, 384);
        let b = downsample_luma(&changed, 384);
        // 16 of 64 samples changed.
        assert!((changed_area_ratio(&a, &b, None, 12.0) - 0.25).abs() < 1e-6);
        assert!(masked_luma_diff(&a, &b, None) > 0.0);
    }

    #[test]
    fn mask_excludes_changed_region_from_metrics() {
        let base = solid(8, 8, [0, 0, 0, 255]);
        let mut changed = base.clone();
        for y in 0..4 {
            for x in 0..4 {
                changed.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let a = downsample_luma(&base, 384);
        let b = downsample_luma(&changed, 384);
        let mask = Some(Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        });
        assert_eq!(changed_area_ratio(&a, &b, mask, 12.0), 0.0);
        assert_eq!(masked_luma_diff(&a, &b, mask), 0.0);
    }
}
