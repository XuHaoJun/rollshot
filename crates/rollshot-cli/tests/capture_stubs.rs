mod common;

use std::process::Command;

use common::temp_dir;

#[test]
#[cfg(target_os = "linux")]
fn linux_portal_backend_exits_with_not_implemented_code() {
    let tempdir = temp_dir("linux-portal");
    let out = tempdir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "linux-portal"])
        .args(["--output"])
        .arg(&out)
        .output()
        .expect("run rollshot capture");

    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not implemented"), "stderr = {stderr}");
    assert!(stderr.contains("linux-portal"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
#[cfg(target_os = "linux")]
fn macos_sck_backend_on_linux_exits_with_unsupported_code() {
    let tempdir = temp_dir("macos-on-linux");
    let out = tempdir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "macos-sck"])
        .args(["--output"])
        .arg(&out)
        .output()
        .expect("run rollshot capture");

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
fn macos_sck_backend_without_permission_prompt_exits_cleanly() {
    if std::env::var("ROLLSHOT_REAL_CAPTURE").ok().as_deref() == Some("1") {
        eprintln!("real macOS capture is covered by macos_sck_smoke");
        return;
    }

    let tempdir = temp_dir("macos-sck");
    let out = tempdir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "macos-sck"])
        .args(["--output"])
        .arg(&out)
        .args(["--max-frames", "1"])
        .env("ROLLSHOT_NO_PERMISSION_PROMPT", "1")
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success() || output.status.code() == Some(3),
        "expected success with existing permission or permission-denied without prompt; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "macos-sck should no longer be a NotImplemented stub; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

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
        .args(["--backend", "auto"])
        .args(["--output"])
        .arg(&out)
        .args(["--max-frames", "1"]);
    if cfg!(target_os = "macos") {
        command.env("ROLLSHOT_NO_PERMISSION_PROMPT", "1");
    }
    let output = command.output().expect("run rollshot capture");

    let expected_code = if cfg!(target_os = "macos") {
        // macOS auto now reaches the real backend. Hosted CI normally lacks
        // Screen Recording permission, so permission denied is the expected
        // non-interactive result. The env var prevents a system permission
        // prompt during tests. If permission is already granted, the command may
        // succeed after one frame.
        if output.status.success() {
            0
        } else {
            3
        }
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
