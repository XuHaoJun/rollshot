use std::process::Command;

#[test]
fn failing_launch_flushes_json_log_and_keeps_console_output() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("rollshot.jsonl");
    let backend = if cfg!(target_os = "linux") {
        "macos-sck"
    } else {
        "linux-portal"
    };
    let payload = format!(r#"{{"backend":"{}","fps":5,"show_cursor":false}}"#, backend);

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-app"))
        .env("RUST_LOG", "warn,rollshot=debug")
        .args([
            "--log-file",
            log_path.to_str().unwrap(),
            "--capture",
            &payload,
        ])
        .output()
        .expect("run rollshot-app");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("application failed"), "stderr = {stderr}");

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(
        log.contains("\"message\":\"capture session started\""),
        "log = {log}"
    );
    assert!(
        log.contains("\"message\":\"application failed\""),
        "log = {log}"
    );
    assert!(log.ends_with('\n'), "log must be completely flushed");
}

#[test]
fn error_filter_omits_debug_session_event_but_includes_final_error() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("rollshot.jsonl");
    let backend = if cfg!(target_os = "linux") {
        "macos-sck"
    } else {
        "linux-portal"
    };
    let payload = format!(r#"{{"backend":"{}","fps":5,"show_cursor":false}}"#, backend);

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-app"))
        .env("RUST_LOG", "error")
        .args([
            "--log-file",
            log_path.to_str().unwrap(),
            "--capture",
            &payload,
        ])
        .output()
        .expect("run rollshot-app");

    assert!(!output.status.success());

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(
        !log.contains("\"message\":\"capture session started\""),
        "debug event should be filtered out at error level, log = {log}"
    );
    assert!(
        log.contains("\"message\":\"application failed\""),
        "error-level event must be present, log = {log}"
    );
}

#[test]
fn invalid_directives_warn_and_still_launch() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("rollshot.jsonl");
    let backend = if cfg!(target_os = "linux") {
        "macos-sck"
    } else {
        "linux-portal"
    };
    let payload = format!(r#"{{"backend":"{}","fps":5,"show_cursor":false}}"#, backend);

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-app"))
        .env("RUST_LOG", "warn,rollshot::app=debug,not valid")
        .args([
            "--log-file",
            log_path.to_str().unwrap(),
            "--capture",
            &payload,
        ])
        .output()
        .expect("run rollshot-app");

    assert!(!output.status.success());

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(
        log.contains("ignored invalid RUST_LOG directives"),
        "must warn about invalid directives, log = {log}"
    );
    assert!(
        log.contains("\"message\":\"capture session started\""),
        "must still launch with valid portion of filter, log = {log}"
    );
}
