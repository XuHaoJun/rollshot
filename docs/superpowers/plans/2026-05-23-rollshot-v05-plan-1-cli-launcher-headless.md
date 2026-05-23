# rollshot v0.5 Plan 1: CLI Launcher and Headless Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `rollshot capture` launch the interactive GUI app, while `rollshot capture --headless` preserves the existing CLI capture/stitch behavior.

**Architecture:** Keep `rollshot-cli` as the user entrypoint, but split capture into two paths: a new interactive launcher path and the existing headless stitch path. Store the small serializable `InteractiveLaunchOptions` type in `rollshot-capture` so `rollshot-cli` and `rollshot-app` can share the launch contract without creating a new crate.

**Tech Stack:** Rust 2021, clap v4, serde/serde_json, existing `rollshot-capture` and `rollshot-cli` crates, CLI integration tests using `CARGO_BIN_EXE_rollshot`.

---

## Source Spec

Plan 1 implements only this section of the replacement spec:

```text
Plan 1: CLI Launcher and Headless Split

- change `rollshot capture` into the interactive entrypoint
- add `rollshot capture --headless`
- make `--output` required only for headless mode
- add GUI launcher discovery/spawn behavior
- preserve existing headless capture behavior and tests

Do not build the Tauri UI in this plan.
```

Source: `docs/superpowers/specs/2026-05-23-rollshot-v05-interactive-capture-replacement-design.md`

---

## File Structure

Modify:

- `crates/rollshot-capture/Cargo.toml`
  - Add `serde` dependency and `serde_json` dev-dependency for the shared launch-options type and its round-trip test.

- `crates/rollshot-capture/src/types.rs`
  - Add `InteractiveLaunchOptions`.
  - Add a JSON round-trip test for the launch contract.

- `crates/rollshot-capture/src/lib.rs`
  - Re-export `InteractiveLaunchOptions`.

- `crates/rollshot-cli/src/args.rs`
  - Add `--headless`.
  - Change `--output` from required `PathBuf` to `Option<PathBuf>`.

- `crates/rollshot-cli/src/lib.rs`
  - Register the new launcher module.

- `crates/rollshot-cli/src/cmd_capture.rs`
  - Route non-headless capture to the launcher.
  - Move current implementation into a headless-only helper.
  - Require `--output` only in headless mode.

- `crates/rollshot-cli/tests/capture_fixture.rs`
  - Add `--headless` to existing fixture capture tests.

- `crates/rollshot-cli/tests/capture_stubs.rs`
  - Add `--headless` to backend stub tests so they keep testing the current CLI backend path.

Create:

- `crates/rollshot-cli/src/cmd_capture_launcher.rs`
  - Validate interactive-mode flags.
  - Resolve `rollshot-app`.
  - Serialize `InteractiveLaunchOptions`.
  - Launch the app process.
  - Unit-test pure validation, serialization, and binary resolution.

- `crates/rollshot-cli/tests/capture_launcher.rs`
  - Integration-test that `rollshot capture` no longer requires `--output`.
  - Integration-test that `--headless` requires `--output`.
  - Integration-test that headless-only flags are rejected outside `--headless`.
  - Integration-test fake app launch through `ROLLSHOT_APP`.

Do not modify `rollshot-app` beyond relying on its existing default binary name.

---

## Task 1: Add Shared Interactive Launch Options

**Files:**
- Modify: `crates/rollshot-capture/Cargo.toml`
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Add the failing round-trip test**

Append this test module to `crates/rollshot-capture/src/types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::InteractiveLaunchOptions;

    #[test]
    fn interactive_launch_options_round_trip_json() {
        let options = InteractiveLaunchOptions {
            backend: "linux-portal".to_string(),
            fps: 7,
            show_cursor: true,
        };

        let json = serde_json::to_string(&options).expect("serialize launch options");
        assert!(
            json.contains("\"backend\":\"linux-portal\""),
            "json = {json}"
        );

        let decoded: InteractiveLaunchOptions =
            serde_json::from_str(&json).expect("deserialize launch options");
        assert_eq!(decoded, options);
    }
}
```

- [ ] **Step 2: Run the focused failing test**

Run:

```bash
rtk cargo test -p rollshot-capture interactive_launch_options_round_trip_json
```

Expected: FAIL because `InteractiveLaunchOptions` is not defined and `serde_json` is not available in `rollshot-capture` tests.

- [ ] **Step 3: Add serde dependencies**

Edit `crates/rollshot-capture/Cargo.toml`:

```toml
[dependencies]
anyhow = { workspace = true }
image = { workspace = true }
serde = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
```

Keep the existing target-specific dependencies and lints unchanged.

- [ ] **Step 4: Define `InteractiveLaunchOptions`**

Add this struct in `crates/rollshot-capture/src/types.rs` immediately after `CaptureOptions`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InteractiveLaunchOptions {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
}
```

- [ ] **Step 5: Re-export the type**

Update the re-export list in `crates/rollshot-capture/src/lib.rs`:

```rust
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, InteractiveLaunchOptions,
    PixelFormat, Region, RegionMode, Size,
};
```

- [ ] **Step 6: Verify the focused test passes**

Run:

```bash
rtk cargo test -p rollshot-capture interactive_launch_options_round_trip_json
```

Expected: PASS.

- [ ] **Step 7: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-capture/Cargo.toml crates/rollshot-capture/src/types.rs crates/rollshot-capture/src/lib.rs
rtk git commit -m "feat(capture): add interactive launch options"
```

---

## Task 2: Add CLI Mode Parsing For Interactive vs Headless

**Files:**
- Modify: `crates/rollshot-cli/src/args.rs`
- Create: `crates/rollshot-cli/tests/capture_launcher.rs`

- [ ] **Step 1: Create integration tests for the new CLI contract**

Create `crates/rollshot-cli/tests/capture_launcher.rs`:

```rust
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
    assert!(
        stderr.contains("exited with status"),
        "stderr = {stderr}"
    );

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
```

- [ ] **Step 2: Run the focused failing tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_launcher
```

Expected:

- `capture_without_output_launches_interactive_app` fails because clap still requires `--output`.
- `headless_capture_requires_output` fails because `--headless` is not defined.
- `interactive_capture_rejects_headless_only_flags` fails because interactive validation does not exist.

- [ ] **Step 3: Update `CaptureArgs`**

Edit `crates/rollshot-cli/src/args.rs` so `CaptureArgs` begins with these fields:

```rust
#[derive(Debug, clap::Args)]
pub struct CaptureArgs {
    /// Run capture/stitch without the GUI.
    #[arg(long, default_value_t = false)]
    pub headless: bool,

    /// Which capture backend to use.
    #[arg(
        long,
        default_value = "auto",
        value_parser = ["auto", "fixture", "linux-portal", "macos-sck"],
    )]
    pub backend: String,

    /// Region selection mode. Accepts `auto`, `portal`, `full`, or `"X,Y WxH"`.
    #[arg(long, default_value = "auto")]
    pub region: String,

    /// Output PNG path. Required with --headless.
    #[arg(long)]
    pub output: Option<PathBuf>,
```

Leave the rest of the existing fields in place.

- [ ] **Step 4: Run the focused tests again**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_launcher
```

Expected: tests still fail, but clap should no longer reject missing `--output` or unknown `--headless`. Failures should now point at missing implementation in `cmd_capture.rs` or the absent launcher.

- [ ] **Step 5: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-cli/src/args.rs crates/rollshot-cli/tests/capture_launcher.rs
rtk git commit -m "feat(cli): parse interactive capture mode"
```

---

## Task 3: Implement The Interactive Launcher Module

**Files:**
- Create: `crates/rollshot-cli/src/cmd_capture_launcher.rs`
- Modify: `crates/rollshot-cli/src/lib.rs`

- [ ] **Step 1: Create launcher unit tests**

Create `crates/rollshot-cli/src/cmd_capture_launcher.rs` with this initial test-only skeleton:

```rust
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rollshot_capture::InteractiveLaunchOptions;

use crate::args::CaptureArgs;
use crate::cli_error::CliError;

pub fn run(_args: &CaptureArgs) -> Result<String, CliError> {
    Err(CliError::new("interactive launcher is not implemented", 1))
}

#[cfg(windows)]
fn default_app_binary_name() -> &'static str {
    "rollshot-app.exe"
}

#[cfg(not(windows))]
fn default_app_binary_name() -> &'static str {
    "rollshot-app"
}

#[cfg(test)]
mod tests {
    use super::{
        app_args, launch_options, reject_headless_only_flags, resolve_app_binary_from_env_and_exe,
    };
    use crate::args::CaptureArgs;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    fn base_args() -> CaptureArgs {
        CaptureArgs {
            headless: false,
            backend: "linux-portal".to_string(),
            region: "auto".to_string(),
            output: None,
            fixture: None,
            dump_frames: None,
            debug_match_report: None,
            max_frames: 200,
            fps: 7,
            show_cursor: true,
            quiet: false,
            enable_akaze: false,
            disable_feature_fallback: false,
        }
    }

    #[test]
    fn launch_options_keep_only_interactive_fields() {
        let args = base_args();
        let options = launch_options(&args);

        assert_eq!(options.backend, "linux-portal");
        assert_eq!(options.fps, 7);
        assert!(options.show_cursor);
    }

    #[test]
    fn app_args_include_capture_flag_and_json_payload() {
        let args = base_args();
        let options = launch_options(&args);

        let app_args = app_args(&options).expect("build app args");
        assert_eq!(app_args[0], OsString::from("--capture"));

        let payload = app_args[1].to_string_lossy();
        assert!(payload.contains("\"backend\":\"linux-portal\""));
        assert!(payload.contains("\"fps\":7"));
        assert!(payload.contains("\"show_cursor\":true"));
    }

    #[test]
    fn reject_headless_only_flags_lists_all_rejected_flags() {
        let mut args = base_args();
        args.output = Some(PathBuf::from("out.png"));
        args.region = "10,20 100x200".to_string();
        args.fixture = Some(PathBuf::from("frames"));
        args.dump_frames = Some(PathBuf::from("dump"));
        args.debug_match_report = Some(PathBuf::from("report.json"));
        args.max_frames = 10;
        args.quiet = true;
        args.enable_akaze = true;
        args.disable_feature_fallback = true;

        let err = reject_headless_only_flags(&args).expect_err("flags rejected");
        assert!(err.message.contains("--headless"), "{}", err.message);
        for flag in [
            "--output",
            "--region",
            "--fixture",
            "--dump-frames",
            "--debug-match-report",
            "--max-frames",
            "--quiet",
            "--enable-akaze",
            "--disable-feature-fallback",
        ] {
            assert!(err.message.contains(flag), "{} missing {flag}", err.message);
        }
    }

    #[test]
    fn resolve_app_binary_prefers_env_override() {
        let env_path = PathBuf::from("custom-rollshot-app");
        let current_exe = Path::new("target/debug/rollshot");

        let resolved = resolve_app_binary_from_env_and_exe(
            Some(OsString::from(env_path.as_os_str())),
            current_exe,
        )
        .expect("env override resolves");

        assert_eq!(resolved, env_path);
    }
}
```

- [ ] **Step 2: Run the focused failing unit tests**

Run:

```bash
rtk cargo test -p rollshot-cli cmd_capture_launcher
```

Expected: FAIL because helper functions such as `launch_options`, `app_args`, `reject_headless_only_flags`, and `resolve_app_binary_from_env_and_exe` are not implemented.

- [ ] **Step 3: Implement the launcher module**

Replace `crates/rollshot-cli/src/cmd_capture_launcher.rs` with:

```rust
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use rollshot_capture::InteractiveLaunchOptions;

use crate::args::CaptureArgs;
use crate::cli_error::CliError;

const APP_ENV: &str = "ROLLSHOT_APP";

pub fn run(args: &CaptureArgs) -> Result<String, CliError> {
    reject_headless_only_flags(args)?;

    let options = launch_options(args);
    let app_path = resolve_app_binary()?;
    let mut command = Command::new(&app_path);
    command.args(app_args(&options)?);

    let status = command.status().map_err(|err| {
        CliError::new(
            format!("failed to launch {}: {err}", app_path.display()),
            1,
        )
    })?;

    if status.success() {
        Ok(String::new())
    } else {
        Err(CliError::new(
            format!(
                "{} exited with status {}",
                app_path.display(),
                status_label(status)
            ),
            status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1),
        ))
    }
}

fn launch_options(args: &CaptureArgs) -> InteractiveLaunchOptions {
    InteractiveLaunchOptions {
        backend: args.backend.clone(),
        fps: args.fps,
        show_cursor: args.show_cursor,
    }
}

fn app_args(options: &InteractiveLaunchOptions) -> Result<Vec<OsString>, CliError> {
    let payload = serde_json::to_string(options).map_err(|err| {
        CliError::new(format!("failed to encode GUI launch options: {err}"), 1)
    })?;
    Ok(vec![OsString::from("--capture"), OsString::from(payload)])
}

fn reject_headless_only_flags(args: &CaptureArgs) -> Result<(), CliError> {
    let mut rejected = Vec::new();

    if args.output.is_some() {
        rejected.push("--output");
    }
    if args.region != "auto" {
        rejected.push("--region");
    }
    if args.fixture.is_some() {
        rejected.push("--fixture");
    }
    if args.dump_frames.is_some() {
        rejected.push("--dump-frames");
    }
    if args.debug_match_report.is_some() {
        rejected.push("--debug-match-report");
    }
    if args.max_frames != 200 {
        rejected.push("--max-frames");
    }
    if args.quiet {
        rejected.push("--quiet");
    }
    if args.enable_akaze {
        rejected.push("--enable-akaze");
    }
    if args.disable_feature_fallback {
        rejected.push("--disable-feature-fallback");
    }

    if rejected.is_empty() {
        Ok(())
    } else {
        Err(CliError::new(
            format!(
                "the following flags are only supported with --headless: {}",
                rejected.join(", ")
            ),
            1,
        ))
    }
}

fn resolve_app_binary() -> Result<PathBuf, CliError> {
    let current_exe = std::env::current_exe()
        .map_err(|err| CliError::new(format!("failed to locate rollshot binary: {err}"), 1))?;
    resolve_app_binary_from_env_and_exe(std::env::var_os(APP_ENV), &current_exe)
}

fn resolve_app_binary_from_env_and_exe(
    env_path: Option<OsString>,
    current_exe: &Path,
) -> Result<PathBuf, CliError> {
    if let Some(path) = env_path {
        if path.is_empty() {
            return Err(CliError::new(
                format!("{APP_ENV} is set but empty; expected path to rollshot-app"),
                1,
            ));
        }
        return Ok(PathBuf::from(path));
    }

    let bin_dir = current_exe.parent().ok_or_else(|| {
        CliError::new(
            format!("failed to locate {} next to rollshot", default_app_binary_name()),
            1,
        )
    })?;
    Ok(bin_dir.join(default_app_binary_name()))
}

#[cfg(windows)]
fn default_app_binary_name() -> &'static str {
    "rollshot-app.exe"
}

#[cfg(not(windows))]
fn default_app_binary_name() -> &'static str {
    "rollshot-app"
}

fn status_label(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string())
}
```

Keep the unit tests from Step 1 at the bottom of the file.

- [ ] **Step 4: Register the module**

Edit `crates/rollshot-cli/src/lib.rs`:

```rust
pub mod args;
pub mod cli_error;
pub mod cmd_capture;
pub mod cmd_capture_launcher;
pub mod cmd_probe;
pub mod cmd_stitch_folder;
mod frame_slot;
```

- [ ] **Step 5: Run launcher unit tests**

Run:

```bash
rtk cargo test -p rollshot-cli cmd_capture_launcher
```

Expected: PASS.

- [ ] **Step 6: Run capture launcher integration tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_launcher
```

Expected: `headless_capture_requires_output` still fails until Task 4 wires `cmd_capture::run` into headless vs launcher paths. The other two tests should either pass or fail only because `cmd_capture::run` has not been routed yet.

- [ ] **Step 7: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-cli/src/cmd_capture_launcher.rs crates/rollshot-cli/src/lib.rs
rtk git commit -m "feat(cli): add interactive capture launcher"
```

---

## Task 4: Route Capture Between Launcher And Headless Runner

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`

- [ ] **Step 1: Confirm routing tests fail**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_launcher
```

Expected: FAIL because `cmd_capture::run` still treats every capture as the headless path and still uses `args.output` as a required `PathBuf`.

- [ ] **Step 2: Split `run` into mode router plus headless helper**

At the top of `crates/rollshot-cli/src/cmd_capture.rs`, replace the current `pub fn run(args: &CaptureArgs) -> Result<String, CliError>` body with this shape:

```rust
pub fn run(args: &CaptureArgs) -> Result<String, CliError> {
    if args.headless {
        run_headless(args)
    } else {
        crate::cmd_capture_launcher::run(args)
    }
}

fn run_headless(args: &CaptureArgs) -> Result<String, CliError> {
    let output = args
        .output
        .as_ref()
        .ok_or_else(|| CliError::new("--output is required with --headless", 1))?;

    let kind = BackendKind::from_cli_flag(&args.backend).map_err(CliError::from_capture)?;
```

Move the rest of the old `run` implementation into `run_headless`.

- [ ] **Step 3: Update `args.output` usages inside `run_headless`**

Inside `run_headless`, replace direct uses of `args.output` with the local `output` reference:

```rust
save_png(stitched, output)?;

Ok(format!(
    "captured {captured} frames, appended {appended} \
     (duplicates {duplicates}, no-progress {no_progress}, \
     no-match {no_match}, frames-read {frames_read})\n\
     output: {out} ({w}x{h})\n",
    out = output.display(),
    w = stitched.width(),
    h = stitched.height(),
))
```

Leave uses of `args.debug_match_report`, `args.dump_frames`, `args.fps`, and matcher flags unchanged because they are headless behavior.

- [ ] **Step 4: Run routing tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_launcher
```

Expected: PASS.

- [ ] **Step 5: Run focused headless fixture test before updating old tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_fixture rollshot_capture_fixture_writes_png
```

Expected: FAIL because this existing test does not pass `--headless` yet and now goes through the interactive launcher.

- [ ] **Step 6: Commit Task 4**

Run:

```bash
rtk git add crates/rollshot-cli/src/cmd_capture.rs
rtk git commit -m "feat(cli): route capture by interactive mode"
```

---

## Task 5: Update Existing Capture Tests To Use `--headless`

**Files:**
- Modify: `crates/rollshot-cli/tests/capture_fixture.rs`
- Modify: `crates/rollshot-cli/tests/capture_stubs.rs`

- [ ] **Step 1: Add `--headless` after every capture subcommand in existing headless tests**

In `crates/rollshot-cli/tests/capture_fixture.rs`, every command currently starting like this:

```rust
let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
    .arg("capture")
```

must become:

```rust
let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
    .arg("capture")
    .arg("--headless")
```

In `crates/rollshot-cli/tests/capture_stubs.rs`, every command currently starting like this:

```rust
command
    .arg("capture")
```

must become:

```rust
command
    .arg("capture")
    .arg("--headless")
```

This keeps those tests pointed at the legacy capture backend behavior.

- [ ] **Step 2: Verify no old capture backend test still omits `--headless`**

Run:

```bash
rtk rg -n 'arg\("capture"\)' crates/rollshot-cli/tests/capture_fixture.rs crates/rollshot-cli/tests/capture_stubs.rs
```

Expected: every matching block has `.arg("--headless")` immediately after `.arg("capture")`.

- [ ] **Step 3: Run fixture tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_fixture
```

Expected: PASS.

- [ ] **Step 4: Run backend stub tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_stubs
```

Expected: PASS, with platform-specific expected exit codes unchanged.

- [ ] **Step 5: Commit Task 5**

Run:

```bash
rtk git add crates/rollshot-cli/tests/capture_fixture.rs crates/rollshot-cli/tests/capture_stubs.rs
rtk git commit -m "test(cli): mark capture backend tests headless"
```

---

## Task 6: Final Verification

**Files:**
- Verify all files changed by Tasks 1-5.

- [ ] **Step 1: Run all Rust tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 2: Check formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 3: Run clippy**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Inspect git status**

Run:

```bash
rtk git status --short
```

Expected: no unstaged changes except files intentionally left for the next commit. If formatting changed files, inspect them and commit them with the task they belong to.

- [ ] **Step 5: Record Plan 1 completion**

If all verification passes, the implementation worker should report:

```text
Plan 1 complete:
- `rollshot capture` launches `rollshot-app` through the launcher path.
- `rollshot capture --headless` preserves the existing capture/stitch behavior.
- `--output` is required only with `--headless`.
- existing headless capture tests pass with explicit `--headless`.
```

Do not start Plan 2 until Plan 1 has been reviewed.

---

## Self-Review Notes

Spec coverage:

- `rollshot capture` as interactive entrypoint: Tasks 2-4.
- `--headless` legacy mode: Tasks 2, 4, and 5.
- `--output` required only for headless: Tasks 2 and 4.
- GUI launcher discovery/spawn behavior: Task 3.
- Preserve existing headless capture tests: Task 5 and Task 6.
- Do not build Tauri UI: no task modifies `crates/rollshot-app`.

Boundary choices:

- `ROLLSHOT_APP` is an explicit launcher override used for tests and local development.
- The default launcher path is a sibling `rollshot-app` binary next to the running `rollshot` executable.
- Non-headless mode rejects flags that currently belong only to the CLI stitch/debug workflow.
