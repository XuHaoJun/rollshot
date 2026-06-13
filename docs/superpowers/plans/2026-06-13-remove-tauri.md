# Remove Tauri Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the deprecated Tauri application, its active repository support, the unused Tauri launch API, and the `tauri-template` reference submodule in one coherent change.

**Architecture:** Keep the existing iced product path unchanged. First shrink the shared launch-options contract while preserving compatibility with old JSON payloads, then remove the deprecated application and its build graph, and finally rewrite active repository guidance and comments to describe only the current iced architecture. Historical docs and spikes remain untouched.

**Tech Stack:** Rust workspace/Cargo, serde JSON launch contract, Git submodules, GitHub Actions YAML, Markdown documentation

---

## File Map

- Delete: `crates/rollshot-tauri-app/` — deprecated Rust/Tauri host, React frontend, tests, and app-local tooling.
- Modify: `crates/rollshot-capture/src/types.rs` — remove `OverlayMode` and `InteractiveLaunchOptions::overlay_mode`; retain old-JSON compatibility coverage.
- Modify: `crates/rollshot-capture/src/lib.rs` — stop exporting `OverlayMode`.
- Modify: `crates/rollshot-cli/src/cmd_capture_launcher.rs` — construct the smaller launch-options contract and remove migration-era negative assertions.
- Modify: `crates/rollshot-cli/tests/capture_launcher.rs` — rename fake GUI executables to `fake-rollshot-app`.
- Modify: `crates/rollshot-app/src/launch.rs` — remove `OverlayMode` assertions and verify obsolete JSON fields are ignored.
- Modify: `Cargo.toml` — remove the deprecated workspace member.
- Modify: `Cargo.lock` — regenerate after workspace removal.
- Modify: `.github/workflows/ci.yml` — remove frontend/Tauri jobs and Tauri-only Linux packages.
- Modify: `.gitmodules` — remove the unused `tauri-template` submodule entry.
- Delete: `learn-projects/tauri-template` — unused reference gitlink.
- Modify: `README.md` — remove deprecated app setup, CI, workspace, and manual-check guidance.
- Modify: `AGENTS.md` — describe iced as the sole desktop path and remove Tauri/reference-project guidance.
- Modify: `crates/rollshot-overlay-core/src/lib.rs` — describe framework-neutral overlay logic without removed consumers.
- Modify: `crates/rollshot-overlay-core/src/tokens.rs` — describe iced as the visual-token consumer.
- Modify: `crates/rollshot-iced-overlay/src/lib.rs` — remove migration/coexistence wording.
- Modify: `crates/rollshot-iced-overlay/src/driver.rs` — document current stitch configuration without deleted-file references.
- Modify: `crates/rollshot-iced-overlay/src/bin/capture_overlay.rs` — describe the binary as a standalone harness.

### Task 1: Remove the obsolete launch option with JSON compatibility

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-cli/src/cmd_capture_launcher.rs`
- Modify: `crates/rollshot-app/src/launch.rs`

- [ ] **Step 1: Add tests that define the smaller launch contract**

In `crates/rollshot-capture/src/types.rs`, remove `OverlayMode` from the test import, build `InteractiveLaunchOptions` without `overlay_mode`, assert serialized JSON omits the obsolete field, and replace the old overlay-default test with an unknown-field compatibility test:

```rust
use super::{CaptureMode, CaptureOptions, InteractiveLaunchOptions};

#[test]
fn interactive_launch_options_round_trip_json() {
    let options = InteractiveLaunchOptions {
        backend: "linux-portal".to_string(),
        fps: 7,
        show_cursor: true,
        initial_mode: CaptureMode::Screenshot,
    };

    let json = serde_json::to_string(&options).expect("serialize launch options");
    assert!(json.contains("\"backend\":\"linux-portal\""), "json = {json}");
    assert!(
        json.contains("\"initial_mode\":\"screenshot\""),
        "json = {json}"
    );
    let obsolete_field = concat!("overlay", "_mode");
    assert!(!json.contains(obsolete_field), "json = {json}");

    let decoded: InteractiveLaunchOptions =
        serde_json::from_str(&json).expect("deserialize launch options");
    assert_eq!(decoded, options);
}

#[test]
fn interactive_launch_options_ignore_obsolete_field() {
    let obsolete_field = concat!("overlay", "_mode");
    let json = format!(
        r#"{{"backend":"auto","fps":5,"show_cursor":false,"{obsolete_field}":"legacy"}}"#
    );
    let decoded: InteractiveLaunchOptions =
        serde_json::from_str(&json).expect("deserialize payload with obsolete field");

    assert_eq!(decoded.initial_mode, CaptureMode::Scrolling);
}
```

In `crates/rollshot-app/src/launch.rs`, replace `parses_overlay_mode` with:

```rust
#[test]
fn ignores_obsolete_capture_option() {
    let obsolete_field = concat!("overlay", "_mode");
    let payload = format!(
        r#"{{"backend":"macos-sck","fps":30,"show_cursor":false,"{obsolete_field}":"legacy"}}"#
    );
    let mode = parse_launch_args([
        "rollshot-app",
        "--capture",
        payload.as_str(),
    ])
    .expect("parse launch args");

    match mode {
        LaunchMode::Capture(options) => {
            assert_eq!(options.backend, "macos-sck");
            assert_eq!(options.initial_mode, CaptureMode::Scrolling);
        }
    }
}
```

Remove `OverlayMode` from that test module's import and remove the `overlay_mode`
assertion from `no_args_uses_defaults`.

- [ ] **Step 2: Run the focused tests to verify the new contract fails**

Run:

```bash
rtk cargo test -p rollshot-capture interactive_launch_options_round_trip_json
```

Expected: FAIL because serialized JSON still contains `overlay_mode`.

- [ ] **Step 3: Remove `OverlayMode` and update active constructors**

In `crates/rollshot-capture/src/types.rs`, delete the `OverlayMode` enum and
remove `overlay_mode` from `InteractiveLaunchOptions` and `default_capture()`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InteractiveLaunchOptions {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
    #[serde(default)]
    pub initial_mode: CaptureMode,
}
```

In `crates/rollshot-capture/src/lib.rs`, remove `OverlayMode` from the `pub use
types::{...}` list.

In `crates/rollshot-cli/src/cmd_capture_launcher.rs`, remove the `overlay_mode`
field from `launch_options()`:

```rust
InteractiveLaunchOptions {
    backend: args.backend.clone(),
    fps: args.fps,
    show_cursor: args.show_cursor,
    initial_mode: rollshot_capture::CaptureMode::Scrolling,
}
```

- [ ] **Step 4: Run focused launch-contract tests**

Run:

```bash
rtk cargo test -p rollshot-capture interactive_launch_options
rtk cargo test -p rollshot-app launch::tests
rtk cargo test -p rollshot-cli cmd_capture_launcher::tests
```

Expected: all tests PASS; payloads containing obsolete `overlay_mode` deserialize
successfully.

- [ ] **Step 5: Commit the launch API removal**

```bash
rtk git add crates/rollshot-capture/src/types.rs crates/rollshot-capture/src/lib.rs crates/rollshot-cli/src/cmd_capture_launcher.rs crates/rollshot-app/src/launch.rs
rtk git commit -m "refactor!: remove obsolete overlay mode"
```

### Task 2: Remove the deprecated app, submodule, build graph, and CI support

**Files:**
- Delete: `crates/rollshot-tauri-app/`
- Delete: `learn-projects/tauri-template`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `.gitmodules`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Capture the expected failing removal checks**

Run:

```bash
rtk git ls-files crates/rollshot-tauri-app learn-projects/tauri-template
rtk rg -n '^name = "(rollshot-tauri-app|tauri)"$' Cargo.lock
rtk rg -n 'rollshot-tauri-app|pnpm|libwebkit2gtk|libxdo|libayatana-appindicator|librsvg' Cargo.toml .github/workflows/ci.yml .gitmodules
```

Expected: each command reports existing deprecated assets or support. These are
the checks that must become empty after the removal.

- [ ] **Step 2: Delete the deprecated app and reference submodule**

Run:

```bash
rtk git rm -r crates/rollshot-tauri-app learn-projects/tauri-template
```

Expected: Git stages deletion of the tracked Tauri app and the submodule
gitlink. Do not delete or edit `docs/**` or `spikes/**`.

- [ ] **Step 3: Remove workspace and submodule registrations**

Remove this member from `Cargo.toml`:

```toml
"crates/rollshot-tauri-app/src-tauri",
```

Remove this complete block from `.gitmodules`:

```ini
[submodule "learn-projects/tauri-template"]
	path = learn-projects/tauri-template
	url = https://github.com/dannysmith/tauri-template.git
```

- [ ] **Step 4: Remove Tauri-only CI work**

In `.github/workflows/ci.yml`:

- Delete the complete `frontend-test` job.
- Keep the existing Rust matrix job.
- Replace the Linux dependency installation with:

```yaml
      - name: Install Linux capture deps
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-dev libdbus-1-dev
```

- Remove `cargo check -p rollshot-tauri-app --all-targets` from the macOS target
  check block.

- [ ] **Step 5: Refresh the lockfile from the remaining workspace**

Run:

```bash
rtk cargo metadata --no-deps --format-version 1 >/dev/null
```

Expected: command succeeds, updates the lockfile for the current workspace, and
removes packages reachable only from the deleted Tauri workspace member without
requesting dependency upgrades.

- [ ] **Step 6: Verify deprecated assets and build support are gone**

Run:

```bash
rtk git ls-files crates/rollshot-tauri-app learn-projects/tauri-template
rtk rg -n '^name = "(rollshot-tauri-app|tauri)"$' Cargo.lock
rtk rg -n 'rollshot-tauri-app|pnpm|libwebkit2gtk|libxdo|libayatana-appindicator|librsvg' Cargo.toml .github/workflows/ci.yml .gitmodules
rtk git submodule status
rtk cargo metadata --no-deps --format-version 1
```

Expected: the first three commands print no matches; submodule status does not
list `learn-projects/tauri-template`; Cargo metadata succeeds without the
deleted crate.

- [ ] **Step 7: Commit the repository support removal**

```bash
rtk git add Cargo.toml Cargo.lock .gitmodules .github/workflows/ci.yml
rtk git commit -m "build!: remove deprecated Tauri app"
```

The deletions are already staged by `git rm`.

### Task 3: Rewrite active guidance, comments, and test names

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `crates/rollshot-cli/src/cmd_capture_launcher.rs`
- Modify: `crates/rollshot-cli/tests/capture_launcher.rs`
- Modify: `crates/rollshot-overlay-core/src/lib.rs`
- Modify: `crates/rollshot-overlay-core/src/tokens.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-iced-overlay/src/bin/capture_overlay.rs`

- [ ] **Step 1: Rewrite active user and agent documentation**

In `README.md`:

- Remove the deprecated Tauri workspace entry and the complete "Desktop app
  crates during iced migration" and "Tauri App" sections.
- Add `crates/rollshot-iced-overlay` to the workspace list as the capture
  overlay renderer used by `rollshot-app`.
- Change the GitHub Actions description to say it installs PipeWire/D-Bus
  development packages on Ubuntu.
- Remove the `cargo check -p rollshot-tauri-app` manual bootstrap item.

In `AGENTS.md`:

- Change the opening description to name an iced desktop app.
- Remove the frontend/Tauri verification instructions.
- Describe Linux and macOS as the two active iced capture UI paths without a
  host/fallback qualification.
- Remove deprecated Tauri files from the shared-path list.
- Remove the `rollshot-tauri-app` project-map item.
- Describe `rollshot-overlay-core` as framework-neutral logic shared by active
  overlay components, without naming Tauri/webview.
- Remove the `tauri-template` row from the `learn-projects` table.

- [ ] **Step 2: Rewrite active source comments to current ownership**

Use these direct replacements:

`crates/rollshot-overlay-core/src/lib.rs`:

```rust
//! Platform-independent overlay UI logic used by the native iced overlay:
//! live-preview viewport generation, capture-miss state, chrome placement, and
//! crop visual design tokens.
```

`crates/rollshot-overlay-core/src/tokens.rs` opening:

```rust
//! Crop selection visual design tokens consumed by the iced overlay's
//! `CropCanvas`.
```

Also rewrite `Rgba::to_css`'s comment to describe CSS-compatible formatting
without referring to `App.css` or a deleted sync test.

`crates/rollshot-iced-overlay/src/lib.rs` opening:

```rust
//! Iced capture overlay renderer for the active Linux and macOS product paths.
```

`crates/rollshot-iced-overlay/src/driver.rs`:

```rust
/// Stitch configuration used by the live capture overlay.
```

Rewrite the `stitch_stream` comment to describe the tested finite-stream core
without referencing `session.rs`.

`crates/rollshot-iced-overlay/src/bin/capture_overlay.rs` opening:

```rust
//! Standalone harness for the Phase 3 KDE 6 acceptance checks. Runs the
//! overlay, then saves the finalized image as a PNG.
```

- [ ] **Step 3: Remove migration-era CLI references**

In `crates/rollshot-cli/src/cmd_capture_launcher.rs`, delete the two negative
assertions that require error messages not to mention Tauri. Keep the positive
error-message assertions.

In `crates/rollshot-cli/tests/capture_launcher.rs`, rename the fake executable
paths:

```rust
"fake-rollshot-app"
"fake-rollshot-app.cmd"
"fake-rollshot-app-fail"
"fake-rollshot-app-fail.cmd"
```

- [ ] **Step 4: Verify active surfaces contain no stale Tauri references**

Run:

```bash
rtk rg -n --hidden \
  --glob '!docs/**' \
  --glob '!spikes/**' \
  --glob '!learn-projects/**' \
  --glob '!target/**' \
  --glob '!crates/rollshot-tauri-app/**' \
  'rollshot-tauri-app|Tauri|tauri|OverlayMode|overlay_mode' \
  Cargo.toml Cargo.lock README.md AGENTS.md .github crates scripts .gitmodules .ignore .rgignore
```

Expected: no matches. Investigate and remove every active match; do not edit
historical docs or spikes to satisfy this check.

- [ ] **Step 5: Run focused tests for touched active crates**

Run:

```bash
rtk cargo test -p rollshot-capture
rtk cargo test -p rollshot-cli
rtk cargo test -p rollshot-overlay-core
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-app
```

Expected: all tests PASS.

- [ ] **Step 6: Commit active-reference cleanup**

```bash
rtk git add README.md AGENTS.md crates/rollshot-cli/src/cmd_capture_launcher.rs crates/rollshot-cli/tests/capture_launcher.rs crates/rollshot-overlay-core/src/lib.rs crates/rollshot-overlay-core/src/tokens.rs crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/driver.rs crates/rollshot-iced-overlay/src/bin/capture_overlay.rs
rtk git commit -m "docs: describe iced-only desktop architecture"
```

### Task 4: Verify the complete removal

**Files:**
- Verify all changed files

- [ ] **Step 1: Run repository integrity checks**

Run:

```bash
rtk git diff --check main...HEAD
rtk git ls-files crates/rollshot-tauri-app learn-projects/tauri-template
rtk git submodule status
rtk rg -n '^name = "(rollshot-tauri-app|tauri)"$' Cargo.lock
rtk cargo metadata --no-deps --format-version 1
```

Expected: diff check and Cargo metadata succeed; the tracked-file and lockfile
searches print no matches; submodule status omits `tauri-template`.

- [ ] **Step 2: Run the active-reference scan**

Run:

```bash
rtk rg -n --hidden \
  --glob '!docs/**' \
  --glob '!spikes/**' \
  --glob '!learn-projects/**' \
  --glob '!target/**' \
  'rollshot-tauri-app|Tauri|tauri|OverlayMode|overlay_mode' \
  Cargo.toml Cargo.lock README.md AGENTS.md .github crates scripts .gitmodules .ignore .rgignore
```

Expected: no matches.

- [ ] **Step 3: Run full workspace verification**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test --workspace
```

Expected: all commands PASS. No Node, pnpm, WebKitGTK, or Tauri tooling is
required.

- [ ] **Step 4: Inspect final change scope**

Run:

```bash
rtk git status --short --branch
rtk git diff --stat main...HEAD
rtk git log --oneline main..HEAD
```

Expected: only the approved spec/plan and Tauri-removal work are present; the
working tree is clean after the task commits.
