use std::process::Command;

#[test]
fn missing_open_image_exits_nonzero_with_actionable_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-app"))
        .arg("open")
        .arg(&missing)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("could not open image"), "stderr = {stderr}");
    assert!(stderr.contains("missing.png"), "stderr = {stderr}");
}
