mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{command_output, temp_dir};

#[test]
fn capture_without_output_launches_interactive_app() {
    let tempdir = temp_dir("interactive-launch");
    let marker = tempdir.join("marker.txt");
    let fake_app = write_fake_app(&tempdir);

    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .args(["--backend", "linux-portal"])
        .args(["--fps", "7"])
        .arg("--show-cursor")
        .env("ROLLSHOT_APP", &fake_app)
        .env("ROLLSHOT_FAKE_APP_MARKER", &marker);

    let output = command_output(&mut command);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker_text = std::fs::read_to_string(&marker).expect("fake app marker written");
    assert_eq!(marker_text, "launched");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn headless_capture_requires_output() {
    let tempdir = temp_dir("headless-requires-output");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir);

    let output = command_output(&mut command);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--output is required with --headless"),
        "stderr = {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn interactive_capture_rejects_headless_only_flags() {
    let tempdir = temp_dir("interactive-rejects-headless-flags");
    let dump_dir = tempdir.join("dump");

    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .args(["--dump-frames"])
        .arg(&dump_dir);

    let output = command_output(&mut command);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("only supported with --headless"),
        "stderr = {stderr}"
    );
    assert!(stderr.contains("--dump-frames"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn capture_interactive_forwards_app_failure() {
    let tempdir = temp_dir("interactive-app-failure");
    let fake_app = write_failing_fake_app(&tempdir);

    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .args(["--backend", "linux-portal"])
        .env("ROLLSHOT_APP", &fake_app);

    let output = command_output(&mut command);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exited with status"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[cfg(unix)]
fn write_fake_app(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("fake-rollshot-app");
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf launched > \"$ROLLSHOT_FAKE_APP_MARKER\"\nexit 0\n",
    )
    .expect("write fake app");
    let mut permissions = std::fs::metadata(&path)
        .expect("fake app metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("make fake app executable");
    path
}

#[cfg(windows)]
fn write_fake_app(dir: &Path) -> PathBuf {
    let path = dir.join("fake-rollshot-app.cmd");
    std::fs::write(
        &path,
        "@echo off\r\n<nul set /p=launched > \"%ROLLSHOT_FAKE_APP_MARKER%\"\r\nexit /B 0\r\n",
    )
    .expect("write fake app");
    path
}

#[cfg(unix)]
fn write_failing_fake_app(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("fake-rollshot-app-fail");
    std::fs::write(&path, "#!/bin/sh\nexit 1\n").expect("write failing fake app");
    let mut permissions = std::fs::metadata(&path)
        .expect("fake app metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("make fake app executable");
    path
}

#[cfg(windows)]
fn write_failing_fake_app(dir: &Path) -> PathBuf {
    let path = dir.join("fake-rollshot-app-fail.cmd");
    std::fs::write(&path, "@echo off\r\nexit /B 1\r\n").expect("write failing fake app");
    path
}
