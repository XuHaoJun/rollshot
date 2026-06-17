# rollshot-app clap Flag Launch Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `rollshot-app`'s hand-rolled `--capture '{json}'` launch interface with a clap-derived subcommand + flag surface that mirrors `rollshot-cli`, and update the README examples.

**Architecture:** `crates/rollshot-app/src/launch.rs` is rewritten to define clap `Parser`/`Subcommand`/`Args`/`ValueEnum` types and a pure `resolve_launch_mode` that lowers the parsed command into the existing `LaunchMode` enum. `main.rs` parses with clap once, initializes diagnostics from the global `--log-file`, then dispatches the unchanged downstream paths. The `--capture` JSON path and the two-pass `--log-file` splitter are deleted.

**Tech Stack:** Rust, clap 4 (derive), `rollshot_capture` types (`Workflow`, `CaptureScope`, `CaptureRequest`, `InteractiveLaunchOptions`).

## Global Constraints

- Rust edition/toolchain per workspace; Rust 1.85+ (workspace `rust-version`).
- Workspace is `unsafe_code = "forbid"`; `rollshot-app` narrows to `deny` — do not add unsafe.
- Diagnostics: use `tracing` with stable `rollshot::*` targets; no `println!`/`eprintln!`/`dbg!` as diagnostics. clap's own usage/error output to stderr is the allowed pre-subscriber stderr case (AGENTS.md §7).
- Defaults must match `InteractiveLaunchOptions::default_capture()`: `backend=auto`, `fps=5`, `show_cursor=false`, `workflow=scrolling`, `scope=region`.
- `--workflow` exposes only `screenshot` and `scrolling`; the `action-guide` workflow has its own subcommand.
- `action-guide` and `action-guide-probe` subcommands stay behind the `action-guide` cargo feature.
- `rollshot-cli` is not modified.
- Shell commands are prefixed with `rtk`.

---

### Task 1: clap launch surface in `rollshot-app`

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml` (add `clap`, remove `serde_json`)
- Modify (full rewrite): `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/main.rs` (`main` flow + `run` signature + tests)

**Interfaces:**
- Consumes: `rollshot_capture::{InteractiveLaunchOptions, CaptureRequest, Workflow, CaptureScope}`.
- Produces (used by `main.rs`):
  - `pub struct LaunchCli { pub log_file: Option<PathBuf>, pub command: Option<LaunchCommand> }` (derives `clap::Parser`)
  - `pub enum LaunchCommand` (derives `clap::Subcommand`): `Capture(CaptureArgs)`, plus feature-gated `ActionGuide { fullscreen: bool }` and `ActionGuideProbe`.
  - `pub enum LaunchMode` — unchanged variants: `Capture(InteractiveLaunchOptions)`, feature-gated `ActionGuideProbe`, `ActionGuide { fullscreen: bool }`.
  - `pub fn resolve_launch_mode(command: Option<LaunchCommand>) -> Result<LaunchMode, String>`

- [ ] **Step 1: Update `Cargo.toml` dependencies**

In `crates/rollshot-app/Cargo.toml`, inside `[dependencies]`, add `clap` and remove the `serde_json` line (only `launch.rs` used it; that use is deleted in this task). Final dependency lines should include:

```toml
clap = { workspace = true }
```

and must no longer contain:

```toml
serde_json = { workspace = true }
```

- [ ] **Step 2: Rewrite `crates/rollshot-app/src/launch.rs`**

Replace the entire file contents with:

```rust
use clap::{Parser, Subcommand, ValueEnum};
use rollshot_capture::{CaptureRequest, CaptureScope, InteractiveLaunchOptions, Workflow};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
    #[cfg(feature = "action-guide")]
    ActionGuide {
        fullscreen: bool,
    },
}

/// Top-level launch parser for the interactive capture app. Running with no
/// subcommand is equivalent to `capture` with all defaults.
#[derive(Debug, Parser)]
#[command(name = "rollshot-app", version, about = "rollshot interactive capture app")]
pub struct LaunchCli {
    /// Write the diagnostic session to a JSONL file alongside console output.
    #[arg(long, global = true)]
    pub log_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<LaunchCommand>,
}

#[derive(Debug, Subcommand)]
pub enum LaunchCommand {
    /// Capture a screenshot or scrolling capture (default when no subcommand).
    Capture(CaptureArgs),

    /// Record a desktop workflow into an Action Guide.
    #[cfg(feature = "action-guide")]
    ActionGuide {
        /// Record the whole display instead of selecting a region. The
        /// recording is stopped by clicking the temporary system-tray icon
        /// (Linux/KDE only).
        #[arg(long, default_value_t = false)]
        fullscreen: bool,
    },

    /// Probe Action Guide input capability and exit.
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
}

#[derive(Debug, clap::Args)]
pub struct CaptureArgs {
    /// Which capture backend to use.
    #[arg(
        long,
        default_value = "auto",
        value_parser = ["auto", "fixture", "linux-kwin", "linux-portal", "macos-sck"],
    )]
    pub backend: String,

    /// Capture frame rate (used by real backends).
    #[arg(long, default_value_t = 5)]
    pub fps: u32,

    /// Include the cursor in captured frames.
    #[arg(long, default_value_t = false)]
    pub show_cursor: bool,

    /// What to do with the captured frames.
    #[arg(long, value_enum, default_value_t = WorkflowArg::Scrolling)]
    pub workflow: WorkflowArg,

    /// What area to capture.
    #[arg(long, value_enum, default_value_t = ScopeArg::Region)]
    pub scope: ScopeArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum WorkflowArg {
    Screenshot,
    Scrolling,
}

impl From<WorkflowArg> for Workflow {
    fn from(value: WorkflowArg) -> Self {
        match value {
            WorkflowArg::Screenshot => Workflow::Screenshot,
            WorkflowArg::Scrolling => Workflow::Scrolling,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ScopeArg {
    Region,
    Fullscreen,
}

impl From<ScopeArg> for CaptureScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::Region => CaptureScope::Region,
            ScopeArg::Fullscreen => CaptureScope::Fullscreen,
        }
    }
}

/// Lower a parsed launch command into a `LaunchMode`. `None` (no subcommand)
/// resolves to the default capture options. Rejects the unwired
/// `scrolling + fullscreen` capture combination with a clear message.
pub fn resolve_launch_mode(command: Option<LaunchCommand>) -> Result<LaunchMode, String> {
    match command {
        None => Ok(LaunchMode::Capture(
            InteractiveLaunchOptions::default_capture(),
        )),
        Some(LaunchCommand::Capture(args)) => {
            let request = CaptureRequest {
                workflow: args.workflow.into(),
                scope: args.scope.into(),
            };
            if !request.is_supported() {
                return Err(
                    "unsupported capture combination: scrolling + fullscreen is not wired; \
                     use scrolling + region or screenshot + fullscreen"
                        .to_string(),
                );
            }
            Ok(LaunchMode::Capture(InteractiveLaunchOptions {
                backend: args.backend,
                fps: args.fps,
                show_cursor: args.show_cursor,
                initial_request: request,
            }))
        }
        #[cfg(feature = "action-guide")]
        Some(LaunchCommand::ActionGuide { fullscreen }) => {
            Ok(LaunchMode::ActionGuide { fullscreen })
        }
        #[cfg(feature = "action-guide")]
        Some(LaunchCommand::ActionGuideProbe) => Ok(LaunchMode::ActionGuideProbe),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_launch_mode, LaunchCli, LaunchMode};
    use clap::Parser;
    use rollshot_capture::CaptureRequest;

    fn parse(args: &[&str]) -> Result<LaunchMode, String> {
        let cli = LaunchCli::try_parse_from(args).map_err(|e| e.to_string())?;
        resolve_launch_mode(cli.command)
    }

    #[test]
    fn no_subcommand_uses_defaults() {
        let mode = parse(&["rollshot-app"]).expect("no args should succeed");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "auto");
                assert_eq!(options.fps, 5);
                assert!(!options.show_cursor);
                assert_eq!(options.initial_request, CaptureRequest::scrolling_region());
            }
            #[cfg(feature = "action-guide")]
            _ => unreachable!("test expects Capture mode"),
        }
    }

    #[test]
    fn capture_backend_and_fps_flags() {
        let mode = parse(&["rollshot-app", "capture", "--backend", "macos-sck", "--fps", "30"])
            .expect("parse capture flags");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "macos-sck");
                assert_eq!(options.fps, 30);
                assert_eq!(options.initial_request, CaptureRequest::scrolling_region());
            }
            #[cfg(feature = "action-guide")]
            _ => unreachable!("test expects Capture mode"),
        }
    }

    #[test]
    fn capture_show_cursor_flag() {
        let mode = parse(&["rollshot-app", "capture", "--show-cursor"]).expect("parse");
        match mode {
            LaunchMode::Capture(options) => assert!(options.show_cursor),
            #[cfg(feature = "action-guide")]
            _ => unreachable!("test expects Capture mode"),
        }
    }

    #[test]
    fn capture_screenshot_fullscreen() {
        let mode = parse(&[
            "rollshot-app",
            "capture",
            "--workflow",
            "screenshot",
            "--scope",
            "fullscreen",
        ])
        .expect("parse");
        assert!(matches!(
            mode,
            LaunchMode::Capture(options)
                if options.initial_request == CaptureRequest::screenshot_fullscreen()
        ));
    }

    #[test]
    fn scrolling_fullscreen_is_rejected() {
        let err = parse(&["rollshot-app", "capture", "--scope", "fullscreen"])
            .expect_err("scrolling + fullscreen should be rejected");
        assert!(err.contains("scrolling"), "err = {err}");
        assert!(err.contains("fullscreen"), "err = {err}");
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let err = parse(&["rollshot-app", "capture", "--bogus"]).expect_err("unknown flag");
        assert!(!err.is_empty());
    }

    #[test]
    fn unknown_subcommand_is_rejected() {
        let err = parse(&["rollshot-app", "bogus"]).expect_err("unknown subcommand");
        assert!(!err.is_empty());
    }

    #[test]
    fn save_dialog_temp_is_rejected() {
        let err = parse(&["rollshot-app", "--save-dialog-temp", "/tmp/rollshot.png"])
            .expect_err("save-dialog-temp should be rejected");
        assert!(!err.is_empty());
    }

    #[test]
    fn log_file_global_before_subcommand() {
        let cli = LaunchCli::try_parse_from(["rollshot-app", "--log-file", "/tmp/x.jsonl"])
            .expect("parse log-file");
        assert_eq!(cli.log_file.as_deref(), Some(std::path::Path::new("/tmp/x.jsonl")));
        assert!(cli.command.is_none());
    }

    #[test]
    fn log_file_global_after_subcommand() {
        let cli = LaunchCli::try_parse_from([
            "rollshot-app",
            "capture",
            "--log-file",
            "/tmp/x.jsonl",
            "--backend",
            "auto",
        ])
        .expect("parse log-file after subcommand");
        assert_eq!(cli.log_file.as_deref(), Some(std::path::Path::new("/tmp/x.jsonl")));
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_without_fullscreen() {
        let mode = parse(&["rollshot-app", "action-guide"]).expect("parse");
        assert!(matches!(mode, LaunchMode::ActionGuide { fullscreen: false }));
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_with_fullscreen() {
        let mode = parse(&["rollshot-app", "action-guide", "--fullscreen"]).expect("parse");
        assert!(matches!(mode, LaunchMode::ActionGuide { fullscreen: true }));
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_probe_mode() {
        let mode = parse(&["rollshot-app", "action-guide-probe"]).expect("parse");
        assert!(matches!(mode, LaunchMode::ActionGuideProbe));
    }
}
```

- [ ] **Step 3: Update `crates/rollshot-app/src/main.rs`**

Update the imports near the top of the file. Change:

```rust
use launch::LaunchMode;
use std::process::ExitCode;
```

to:

```rust
use clap::Parser;
use launch::{LaunchCommand, LaunchMode};
use std::process::ExitCode;
```

Replace the entire `fn main()` body (lines from `let logging = match launch::extract_logging_args(...)` through the closing `}` of `main`) with:

```rust
fn main() -> ExitCode {
    let cli = launch::LaunchCli::parse();

    let selected = diagnostics::select_filter(std::env::var("RUST_LOG").ok().as_deref());
    let _diagnostics = match diagnostics::init(cli.log_file.as_deref(), &selected) {
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

    match run(cli.command, cli.log_file.is_some()) {
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

Replace the `run` function signature and its first line. Change:

```rust
fn run(args: Vec<String>, file_logging: bool) -> Result<(), String> {
    let launch_mode = launch::parse_launch_args(args)?;
```

to:

```rust
fn run(command: Option<LaunchCommand>, file_logging: bool) -> Result<(), String> {
    let launch_mode = launch::resolve_launch_mode(command)?;
```

The rest of `run` (the `match launch_mode { ... }` block, including the `capture session started` tracing event) is unchanged.

In the `#[cfg(test)] mod tests` block at the bottom of `main.rs`, replace both tests with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn run_returns_error_for_unsupported_platform() {
        // `None` resolves to default capture, which reaches the platform guard.
        let err = run(None, false);
        assert!(err.is_err());
    }
}
```

(The `save_dialog_temp_mode_is_no_longer_accepted` and the JSON-based unsupported-platform test are removed; the `--save-dialog-temp` rejection is now covered by `launch::tests::save_dialog_temp_is_rejected`.)

- [ ] **Step 4: Run the app unit tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS (all `launch::tests` cases and the `main` platform test on non-linux/macos; on linux/macos the platform test is cfg'd out).

- [ ] **Step 5: Build with the action-guide feature**

Run: `rtk cargo build -p rollshot-app --features action-guide`
Expected: compiles cleanly (verifies the feature-gated subcommands and match arms).

- [ ] **Step 6: Format and lint**

Run: `rtk cargo fmt --all -- --check`
Expected: no diff.

Run: `rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): replace --capture JSON launch with clap subcommands"
```

---

### Task 2: README example cleanup

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: the flag surface from Task 1 (`capture --backend ... --fps ... --workflow ... --scope ...`, `action-guide [--fullscreen]`, `action-guide-probe`, global `--log-file`).
- Produces: documentation only.

- [ ] **Step 1: Rewrite the KDE "Verification commands" block**

In `README.md`, replace the three `--capture '{json}'` verification commands (the block under "**Verification commands** (after local install):") with:

````markdown
```bash
~/.local/bin/rollshot-app capture --backend auto --fps 5
~/.local/bin/rollshot-app capture --backend linux-kwin --fps 5
~/.local/bin/rollshot-app capture --backend linux-portal --fps 5
```
````

- [ ] **Step 2: Rewrite the "One-shot dev run" launch line**

In the one-shot dev run fenced block, replace the trailing launch command:

```
~/.local/bin/rollshot-app --capture '{"backend":"auto","fps":30,"show_cursor":false,"initial_request":{"workflow":"screenshot","scope":"region"}}'
```

with:

```
~/.local/bin/rollshot-app capture --backend auto --fps 30 --workflow screenshot --scope region
```

(Leave the `cargo build --release` / `install` / `sed` / `update-desktop-database` lines above it unchanged.)

- [ ] **Step 3: Replace the `#### initial_request JSON` subsection**

Delete the entire `#### `initial_request` JSON` subsection (its intro paragraph, the JSON fenced block, the "default is ... when the field is omitted" paragraph, and the "`Scrolling × Fullscreen` is expressible but not wired" paragraph) and replace it with:

````markdown
#### `--workflow` and `--scope`

`rollshot-app capture` takes two orthogonal axes — **workflow** (what we do with
the frames) and **scope** (what area we capture):

- `--workflow <screenshot|scrolling>` (default `scrolling`)
- `--scope <region|fullscreen>` (default `region`)

```bash
rollshot-app capture --backend auto --fps 5 --workflow scrolling --scope region
rollshot-app capture --backend auto --fps 5 --workflow screenshot --scope region
rollshot-app capture --backend auto --fps 5 --workflow screenshot --scope fullscreen
```

The default is `--workflow scrolling --scope region`; running `rollshot-app`
with no subcommand uses these defaults.

`--workflow scrolling --scope fullscreen` is expressible but not wired — passing
it returns an error.

Fullscreen scope captures the display containing the pointer, skipping the
selection overlay. It is supported on macOS and KDE/KWin. On other Linux
environments without portal fallback, fullscreen returns an `Unsupported` error.
````

- [ ] **Step 4: Confirm every remaining example names its binary**

Scan `README.md` for any leftover `--capture` occurrences and for command blocks whose binary is ambiguous. Verify:

Run: `rtk rg -n "\-\-capture|initial_request|initial_mode" README.md`
Expected: no matches.

Confirm CLI examples keep their `cargo run -p rollshot-cli --` (or `rollshot`) prefix and app examples use `rollshot-app`, so each block's binary is unambiguous. The `RUST_LOG=... rollshot-app --log-file ...` quick-start examples remain valid (no subcommand = default capture, global `--log-file`) and need no change.

- [ ] **Step 5: Commit**

```bash
rtk git add README.md
rtk git commit -m "docs: switch rollshot-app examples to clap flags"
```

---

## Self-Review

**Spec coverage:**
- Parser = clap → Task 1 Steps 1–2. ✓
- Remove `--capture` JSON entirely → Task 1 Step 2 (file rewrite drops all JSON/`initial_mode`/`extract_logging_args`). ✓
- All-subcommands structure, no-subcommand defaults to capture → `LaunchCommand` + `resolve_launch_mode(None)`. ✓
- `--workflow`/`--scope` two flags, workflow limited to screenshot/scrolling → `WorkflowArg`/`ScopeArg`. ✓
- `--log-file` global → `#[arg(long, global = true)]` + tests. ✓
- `is_supported()` rejection preserved → `resolve_launch_mode` + `scrolling_fullscreen_is_rejected` test. ✓
- Defaults match `default_capture()` → `no_subcommand_uses_defaults` test + CaptureArgs defaults. ✓
- Cargo.toml: add clap, remove serde_json → Task 1 Step 1. ✓
- main.rs restructured (parse → init diagnostics → dispatch) → Task 1 Step 3. ✓
- Tests rewritten to flag surface, JSON tests removed → Task 1 Step 2 (tests mod) + Step 3 (main tests). ✓
- README examples rewritten + binary-labeled, `initial_request` subsection replaced → Task 2. ✓
- rollshot-cli untouched → no task modifies it. ✓
- Verification commands → Task 1 Steps 4–6. ✓

**Placeholder scan:** No TBD/TODO; all code blocks are complete; commands have expected output.

**Type consistency:** `LaunchCli`, `LaunchCommand`, `CaptureArgs`, `WorkflowArg`, `ScopeArg`, `resolve_launch_mode`, `LaunchMode` names are consistent between Task 1 interfaces, the launch.rs body, and the main.rs edits. `resolve_launch_mode` takes `Option<LaunchCommand>` everywhere (launch.rs definition, main.rs call, test helper).
