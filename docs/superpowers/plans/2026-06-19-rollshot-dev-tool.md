# Rollshot Developer Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the product-like `rollshot-cli`/`rollshot` interface with an internal `rollshot-dev` tool that exposes only `probe` and `stitch-folder`.

**Architecture:** Rename the existing crate and binary in place, then delete the capture and GUI-launching command paths instead of relocating them. `rollshot-app` remains the only product executable; `rollshot-dev` retains only backend diagnostics through `rollshot-capture` and offline stitching through `rollshot-core`.

**Tech Stack:** Rust 2021, Cargo workspace, clap 4, `rollshot-capture`, `rollshot-core`, Rust integration tests, GitHub Actions.

---

## File Structure

- Rename `crates/rollshot-cli/` to `crates/rollshot-dev/`.
- Keep `src/args.rs` responsible only for the `probe` and `stitch-folder` clap surface.
- Keep `src/cmd_probe.rs` responsible for backend capability reporting without starting capture.
- Keep `src/cmd_stitch_folder.rs` responsible for offline frame loading, stitching, and matcher artifacts.
- Keep `src/cli_error.rs` responsible only for generic CLI rendering and exit codes.
- Delete capture execution, GUI launcher, Action Guide forwarding, and frame-slot files.
- Keep `tests/cli_smoke.rs` and `tests/probe_cli.rs` as binary-level coverage for the retained commands.
- Delete integration-test files whose only subject is the removed capture command.
- Update workspace metadata, CI, README, and AGENTS to make `rollshot-app` the product boundary and `rollshot-dev` an internal tool.

### Task 1: Rename the Crate and Binary Without Changing Behavior

**Files:**
- Rename: `crates/rollshot-cli/` → `crates/rollshot-dev/`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/rollshot-dev/Cargo.toml`
- Modify: `crates/rollshot-dev/src/main.rs`
- Modify: `crates/rollshot-dev/src/args.rs`
- Modify: `crates/rollshot-dev/src/cmd_probe.rs`
- Modify: `crates/rollshot-dev/tests/cli_smoke.rs`
- Modify: `crates/rollshot-dev/tests/probe_cli.rs`
- Modify: `crates/rollshot-dev/tests/capture_launcher.rs`
- Modify: `crates/rollshot-dev/tests/capture_fixture.rs`
- Modify: `crates/rollshot-dev/tests/capture_stubs.rs`
- Modify: `crates/rollshot-dev/tests/common/mod.rs`

- [ ] **Step 1: Verify the new package does not exist yet**

Run:

```bash
rtk cargo run -p rollshot-dev -- --help
```

Expected: FAIL with `package(s) 'rollshot-dev' not found in workspace`.

- [ ] **Step 2: Rename the crate directory**

Run:

```bash
rtk git mv crates/rollshot-cli crates/rollshot-dev
```

- [ ] **Step 3: Rename the workspace member, package, and binary**

Change the workspace member in `Cargo.toml`:

```toml
"crates/rollshot-dev",
```

Change the package and binary declarations in `crates/rollshot-dev/Cargo.toml`:

```toml
[package]
name = "rollshot-dev"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "rollshot-dev"
path = "src/main.rs"
```

Keep the existing dependencies and `action-guide` feature for this intermediate
commit; Task 2 removes them with the commands they support.

- [ ] **Step 4: Update the Rust crate name and displayed command name**

Replace `crates/rollshot-dev/src/main.rs` with:

```rust
use std::process::ExitCode;

use rollshot_dev::cli_error::Stream;

fn main() -> ExitCode {
    match rollshot_dev::run(std::env::args_os()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            match error.stream {
                Stream::Stdout => print!("{}", error.message),
                Stream::Stderr => eprintln!("{}", error.message),
            }
            ExitCode::from(error.exit_code)
        }
    }
}
```

Change the clap metadata in `crates/rollshot-dev/src/args.rs`:

```rust
#[derive(Debug, Parser)]
#[command(
    name = "rollshot-dev",
    version,
    about = "Rollshot developer diagnostics and offline stitching"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
```

Change the text report heading in `crates/rollshot-dev/src/cmd_probe.rs`:

```rust
let _ = writeln!(out, "rollshot-dev probe");
```

- [ ] **Step 5: Point all existing integration tests at the renamed binary**

In every file under `crates/rollshot-dev/tests/`, replace:

```rust
env!("CARGO_BIN_EXE_rollshot")
```

with:

```rust
env!("CARGO_BIN_EXE_rollshot-dev")
```

Update command expectation strings such as:

```rust
.expect("run rollshot-dev probe")
.expect("run rollshot-dev stitch-folder")
.expect("run rollshot-dev capture")
```

Rename retained test functions from the `rollshot_*` prefix to
`rollshot_dev_*`, for example:

```rust
fn rollshot_dev_probe_binary_runs()
fn rollshot_dev_stitch_folder_writes_png()
```

Change the temporary-directory prefix in
`crates/rollshot-dev/tests/common/mod.rs`:

```rust
let path = std::env::temp_dir().join(format!(
    "rollshot-dev-{label}-{nanos}-{}",
    std::process::id()
));
```

- [ ] **Step 6: Regenerate the lockfile and verify the rename**

Run:

```bash
rtk cargo check -p rollshot-dev --all-targets
rtk cargo test -p rollshot-dev
rtk cargo test -p rollshot-dev --features action-guide
rtk cargo run -p rollshot-dev -- --help
```

Expected:

- Cargo updates `Cargo.lock` from `rollshot-cli` to `rollshot-dev`.
- Check and tests PASS with the existing capture/probe/stitch behavior and the
  feature-gated Action Guide path.
- Help begins with `Rollshot developer diagnostics and offline stitching`.

- [ ] **Step 7: Commit the mechanical rename**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-dev
rtk git commit -m "build!: rename CLI crate to rollshot-dev"
```

### Task 2: Remove Capture and Action Guide Commands

**Files:**
- Modify: `crates/rollshot-dev/Cargo.toml`
- Modify: `crates/rollshot-dev/src/args.rs`
- Modify: `crates/rollshot-dev/src/lib.rs`
- Modify: `crates/rollshot-dev/src/cli_error.rs`
- Delete: `crates/rollshot-dev/src/cmd_capture.rs`
- Delete: `crates/rollshot-dev/src/cmd_capture_launcher.rs`
- Delete: `crates/rollshot-dev/src/cmd_action_guide.rs`
- Delete: `crates/rollshot-dev/src/frame_slot.rs`
- Modify: `crates/rollshot-dev/tests/cli_smoke.rs`
- Delete: `crates/rollshot-dev/tests/capture_launcher.rs`
- Delete: `crates/rollshot-dev/tests/capture_fixture.rs`
- Delete: `crates/rollshot-dev/tests/capture_stubs.rs`
- Delete: `crates/rollshot-dev/tests/common/mod.rs`

- [ ] **Step 1: Add a failing test for the final command boundary**

Add to `crates/rollshot-dev/tests/cli_smoke.rs`:

```rust
#[test]
fn help_lists_only_developer_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-dev"))
        .arg("--help")
        .output()
        .expect("run rollshot-dev --help");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("probe"), "stdout = {stdout}");
    assert!(stdout.contains("stitch-folder"), "stdout = {stdout}");
    assert!(!stdout.contains("capture"), "stdout = {stdout}");
    assert!(!stdout.contains("action-guide"), "stdout = {stdout}");
}
```

- [ ] **Step 2: Run the boundary test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-dev --test cli_smoke help_lists_only_developer_commands -- --exact
```

Expected: FAIL because help still contains `capture` and, with the feature,
the source still supports `action-guide`.

- [ ] **Step 3: Reduce the clap surface to the two retained commands**

Replace `crates/rollshot-dev/src/args.rs` with:

```rust
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rollshot-dev",
    version,
    about = "Rollshot developer diagnostics and offline stitching"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print diagnostics about the host and known capture backends.
    Probe(ProbeArgs),

    /// Stitch a directory of pre-recorded frames without starting capture.
    StitchFolder(StitchFolderArgs),
}

#[derive(Debug, clap::Args)]
pub struct ProbeArgs {
    /// Emit JSON instead of human-readable text.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct StitchFolderArgs {
    /// Directory of frames to stitch.
    pub frames_dir: PathBuf,

    /// Output PNG path.
    #[arg(long, short)]
    pub output: PathBuf,

    /// Write a JSON report with one match outcome per input frame.
    #[arg(long)]
    pub debug_match_report: Option<PathBuf>,

    /// Write overlap and diff images for frames with estimates.
    #[arg(long)]
    pub dump_overlap_debug: Option<PathBuf>,

    /// Disable the FAST + linear-KNN feature fallback.
    #[arg(long, default_value_t = false)]
    pub disable_feature_fallback: bool,
}
```

- [ ] **Step 4: Reduce command dispatch to the retained modules**

Replace `crates/rollshot-dev/src/lib.rs` with:

```rust
pub mod args;
pub mod cli_error;
pub mod cmd_probe;
pub mod cmd_stitch_folder;

use clap::Parser;

pub use cli_error::CliError;

use crate::args::{Cli, Command};

pub fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|err| {
        use clap::error::ErrorKind;
        let msg = err.to_string();
        match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => CliError::stdout(msg, 0),
            _ => CliError::new(msg, 1),
        }
    })?;

    match &cli.command {
        Command::Probe(args) => cmd_probe::run(args),
        Command::StitchFolder(args) => cmd_stitch_folder::run(args),
    }
}
```

- [ ] **Step 5: Remove capture-specific error conversion**

Replace `crates/rollshot-dev/src/cli_error.rs` with:

```rust
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub exit_code: u8,
    pub stream: Stream,
}

impl CliError {
    pub fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
            stream: Stream::Stderr,
        }
    }

    pub fn stdout(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
            stream: Stream::Stdout,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
```

- [ ] **Step 6: Delete the removed implementation and test files**

Delete:

```text
crates/rollshot-dev/src/cmd_capture.rs
crates/rollshot-dev/src/cmd_capture_launcher.rs
crates/rollshot-dev/src/cmd_action_guide.rs
crates/rollshot-dev/src/frame_slot.rs
crates/rollshot-dev/tests/capture_launcher.rs
crates/rollshot-dev/tests/capture_fixture.rs
crates/rollshot-dev/tests/capture_stubs.rs
crates/rollshot-dev/tests/common/mod.rs
```

Delete only the `rollshot_capture_fixture_writes_debug_report` test from
`crates/rollshot-dev/tests/cli_smoke.rs`. Keep all `probe` and `stitch-folder`
tests and their image-fixture helpers.

- [ ] **Step 7: Remove dependencies and features orphaned by the deletion**

Make `crates/rollshot-dev/Cargo.toml`:

```toml
[package]
name = "rollshot-dev"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "rollshot-dev"
path = "src/main.rs"

[dependencies]
clap = { workspace = true }
image = { workspace = true }
rollshot-capture = { path = "../rollshot-capture" }
rollshot-core = { path = "../rollshot-core" }
serde = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
image = { workspace = true }

[lints]
workspace = true
```

This removes `anyhow` and the `action-guide` feature. Keep
`rollshot-capture` because `probe` constructs platform backend probes.

- [ ] **Step 8: Verify the command boundary and retained behavior**

Run:

```bash
rtk cargo test -p rollshot-dev --test cli_smoke help_lists_only_developer_commands -- --exact
rtk cargo test -p rollshot-dev
rtk cargo run -p rollshot-dev -- --help
rtk cargo run -p rollshot-dev -- capture
rtk cargo run -p rollshot-dev -- action-guide
```

Expected:

- Boundary test and full crate tests PASS.
- Help lists only `probe`, `stitch-folder`, and clap's `help`.
- Both removed commands fail with `unrecognized subcommand`.

- [ ] **Step 9: Commit the command removal**

```bash
rtk git add crates/rollshot-dev Cargo.lock
rtk git commit -m "refactor!: limit rollshot-dev to offline tools"
```

### Task 3: Update CI and Workspace Checks

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Confirm CI still references removed package features**

Run:

```bash
rtk rg -n "rollshot-cli|rollshot-dev|action-guide" .github/workflows/ci.yml
```

Expected: output includes `rollshot-cli/action-guide` and
`cargo check -p rollshot-cli`.

- [ ] **Step 2: Update feature and package checks**

Use these action-guide commands:

```yaml
- name: Clippy (action-guide feature)
  run: cargo clippy --workspace --all-targets --features rollshot-app/action-guide -- -D warnings

- name: Test (action-guide feature)
  run: cargo test --workspace --features rollshot-app/action-guide
```

Use this developer-tool check in the macOS target block:

```yaml
cargo check -p rollshot-dev --all-targets
```

Leave all other CI commands unchanged.

- [ ] **Step 3: Run the exact updated CI feature checks locally**

Run:

```bash
rtk cargo check -p rollshot-dev --all-targets
rtk cargo clippy --workspace --all-targets --features rollshot-app/action-guide -- -D warnings
rtk cargo test --workspace --features rollshot-app/action-guide
```

Expected: all commands PASS without an unknown package or feature error.

- [ ] **Step 4: Commit the CI update**

```bash
rtk git add .github/workflows/ci.yml
rtk git commit -m "ci: check rollshot-dev and app features"
```

### Task 4: Update README and Agent Instructions

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`

- [ ] **Step 1: Update README product and workspace descriptions**

Replace the headless CLI capability bullet with:

```markdown
- **Developer diagnostics and offline stitching**: the internal `rollshot-dev`
  tool inspects backend availability with `probe` and stitches pre-recorded
  frame folders for matcher development. Product capture remains in
  `rollshot-app`.
```

Replace the workspace entry with:

```markdown
- `crates/rollshot-dev`: internal developer diagnostics and offline
  `stitch-folder` tooling
```

- [ ] **Step 2: Update README development commands**

Use:

````markdown
Useful developer-tool commands:

```bash
cargo run -p rollshot-dev -- probe
mkdir -p target/test-artifacts
cargo run -p rollshot-dev -- stitch-folder \
  crates/rollshot-core/tests/fixtures/linearscroll_v2/linear_vertical_down/frames \
  --output target/test-artifacts/stitch-folder.png
```

`rollshot-dev` is not a product entry point and does not start capture.
`stitch-folder` works only with existing image frames.
````

Remove the paragraph describing `rollshot capture` progress and `--quiet`.

- [ ] **Step 3: Update README CI and bootstrap commands**

Use:

```bash
cargo clippy --workspace --all-targets --features rollshot-app/action-guide -- -D warnings
cargo test --workspace --features rollshot-app/action-guide
```

Update bootstrap checklist commands from `rollshot-cli` to `rollshot-dev`.

- [ ] **Step 4: Replace removed Linux and macOS CLI capture checks**

Keep `rollshot-dev probe --json` in each platform checklist. Remove all
`rollshot-dev capture`, `--headless`, `--max-frames`, `--dump-frames`, and
manual-region command examples.

Use product-app smoke commands instead:

```bash
# Linux
cargo run -p rollshot-app -- capture --backend linux-portal \
  --workflow scrolling --scope region

# macOS
cargo run -p rollshot-app -- capture --backend macos-sck \
  --workflow scrolling --scope region
```

Retain the ignored `rollshot-capture` real-capture smoke tests as backend-level
verification.

- [ ] **Step 5: Make Action Guide documentation app-only**

Use:

````markdown
Action Guide is gated behind the non-default `action-guide` Cargo feature on
`rollshot-app`:

```bash
cargo build --release -p rollshot-app --features action-guide
cargo run -p rollshot-app --features action-guide -- action-guide
```
````

Change fullscreen recording usage to:

```text
rollshot-app action-guide --fullscreen
```

- [ ] **Step 6: Update the AGENTS project map and feature ownership**

Replace the CLI project-map entry with:

```markdown
- `crates/rollshot-dev`: internal developer tooling. `src/args.rs` is the
  source of truth for its two subcommands: `probe` and `stitch-folder`. It does
  not start capture or launch `rollshot-app`.
```

Replace the Action Guide heading with:

```markdown
- **Action Guide crates** (built behind the non-default `action-guide` Cargo
  feature on `rollshot-app`):
```

Do not modify historical files under `docs/superpowers/` or other snapshot and
research documents under `docs/`.

- [ ] **Step 7: Verify active documentation has the new boundary**

Run:

```bash
rtk rg -n "rollshot-cli|rollshot capture|rollshot action-guide|--headless|ROLLSHOT_APP" README.md AGENTS.md
rtk rg -n "rollshot-dev|rollshot-app action-guide" README.md AGENTS.md
```

Expected:

- First command returns no matches.
- Second command shows the developer-tool and app-only documentation.

- [ ] **Step 8: Commit documentation**

```bash
rtk git add README.md AGENTS.md
rtk git commit -m "docs: document app and developer tool boundary"
```

### Task 5: Final Repository Verification

**Files:**
- Verify: `Cargo.toml`
- Verify: `Cargo.lock`
- Verify: `.github/workflows/ci.yml`
- Verify: `crates/rollshot-dev/`
- Verify: `README.md`
- Verify: `AGENTS.md`

- [ ] **Step 1: Scan active implementation and automation for stale names**

Run:

```bash
rtk rg -n 'rollshot-cli|CARGO_BIN_EXE_rollshot"|ROLLSHOT_APP|--headless|rollshot capture|rollshot action-guide' \
  Cargo.toml Cargo.lock .github crates README.md AGENTS.md
```

Expected: no matches. `docs/` is intentionally excluded because its issue,
research, plan, and spec files are snapshots rather than active source of
truth.

- [ ] **Step 2: Verify only the two intended command variants remain**

Run:

```bash
rtk cargo run -p rollshot-dev -- --help
```

Expected command section:

```text
Commands:
  probe
  stitch-folder
  help
```

- [ ] **Step 3: Run formatting and focused tests**

Run:

```bash
rtk cargo fmt --all -- --check
rtk cargo test -p rollshot-dev
```

Expected: PASS.

- [ ] **Step 4: Run full workspace verification**

Run:

```bash
rtk cargo clippy --workspace --all-targets --features rollshot-app/action-guide -- -D warnings
rtk cargo test --workspace --features rollshot-app/action-guide
```

Expected: PASS on the current platform.

- [ ] **Step 5: Inspect the final change set**

Run:

```bash
rtk git diff --check origin/main...HEAD
rtk git status --short --branch
rtk git log --oneline --decorate origin/main..HEAD
```

Expected:

- No whitespace errors.
- Clean working tree.
- Commits show the approved spec, crate rename, command removal, CI update,
  and documentation update.
