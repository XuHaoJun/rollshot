//! Direct fullscreen completion: one-shot capture straight to `CaptureResult`,
//! no selection overlay, no streaming/stitching. Routed *before* any overlay
//! state on both platforms — this module owns the shared completion; the two
//! platform entry points only decide whether to call it.
//!
//!   launch JSON: initial_mode
//!          │
//!          ├─ "fullscreen" ──┐
//!          │                 ▼
//!          │   ┌─────────────────────────────────────────────┐
//!          │   │ Linux:  linux_runner::run_initial_path       │
//!          │   │ macOS:  MacosProduct::new (initial_capture_  │
//!          │   │         path == Fullscreen)                  │
//!          │   └─────────────────────────────────────────────┘
//!          │                 │ both call
//!          │                 ▼
//!          │       fullscreen::capture(config)
//!          │                 │  from_fullscreen_environment → capture_once
//!          │                 ▼
//!          │       Ok(Some(CaptureResult{ stats: None }))   ── existing
//!          │       Ok(None)  on UserCancelled                  presentation
//!          │       Err(..)   on Unsupported / backend error    (Workspace /
//!          │                                                    thumbnail)
//!          └─ "region" | "scrolling" ─► overlay session (unchanged)
//!
use crate::{CaptureResult, OverlayConfig, OverlayError};
use rollshot_capture::{CaptureError, CaptureMode, OneShotCapture};

pub(crate) fn capture_with<F>(
    config: &OverlayConfig,
    capture_once: F,
) -> Result<Option<CaptureResult>, OverlayError>
where
    F: FnOnce(bool) -> Result<OneShotCapture, CaptureError>,
{
    if config.initial_mode != CaptureMode::Fullscreen {
        return Err(OverlayError::Capture(
            "direct fullscreen completion requires fullscreen mode".to_string(),
        ));
    }

    match capture_once(config.show_cursor) {
        Ok(capture) => Ok(Some(CaptureResult {
            image: capture.into_image(),
            stats: None,
        })),
        Err(CaptureError::UserCancelled) => Ok(None),
        Err(error) => Err(OverlayError::Capture(error.to_string())),
    }
}

pub fn capture(config: &OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    tracing::info!(
        target: crate::diagnostics::TARGET_OVERLAY,
        mode = ?config.initial_mode,
        backend = %config.backend,
        show_cursor = config.show_cursor,
        "direct fullscreen capture starting"
    );
    let kind = rollshot_capture::OneShotBackendKind::from_fullscreen_environment(&config.backend)
        .map_err(|error| OverlayError::Capture(error.to_string()))?;
    let result = capture_with(config, |show_cursor| kind.capture_once(show_cursor));
    tracing::info!(
        target: crate::diagnostics::TARGET_OVERLAY,
        outcome = match &result {
            Ok(Some(_)) => "completed",
            Ok(None) => "cancelled",
            Err(_) => "failed",
        },
        "direct fullscreen capture finished"
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{CaptureMode, DisplayTarget, Region, Size};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn config(mode: CaptureMode) -> OverlayConfig {
        OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            initial_mode: mode,
            target_output_name: None,
        }
    }

    fn one_shot() -> rollshot_capture::OneShotCapture {
        rollshot_capture::OneShotCapture::new(
            RgbaImage::from_pixel(2, 1, Rgba([1, 2, 3, 255])),
            DisplayTarget {
                output_name: Some("display".to_string()),
                logical_region: Region { x: 0, y: 0, width: 2, height: 1 },
                physical_size: Size { width: 2, height: 1 },
            },
        )
        .unwrap()
    }

    #[test]
    fn fullscreen_returns_the_unchanged_one_shot_image() {
        let result = capture_with(&config(CaptureMode::Fullscreen), |_| Ok(one_shot()))
            .unwrap()
            .unwrap();
        assert_eq!(result.image.dimensions(), (2, 1));
        assert_eq!(result.image.get_pixel(0, 0).0, [1, 2, 3, 255]);
        assert!(result.stats.is_none());
    }

    #[test]
    fn fullscreen_invokes_only_one_shot_acquisition() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        CALLS.store(0, Ordering::SeqCst);
        capture_with(&config(CaptureMode::Fullscreen), |_| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(one_shot())
        })
        .unwrap();
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cancellation_produces_no_result() {
        let result = capture_with(&config(CaptureMode::Fullscreen), |_| {
            Err(rollshot_capture::CaptureError::UserCancelled)
        })
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn non_fullscreen_mode_is_rejected() {
        let err = capture_with(&config(CaptureMode::Region), |_| Ok(one_shot())).unwrap_err();
        assert!(matches!(err, OverlayError::Capture(_)));
    }
}
