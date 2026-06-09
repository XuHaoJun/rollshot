use rollshot_capture::one_shot::OneShotCapture;
use rollshot_capture::{crop_image, Size};

use crate::coords::{map_crop_to_frame, LogicalRect};
use crate::CaptureResult;

pub fn finish_screenshot(
    capture: &OneShotCapture,
    crop: LogicalRect,
    overlay_logical: Size,
) -> Result<CaptureResult, String> {
    let region = map_crop_to_frame(
        crop,
        overlay_logical,
        capture.target_display().physical_size,
    );
    let image = crop_image(capture.image(), region).map_err(|e| e.to_string())?;
    Ok(CaptureResult { image, stats: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{DisplayTarget, Region as CaptureRegion, Size};

    fn test_capture() -> OneShotCapture {
        let mut img = RgbaImage::new(200, 200);
        for y in 0..200 {
            for x in 0..200 {
                img.put_pixel(x, y, Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
            }
        }
        OneShotCapture::new(
            img,
            DisplayTarget {
                output_name: Some("test".to_string()),
                logical_region: CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 200,
                },
                physical_size: Size {
                    width: 200,
                    height: 200,
                },
            },
        )
        .expect("test capture")
    }

    #[test]
    fn finish_screenshot_returns_result_with_none_stats() {
        let capture = test_capture();
        let crop = LogicalRect {
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
        };
        let overlay_logical = Size {
            width: 200,
            height: 200,
        };

        let result = finish_screenshot(&capture, crop, overlay_logical).expect("screenshot ok");

        assert!(result.stats.is_none());
        assert_eq!(result.image.width(), 50);
        assert_eq!(result.image.height(), 50);
    }

    #[test]
    fn finish_screenshot_maps_crop_to_frame_coordinates() {
        let capture = test_capture();
        let crop = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let overlay_logical = Size {
            width: 200,
            height: 200,
        };

        let result = finish_screenshot(&capture, crop, overlay_logical).expect("screenshot ok");

        assert_eq!(result.image.width(), 100);
        assert_eq!(result.image.height(), 100);
    }

    #[test]
    fn finish_screenshot_scales_at_2x() {
        let mut img = RgbaImage::new(400, 400);
        for y in 0..400 {
            for x in 0..400 {
                img.put_pixel(x, y, Rgba([100, 150, 200, 255]));
            }
        }
        let capture = OneShotCapture::new(
            img,
            DisplayTarget {
                output_name: Some("2x".to_string()),
                logical_region: CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 200,
                },
                physical_size: Size {
                    width: 400,
                    height: 400,
                },
            },
        )
        .expect("2x capture");

        let crop = LogicalRect {
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
        };
        let overlay_logical = Size {
            width: 200,
            height: 200,
        };

        let result = finish_screenshot(&capture, crop, overlay_logical).expect("screenshot ok");

        assert!(result.stats.is_none());
        assert_eq!(result.image.width(), 100);
        assert_eq!(result.image.height(), 100);
    }

    #[test]
    fn finish_screenshot_rejects_empty_crop() {
        let capture = test_capture();
        let crop = LogicalRect {
            x: 10.0,
            y: 10.0,
            width: 0.0,
            height: 0.0,
        };
        let overlay_logical = Size {
            width: 200,
            height: 200,
        };

        let err = finish_screenshot(&capture, crop, overlay_logical).expect_err("empty crop");
        assert!(err.contains("non-zero"), "err = {err}");
    }
}
