use crate::error::CaptureError;
use crate::types::Region;
use image::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPixelFormat {
    Bgra,
    Rgba,
    Bgrx,
    Rgbx,
    Rgb,
}

#[derive(Debug, Clone, Copy)]
pub struct LinuxRawFrame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LinuxPixelFormat,
    pub crop: Option<Region>,
}

pub(super) fn bytes_per_pixel(format: LinuxPixelFormat) -> u32 {
    match format {
        LinuxPixelFormat::Bgra
        | LinuxPixelFormat::Rgba
        | LinuxPixelFormat::Bgrx
        | LinuxPixelFormat::Rgbx => 4,
        LinuxPixelFormat::Rgb => 3,
    }
}

fn validate_region(frame: LinuxRawFrame<'_>) -> Result<Region, CaptureError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(CaptureError::InvalidConfig {
            message: "PipeWire frame dimensions must be non-zero".to_string(),
        });
    }
    let bpp = bytes_per_pixel(frame.format);
    let min_stride = frame
        .width
        .checked_mul(bpp)
        .ok_or_else(|| CaptureError::InvalidConfig {
            message: "PipeWire frame row size overflowed u32".to_string(),
        })?;
    if frame.stride < min_stride {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "PipeWire frame stride {} is smaller than row size {}",
                frame.stride, min_stride
            ),
        });
    }
    let required = (frame.stride as usize)
        .checked_mul(frame.height as usize)
        .ok_or_else(|| CaptureError::InvalidConfig {
            message: "PipeWire frame buffer size overflowed usize".to_string(),
        })?;
    if frame.data.len() < required {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "PipeWire frame buffer has {} bytes but needs at least {}",
                frame.data.len(),
                required
            ),
        });
    }
    let region = frame.crop.unwrap_or(Region {
        x: 0,
        y: 0,
        width: frame.width,
        height: frame.height,
    });
    if region.x < 0 || region.y < 0 || region.width == 0 || region.height == 0 {
        return Err(CaptureError::InvalidConfig {
            message: format!("invalid crop region {:?}", region),
        });
    }
    let x2 = region.x as u32 + region.width;
    let y2 = region.y as u32 + region.height;
    if x2 > frame.width || y2 > frame.height {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "crop region {:?} is outside source frame {}x{}",
                region, frame.width, frame.height
            ),
        });
    }
    Ok(region)
}

pub fn raw_frame_to_rgba(frame: LinuxRawFrame<'_>) -> Result<RgbaImage, CaptureError> {
    let region = validate_region(frame)?;
    let bpp = bytes_per_pixel(frame.format) as usize;
    let mut out = vec![0u8; region.width as usize * region.height as usize * 4];
    let mut out_index = 0;

    for y in region.y as u32..region.y as u32 + region.height {
        let row_start = y as usize * frame.stride as usize;
        for x in region.x as u32..region.x as u32 + region.width {
            let pixel = row_start + x as usize * bpp;
            let rgba = match frame.format {
                LinuxPixelFormat::Bgra => [
                    frame.data[pixel + 2],
                    frame.data[pixel + 1],
                    frame.data[pixel],
                    frame.data[pixel + 3],
                ],
                LinuxPixelFormat::Rgba => [
                    frame.data[pixel],
                    frame.data[pixel + 1],
                    frame.data[pixel + 2],
                    frame.data[pixel + 3],
                ],
                LinuxPixelFormat::Bgrx => [
                    frame.data[pixel + 2],
                    frame.data[pixel + 1],
                    frame.data[pixel],
                    255,
                ],
                LinuxPixelFormat::Rgbx => [
                    frame.data[pixel],
                    frame.data[pixel + 1],
                    frame.data[pixel + 2],
                    255,
                ],
                LinuxPixelFormat::Rgb => [
                    frame.data[pixel],
                    frame.data[pixel + 1],
                    frame.data[pixel + 2],
                    255,
                ],
            };
            out[out_index..out_index + 4].copy_from_slice(&rgba);
            out_index += 4;
        }
    }

    RgbaImage::from_raw(region.width, region.height, out).ok_or_else(|| {
        CaptureError::Backend(anyhow::anyhow!(
            "failed to build RGBA image from PipeWire frame"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frame<'a>(
        data: &'a [u8],
        width: u32,
        height: u32,
        stride: u32,
        format: LinuxPixelFormat,
    ) -> LinuxRawFrame<'a> {
        LinuxRawFrame {
            data,
            width,
            height,
            stride,
            format,
            crop: None,
        }
    }

    #[test]
    fn bgra_converts_to_rgba() {
        let frame = make_frame(
            &[10, 20, 30, 40, 50, 60, 70, 80],
            2,
            1,
            8,
            LinuxPixelFormat::Bgra,
        );
        let img = raw_frame_to_rgba(frame).unwrap();
        assert_eq!(img.as_raw(), &[30, 20, 10, 40, 70, 60, 50, 80]);
    }

    #[test]
    fn rgba_is_preserved() {
        let frame = make_frame(
            &[10, 20, 30, 40, 50, 60, 70, 80],
            2,
            1,
            8,
            LinuxPixelFormat::Rgba,
        );
        let img = raw_frame_to_rgba(frame).unwrap();
        assert_eq!(img.as_raw(), &[10, 20, 30, 40, 50, 60, 70, 80]);
    }

    #[test]
    fn bgrx_sets_alpha_to_255() {
        let frame = make_frame(
            &[10, 20, 30, 0, 50, 60, 70, 0],
            2,
            1,
            8,
            LinuxPixelFormat::Bgrx,
        );
        let img = raw_frame_to_rgba(frame).unwrap();
        assert_eq!(img.as_raw(), &[30, 20, 10, 255, 70, 60, 50, 255]);
    }

    #[test]
    fn rgbx_sets_alpha_to_255() {
        let frame = make_frame(
            &[10, 20, 30, 0, 50, 60, 70, 0],
            2,
            1,
            8,
            LinuxPixelFormat::Rgbx,
        );
        let img = raw_frame_to_rgba(frame).unwrap();
        assert_eq!(img.as_raw(), &[10, 20, 30, 255, 50, 60, 70, 255]);
    }

    #[test]
    fn rgb_sets_alpha_to_255() {
        let frame = make_frame(&[10, 20, 30, 50, 60, 70], 2, 1, 6, LinuxPixelFormat::Rgb);
        let img = raw_frame_to_rgba(frame).unwrap();
        assert_eq!(img.as_raw(), &[10, 20, 30, 255, 50, 60, 70, 255]);
    }

    #[test]
    fn stride_larger_than_row_width_is_honored() {
        // 2 pixels BGRA = 8 bytes per row, but stride = 12 (padded)
        let data: Vec<u8> = vec![
            10, 20, 30, 40, 50, 60, 70, 80, 0, 0, 0, 0, // row 0 (padded)
            11, 21, 31, 41, 51, 61, 71, 81, 0, 0, 0, 0, // row 1 (padded)
        ];
        let frame = LinuxRawFrame {
            data: &data,
            width: 2,
            height: 2,
            stride: 12,
            format: LinuxPixelFormat::Bgra,
            crop: None,
        };
        let img = raw_frame_to_rgba(frame).unwrap();
        assert_eq!(
            img.as_raw(),
            &[
                30, 20, 10, 40, 70, 60, 50, 80, // row 0
                31, 21, 11, 41, 71, 61, 51, 81 // row 1
            ]
        );
    }

    #[test]
    fn crop_is_applied() {
        // 3x2 BGRA image, crop to 2x1 at (1,1)
        let data: Vec<u8> = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // row 0
            0, 0, 0, 0, 10, 20, 30, 40, 50, 60, 70, 80, // row 1
        ];
        let frame = LinuxRawFrame {
            data: &data,
            width: 3,
            height: 2,
            stride: 12,
            format: LinuxPixelFormat::Bgra,
            crop: Some(Region {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            }),
        };
        let img = raw_frame_to_rgba(frame).unwrap();
        assert_eq!(img.as_raw(), &[30, 20, 10, 40, 70, 60, 50, 80]);
    }

    #[test]
    fn crop_outside_bounds_returns_invalid_config() {
        let data = vec![0u8; 48];
        let frame = LinuxRawFrame {
            data: &data,
            width: 3,
            height: 2,
            stride: 12,
            format: LinuxPixelFormat::Bgra,
            crop: Some(Region {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
            }),
        };
        match raw_frame_to_rgba(frame) {
            Err(CaptureError::InvalidConfig { .. }) => {}
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn empty_dimensions_returns_invalid_config() {
        let frame = make_frame(&[], 0, 0, 0, LinuxPixelFormat::Bgra);
        match raw_frame_to_rgba(frame) {
            Err(CaptureError::InvalidConfig { .. }) => {}
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn too_short_data_returns_invalid_config() {
        let frame = make_frame(&[0; 4], 2, 1, 8, LinuxPixelFormat::Bgra);
        match raw_frame_to_rgba(frame) {
            Err(CaptureError::InvalidConfig { .. }) => {}
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(debug_assertions, ignore)]
    fn four_k_bgrx_conversion_under_20ms() {
        let width = 3840u32;
        let height = 2160u32;
        let stride = width * 4;
        let data = vec![128u8; (stride * height) as usize];
        let frame = LinuxRawFrame {
            data: &data,
            width,
            height,
            stride,
            format: LinuxPixelFormat::Bgrx,
            crop: None,
        };
        let start = std::time::Instant::now();
        let _img = raw_frame_to_rgba(frame).unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 20,
            "4K BGRx conversion took {}ms, expected < 20ms",
            elapsed.as_millis()
        );
    }
}
