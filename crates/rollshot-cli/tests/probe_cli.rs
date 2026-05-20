use std::process::Command;

#[test]
fn probe_text_includes_os_and_default_backend() {
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
    assert!(stdout.contains("os:"), "stdout = {stdout}");
    assert!(stdout.contains("default backend:"), "stdout = {stdout}");
    assert!(stdout.contains("fixture"), "stdout = {stdout}");
}

#[test]
fn probe_json_parses_and_has_expected_shape() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("probe")
        .arg("--json")
        .output()
        .expect("run rollshot probe --json");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("probe --json must be valid json");
    assert!(parsed.get("os").is_some(), "missing os: {stdout}");
    assert!(
        parsed.get("default_backend").is_some(),
        "missing default_backend: {stdout}"
    );
    let backends = parsed
        .get("backends")
        .and_then(|v| v.as_array())
        .expect("backends array");
    let names: Vec<&str> = backends
        .iter()
        .filter_map(|b| b.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(names.contains(&"fixture"), "names = {names:?}");
}
