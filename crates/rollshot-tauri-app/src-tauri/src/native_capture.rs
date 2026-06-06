use std::sync::Arc;
use std::thread::JoinHandle;

use rollshot_capture::InteractiveLaunchOptions;
use rollshot_iced_overlay::{run_overlay, CaptureResult, OverlayConfig, OverlayError};

use crate::session::{DoneImageDto, SharedSession};

#[tauri::command]
pub fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

type OverlayOutcome = Result<Option<CaptureResult>, OverlayError>;

/// Minimum capture fps for the native overlay path. The live stitcher is
/// throughput-sensitive: in a debug build, lower fps lets fast scrolling outrun
/// the matcher and stall the live stitch (Phase 3 finding). The CLI/launch
/// default (`fps = 5`) predates the native overlay, so floor it here. This does
/// not affect the Windows/macOS webview path.
const NATIVE_OVERLAY_MIN_FPS: u32 = 30;

/// Build the native overlay config from the launch options, flooring fps so the
/// live stitch stays smooth.
fn overlay_config(options: &InteractiveLaunchOptions) -> OverlayConfig {
    OverlayConfig {
        backend: options.backend.clone(),
        fps: options.fps.max(NATIVE_OVERLAY_MIN_FPS),
        show_cursor: options.show_cursor,
        initial_mode: options.initial_mode,
    }
}

/// Whether this build uses the native Wayland layer-shell overlay (Linux) or
/// the webview capture UI (Windows/macOS). Drives the frontend's top-level
/// branch instead of a JS platform check.
#[tauri::command]
pub fn uses_native_overlay() -> bool {
    cfg!(target_os = "linux")
}

async fn wait_for_overlay_thread(
    handle: JoinHandle<()>,
    rx: tokio::sync::oneshot::Receiver<OverlayOutcome>,
) -> Result<OverlayOutcome, String> {
    let outcome = rx
        .await
        .map_err(|_| "native overlay thread ended without a result".to_string());

    let join_result = handle.join();
    if join_result.is_err() {
        return Err("native overlay thread panicked".to_string());
    }

    outcome
}

fn store_overlay_outcome(
    session: &SharedSession,
    outcome: OverlayOutcome,
) -> Result<Option<DoneImageDto>, String> {
    match outcome {
        Ok(Some(result)) => {
            let done = session.store_capture_result(result.image, result.stats)?;
            Ok(Some(done))
        }
        Ok(None) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

/// Linux save handoff (Phase 4): run the native layer-shell overlay to capture
/// + stitch, then store the finalized image as the session's final image so the
/// existing save flow can write it. `run_overlay` blocks its thread for the
/// whole session, so it runs on a dedicated thread and the result is awaited
/// without blocking the async runtime. Named generically (spec D5): this is the
/// capture handoff, not "save PNG".
#[tauri::command]
pub async fn run_native_capture(
    session: tauri::State<'_, Arc<SharedSession>>,
    options: InteractiveLaunchOptions,
) -> Result<Option<DoneImageDto>, String> {
    let config = overlay_config(&options);
    let (tx, rx) = tokio::sync::oneshot::channel();

    // run_overlay blocks (portal negotiation + first-frame wait + iced loop);
    // a dedicated std::thread keeps the Tauri async worker free.
    let handle = std::thread::spawn(move || {
        let _ = tx.send(run_overlay(config));
    });

    // The overlay returned or the worker failed; join either way so the host
    // never orphans it (roadmap Phase 4 thread-cleanup item).
    let outcome = wait_for_overlay_thread(handle, rx).await?;
    store_overlay_outcome(&session, outcome)
}

#[cfg(test)]
mod tests {
    use super::{
        overlay_config, store_overlay_outcome, uses_native_overlay, wait_for_overlay_thread,
    };
    use image::{Rgba, RgbaImage};
    use rollshot_capture::InteractiveLaunchOptions;
    use rollshot_core::StitchStats;
    use rollshot_iced_overlay::{CaptureResult, OverlayError};

    use crate::session::SharedSession;

    #[test]
    fn overlay_config_floors_fps_at_30() {
        let config = overlay_config(&InteractiveLaunchOptions {
            backend: "linux-portal".to_string(),
            fps: 5,
            show_cursor: true,
            overlay_mode: rollshot_capture::OverlayMode::Auto,
            initial_mode: rollshot_capture::CaptureMode::Scrolling,
        });
        assert_eq!(config.backend, "linux-portal");
        assert_eq!(config.fps, 30);
        assert!(config.show_cursor);
        assert_eq!(
            config.initial_mode,
            rollshot_capture::CaptureMode::Scrolling
        );
    }

    #[test]
    fn overlay_config_keeps_higher_fps() {
        let config = overlay_config(&InteractiveLaunchOptions {
            backend: "auto".to_string(),
            fps: 60,
            show_cursor: false,
            overlay_mode: rollshot_capture::OverlayMode::Auto,
            initial_mode: rollshot_capture::CaptureMode::Screenshot,
        });
        assert_eq!(config.fps, 60);
        assert_eq!(config.backend, "auto");
        assert!(!config.show_cursor);
        assert_eq!(
            config.initial_mode,
            rollshot_capture::CaptureMode::Screenshot
        );
    }

    #[test]
    fn uses_native_overlay_matches_target_os() {
        assert_eq!(uses_native_overlay(), cfg!(target_os = "linux"));
    }

    #[test]
    fn store_overlay_outcome_stores_finished_capture() {
        let session = SharedSession::new();
        let done = store_overlay_outcome(
            &session,
            Ok(Some(CaptureResult {
                image: RgbaImage::from_pixel(20, 30, Rgba([4, 5, 6, 255])),
                stats: StitchStats::default(),
            })),
        )
        .expect("store outcome")
        .expect("finished capture");

        assert_eq!(done.image_width, 20);
        assert_eq!(done.image_height, 30);
    }

    #[test]
    fn store_overlay_outcome_preserves_cancel() {
        let session = SharedSession::new();
        let done = store_overlay_outcome(&session, Ok(None)).expect("cancel outcome");
        assert_eq!(done, None);
    }

    #[test]
    fn store_overlay_outcome_maps_overlay_error() {
        let session = SharedSession::new();
        let err = store_overlay_outcome(&session, Err(OverlayError::Overlay("boom".to_string())))
            .expect_err("overlay error");
        assert!(err.contains("overlay error: boom"), "err = {err}");
    }

    #[test]
    fn wait_for_overlay_thread_reports_panic() {
        let (tx, rx) = tokio::sync::oneshot::channel::<super::OverlayOutcome>();
        let handle = std::thread::spawn(move || {
            drop(tx);
            panic!("overlay panic for test");
        });

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let err = runtime
            .block_on(wait_for_overlay_thread(handle, rx))
            .expect_err("panic should be reported");

        assert!(err.contains("panicked"), "err = {err}");
    }
}
