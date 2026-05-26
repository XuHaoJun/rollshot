use image::RgbaImage;

use crate::{CaptureError, CapturedFrame, FrameMetadata, Region};

pub fn crop_frame(frame: &CapturedFrame, region: Region) -> Result<CapturedFrame, CaptureError> {
    if region.x < 0 || region.y < 0 || region.width == 0 || region.height == 0 {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "crop region x={},y={},w={},h={} must have non-negative origin and non-zero size",
                region.x, region.y, region.width, region.height
            ),
        });
    }

    let right = region.x as u64 + region.width as u64;
    let bottom = region.y as u64 + region.height as u64;
    if right > frame.image.width() as u64 || bottom > frame.image.height() as u64 {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "crop region x={},y={},w={},h={} is outside frame bounds {}x{}",
                region.x,
                region.y,
                region.width,
                region.height,
                frame.image.width(),
                frame.image.height()
            ),
        });
    }

    let cropped = image::imageops::crop_imm(
        &frame.image,
        region.x as u32,
        region.y as u32,
        region.width,
        region.height,
    )
    .to_image();

    let mut metadata: FrameMetadata = frame.metadata.clone();
    metadata.effective_region = Some(region);
    metadata.stride = Some(region.width.saturating_mul(4));

    Ok(CapturedFrame {
        image: RgbaImage::from(cropped),
        timestamp: frame.timestamp,
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::crop_frame;
    use crate::{CapturedFrame, FrameMetadata, PixelFormat, Region, Size};
    use image::{Rgba, RgbaImage};
    use std::time::SystemTime;

    fn test_frame() -> CapturedFrame {
        let mut image = RgbaImage::new(4, 3);
        for y in 0..3 {
            for x in 0..4 {
                image.put_pixel(x, y, Rgba([x as u8, y as u8, 200, 255]));
            }
        }

        CapturedFrame {
            image,
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata {
                source_size: Some(Size {
                    width: 4,
                    height: 3,
                }),
                effective_region: None,
                pixel_format: Some(PixelFormat::Rgba),
                stride: Some(16),
                backend: "fake",
            },
        }
    }

    #[test]
    fn crop_frame_returns_selected_source_pixels() {
        let cropped = crop_frame(
            &test_frame(),
            Region {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .expect("crop succeeds");

        assert_eq!(cropped.image.dimensions(), (2, 2));
        assert_eq!(*cropped.image.get_pixel(0, 0), Rgba([1, 1, 200, 255]));
        assert_eq!(*cropped.image.get_pixel(1, 1), Rgba([2, 2, 200, 255]));
        assert_eq!(
            cropped.metadata.effective_region,
            Some(Region {
                x: 1,
                y: 1,
                width: 2,
                height: 2
            })
        );
        assert_eq!(cropped.metadata.stride, Some(8));
    }

    #[test]
    fn crop_frame_rejects_negative_origin() {
        let err = crop_frame(
            &test_frame(),
            Region {
                x: -1,
                y: 0,
                width: 2,
                height: 2,
            },
        )
        .expect_err("negative origin rejected");

        assert!(err.to_string().contains("non-negative"), "err = {err}");
    }

    #[test]
    fn crop_frame_rejects_out_of_bounds_region() {
        let err = crop_frame(
            &test_frame(),
            Region {
                x: 3,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .expect_err("outside region rejected");

        assert!(
            err.to_string().contains("outside frame bounds"),
            "err = {err}"
        );
    }
}
