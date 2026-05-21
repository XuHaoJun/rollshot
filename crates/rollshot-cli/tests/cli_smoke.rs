use std::path::PathBuf;
use std::process::Command;

use image::{imageops, Rgba, RgbaImage};

#[test]
fn rollshot_probe_binary_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("probe")
        .output()
        .expect("run rollshot probe");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("probe"), "stdout = {stdout}");
}

#[test]
fn rollshot_stitch_folder_writes_png() {
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

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .output()
        .expect("run rollshot stitch-folder");

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

#[test]
fn rollshot_stitch_folder_writes_debug_report() {
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

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .arg("--dump-overlap-debug")
        .arg(&debug_dir)
        .arg("--disable-akaze")
        .output()
        .expect("run rollshot stitch-folder");

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
            .map(|it| it.filter_map(Result::ok).map(|e| e.file_name()).collect::<Vec<_>>())
            .unwrap_or_default(),
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[cfg(feature = "akaze")]
#[test]
fn rollshot_stitch_folder_disable_akaze_skips_akaze() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder-disable-akaze");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_akaze_fallback_smoke_canvas(320, 820);
    use std::hash::{Hash, Hasher};
    for (idx, y) in [0u32, 96, 192].iter().enumerate() {
        let mut frame = imageops::crop_imm(&canvas, 0, *y, 320, 320).to_image();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        idx.hash(&mut hasher);
        y.hash(&mut hasher);
        let seed = hasher.finish();
        for (i, px) in frame.pixels_mut().enumerate() {
            let h = seed.wrapping_mul(6364136223846793005).wrapping_add(i as u64);
            let n0 = (h as i32 % 61) - 30;
            let n1 = ((h >> 16) as i32 % 61) - 30;
            let n2 = ((h >> 32) as i32 % 61) - 30;
            px[0] = (px[0] as i32 + n0).clamp(0, 255) as u8;
            px[1] = (px[1] as i32 + n1).clamp(0, 255) as u8;
            px[2] = (px[2] as i32 + n2).clamp(0, 255) as u8;
        }
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let run = |label: &str, extra_args: &[&str]| -> String {
        let output_png = tempdir.join(format!("stitched_{label}.png"));
        let report_json = tempdir.join(format!("report_{label}.json"));
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rollshot"));
        cmd.arg("stitch-folder")
            .arg(&frames_dir)
            .arg("--output")
            .arg(&output_png)
            .arg("--debug-match-report")
            .arg(&report_json);
        for a in extra_args {
            cmd.arg(a);
        }
        let output = cmd.output().expect("run rollshot stitch-folder");
        assert!(
            output.status.success(),
            "{label} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read_to_string(&report_json).expect("read report")
    };

    let with_akaze = run("with", &[]);
    let without_akaze = run("without", &["--disable-akaze"]);

    assert!(
        with_akaze.contains("\"method\": \"Akaze\""),
        "expected AKAZE to fire in baseline run, report = {with_akaze}"
    );
    assert!(
        !without_akaze.contains("\"method\": \"Akaze\""),
        "--disable-akaze should suppress AKAZE entirely, report = {without_akaze}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[cfg(feature = "akaze")]
fn make_akaze_fallback_smoke_canvas(width: u32, height: u32) -> RgbaImage {
    use image::{Rgba, RgbaImage};
    let mut img = RgbaImage::from_pixel(width, height, Rgba([246, 246, 246, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 18 + y / 18) % 2 == 0 { 232 } else { 214 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    for i in 0..90u32 {
        let x = 20 + ((i * 43) % width.saturating_sub(40).max(1));
        let y = 20 + ((i * 61) % height.saturating_sub(40).max(1));
        let color = Rgba([
            (20 + (i * 19) % 180) as u8,
            (30 + (i * 23) % 160) as u8,
            (40 + (i * 29) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 9).min(height) {
            for xx in x..(x + 9).min(width) {
                if xx == x || yy == y || xx + 1 == x + 9 || yy + 1 == y + 9 || xx == x + yy - y {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }
    img
}
