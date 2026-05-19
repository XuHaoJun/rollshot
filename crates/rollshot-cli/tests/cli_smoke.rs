use std::process::Command;

#[test]
fn rollshot_probe_binary_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("probe")
        .output()
        .expect("run rollshot probe");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("rollshot"));
    assert!(stdout.contains("real capture: unavailable"));
}
