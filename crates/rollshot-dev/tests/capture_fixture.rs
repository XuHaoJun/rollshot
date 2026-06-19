mod common;

use std::process::Command;

use common::{temp_dir, write_scroll_fixture};

#[test]
fn rollshot_dev_capture_fixture_writes_png() {
    let tempdir = temp_dir("fixture-flow");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot-dev capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
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
fn rollshot_dev_capture_fixture_requires_fixture_path() {
    let tempdir = temp_dir("missing-fixture-path");
    let output_png = tempdir.join("out.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot-dev capture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--fixture"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_dump_frames_writes_each_frame() {
    let tempdir = temp_dir("dump-frames");
    let frames_dir = tempdir.join("frames");
    let dump_dir = tempdir.join("dump");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    std::fs::create_dir_all(&dump_dir).expect("create dump dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--dump-frames"])
        .arg(&dump_dir)
        .output()
        .expect("run rollshot-dev capture");

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
    assert!(!dumped.is_empty(), "at least one frame should be dumped");
    for path in &dumped {
        let bytes = std::fs::read(path).expect("read dumped frame");
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "dumped file {} should be a valid PNG",
            path.display()
        );
    }

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_respects_max_frames() {
    let tempdir = temp_dir("max-frames");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let dump_dir = tempdir.join("dump");
    std::fs::create_dir_all(&dump_dir).expect("create dump dir");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--dump-frames"])
        .arg(&dump_dir)
        .args(["--max-frames", "2"])
        .output()
        .expect("run rollshot-dev capture");

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
fn rollshot_dev_capture_accepts_manual_region_string() {
    let tempdir = temp_dir("region-manual");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--region", "10,20 100x200"])
        .output()
        .expect("run rollshot-dev capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_rejects_garbage_region() {
    let tempdir = temp_dir("region-garbage");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--region", "totally bogus"])
        .output()
        .expect("run rollshot-dev capture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("region"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_rejects_portal_region_for_fixture_backend() {
    let tempdir = temp_dir("region-portal-fixture");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--region", "portal"])
        .output()
        .expect("run rollshot-dev capture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("portal"), "stderr = {stderr}");
    assert!(stderr.contains("linux-portal"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_prints_default_progress_to_stderr() {
    let tempdir = temp_dir("progress-stderr");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--max-frames", "2"])
        .output()
        .expect("run rollshot-dev capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("captured 2 frames"), "stdout = {stdout}");

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("frame 1/2: stitching..."),
        "stderr = {stderr}"
    );
    assert!(stderr.contains("FirstFrame"), "stderr = {stderr}");
    assert!(
        stderr.contains("frame 2/2: stitching..."),
        "stderr = {stderr}"
    );
    assert!(stderr.contains("elapsed="), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_quiet_suppresses_progress_stderr() {
    let tempdir = temp_dir("progress-quiet");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--max-frames", "2"])
        .arg("--quiet")
        .output()
        .expect("run rollshot-dev capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert_eq!(stderr, "");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_fixture_prints_diagnostics_summary() {
    let tempdir = temp_dir("diagnostics-summary");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot-dev capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("capture_interval_ms"),
        "stderr should contain capture_interval_ms: {stderr}"
    );
    assert!(
        stderr.contains("max_accepted_dy"),
        "stderr should contain max_accepted_dy: {stderr}"
    );
    assert!(
        stderr.contains("longest_no_match_run"),
        "stderr should contain longest_no_match_run: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_dev_capture_quiet_suppresses_diagnostics() {
    let tempdir = temp_dir("diagnostics-quiet");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .arg("--quiet")
        .output()
        .expect("run rollshot-dev capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        !stderr.contains("capture_interval_ms"),
        "stderr should NOT contain diagnostics when --quiet: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}
