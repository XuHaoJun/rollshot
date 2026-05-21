use anyhow::anyhow;
use image::RgbaImage;
use std::time::SystemTime;

use crate::error::CaptureError;
use crate::types::{CapturedFrame, FrameMetadata, PixelFormat, Region, Size};

use super::BACKEND_NAME;

pub(super) fn captured_frame_from_bgra(
    frame: scap::frame::BGRAFrame,
    effective_region: Option<Region>,
) -> Result<CapturedFrame, CaptureError> {
    let width = u32::try_from(frame.width).map_err(|_| {
        CaptureError::Backend(anyhow!(
            "invalid negative BGRA frame width: {}",
            frame.width
        ))
    })?;
    let height = u32::try_from(frame.height).map_err(|_| {
        CaptureError::Backend(anyhow!(
            "invalid negative BGRA frame height: {}",
            frame.height
        ))
    })?;
    let image = bgra_to_rgba_image(width, height, &frame.data)?;

    Ok(CapturedFrame {
        image,
        timestamp: SystemTime::now(),
        metadata: FrameMetadata {
            source_size: Some(Size { width, height }),
            effective_region,
            pixel_format: Some(PixelFormat::Bgra),
            stride: Some(width * 4),
            backend: BACKEND_NAME,
        },
    })
}

fn bgra_to_rgba_image(width: u32, height: u32, data: &[u8]) -> Result<RgbaImage, CaptureError> {
    if width == 0 || height == 0 {
        return Err(CaptureError::Backend(anyhow!(
            "BGRA frame has empty dimensions: {width}x{height}"
        )));
    }

    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            CaptureError::Backend(anyhow!("BGRA frame dimensions overflow: {width}x{height}"))
        })?;

    if data.len() != expected_len {
        return Err(CaptureError::Backend(anyhow!(
            "BGRA frame length mismatch: got {}, expected {} for {}x{}",
            data.len(),
            expected_len,
            width,
            height
        )));
    }

    let mut rgba = vec![0; data.len()];
    for (src, dst) in data.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| CaptureError::Backend(anyhow!("failed to create RGBA image")))
}

#[cfg(test)]
mod tests {
    use super::bgra_to_rgba_image;

    #[test]
    fn bgra_to_rgba_swaps_blue_and_red_channels() {
        let image = bgra_to_rgba_image(2, 1, &[10, 20, 30, 255, 1, 2, 3, 4]).expect("valid image");
        assert_eq!(image.as_raw(), &[30, 20, 10, 255, 3, 2, 1, 4]);
    }

    #[test]
    fn bgra_to_rgba_rejects_invalid_length() {
        let err = bgra_to_rgba_image(2, 1, &[1, 2, 3, 4]).expect_err("invalid length");
        assert!(err.to_string().contains("length mismatch"));
    }

    #[test]
    fn bgra_to_rgba_rejects_empty_dimensions() {
        let err = bgra_to_rgba_image(0, 1, &[]).expect_err("empty width");
        assert!(err.to_string().contains("empty dimensions"));
    }
}
