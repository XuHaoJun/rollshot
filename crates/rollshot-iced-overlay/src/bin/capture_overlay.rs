//! Standalone harness for the Phase 3 KDE 6 acceptance checks. Stands in for
//! Tauri: runs the overlay, then saves the finalized image as a PNG.

use rollshot_iced_overlay::{run_overlay, OverlayConfig};

fn main() {
    let backend = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "auto".to_string());
    // Higher fps shrinks the per-frame scroll motion, so the matcher keeps up and
    // its anchor keeps advancing (a too-big jump strands the anchor and freezes
    // live stitching until Esc). Overridable as arg 2 for experimenting.
    let fps = std::env::args()
        .nth(2)
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&f| f > 0)
        .unwrap_or(30);
    let config = OverlayConfig {
        backend,
        fps,
        show_cursor: false,
        initial_mode: rollshot_capture::CaptureMode::Scrolling,
        target_output_name: None,
    };

    // The matcher is the throughput bottleneck. In a debug build it is slow
    // enough that fast scrolling outruns it, strands the stitcher anchor, and
    // stalls the live preview. Release keeps up and stays smooth.
    if cfg!(debug_assertions) {
        eprintln!(
            "[overlay] debug build: fast scrolling can outrun the unoptimized \
             matcher and stall live stitching — run with --release for smooth capture."
        );
    }

    match run_overlay(config) {
        Ok(Some(result)) => {
            let out = "capture_overlay_result.png";
            match result.image.save(out) {
                Ok(()) => {
                    print!(
                        "saved {out}: {}x{}",
                        result.image.width(),
                        result.image.height()
                    );
                    if let Some(stats) = result.stats {
                        print!(" ({} frames)", stats.frame_count);
                    }
                    println!();
                }
                Err(e) => eprintln!("failed to save {out}: {e}"),
            }
        }
        Ok(None) => println!("cancelled"),
        Err(e) => eprintln!("overlay failed: {e}"),
    }
}
