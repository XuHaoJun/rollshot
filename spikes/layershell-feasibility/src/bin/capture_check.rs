use std::time::Duration;

use rollshot_capture::backend::default_backend;
use rollshot_capture::types::CaptureOptions;

const SENTINEL_R: u8 = 255;
const SENTINEL_G: u8 = 0;
const SENTINEL_B: u8 = 255;
const SENTINEL_A: u8 = 255;

fn main() {
    // 1. Spawn the overlay on a background thread
    let (_tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("overlay".into())
        .spawn(move || {
            let _ = overlay_app::run(iced_layershell::settings::StartMode::Active, rx);
        })
        .expect("spawn overlay thread");

    // Give the overlay time to appear
    std::thread::sleep(Duration::from_secs(2));

    // 2. Capture a single frame via the portal backend
    eprintln!("capture_check: starting portal capture");
    let kind = default_backend();
    let mut backend = match kind.create() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("capture_check: backend unavailable: {e}");
            eprintln!("capture_check: SKIP (no Wayland portal session)");
            std::process::exit(0);
        }
    };

    let mut stream = match backend.start(CaptureOptions::default()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("capture_check: start failed: {e}");
            eprintln!("capture_check: SKIP (portal start failed)");
            std::process::exit(0);
        }
    };

    let frame = match stream.next_frame() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("capture_check: next_frame failed: {e}");
            std::process::exit(1);
        }
    };

    eprintln!(
        "capture_check: frame {}x{}, backend={}",
        frame.image.width(),
        frame.image.height(),
        frame.metadata.backend
    );

    // 3. R3 — scan for sentinel magenta pixels
    let sentinel_count = frame
        .image
        .pixels()
        .filter(|p| p[0] == SENTINEL_R && p[1] == SENTINEL_G && p[2] == SENTINEL_B && p[3] == SENTINEL_A)
        .count();

    println!("R3 sentinel_pixels: {sentinel_count}");
    if sentinel_count > 0 {
        println!("R3 self-capture: PASS (overlay toolbar visible in captured frame)");
    } else {
        println!("R3 self-capture: FAIL (no sentinel pixels found)");
    }

    // 4. R4 — report frame metadata for fractional scaling analysis
    if let Some(size) = frame.metadata.source_size {
        println!("R4 source_size: {}x{}", size.width, size.height);
    }
    if let Some(region) = frame.metadata.effective_region {
        println!(
            "R4 effective_region: x={}, y={}, w={}, h={}",
            region.x, region.y, region.width, region.height
        );
    }
    println!("R4 frame_pixels: {}x{}", frame.image.width(), frame.image.height());
    println!("R4 note: coordinate mapping between overlay logical coords and frame pixel coords requires output scale factor");

    // 5. R5 — output match
    println!("R5 pixel_format: {:?}", frame.metadata.pixel_format);
    println!("R5 stride: {:?}", frame.metadata.stride);
    println!("R5 backend: {}", frame.metadata.backend);

    // 6. Save frame as PNG
    let out_path = "capture_check_frame.png";
    if let Err(e) = frame.image.save(out_path) {
        eprintln!("capture_check: failed to save {out_path}: {e}");
    } else {
        println!("saved: {out_path}");
    }
}

#[path = "../overlay_app.rs"]
mod overlay_app;
