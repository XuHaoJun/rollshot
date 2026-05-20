//! Shared helpers for CLI integration tests. Each test file declares
//! `mod common;` to pull these in.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use image::{imageops, Rgba, RgbaImage};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

pub fn temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "rollshot-cli-{label}-{nanos}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[allow(dead_code)]
pub fn command_output(command: &mut Command) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn command");
    let started = Instant::now();

    loop {
        if child.try_wait().expect("poll command").is_some() {
            return child.wait_with_output().expect("collect command output");
        }

        if started.elapsed() >= COMMAND_TIMEOUT {
            let _ = child.kill();
            let output = child.wait_with_output().expect("collect timed-out output");
            panic!(
                "command timed out after {:?}\nstdout: {}\nstderr: {}",
                COMMAND_TIMEOUT,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

#[allow(dead_code)]
pub fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
    for y in (0..height).step_by(36) {
        let accent = ((y / 3) % 180) as u8;
        for x in 24..width.saturating_sub(24) {
            let stripe = if (x / 7 + y / 11) % 2 == 0 { 220 } else { 180 };
            img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
            if y + 1 < height {
                img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
            }
        }
    }
    for col in [21u32, 47, 73, 99, 125] {
        if col >= width {
            continue;
        }
        for y in 12..height.saturating_sub(12) {
            if (y / 13) % 3 != 0 {
                img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
            }
        }
    }
    img
}

#[allow(dead_code)]
pub fn write_scroll_fixture(dir: &Path) {
    let canvas = make_scroll_canvas(160, 600);
    for (idx, y) in [0u32, 40, 80, 120].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
        frame
            .save(dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }
}
