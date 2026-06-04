# No-Tray Focus Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a no-tray escape path for active scrolling capture by upgrading the existing iced capture chrome with explicit `Finish` / `Cancel` controls while preserving focused `Esc` behavior.

**Architecture:** Keep the change inside `crates/rollshot-iced-overlay`. Add a capture-phase finish message so the new `Finish` button can finalize after `crop_confirmed = true` without weakening the existing selection-phase empty-crop validation. Reuse the existing outside-crop chrome placement and input-region machinery; do not introduce a new toolbar subsystem.

**Tech Stack:** Rust, iced, iced layer-shell, existing `rollshot-iced-overlay` shared app state, existing Linux/macOS runners.

---

## File Structure

- Modify `crates/rollshot-iced-overlay/src/app.rs`
  - Add a capture-phase finish message.
  - Keep `OverlayMessage::Finish` semantics for selection-phase validation.
  - Add a small helper for the confirmed-crop control strip.
  - Replace the current text-only capture chrome with that helper.
  - Add focused unit tests for message/effect behavior and toolbar sizing constants.
- Inspect `crates/rollshot-iced-overlay/src/linux_runner.rs`
  - No behavior rewrite.
  - The existing `toolbar_input_rect(...)` call remains the Linux input-region source of truth.
- Inspect `crates/rollshot-iced-overlay/src/macos_runner.rs`
  - No behavior rewrite in the first pass. The shared UI controls should work through the existing iced message path.
  - Leave platform-specific focus restoration out unless tests/manual verification show the control strip cannot receive clicks.

Important constraint: the existing capture chrome is already a toolbar-like strip. Do not add a second independent toolbar. Upgrade the existing `Capturing - scroll...` chrome in place.

## Task 1: Add Capture-Phase Finish Message

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/app.rs`

- [ ] **Step 1: Add failing tests for confirmed-crop finish and existing selection validation**

Append these tests inside the existing `#[cfg(test)] mod tests` in `crates/rollshot-iced-overlay/src/app.rs`:

```rust
    #[test]
    fn finish_capture_control_finishes_after_crop_is_confirmed() {
        let mut state = OverlayState {
            crop: Some(Rectangle {
                x: 10.0,
                y: 20.0,
                width: 120.0,
                height: 80.0,
            }),
            crop_confirmed: true,
            ..OverlayState::default()
        };

        let effect = super::update(&mut state, OverlayMessage::FinishCapture);

        assert_eq!(effect, super::OverlayEffect::Finish);
    }

    #[test]
    fn selection_finish_still_validates_empty_crop() {
        let mut state = OverlayState::default();

        let effect = super::update(&mut state, OverlayMessage::Finish);

        assert_eq!(effect, super::OverlayEffect::None);
        assert!(state.warning().is_some());
    }
```

The second test intentionally overlaps the existing `finish_without_crop_requests_warning_not_effect` behavior. If both tests are present, keep both or replace the older one with the clearer new name in the implementation commit.

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay finish_capture_control_finishes_after_crop_is_confirmed
```

Expected: FAIL to compile because `OverlayMessage::FinishCapture` does not exist.

- [ ] **Step 3: Add the new message variant**

In `OverlayMessage`, add `FinishCapture` immediately after `Finish`:

```rust
pub(crate) enum OverlayMessage {
    IcedEvent(iced::Event),
    WindowOpened { id: window::Id, size: Size },
    Finish,
    FinishCapture,
    Cancel,
    LiveEvent(crate::driver::LiveOverlayEvent),
    Tick,
}
```

- [ ] **Step 4: Handle the capture-phase finish message**

In `update(...)`, add this match arm before the existing `OverlayMessage::Finish => { ... }` arm:

```rust
        OverlayMessage::FinishCapture => {
            if state.crop_confirmed {
                OverlayEffect::Finish
            } else {
                OverlayEffect::None
            }
        }
```

Do not change the existing `OverlayMessage::Finish` arm in this task. It protects the selection phase by warning on empty crop and beginning stitching only after a valid crop.

- [ ] **Step 5: Run the focused tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay finish_capture_control_finishes_after_crop_is_confirmed selection_finish_still_validates_empty_crop
```

Expected: PASS for both tests.

- [ ] **Step 6: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-iced-overlay/src/app.rs
rtk git commit -m "feat(overlay): add capture finish action"
```

## Task 2: Upgrade Existing Capture Chrome In Place

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/app.rs`

- [ ] **Step 1: Add constants for the capture control copy**

Near the existing toolbar constants in `app.rs`, add:

```rust
const CAPTURE_STATUS_TEXT: &str = "Capturing - scroll the target";
#[allow(dead_code)]
const FOCUS_PAUSED_TEXT: &str = "Shortcuts paused - click Rollshot controls to restore Esc";
const FINISH_LABEL: &str = "Finish";
const CANCEL_LABEL: &str = "Cancel";
```

Keep `FOCUS_PAUSED_TEXT` even if focus detection is not implemented in this first pass. It documents the approved copy and prevents drift when focus detection is added later.

- [ ] **Step 2: Add a helper for the confirmed-crop control strip**

Place this helper near `magenta_toolbar(...)`:

```rust
pub(crate) fn capture_control_strip<'a>() -> Element<'a, OverlayMessage> {
    magenta_toolbar(
        row![
            text(CAPTURE_STATUS_TEXT).size(16),
            button(FINISH_LABEL).on_press(OverlayMessage::FinishCapture),
            button(CANCEL_LABEL).on_press(OverlayMessage::Cancel),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into(),
    )
}
```

This is the existing chrome upgraded in place. Do not create a new overlay layer, window, panel, or toolbar module.

- [ ] **Step 3: Replace the text-only capture toolbar**

In the `if state.crop_confirmed { ... }` branch of `view(...)`, replace:

```rust
        let toolbar = magenta_toolbar(
            text("Capturing — scroll the target, Esc to finish")
                .size(16)
                .into(),
        );
```

with:

```rust
        let toolbar = capture_control_strip();
```

Leave the rest of the `chrome` composition intact:

```rust
        let chrome: Element<'_, OverlayMessage> = {
            let mut col = column![toolbar];
            col = col.spacing(CHROME_SPACING);
            if let Some(w) = warning {
                col = col.push(w);
            }
            if let Some(handle) = &state.preview {
                col = col.push(image(handle.clone()));
            }
            col.into()
        };
```

This preserves the existing toolbar-first contract that `toolbar_input_rect(...)` relies on.

- [ ] **Step 4: Add a low-risk copy regression test**

Inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn capture_control_copy_matches_spec() {
        assert_eq!(super::CAPTURE_STATUS_TEXT, "Capturing - scroll the target");
        assert_eq!(
            super::FOCUS_PAUSED_TEXT,
            "Shortcuts paused - click Rollshot controls to restore Esc"
        );
        assert_eq!(super::FINISH_LABEL, "Finish");
        assert_eq!(super::CANCEL_LABEL, "Cancel");
    }
```

This test does not inspect iced internals. It protects the approved overlay copy and keeps the task small.

- [ ] **Step 5: Run the focused tests**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay capture_control_copy_matches_spec finish_capture_control_finishes_after_crop_is_confirmed
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-iced-overlay/src/app.rs
rtk git commit -m "feat(overlay): add capture control strip"
```

## Task 3: Keep Input Region Aligned With The Upgraded Strip

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Inspect: `crates/rollshot-iced-overlay/src/linux_runner.rs`

- [ ] **Step 1: Increase toolbar input width for the new controls**

The existing capture input region uses `TOOLBAR_W = 300.0` and `TOOLBAR_H = 50.0`. The new strip contains status text plus two buttons. Update `TOOLBAR_W` conservatively to avoid clipping the clickable controls:

```rust
const TOOLBAR_W: f32 = 360.0;
const TOOLBAR_H: f32 = 50.0;
```

Do not change `TOOLBAR_H` unless manual testing shows the strip is vertically clipped. Keeping height stable reduces risk for crop-adjacent chrome placement.

- [ ] **Step 2: Add a test for the wider input region**

Update or add a unit test in `app.rs`:

```rust
    #[test]
    fn toolbar_input_rect_uses_control_strip_width() {
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 200.0,
        };
        let window = Size::new(800.0, 600.0);

        let rect = toolbar_input_rect(crop, window).expect("toolbar input rect");

        assert_eq!(rect.2, 360);
        assert!(rect.3 > 0);
    }
```

If the chosen chrome band for these dimensions is not bottom in practice, keep the assertion focused on width (`rect.2`) and positive height (`rect.3`), as shown.

- [ ] **Step 3: Verify Linux runner still uses the shared rect**

Inspect `crates/rollshot-iced-overlay/src/linux_runner.rs` and confirm `BeginStitch` still calls:

```rust
let Some((x, y, w, h)) = app::toolbar_input_rect(crop, ws) else {
    return Task::none();
};
Task::done(Message::SetInputRegion(ActionCallback::new(
    move |region| {
        region.add(x, y, w, h);
    },
)))
```

Do not duplicate the sizing constants into the runner. The app-level helper stays the source of truth.

- [ ] **Step 4: Run focused placement/input tests**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay toolbar_input_rect
```

Expected: PASS for all `toolbar_input_rect...` tests.

- [ ] **Step 5: Run all iced overlay tests**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
```

Expected: PASS.

- [ ] **Step 6: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-iced-overlay/src/app.rs crates/rollshot-iced-overlay/src/linux_runner.rs
rtk git commit -m "fix(overlay): size capture controls input region"
```

If `linux_runner.rs` was not modified, omit it from `git add`.

## Task 4: Manual Runtime Verification Notes

**Files:**
- No planned file edits.

- [ ] **Step 1: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS. If it fails only because the edited files need formatting, run `rtk cargo fmt`, then rerun `rtk cargo fmt --check`.

- [ ] **Step 2: Run the package test suite**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
```

Expected: PASS.

- [ ] **Step 3: Run broader tests if runner behavior changed**

If any code outside `crates/rollshot-iced-overlay/src/app.rs` changed, run:

```bash
rtk cargo test
```

Expected: PASS. If this is too slow for the current environment, record the reason in the final handoff.

- [ ] **Step 4: Manual Linux check**

On a Linux Wayland session:

```bash
rtk cargo run -p rollshot-iced-overlay --bin capture_overlay
```

Manual expected behavior:

- Select a crop.
- Confirm the capture phase shows the existing chrome upgraded with `Capturing - scroll the target`, `Finish`, and `Cancel`.
- Click/scroll the target window.
- Click `Finish`; capture finalizes and exits.
- Repeat and click `Cancel`; capture exits without a result.
- Repeat and press `Esc` while overlay shortcuts still work; capture finalizes as before.

- [ ] **Step 5: Manual macOS check**

On macOS:

```bash
rtk cargo run -p rollshot-iced-overlay --bin capture_overlay
```

Manual expected behavior:

- Select a crop.
- Confirm the control strip is outside the crop.
- Click/scroll the target window.
- Click `Finish`; capture finalizes and exits.
- Repeat and click `Cancel`; capture exits without a result.

## Self-Review Checklist

- Spec coverage: Tasks implement the no-tray explicit controls, preserve `Esc`, avoid global hooks/tray, and keep controls in the existing outside-crop chrome.
- Existing toolbar caution: Task 2 upgrades the current capture chrome in place and preserves the toolbar-first composition used by `toolbar_input_rect(...)`.
- Platform split: Linux input region remains centralized in `toolbar_input_rect(...)`; macOS gets the shared controls without new platform hooks.
- Placeholder scan: complete; all steps have concrete code, commands, and expected outcomes.
- Type consistency: the new message is consistently named `OverlayMessage::FinishCapture`.
