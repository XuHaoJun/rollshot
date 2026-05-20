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

pub fn raw_frame_to_rgba(_frame: LinuxRawFrame<'_>) -> Result<RgbaImage, CaptureError> {
    Err(CaptureError::NotImplemented {
        backend: "linux-portal-pixel",
    })
}
