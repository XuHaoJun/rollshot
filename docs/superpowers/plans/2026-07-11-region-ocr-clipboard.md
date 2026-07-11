# Region OCR to Clipboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a cross-platform region-selection OCR command that copies recognized text to the clipboard and stdout, with daemon tray actions and `Alt+Shift+7` / `Command+Shift+7` shortcuts.

**Architecture:** Reuse the existing screenshot-region overlays and add an explicit OCR post-capture purpose. Extract OCR preparation and reading-order text assembly into a UI-independent app module, then let Linux run it after the overlay returns and macOS run it as an iced `Task` before exiting. The daemon launches that same CLI path through typed events and launch kinds.

**Tech Stack:** Rust, clap, iced 0.14, `rollshot-iced-overlay`, `rollshot-vision` with the `ocr` feature, `arboard`, Linux GlobalShortcuts portal/ksni, macOS `global-hotkey`/`tray-icon`, tracing.

## File Structure

- Create `crates/rollshot-app/src/product_ocr.rs` — shared OCR preparation, ordering metadata, and full-text assembly.
- Create `crates/rollshot-app/src/quick_ocr.rs` — clipboard/output coordination, typed failures, and daemon-child feedback.
- Modify `crates/rollshot-app/src/main.rs` — launch routing and user-facing stderr.
- Modify `crates/rollshot-app/src/launch.rs` — `ocr` command and fixed screenshot-region options.
- Modify `crates/rollshot-app/src/image_clipboard.rs` — text clipboard adapter.
- Modify `crates/rollshot-app/src/post_capture.rs` — capture-purpose dispatch.
- Modify `crates/rollshot-app/src/macos_product.rs` — in-loop background OCR completion.
- Modify `crates/rollshot-app/src/result_workspace/ocr_text.rs` — consume shared OCR types and ordering metadata.
- Modify `crates/rollshot-app/src/result_workspace/update.rs` — call shared OCR preparation.
- Modify `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs` — preserve daemon shortcut fields when provider settings are saved.
- Modify `crates/rollshot-app/src/diagnostics.rs` — stable quick-OCR diagnostic target/category if needed.
- Modify `crates/rollshot-app/src/daemon/{config,core,process,mod}.rs` — typed text-capture event, config, and child launch.
- Modify `crates/rollshot-app/src/daemon/linux{,/tray,/shortcut}.rs` — Linux tray and two-ID portal routing.
- Modify `crates/rollshot-app/src/daemon/macos{,/tray,/shortcut}.rs` — macOS tray and best-effort second hotkey.
- Modify `crates/rollshot-app/Cargo.toml` — reuse `notify-rust = "4"` for transient daemon-child feedback.
- Modify `.github/workflows/ci-ocr.yml` — run all app OCR tests and clippy on Linux and macOS.
- Modify `README.md` — command, shortcuts, feature gate, stdout, and configuration.

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
- Treat desktop notification delivery as best-effort. Notification failure must not turn a successful OCR/clipboard operation into failure; an OCR failure falls back to an error dialog when notification delivery itself fails.
- Rollshot must not clear or write the clipboard before non-empty OCR text exists. Arbitrary-format clipboard restoration after an OS clipboard API partially fails is not portable and is explicitly outside the application-level guarantee.
- Commit clipboard text before writing stdout. A later broken-pipe error is a partial-success output failure: clipboard text remains valid, no success text was falsely emitted, and the process exits nonzero.

## Engineering Review Record (Auto Mode)

### What already exists

- `result_workspace/ocr_text.rs` already owns OCR tiling, match conversion, reading order, duplicate merging, and full-document text generation. The plan extracts and reuses these rules instead of introducing a second OCR implementation.
- `result_workspace/update.rs::prepare_ocr_task` already demonstrates the project-approved `Task::perform` plus `tokio::task::spawn_blocking` pattern. macOS quick OCR reuses this pattern and handles `JoinError` explicitly.
- `post_capture.rs::CaptureCompletion` already distinguishes cancellation from a captured result. The plan extends post-capture purpose without duplicating overlay execution.
- `daemon::core` already serializes capture children and protects against stale exits; the new typed kind extends the same state machine.
- Linux already binds one GlobalShortcuts portal session, and macOS already owns a `GlobalHotKeyManager`; both are extended in place.
- `rollshot-iced-overlay` already uses `notify-rust = "4"` for transient Linux feedback. The app reuses the same dependency family rather than building notification IPC.
- `.github/workflows/ci-ocr.yml` already provisions models and ONNX Runtime on Ubuntu and macOS. The plan expands its app coverage rather than adding a workflow.

### Auto decisions

#### Auto decision D1 — Share ordering metadata, not a second formatter

Context: The original `assemble_text` snippet trimmed every OCR item, while the current workspace preserves item text and trims only the completed document.

ELI10: Two paths that supposedly copy the same page could produce different text around punctuation or whitespace. Sharing only the sort comparator is not enough; they must share where line breaks occur too.

Stakes if we pick wrong: CLI output and Result Workspace copy-all drift for the same OCR result.

Recommendation: **D1A** because DRY and explicit shared metadata preserve current behavior with a small diff.

Note: options differ in kind, not coverage — no completeness score.

- **D1A — Shared `OrderedOcrItems` (recommended):** ✅ one ordering/line-break source; ❌ moves a little more state into `product_ocr` (human: ~3 hours / AI: ~20 min).
- **D1B — Independent formatter:** ✅ smallest immediate edit; ❌ guaranteed long-term semantic drift (human: ~1 hour / AI: ~10 min).

Net: pay a small extraction cost once to keep both product surfaces identical.

#### Auto decision D2 — Use transient notifications, not success dialogs

Context: `rfd::MessageDialog::show` blocks for a button press, so it does not satisfy the approved brief `Text copied` notification.

ELI10: A quick copy shortcut should finish without making the user click OK every time. A notification is a small banner; a dialog interrupts the workflow.

Stakes if we pick wrong: every hotkey use adds an unwanted modal interaction.

Recommendation: **D2A** because boring existing technology (`notify-rust`) matches the UX and is already in the repository.

Completeness: D2A=9/10, D2B=6/10.

- **D2A — `notify-rust` with error-dialog fallback (recommended):** ✅ transient success and visible failure; ❌ adds one app dependency and macOS delivery remains permission-dependent (human: ~4 hours / AI: ~30 min).
- **D2B — `rfd` dialogs only:** ✅ no new dependency; ❌ success becomes modal and violates the interaction goal (human: ~1 hour / AI: ~10 min).

Net: use notifications for the normal path and reserve a modal fallback for failed operations whose notification cannot be delivered.

#### Auto decision D3 — Preserve the original shortcut when text registration fails

Context: Both portal and macOS APIs can accept one shortcut and reject the other; returning one aggregate error would tear down the working region shortcut.

ELI10: Adding shortcut 7 must not break shortcut 6. Treat shortcut 6 as required and shortcut 7 as best-effort, while leaving `Capture Text` in the tray.

Stakes if we pick wrong: an OCR shortcut conflict silently disables ordinary capture too.

Recommendation: **D3A** because reversibility and blast-radius control protect existing behavior.

Completeness: D3A=10/10, D3B=5/10.

- **D3A — Required region + best-effort text registration (recommended):** ✅ existing shortcut survives partial failure; ❌ adapter state tracks an optional second registration (human: ~5 hours / AI: ~35 min).
- **D3B — All-or-nothing pair:** ✅ simpler constructor; ❌ one conflict removes both shortcuts (human: ~2 hours / AI: ~15 min).

Net: a new optional capability must not regress the established capture path.

#### Auto decision D4 — Return all configuration warnings

Context: Two independently invalid shortcut fields cannot be represented faithfully by `LoadedConfig.warning: Option<String>`.

ELI10: If both settings are wrong, showing only one makes the user fix the file twice. A list reports every problem in one startup.

Stakes if we pick wrong: diagnostics conceal one fallback and slow recovery.

Recommendation: **D4A** because explicit structured warnings are easier to test and operate.

Completeness: D4A=10/10, D4B=7/10.

- **D4A — `warnings: Vec<String>` (recommended):** ✅ represents zero, one, or two failures directly; ❌ updates two platform consumers and tests (human: ~3 hours / AI: ~20 min).
- **D4B — Concatenate into one string:** ✅ smaller type change; ❌ loses structure and encourages string parsing (human: ~1 hour / AI: ~10 min).

Net: use a collection when the domain now permits multiple simultaneous warnings.

#### Auto decision D5 — Reuse and harden the existing blocking-worker pattern

Context: OCR is synchronous CPU work and macOS must keep its iced event loop responsive.

ELI10: Running OCR on the UI thread freezes every window. `spawn_blocking` uses a worker thread, but the plan must also handle a worker panic and remember that started blocking work cannot be cancelled.

Stakes if we pick wrong: macOS can freeze, panic, or exit without reporting why.

Recommendation: **D5A** because it follows an established local pattern and makes its failure explicit.

Completeness: D5A=9/10, D5B=6/10.

- **D5A — `Task::perform` + `spawn_blocking` + typed join failure (recommended):** ✅ responsive and consistent with existing OCR workspace code; ❌ an in-flight worker finishes unless the daemon child is terminated (human: ~3 hours / AI: ~20 min).
- **D5B — synchronous OCR in `update`:** ✅ fewer types; ❌ blocks the event loop for model initialization and detection (human: ~1 hour / AI: ~10 min).

Net: bounded one-shot OCR belongs on the runtime's blocking pool, with child termination as the hard cancellation boundary.

#### Auto decision D6 — Test output and feedback boundaries with fakes

Context: The original plan tested returned strings but not stdout formatting or feedback gating.

ELI10: A function can return correct text while the CLI prints it twice, omits the newline, or shows a popup during direct CLI use. Injecting output and feedback makes these rules deterministic.

Stakes if we pick wrong: automation-visible behavior regresses without unit-test detection.

Recommendation: **D6A** because AI-assisted completeness is cheap and avoids GUI-dependent tests.

Completeness: D6A=10/10, D6B=7/10.

- **D6A — Fake clipboard/output/feedback ports (recommended):** ✅ deterministic success, cancel, and failure coverage; ❌ three narrow internal traits/functions instead of one (human: ~5 hours / AI: ~40 min).
- **D6B — Return-value tests plus manual smoke:** ✅ less test scaffolding; ❌ stdout and notification rules remain manual-only (human: ~2 hours / AI: ~15 min).

Net: fake only side-effect boundaries; keep OCR orchestration concrete.

#### Auto decision D7 — Expand the existing OCR CI lane

Context: The OCR workflow currently runs only `rollshot-app` tests matching `eval`, so new OCR app behavior would compile but not execute its tests.

ELI10: Tests that never run are decoration. The existing expensive OCR lane already has models and both OSes, so it should own the new app tests and clippy.

Stakes if we pick wrong: Linux passes locally while macOS feature-gated paths rot unnoticed.

Recommendation: **D7A** because distribution quality requires both supported product targets in CI.

Completeness: D7A=10/10, D7B=6/10.

- **D7A — Full app OCR tests/clippy in `ci-ocr.yml` (recommended):** ✅ exercises both OS matrices with provisioned runtime; ❌ increases OCR lane duration (human: ~2 hours / AI: ~15 min).
- **D7B — Keep local/manual verification:** ✅ no CI time increase; ❌ feature tests can silently stop running (human: ~0 / AI: ~0).

Net: spend time in the lane already designed for this heavyweight feature.

#### Auto decision D8 — Make plan steps mechanically executable

Context: The original plan lacked a top-level file inventory, contained an invalid Cargo alternation filter, and bundled some Run/Expected pairs.

ELI10: An execution plan should work when copied line by line. Ambiguous file lists and commands create avoidable review churn.

Stakes if we pick wrong: agents touch undeclared files or believe tests ran when Cargo filtered everything out.

Recommendation: **D8A** because systems should guide tired humans and agents reliably.

Completeness: D8A=10/10, D8B=7/10.

- **D8A — Exact inventory and separate commands (recommended):** ✅ reproducible steps and accurate commits; ❌ longer plan text (human: ~2 hours / AI: ~15 min).
- **D8B — Leave executor discretion:** ✅ shorter plan; ❌ inconsistent execution and review evidence (human: ~0 / AI: ~0).

Net: verbosity in a runbook is cheaper than ambiguity during implementation.

#### Auto decision D9 — State the portable clipboard guarantee precisely

Context: `arboard` cannot transactionally restore every platform clipboard format after a lower-level partial write failure.

ELI10: Rollshot can promise not to touch the clipboard until good text exists. It cannot reliably reconstruct arbitrary clipboard data owned by another app if the operating system fails halfway through replacing it.

Stakes if we pick wrong: tests claim an atomic guarantee the production API cannot provide.

Recommendation: **D9A** because explicit operational limits are more trustworthy than an unimplementable abstraction.

Note: options differ in kind, not coverage — no completeness score.

- **D9A — Guarantee no preparatory mutation; disclose OS atomicity limit (recommended):** ✅ testable and portable; ❌ rare platform partial-write behavior remains outside Rollshot's control (human: ~1 hour / AI: ~10 min).
- **D9B — Snapshot only text/image formats:** ✅ can restore two common types; ❌ rejects or loses unsupported formats and doubles large-image clipboard memory (human: ~2 days / AI: ~2 hours).

Net: never clear early, but do not invent a false arbitrary-format transaction.

#### Auto decision D10 — Define the unavoidable stdout/clipboard partial-success order

Context: Clipboard replacement and stdout writing are separate OS side effects with no cross-resource transaction.

ELI10: If one succeeds and the second fails, Rollshot cannot rewind both perfectly. Writing the clipboard first ensures stdout never claims success before the requested copy has happened.

Stakes if we pick wrong: scripts may consume text even though the clipboard operation failed.

Recommendation: **D10A** because explicit ordering protects the stronger automation contract.

Note: options differ in kind, not coverage — no completeness score.

- **D10A — Clipboard then stdout (recommended):** ✅ stdout implies clipboard success; ❌ broken pipe leaves copied text with a nonzero exit (human: ~1 hour / AI: ~10 min).
- **D10B — Stdout then clipboard:** ✅ broken pipe leaves clipboard untouched; ❌ scripts can receive text before clipboard failure (human: ~1 hour / AI: ~10 min).

Net: preserve truthful stdout semantics and document the rare partial-success case.

#### Auto decision D11 — Preserve redacted `Debug` for OCR items

Context: The original plan snippet replaced the existing custom privacy-safe `Debug` implementation with `#[derive(Debug)]`.

ELI10: Debug output often reaches failure logs. If it prints the `text` field, private screen content can leak even though normal tracing is careful.

Stakes if we pick wrong: OCR content appears in CI failures, panic reports, or diagnostics.

Recommendation: **D11A** because privacy must be enforced by the type, not reviewer memory.

Completeness: D11A=10/10, D11B=3/10.

- **D11A — Keep custom redacted `Debug` plus sentinel test (recommended):** ✅ every caller gets safe debug output automatically; ❌ a few explicit formatter lines (human: ~1 hour / AI: ~10 min).
- **D11B — Derive `Debug` and avoid logging manually:** ✅ less code; ❌ one accidental `?item` leaks recognized text (human: ~0 / AI: ~0).

Net: preserve the existing privacy boundary during extraction.

### NOT in scope

- Fullscreen or scrolling OCR — region screenshot is the approved minimum flow.
- OCR language/engine selection — existing bundled OCR configuration remains unchanged.
- Review/edit UI before copying — quick OCR intentionally skips the workspace.
- Enabling OCR in default workspace members — preserves current model/runtime build isolation.
- New notification infrastructure or daemon-child IPC — reuse `notify-rust`; child owns feedback.
- Atomic restoration of arbitrary clipboard MIME formats after a platform API partially mutates then errors — not representable portably through `arboard`.
- Atomic rollback across clipboard and stdout — these are independent OS resources; clipboard commits first and broken pipe is reported as partial success.
- Windows support — current active product capture paths are Linux and macOS.
- Stitching benchmarks — no `rollshot-core` stitching path changes.

### Data flow

```text
CLI `ocr` / tray / hotkey
          │
          ▼
 screenshot-region overlay ──Esc──▶ Cancelled (no stdout, clipboard, feedback)
          │ CaptureResult<RgbaImage>
          ▼
 blocking OCR prepare ──Err──▶ typed privacy-safe failure
          │ Vec<OcrTextItem>
          ▼
 OrderedOcrItems + assemble ──empty──▶ EmptyResult
          │ non-empty String
          ▼
 clipboard write ──Err──▶ Clipboard error
          │
          ├──▶ stdout + "\n"
          └──▶ feedback only when daemon child requested it
```

### Daemon state

```text
Idle ──CaptureRegion(Region)──┐
Idle ──CaptureText(Text)──────┴──▶ Capturing(kind, id)
  ▲                                  │
  └──────── CaptureExited(id) ───────┘

While Capturing: either trigger is ignored.
Quit: terminate the child process group, then exit.
```

---

### Task 1: Extract the shared product OCR service

**Files:**
- Create: `crates/rollshot-app/src/product_ocr.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/result_workspace/ocr_text.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**
- Produces: `product_ocr::OcrItemId`, `product_ocr::OcrTextItem`, `product_ocr::OrderedOcrItems`, `product_ocr::ProductOcrError`, `product_ocr::prepare(&RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError>`, and `OrderedOcrItems::into_text() -> Result<String, ProductOcrError>`.
- Consumes: `rollshot_vision::RealAutomationHost`, `rollshot_automation::{AutomationHost, OcrQuery, Region}`, and existing tiling/duplicate-merging rules.
- The result workspace imports the shared item/error/prepare functions; its selection/redaction state remains in `result_workspace::ocr_text`.

- [ ] **Step 1: Add failing shared-service tests**

Create `product_ocr.rs` with the public data types and tests first. Use the existing `ImageRect`/`ImageQuad` fields so no conversion layer is introduced:

Register `mod product_ocr;` in `main.rs` in this test step so Cargo compiles the new module and the RED test cannot pass through an unmatched test filter.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OcrItemId(pub u64);

#[derive(Clone, PartialEq)]
pub struct OcrTextItem {
    pub id: OcrItemId,
    pub text: String,
    pub confidence: f32,
    pub bounds: rollshot_automation::ImageRect,
    pub quad: rollshot_automation::ImageQuad,
}

impl std::fmt::Debug for OcrTextItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrTextItem")
            .field("id", &self.id)
            .field("confidence", &self.confidence)
            .field("bounds", &self.bounds)
            .field("quad", &self.quad)
            .finish_non_exhaustive()
    }
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
        assert_eq!(OrderedOcrItems::new(items).into_text().unwrap(), "hello world\nsecond");
    }

    #[test]
    fn assemble_text_rejects_empty_or_whitespace_only_results() {
        assert_eq!(OrderedOcrItems::new(vec![]).into_text(), Err(ProductOcrError::EmptyResult));
        assert_eq!(
            OrderedOcrItems::new(vec![item(0, "   ", 0.0, 0.0)]).into_text(),
            Err(ProductOcrError::EmptyResult)
        );
    }

    #[test]
    fn debug_omits_recognized_text() {
        let item = item(0, "PRIVATE_OCR_SENTINEL", 0.0, 0.0);
        assert!(!format!("{item:?}").contains("PRIVATE_OCR_SENTINEL"));
    }
}
```

Move the existing test item helper or reproduce it with `ImageRect` and an axis-aligned `ImageQuad`. Add a `message()` arm for `EmptyResult` returning `No text was recognized in the selected region`.

- [ ] **Step 2: Run the focused test and verify failure**

Run: `rtk cargo test -p rollshot-app product_ocr::tests --no-default-features`

Expected: FAIL because `OrderedOcrItems` is not defined.

- [ ] **Step 3: Implement shared reading-order metadata and full-text assembly**

Move the existing `reading_order` and `same_line` rules into `product_ocr.rs`. `OrderedOcrItems` sorts once and owns the line-break metadata used by both quick OCR and `OcrTextDocument`:

```rust
pub struct OrderedOcrItems {
    items: Vec<OcrTextItem>,
    line_break_after: Vec<bool>,
}

impl OrderedOcrItems {
    pub fn new(mut items: Vec<OcrTextItem>) -> Self {
        items.sort_by(reading_order);
        let line_break_after = items.windows(2)
            .map(|pair| !same_line(pair[0].bounds, pair[1].bounds))
            .collect();
        Self { items, line_break_after }
    }

    pub fn as_parts(&self) -> (&[OcrTextItem], &[bool]) {
        (&self.items, &self.line_break_after)
    }

    pub fn into_parts(self) -> (Vec<OcrTextItem>, Vec<bool>) {
        (self.items, self.line_break_after)
    }

    pub fn into_text(self) -> Result<String, ProductOcrError> {
        let mut out = String::new();
        for (index, item) in self.items.iter().enumerate() {
            out.push_str(&item.text);
            if index + 1 < self.items.len() {
                out.push(if self.line_break_after[index] { '\n' } else { ' ' });
            }
        }
        let text = out.trim().to_owned();
        (!text.is_empty()).then_some(text).ok_or(ProductOcrError::EmptyResult)
    }
}
```

Update `OcrTextDocument::from_items` to filter redactions first, then call `OrderedOcrItems::new` and store its `into_parts()` result. This makes workspace copy-all and quick OCR share sorting and line-break decisions while preserving the existing rule that only the completed document is trimmed.

- [ ] **Step 4: Move OCR preparation without changing behavior**

Move `OcrItemId`, `OcrTile`, `vertical_tiles`, `merge_tile_items`, `iou`, their tiling/merge tests, and both cfg variants of `prepare_product_ocr` to `product_ocr.rs`. Rename the entry point to `prepare`. Keep `MAX_OCR_AREA`, 64-pixel tile overlap, 5,000-result limit, error mapping, and duplicate merge thresholds unchanged. Preserve `OcrItemId(items.len() as u64)` so result-workspace selection identity does not change. Extract `map_capability_error` and add unit tests for `ocr_session_init`, `ocr_detect`, and the invalid-region fallback without initializing a real OCR session.

In `result_workspace/ocr_text.rs`, import:

```rust
pub use crate::product_ocr::{OcrItemId, OcrTextItem, ProductOcrError};
use crate::product_ocr::OrderedOcrItems;
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
- Produces: `launch::OcrArgs`, `LaunchMode::Ocr { options, graphical_feedback }`, `quick_ocr::finish_with`, and `quick_ocr::run`.
- Consumes: `product_ocr::{prepare, OrderedOcrItems, ProductOcrError}` and a text-only clipboard boundary.
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

Register `mod quick_ocr;` in `main.rs` in this test step so the RED command compiles and runs these tests.

- [ ] **Step 5: Verify coordinator tests fail**

Run: `rtk cargo test -p rollshot-app quick_ocr::tests --no-default-features`

Expected: FAIL because `finish_with` and the production text clipboard adapter do not exist.

- [ ] **Step 6: Implement the coordinator and production clipboard adapter**

Add `set_text` beside the existing image clipboard function:

```rust
pub fn set_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("clipboard error: {error}"))?;
    clipboard.set_text(text)
        .map_err(|error| format!("clipboard write error: {error}"))
}
```

Implement `finish_with` by calling `OrderedOcrItems::new(items).into_text()` before `clipboard.set_text`. Implement `run(image)` by calling `product_ocr::prepare`, then `finish_with` using the arboard adapter. Define typed `QuickOcrError::{Ocr(ProductOcrError), Clipboard, Worker}` with privacy-safe `Display` strings; do not store recognized text or raw clipboard payloads in an error variant.

Route feature-disabled `LaunchMode::Ocr` to `Err(ProductOcrError::Disabled.message().into())`. Ensure the existing top-level error branch writes that privacy-safe message to stderr with intentional user-facing `eprintln!`; do not print successful text here yet, because platform handoff is Task 3.

Under `#[cfg(not(feature = "ocr"))]`, add a `main.rs` unit test that parses `ocr`, calls `run`, and asserts it returns `OCR is not available in this build` before any platform capture function is reached.

- [ ] **Step 7: Run CLI and coordinator tests**

Run: `rtk cargo test -p rollshot-app 'launch::tests::ocr' --no-default-features`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app quick_ocr::tests --no-default-features`

Expected: PASS without touching the system clipboard.

Run: `rtk cargo test -p rollshot-app tests::ocr_disabled_build_fails_before_capture --no-default-features`

Expected: PASS and assert the disabled error is explicit and privacy-safe.

- [ ] **Step 8: Commit the CLI and coordinator**

```bash
rtk git add crates/rollshot-app/src/image_clipboard.rs crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs crates/rollshot-app/src/quick_ocr.rs
rtk git commit -m "feat(ocr): add quick text capture command"
```

---

### Task 3: Route Linux and macOS capture completion into quick OCR

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/post_capture.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/quick_ocr.rs`

**Interfaces:**
- Produces: `CapturePurpose::{Present, Ocr { graphical_feedback }}` and platform capture runners accepting that purpose.
- Consumes: `quick_ocr::run(RgbaImage) -> Result<String, QuickOcrError>`.
- stdout is written only after both OCR and clipboard succeed.
- Produces internal side-effect ports `CliOutput` and `QuickOcrFeedback`, with production stdout/`notify-rust` adapters and deterministic fakes.

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

- [ ] **Step 3: Add failing output and feedback boundary tests**

Define the two narrow side-effect ports and tests before capture routing:

```rust
pub(crate) trait CliOutput {
    fn write_text(&mut self, text: &str) -> Result<(), String>;
}

pub(crate) trait QuickOcrFeedback {
    fn copied(&mut self) -> Result<(), String>;
    fn failed(&mut self, message: &str) -> Result<(), String>;
}
```

Test that successful output is exactly `"hello\n"`, the fake clipboard receives `"hello"`, direct CLI mode makes zero feedback calls, daemon-child mode makes one `copied` call, and OCR/clipboard failure writes no stdout. Add a broken-output fake and assert clipboard committed first, output failure returns nonzero-worthy `Err`, and success feedback is not emitted. Then implement `complete_cli_with`; production `complete_cli` supplies a locked stdout writer and a notification adapter.

- [ ] **Step 4: Run boundary tests and verify failure**

Run: `rtk cargo test -p rollshot-app quick_ocr::tests::completion --no-default-features`

Expected: FAIL because `complete_cli_with`, `CliOutput`, and `QuickOcrFeedback` do not exist.

- [ ] **Step 5: Implement Linux quick-OCR handoff**

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

Implement `complete_cli_with`; production `complete_cli` supplies a locked stdout writer and a notification adapter.

Add optional app dependency `notify-rust = { version = "4", optional = true }` and include `dep:notify-rust` in the existing `ocr` feature. Use `Notification::new().summary("Text copied").body("Recognized text is in the clipboard.").show()` for success. Notification delivery is best-effort and privacy-safe. For OCR/clipboard failure, try a notification containing only the typed public error message; if delivery fails, use `rfd::MessageDialog` as an error-only fallback. Cancellation never reaches this boundary.

- [ ] **Step 6: Add macOS state-transition tests**

Add `CapturePurpose` to `MacosProduct` state. Test that OCR completion does not create thumbnail/workspace state and that `QuickOcrFinished(Ok(text))` requests exit. Keep tests on pure helpers where macOS runtime construction is unavailable on Linux.

- [ ] **Step 7: Implement macOS background OCR in the existing iced daemon**

Add:

```rust
Message::QuickOcrFinished(Result<String, crate::quick_ocr::QuickOcrError>)
```

When `complete_capture` sees OCR purpose, close capture-owned windows, shut down the component, and return a `Task::perform` that uses the existing project pattern:

```rust
async move {
    tokio::task::spawn_blocking(move || quick_ocr::run(image))
        .await
        .map_err(|_| crate::quick_ocr::QuickOcrError::Worker)?
}
```

Map the result to `QuickOcrFinished`. In `update`, delegate stdout/feedback handling to the same tested completion boundary and return `iced::exit()`. A started blocking worker is not cancellable through iced, but the daemon's existing process-group termination remains the hard cancellation path.

Do not create a second iced event loop, a custom widget, a thumbnail, an auto-save, or a result workspace for OCR purpose.

- [ ] **Step 8: Run platform-neutral and feature builds**

Run: `rtk cargo test -p rollshot-app post_capture::tests --no-default-features`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app quick_ocr::tests::completion --no-default-features`

Expected: PASS, including stdout newline and feedback gating.

Run: `rtk cargo check -p rollshot-app --features ocr`

Expected: PASS with the OCR-enabled Linux path compiled. macOS compilation remains a required macOS verification in Task 6.

- [ ] **Step 9: Commit capture routing**

```bash
rtk git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/main.rs crates/rollshot-app/src/post_capture.rs crates/rollshot-app/src/macos_product.rs crates/rollshot-app/src/quick_ocr.rs
rtk git commit -m "feat(ocr): route region captures to clipboard"
```

---

### Task 4: Add typed daemon text-capture configuration and process launch

**Files:**
- Modify: `crates/rollshot-app/src/daemon/config.rs`
- Modify: `crates/rollshot-app/src/daemon/core.rs`
- Modify: `crates/rollshot-app/src/daemon/process.rs`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs`

**Interfaces:**
- Produces: `DaemonConfig::capture_text_hotkey: Option<Shortcut>`, `DaemonEvent::CaptureText`, and `CaptureKind::{Region, Text}`.
- Changes: `CaptureLauncher::launch(&mut self, id, kind, events)`.
- Process mapping: `Region` uses existing args; `Text` uses `ocr --graphical-feedback`.

- [ ] **Step 1: Write backward-compatible config tests**

Add tests for Linux/macOS defaults, a legacy file containing only `capture_region_hotkey`, independent override, and invalid text shortcut fallback. In `provider_config.rs`, extend the existing preservation test so saving provider settings retains both daemon shortcut keys byte-for-byte. Expected defaults are exact:

```rust
assert_eq!(linux.capture_text_hotkey.unwrap().to_string(), "Alt+Shift+7");
assert_eq!(macos.capture_text_hotkey.unwrap().to_string(), "Command+Shift+7");
```

Add a test where both fields are invalid and assert the eventual result contains two warnings in deterministic region-then-text order. Add a collision test asserting the region shortcut remains enabled, the text shortcut becomes `None`, and one privacy-safe warning is returned.

- [ ] **Step 2: Verify config tests fail**

Run: `rtk cargo test -p rollshot-app daemon::config::tests --no-default-features`

Expected: FAIL because `capture_text_hotkey` does not exist.

- [ ] **Step 3: Implement independent config fallback**

Change `RawDaemonConfig` fields to `Option<String>` so either field may be omitted. Change `LoadedConfig.warning: Option<String>` to `warnings: Vec<String>` and update both platform startup loops to emit each warning separately. Add optional `capture_text_hotkey` to `DaemonConfig`, set both platform defaults to `Some`, and parse each optional raw field independently. Preserve a valid configured region shortcut when text is invalid and vice versa. If both configured shortcuts resolve to the same normalized `Shortcut`, keep region, set text to `None`, and add a warning; the tray action remains available. Update existing config tests and provider-config fixtures only where compilation or exact serialized config requires the new field.

- [ ] **Step 4: Write daemon core and launcher tests**

Extend the fake launcher state to record kinds and assert:

```rust
core.handle(DaemonEvent::CaptureText);
assert_eq!(state.lock().unwrap().kinds, vec![CaptureKind::Text]);
```

Also start `CaptureRegion`, trigger `CaptureText`, and assert only one launch. Add process argument tests expecting:

```rust
assert_eq!(capture_args(CaptureKind::Region), &["capture", "--workflow", "screenshot", "--scope", "region"]);
assert_eq!(capture_args(CaptureKind::Text), &["ocr", "--graphical-feedback"]);
```

Use a slice return type because the two argument lists have different lengths.

- [ ] **Step 5: Verify daemon routing tests fail**

Run: `rtk cargo test -p rollshot-app daemon::core::tests --no-default-features`

Expected: FAIL because `CaptureKind::Text` and typed launcher routing do not exist.

Run: `rtk cargo test -p rollshot-app daemon::process::tests --no-default-features`

Expected: FAIL because text child arguments do not exist.

- [ ] **Step 6: Implement typed daemon launch routing**

Define `CaptureKind` beside `CaptureId`; add `kind` to the launcher trait. Route `CaptureRegion` and `CaptureText` through one private `start_capture(kind)` helper while preserving monotonically increasing IDs, active-child exclusion, stale exit handling, and process-group cleanup.

Do not register or emit `CaptureText` from non-OCR platform code yet; Task 5 gates those adapters with `#[cfg(feature = "ocr")]`.

- [ ] **Step 7: Run daemon domain tests**

Run: `rtk cargo test -p rollshot-app daemon::config::tests --no-default-features`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app daemon::core::tests --no-default-features`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app daemon::process::tests --no-default-features`

Expected: PASS.

- [ ] **Step 8: Commit daemon domain changes**

```bash
rtk git add crates/rollshot-app/src/daemon/config.rs crates/rollshot-app/src/daemon/core.rs crates/rollshot-app/src/daemon/process.rs crates/rollshot-app/src/daemon/mod.rs crates/rollshot-app/src/result_workspace/workbench/provider_config.rs
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
- Consumes: `DaemonEvent::CaptureText` and `DaemonConfig::capture_text_hotkey: Option<Shortcut>` from Task 4.
- Produces: feature-gated `Capture Text` tray action and two-ID shortcut routing on both platforms.
- Builds without `ocr` retain exactly the existing tray and shortcut behavior.

- [ ] **Step 1: Add failing Linux adapter tests**

Under `#[cfg(feature = "ocr")]`, assert that `activate_text` sends `CaptureText` and menu labels are `Capture Region`, `Capture Text`, `Quit Rollshot`. Write tests against this intended helper:

```rust
fn event_for_shortcut(id: &str) -> Option<DaemonEvent> {
    match id {
        "capture-region" => Some(DaemonEvent::CaptureRegion),
        "capture-text" => Some(DaemonEvent::CaptureText),
        _ => None,
    }
}
```

Test both IDs and both configured portal triggers. Add response-subset tests proving: region-only binding keeps the guard active and logs text degradation; text-only or empty binding fails startup because the established region shortcut is required.

- [ ] **Step 2: Verify Linux adapter tests fail**

Run: `rtk cargo test -p rollshot-app daemon::linux --features ocr`

Expected: FAIL because the text tray action and two-ID portal policy do not exist.

- [ ] **Step 3: Implement Linux portal and tray wiring**

Pass the required region shortcut and optional text shortcut into `ShortcutGuard::start`. Bind the available `NewShortcut` values in one portal session and route activation through `event_for_shortcut`. Treat a missing region ID in the portal response as startup failure. Treat a missing text ID as best-effort degradation: log a privacy-safe warning and keep the session/region shortcut alive. Add the tray item whenever the build has `ocr`, even when config collision or portal refusal disables the text hotkey. Keep portal startup best-effort and tray startup required.

- [ ] **Step 4: Add failing macOS adapter tests**

Add `TEXT_ID = "capture-text"`, test tray ID mapping, and test `Command+Shift+7` translates to `Modifiers::SUPER | Modifiers::SHIFT` plus `Code::Digit7`. Add a pure helper that maps registered hotkey IDs to daemon events and test both mappings. Add a manager seam or pure registration-policy helper proving text registration failure retains the registered region hotkey.

- [ ] **Step 5: Verify macOS adapter tests fail**

Run on macOS: `rtk cargo test -p rollshot-app daemon::macos --features ocr`

Expected: FAIL because the text tray ID, `Digit7` mapping, and partial registration policy do not exist.

- [ ] **Step 6: Implement macOS dual registration and tray wiring**

Change `ShortcutGuard` to own `Vec<HotKey>` and give the handler a separate `Vec<(u32, CaptureKind)>`, where `CaptureKind` is copyable and maps explicitly to the semantic event. Register region first and return an error if it fails. When an optional OCR text shortcut is present, attempt it separately; on failure log a warning and retain region. The process-global handler compares registered IDs and sends the corresponding semantic event. `Drop` clears the handler and unregisters every successfully owned hotkey.

Change `DaemonApp` to hold the cloned `DaemonConfig` and log the required region shortcut plus optional text shortcut without recognized text. Add the feature-gated tray item between capture and quit.

- [ ] **Step 7: Run both feature configurations on Linux**

Run: `rtk cargo test -p rollshot-app daemon --no-default-features`

Expected: PASS with legacy menu/shortcut behavior.

Run: `rtk cargo test -p rollshot-app daemon --features ocr`

Expected: PASS with `Capture Text`, dual portal binding, and text process routing tests.

- [ ] **Step 8: Commit platform daemon wiring**

```bash
rtk git add crates/rollshot-app/src/daemon/linux.rs crates/rollshot-app/src/daemon/linux/tray.rs crates/rollshot-app/src/daemon/linux/shortcut.rs crates/rollshot-app/src/daemon/macos.rs crates/rollshot-app/src/daemon/macos/tray.rs crates/rollshot-app/src/daemon/macos/shortcut.rs crates/rollshot-app/src/daemon/mod.rs
rtk git commit -m "feat(daemon): add OCR tray and shortcuts"
```

---

### Task 6: Privacy regression tests, documentation, and release verification

**Files:**
- Modify: `crates/rollshot-app/src/quick_ocr.rs`
- Modify: `crates/rollshot-app/src/diagnostics.rs`
- Modify: `.github/workflows/ci-ocr.yml`
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

- [ ] **Step 4: Expand the existing OCR CI lane**

In `.github/workflows/ci-ocr.yml`, replace the OCR clippy command with:

```yaml
- name: Clippy (ocr)
  run: cargo clippy -p rollshot-ocr -p rollshot-vision -p rollshot-app --features rollshot-app/ocr --all-targets -- -D warnings
```

The app feature also enables `rollshot-vision/ocr`. Replace the filtered `rollshot-app ... eval` test with:

```yaml
- name: Test rollshot-app (ocr)
  run: bash scripts/ci/run-ocr-test.sh cargo test -p rollshot-app --features ocr
```

Keep the existing Ubuntu/macOS matrix, model/runtime caches, and teardown wrapper. The pull-request path filter already includes `crates/rollshot-app/**`; this workflow edit also triggers itself.

- [ ] **Step 5: Run formatting and all app tests**

Run: `rtk cargo fmt --check`

Expected: PASS.

Run: `rtk cargo test -p rollshot-app --no-default-features`

Expected: PASS without building the OCR runtime.

Run: `rtk cargo test -p rollshot-app --features ocr`

Expected: PASS, including OCR service, CLI, daemon, and privacy tests.

- [ ] **Step 6: Run workspace lint verification**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS. This verifies the default workspace path; run `rtk cargo clippy -p rollshot-app --all-targets --features ocr -- -D warnings` separately for feature-gated code.

- [ ] **Step 7: Perform platform runtime verification**

On Linux, verify `Alt+Shift+6` is unchanged and `Alt+Shift+7` plus `Capture Text` select a region and copy text. On macOS, repeat with `Command+Shift+6` and `Command+Shift+7`. On both platforms verify CLI stdout matches clipboard contents, Esc is silent, empty selection preserves clipboard, only one overlay can be active, and success/failure feedback appears only for daemon-launched OCR.

Record any platform that could not be exercised in the final handoff, including the unchecked counterpart path and remaining runtime risk. No stitching benchmark is required.

- [ ] **Step 8: Commit documentation, CI, and verification tests**

```bash
rtk git add crates/rollshot-app/src/quick_ocr.rs crates/rollshot-app/src/diagnostics.rs .github/workflows/ci-ocr.yml README.md
rtk git commit -m "test(ocr): verify region text capture flow"
```

---

## Test Coverage Review

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|:---:|:---:|:---:|:---:|
| Task 1 / ordering, line breaks, trimming, empty result | ✓ | — | — | no |
| Task 1 / tiling, duplicate merge, OCR error mapping | ✓ | OCR feature build | — | real model quality |
| Task 1 / workspace copy semantics unchanged | ✓ | — | — | no |
| Task 2 / CLI flags, fixed region workflow, disabled build | ✓ | parse → `run` | — | no |
| Task 2 / non-empty-before-clipboard and clipboard failure | ✓ with fake | — | — | OS clipboard API |
| Task 3 / cancel dispatch and capture purpose | ✓ | Linux capture handoff | — | overlay interaction |
| Task 3 / stdout newline and feedback gating | ✓ with fakes | — | — | notification appearance |
| Task 3 / macOS background worker and exit transition | ✓ on macOS | macOS feature compile | — | real capture/OCR |
| Task 4 / defaults, legacy config, dual warnings, collision | ✓ | config load | — | no |
| Task 4 / typed child args, one-active-child invariant | ✓ | process spawn args | — | process-group signals |
| Task 5 / Linux portal response subsets and tray routing | ✓ | feature build | — | real portal grant UI |
| Task 5 / macOS partial registration and tray routing | ✓ on macOS | macOS CI | — | real global hotkey conflict |
| Task 6 / privacy-safe logs and errors | ✓ | capture-test subscriber | — | no |
| Task 6 / full OCR-enabled app suite | — | ✓ Linux + macOS CI | — | no |
| Complete user flow / shortcut → selection → clipboard | — | — | manual smoke | yes, both OSes |

## Production Failure Modes

| New codepath | Realistic failure | Test coverage | Planned handling | User-visible result |
|---|---|---|---|---|
| CLI parse/lowering | unsupported backend or irrelevant flag | Task 2 Steps 1–3 | clap rejects before capture | clear CLI error |
| Region overlay | Esc/cancel or backend permission denial | Task 3 Steps 1–2; existing overlay tests | `CaptureCompletion::Cancelled` or existing `OverlayError` | cancel silent; backend error on stderr/feedback |
| OCR worker | worker panic / runtime join failure | Task 3 Steps 6–8 | `QuickOcrError::Worker` | nonzero exit and privacy-safe failure feedback |
| OCR session | model/runtime initialization failure | Task 1 Step 4 | `ProductOcrError::SessionInit` | clear typed error, no clipboard call |
| OCR detection | detector failure or invalid region | Task 1 Step 4 | `Detect` / `InvalidRegion` | clear typed error, no clipboard call |
| Text assembly | zero or whitespace-only matches | Task 1 Steps 1–3 | `EmptyResult` | clear error, previous clipboard untouched |
| Clipboard open/write | clipboard owner unavailable or write rejected | Task 2 Steps 4–7 | `QuickOcrError::Clipboard`; no preparatory mutation | clear error; OS partial-write atomicity remains platform-controlled |
| stdout | broken pipe / write failure | Task 3 Steps 3–5 | output port returns error | nonzero exit; clipboard may already contain valid text |
| Desktop feedback | notification daemon absent / macOS permission denied | Task 3 Steps 3–5 | success logs and remains successful; failure uses error-dialog fallback | success may lack banner; failure remains visible |
| Config loading | missing, malformed, independently invalid, or colliding shortcuts | Task 4 Steps 1–3 | defaults plus `Vec<String>` warnings; collision disables text hotkey only | tray remains; warning in diagnostics |
| Linux shortcut bind | portal returns subset or closes activation stream | Task 5 Steps 1–3 | require region; degrade missing text; existing close/error handling | tray works; region preserved where bound |
| macOS hotkey register | text key conflicts with another app | Task 5 Steps 4–6 | retain region registration, omit text registration | tray works; warning logged |
| Child lifecycle | second trigger or stale child exit | Task 4 Steps 4–7 | existing active guard and monotonic ID | trigger ignored; no overlapping overlay |
| OCR CI | model/runtime cache miss or known macOS teardown abort | existing workflow + Task 6 Step 4 | existing provisioning and `run-ocr-test.sh` policy | CI failure unless known post-pass abort |

No failure mode is untested, unhandled, and silent. Best-effort success notification delivery and arbitrary-format clipboard atomicity are explicitly bounded above.

## Performance and Resource Review

- OCR runs once per selected screenshot, never per frame; this does not enter stitching hot loops.
- `VisualIndex::build(image.clone())` retains the existing extra image allocation. The selected image is bounded by the captured display region; avoid adding another clone in `quick_ocr` or the macOS task handoff.
- Only one daemon capture/OCR child can run, so the blocking pool cannot accumulate multiple OCR jobs through daemon triggers.
- `spawn_blocking` work cannot be aborted once started. Interactive completion waits for it; daemon quit uses the existing child process-group termination boundary.
- Linux portal session cleanup remains in `ShortcutGuard::Drop` and the existing close timeout. macOS unregisters every successfully registered hotkey in `Drop`.
- Notification failure is best-effort and must not retain OCR text or keep the child alive waiting for user action except for the error-only fallback dialog.
- No new unbounded channel, frame buffer, persistent image cache, or disk I/O is introduced.

## Execution Strategy

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1 | `rollshot-app/product_ocr`, `result_workspace` | — |
| Task 2 | `rollshot-app/launch`, `quick_ocr`, clipboard | Task 1 |
| Task 3 | `rollshot-app` platform capture handoff | Tasks 1–2 |
| Task 4 | `rollshot-app/daemon` domain/process | — logically, but same crate/branch |
| Task 5 | `rollshot-app/daemon` platform adapters | Task 4 |
| Task 6 | diagnostics, CI, README | Tasks 1–5 |

Sequential execution, no parallelization opportunity in this workspace. Tasks 1–3 and 4–5 are logically separate lanes, but the repository forbids unrequested worktrees, all agents share one working tree, and both lanes modify `rollshot-app`; running them concurrently would create staging and commit hazards. Execute `1 → 2 → 3 → 4 → 5 → 6`.
