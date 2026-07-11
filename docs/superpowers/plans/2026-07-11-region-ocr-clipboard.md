# Region OCR to Clipboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a cross-platform region-selection OCR command that copies recognized text to the clipboard and stdout, with daemon tray actions and `Alt+Shift+7` / `Command+Shift+7` shortcuts.

**Architecture:** Reuse the existing screenshot-region overlays and add an explicit OCR post-capture purpose. Extract OCR preparation and reading-order text assembly into a UI-independent app module, then let Linux run it after the overlay returns and macOS run it as an iced `Task` before exiting. The daemon launches that same CLI path through typed events and launch kinds.

**Tech Stack:** Rust, clap, iced 0.14, `rollshot-iced-overlay`, `rollshot-vision` with the `ocr` feature, `arboard`, Linux GlobalShortcuts portal/ksni, macOS `global-hotkey`/`tray-icon`, tracing.

## Global Constraints

- Existing shortcuts stay `Alt+Shift+6` on Linux and `Command+Shift+6` on macOS.
- Text shortcuts default to `Alt+Shift+7` on Linux and `Command+Shift+7` on macOS.
- OCR remains an off-by-default `rollshot-app` feature; non-OCR builds expose an explicit disabled CLI error but no OCR tray item or shortcut.
- OCR always captures one selected region; do not add fullscreen, scrolling, language, or engine options.
- Successful CLI output and clipboard text must be identical except for stdout's trailing newline.
- Cancellation and every failure preserve the previous clipboard contents.
- Never record recognized text in tracing or error messages. Use stable explicit `rollshot::*` tracing targets and structured privacy-safe fields.
- Check and test both active capture paths: Linux iced layer-shell and macOS iced ScreenCaptureKit.
- Prefix every shell command with `rtk`; do not create a worktree.

---

### Task 1: Extract the shared product OCR service

**Files:**
- Create: `crates/rollshot-app/src/product_ocr.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**
- Produces: `product_ocr::OcrItemId`, `product_ocr::OcrTextItem`, `product_ocr::ProductOcrError`, `product_ocr::prepare(&RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError>`, and `product_ocr::assemble_text(Vec<OcrTextItem>) -> Result<String, ProductOcrError>`.
- Consumes: `rollshot_vision::RealAutomationHost`, `rollshot_automation::{AutomationHost, OcrQuery, Region}`, and existing tiling/duplicate-merging rules.
- The result workspace imports the shared item/error/prepare functions; its selection/redaction state remains in `result_workspace::ocr_text`.

- [ ] **Step 1: Add failing shared-service tests**

Create `product_ocr.rs` with the public data types and tests first. Use the existing `ImageRect`/`ImageQuad` fields so no conversion layer is introduced:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OcrItemId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextItem {
    pub id: OcrItemId,
    pub text: String,
    pub confidence: f32,
    pub bounds: rollshot_automation::ImageRect,
    pub quad: rollshot_automation::ImageQuad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductOcrError {
    Disabled,
    SessionInit,
    Detect,
    InvalidRegion,
    EmptyResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_text_orders_lines_and_words() {
        let items = vec![
            item(2, "world", 60.0, 10.0),
            item(0, "second", 10.0, 40.0),
            item(1, "hello", 10.0, 10.0),
        ];
        assert_eq!(assemble_text(items).unwrap(), "hello world\nsecond");
    }

    #[test]
    fn assemble_text_rejects_empty_or_whitespace_only_results() {
        assert_eq!(assemble_text(vec![]), Err(ProductOcrError::EmptyResult));
        assert_eq!(
            assemble_text(vec![item(0, "   ", 0.0, 0.0)]),
            Err(ProductOcrError::EmptyResult)
        );
    }
}
```

Move the existing test item helper or reproduce it with `ImageRect` and an axis-aligned `ImageQuad`. Add a `message()` arm for `EmptyResult` returning `No text was recognized in the selected region`.

- [ ] **Step 2: Run the focused test and verify failure**

Run: `rtk cargo test -p rollshot-app product_ocr::tests --no-default-features`

Expected: FAIL because `assemble_text` is not defined.

- [ ] **Step 3: Implement minimal reading-order assembly**

Move the existing `reading_order` and `same_line` rules into `product_ocr.rs`, expose them as `pub(crate)`, and implement:

```rust
pub fn assemble_text(mut items: Vec<OcrTextItem>) -> Result<String, ProductOcrError> {
    items.sort_by(reading_order);
    let mut out = String::new();
    for (index, item) in items.iter().enumerate() {
        let text = item.text.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(previous) = items[..index].iter().rev().find(|item| !item.text.trim().is_empty()) {
            out.push(if same_line(previous.bounds, item.bounds) { ' ' } else { '\n' });
        }
        out.push_str(text);
    }
    let out = out.trim().to_owned();
    if out.is_empty() {
        Err(ProductOcrError::EmptyResult)
    } else {
        Ok(out)
    }
}
```

- [ ] **Step 4: Move OCR preparation without changing behavior**

Move `OcrItemId`, `OcrTile`, `vertical_tiles`, `merge_tile_items`, `iou`, and both cfg variants of `prepare_product_ocr` to `product_ocr.rs`. Rename the entry point to `prepare`. Keep `MAX_OCR_AREA`, 64-pixel tile overlap, 5,000-result limit, error mapping, and duplicate merge thresholds unchanged. Preserve `OcrItemId(items.len() as u64)` so result-workspace selection identity does not change.

In `main.rs`, add:

```rust
mod product_ocr;
```

In `result_workspace/ocr_text.rs`, import:

```rust
pub use crate::product_ocr::{OcrItemId, OcrTextItem, ProductOcrError};
use crate::product_ocr::{reading_order, same_line};
```

Remove only the definitions moved by this task. In `result_workspace/update.rs`, replace the preparation call with `crate::product_ocr::prepare(&image)`.

- [ ] **Step 5: Run shared and workspace OCR tests**

Run: `rtk cargo test -p rollshot-app product_ocr::tests --no-default-features`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app result_workspace::ocr_text::tests --no-default-features`

Expected: PASS, proving selection and redaction behavior did not drift.

- [ ] **Step 6: Commit the shared service**

```bash
rtk git add crates/rollshot-app/src/main.rs crates/rollshot-app/src/product_ocr.rs crates/rollshot-app/src/result_workspace/ocr_text.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "refactor(ocr): extract shared product service"
```

---

### Task 2: Add the quick-OCR coordinator and CLI contract

**Files:**
- Create: `crates/rollshot-app/src/quick_ocr.rs`
- Modify: `crates/rollshot-app/src/image_clipboard.rs`
- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/main.rs`

**Interfaces:**
- Produces: `launch::OcrArgs`, `LaunchMode::Ocr { options, graphical_feedback }`, `quick_ocr::run_with`, and `quick_ocr::run`.
- Consumes: `product_ocr::{prepare, assemble_text, ProductOcrError}` and a text-only clipboard boundary.
- Later platform tasks receive `InteractiveLaunchOptions` fixed to `CaptureRequest::screenshot_region()` and the graphical-feedback flag.

- [ ] **Step 1: Write failing CLI parsing tests**

Add tests in `launch.rs` that assert:

```rust
#[test]
fn ocr_uses_screenshot_region_and_capture_flags() {
    let mode = parse(&[
        "rollshot-app", "ocr", "--backend", "fixture", "--show-cursor",
    ]).unwrap();
    assert!(matches!(
        mode,
        LaunchMode::Ocr { options, graphical_feedback: false }
            if options.backend == "fixture"
                && options.show_cursor
                && options.initial_request == CaptureRequest::screenshot_region()
    ));
}

#[test]
fn ocr_rejects_workflow_and_scope_flags() {
    assert!(parse(&["rollshot-app", "ocr", "--scope", "fullscreen"]).is_err());
    assert!(parse(&["rollshot-app", "ocr", "--workflow", "scrolling"]).is_err());
}
```

Add a hidden `--graphical-feedback` bool to `OcrArgs` for daemon children only.

- [ ] **Step 2: Verify CLI tests fail**

Run: `rtk cargo test -p rollshot-app launch::tests::ocr --no-default-features`

Expected: FAIL because `LaunchCommand::Ocr` and `LaunchMode::Ocr` do not exist.

- [ ] **Step 3: Implement the CLI lowering**

Add:

```rust
#[derive(Debug, clap::Args)]
pub struct OcrArgs {
    #[arg(long, default_value = "auto", value_parser = rollshot_capture::KNOWN_BACKEND_NAMES)]
    pub backend: String,
    #[arg(long, default_value_t = false)]
    pub show_cursor: bool,
    #[arg(long, hide = true, default_value_t = false)]
    pub graphical_feedback: bool,
}
```

Lower it to `InteractiveLaunchOptions { backend, fps: 5, show_cursor, initial_request: CaptureRequest::screenshot_region() }`. Keep the `Ocr` command present without the Cargo feature so disabled builds can return a product error rather than clap's unknown-command error.

- [ ] **Step 4: Add failing coordinator side-effect tests**

Define a narrow test boundary:

```rust
pub(crate) trait TextClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

pub(crate) fn finish_with(
    items: Vec<crate::product_ocr::OcrTextItem>,
    clipboard: &mut dyn TextClipboard,
) -> Result<String, QuickOcrError>;
```

Tests must prove successful text is returned and written once, while empty OCR items never call the fake clipboard. Add a clipboard-error test whose rendered error contains no recognized text.

- [ ] **Step 5: Implement the coordinator and production clipboard adapter**

Add `set_text` beside the existing image clipboard function:

```rust
pub fn set_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("clipboard error: {error}"))?;
    clipboard.set_text(text)
        .map_err(|error| format!("clipboard write error: {error}"))
}
```

Implement `finish_with` by calling `assemble_text` before `clipboard.set_text`. Implement `run(image)` by calling `product_ocr::prepare`, then `finish_with` using the arboard adapter. Define typed `QuickOcrError::{Ocr(ProductOcrError), Clipboard}` with privacy-safe `Display` strings; do not store recognized text in the clipboard error variant.

In `main.rs`, add `mod quick_ocr;` and route feature-disabled `LaunchMode::Ocr` to `Err(ProductOcrError::Disabled.message().into())`. Ensure the existing top-level error branch writes that privacy-safe message to stderr with intentional user-facing `eprintln!`; do not print successful text here yet, because platform handoff is Task 3.

- [ ] **Step 6: Run CLI and coordinator tests**

Run: `rtk cargo test -p rollshot-app 'launch::tests::ocr' --no-default-features`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app quick_ocr::tests --no-default-features`

Expected: PASS without touching the system clipboard.

- [ ] **Step 7: Commit the CLI and coordinator**

```bash
rtk git add crates/rollshot-app/src/image_clipboard.rs crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs crates/rollshot-app/src/quick_ocr.rs
rtk git commit -m "feat(ocr): add quick text capture command"
```

---

### Task 3: Route Linux and macOS capture completion into quick OCR

**Files:**
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/post_capture.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/quick_ocr.rs`

**Interfaces:**
- Produces: `CapturePurpose::{Present, Ocr { graphical_feedback }}` and platform capture runners accepting that purpose.
- Consumes: `quick_ocr::run(RgbaImage) -> Result<String, QuickOcrError>`.
- stdout is written only after both OCR and clipboard succeed.

- [ ] **Step 1: Write Linux post-capture purpose tests**

Add a pure dispatch decision in `post_capture.rs` and test completed/cancelled OCR separately:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePurpose {
    Present,
    Ocr { graphical_feedback: bool },
}

#[test]
fn cancelled_ocr_has_no_post_capture_work() {
    assert!(matches!(
        select_completion(CapturePurpose::Ocr { graphical_feedback: false }, None),
        PurposeCompletion::Cancelled
    ));
}
```

The decision type must preserve the selected purpose alongside a completed `CaptureResult`; do not duplicate capture execution.

- [ ] **Step 2: Run the focused test and verify failure**

Run: `rtk cargo test -p rollshot-app post_capture::tests --no-default-features`

Expected: FAIL because capture purpose dispatch is missing.

- [ ] **Step 3: Implement Linux quick-OCR handoff**

Change `run_iced_capture` / `run_product_capture` to accept `CapturePurpose`. For Linux, after `run_overlay` returns:

```rust
match post_capture::capture_completion(result) {
    CaptureCompletion::Cancelled => Ok(()),
    CaptureCompletion::Present(result) => match purpose {
        CapturePurpose::Present => post_capture::handle_linux_capture(result),
        CapturePurpose::Ocr { graphical_feedback } => {
            quick_ocr::complete_cli(result.image, graphical_feedback)
        }
    },
}
```

`complete_cli` runs OCR, writes successful text using `println!("{text}")` as intentional user-facing CLI stdout, and invokes graphical feedback only when requested. Cancellation never reaches it. Use a small feedback function backed by the already-present `rfd::MessageDialog`; call it only after the capture overlay has closed. Errors return to `main` for nonzero exit status and stderr output.

- [ ] **Step 4: Add macOS state-transition tests**

Add `CapturePurpose` to `MacosProduct` state. Test that OCR completion does not create thumbnail/workspace state and that `QuickOcrFinished(Ok(text))` requests exit. Keep tests on pure helpers where macOS runtime construction is unavailable on Linux.

- [ ] **Step 5: Implement macOS background OCR in the existing iced daemon**

Add:

```rust
Message::QuickOcrFinished(Result<String, crate::quick_ocr::QuickOcrError>)
```

When `complete_capture` sees OCR purpose, close capture-owned windows, shut down the component, and return a `Task::perform` that uses `tokio::task::spawn_blocking(move || quick_ocr::run(image))`. Map the result to `QuickOcrFinished`. In `update`, print and optionally show feedback only on success, report a privacy-safe error on failure, then return `iced::exit()`.

Do not create a second iced event loop, a custom widget, a thumbnail, an auto-save, or a result workspace for OCR purpose.

- [ ] **Step 6: Run platform-neutral and feature builds**

Run: `rtk cargo test -p rollshot-app post_capture::tests --no-default-features`

Expected: PASS.

Run: `rtk cargo check -p rollshot-app --features ocr`

Expected: PASS with the OCR-enabled Linux path compiled. macOS compilation remains a required macOS verification in Task 6.

- [ ] **Step 7: Commit capture routing**

```bash
rtk git add crates/rollshot-app/src/main.rs crates/rollshot-app/src/post_capture.rs crates/rollshot-app/src/macos_product.rs crates/rollshot-app/src/quick_ocr.rs
rtk git commit -m "feat(ocr): route region captures to clipboard"
```

---

### Task 4: Add typed daemon text-capture configuration and process launch

**Files:**
- Modify: `crates/rollshot-app/src/daemon/config.rs`
- Modify: `crates/rollshot-app/src/daemon/core.rs`
- Modify: `crates/rollshot-app/src/daemon/process.rs`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`

**Interfaces:**
- Produces: `DaemonConfig::capture_text_hotkey`, `DaemonEvent::CaptureText`, and `CaptureKind::{Region, Text}`.
- Changes: `CaptureLauncher::launch(&mut self, id, kind, events)`.
- Process mapping: `Region` uses existing args; `Text` uses `ocr --graphical-feedback`.

- [ ] **Step 1: Write backward-compatible config tests**

Add tests for Linux/macOS defaults, a legacy file containing only `capture_region_hotkey`, independent override, and invalid text shortcut fallback. Expected defaults are exact:

```rust
assert_eq!(linux.capture_text_hotkey.to_string(), "Alt+Shift+7");
assert_eq!(macos.capture_text_hotkey.to_string(), "Command+Shift+7");
```

Change `RawDaemonConfig` fields to `Option<String>` so either field may be omitted. Return warnings that identify only the invalid field without including unrelated configuration values.

- [ ] **Step 2: Verify config tests fail**

Run: `rtk cargo test -p rollshot-app daemon::config::tests --no-default-features`

Expected: FAIL because `capture_text_hotkey` does not exist.

- [ ] **Step 3: Implement independent config fallback**

Add `capture_text_hotkey` to `DaemonConfig`, set both platform defaults, and parse each optional raw field independently. Preserve a valid configured region shortcut when text is invalid and vice versa. Update existing config tests and provider-config fixtures only where compilation or exact serialized config requires the new field.

- [ ] **Step 4: Write daemon core and launcher tests**

Extend the fake launcher state to record kinds and assert:

```rust
core.handle(DaemonEvent::CaptureText);
assert_eq!(state.lock().unwrap().kinds, vec![CaptureKind::Text]);
```

Also start `CaptureRegion`, trigger `CaptureText`, and assert only one launch. Add process argument tests expecting:

```rust
assert_eq!(capture_args(CaptureKind::Region), ["capture", "--workflow", "screenshot", "--scope", "region"]);
assert_eq!(capture_args(CaptureKind::Text), ["ocr", "--graphical-feedback"]);
```

Use a slice return type because the two argument lists have different lengths.

- [ ] **Step 5: Implement typed daemon launch routing**

Define `CaptureKind` beside `CaptureId`; add `kind` to the launcher trait. Route `CaptureRegion` and `CaptureText` through one private `start_capture(kind)` helper while preserving monotonically increasing IDs, active-child exclusion, stale exit handling, and process-group cleanup.

Do not register or emit `CaptureText` from non-OCR platform code yet; Task 5 gates those adapters with `#[cfg(feature = "ocr")]`.

- [ ] **Step 6: Run daemon domain tests**

Run: `rtk cargo test -p rollshot-app 'daemon::config::tests|daemon::core::tests|daemon::process::tests' --no-default-features`

Expected: PASS. If Cargo's name filter does not accept the alternation, run the three module filters as separate `rtk cargo test` commands.

- [ ] **Step 7: Commit daemon domain changes**

```bash
rtk git add crates/rollshot-app/src/daemon/config.rs crates/rollshot-app/src/daemon/core.rs crates/rollshot-app/src/daemon/process.rs crates/rollshot-app/src/daemon/mod.rs
rtk git commit -m "feat(daemon): add text capture launch kind"
```

---

### Task 5: Wire OCR tray actions and both global-shortcut adapters

**Files:**
- Modify: `crates/rollshot-app/src/daemon/linux.rs`
- Modify: `crates/rollshot-app/src/daemon/linux/tray.rs`
- Modify: `crates/rollshot-app/src/daemon/linux/shortcut.rs`
- Modify: `crates/rollshot-app/src/daemon/macos.rs`
- Modify: `crates/rollshot-app/src/daemon/macos/tray.rs`
- Modify: `crates/rollshot-app/src/daemon/macos/shortcut.rs`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`

**Interfaces:**
- Consumes: `DaemonEvent::CaptureText` and `DaemonConfig::capture_text_hotkey` from Task 4.
- Produces: feature-gated `Capture Text` tray action and two-ID shortcut routing on both platforms.
- Builds without `ocr` retain exactly the existing tray and shortcut behavior.

- [ ] **Step 1: Add failing Linux adapter tests**

Under `#[cfg(feature = "ocr")]`, assert that `activate_text` sends `CaptureText` and menu labels are `Capture Region`, `Capture Text`, `Quit Rollshot`. Replace the single-ID predicate with:

```rust
fn event_for_shortcut(id: &str) -> Option<DaemonEvent> {
    match id {
        "capture-region" => Some(DaemonEvent::CaptureRegion),
        "capture-text" => Some(DaemonEvent::CaptureText),
        _ => None,
    }
}
```

Test both IDs and both configured portal triggers.

- [ ] **Step 2: Implement Linux portal and tray wiring**

Pass both shortcuts into `ShortcutGuard::start`. Bind two `NewShortcut` values in one portal session, verify both returned IDs in OCR builds, and route activation through `event_for_shortcut`. Add the tray item only under the `ocr` feature. Keep portal startup best-effort and tray startup required.

- [ ] **Step 3: Add failing macOS adapter tests**

Add `TEXT_ID = "capture-text"`, test tray ID mapping, and test `Command+Shift+7` translates to `Modifiers::SUPER | Modifiers::SHIFT` plus `Code::Digit7`. Add a pure helper that maps registered hotkey IDs to daemon events and test both mappings.

- [ ] **Step 4: Implement macOS dual registration and tray wiring**

Change `ShortcutGuard` to own `Vec<HotKey>`. Register region and, under the OCR feature, text hotkeys with the same manager. The process-global handler compares IDs and sends the corresponding semantic event. `Drop` clears the handler and unregisters every owned hotkey.

Change `DaemonApp` to hold the two configured shortcuts (or the whole cloned `DaemonConfig`) and log both preferred shortcuts without recognized text. Add the feature-gated tray item between capture and quit.

- [ ] **Step 5: Run both feature configurations on Linux**

Run: `rtk cargo test -p rollshot-app daemon --no-default-features`

Expected: PASS with legacy menu/shortcut behavior.

Run: `rtk cargo test -p rollshot-app daemon --features ocr`

Expected: PASS with `Capture Text`, dual portal binding, and text process routing tests.

- [ ] **Step 6: Commit platform daemon wiring**

```bash
rtk git add crates/rollshot-app/src/daemon/linux.rs crates/rollshot-app/src/daemon/linux/tray.rs crates/rollshot-app/src/daemon/linux/shortcut.rs crates/rollshot-app/src/daemon/macos.rs crates/rollshot-app/src/daemon/macos/tray.rs crates/rollshot-app/src/daemon/macos/shortcut.rs crates/rollshot-app/src/daemon/mod.rs
rtk git commit -m "feat(daemon): add OCR tray and shortcuts"
```

---

### Task 6: Privacy regression tests, documentation, and release verification

**Files:**
- Modify: `crates/rollshot-app/src/quick_ocr.rs`
- Modify: `crates/rollshot-app/src/diagnostics.rs` if a new stable target constant is needed
- Modify: `README.md`

**Interfaces:**
- Verifies all earlier interfaces together.
- Documents only user-visible command, shortcuts, feature requirement, stdout behavior, and configuration key.

- [ ] **Step 1: Add privacy regression tests**

Use `diagnostics::capture_test_logs` around empty/OCR/clipboard failures with a sentinel recognized string such as `PRIVATE_OCR_SENTINEL`. Assert the log and rendered errors do not contain the sentinel, while they do contain stable categories such as `empty_result` or `clipboard_write`.

- [ ] **Step 2: Run privacy tests and verify failure before instrumentation is finalized**

Run: `rtk cargo test -p rollshot-app quick_ocr::tests --no-default-features`

Expected: the new privacy/category assertion fails until coordinator diagnostics use only structured category fields.

- [ ] **Step 3: Add privacy-safe diagnostics and README usage**

Use a stable target such as `rollshot::app::quick_ocr`. Log only `image_width`, `image_height`, `item_count`, `stage`, and `error_category`. Never attach the OCR result or the underlying clipboard error with `%error` if it could contain input text.

Document:

```text
cargo run -p rollshot-app --features ocr -- ocr
```

Document successful stdout + clipboard behavior, Esc cancellation, `capture_text_hotkey`, and both platform defaults. State that OCR tray/shortcut entries require an OCR-enabled build.

- [ ] **Step 4: Run formatting and all app tests**

Run: `rtk cargo fmt --check`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app --no-default-features`

Expected: PASS without building the OCR runtime.

Run: `rtk cargo test -p rollshot-app --features ocr`

Expected: PASS, including OCR service, CLI, daemon, and privacy tests.

- [ ] **Step 5: Run workspace lint verification**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS. This verifies the default workspace path; run `rtk cargo clippy -p rollshot-app --all-targets --features ocr -- -D warnings` separately for feature-gated code.

- [ ] **Step 6: Perform platform runtime verification**

On Linux, verify `Alt+Shift+6` is unchanged and `Alt+Shift+7` plus `Capture Text` select a region and copy text. On macOS, repeat with `Command+Shift+6` and `Command+Shift+7`. On both platforms verify CLI stdout matches clipboard contents, Esc is silent, empty selection preserves clipboard, only one overlay can be active, and success/failure feedback appears only for daemon-launched OCR.

Record any platform that could not be exercised in the final handoff, including the unchecked counterpart path and remaining runtime risk. No stitching benchmark is required.

- [ ] **Step 7: Commit documentation and verification tests**

```bash
rtk git add crates/rollshot-app/src/quick_ocr.rs crates/rollshot-app/src/diagnostics.rs README.md
rtk git commit -m "docs(ocr): document region text capture"
```
