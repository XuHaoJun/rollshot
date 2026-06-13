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
    /// Capture this specific display (macOS `CGDirectDisplayID`). `None` lets
    /// the backend pick (macOS falls back to the display under the cursor).
    /// Ignored by backends with their own source selection (Linux portal).
    pub target_display_id: Option<u32>,
    /// Wayland output name selected by a platform host. Linux KWin uses this to
    /// bind the same output as the selection overlay. Other backends ignore it.
    pub target_output_name: Option<String>,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            region: RegionMode::FullSource,
            fps: 5,
            show_cursor: false,
            prefer_portal_region: true,
            target_display_id: None,
            target_output_name: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureMode {
    #[default]
    Scrolling,
    Screenshot,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InteractiveLaunchOptions {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
    #[serde(default)]
    pub initial_mode: CaptureMode,
}

impl InteractiveLaunchOptions {
    pub fn default_capture() -> Self {
        Self {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            initial_mode: CaptureMode::Scrolling,
        }
    }
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
    use super::{CaptureMode, CaptureOptions, InteractiveLaunchOptions};

    #[test]
    fn interactive_launch_options_round_trip_json() {
        let options = InteractiveLaunchOptions {
            backend: "linux-portal".to_string(),
            fps: 7,
            show_cursor: true,
            initial_mode: CaptureMode::Screenshot,
        };

        let json = serde_json::to_string(&options).expect("serialize launch options");
        assert!(
            json.contains("\"backend\":\"linux-portal\""),
            "json = {json}"
        );
        assert!(
            json.contains("\"initial_mode\":\"screenshot\""),
            "json = {json}"
        );
        let obsolete_field = concat!("overlay", "_mode");
        assert!(!json.contains(obsolete_field), "json = {json}");

        let decoded: InteractiveLaunchOptions =
            serde_json::from_str(&json).expect("deserialize launch options");
        assert_eq!(decoded, options);
    }

    #[test]
    fn interactive_launch_options_ignore_obsolete_field() {
        let obsolete_field = concat!("overlay", "_mode");
        let json = format!(
            r#"{{"backend":"auto","fps":5,"show_cursor":false,"{obsolete_field}":"legacy"}}"#
        );
        let decoded: InteractiveLaunchOptions =
            serde_json::from_str(&json).expect("deserialize payload with obsolete field");

        assert_eq!(decoded.initial_mode, CaptureMode::Scrolling);
    }

    #[test]
    fn fps_change_does_not_affect_initial_mode() {
        let mut opts = InteractiveLaunchOptions::default_capture();
        opts.fps = 60;
        assert_eq!(opts.initial_mode, CaptureMode::Scrolling);
    }

    #[test]
    fn capture_options_default_has_no_target_output_name() {
        assert_eq!(CaptureOptions::default().target_output_name, None);
    }
}
