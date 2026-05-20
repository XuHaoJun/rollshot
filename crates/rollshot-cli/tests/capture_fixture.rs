mod common;

use std::process::Command;

use common::{temp_dir, write_scroll_fixture};

#[test]
fn rollshot_capture_fixture_writes_png() {
    let tempdir = temp_dir("fixture-flow");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("captured 4 frames"), "stdout = {stdout}");
    assert!(stdout.contains("appended"), "stdout = {stdout}");
    assert!(stdout.contains(output_png.to_string_lossy().as_ref()));

    let bytes = std::fs::read(&output_png).expect("read stitched png");
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    let stitched = image::load_from_memory(&bytes)
        .expect("decode stitched png")
        .to_rgba8();
    assert_eq!(stitched.width(), 160);
    assert!(stitched.height() > 160, "height = {}", stitched.height());

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_fixture_requires_fixture_path() {
    let tempdir = temp_dir("missing-fixture-path");
    let output_png = tempdir.join("out.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot capture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--fixture"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_dump_frames_writes_each_frame() {
    let tempdir = temp_dir("dump-frames");
    let frames_dir = tempdir.join("frames");
    let dump_dir = tempdir.join("dump");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    std::fs::create_dir_all(&dump_dir).expect("create dump dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--dump-frames"])
        .arg(&dump_dir)
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut dumped: Vec<_> = std::fs::read_dir(&dump_dir)
        .expect("read dump dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    dumped.sort();
    assert_eq!(dumped.len(), 4, "dumped = {dumped:?}");
    for (idx, path) in dumped.iter().enumerate() {
        let expected = format!("frame_{:04}.png", idx);
        assert!(
            path.file_name().unwrap().to_string_lossy().contains(&expected),
            "file {} should match {expected}",
            path.display()
        );
    }

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_respects_max_frames() {
    let tempdir = temp_dir("max-frames");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let dump_dir = tempdir.join("dump");
    std::fs::create_dir_all(&dump_dir).expect("create dump dir");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--dump-frames"])
        .arg(&dump_dir)
        .args(["--max-frames", "2"])
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let dumped = std::fs::read_dir(&dump_dir).expect("read dump dir").count();
    assert_eq!(dumped, 2, "expected exactly 2 dumped frames");

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("captured 2 frames"), "stdout = {stdout}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_accepts_manual_region_string() {
    let tempdir = temp_dir("region-manual");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--region", "10,20 100x200"])
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_rejects_garbage_region() {
    let tempdir = temp_dir("region-garbage");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--region", "totally bogus"])
        .output()
        .expect("run rollshot capture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("region"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}
