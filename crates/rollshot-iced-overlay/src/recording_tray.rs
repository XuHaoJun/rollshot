//! Temporary system-tray (SNI) item + notification used by the headless
//! fullscreen Action Guide runner to signal "finish recording".
//!
//! The runner is generic over [`RecordingTray`] so orchestration (ordering and
//! cleanup) is unit-tested on CI with a fake — CI has no SNI host or DBus.

use std::sync::mpsc::{Receiver, Sender};

use crate::OverlayError;

const TARGET_TRAY: &str = "rollshot::overlay::tray";

/// A temporary recording tray item. The concrete impl tears the item down on
/// `Drop`, so the runner gets RAII cleanup on every exit path.
pub(crate) trait RecordingTray: Send {
    /// Block the calling thread until the user activates (clicks) the tray item.
    fn wait_for_finish(&self);

    /// Update the tray title. Default is a no-op (for fake trays in tests).
    fn update_title(&self, _title: String) {}

    /// Update the tray tooltip. Default is a no-op.
    fn update_tooltip(&self, _title: String, _description: String) {}
}

impl RecordingTray for Box<dyn RecordingTray> {
    fn wait_for_finish(&self) {
        (**self).wait_for_finish()
    }
    fn update_title(&self, title: String) {
        (**self).update_title(title)
    }
    fn update_tooltip(&self, title: String, description: String) {
        (**self).update_tooltip(title, description)
    }
}

/// The ksni-backed tray item. `activate` (click) fires the finish channel.
struct RecordingItem {
    finish_tx: Sender<()>,
    title_override: Option<String>,
    tooltip_title: Option<String>,
    tooltip_description: Option<String>,
}

impl ksni::Tray for RecordingItem {
    fn id(&self) -> String {
        "rollshot-recording".into()
    }
    fn title(&self) -> String {
        self.title_override.clone().unwrap_or_else(|| "Rollshot is recording".into())
    }
    fn icon_name(&self) -> String {
        "media-record".into()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.tooltip_title.clone().unwrap_or_else(|| "Rollshot is recording \u{2014} click to finish".into()),
            description: self.tooltip_description.clone().unwrap_or_default(),
            icon_name: "media-record".into(),
            icon_pixmap: Vec::new(),
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        tracing::info!(target: TARGET_TRAY, "tray activated; finishing recording");
        let _ = self.finish_tx.send(());
    }
}

/// Owns the spawned ksni service; shuts it down on Drop (RAII cleanup).
struct KsniTray {
    finish_rx: Receiver<()>,
    handle: ksni::blocking::Handle<RecordingItem>,
}

impl RecordingTray for KsniTray {
    fn wait_for_finish(&self) {
        let _ = self.finish_rx.recv();
    }

    fn update_title(&self, title: String) {
        self.handle.update(|item: &mut RecordingItem| {
            item.title_override = Some(title);
        });
    }

    fn update_tooltip(&self, title: String, description: String) {
        self.handle.update(|item: &mut RecordingItem| {
            item.tooltip_title = Some(title);
            item.tooltip_description = Some(description);
        });
    }
}

impl Drop for KsniTray {
    fn drop(&mut self) {
        tracing::debug!(target: TARGET_TRAY, "tearing down tray item");
        self.handle.shutdown().wait();
    }
}

/// Create and register the temporary recording tray item.
pub(crate) fn create_recording_tray() -> Result<Box<dyn RecordingTray>, OverlayError> {
    if !rollshot_linux_desktop::sni_host_available() {
        return Err(OverlayError::Capture(
            "Fullscreen Action Guide requires a system tray. \
             This environment does not support tray icons."
                .to_string(),
        ));
    }
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let handle = ksni::blocking::TrayMethods::spawn(RecordingItem {
        finish_tx,
        title_override: None,
        tooltip_title: None,
        tooltip_description: None,
    })
        .map_err(|e| OverlayError::Capture(format!("failed to spawn tray service: {e}")))?;
    tracing::info!(target: TARGET_TRAY, "recording tray item registered");
    Ok(Box::new(KsniTray { finish_rx, handle }))
}

/// Best-effort transient "recording started" notification. Never aborts
/// recording: a failure is logged and swallowed.
pub(crate) fn notify_recording_started() {
    use notify_rust::{Hint, Notification, Timeout};
    let result = Notification::new()
        .summary("Rollshot is recording")
        .body("Click the tray icon to finish recording.")
        .icon("media-record")
        .hint(Hint::Transient(true))
        .timeout(Timeout::Milliseconds(4000))
        .show();
    if let Err(err) = result {
        tracing::warn!(target: TARGET_TRAY, %err, "recording notification failed (continuing)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct FakeTray {
        dropped: Arc<AtomicBool>,
        waited: Arc<AtomicBool>,
    }
    impl RecordingTray for FakeTray {
        fn wait_for_finish(&self) {
            self.waited.store(true, Ordering::SeqCst);
        }
    }
    impl Drop for FakeTray {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn fake_tray_drops_and_waits() {
        let dropped = Arc::new(AtomicBool::new(false));
        let waited = Arc::new(AtomicBool::new(false));
        {
            let tray: Box<dyn RecordingTray> = Box::new(FakeTray {
                dropped: dropped.clone(),
                waited: waited.clone(),
            });
            tray.wait_for_finish();
            assert!(waited.load(Ordering::SeqCst));
            assert!(
                !dropped.load(Ordering::SeqCst),
                "not dropped until scope end"
            );
        }
        assert!(dropped.load(Ordering::SeqCst), "dropped at scope end");
    }

    // ---- Motion indicator tray RED tests ----

    #[test]
    fn tray_update_title_is_no_op_on_fake() {
        // FakeTray implements default no-op update methods.
        let tray: Box<dyn RecordingTray> = Box::new(FakeTray {
            dropped: Arc::new(AtomicBool::new(false)),
            waited: Arc::new(AtomicBool::new(false)),
        });
        // Should not panic.
        tray.update_title("Motion recording on".into());
        tray.update_tooltip(
            "Motion recording on \u{2014} click to finish".into(),
            String::new(),
        );
        // Finish still works.
        tray.wait_for_finish();
    }
}
