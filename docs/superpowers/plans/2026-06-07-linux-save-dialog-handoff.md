# Linux Save Dialog Handoff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the Linux layer-shell Result Review workspace before opening the native Save As dialog.

**Architecture:** Preserve the existing `run_overlay -> Result<Option<CaptureResult>>` API and add a post-overlay request to `CaptureResult`. Linux Result Review Save marks the result for Save As and exits iced; `rollshot-app` receives the completed image after the overlay is gone, opens the native dialog, and writes the PNG. macOS keeps its current in-workspace output path.

**Tech Stack:** Rust, iced/layer-shell, `rfd`, `image`, Cargo tests

---

### Task 1: Represent Post-Overlay Save As Intent

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/screenshot.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`

- [ ] **Step 1: Write the failing result-intent tests**

Add tests proving ordinary screenshot and stitch results default to
`PostOverlayRequest::None`, and update the `rollshot-app` capture-result fixture
to require the new field.

```rust
assert_eq!(result.post_overlay_request, PostOverlayRequest::None);
```

- [ ] **Step 2: Run focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay screenshot
rtk cargo test -p rollshot-iced-overlay driver
rtk cargo test -p rollshot-app
```

Expected: compilation or assertions fail because `PostOverlayRequest` and the
new result field do not exist.

- [ ] **Step 3: Add the result request type and initialize constructors**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PostOverlayRequest {
    #[default]
    None,
    SaveAs,
}

pub struct CaptureResult {
    pub image: RgbaImage,
    pub stats: Option<StitchStats>,
    pub post_overlay_request: PostOverlayRequest,
}
```

Initialize every `CaptureResult` constructor with
`PostOverlayRequest::None`. Update explicit test fixtures and deprecated Tauri
reference fixtures to compile without changing their behavior.

- [ ] **Step 4: Run focused tests and verify they pass**

Run the Step 2 commands.

Expected: all focused tests pass.

### Task 2: Exit Linux Result Review With Save As Handoff

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Test: `crates/rollshot-iced-overlay/src/linux_runner.rs`

- [ ] **Step 1: Write failing Linux handoff tests**

Extract a pure decision helper for Linux result-review output:

```rust
enum LinuxOutputDecision {
    ExitForSaveAs,
    PerformInOverlay,
}
```

Add tests proving `Save` returns `ExitForSaveAs` and `Copy` returns
`PerformInOverlay`.

- [ ] **Step 2: Run the focused Linux tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner
```

Expected: fail because the decision helper and Save handoff do not exist.

- [ ] **Step 3: Implement Linux Save handoff**

When `OverlayEffect::PerformOutput(OutputAction::Save)` is received:

1. Verify `RESULT_SLOT` contains a completed result.
2. Set `result.post_overlay_request = PostOverlayRequest::SaveAs`.
3. Return `iced::exit()` without constructing `ArboardOutput` or calling `rfd`.

Keep `Copy` routed through the existing `perform_output_action`. Preserve the
current in-workspace error when no result exists.

- [ ] **Step 4: Run the focused Linux tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner
rtk cargo test -p rollshot-iced-overlay
```

Expected: all tests pass.

### Task 3: Open Save As After Overlay Exit

**Files:**
- Create: `crates/rollshot-app/src/save.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `Cargo.lock`
- Test: `crates/rollshot-app/src/save.rs`
- Test: `crates/rollshot-app/src/main.rs`

- [ ] **Step 1: Write failing app handoff and PNG-write tests**

Add a pure post-overlay decision:

```rust
pub enum PostOverlayAction {
    ExitSuccess,
    ExitCancelled,
    SaveAs(CaptureResult),
}
```

Test that a result marked `PostOverlayRequest::SaveAs` maps to
`PostOverlayAction::SaveAs`. Add a PNG-write test that writes a small image and
decodes it successfully.

- [ ] **Step 2: Run app tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: fail because the Save As action and save module do not exist.

- [ ] **Step 3: Implement app-owned Save As**

Add `rfd = "0.15"` and move `image` from dev-dependencies to dependencies.
Implement `save.rs` with:

```rust
pub fn prompt_save_path() -> Option<PathBuf>
pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String>
pub fn save_as(image: &RgbaImage) -> Result<SaveOutcome, String>
```

After `run_overlay` returns, handle `PostOverlayAction::SaveAs(result)` by
opening the dialog and writing the image. Treat dialog cancellation as a normal
cancelled completion and propagate write failures as application errors.

- [ ] **Step 4: Run app and overlay tests**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-iced-overlay
```

Expected: all tests pass.

### Task 4: Verify Platform Scope and Workspace

**Files:**
- Verify only

- [ ] **Step 1: Verify Linux Save no longer calls overlay Save As**

Run:

```bash
rtk rg -n "save_as\\(|FileDialog" crates/rollshot-iced-overlay/src/linux_runner.rs
```

Expected: no matches.

- [ ] **Step 2: Verify macOS keeps its current in-workspace output path**

Run:

```bash
rtk rg -n "perform_output_action|OutputAction::Save" crates/rollshot-iced-overlay/src/macos_runner.rs
```

Expected: macOS output handling remains present.

- [ ] **Step 3: Run full verification**

Run:

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk git diff --check
```

Expected: all commands pass.

- [ ] **Step 4: Manual runtime verification**

On Linux, verify normal and scrolling Result Review Save both close the overlay
before the dialog appears; verify successful save writes the full-resolution
PNG and cancellation exits without reopening Result Review. On macOS, verify
Save behavior is unchanged.
