# Rollshot Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the initial Rust workspace, CI workflows, baseline tests, and README manual-testing documentation for rollshot.

**Architecture:** The workspace is split into four crates: `rollshot-core` for platform-independent stitching concepts, `rollshot-capture` for capture traits and data types, `rollshot-cli` for command entry points, and `rollshot-app` for a compiling future UI binary. Phase 0 intentionally avoids real screen capture and external Rust dependencies so the bootstrap can build reliably before backend work starts.

**Tech Stack:** Rust 2021 edition, Cargo workspace resolver v2, standard library CLI parsing, GitHub Actions, `cargo fmt`, `cargo clippy`, `cargo test`.

---

## File Structure

- Create: `Cargo.toml`  
  Root workspace metadata and crate membership.
- Create: `.gitignore`  
  Rust and editor ignores.
- Create: `crates/rollshot-core/Cargo.toml`  
  Core crate manifest.
- Create: `crates/rollshot-core/src/lib.rs`  
  Minimal core API and tests.
- Create: `crates/rollshot-capture/Cargo.toml`  
  Capture crate manifest.
- Create: `crates/rollshot-capture/src/lib.rs`  
  Capture API exports.
- Create: `crates/rollshot-capture/src/types.rs`  
  Shared capture data types.
- Create: `crates/rollshot-capture/src/backend.rs`  
  Capture backend and frame stream traits plus fake stream tests.
- Create: `crates/rollshot-cli/Cargo.toml`  
  CLI crate manifest.
- Create: `crates/rollshot-cli/src/main.rs`  
  Binary entry point.
- Create: `crates/rollshot-cli/src/lib.rs`  
  Testable CLI command runner.
- Create: `crates/rollshot-cli/tests/cli_smoke.rs`  
  Binary smoke tests.
- Create: `crates/rollshot-app/Cargo.toml`  
  App crate manifest.
- Create: `crates/rollshot-app/src/main.rs`  
  Minimal future app binary.
- Create: `.github/workflows/ci.yml`  
  PR and main-branch CI.
- Create: `.github/workflows/real-capture.yml`  
  Manual self-hosted smoke workflow skeleton.
- Create: `README.md`  
  Project overview, local verification, CI behavior, and manual test checklists.

## Task 1: Workspace Metadata

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`

- [ ] **Step 1: Write the root workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
members = []
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/noah/rollshot"
rust-version = "1.80"

[workspace.lints.rust]
unsafe_code = "forbid"
```

- [ ] **Step 2: Write the repository ignore file**

Create `.gitignore`:

```gitignore
/target/
**/*.rs.bk
.DS_Store
.idea/
.vscode/
```

- [ ] **Step 3: Verify Cargo sees the empty workspace**

Run: `cargo metadata --no-deps --format-version 1`

Expected: PASS. This confirms the root virtual workspace is syntactically valid before crates are added.

- [ ] **Step 4: Commit workspace metadata**

```bash
git add Cargo.toml .gitignore
git commit -m "chore: add workspace metadata"
```

## Task 2: Core Crate Skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/rollshot-core/Cargo.toml`
- Create: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Add the core crate to the workspace**

Update `Cargo.toml`:

```toml
[workspace]
members = [
    "crates/rollshot-core",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/noah/rollshot"
rust-version = "1.80"

[workspace.lints.rust]
unsafe_code = "forbid"
```

- [ ] **Step 2: Write the core crate manifest**

Create `crates/rollshot-core/Cargo.toml`:

```toml
[package]
name = "rollshot-core"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Write the failing core API test first**

Create `crates/rollshot-core/src/lib.rs` with this test and public API shell:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAlgorithm {
    Template,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StitchConfig {
    pub algorithm: MatchAlgorithm,
    pub min_overlap: u32,
    pub min_append: u32,
    pub accept_diff: f32,
    pub match_width: u32,
    pub duplicate_threshold: f32,
}

#[cfg(test)]
mod tests {
    use super::{MatchAlgorithm, StitchConfig};

    #[test]
    fn default_config_uses_template_matching() {
        let config = StitchConfig::default();

        assert_eq!(config.algorithm, MatchAlgorithm::Template);
        assert_eq!(config.min_overlap, 64);
        assert_eq!(config.min_append, 8);
        assert_eq!(config.match_width, 512);
    }
}
```

- [ ] **Step 4: Run the core test to verify it fails**

Run: `cargo test -p rollshot-core default_config_uses_template_matching`

Expected: FAIL with an error that `StitchConfig::default` is not defined.

- [ ] **Step 5: Implement the minimal core default**

Update `crates/rollshot-core/src/lib.rs` to:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAlgorithm {
    Template,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StitchConfig {
    pub algorithm: MatchAlgorithm,
    pub min_overlap: u32,
    pub min_append: u32,
    pub accept_diff: f32,
    pub match_width: u32,
    pub duplicate_threshold: f32,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            algorithm: MatchAlgorithm::Template,
            min_overlap: 64,
            min_append: 8,
            accept_diff: 0.15,
            match_width: 512,
            duplicate_threshold: 0.01,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MatchAlgorithm, StitchConfig};

    #[test]
    fn default_config_uses_template_matching() {
        let config = StitchConfig::default();

        assert_eq!(config.algorithm, MatchAlgorithm::Template);
        assert_eq!(config.min_overlap, 64);
        assert_eq!(config.min_append, 8);
        assert_eq!(config.match_width, 512);
    }
}
```

- [ ] **Step 6: Run the core test to verify it passes**

Run: `cargo test -p rollshot-core default_config_uses_template_matching`

Expected: PASS.

- [ ] **Step 7: Commit the core crate**

```bash
git add Cargo.toml crates/rollshot-core
git commit -m "feat: add core crate skeleton"
```

## Task 3: Capture Crate API

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/rollshot-capture/Cargo.toml`
- Create: `crates/rollshot-capture/src/lib.rs`
- Create: `crates/rollshot-capture/src/types.rs`
- Create: `crates/rollshot-capture/src/backend.rs`

- [ ] **Step 1: Add the capture crate to the workspace**

Update `Cargo.toml`:

```toml
[workspace]
members = [
    "crates/rollshot-core",
    "crates/rollshot-capture",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/noah/rollshot"
rust-version = "1.80"

[workspace.lints.rust]
unsafe_code = "forbid"
```

- [ ] **Step 2: Write the capture crate manifest**

Create `crates/rollshot-capture/Cargo.toml`:

```toml
[package]
name = "rollshot-capture"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Write the capture exports**

Create `crates/rollshot-capture/src/lib.rs`:

```rust
pub mod backend;
pub mod types;

pub use backend::{CaptureBackend, FrameStream};
pub use types::{
    CapturedFrame, CaptureOptions, CaptureProbe, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 4: Write the capture data types**

Create `crates/rollshot-capture/src/types.rs`:

```rust
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub pixels: Vec<u8>,
    pub size: Size,
    pub timestamp: SystemTime,
    pub metadata: FrameMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOptions {
    pub region: RegionMode,
    pub fps: u32,
    pub show_cursor: bool,
    pub prefer_portal_region: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            region: RegionMode::FullSource,
            fps: 5,
            show_cursor: false,
            prefer_portal_region: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureProbe {
    pub backend: &'static str,
    pub available: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameMetadata {
    pub source_size: Option<Size>,
    pub effective_region: Option<Region>,
    pub pixel_format: Option<PixelFormat>,
    pub stride: Option<u32>,
    pub backend: &'static str,
}

impl FrameMetadata {
    pub fn fake() -> Self {
        Self {
            source_size: None,
            effective_region: None,
            pixel_format: Some(PixelFormat::Rgba),
            stride: None,
            backend: "fake",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionMode {
    Manual(Region),
    PortalPicker,
    FullSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba,
    Bgra,
    Bgrx,
    Rgbx,
    Rgb,
}
```

- [ ] **Step 5: Write the failing fake stream test first**

Create `crates/rollshot-capture/src/backend.rs`:

```rust
use crate::types::{CapturedFrame, CaptureOptions, CaptureProbe};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, String>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, String>;
}

#[cfg(test)]
mod tests {
    use super::FrameStream;
    use crate::types::{CapturedFrame, FrameMetadata, Size};
    use std::time::SystemTime;

    #[test]
    fn fake_stream_returns_frames_in_order() {
        let first = CapturedFrame {
            pixels: vec![255, 0, 0, 255],
            size: Size {
                width: 1,
                height: 1,
            },
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        };
        let second = CapturedFrame {
            pixels: vec![0, 255, 0, 255],
            size: Size {
                width: 1,
                height: 1,
            },
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        };
        let mut stream = FakeFrameStream::new(vec![first.clone(), second.clone()]);

        assert_eq!(stream.next_frame().expect("first frame"), first);
        assert_eq!(stream.next_frame().expect("second frame"), second);
        assert_eq!(stream.next_frame().expect_err("end of stream"), "end of fake stream");
    }
}
```

- [ ] **Step 6: Run the fake stream test to verify it fails**

Run: `cargo test -p rollshot-capture fake_stream_returns_frames_in_order`

Expected: FAIL with an error that `FakeFrameStream` is not defined.

- [ ] **Step 7: Implement the fake frame stream**

Update `crates/rollshot-capture/src/backend.rs` to:

```rust
use crate::types::{CapturedFrame, CaptureOptions, CaptureProbe};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, String>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, String>;
}

#[derive(Debug, Clone)]
pub struct FakeFrameStream {
    frames: Vec<CapturedFrame>,
    index: usize,
}

impl FakeFrameStream {
    pub fn new(frames: Vec<CapturedFrame>) -> Self {
        Self { frames, index: 0 }
    }
}

impl FrameStream for FakeFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, String> {
        let frame = self
            .frames
            .get(self.index)
            .cloned()
            .ok_or_else(|| String::from("end of fake stream"))?;

        self.index += 1;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeFrameStream, FrameStream};
    use crate::types::{CapturedFrame, FrameMetadata, Size};
    use std::time::SystemTime;

    #[test]
    fn fake_stream_returns_frames_in_order() {
        let first = CapturedFrame {
            pixels: vec![255, 0, 0, 255],
            size: Size {
                width: 1,
                height: 1,
            },
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        };
        let second = CapturedFrame {
            pixels: vec![0, 255, 0, 255],
            size: Size {
                width: 1,
                height: 1,
            },
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        };
        let mut stream = FakeFrameStream::new(vec![first.clone(), second.clone()]);

        assert_eq!(stream.next_frame().expect("first frame"), first);
        assert_eq!(stream.next_frame().expect("second frame"), second);
        assert_eq!(stream.next_frame().expect_err("end of stream"), "end of fake stream");
    }
}
```

- [ ] **Step 8: Export the fake stream from the crate**

Update `crates/rollshot-capture/src/lib.rs`:

```rust
pub mod backend;
pub mod types;

pub use backend::{CaptureBackend, FakeFrameStream, FrameStream};
pub use types::{
    CapturedFrame, CaptureOptions, CaptureProbe, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 9: Run capture tests**

Run: `cargo test -p rollshot-capture`

Expected: PASS.

- [ ] **Step 10: Commit the capture crate**

```bash
git add Cargo.toml crates/rollshot-capture
git commit -m "feat: add capture crate API"
```

## Task 4: CLI Crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/rollshot-cli/Cargo.toml`
- Create: `crates/rollshot-cli/src/main.rs`
- Create: `crates/rollshot-cli/src/lib.rs`
- Create: `crates/rollshot-cli/tests/cli_smoke.rs`

- [ ] **Step 1: Add the CLI crate to the workspace**

Update `Cargo.toml`:

```toml
[workspace]
members = [
    "crates/rollshot-core",
    "crates/rollshot-capture",
    "crates/rollshot-cli",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/noah/rollshot"
rust-version = "1.80"

[workspace.lints.rust]
unsafe_code = "forbid"
```

- [ ] **Step 2: Write the CLI crate manifest**

Create `crates/rollshot-cli/Cargo.toml`:

```toml
[package]
name = "rollshot-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "rollshot"
path = "src/main.rs"

[dependencies]
rollshot-capture = { path = "../rollshot-capture" }
rollshot-core = { path = "../rollshot-core" }

[lints]
workspace = true
```

- [ ] **Step 3: Write failing command tests first**

Create `crates/rollshot-cli/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn probe_reports_bootstrap_status() {
        let output = run(["rollshot", "probe"]).expect("probe should succeed");

        assert!(output.contains("rollshot"));
        assert!(output.contains("real capture: unavailable"));
    }

    #[test]
    fn stitch_folder_reports_deferred_status() {
        let output = run(["rollshot", "stitch-folder", "tests/fixtures"]).expect("command runs");

        assert!(output.contains("stitch-folder"));
        assert!(output.contains("not available in bootstrap phase"));
    }
}
```

- [ ] **Step 4: Run CLI library tests to verify they fail**

Run: `cargo test -p rollshot-cli`

Expected: FAIL with an error that `run` is not defined.

- [ ] **Step 5: Implement the CLI command runner**

Update `crates/rollshot-cli/src/lib.rs`:

```rust
pub fn run<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        None | Some("--help" | "-h") => Ok(help()),
        Some("--version" | "-V") => Ok(format!("rollshot {}\n", env!("CARGO_PKG_VERSION"))),
        Some("probe") => Ok(probe()),
        Some("stitch-folder") => stitch_folder(&args[2..]),
        Some(command) => Err(format!("unknown command: {command}\n\n{}", help())),
    }
}

fn help() -> String {
    String::from(
        "rollshot\n\
         \n\
         Usage:\n\
           rollshot probe\n\
           rollshot stitch-folder <frames-dir>\n\
           rollshot --version\n",
    )
}

fn probe() -> String {
    format!(
        "rollshot {}\n\
         os: {}\n\
         real capture: unavailable in bootstrap phase\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
    )
}

fn stitch_folder(args: &[String]) -> Result<String, String> {
    let frames_dir = args
        .first()
        .ok_or_else(|| String::from("usage: rollshot stitch-folder <frames-dir>"))?;

    Ok(format!(
        "stitch-folder: {frames_dir}\n\
         status: not available in bootstrap phase\n",
    ))
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn probe_reports_bootstrap_status() {
        let output = run(["rollshot", "probe"]).expect("probe should succeed");

        assert!(output.contains("rollshot"));
        assert!(output.contains("real capture: unavailable"));
    }

    #[test]
    fn stitch_folder_reports_deferred_status() {
        let output = run(["rollshot", "stitch-folder", "tests/fixtures"]).expect("command runs");

        assert!(output.contains("stitch-folder"));
        assert!(output.contains("not available in bootstrap phase"));
    }
}
```

- [ ] **Step 6: Write the binary entry point**

Create `crates/rollshot-cli/src/main.rs`:

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
    match rollshot_cli::run(std::env::args()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 7: Write binary smoke tests**

Create `crates/rollshot-cli/tests/cli_smoke.rs`:

```rust
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
```

- [ ] **Step 8: Run CLI tests**

Run: `cargo test -p rollshot-cli`

Expected: PASS.

- [ ] **Step 9: Commit the CLI crate**

```bash
git add Cargo.toml crates/rollshot-cli
git commit -m "feat: add bootstrap CLI"
```

## Task 5: App Crate Skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/rollshot-app/Cargo.toml`
- Create: `crates/rollshot-app/src/main.rs`

- [ ] **Step 1: Add the app crate to the workspace**

Update `Cargo.toml`:

```toml
[workspace]
members = [
    "crates/rollshot-core",
    "crates/rollshot-capture",
    "crates/rollshot-cli",
    "crates/rollshot-app",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/noah/rollshot"
rust-version = "1.80"

[workspace.lints.rust]
unsafe_code = "forbid"
```

- [ ] **Step 2: Write the app crate manifest**

Create `crates/rollshot-app/Cargo.toml`:

```toml
[package]
name = "rollshot-app"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Write the minimal app binary**

Create `crates/rollshot-app/src/main.rs`:

```rust
fn main() {
    println!("rollshot app is not available in bootstrap phase");
}
```

- [ ] **Step 4: Verify the app crate compiles**

Run: `cargo check -p rollshot-app`

Expected: PASS.

- [ ] **Step 5: Commit the app crate**

```bash
git add Cargo.toml crates/rollshot-app
git commit -m "feat: add app crate skeleton"
```

## Task 6: CI Workflows

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/real-capture.yml`

- [ ] **Step 1: Write the hosted CI workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    name: Test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, macos-14]

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Format
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings

      - name: Test
        run: cargo test --workspace
```

- [ ] **Step 2: Write the manual real-capture workflow skeleton**

Create `.github/workflows/real-capture.yml`:

```yaml
name: Real Capture Smoke

on:
  workflow_dispatch:

jobs:
  linux-kde-wayland:
    name: Linux KDE Wayland smoke
    runs-on: [self-hosted, linux, kde6, wayland]
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Explain current bootstrap status
        run: |
          echo "Real Linux capture tests are added in the Linux backend phase."
          echo "This workflow reserves the self-hosted KDE Wayland runner path."

  macos-screencapturekit:
    name: macOS ScreenCaptureKit smoke
    runs-on: [self-hosted, macos, screencapturekit]
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Explain current bootstrap status
        run: |
          echo "Real macOS capture tests are added in the macOS backend phase."
          echo "This workflow reserves the self-hosted ScreenCaptureKit runner path."
```

- [ ] **Step 3: Run local workflow-equivalent commands**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all commands PASS.

- [ ] **Step 4: Commit CI workflows**

```bash
git add .github/workflows/ci.yml .github/workflows/real-capture.yml
git commit -m "ci: add github actions workflows"
```

## Task 7: README Manual Testing Documentation

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write the README**

Create `README.md`:

```markdown
# rollshot

`rollshot` is a Rust rewrite of the long screenshot workflow described in
`rollshot_mvp_design.md`. The project is in bootstrap phase: the workspace,
crate boundaries, CI, and tests exist, while real KDE Wayland and macOS capture
backends are not available yet.

## Workspace

- `crates/rollshot-core`: platform-independent stitching concepts
- `crates/rollshot-capture`: capture traits and frame metadata
- `crates/rollshot-cli`: command-line interface
- `crates/rollshot-app`: future app entry point

## Local Development

Install a stable Rust toolchain with `rustup`, then run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Useful smoke commands:

```bash
cargo run -p rollshot-cli -- probe
cargo run -p rollshot-cli -- stitch-folder tests/fixtures
```

`stitch-folder` is intentionally a bootstrap smoke command until the stitching
core phase adds image fixtures and golden output tests.

## GitHub Actions

`.github/workflows/ci.yml` runs on `ubuntu-24.04` and `macos-14` for pushes to
`main` and pull requests.

It runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Hosted PR CI does not run real desktop capture. KDE Wayland capture needs a real
interactive desktop session, xdg-desktop-portal-kde, PipeWire, and user
selection. macOS ScreenCaptureKit needs Screen Recording permission. Those
conditions belong on manual or self-hosted smoke runners.

## Manual Testing: Bootstrap

Use this checklist after changing workspace, CI, or crate wiring:

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo run -p rollshot-cli -- probe` prints the version, OS, and real capture status.
- [ ] `cargo run -p rollshot-cli -- stitch-folder tests/fixtures` exits successfully with bootstrap status text.

## Manual Testing: Future Linux KDE Wayland Capture

Use this checklist when the Linux backend phase adds real tests:

- [ ] Test machine is running KDE Plasma 6 on Wayland.
- [ ] `XDG_SESSION_TYPE=wayland`.
- [ ] `XDG_CURRENT_DESKTOP` mentions KDE or Plasma.
- [ ] PipeWire is running.
- [ ] WirePlumber is running.
- [ ] `xdg-desktop-portal` is running.
- [ ] `xdg-desktop-portal-kde` is running.
- [ ] `rollshot probe` reports portal and PipeWire availability.
- [ ] Portal source picker opens.
- [ ] Rectangular Region selection returns frames.
- [ ] At least three frames are captured.
- [ ] Captured frames have non-zero width and height.
- [ ] The first frame can be saved under `target/test-artifacts/`.

Expected future command:

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture
```

## Manual Testing: Future macOS ScreenCaptureKit Capture

Use this checklist when the macOS backend phase adds real tests:

- [ ] Test runner has Screen Recording permission.
- [ ] Main display is visible and unlocked.
- [ ] `rollshot probe` reports macOS capture status.
- [ ] A small manual region can be selected or configured.
- [ ] At least three frames are captured.
- [ ] Captured frames have non-zero width and height.
- [ ] BGRA to RGBA conversion is visually correct.
- [ ] The first frame can be saved under `target/test-artifacts/`.

Expected future command:

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

## Manual Self-Hosted Workflow

`.github/workflows/real-capture.yml` reserves the manual smoke-test path for
self-hosted runners:

- Linux runner labels: `self-hosted`, `linux`, `kde6`, `wayland`
- macOS runner labels: `self-hosted`, `macos`, `screencapturekit`

Run it from GitHub Actions with `workflow_dispatch`. In bootstrap phase the jobs
only explain that real backend smoke tests are added in later backend phases.
```

- [ ] **Step 2: Run markdown-adjacent verification**

Run: `cargo test --workspace`

Expected: PASS. This confirms README examples still point at valid workspace packages and commands compile.

- [ ] **Step 3: Commit README**

```bash
git add README.md
git commit -m "docs: add bootstrap testing guide"
```

## Task 8: Final Verification

**Files:**
- Create: `Cargo.lock`

- [ ] **Step 1: Run formatting**

Run: `cargo fmt --all -- --check`

Expected: PASS.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`

Expected: PASS.

- [ ] **Step 4: Run CLI probe manually**

Run: `cargo run -p rollshot-cli -- probe`

Expected output contains:

```text
rollshot 0.1.0
real capture: unavailable in bootstrap phase
```

- [ ] **Step 5: Run CLI stitch-folder smoke command**

Run: `cargo run -p rollshot-cli -- stitch-folder tests/fixtures`

Expected output contains:

```text
stitch-folder: tests/fixtures
status: not available in bootstrap phase
```

- [ ] **Step 6: Commit generated lockfile**

Commit the generated `Cargo.lock`:

```bash
git add Cargo.lock
git commit -m "chore: add cargo lockfile"
```

- [ ] **Step 7: Inspect final status**

Run: `git status --short`

Expected: only pre-existing untracked `rollshot_mvp_design.md` remains, unless the user asks to track it.
