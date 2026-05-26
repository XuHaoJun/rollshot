use std::time::SystemTime;

use image::RgbaImage;

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub image: RgbaImage,
    pub timestamp: SystemTime,
    pub metadata: FrameMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOptions {
    pub region: RegionMode,
    pub fps: u32,
    pub show_cursor: bool,
    pub prefer_portal_region: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            region: RegionMode::FullSource,
            fps: 5,
            show_cursor: false,
            prefer_portal_region: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InteractiveLaunchOptions {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureProbe {
    pub backend: &'static str,
    pub available: bool,
    pub message: String,
    pub details: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameMetadata {
    pub source_size: Option<Size>,
    pub effective_region: Option<Region>,
    pub pixel_format: Option<PixelFormat>,
    pub stride: Option<u32>,
    pub backend: &'static str,
}

impl FrameMetadata {
    pub fn fake() -> Self {
        Self {
            source_size: None,
            effective_region: None,
            pixel_format: Some(PixelFormat::Rgba),
            stride: None,
            backend: "fake",
        }
    }

    pub fn fixture() -> Self {
        Self {
            source_size: None,
            effective_region: None,
            pixel_format: Some(PixelFormat::Rgba),
            stride: None,
            backend: "fixture",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionMode {
    Manual(Region),
    PortalPicker,
    FullSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba,
    Bgra,
    Bgrx,
    Rgbx,
    Rgb,
}

#[cfg(test)]
mod tests {
    use super::InteractiveLaunchOptions;

    #[test]
    fn interactive_launch_options_round_trip_json() {
        let options = InteractiveLaunchOptions {
            backend: "linux-portal".to_string(),
            fps: 7,
            show_cursor: true,
        };

        let json = serde_json::to_string(&options).expect("serialize launch options");
        assert!(
            json.contains("\"backend\":\"linux-portal\""),
            "json = {json}"
        );

        let decoded: InteractiveLaunchOptions =
            serde_json::from_str(&json).expect("deserialize launch options");
        assert_eq!(decoded, options);
    }
}
