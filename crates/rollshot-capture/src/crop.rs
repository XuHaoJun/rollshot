use image::RgbaImage;

use crate::{CaptureError, CapturedFrame, FrameMetadata, Region};

pub fn crop_image(image: &RgbaImage, region: Region) -> Result<RgbaImage, CaptureError> {
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
    if right > image.width() as u64 || bottom > image.height() as u64 {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "crop region x={},y={},w={},h={} is outside frame bounds {}x{}",
                region.x,
                region.y,
                region.width,
                region.height,
                image.width(),
                image.height()
            ),
        });
    }

    Ok(image::imageops::crop_imm(
        image,
        region.x as u32,
        region.y as u32,
        region.width,
        region.height,
    )
    .to_image())
}

pub fn crop_frame(frame: &CapturedFrame, region: Region) -> Result<CapturedFrame, CaptureError> {
    let cropped = crop_image(&frame.image, region)?;

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
    use super::{crop_frame, crop_image};
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

    fn test_image() -> RgbaImage {
        test_frame().image
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

    #[test]
    fn crop_image_returns_selected_pixels() {
        let img = test_image();
        let cropped = crop_image(
            &img,
            Region {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .expect("crop succeeds");

        assert_eq!(cropped.dimensions(), (2, 2));
        assert_eq!(*cropped.get_pixel(0, 0), Rgba([1, 1, 200, 255]));
        assert_eq!(*cropped.get_pixel(1, 1), Rgba([2, 2, 200, 255]));
    }

    #[test]
    fn crop_image_rejects_negative_origin() {
        let img = test_image();
        let err = crop_image(
            &img,
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
    fn crop_image_rejects_out_of_bounds_region() {
        let img = test_image();
        let err = crop_image(
            &img,
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

    #[test]
    fn crop_frame_delegates_to_crop_image_for_bounds_validation() {
        let frame = test_frame();
        let region = Region {
            x: 3,
            y: 1,
            width: 2,
            height: 2,
        };
        let frame_err = crop_frame(&frame, region).expect_err("frame crop rejects");
        let img_err = crop_image(&frame.image, region).expect_err("image crop rejects");
        assert_eq!(frame_err.to_string(), img_err.to_string());
    }
}
