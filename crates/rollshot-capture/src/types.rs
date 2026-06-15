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

/// WHAT we do with the captured frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Workflow {
    Screenshot,
    #[default]
    Scrolling,
}

/// WHAT AREA we capture. Resolves down to the backend `RegionMode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureScope {
    #[default]
    Region,
    Fullscreen,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CaptureRequest {
    pub workflow: Workflow,
    pub scope: CaptureScope,
}

impl CaptureRequest {
    pub const fn screenshot_region() -> Self {
        Self {
            workflow: Workflow::Screenshot,
            scope: CaptureScope::Region,
        }
    }
    pub const fn screenshot_fullscreen() -> Self {
        Self {
            workflow: Workflow::Screenshot,
            scope: CaptureScope::Fullscreen,
        }
    }
    pub const fn scrolling_region() -> Self {
        Self {
            workflow: Workflow::Scrolling,
            scope: CaptureScope::Region,
        }
    }

    /// Region scope uses the selection overlay; Fullscreen captures directly.
    pub fn needs_overlay(&self) -> bool {
        matches!(self.scope, CaptureScope::Region)
    }

    /// `Scrolling × Fullscreen` is expressible but not wired in this refactor.
    pub fn is_supported(&self) -> bool {
        !matches!(
            (self.workflow, self.scope),
            (Workflow::Scrolling, CaptureScope::Fullscreen)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InteractiveLaunchOptions {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
    #[serde(default)]
    pub initial_request: CaptureRequest,
}

impl InteractiveLaunchOptions {
    pub fn default_capture() -> Self {
        Self {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            initial_request: CaptureRequest::scrolling_region(),
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
    use super::{CaptureOptions, CaptureRequest, InteractiveLaunchOptions};

    #[test]
    fn interactive_launch_options_round_trip_json() {
        let options = InteractiveLaunchOptions {
            backend: "linux-portal".to_string(),
            fps: 7,
            show_cursor: true,
            initial_request: CaptureRequest::screenshot_region(),
        };
        let json = serde_json::to_string(&options).expect("serialize");
        assert!(
            json.contains(r#""initial_request":{"workflow":"screenshot","scope":"region"}"#),
            "json = {json}"
        );
        let decoded: InteractiveLaunchOptions = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, options);
    }

    #[test]
    fn interactive_launch_options_default_initial_request() {
        let json = r#"{"backend":"auto","fps":5,"show_cursor":false}"#;
        let decoded: InteractiveLaunchOptions = serde_json::from_str(json).expect("deserialize");
        assert_eq!(decoded.initial_request, CaptureRequest::scrolling_region());
    }

    #[test]
    fn interactive_launch_options_ignore_obsolete_field() {
        let obsolete_field = concat!("overlay", "_mode");
        let json = format!(
            r#"{{"backend":"auto","fps":5,"show_cursor":false,"{obsolete_field}":"legacy"}}"#
        );
        let decoded: InteractiveLaunchOptions =
            serde_json::from_str(&json).expect("deserialize payload with obsolete field");

        assert_eq!(decoded.initial_request, CaptureRequest::scrolling_region());
    }

    #[test]
    fn fps_change_does_not_affect_initial_request() {
        let mut opts = InteractiveLaunchOptions::default_capture();
        opts.fps = 60;
        assert_eq!(opts.initial_request, CaptureRequest::scrolling_region());
    }

    #[test]
    fn capture_options_default_has_no_target_output_name() {
        assert_eq!(CaptureOptions::default().target_output_name, None);
    }

    use super::{CaptureScope, Workflow};

    #[test]
    fn capture_request_default_is_scrolling_region() {
        let r = CaptureRequest::default();
        assert_eq!(r.workflow, Workflow::Scrolling);
        assert_eq!(r.scope, CaptureScope::Region);
    }

    #[test]
    fn needs_overlay_matches_region_scope() {
        assert!(CaptureRequest::screenshot_region().needs_overlay());
        assert!(CaptureRequest::scrolling_region().needs_overlay());
        assert!(!CaptureRequest::screenshot_fullscreen().needs_overlay());
    }

    #[test]
    fn is_supported_rejects_scrolling_fullscreen() {
        let bad = CaptureRequest {
            workflow: Workflow::Scrolling,
            scope: CaptureScope::Fullscreen,
        };
        assert!(!bad.is_supported());
        for r in [
            CaptureRequest::screenshot_region(),
            CaptureRequest::screenshot_fullscreen(),
            CaptureRequest::scrolling_region(),
        ] {
            assert!(r.is_supported());
        }
    }

    #[test]
    fn capture_request_serde_round_trips_all_combinations() {
        for (request, expected) in [
            (
                CaptureRequest::screenshot_region(),
                r#"{"workflow":"screenshot","scope":"region"}"#,
            ),
            (
                CaptureRequest::screenshot_fullscreen(),
                r#"{"workflow":"screenshot","scope":"fullscreen"}"#,
            ),
            (
                CaptureRequest::scrolling_region(),
                r#"{"workflow":"scrolling","scope":"region"}"#,
            ),
            (
                CaptureRequest {
                    workflow: Workflow::Scrolling,
                    scope: CaptureScope::Fullscreen,
                },
                r#"{"workflow":"scrolling","scope":"fullscreen"}"#,
            ),
        ] {
            let json = serde_json::to_string(&request).unwrap();
            assert_eq!(json, expected);
            assert_eq!(
                serde_json::from_str::<CaptureRequest>(&json).unwrap(),
                request
            );
        }
    }
}
