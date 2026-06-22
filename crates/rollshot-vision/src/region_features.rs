//! Deterministic, per-region numeric features used as a runtime sanity filter.
//! Pure functions over `VisualIndex` data; no host state, no QuickJS, no alloc
//! of new images. Computed inside `prepare_region_features` (outside QuickJS).

use image::{GrayImage, RgbaImage};

use crate::rect::PixelRect;

/// RGB quantization step. MUST divide 256 (256 / 16 = 16 bins per channel).
pub(crate) const QUANTIZE_STEP: u32 = 16;

/// Per-pixel combined-gradient threshold (`|dx| + |dy|`) for counting an edge.
pub(crate) const EDGE_THRESHOLD: u16 = 32;

/// Area cap for a regionFeatures query (reuse the template search-area cap).
pub(crate) const MAX_REGION_FEATURES_AREA: u64 = crate::rect::MAX_SEARCH_AREA;

/// Dominant quantized color of `rect`, returned as the winning bin's center.
/// Alpha is fixed at 255 (SP2 assumes screenshot-like opaque input).
pub(crate) fn dominant_rgba(image: &RgbaImage, rect: PixelRect) -> [u8; 4] {
    let bins_per_channel = 256 / QUANTIZE_STEP; // 16
    let bin_count = (bins_per_channel * bins_per_channel * bins_per_channel) as usize;
    let mut histogram = vec![0u32; bin_count];

    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let px = image.get_pixel(x, y).0;
            let rb = px[0] as u32 / QUANTIZE_STEP;
            let gb = px[1] as u32 / QUANTIZE_STEP;
            let bb = px[2] as u32 / QUANTIZE_STEP;
            let index = (rb * bins_per_channel + gb) * bins_per_channel + bb;
            histogram[index as usize] += 1;
        }
    }

    // Lowest index wins ties: `>` keeps the first (lowest) max.
    let mut best_index = 0usize;
    let mut best_count = 0u32;
    for (index, &count) in histogram.iter().enumerate() {
        if count > best_count {
            best_count = count;
            best_index = index;
        }
    }

    let bins = bins_per_channel as usize;
    let rb = (best_index / (bins * bins)) as u32;
    let gb = ((best_index / bins) % bins) as u32;
    let bb = (best_index % bins) as u32;
    let center = |bin: u32| (bin * QUANTIZE_STEP + QUANTIZE_STEP / 2) as u8;
    [center(rb), center(gb), center(bb), 255]
}

/// Fraction of in-rect pixels (with both a right and a down neighbor) whose
/// combined gradient exceeds `EDGE_THRESHOLD`. Range [0, 1]; 0.0 if rect is
/// narrower/shorter than 2 px.
pub(crate) fn edge_density(gray: &GrayImage, rect: PixelRect) -> f32 {
    if rect.width < 2 || rect.height < 2 {
        return 0.0;
    }
    let mut edge_count: u64 = 0;
    let mut counted: u64 = 0;
    let x_end = rect.x + rect.width - 1; // exclusive of last col (no right neighbor)
    let y_end = rect.y + rect.height - 1; // exclusive of last row (no down neighbor)
    for y in rect.y..y_end {
        for x in rect.x..x_end {
            let here = gray.get_pixel(x, y).0[0] as i16;
            let right = gray.get_pixel(x + 1, y).0[0] as i16;
            let down = gray.get_pixel(x, y + 1).0[0] as i16;
            let grad = (here - right).unsigned_abs() + (here - down).unsigned_abs();
            if grad >= EDGE_THRESHOLD {
                edge_count += 1;
            }
            counted += 1;
        }
    }
    if counted == 0 {
        0.0
    } else {
        edge_count as f32 / counted as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::PixelRect;

    fn full(w: u32, h: u32) -> PixelRect {
        PixelRect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        }
    }

    #[test]
    fn dominant_of_solid_region_is_that_bins_center() {
        // (104,152,200) are already bin centers for QUANTIZE_STEP=16
        // (104/16=6 -> 6*16+8=104, 152->152, 200->200), so output == input rgb.
        let img = RgbaImage::from_pixel(8, 8, image::Rgba([104, 152, 200, 255]));
        assert_eq!(dominant_rgba(&img, full(8, 8)), [104, 152, 200, 255]);
    }

    #[test]
    fn dominant_picks_majority_color() {
        // Left 6 cols red, right 2 cols blue -> red wins.
        let mut img = RgbaImage::from_pixel(8, 4, image::Rgba([200, 40, 40, 255]));
        for y in 0..4 {
            for x in 6..8 {
                img.put_pixel(x, y, image::Rgba([40, 40, 200, 255]));
            }
        }
        // 200 -> bin 12 -> center 200; 40 -> bin 2 -> center 40.
        assert_eq!(dominant_rgba(&img, full(8, 4)), [200, 40, 40, 255]);
    }

    #[test]
    fn dominant_tie_breaks_to_lowest_bin_index() {
        // Half pixels color A (lower bin), half color B (higher bin), equal count.
        let mut img = RgbaImage::from_pixel(4, 2, image::Rgba([8, 8, 8, 255])); // bin 0 -> center 8
        for x in 2..4 {
            img.put_pixel(x, 0, image::Rgba([200, 200, 200, 255])); // bin 12 -> center 200
            img.put_pixel(x, 1, image::Rgba([200, 200, 200, 255]));
        }
        // 4 px at (8,8,8) bin index 0, 4 px at (200,200,200) higher index -> tie -> lowest wins.
        assert_eq!(dominant_rgba(&img, full(4, 2)), [8, 8, 8, 255]);
    }

    fn gray_from_fn(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> GrayImage {
        GrayImage::from_fn(w, h, |x, y| image::Luma([f(x, y)]))
    }

    #[test]
    fn edge_density_of_solid_region_is_zero() {
        let g = gray_from_fn(8, 8, |_, _| 128);
        assert_eq!(edge_density(&g, full(8, 8)), 0.0);
    }

    #[test]
    fn edge_density_of_vertical_stripes_is_one() {
        // Alternating columns 0/255: every counted pixel has |dx|=255 >= threshold.
        let g = gray_from_fn(4, 4, |x, _| if x % 2 == 0 { 0 } else { 255 });
        assert_eq!(edge_density(&g, full(4, 4)), 1.0);
    }

    #[test]
    fn edge_density_below_threshold_is_zero() {
        // Constant small horizontal ramp with step < EDGE_THRESHOLD -> no edges.
        let step = (EDGE_THRESHOLD - 1) as u32;
        let g = gray_from_fn(4, 4, |x, _| (x * step).min(255) as u8);
        assert_eq!(edge_density(&g, full(4, 4)), 0.0);
    }

    #[test]
    fn edge_density_narrow_region_is_zero_no_panic() {
        let g = gray_from_fn(8, 8, |x, _| if x % 2 == 0 { 0 } else { 255 });
        assert_eq!(
            edge_density(
                &g,
                PixelRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 8
                }
            ),
            0.0
        );
        assert_eq!(
            edge_density(
                &g,
                PixelRect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 1
                }
            ),
            0.0
        );
    }
}
