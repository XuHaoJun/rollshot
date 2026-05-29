//! Standalone harness for the Phase 3 KDE 6 acceptance checks. Stands in for
//! Tauri: runs the overlay, then saves the finalized image as a PNG.

use rollshot_overlay::{run_overlay, OverlayConfig};

fn main() {
    let backend = std::env::args().nth(1).unwrap_or_else(|| "auto".to_string());
    let config = OverlayConfig {
        backend,
        fps: 5,
        show_cursor: false,
    };

    match run_overlay(config) {
        Ok(Some(result)) => {
            let out = "capture_overlay_result.png";
            match result.image.save(out) {
                Ok(()) => println!(
                    "saved {out}: {}x{} ({} frames)",
                    result.image.width(),
                    result.image.height(),
                    result.stats.frame_count
                ),
                Err(e) => eprintln!("failed to save {out}: {e}"),
            }
        }
        Ok(None) => println!("cancelled"),
        Err(e) => eprintln!("overlay failed: {e}"),
    }
}
