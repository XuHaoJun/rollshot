use std::path::PathBuf;
use std::process::Command;

use image::{imageops, Rgba, RgbaImage};

#[test]
fn rollshot_dev_probe_binary_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("probe")
        .output()
        .expect("run rollshot-dev probe");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("probe"), "stdout = {stdout}");
}

#[test]
fn rollshot_dev_stitch_folder_writes_png() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_scroll_canvas(160, 600);
    for (idx, y) in [0u32, 40, 80, 120].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
        let path = frames_dir.join(format!("frame_{:03}.png", idx));
        frame.save(&path).expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .output()
        .expect("run rollshot-dev stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("stitch-folder"), "stdout = {stdout}");
    assert!(stdout.contains("input frames: 4"), "stdout = {stdout}");
    assert!(stdout.contains("appended:"), "stdout = {stdout}");
    assert!(stdout.contains(output_png.to_string_lossy().as_ref()));

    assert!(output_png.exists(), "{} should exist", output_png.display());
    let stitched = image::open(&output_png)
        .expect("decode stitched png")
        .to_rgba8();
    assert_eq!(stitched.width(), 160);
    assert!(stitched.height() > 160, "height = {}", stitched.height());

    let _ = std::fs::remove_dir_all(&tempdir);
}

fn tempdir_for_test(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("{label}-{nanos}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
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

fn make_axis_debug_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 17 + y * 31 + (x / 13) * 19) % 255) as u8;
            let g = ((x * 43 + y * 11 + (y / 17) * 23) % 255) as u8;
            let b = ((x * 7 + y * 53 + (x / 29 + y / 31) * 41) % 255) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
    img
}

fn make_feature_fallback_smoke_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([246, 246, 246, 255]));
    for y in 0..height {
        let band = (y / 10) % 2;
        let v = if band == 0 { 60 } else { 220 };
        for x in 0..width {
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    // Small cross-shaped features create strong FAST corners but barely
    // affect template NCC on periodic bands.
    for i in 0..80u32 {
        let cx = 24 + ((i * 59) % width.saturating_sub(48).max(1));
        let cy = 24 + ((i * 89) % height.saturating_sub(48).max(1));
        let color = Rgba([
            (50 + (i * 17) % 150) as u8,
            (60 + (i * 23) % 140) as u8,
            (70 + (i * 29) % 130) as u8,
            255,
        ]);
        // 3x3 cross
        img.put_pixel(cx, cy, color);
        if cx > 0 {
            img.put_pixel(cx - 1, cy, color);
        }
        if cx + 1 < width {
            img.put_pixel(cx + 1, cy, color);
        }
        if cy > 0 {
            img.put_pixel(cx, cy - 1, color);
        }
        if cy + 1 < height {
            img.put_pixel(cx, cy + 1, color);
        }
    }
    img
}

#[test]
fn rollshot_dev_stitch_folder_writes_debug_report() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder-debug");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_scroll_canvas(160, 600);
    for (idx, y) in [0u32, 40, 80].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");
    let report_json = tempdir.join("report.json");
    let debug_dir = tempdir.join("debug");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .arg("--dump-overlap-debug")
        .arg(&debug_dir)
        .output()
        .expect("run rollshot-dev stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&report_json).expect("read report");
    assert!(report.contains("\"frames\""), "report = {report}");
    assert!(report.contains("\"outcome\""), "report = {report}");
    assert!(debug_dir.exists(), "{} should exist", debug_dir.display());

    let overlap_entries: Vec<_> = std::fs::read_dir(&debug_dir)
        .expect("read debug dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains("overlap_prev") && n.ends_with(".png"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !overlap_entries.is_empty(),
        "expected at least one overlap_prev PNG in {}, found {:?}",
        debug_dir.display(),
        std::fs::read_dir(&debug_dir)
            .map(|it| it
                .filter_map(Result::ok)
                .map(|e| e.file_name())
                .collect::<Vec<_>>())
            .unwrap_or_default(),
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_fixture_writes_debug_report() {
    let tempdir = tempdir_for_test("rollshot-capture-debug");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_scroll_canvas(160, 600);
    for (idx, y) in [0u32, 40, 80].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let output_png = tempdir.join("captured.png");
    let report_json = tempdir.join("capture-report.json");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .arg("--backend")
        .arg("fixture")
        .arg("--fixture")
        .arg(&frames_dir)
        .arg("--max-frames")
        .arg("3")
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .output()
        .expect("run rollshot-dev capture fixture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&report_json).expect("read report");
    assert!(report.contains("\"frames\""), "report = {report}");
    assert!(
        report.contains("\"capture_interval_ms\""),
        "report = {report}"
    );
    assert!(
        report.contains("\"stitch_elapsed_ms\""),
        "report = {report}"
    );
    assert!(
        report.contains("\"outcome\": \"Appended\""),
        "report = {report}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_stitch_folder_dumps_axis_changed_overlap_debug() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder-axis-debug");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_axis_debug_canvas(900, 600);
    for (idx, (x, y)) in [(200u32, 0u32), (200, 80), (280, 80)].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, *x, *y, 320, 320).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");
    let report_json = tempdir.join("report.json");
    let debug_dir = tempdir.join("debug");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .arg("--dump-overlap-debug")
        .arg(&debug_dir)
        .output()
        .expect("run rollshot-dev stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&report_json).expect("read report");
    assert!(
        report.contains("\"outcome\": \"AxisChanged\""),
        "report = {report}"
    );
    assert!(
        debug_dir.join("frame_002_overlap_prev.png").exists(),
        "expected axis-changed overlap debug for frame 002 in {}",
        debug_dir.display()
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_stitch_folder_default_uses_fast_hnsw() {
    let tempdir = tempdir_for_test("rollshot-stitch-fast-hnsw");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_feature_fallback_smoke_canvas(320, 820);
    for (idx, y) in [0u32, 86].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, 320, 320).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");
    let report_json = tempdir.join("report.json");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .output()
        .expect("run rollshot-dev stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&report_json).expect("read report");
    assert!(
        report.contains("\"method\": \"FastHnsw\""),
        "expected FastHnsw in report, got {report}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_stitch_folder_disable_feature_fallback_makes_no_match() {
    let tempdir = tempdir_for_test("rollshot-stitch-disable-fallback");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_feature_fallback_smoke_canvas(320, 820);
    for (idx, y) in [0u32, 86].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, 320, 320).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");
    let report_json = tempdir.join("report.json");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .arg("--disable-feature-fallback")
        .output()
        .expect("run rollshot-dev stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&report_json).expect("read report");
    assert!(
        report.contains("FeatureFallbackDisabled"),
        "expected FeatureFallbackDisabled in report, got {report}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}
