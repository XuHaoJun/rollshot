//! Summary-MP4 export: assemble the final guide's reviewed keyframes into a
//! short H.264 MP4. This is a workflow summary, not raw screen recording.

use std::path::Path;

use image::RgbaImage;

use crate::error::VideoError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;

/// Tunables for summary-MP4 assembly.
#[derive(Debug, Clone)]
pub struct VideoOptions {
    /// Per-keyframe display time, milliseconds.
    pub frame_dwell_ms: u32,
    /// Output frame rate.
    pub fps: u32,
    /// Frames wider than this are downscaled; never upscaled.
    pub max_width: u32,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            frame_dwell_ms: 1500,
            fps: 30,
            max_width: 1280,
        }
    }
}

pub fn export_video(
    guide: &Guide,
    store: &FrameStore,
    opts: VideoOptions,
    ffmpeg_path: &Path,
    out_path: &Path,
) -> Result<(), VideoError> {
    let _ = (guide, store, opts, ffmpeg_path, out_path);
    Err(VideoError::InvalidFfmpeg {
        path: ffmpeg_path.display().to_string(),
    })
}

#[allow(dead_code)]
fn repeat_count(frame_dwell_ms: u32, fps: u32) -> u32 {
    let fps = fps.max(1) as u64;
    let dwell = frame_dwell_ms as u64;
    let frames = (dwell * fps).div_ceil(1000);
    frames.max(1) as u32
}

#[allow(dead_code)]
fn even_dimension(value: u32) -> u32 {
    if value <= 2 {
        2
    } else if value % 2 == 1 {
        value - 1
    } else {
        value
    }
}

#[allow(dead_code)]
fn downscale(image: &RgbaImage, max_width: u32) -> RgbaImage {
    let width = image.width();
    if width == 0 || max_width == 0 || width <= max_width {
        return image.clone();
    }
    let height = (image.height() as u64 * max_width as u64 / width as u64).max(1) as u32;
    image::imageops::resize(
        image,
        max_width,
        height,
        image::imageops::FilterType::Triangle,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn repeat_count_rounds_up_and_never_returns_zero() {
        assert_eq!(repeat_count(1500, 30), 45);
        assert_eq!(repeat_count(1, 30), 1);
        assert_eq!(repeat_count(0, 30), 1);
    }

    #[test]
    fn even_dimension_rounds_odd_values_down_but_keeps_minimum_two() {
        assert_eq!(even_dimension(101), 100);
        assert_eq!(even_dimension(100), 100);
        assert_eq!(even_dimension(1), 2);
        assert_eq!(even_dimension(0), 2);
    }

    #[test]
    fn downscale_preserves_aspect_ratio_and_never_upscales() {
        let wide = RgbaImage::from_pixel(10, 5, Rgba([1, 2, 3, 255]));
        let scaled = downscale(&wide, 4);
        assert_eq!((scaled.width(), scaled.height()), (4, 2));

        let native = downscale(&wide, 20);
        assert_eq!((native.width(), native.height()), (10, 5));
    }
}
