//! Pixelate effect: mosaic kernel for annotation regions.

use image::RgbaImage;

use crate::geometry::ImageRect;

pub const DEFAULT_PIXELATE_BLOCK_SIZE: u32 = 16;
pub const MIN_PIXELATE_BLOCK_SIZE: u32 = 4;
pub const MAX_PIXELATE_BLOCK_SIZE: u32 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl RasterRegion {
    pub fn byte_len(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PixelatedRegion {
    pub region: RasterRegion,
    pub pixels: RgbaImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PixelateError {
    #[error("bounds are invalid or fall outside the source image")]
    InvalidBounds,
    #[error("block size {0} is outside the allowed range")]
    InvalidBlockSize(u32),
}

pub fn raster_region(
    bounds: ImageRect,
    source_width: u32,
    source_height: u32,
) -> Result<RasterRegion, PixelateError> {
    if !bounds.is_finite() {
        return Err(PixelateError::InvalidBounds);
    }
    let clamped = bounds.clamp_to(source_width, source_height);
    let x0 = clamped.x.round() as u32;
    let y0 = clamped.y.round() as u32;
    let x1 = (clamped.x + clamped.width).round() as u32;
    let y1 = (clamped.y + clamped.height).round() as u32;
    let x0 = x0.min(source_width);
    let y0 = y0.min(source_height);
    let x1 = x1.min(source_width);
    let y1 = y1.min(source_height);
    if x1 <= x0 || y1 <= y0 {
        return Err(PixelateError::InvalidBounds);
    }
    Ok(RasterRegion {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

pub fn pixelate_region(
    source: &RgbaImage,
    bounds: ImageRect,
    block_size: u32,
) -> Result<PixelatedRegion, PixelateError> {
    if !(MIN_PIXELATE_BLOCK_SIZE..=MAX_PIXELATE_BLOCK_SIZE).contains(&block_size) {
        return Err(PixelateError::InvalidBlockSize(block_size));
    }
    let region = raster_region(bounds, source.width(), source.height())?;
    let mut pixels = RgbaImage::new(region.width, region.height);
    for local_y in (0..region.height).step_by(block_size as usize) {
        for local_x in (0..region.width).step_by(block_size as usize) {
            let cell_w = block_size.min(region.width - local_x);
            let cell_h = block_size.min(region.height - local_y);
            let sample_count = u64::from(cell_w) * u64::from(cell_h);
            let mut alpha_sum = 0_u64;
            let mut premul = [0_u64; 3];
            for y in 0..cell_h {
                for x in 0..cell_w {
                    let p = source.get_pixel(region.x + local_x + x, region.y + local_y + y).0;
                    let a = u64::from(p[3]);
                    alpha_sum += a;
                    for channel in 0..3 {
                        premul[channel] += u64::from(p[channel]) * a;
                    }
                }
            }
            let out_alpha = ((alpha_sum + sample_count / 2) / sample_count) as u8;
            let mut out = [0_u8; 4];
            out[3] = out_alpha;
            if alpha_sum != 0 {
                for channel in 0..3 {
                    out[channel] = ((premul[channel] + alpha_sum / 2) / alpha_sum) as u8;
                }
            }
            for y in 0..cell_h {
                for x in 0..cell_w {
                    pixels.put_pixel(local_x + x, local_y + y, image::Rgba(out));
                }
            }
        }
    }
    Ok(PixelatedRegion { region, pixels })
}

pub(crate) fn apply_pixelate(destination: &mut RgbaImage, region: &PixelatedRegion) {
    for y in 0..region.region.height {
        for x in 0..region.region.width {
            let px = region.pixels.get_pixel(x, y);
            destination.put_pixel(region.region.x + x, region.region.y + y, *px);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::ImageRect;

    fn numbered_10_by_7_opaque_image() -> RgbaImage {
        let mut img = RgbaImage::new(10, 7);
        for y in 0..7 {
            for x in 0..10 {
                let idx = y * 10 + x;
                img.put_pixel(x, y, image::Rgba([idx as u8, idx as u8, idx as u8, 255]));
            }
        }
        img
    }

    fn assert_block_equals_average(
        source: &RgbaImage,
        result: &PixelatedRegion,
        (bx, by, bw, bh): (u32, u32, u32, u32),
    ) {
        let mut alpha_sum = 0u64;
        let mut premul = [0u64; 3];
        let sample_count = u64::from(bw) * u64::from(bh);
        for y in by..by + bh {
            for x in bx..bx + bw {
                let p = source.get_pixel(result.region.x + x, result.region.y + y).0;
                let a = u64::from(p[3]);
                alpha_sum += a;
                for c in 0..3 {
                    premul[c] += u64::from(p[c]) * a;
                }
            }
        }
        let out_alpha = ((alpha_sum + sample_count / 2) / sample_count) as u8;
        let mut expected = [0u8; 4];
        expected[3] = out_alpha;
        if alpha_sum != 0 {
            for c in 0..3 {
                expected[c] = ((premul[c] + alpha_sum / 2) / alpha_sum) as u8;
            }
        }
        for y in 0..bh {
            for x in 0..bw {
                let actual = result.pixels.get_pixel(bx + x, by + y).0;
                assert_eq!(
                    actual, expected,
                    "pixel at local ({}, {}) expected {:?}, got {:?}",
                    bx + x, by + y, expected, actual,
                );
            }
        }
    }

    #[test]
    fn grid_is_region_local_and_partial_cells_use_actual_pixels() {
        let source = numbered_10_by_7_opaque_image();
        let result = pixelate_region(
            &source,
            ImageRect::new(1.0, 1.0, 8.0, 6.0),
            4,
        )
        .unwrap();
        assert_eq!(result.region, RasterRegion { x: 1, y: 1, width: 8, height: 6 });
        assert_block_equals_average(&source, &result, (0, 0, 4, 4));
        assert_block_equals_average(&source, &result, (4, 0, 4, 4));
        assert_block_equals_average(&source, &result, (0, 4, 4, 2));
        assert_block_equals_average(&source, &result, (4, 4, 4, 2));
    }

    #[test]
    fn transparent_colors_are_averaged_in_premultiplied_space() {
        let source = RgbaImage::from_raw(
            2,
            1,
            vec![255, 0, 0, 255, 0, 0, 255, 0],
        )
        .unwrap();
        let result = pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 1.0), 4).unwrap();
        assert_eq!(result.pixels.get_pixel(0, 0).0, [255, 0, 0, 128]);
        assert_eq!(result.pixels.get_pixel(1, 0).0, [255, 0, 0, 128]);
    }

    #[test]
    fn partial_alpha_is_averaged_in_premultiplied_space() {
        let source = RgbaImage::from_raw(
            2,
            1,
            vec![255, 0, 0, 255, 0, 0, 255, 128],
        )
        .unwrap();
        let result = pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 1.0), 4).unwrap();
        assert_eq!(result.pixels.get_pixel(0, 0).0, [170, 0, 85, 192]);
        assert_eq!(result.pixels.get_pixel(1, 0).0, [170, 0, 85, 192]);
    }

    #[test]
    fn validation_rejects_invalid_strength_and_empty_clamped_region() {
        let source = RgbaImage::new(8, 8);
        assert_eq!(pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 2.0), 3), Err(PixelateError::InvalidBlockSize(3)));
        assert_eq!(pixelate_region(&source, ImageRect::new(0.0, 0.0, 2.0, 2.0), 49), Err(PixelateError::InvalidBlockSize(49)));
        assert_eq!(pixelate_region(&source, ImageRect::new(20.0, 20.0, 2.0, 2.0), 16), Err(PixelateError::InvalidBounds));
        assert_eq!(pixelate_region(&source, ImageRect::new(f32::NAN, 0.0, 2.0, 2.0), 16), Err(PixelateError::InvalidBounds));
    }
}
