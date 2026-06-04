mod common;

use std::process::Command;

use common::{command_output, temp_dir};

#[test]
#[cfg(target_os = "linux")]
fn linux_portal_backend_exits_with_unsupported_code() {
    let tempdir = temp_dir("linux-portal");
    let out = tempdir.join("out.png");
    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "linux-portal"])
        .args(["--output"])
        .arg(&out);
    let output = command_output(&mut command);

    assert_eq!(
        output.status.code(),
        Some(4),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported"), "stderr = {stderr}");
    assert!(stderr.contains("Wayland portals only"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
#[cfg(target_os = "linux")]
fn macos_sck_backend_on_linux_exits_with_unsupported_code() {
    let tempdir = temp_dir("macos-on-linux");
    let out = tempdir.join("out.png");
    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "macos-sck"])
        .args(["--output"])
        .arg(&out);
    let output = command_output(&mut command);

    assert_eq!(
        output.status.code(),
        Some(4),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
#[cfg(target_os = "macos")]
fn macos_sck_backend_rejects_portal_region_without_starting_capture() {
    let tempdir = temp_dir("macos-sck");
    let out = tempdir.join("out.png");
    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "macos-sck"])
        .args(["--region", "portal"])
        .args(["--output"])
        .arg(&out);
    let output = command_output(&mut command);

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected invalid config before macOS capture starts; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--region portal"), "stderr = {stderr}");
    assert!(stderr.contains("linux-portal"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

/// `--backend auto` is the default value every user hits first. Its expected
/// exit code depends on the host, so the test computes the expectation from
/// the same env vars `default_backend()` reads.
#[test]
fn backend_auto_exits_with_host_appropriate_code() {
    let tempdir = temp_dir("backend-auto");
    let out = tempdir.join("out.png");
    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .arg("--headless")
        .args(["--backend", "auto"])
        .args(["--output"])
        .arg(&out);
    if cfg!(target_os = "macos") {
        command.args(["--region", "portal"]);
    } else {
        command.args(["--max-frames", "1"]);
    }
    let output = command_output(&mut command);

    let expected_code = if cfg!(target_os = "macos") {
        // Hosted macOS CI must not start ScreenCaptureKit. `auto` still
        // resolves to macos-sck, and `portal` fails during argument validation
        // before backend startup.
        1
    } else if cfg!(target_os = "linux")
        && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
    {
        2
    } else {
        4
    };

    assert_eq!(
        output.status.code(),
        Some(expected_code),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}
