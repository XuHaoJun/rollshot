# Runtime Diagnostic Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add release-retained, fine-grained structured diagnostics controlled by `RUST_LOG`, with optional console-plus-JSONL file output through `--log-file <PATH>`.

**Architecture:** `rollshot-app` owns subscriber initialization, logging CLI extraction, and the non-blocking file-writer guard. Library crates emit structured events under stable explicit `rollshot::*` targets but never install a subscriber. A repo check mechanically rejects tracing macros that omit `target:`, while subprocess tests verify release-style filtering, console mirroring, and guard flushing on a failing exit path.

**Tech Stack:** Rust 1.85, `tracing`, `tracing-subscriber` with `env-filter` and `json`, `tracing-appender`, existing Rust unit/integration tests, ripgrep-based CI check, existing stitch benchmark workflow.

---

## Scope And File Map

Create:

- `crates/rollshot-app/src/diagnostics.rs`: pure filter selection, subscriber setup, file creation, writer-guard ownership, and app/save target constants.
- `crates/rollshot-app/tests/diagnostic_logging.rs`: subprocess coverage for file output, console mirroring, filtering, invalid directives, and failing-path flushing.
- `crates/rollshot-core/src/diagnostics.rs`: stable stitch/matcher/verifier/canvas target constants.
- `crates/rollshot-capture/src/diagnostics.rs`: stable capture/portal/PipeWire/SCK target constants.
- `crates/rollshot-iced-overlay/src/diagnostics.rs`: stable overlay/capture/stitch target constants used by the active overlay path.
- `scripts/check-tracing-targets.sh`: mechanical check that instrumented crates use fully-qualified tracing macros with explicit targets.

Modify:

- `Cargo.toml`, `Cargo.lock`: workspace tracing dependencies.
- `crates/rollshot-app/Cargo.toml`: subscriber/appender dependencies and integration-test support.
- `crates/rollshot-app/src/launch.rs`: extract `--log-file <PATH>` before normal launch parsing.
- `crates/rollshot-app/src/main.rs`: initialize diagnostics, return `ExitCode`, and emit app lifecycle events.
- `crates/rollshot-app/src/post_capture.rs`, `storage.rs`, `macos_product.rs`: completion/save events and migration of diagnostic `eprintln!`.
- `crates/rollshot-core/Cargo.toml`, `src/lib.rs`, `src/stitcher.rs`, `src/matcher.rs`, `src/verifier.rs`, `src/canvas.rs`: critical stitching events and target constants.
- `crates/rollshot-capture/Cargo.toml`, `src/lib.rs`, `src/backend.rs`, `src/linux/mod.rs`, `src/linux/portal.rs`, `src/linux/pipewire.rs`, `src/macos/mod.rs`: backend lifecycle, platform capture diagnostics, and migration of diagnostic `eprintln!`.
- `crates/rollshot-iced-overlay/Cargo.toml`, `src/lib.rs`, `src/app.rs`, `src/driver.rs`, `src/linux_runner.rs`, `src/macos_capture.rs`: active overlay lifecycle, capture/stitch orchestration, and migration of diagnostic `eprintln!`.
- `.github/workflows/ci.yml`: run the explicit-target check.
- `README.md`: document `RUST_LOG`, stable targets, and `--log-file`.

Do not modify:

- `crates/rollshot-cli`: its `eprintln!` output is user-facing progress controlled by `--quiet`.
- `crates/rollshot-tauri-app`: deprecated capture path is out of scope.
- Benchmark and smoke-test `eprintln!` output: those messages are test/benchmark UX, not product diagnostics.

### Event Volume Contract

- `error`: terminal product/capture/save failure.
- `warn`: recoverable abnormal state, invalid filter directive, poisoned capture queue, re-anchor, or platform cleanup failure.
- `info`: session, overlay, capture, stitch, completion, and save lifecycle milestones.
- `debug`: backend decisions, phase transitions, significant stitch outcomes, capture misses, and non-terminal failures.
- `trace`: successful per-frame capture metadata and detailed per-frame stitch/matcher/verifier/canvas metrics.

Per-frame events must stay at `trace`. Do not format or clone image data, full paths, environment dumps, or error payloads that have not been reviewed for privacy.

## Task 1: Capture The Stitching Performance Baseline

**Files:**
- Create through benchmark command: `bench-results/runs/runtime-diagnostic-logging/before.jsonl`

- [ ] **Step 1: Run the pre-instrumentation stitching benchmark**

Run:

```bash
rtk mkdir -p bench-results/runs/runtime-diagnostic-logging
rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/runtime-diagnostic-logging/before.jsonl
```

Expected: benchmark workers complete and write `before.jsonl`.

- [ ] **Step 2: Confirm the baseline file contains frame and summary records**

Run:

```bash
rtk rg -n '"record":"(frame|summary)"' bench-results/runs/runtime-diagnostic-logging/before.jsonl
```

Expected: at least one frame record and one summary record. Do not commit benchmark run output.

## Task 2: Add Dependencies And Enforce Explicit Targets

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-core/Cargo.toml`
- Modify: `crates/rollshot-capture/Cargo.toml`
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Create: `crates/rollshot-app/src/diagnostics.rs`
- Create: `crates/rollshot-core/src/diagnostics.rs`
- Create: `crates/rollshot-capture/src/diagnostics.rs`
- Create: `crates/rollshot-iced-overlay/src/diagnostics.rs`
- Modify: `crates/rollshot-core/src/lib.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Create: `scripts/check-tracing-targets.sh`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add a failing explicit-target check**

Create executable `scripts/check-tracing-targets.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

paths=(
  crates/rollshot-app/src
  crates/rollshot-core/src
  crates/rollshot-capture/src
  crates/rollshot-iced-overlay/src
)

status=0

if rg -U --pcre2 -n \
  'tracing::(?:trace|debug|info|warn|error|event)!\(\s*+(?!target:)' \
  "${paths[@]}"; then
  echo "tracing macros must begin with an explicit target:" >&2
  status=1
fi

if rg -n --pcre2 \
  '(^|[^:[:alnum:]_])(trace|debug|info|warn|error|event)!\(' \
  "${paths[@]}"; then
  echo "use fully-qualified tracing macros so the target check can inspect them" >&2
  status=1
fi

exit "$status"
```

Mark it executable:

```bash
rtk chmod +x scripts/check-tracing-targets.sh
```

- [ ] **Step 2: Run the check before tracing calls exist**

Run:

```bash
rtk ./scripts/check-tracing-targets.sh
```

Expected: PASS with no output.

- [ ] **Step 3: Add tracing dependencies**

Add to `[workspace.dependencies]`:

```toml
tracing = "0.1"
tracing-appender = "0.2"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
```

Add `tracing = { workspace = true }` to `rollshot-core`, `rollshot-capture`, and `rollshot-iced-overlay`.

Add to `rollshot-app`:

```toml
tracing = { workspace = true }
tracing-appender = { workspace = true }
tracing-subscriber = { workspace = true }
```

- [ ] **Step 4: Define per-crate target constants**

Create `crates/rollshot-core/src/diagnostics.rs`:

```rust
pub(crate) const TARGET_STITCH: &str = "rollshot::stitch";
pub(crate) const TARGET_MATCHER: &str = "rollshot::stitch::matcher";
pub(crate) const TARGET_VERIFIER: &str = "rollshot::stitch::verifier";
pub(crate) const TARGET_CANVAS: &str = "rollshot::stitch::canvas";
```

Create `crates/rollshot-capture/src/diagnostics.rs`:

```rust
pub(crate) const TARGET_CAPTURE: &str = "rollshot::capture";
#[cfg(target_os = "linux")]
pub(crate) const TARGET_LINUX_PORTAL: &str = "rollshot::capture::linux::portal";
#[cfg(target_os = "linux")]
pub(crate) const TARGET_LINUX_PIPEWIRE: &str = "rollshot::capture::linux::pipewire";
#[cfg(target_os = "macos")]
pub(crate) const TARGET_MACOS_SCK: &str = "rollshot::capture::macos::sck";
```

Create `crates/rollshot-iced-overlay/src/diagnostics.rs`:

```rust
pub(crate) const TARGET_OVERLAY: &str = "rollshot::overlay";
pub(crate) const TARGET_CAPTURE: &str = "rollshot::capture";
pub(crate) const TARGET_STITCH: &str = "rollshot::stitch";
```

Start `crates/rollshot-app/src/diagnostics.rs` with:

```rust
pub(crate) const TARGET_APP: &str = "rollshot::app";
pub(crate) const TARGET_FILTER: &str = "rollshot::app::filter";
pub(crate) const TARGET_SAVE: &str = "rollshot::save";
```

Register each private module from its crate root.

`TARGET_FILTER` is an additive child target used only to guarantee that a
post-initialization invalid-directive warning is visible without broadening or
suppressing the user's filters for other app events.

- [ ] **Step 5: Add the target check to CI**

In `.github/workflows/ci.yml`, add a Rust-job step before formatting/clippy:

```yaml
- name: Check explicit tracing targets
  run: ./scripts/check-tracing-targets.sh
```

- [ ] **Step 6: Verify dependencies and the check**

Run:

```bash
rtk cargo check -p rollshot-app -p rollshot-core -p rollshot-capture -p rollshot-iced-overlay
rtk ./scripts/check-tracing-targets.sh
```

Expected: both commands PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-core/Cargo.toml crates/rollshot-capture/Cargo.toml crates/rollshot-iced-overlay/Cargo.toml crates/rollshot-app/src/diagnostics.rs crates/rollshot-core/src/diagnostics.rs crates/rollshot-capture/src/diagnostics.rs crates/rollshot-iced-overlay/src/diagnostics.rs crates/rollshot-core/src/lib.rs crates/rollshot-capture/src/lib.rs crates/rollshot-iced-overlay/src/lib.rs scripts/check-tracing-targets.sh .github/workflows/ci.yml
rtk git commit -m "build(logging): add structured diagnostics dependencies"
```

## Task 3: Parse `--log-file` Before Product Arguments

**Files:**
- Modify: `crates/rollshot-app/src/launch.rs`

- [ ] **Step 1: Write failing logging-option extraction tests**

Add:

```rust
#[test]
fn extracts_log_file_before_capture_args() {
    let extracted = extract_logging_args([
        "rollshot-app",
        "--log-file",
        "/tmp/rollshot.jsonl",
        "--capture",
        r#"{"backend":"auto","fps":5,"show_cursor":false}"#,
    ])
    .expect("extract logging args");

    assert_eq!(
        extracted.log_file,
        Some(PathBuf::from("/tmp/rollshot.jsonl"))
    );
    assert_eq!(extracted.remaining[0], "rollshot-app");
    assert_eq!(extracted.remaining[1], "--capture");
}

#[test]
fn rejects_missing_log_file_path() {
    let err = extract_logging_args(["rollshot-app", "--log-file"])
        .expect_err("missing path must fail");
    assert_eq!(err, "--log-file requires a path");
}

#[test]
fn rejects_duplicate_log_file() {
    let err = extract_logging_args([
        "rollshot-app",
        "--log-file",
        "a.jsonl",
        "--log-file",
        "b.jsonl",
    ])
    .expect_err("duplicate option must fail");
    assert_eq!(err, "--log-file may only be specified once");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app launch::tests::extracts_log_file_before_capture_args
```

Expected: FAIL because `extract_logging_args` does not exist.

- [ ] **Step 3: Implement logging-option extraction**

Add:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingArgs {
    pub log_file: Option<PathBuf>,
    pub remaining: Vec<String>,
}

pub fn extract_logging_args<I, S>(args: I) -> Result<LoggingArgs, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut input = args.into_iter().map(Into::into);
    let program = input.next().unwrap_or_else(|| "rollshot-app".to_string());
    let mut remaining = vec![program];
    let mut log_file = None;

    while let Some(arg) = input.next() {
        if arg == "--log-file" {
            if log_file.is_some() {
                return Err("--log-file may only be specified once".to_string());
            }
            let path = input
                .next()
                .ok_or_else(|| "--log-file requires a path".to_string())?;
            log_file = Some(PathBuf::from(path));
        } else {
            remaining.push(arg);
        }
    }

    Ok(LoggingArgs {
        log_file,
        remaining,
    })
}
```

Keep `parse_launch_args` responsible only for the remaining product arguments.

- [ ] **Step 4: Run launch tests**

Run:

```bash
rtk cargo test -p rollshot-app launch::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/launch.rs
rtk git commit -m "feat(app): parse diagnostic log file option"
```

## Task 4: Implement Filter Selection And Subscriber Initialization

**Files:**
- Modify: `crates/rollshot-app/src/diagnostics.rs`

- [ ] **Step 1: Write failing pure filter-selection tests**

Add tests for:

```rust
#[test]
fn absent_filter_defaults_to_warn() {
    let selected = select_filter(None);
    assert_eq!(selected.accepted, "warn");
    assert!(selected.ignored.is_empty());
}

#[test]
fn valid_directives_are_preserved() {
    let selected = select_filter(Some("warn,rollshot::capture=debug"));
    assert_eq!(selected.accepted, "warn,rollshot::capture=debug");
    assert!(selected.ignored.is_empty());
}

#[test]
fn invalid_directives_are_reported_and_valid_ones_survive() {
    let selected = select_filter(Some("warn,not a directive,rollshot::stitch=trace"));
    assert_eq!(
        selected.accepted,
        "warn,rollshot::stitch=trace,rollshot::app::filter=warn"
    );
    assert_eq!(selected.ignored, vec!["not a directive"]);
}

#[test]
fn all_invalid_directives_fall_back_to_warn() {
    let selected = select_filter(Some("not valid"));
    assert_eq!(selected.accepted, "warn,rollshot::app::filter=warn");
    assert_eq!(selected.ignored, vec!["not valid"]);
}
```

- [ ] **Step 2: Run the filter tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app diagnostics::tests::absent_filter_defaults_to_warn
```

Expected: FAIL because `select_filter` does not exist.

- [ ] **Step 3: Implement pure lossy filter selection**

Add:

```rust
const DEFAULT_FILTER: &str = "warn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedFilter {
    pub(crate) accepted: String,
    pub(crate) ignored: Vec<String>,
}

pub(crate) fn select_filter(raw: Option<&str>) -> SelectedFilter {
    let Some(raw) = raw else {
        return SelectedFilter {
            accepted: DEFAULT_FILTER.to_string(),
            ignored: Vec::new(),
        };
    };
    let mut accepted = Vec::new();
    let mut ignored = Vec::new();

    for part in raw.split(',').filter(|part| !part.is_empty()) {
        if EnvFilter::try_new(part).is_ok() {
            accepted.push(part);
        } else {
            ignored.push(part.to_string());
        }
    }

    if accepted.is_empty() {
        accepted.push(DEFAULT_FILTER);
    }
    if !ignored.is_empty() {
        accepted.push("rollshot::app::filter=warn");
    }

    SelectedFilter {
        accepted: accepted.join(","),
        ignored,
    }
}
```

This mirrors `tracing-subscriber`'s comma-separated lossy parser while retaining rejected directives for the post-init warning.

- [ ] **Step 4: Write failing file-open tests**

Add tests using `tempfile::tempdir()`:

```rust
#[test]
fn open_log_file_truncates_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollshot.jsonl");
    std::fs::write(&path, "old data").unwrap();
    drop(open_log_file(&path).unwrap());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "");
}

#[test]
fn open_log_file_rejects_missing_parent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing").join("rollshot.jsonl");
    assert!(open_log_file(&path).is_err());
}
```

- [ ] **Step 5: Implement subscriber initialization**

Implement:

```rust
use std::fs::{File, OpenOptions};
use std::path::Path;
use tracing_appender::non_blocking::{ErrorCounter, WorkerGuard};
use tracing_subscriber::layer::{SubscriberExt, Layer};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

pub(crate) struct DiagnosticsGuard {
    _file_guard: Option<WorkerGuard>,
    dropped_lines: Option<ErrorCounter>,
}

fn open_log_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|err| format!("failed to open diagnostic log {}: {err}", path.display()))
}

pub(crate) fn init(
    log_file: Option<&Path>,
    selected: &SelectedFilter,
) -> Result<DiagnosticsGuard, String> {
    let console_filter = EnvFilter::try_new(&selected.accepted)
        .map_err(|err| format!("failed to build diagnostic filter: {err}"))?;
    let console = fmt::layer()
        .compact()
        .with_writer(std::io::stderr)
        .with_filter(console_filter);

    let (file_guard, dropped_lines) = match log_file {
        Some(path) => {
            let file = open_log_file(path)?;
            let (writer, guard) = tracing_appender::non_blocking(file);
            let dropped_lines = writer.error_counter();
            let file_filter = EnvFilter::try_new(&selected.accepted)
                .map_err(|err| format!("failed to build diagnostic filter: {err}"))?;
            let file_layer = fmt::layer()
                .json()
                .with_writer(writer)
                .with_filter(file_filter);
            tracing_subscriber::registry()
                .with(console)
                .with(file_layer)
                .try_init()
                .map_err(|err| format!("failed to initialize diagnostics: {err}"))?;
            (Some(guard), Some(dropped_lines))
        }
        None => {
            tracing_subscriber::registry()
                .with(console)
                .try_init()
                .map_err(|err| format!("failed to initialize diagnostics: {err}"))?;
            (None, None)
        }
    };

    Ok(DiagnosticsGuard {
        _file_guard: file_guard,
        dropped_lines,
    })
}

impl Drop for DiagnosticsGuard {
    fn drop(&mut self) {
        let dropped_lines = self
            .dropped_lines
            .as_ref()
            .map(ErrorCounter::dropped_lines)
            .unwrap_or(0);
        if dropped_lines > 0 {
            tracing::warn!(
                target: TARGET_APP,
                dropped_lines,
                "diagnostic file writer dropped events"
            );
        }
    }
}
```

- [ ] **Step 6: Verify diagnostics unit tests**

Run:

```bash
rtk cargo test -p rollshot-app diagnostics::tests
rtk cargo check -p rollshot-app
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/diagnostics.rs
rtk git commit -m "feat(app): initialize runtime diagnostic logging"
```

## Task 5: Return `ExitCode` And Verify Failing-Path Flush

**Files:**
- Modify: `crates/rollshot-app/src/main.rs`
- Create: `crates/rollshot-app/tests/diagnostic_logging.rs`

- [ ] **Step 1: Write a failing subprocess integration test**

Create a test that runs a parsed-but-failing backend so diagnostics initialize and capture configuration is available:

```rust
use std::process::Command;

#[test]
fn failing_launch_flushes_json_log_and_keeps_console_output() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("rollshot.jsonl");
    let payload = r#"{"backend":"macos-sck","fps":5,"show_cursor":false}"#;

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-app"))
        .env("RUST_LOG", "warn,rollshot=debug")
        .args([
            "--log-file",
            log_path.to_str().unwrap(),
            "--capture",
            payload,
        ])
        .output()
        .expect("run rollshot-app");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("application failed"), "stderr = {stderr}");

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(log.contains("\"message\":\"capture session started\""), "log = {log}");
    assert!(log.contains("\"message\":\"application failed\""), "log = {log}");
    assert!(log.ends_with('\n'), "log must be completely flushed");
}
```

On macOS, use `"linux-portal"` instead. Select the foreign-platform backend with `cfg!` so the failure occurs before opening UI.

- [ ] **Step 2: Run the subprocess test to verify it fails**

Run:

```bash
rtk cargo test -p rollshot-app --test diagnostic_logging
```

Expected: FAIL because `--log-file` is not wired into `main`.

- [ ] **Step 3: Refactor `main` to retain the guard and return `ExitCode`**

Implement this control flow:

```rust
mod diagnostics;

use std::process::ExitCode;

fn main() -> ExitCode {
    let logging = match launch::extract_logging_args(std::env::args()) {
        Ok(logging) => logging,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    let selected = diagnostics::select_filter(std::env::var("RUST_LOG").ok().as_deref());
    let _diagnostics = match diagnostics::init(logging.log_file.as_deref(), &selected) {
        Ok(guard) => guard,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    if !selected.ignored.is_empty() {
        tracing::warn!(
            target: diagnostics::TARGET_FILTER,
            ignored = ?selected.ignored,
            "ignored invalid RUST_LOG directives"
        );
    }

    match run(logging.remaining, logging.log_file.is_some()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(
                target: diagnostics::TARGET_APP,
                error_category = diagnostics::classify_app_error(&error),
                "application failed"
            );
            ExitCode::FAILURE
        }
    }
}
```

Make `run` parse `LaunchMode`, emit `"capture session started"` with version, OS, architecture, backend, FPS, cursor setting, initial mode, and `file_logging`, then call `run_iced_capture`. Change `run_iced_capture` to return `Result<(), String>` and remove all `std::process::exit` calls.

Implement `classify_app_error` as a small closed-category classifier for the
known top-level failures (`launch`, `capture`, `overlay`, `save`, `workspace`,
and `unknown`). Do not attach the raw top-level string to the event because
save/workspace errors may contain full user paths.

- [ ] **Step 4: Add subprocess cases for filtering and invalid directives**

Add tests that assert:

- `RUST_LOG=error` omits the debug `"capture session started"` event but includes final error.
- `RUST_LOG=warn,rollshot::app=debug,not valid` writes `"ignored invalid RUST_LOG directives"` and still launches.
- The diagnostic JSON never contains the full output/save path; the only allowed explicit path in this test is the user-supplied diagnostic path, which is not emitted as a field.

- [ ] **Step 5: Run app tests and explicit-target check**

Run:

```bash
rtk cargo test -p rollshot-app
rtk ./scripts/check-tracing-targets.sh
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/main.rs crates/rollshot-app/tests/diagnostic_logging.rs
rtk git commit -m "feat(app): flush diagnostics on failing exits"
```

## Task 6: Instrument The Stitching Core

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Modify: `crates/rollshot-core/src/matcher.rs`
- Modify: `crates/rollshot-core/src/verifier.rs`
- Modify: `crates/rollshot-core/src/canvas.rs`
- Modify: `crates/rollshot-core/tests/metrics_population.rs`

- [ ] **Step 1: Add a failing stitch target/behavior test**

Extend a metrics test to install a scoped test subscriber with a writer buffer, enable `rollshot::stitch=trace`, push representative frames, and assert the output contains:

```text
target=rollshot::stitch
message="processed stitch frame"
frame_index
outcome
total_us
canvas_logical_pixels
```

Also assert no pixel buffers or fixture paths appear.

- [ ] **Step 2: Run the focused test to verify it fails**

Run:

```bash
rtk cargo test -p rollshot-core --test metrics_population -- --nocapture
```

Expected: FAIL because the stitch event does not exist.

- [ ] **Step 3: Emit stitch lifecycle and per-frame events**

At the end of `Stitcher::push_frame`, after `last_metrics.total_us` is populated:

```rust
let metrics = &self.last_metrics;
tracing::trace!(
    target: crate::diagnostics::TARGET_STITCH,
    frame_index = metrics.frame_index,
    outcome = ?metrics.outcome,
    no_match_reason = ?metrics.no_match_reason,
    total_us = metrics.total_us,
    best_dx = metrics.best_dx,
    best_dy = metrics.best_dy,
    best_score = metrics.best_score,
    second_best_score = ?metrics.second_best_score,
    match_method = ?metrics.match_method,
    canvas_logical_pixels = metrics.canvas_logical_pixels,
    canvas_allocated_bytes = metrics.canvas_allocated_bytes,
    "processed stitch frame"
);
```

Emit `debug` only for `NoMatch`, `AxisChanged`, and accepted append state transitions. Replace the mid-capture re-anchor `eprintln!` with a `warn` event containing miss count and canvas height.

- [ ] **Step 4: Add narrow child-target trace events**

Add low-cost `trace` events at these completed decisions:

- `matcher.rs`: selected candidate or final no-match reason with candidate counts, offsets, scores, and method under `TARGET_MATCHER`.
- `verifier.rs`: pass/insufficient-overlap/disagreement outcome with offsets, overlap area, and MAD scores under `TARGET_VERIFIER`.
- `canvas.rs`: successful append with direction, added pixels, logical dimensions, allocated bytes, and copied bytes under `TARGET_CANVAS`.

Do not emit inside inner pixel/NCC loops.

- [ ] **Step 5: Run core tests and target check**

Run:

```bash
rtk cargo test -p rollshot-core
rtk ./scripts/check-tracing-targets.sh
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/src/matcher.rs crates/rollshot-core/src/verifier.rs crates/rollshot-core/src/canvas.rs crates/rollshot-core/tests/metrics_population.rs
rtk git commit -m "feat(core): instrument stitching diagnostics"
```

## Task 7: Instrument Capture Backends And Migrate Capture Prints

**Files:**
- Modify: `crates/rollshot-capture/src/backend.rs`
- Modify: `crates/rollshot-capture/src/linux/mod.rs`
- Modify: `crates/rollshot-capture/src/linux/portal.rs`
- Modify: `crates/rollshot-capture/src/linux/pipewire.rs`
- Modify: `crates/rollshot-capture/src/macos/mod.rs`

- [ ] **Step 1: Write failing backend-decision tests**

Add a scoped-subscriber unit test around `BackendKind::from_cli_flag`/`create` that enables `rollshot::capture=debug` and asserts an unknown backend emits a classified `debug` event without logging arbitrary environment contents.

For Linux queue poison paths, extend existing tests to capture and assert `rollshot::capture::linux::pipewire` warning events.

- [ ] **Step 2: Run capture tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-capture
```

Expected: new event assertions FAIL.

- [ ] **Step 3: Add backend-independent capture events**

Emit under `TARGET_CAPTURE`:

- `debug`: requested backend flag and resolved backend kind.
- `info`: backend start with FPS, cursor setting, region mode category, and whether a target display was supplied.
- `error` or `warn`: classified unsupported, permission, invalid config, timeout, end-of-stream, and backend failures at the control-flow boundary that handles them.

Do not log `XDG_SESSION_TYPE` values beyond the already-classified backend decision, and do not dump `CaptureError::Backend` payloads without reviewing the source.

- [ ] **Step 4: Replace Linux portal/PipeWire diagnostic prints**

Replace:

- `ROLLSHOT_CAPTURE_TRACE`-gated portal stage prints with `debug` events under `TARGET_LINUX_PORTAL`; remove the custom environment-variable gate.
- PipeWire connection/start/disconnect and queue-poison prints with `debug`/`warn` events under `TARGET_LINUX_PIPEWIRE`.

Add `trace` frame events only after successful frame conversion/dequeue, containing sequence-independent metadata such as dimensions, pixel format enum, and source-size presence.

- [ ] **Step 5: Add macOS SCK events**

Under `TARGET_MACOS_SCK`, emit:

- `info` for start/stop lifecycle and selected display/region category.
- `warn` for permission or capturer startup failure.
- `trace` for successful frame dimensions and SCK idle timeout counts.
- `debug` for end-of-stream classification.

- [ ] **Step 6: Run capture tests and cross-platform compile checks**

Run on the current host:

```bash
rtk cargo test -p rollshot-capture
rtk cargo check -p rollshot-capture --all-targets
rtk ./scripts/check-tracing-targets.sh
```

Expected: PASS. CI remains responsible for compiling the opposite platform-specific modules.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-capture/src/backend.rs crates/rollshot-capture/src/linux/mod.rs crates/rollshot-capture/src/linux/portal.rs crates/rollshot-capture/src/linux/pipewire.rs crates/rollshot-capture/src/macos/mod.rs
rtk git commit -m "feat(capture): instrument backend diagnostics"
```

## Task 8: Instrument Active Iced Overlay Capture And Stitch Orchestration

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs`

- [ ] **Step 1: Write failing overlay/driver event tests**

Use existing fake stream and overlay state tests with a scoped subscriber to assert:

- Driver start emits backend/FPS/cursor configuration under `rollshot::capture`.
- Begin stitch emits mapped crop and source dimensions under `rollshot::stitch`.
- Finalize emits final dimensions and stitch stats.
- Cancel/finish/phase transitions emit under `rollshot::overlay`.
- Per-frame success remains `trace`, while reader/crop/finalize failures are `warn` or `error`.

- [ ] **Step 2: Run overlay tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
```

Expected: new event assertions FAIL.

- [ ] **Step 3: Instrument overlay lifecycle and phase changes**

Emit `info`/`debug` events under `TARGET_OVERLAY` for:

- Blocking Linux overlay start, cancellation, completion, and failure.
- macOS embedded component start, terminal effects, and shutdown.
- Capture mode activation and workspace phase transitions.
- Finish/cancel actions.

Replace the macOS overlay-window and passthrough diagnostic `eprintln!` calls with explicit-target events.

- [ ] **Step 4: Instrument the live driver**

Emit under `TARGET_CAPTURE` and `TARGET_STITCH`:

- Backend creation/start and first-frame wait outcome.
- Reader end-of-stream, timeout sampling, and terminal error.
- Source size, mapped crop rectangle, and begin-stitch configuration.
- Significant `StitchOutcome`/capture-miss transitions at `debug`.
- Successful frame and full stitch metrics only at `trace`.
- Final dimensions/stats, cancellation, and thread-join failures.

Do not duplicate the core's detailed matcher/verifier/canvas events at `debug`.

- [ ] **Step 5: Run overlay tests and explicit-target check**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo check -p rollshot-iced-overlay --all-targets
rtk ./scripts/check-tracing-targets.sh
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/app.rs crates/rollshot-iced-overlay/src/driver.rs crates/rollshot-iced-overlay/src/linux_runner.rs crates/rollshot-iced-overlay/src/macos_capture.rs
rtk git commit -m "feat(overlay): instrument active capture lifecycle"
```

## Task 9: Instrument Completion, Save, And Product Errors

**Files:**
- Modify: `crates/rollshot-app/src/post_capture.rs`
- Modify: `crates/rollshot-app/src/storage.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`

- [ ] **Step 1: Write failing completion/save event tests**

Add scoped-subscriber tests asserting:

- Cancelled and present completion dispositions emit under `TARGET_APP`.
- Save start/success/failure events emit under `TARGET_SAVE`.
- Save events contain image width/height, encoded byte count when available, and destination category.
- Save events do not contain full paths such as `/home/noah/Desktop/Rollshot ...png`.

- [ ] **Step 2: Run focused app tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app post_capture::tests storage::tests
```

Expected: new event assertions FAIL.

- [ ] **Step 3: Add privacy-safe completion and save events**

Emit:

- `info` for capture present/cancelled and presentation selected.
- `info` for save start/success with width, height, encoded bytes, platform, and destination category (`desktop`, `save_as`, or `unknown`).
- `error` for save failure with a reviewed error category, not a full path.

Keep user-visible error strings unchanged; logging is additive except where it replaces diagnostic-only prints.

- [ ] **Step 4: Replace macOS product diagnostic prints**

Replace the diagnostic `eprintln!` calls in `macos_product.rs` with `error`/`warn` events under `TARGET_APP`. Preserve the existing UI/exit behavior and do not log saved paths used by native drag or workspace reveal.

- [ ] **Step 5: Run app tests and target check**

Run:

```bash
rtk cargo test -p rollshot-app
rtk ./scripts/check-tracing-targets.sh
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/post_capture.rs crates/rollshot-app/src/storage.rs crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(app): instrument completion and save diagnostics"
```

## Task 10: Document Diagnostic Usage

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a runtime diagnostics section**

Document these exact workflows:

```bash
# Default: warnings and errors on the console.
rollshot-app

# Debug all Rollshot subsystems on the console.
RUST_LOG=warn,rollshot=debug rollshot-app

# Capture-only diagnostics in an explicit JSONL file and on the console.
RUST_LOG=warn,rollshot::capture=debug rollshot-app --log-file ./rollshot-debug.jsonl

# Stitch decisions plus detailed matcher events.
RUST_LOG=warn,rollshot::stitch=debug,rollshot::stitch::matcher=trace rollshot-app --log-file ./rollshot-debug.jsonl
```

State that the file is truncated per launch, parent directories must exist, console output remains enabled, release builds retain debug/trace events, and logs intentionally omit captured pixels and full save paths.

List the stable targets from the spec.
Also document `rollshot::app::filter` as the additive child target reserved for
invalid-`RUST_LOG` warnings.

- [ ] **Step 2: Verify documented flags and targets against code**

Run:

```bash
rtk rg -n -- '--log-file|rollshot::capture|rollshot::stitch::matcher' README.md crates/rollshot-app/src crates/rollshot-core/src crates/rollshot-capture/src crates/rollshot-iced-overlay/src
```

Expected: every documented option/target has a matching implementation.

- [ ] **Step 3: Commit**

```bash
rtk git add README.md
rtk git commit -m "docs(logging): document runtime diagnostics"
```

## Task 11: Final Verification And Performance Comparison

**Files:**
- Create through benchmark command: `bench-results/runs/runtime-diagnostic-logging/after.jsonl`
- Do not commit benchmark run output.

- [ ] **Step 1: Confirm diagnostic prints were migrated only in scope**

Run:

```bash
rtk rg -n 'eprintln!|std::process::exit|process::exit' crates/rollshot-app crates/rollshot-capture/src crates/rollshot-iced-overlay/src crates/rollshot-core/src --glob '*.rs'
```

Expected:

- No `std::process::exit` in `rollshot-app`.
- No diagnostic `eprintln!` in active app/capture/overlay/core product paths.
- Test/benchmark/developer-binary output may remain where it is user-facing or outside the active product path.

- [ ] **Step 2: Run explicit-target, formatting, tests, and clippy checks**

Run:

```bash
rtk ./scripts/check-tracing-targets.sh
rtk cargo fmt --check
rtk cargo test
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all PASS.

- [ ] **Step 3: Verify release binary filtering and failing-path flush**

Run:

```bash
rtk cargo test -p rollshot-app --release --test diagnostic_logging
```

Expected: PASS, proving debug events remain available in release and the file guard flushes on a returned non-zero exit.

- [ ] **Step 4: Run the post-instrumentation stitching benchmark**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/runtime-diagnostic-logging/after.jsonl
rtk python3 scripts/bench/compare.py bench-results/runs/runtime-diagnostic-logging/before.jsonl bench-results/runs/runtime-diagnostic-logging/after.jsonl
```

Expected: disabled/default logging has no material stitching regression. Investigate any consistent regression beyond ordinary run-to-run noise before completion.

- [ ] **Step 5: Perform platform runtime verification**

Linux:

```bash
rtk env RUST_LOG=warn,rollshot::capture=debug,rollshot::overlay=debug cargo run -p rollshot-app -- --log-file /tmp/rollshot-linux.jsonl
```

Verify portal/PipeWire capture, finish/cancel, post-capture save/workspace handoff, console output, and JSONL file.

macOS:

```bash
rtk env RUST_LOG=warn,rollshot::capture=debug,rollshot::overlay=debug cargo run -p rollshot-app -- --log-file /tmp/rollshot-macos.jsonl
```

Verify ScreenCaptureKit capture, finish/cancel, thumbnail/workspace handoff, console output, and JSONL file.

If only one platform is available, record the unchecked counterpart and its remaining runtime risk in the final response.

- [ ] **Step 6: Commit any verification-driven fixes**

Stage only files changed to correct verification findings, then commit with a scoped conventional message. Do not commit `bench-results/runs/runtime-diagnostic-logging/*.jsonl`.
