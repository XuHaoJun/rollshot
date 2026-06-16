# Action Guide P0b — Platform Semantic Input + App Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the two platform semantic-input crates — `rollshot-linux-input` (evdev, safe) and `rollshot-macos-input` (CoreGraphics `CGEventTap`, unsafe-isolation) — each implementing the existing `rollshot_action::SemanticInputSource` trait, and wire them into `rollshot-app` behind an `action-guide` Cargo feature so a recording session can upgrade from `VisualOnly` to `SemanticEvents` (and degrade back with a privacy-safe advisory) — all without changing `rollshot-action`.

**Architecture:** Each platform crate is split into a **pure classification core** (a native event reduced to `type/code/value` → `Option<SemanticAction>`) that is fixture-testable on every CI host, plus a **thin native glue** (the evdev reader thread / the `CGEventTap` run-loop thread) that converts real native events to that core's input and pushes privacy-filtered `TimedSemanticAction`s into a shared queue drained by `poll()`. Privacy is enforced by construction: the glue only ever forwards what the core emits — never raw key codes, typed text, device names, or device paths. The app gains a `#[cfg]`-selected **factory** returning `Box<dyn SemanticInputSource>` and an **`ActionInputSession`** controller that owns `start`/`poll`/`stop`, performs the `Err(reason) → VisualOnlySource{reason}` fallback, reports `InputCapability`, and forwards events into `ActionRecorder::ingest_event`. A feature-gated `action-guide` CLI subcommand provides a reachable, manually-verifiable input-capability probe entry.

**Tech Stack:** Rust (workspace crates, edition 2021); `evdev = "0.12"` (Linux, target-gated, no unsafe); `objc2-core-graphics = "0.3"` + `objc2-core-foundation = "0.3"` (macOS, target-gated, unsafe FFI isolated); `thiserror`, `tracing`; `clap` (CLI); `rtk cargo test` / `fmt --check` / `clippy -D warnings`.

**Spec:** `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md` (§Implementation Increments P0b, §Platform Input Sources, §`rollshot-macos-input`, §`rollshot-linux-input`, §Privacy And Security, §Platform Input Tests).

**Predecessor:** `docs/superpowers/plans/2026-06-15-action-guide-p0a-rollshot-action.md` (the `rollshot-action` crate — landed in #46). The `SemanticInputSource` trait, `VisualOnlySource`, and all model types this plan builds on already exist there.

---

## Scope Boundary (read first — this is the one non-obvious decision)

The spec describes P0b as "the platform input crates **wired into the app** so detection upgrades from `VisualOnly` to `SemanticEvents`." That assumed the **P0a app-integration increment** (toolbar `ActionGuide` entry, `Workflow::ActionGuide`, the recording controls, the frame-reader thread, the `SendFrameStream` lift, and the Action Guide Timeline Workspace) already existed. **It does not.** Only the `rollshot-action` crate (P0a "Plan 1") landed; the app-integration ("Plan 2") was never written or executed. Verified facts:

- `Workflow` (`crates/rollshot-capture/src/types.rs:43`) has only `Screenshot` and `Scrolling`.
- No file under `crates/rollshot-app/` references `rollshot-action`.
- `SendStream` / `unsafe impl Send` still lives only in `crates/rollshot-iced-overlay/src/driver.rs:106-123`.

Therefore this plan **delivers the input half end-to-end** and **explicitly defers the capture/detection/Timeline-Workspace half** to the future app-integration plan:

| In scope for P0b | Deferred to the app-integration plan |
|------------------|--------------------------------------|
| `rollshot-linux-input` crate (evdev) implementing `SemanticInputSource` | `Workflow::ActionGuide` variant + overlay routing + `is_supported()` rejection |
| `rollshot-macos-input` crate (`CGEventTap`) implementing `SemanticInputSource` | `SendFrameStream` lift into `rollshot-capture` + frame-reader thread |
| App factory `create_input_source()` (`#[cfg]`-selected) | Region-only overlay result path (no Stitcher) |
| `ActionInputSession` controller (start/poll/stop, fallback, capability) | Recording controls UI + `Detecting…` state |
| Platform-specific degraded advisory strings | Action Guide Timeline Workspace + export UI |
| `action-guide` feature + reachable CLI probe entry | Toolbar `ToolbarAction::ActionGuide` button |
| CI feature-on lane (build/clippy/test on Linux + macOS) | |
| README evdev-ACL + macOS Input Monitoring docs | |

The `ActionInputSession` and factory built here are **exactly** what the app-integration plan will call from its recording lifecycle — the seam is reusable, not throwaway. The CLI probe is the reachable host that makes the wiring non-dead-code under `-D warnings`, doubles as the spec's manual-verification entry ("watch it report Semantic vs Visual-only"), and is replaced by the real overlay flow later.

**Trait limitation (documented, not fixed here):** `SemanticInputSource::poll(&mut self) -> Vec<TimedSemanticAction>` cannot report a *mid-session* runtime failure (it returns no error). So `DegradedReason::RuntimeFailure` is observable only at `start()` time in P0b; a reader thread that dies mid-session is logged via `tracing` and simply stops yielding events. Surfacing a live capability downgrade needs a trait change, which the spec forbids in P0b ("no change to `rollshot-action`"). The app-integration plan owns that follow-up.

---

## Key Design Decisions (read before starting)

1. **Pure core + thin glue, in both crates.** The unit-testable surface is a pure function over a minimal native-event reduction (`RawEvdevEvent { ev_type: u16, code: u16, value: i32 }` on Linux; `RawCgEvent { kind, button, keycode }` on macOS). All the spec's "Platform Input Tests" classification cases test this core and run on **every** CI host. The native reader/run-loop glue is the only platform-gated, manually-verified code.

2. **Linux stays `unsafe_code = "forbid"`.** The `evdev` crate performs the ioctls internally, so `rollshot-linux-input` needs no `unsafe`. Only `rollshot-macos-input` opts out of the workspace forbid (local `[lints.rust] unsafe_code = "allow"`), exactly mirroring `rollshot-macos-oneshot`.

3. **Both crates depend on `rollshot-action`, never the reverse.** They import the trait + `SemanticAction`/`TimedSemanticAction`/`CaptureRegion`/`InputCapability`/`DegradedReason`. `rollshot-action` gains no dependency on them. No cycle.

4. **Each crate compiles on the "wrong" OS as a stub** (returning `DegradedReason::SourceStartFailed`), because `cargo test --workspace` on the macOS CI host still builds `rollshot-linux-input` and vice-versa. Native deps are target-gated; native code is `#[cfg(target_os = …)]`-gated; the else-branch is a safe stub. This mirrors `rollshot-macos-oneshot`'s non-macOS stub.

5. **Events are timestamped by the app, not the source.** The spec's `Millis` is "ms since recording start." Platform sources don't know the recording epoch, so the trait gives them no clock. Decision: the source stamps each action with a **monotonic ms since its own `start()`** using `std::time::Instant` captured at `start()`. The app's recorder already works in its own `Millis`; P0b's probe entry uses the source-relative stamp directly (good enough for the probe and for the eventual app, which can offset by the recording-start delta). This keeps the core pure (the core does not stamp; the glue does) and avoids `SystemTime`.

6. **No absolute click position on either platform in P0b.** Linux evdev gives no compositor-space pointer position without extra machinery (spec: Linux normally emits `None`). macOS *could* read `CGEventGetLocation`, but the spec says "no required rule may assume it exists" and P0a's detector ignores it. Decision: both sources emit `Click { position: None }` in P0b to keep parity and avoid coordinate persistence. (macOS position is a deferred enhancement.)

7. **Input is observed only between `start()` and `stop()`, enforced by construction (privacy).** Reader/run-loop threads must not keep observing after `stop()`. Linux reads are **non-blocking** with a short poll-sleep so the reader loop checks the `stop` flag and exits within ~15 ms; `stop()` **joins** the threads (it does not detach them). macOS `stop()` stops the CFRunLoop and joins the tap thread. Both sources bound their internal queue (`MAX_QUEUED = 4096`, drop-oldest) so a stalled consumer can never grow memory without bound — matching the spec's "explicit fixed bounds" ethos. Both `start(region)` params are accepted for the trait contract but unused in P0b (evdev is global; the tap is global) — region-scoped filtering is a deferred enhancement.

---

## Interface Contract (locked — every task must match these signatures)

```rust
// ===== rollshot-linux-input =====
pub use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticAction, SemanticInputSource,
    TimedSemanticAction,
};

// classify.rs (pure, host-agnostic)
pub struct RawEvdevEvent { pub ev_type: u16, pub code: u16, pub value: i32 }
pub struct EvdevClassifier { /* typing-run state is not needed; each event maps independently */ }
impl EvdevClassifier {
    pub fn new() -> Self;
    /// Map one raw evdev event to a semantic action, or `None` to ignore it.
    pub fn classify(&mut self, ev: RawEvdevEvent) -> Option<SemanticAction>;
}

// source.rs
pub struct EvdevInputSource { /* threads + shared queue + start instant */ }
impl EvdevInputSource { pub fn new() -> Self; }
impl Default for EvdevInputSource { fn default() -> Self; }
impl SemanticInputSource for EvdevInputSource { /* start/poll/stop */ }

// ===== rollshot-macos-input =====
pub use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticAction, SemanticInputSource,
    TimedSemanticAction, MouseButton, SemanticKey,
};

// classify.rs (pure, host-agnostic)
pub enum RawCgKind { LeftMouseDown, RightMouseDown, OtherMouseDown, ScrollWheel, KeyDown, Other }
pub struct RawCgEvent { pub kind: RawCgKind, pub button_number: i64, pub keycode: i64 }
pub fn classify_cg(ev: RawCgEvent) -> Option<SemanticAction>;

// permission.rs (macOS-gated; pure stub elsewhere)
pub enum InputMonitoringStatus { Granted, Denied, NotDetermined }
pub fn input_monitoring_status() -> InputMonitoringStatus;
pub fn request_input_monitoring() -> InputMonitoringStatus;
pub fn open_input_monitoring_settings();

// source.rs
pub struct MacosInputSource { /* tap thread + shared queue + start instant */ }
impl MacosInputSource { pub fn new() -> Self; }
impl Default for MacosInputSource { fn default() -> Self; }
impl SemanticInputSource for MacosInputSource { /* start/poll/stop */ }

// ===== rollshot-app (behind feature = "action-guide") =====
// action_input.rs
pub fn create_input_source() -> Box<dyn rollshot_action::SemanticInputSource>;
pub fn degraded_advisory(reason: rollshot_action::DegradedReason) -> &'static str;
pub struct ActionInputSession { /* boxed source + capability */ }
impl ActionInputSession {
    pub fn new(source: Box<dyn rollshot_action::SemanticInputSource>) -> Self;
    /// Start observing; on the source's `Err(reason)`, swap to a started
    /// `VisualOnlySource{reason}` and report `VisualOnly{reason}`.
    pub fn start(&mut self, region: rollshot_action::CaptureRegion) -> rollshot_action::InputCapability;
    pub fn capability(&self) -> rollshot_action::InputCapability;
    /// Drain the source and forward each action into the recorder.
    pub fn poll_into(&mut self, recorder: &mut rollshot_action::ActionRecorder);
    pub fn stop(&mut self);
}
```

---

## File Structure

```
crates/rollshot-linux-input/
  Cargo.toml          # target-gated evdev dep; workspace lints (unsafe forbid)
  src/lib.rs          # crate doc + module decls + re-exports + non-linux stub gating
  src/classify.rs     # RawEvdevEvent + EvdevClassifier (pure, tested everywhere)
  src/source.rs       # EvdevInputSource: device discovery, reader threads, queue

crates/rollshot-macos-input/
  Cargo.toml          # target-gated objc2-core-graphics/foundation; [lints.rust] unsafe_code="allow"
  src/lib.rs          # crate doc + module decls + re-exports + non-macos stub gating
  src/classify.rs     # RawCgEvent + classify_cg (pure, tested everywhere)
  src/permission.rs   # Input Monitoring status/request/open-settings (macos impl + stub)
  src/source.rs       # MacosInputSource: CGEventTap, CFRunLoop thread, queue (unsafe, isolated)

crates/rollshot-app/
  src/action_input.rs # NEW (feature="action-guide"): factory + ActionInputSession + advisory
  src/main.rs         # MODIFY: feature-gated ActionGuideProbe launch arm
  src/launch.rs       # MODIFY: feature-gated parse of the probe launch mode
  Cargo.toml          # MODIFY: action-guide feature + target/feature-gated deps

crates/rollshot-cli/
  src/args.rs         # MODIFY: feature-gated `action-guide` subcommand
  src/lib.rs          # MODIFY: feature-gated routing arm
  src/cmd_action_guide.rs # NEW (feature="action-guide"): launch the app probe
  Cargo.toml          # MODIFY: action-guide feature

Cargo.toml            # MODIFY: add the two crates to workspace members
.github/workflows/ci.yml # MODIFY: feature-on lane + macOS check list
README.md             # MODIFY: evdev ACL setup + macOS Input Monitoring sections
```

---

# Part 1 — `rollshot-linux-input` (evdev, safe)

## Task 1: Scaffold `rollshot-linux-input`

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/rollshot-linux-input/Cargo.toml`
- Create: `crates/rollshot-linux-input/src/lib.rs`
- Create: `crates/rollshot-linux-input/src/classify.rs` (placeholder; filled in Task 2)
- Create: `crates/rollshot-linux-input/src/source.rs` (placeholder; filled in Task 3)

- [ ] **Step 1: Add the crate to workspace members**

In `/home/noah/rollshot/Cargo.toml`, add both new crates to the `members` array (group with the other `crates/*` entries):

```toml
    "crates/rollshot-action",
    "crates/rollshot-linux-input",
    "crates/rollshot-macos-input",
```

- [ ] **Step 2: Create the manifest**

`crates/rollshot-linux-input/Cargo.toml` — `evdev` is target-gated so the crate still builds on macOS CI as a stub:

```toml
[package]
name = "rollshot-linux-input"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
rollshot-action = { path = "../rollshot-action" }
tracing = { workspace = true }

[target.'cfg(target_os = "linux")'.dependencies]
evdev = "0.12"

[lints]
workspace = true
```

- [ ] **Step 3: Create `src/lib.rs` with the responsibility doc and module decls**

```rust
//! Linux semantic-input source for Action Guide. Observes global input through
//! read-only evdev access to `/dev/input/event*` (works under KDE Wayland
//! because it reads kernel input devices, not a compositor API). Exposes only
//! privacy-filtered semantic actions and explicit startup/runtime failure
//! reasons — never device paths, device names, or raw key codes. Implements
//! `rollshot_action::SemanticInputSource`. On non-Linux hosts the source is a
//! stub that reports `DegradedReason::SourceStartFailed` so the crate still
//! compiles in the workspace build. See
//! `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

mod classify;
mod source;

pub use classify::{EvdevClassifier, RawEvdevEvent};
pub use source::EvdevInputSource;

pub use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticAction, SemanticInputSource,
    TimedSemanticAction,
};
```

- [ ] **Step 4: Add placeholder module files so the crate compiles**

Create `crates/rollshot-linux-input/src/classify.rs` and `crates/rollshot-linux-input/src/source.rs` each containing only a doc comment for now; the next tasks fill them. To keep this step compiling, put a temporary minimal body:

`src/classify.rs`:
```rust
//! Pure evdev-event classification (filled in Task 2).
```

`src/source.rs`:
```rust
//! `EvdevInputSource` (filled in Task 3).
```

Because `lib.rs` re-exports `EvdevClassifier`, `RawEvdevEvent`, and `EvdevInputSource`, this step will NOT compile yet — that is expected. Do not run the build here; Task 2 adds `classify` items and Task 3 adds `source` items. To keep each commit green, **fold Task 1's commit into Task 2** (commit after Task 2 compiles). Leave the re-exports in place.

> Rationale: re-exporting symbols that don't exist yet would fail compilation. Rather than churn `lib.rs` twice, we accept that Task 1 alone doesn't build and commit at the end of Task 2.

---

## Task 2: Pure evdev classification core

**Files:**
- Create/replace: `crates/rollshot-linux-input/src/classify.rs`
- Test: same file (`#[cfg(test)]`)

These constants come from the Linux `input-event-codes.h` ABI (stable kernel UAPI). Classification is independent per event (clicks/scroll/keys map directly; ordinary keys collapse to `TypingActivity`; Enter/Tab become semantic keys; key *releases*, autorepeat, mouse movement, and sync are ignored).

- [ ] **Step 1: Write the failing classifier tests**

Replace `src/classify.rs` with the test block first (implementation added in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

    fn ev(t: u16, c: u16, v: i32) -> RawEvdevEvent {
        RawEvdevEvent { ev_type: t, code: c, value: v }
    }

    #[test]
    fn left_button_press_is_a_left_click_with_no_position() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_KEY, BTN_LEFT, 1)),
            Some(SemanticAction::Click { button: MouseButton::Left, position: None })
        );
    }

    #[test]
    fn right_and_middle_buttons_map_to_their_buttons() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_KEY, BTN_RIGHT, 1)),
            Some(SemanticAction::Click { button: MouseButton::Right, position: None })
        );
        assert_eq!(
            c.classify(ev(EV_KEY, BTN_MIDDLE, 1)),
            Some(SemanticAction::Click { button: MouseButton::Middle, position: None })
        );
    }

    #[test]
    fn button_release_and_autorepeat_are_ignored() {
        let mut c = EvdevClassifier::new();
        assert_eq!(c.classify(ev(EV_KEY, BTN_LEFT, 0)), None); // release
        assert_eq!(c.classify(ev(EV_KEY, KEY_A, 2)), None); // autorepeat
    }

    #[test]
    fn wheel_and_hwheel_map_to_scroll_activity() {
        let mut c = EvdevClassifier::new();
        assert_eq!(c.classify(ev(EV_REL, REL_WHEEL, 1)), Some(SemanticAction::ScrollActivity));
        assert_eq!(c.classify(ev(EV_REL, REL_HWHEEL, -1)), Some(SemanticAction::ScrollActivity));
    }

    #[test]
    fn pointer_motion_and_sync_never_create_actions() {
        let mut c = EvdevClassifier::new();
        assert_eq!(c.classify(ev(EV_REL, REL_X, 5)), None);
        assert_eq!(c.classify(ev(EV_REL, REL_Y, -3)), None);
        assert_eq!(c.classify(ev(EV_SYN, 0, 0)), None);
    }

    #[test]
    fn enter_and_tab_press_are_semantic_keys() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_KEY, KEY_ENTER, 1)),
            Some(SemanticAction::SemanticKey(SemanticKey::Enter))
        );
        assert_eq!(
            c.classify(ev(EV_KEY, KEY_TAB, 1)),
            Some(SemanticAction::SemanticKey(SemanticKey::Tab))
        );
    }

    #[test]
    fn ordinary_key_press_collapses_to_typing_activity_never_a_code() {
        let mut c = EvdevClassifier::new();
        // A letter, a digit, and space all collapse to TypingActivity — the
        // raw code is never surfaced (privacy by construction).
        for code in [KEY_A, KEY_1, KEY_SPACE] {
            assert_eq!(c.classify(ev(EV_KEY, code, 1)), Some(SemanticAction::TypingActivity));
        }
    }

    #[test]
    fn key_release_is_ignored_so_only_presses_drive_typing() {
        let mut c = EvdevClassifier::new();
        assert_eq!(c.classify(ev(EV_KEY, KEY_A, 0)), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-linux-input classify`
Expected: FAIL — `EvdevClassifier`/constants not defined.

- [ ] **Step 3: Implement the classifier**

Add above the test module in `src/classify.rs`:

```rust
//! Pure, host-agnostic classification of raw evdev events into privacy-filtered
//! semantic actions. No device identity, no raw key code, and no typed text
//! ever leaves this module — ordinary keys collapse to `TypingActivity`; only
//! Enter/Tab survive as semantic keys. Tested on every CI host.

use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

// Linux input-event-codes.h ABI constants (stable kernel UAPI).
pub(crate) const EV_SYN: u16 = 0x00;
pub(crate) const EV_KEY: u16 = 0x01;
pub(crate) const EV_REL: u16 = 0x02;

pub(crate) const REL_X: u16 = 0x00;
pub(crate) const REL_Y: u16 = 0x01;
pub(crate) const REL_HWHEEL: u16 = 0x06;
pub(crate) const REL_WHEEL: u16 = 0x08;

pub(crate) const BTN_LEFT: u16 = 0x110;
pub(crate) const BTN_RIGHT: u16 = 0x111;
pub(crate) const BTN_MIDDLE: u16 = 0x112;

pub(crate) const KEY_TAB: u16 = 15;
pub(crate) const KEY_ENTER: u16 = 28;
// Codes used only by tests to represent "some ordinary key".
#[cfg(test)]
pub(crate) const KEY_A: u16 = 30;
#[cfg(test)]
pub(crate) const KEY_1: u16 = 2;
#[cfg(test)]
pub(crate) const KEY_SPACE: u16 = 57;

/// A native evdev event reduced to the three fields classification needs.
/// Deliberately minimal: no timestamp, no device handle, no name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEvdevEvent {
    pub ev_type: u16,
    pub code: u16,
    pub value: i32,
}

/// Stateless classifier (kept as a struct for symmetry with the macOS side and
/// to leave room for future stateful rules without an API break).
#[derive(Debug, Default)]
pub struct EvdevClassifier {
    _private: (),
}

impl EvdevClassifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Map one raw evdev event to a semantic action, or `None` to ignore it.
    /// Only key/button *presses* (`value == 1`) and wheel motion produce
    /// actions; releases (`0`), autorepeat (`2`), pointer motion, and sync are
    /// ignored.
    pub fn classify(&mut self, ev: RawEvdevEvent) -> Option<SemanticAction> {
        match ev.ev_type {
            EV_KEY if ev.value == 1 => match ev.code {
                BTN_LEFT => Some(SemanticAction::Click { button: MouseButton::Left, position: None }),
                BTN_RIGHT => Some(SemanticAction::Click { button: MouseButton::Right, position: None }),
                BTN_MIDDLE => Some(SemanticAction::Click { button: MouseButton::Middle, position: None }),
                // Any other BTN_* in the mouse range -> Other button click.
                c if (0x110..0x118).contains(&c) => {
                    Some(SemanticAction::Click { button: MouseButton::Other, position: None })
                }
                KEY_ENTER => Some(SemanticAction::SemanticKey(SemanticKey::Enter)),
                KEY_TAB => Some(SemanticAction::SemanticKey(SemanticKey::Tab)),
                // Every other key press is ordinary typing — the code is dropped.
                _ => Some(SemanticAction::TypingActivity),
            },
            EV_REL if ev.code == REL_WHEEL || ev.code == REL_HWHEEL => {
                Some(SemanticAction::ScrollActivity)
            }
            // REL_X/REL_Y pointer motion, EV_SYN, releases, autorepeat: ignored.
            EV_REL if ev.code == REL_X || ev.code == REL_Y => None,
            EV_SYN => None,
            _ => None,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-linux-input classify`
Expected: PASS (8 tests).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 5: Commit (folds in Task 1 scaffold)**

```bash
rtk git add Cargo.toml crates/rollshot-linux-input/
rtk git commit -m "feat(linux-input): scaffold rollshot-linux-input with pure evdev classifier

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `EvdevInputSource` — discovery, reader threads, queue

**Files:**
- Create/replace: `crates/rollshot-linux-input/src/source.rs`
- Test: same file (`#[cfg(test)]`)

This is the native glue. On Linux it discovers readable `/dev/input/event*` devices via `evdev::enumerate()`, spawns one blocking reader thread per device, classifies each event, stamps it with ms-since-`start()`, and pushes it to a shared `Mutex<Vec<TimedSemanticAction>>` drained by `poll()`. On non-Linux it is a stub returning `DegradedReason::SourceStartFailed`. The reader threads and start/fallback logic require real devices/permissions, so they are **manually verified** (spec §Manual Verification) — the unit test here covers only the host-agnostic stub behavior and the `poll`/`stop` contract on an unstarted source.

- [ ] **Step 1: Write the failing source tests**

Replace `src/source.rs` test block first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{CaptureRegion, SemanticInputSource};

    fn region() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 100, height: 80 }
    }

    #[test]
    fn unstarted_source_polls_empty_and_stops_cleanly() {
        let mut src = EvdevInputSource::new();
        assert!(src.poll().is_empty());
        src.stop(); // no-op before start must not panic
        assert!(src.poll().is_empty());
    }

    #[test]
    fn source_is_send_and_object_safe() {
        fn assert_send<T: Send>() {}
        assert_send::<EvdevInputSource>();
        let _boxed: Box<dyn SemanticInputSource> = Box::new(EvdevInputSource::new());
    }

    #[test]
    fn shared_queue_is_bounded_and_drops_oldest() {
        let shared = Shared::default();
        for i in 0..(MAX_QUEUED as u64 + 10) {
            shared.push(rollshot_action::TimedSemanticAction {
                action: rollshot_action::SemanticAction::TypingActivity,
                at_ms: i,
            });
        }
        let q = shared.queue.lock().unwrap();
        assert_eq!(q.len(), MAX_QUEUED);
        assert_eq!(q.front().unwrap().at_ms, 10, "the 10 oldest are dropped");
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_start_degrades_to_source_start_failed() {
        let mut src = EvdevInputSource::new();
        let err = src.start(region()).unwrap_err();
        assert_eq!(err, rollshot_action::DegradedReason::SourceStartFailed);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-linux-input source`
Expected: FAIL — `EvdevInputSource` not defined.

- [ ] **Step 3: Implement the shared queue + the source skeleton (host-agnostic)**

Add above the test module:

```rust
//! `EvdevInputSource`: read-only evdev observation behind the
//! `SemanticInputSource` trait. Discovery, reader threads, and event reads are
//! Linux-only; on other hosts `start` returns `DegradedReason::SourceStartFailed`
//! so the crate compiles in the full workspace build. Reader threads and
//! start/permission paths are manually verified (spec §Manual Verification).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticInputSource, TimedSemanticAction,
};

const TARGET: &str = "rollshot::action::linux_input";

/// Hard cap on buffered actions, so a stalled consumer cannot grow memory
/// without bound. Drop-oldest preserves recency (spec: explicit fixed bounds).
const MAX_QUEUED: usize = 4096;

#[derive(Default)]
struct Shared {
    queue: Mutex<std::collections::VecDeque<TimedSemanticAction>>,
}

impl Shared {
    fn push(&self, ev: TimedSemanticAction) {
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_QUEUED {
                q.pop_front();
            }
            q.push_back(ev);
        }
    }
}

pub struct EvdevInputSource {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    readers: Vec<JoinHandle<()>>,
    started_at: Option<Instant>,
}

impl EvdevInputSource {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            stop: Arc::new(AtomicBool::new(false)),
            readers: Vec::new(),
            started_at: None,
        }
    }
}

impl Default for EvdevInputSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticInputSource for EvdevInputSource {
    fn start(&mut self, region: CaptureRegion) -> Result<InputCapability, DegradedReason> {
        self.started_at = Some(Instant::now());
        self.start_platform(region)
    }

    fn poll(&mut self) -> Vec<TimedSemanticAction> {
        match self.shared.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn stop(&mut self) {
        // Signal the readers and JOIN them, so observation stops promptly and no
        // thread can push events after `stop` returns (privacy: input observed
        // only between start and stop). Reads are non-blocking (see
        // `start_platform`), so each reader exits within one poll interval.
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.readers.drain(..) {
            let _ = handle.join();
        }
        tracing::debug!(target: TARGET, "evdev source stopped");
    }
}
```

- [ ] **Step 4: Implement the non-Linux stub for `start_platform`**

Add (still above the test module):

```rust
#[cfg(not(target_os = "linux"))]
impl EvdevInputSource {
    fn start_platform(
        &mut self,
        _region: CaptureRegion,
    ) -> Result<InputCapability, DegradedReason> {
        tracing::debug!(target: TARGET, "evdev unavailable on this platform");
        Err(DegradedReason::SourceStartFailed)
    }
}
```

- [ ] **Step 5: Implement the Linux `start_platform` (discovery + reader threads)**

Add the Linux implementation. The reducer from `evdev::InputEvent` to `RawEvdevEvent` is the *only* part bound to the `evdev` crate's API; keep it in one small function so an API drift is a one-line fix.

```rust
#[cfg(target_os = "linux")]
impl EvdevInputSource {
    fn start_platform(
        &mut self,
        _region: CaptureRegion,
    ) -> Result<InputCapability, DegradedReason> {
        use crate::classify::EvdevClassifier;

        // evdev::enumerate yields (PathBuf, Device) for each readable device.
        // A device we cannot open is skipped; if none open, decide the reason.
        let devices: Vec<evdev::Device> = evdev::enumerate().map(|(_path, dev)| dev).collect();

        if devices.is_empty() {
            // Distinguish "exists but unreadable" (permission) from "none".
            // `/dev/input` entries exist on any Linux desktop, so an empty
            // enumerate result is overwhelmingly a permission/ACL problem.
            if std::path::Path::new("/dev/input").read_dir().is_ok_and(|mut d| d.next().is_some()) {
                tracing::warn!(target: TARGET, "no readable input devices; ACL likely missing");
                return Err(DegradedReason::PermissionDenied);
            }
            tracing::warn!(target: TARGET, "no input devices present");
            return Err(DegradedReason::NoInputDevice);
        }

        let started_at = self.started_at.expect("start_platform called after stamping start");
        for mut device in devices {
            // Non-blocking so the reader loop can observe the `stop` flag and
            // exit promptly (privacy + joinable shutdown). If this device cannot
            // be set non-blocking, skip it rather than spawn an unstoppable
            // reader.
            if device.set_nonblocking(true).is_err() {
                tracing::warn!(target: TARGET, "could not set device non-blocking; skipping");
                continue;
            }
            let shared = Arc::clone(&self.shared);
            let stop = Arc::clone(&self.stop);
            let handle = std::thread::Builder::new()
                .name("rollshot-evdev-reader".into())
                .spawn(move || {
                    let mut classifier = EvdevClassifier::new();
                    while !stop.load(Ordering::Relaxed) {
                        match device.fetch_events() {
                            Ok(events) => {
                                for ev in events {
                                    let raw = crate::source::reduce_event(&ev);
                                    if let Some(action) = classifier.classify(raw) {
                                        let at_ms = started_at.elapsed().as_millis() as u64;
                                        shared.push(TimedSemanticAction { action, at_ms });
                                    }
                                }
                            }
                            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                // No events pending: sleep briefly, then re-check
                                // `stop`. Keeps shutdown responsive (~15 ms).
                                std::thread::sleep(std::time::Duration::from_millis(15));
                            }
                            Err(err) => {
                                // Reader death is non-fatal: log and exit this
                                // thread; other readers keep running. (Mid-
                                // session capability downgrade is out of scope;
                                // see plan Scope Boundary.)
                                tracing::warn!(target: TARGET, error = %err, "evdev reader stopped");
                                break;
                            }
                        }
                    }
                })
                .map_err(|_| DegradedReason::SourceStartFailed)?;
            self.readers.push(handle);
        }

        tracing::info!(target: TARGET, readers = self.readers.len(), "evdev source started");
        Ok(InputCapability::SemanticEvents)
    }
}

/// Reduce an `evdev::InputEvent` to the pure-core `RawEvdevEvent`. This is the
/// only function coupled to the `evdev` crate's event API; if the crate version
/// changes the accessor names, fix them here only.
#[cfg(target_os = "linux")]
pub(crate) fn reduce_event(ev: &evdev::InputEvent) -> crate::classify::RawEvdevEvent {
    crate::classify::RawEvdevEvent {
        ev_type: ev.event_type().0,
        code: ev.code(),
        value: ev.value(),
    }
}
```

> **evdev API note for the executor:** `evdev = "0.12"` exposes `evdev::enumerate() -> impl Iterator<Item = (PathBuf, Device)>`, `Device::set_nonblocking(&self, bool) -> io::Result<()>`, `Device::fetch_events() -> io::Result<impl Iterator<Item = InputEvent>>` (returns `WouldBlock` immediately when non-blocking and no events are pending), `InputEvent::event_type() -> EventType` (a newtype with `.0: u16`), `InputEvent::code() -> u16`, and `InputEvent::value() -> i32`. The evdev-coupled surface is exactly two places: the reader loop (`set_nonblocking` + `fetch_events`) and `reduce_event`; the entire classifier is API-agnostic. If `cargo build` reports different names — in particular if `set_nonblocking` is unavailable — the localized fallback is to poll the device's `as_raw_fd()` with `nix::poll` (add `nix = { workspace = true }` to the Linux-target deps; it is already a workspace dep and needs no `unsafe`). Confirm with `cargo doc -p evdev --open`.

- [ ] **Step 6: Run tests + Linux build to verify**

Run: `rtk cargo test -p rollshot-linux-input`
Expected: PASS (classifier tests + source tests; on a Linux host the `non_linux` test is cfg-excluded).

Run: `rtk cargo clippy -p rollshot-linux-input --all-targets -- -D warnings`
Expected: no warnings.

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-linux-input/src/source.rs
rtk git commit -m "feat(linux-input): add evdev reader-thread source behind SemanticInputSource

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# Part 2 — `rollshot-macos-input` (CGEventTap, unsafe-isolation)

## Task 4: Scaffold `rollshot-macos-input` (unsafe opt-out)

**Files:**
- Create: `crates/rollshot-macos-input/Cargo.toml`
- Create: `crates/rollshot-macos-input/src/lib.rs`
- Create: `crates/rollshot-macos-input/src/classify.rs` (placeholder; filled in Task 5)
- Create: `crates/rollshot-macos-input/src/permission.rs` (placeholder; filled in Task 6)
- Create: `crates/rollshot-macos-input/src/source.rs` (placeholder; filled in Task 7)

(The crate is already in `members` from Task 1 Step 1.)

- [ ] **Step 1: Create the manifest — mirror `rollshot-macos-oneshot` exactly**

`crates/rollshot-macos-input/Cargo.toml`:

```toml
[package]
name = "rollshot-macos-input"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
rollshot-action = { path = "../rollshot-action" }
thiserror = { workspace = true }
tracing = { workspace = true }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-core-foundation = "0.3"
objc2-core-graphics = "0.3"

[lints.rust]
# This crate intentionally contains unsafe code for CoreGraphics FFI (a
# listen-only CGEventTap and its CFRunLoop thread). Unsafe blocks are kept local
# and documented with ownership invariants. The rest of the workspace maintains
# unsafe_code = "forbid".
unsafe_code = "allow"
```

> Note: this crate uses `[lints.rust]` (NOT `[lints] workspace = true`) so it overrides the workspace `unsafe_code = "forbid"`. This is the verified mechanism `rollshot-macos-oneshot` uses (`crates/rollshot-macos-oneshot/Cargo.toml:21-25`).

- [ ] **Step 2: Create `src/lib.rs` with module decls + re-exports**

```rust
//! macOS semantic-input source for Action Guide. Observes global input through
//! a listen-only CoreGraphics `CGEventTap` on a dedicated CFRunLoop thread.
//! Exposes only privacy-filtered semantic actions and explicit failure reasons
//! — no Unicode text extraction, no input injection, no raw key persistence.
//! Implements `rollshot_action::SemanticInputSource`. This is an
//! unsafe-isolation crate (Objective-C / CoreFoundation FFI); its public API is
//! safe and the workspace keeps `unsafe_code = "forbid"` elsewhere, mirroring
//! `rollshot-macos-oneshot`. On non-macOS hosts the source is a stub reporting
//! `DegradedReason::SourceStartFailed`. See
//! `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

mod classify;
mod permission;
mod source;

pub use classify::{classify_cg, RawCgEvent, RawCgKind};
pub use permission::{
    input_monitoring_status, open_input_monitoring_settings, request_input_monitoring,
    InputMonitoringStatus,
};
pub use source::MacosInputSource;

pub use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, MouseButton, SemanticAction, SemanticInputSource,
    SemanticKey, TimedSemanticAction,
};
```

- [ ] **Step 3: Add placeholder module bodies (commit folds into Task 5)**

Create the three module files with only doc comments (filled by Tasks 5–7). As in Task 1, the re-exports mean the crate won't compile until Task 5; do not build here, and commit at the end of Task 5.

`src/classify.rs`: `//! Pure CGEvent classification (filled in Task 5).`
`src/permission.rs`: `//! Input Monitoring permission API (filled in Task 6).`
`src/source.rs`: `//! MacosInputSource CGEventTap glue (filled in Task 7).`

---

## Task 5: Pure CGEvent classification core

**Files:**
- Create/replace: `crates/rollshot-macos-input/src/classify.rs`
- Test: same file (`#[cfg(test)]`)

macOS virtual keycodes: Return = `0x24` (36), Tab = `0x30` (48). `CGMouseButton`: Left = 0, Right = 1, Center = 2. The tap callback (Task 7) reduces each `CGEvent` to a `RawCgEvent`; this core maps it to a semantic action. Key *up*, `FlagsChanged`, mouse-move, and tap-disabled events reduce to `RawCgKind::Other` and are ignored here.

- [ ] **Step 1: Write the failing classifier tests**

Replace `src/classify.rs` test block first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

    const KEYCODE_RETURN: i64 = 0x24;
    const KEYCODE_TAB: i64 = 0x30;
    const KEYCODE_A: i64 = 0x00;

    fn ev(kind: RawCgKind, button_number: i64, keycode: i64) -> RawCgEvent {
        RawCgEvent { kind, button_number, keycode }
    }

    #[test]
    fn mouse_downs_map_to_their_buttons_without_position() {
        assert_eq!(
            classify_cg(ev(RawCgKind::LeftMouseDown, 0, 0)),
            Some(SemanticAction::Click { button: MouseButton::Left, position: None })
        );
        assert_eq!(
            classify_cg(ev(RawCgKind::RightMouseDown, 1, 0)),
            Some(SemanticAction::Click { button: MouseButton::Right, position: None })
        );
    }

    #[test]
    fn other_mouse_button_two_is_middle_others_are_other() {
        assert_eq!(
            classify_cg(ev(RawCgKind::OtherMouseDown, 2, 0)),
            Some(SemanticAction::Click { button: MouseButton::Middle, position: None })
        );
        assert_eq!(
            classify_cg(ev(RawCgKind::OtherMouseDown, 3, 0)),
            Some(SemanticAction::Click { button: MouseButton::Other, position: None })
        );
    }

    #[test]
    fn scroll_wheel_is_scroll_activity() {
        assert_eq!(classify_cg(ev(RawCgKind::ScrollWheel, 0, 0)), Some(SemanticAction::ScrollActivity));
    }

    #[test]
    fn return_and_tab_keydowns_are_semantic_keys() {
        assert_eq!(
            classify_cg(ev(RawCgKind::KeyDown, 0, KEYCODE_RETURN)),
            Some(SemanticAction::SemanticKey(SemanticKey::Enter))
        );
        assert_eq!(
            classify_cg(ev(RawCgKind::KeyDown, 0, KEYCODE_TAB)),
            Some(SemanticAction::SemanticKey(SemanticKey::Tab))
        );
    }

    #[test]
    fn ordinary_keydown_collapses_to_typing_activity_never_a_keycode() {
        assert_eq!(
            classify_cg(ev(RawCgKind::KeyDown, 0, KEYCODE_A)),
            Some(SemanticAction::TypingActivity)
        );
    }

    #[test]
    fn other_kinds_are_ignored() {
        // KeyUp, FlagsChanged, mouse-move, tap-disabled all reduce to Other.
        assert_eq!(classify_cg(ev(RawCgKind::Other, 0, KEYCODE_A)), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-macos-input classify`
Expected: FAIL — `classify_cg`/`RawCgEvent` not defined.

- [ ] **Step 3: Implement the classifier**

Add above the test module:

```rust
//! Pure, host-agnostic classification of a reduced CoreGraphics event into a
//! privacy-filtered semantic action. No Unicode text is ever read; ordinary
//! key-downs collapse to `TypingActivity`; only Return/Tab survive as semantic
//! keys. Tested on every CI host.

use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

/// macOS virtual keycodes (Carbon `kVK_*`).
const KEYCODE_RETURN: i64 = 0x24;
const KEYCODE_TAB: i64 = 0x30;

/// The subset of `CGEventType` that produces a semantic action. The tap
/// callback (source.rs) reduces every `CGEvent` to one of these; everything
/// else (KeyUp, FlagsChanged, mouse-move, ScrollWheel deltas aside, tap-
/// disabled) becomes `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawCgKind {
    LeftMouseDown,
    RightMouseDown,
    OtherMouseDown,
    ScrollWheel,
    KeyDown,
    Other,
}

/// A native CGEvent reduced to the fields classification needs. `button_number`
/// is meaningful only for `OtherMouseDown`; `keycode` only for `KeyDown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawCgEvent {
    pub kind: RawCgKind,
    pub button_number: i64,
    pub keycode: i64,
}

/// Map a reduced CoreGraphics event to a semantic action, or `None` to ignore.
pub fn classify_cg(ev: RawCgEvent) -> Option<SemanticAction> {
    match ev.kind {
        RawCgKind::LeftMouseDown => {
            Some(SemanticAction::Click { button: MouseButton::Left, position: None })
        }
        RawCgKind::RightMouseDown => {
            Some(SemanticAction::Click { button: MouseButton::Right, position: None })
        }
        RawCgKind::OtherMouseDown => {
            let button = if ev.button_number == 2 { MouseButton::Middle } else { MouseButton::Other };
            Some(SemanticAction::Click { button, position: None })
        }
        RawCgKind::ScrollWheel => Some(SemanticAction::ScrollActivity),
        RawCgKind::KeyDown => match ev.keycode {
            KEYCODE_RETURN => Some(SemanticAction::SemanticKey(SemanticKey::Enter)),
            KEYCODE_TAB => Some(SemanticAction::SemanticKey(SemanticKey::Tab)),
            _ => Some(SemanticAction::TypingActivity),
        },
        RawCgKind::Other => None,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-macos-input classify`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit (folds in Task 4 scaffold)**

```bash
rtk git add crates/rollshot-macos-input/
rtk git commit -m "feat(macos-input): scaffold unsafe-isolation crate with pure CGEvent classifier

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Input Monitoring permission API

**Files:**
- Create/replace: `crates/rollshot-macos-input/src/permission.rs`
- Test: same file (`#[cfg(test)]`)

A listen-only HID tap needs **Input Monitoring** (TCC `kTCCServiceListenEvent`), checked via `CGPreflightListenEventAccess()` / requested via `CGRequestListenEventAccess()`. This is distinct from Accessibility (`AXIsProcessTrusted`) — P0b must NOT request Accessibility or PostEvent (spec §macOS). The status enum + the non-macOS stub are host-agnostic and unit-testable; the macOS FFI calls are verified on the macOS CI runner via `cargo check` and manually.

- [ ] **Step 1: Write the failing permission test**

Replace `src/permission.rs` test block first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_status_is_denied_and_request_does_not_panic() {
        assert_eq!(input_monitoring_status(), InputMonitoringStatus::Denied);
        assert_eq!(request_input_monitoring(), InputMonitoringStatus::Denied);
        open_input_monitoring_settings(); // must be a no-op, not a panic
    }

    #[test]
    fn status_enum_distinguishes_three_states() {
        // Compile-time proof the three TCC states are representable and that
        // Input Monitoring is modeled separately from Accessibility (which this
        // crate never touches).
        let all = [
            InputMonitoringStatus::Granted,
            InputMonitoringStatus::Denied,
            InputMonitoringStatus::NotDetermined,
        ];
        assert_eq!(all.len(), 3);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-macos-input permission`
Expected: FAIL — `InputMonitoringStatus` not defined.

- [ ] **Step 3: Implement the status enum + non-macOS stub**

Add above the test module:

```rust
//! Input Monitoring (TCC `kTCCServiceListenEvent`) permission operations. A
//! listen-only `CGEventTap` needs Input Monitoring only — never Accessibility
//! or PostEvent, which this crate deliberately does not request (spec §macOS).

const TARGET: &str = "rollshot::action::macos_input";

/// Tri-state Input Monitoring permission, mapped from CoreGraphics' boolean
/// preflight plus a request attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMonitoringStatus {
    Granted,
    Denied,
    NotDetermined,
}

#[cfg(not(target_os = "macos"))]
pub fn input_monitoring_status() -> InputMonitoringStatus {
    InputMonitoringStatus::Denied
}

#[cfg(not(target_os = "macos"))]
pub fn request_input_monitoring() -> InputMonitoringStatus {
    InputMonitoringStatus::Denied
}

#[cfg(not(target_os = "macos"))]
pub fn open_input_monitoring_settings() {}
```

- [ ] **Step 4: Implement the macOS FFI variant**

Add (the FFI calls are the manually-verified part):

```rust
#[cfg(target_os = "macos")]
pub fn input_monitoring_status() -> InputMonitoringStatus {
    // SAFETY: CGPreflightListenEventAccess takes no arguments and returns a
    // Boolean; it has no ownership side effects.
    let granted = unsafe { objc2_core_graphics::CGPreflightListenEventAccess() };
    if granted {
        InputMonitoringStatus::Granted
    } else {
        // Preflight cannot distinguish "denied" from "not yet asked"; callers
        // treat both as not-granted and may call `request_input_monitoring`.
        InputMonitoringStatus::NotDetermined
    }
}

#[cfg(target_os = "macos")]
pub fn request_input_monitoring() -> InputMonitoringStatus {
    // SAFETY: CGRequestListenEventAccess prompts (once) and returns whether
    // access is now granted; no ownership side effects.
    let granted = unsafe { objc2_core_graphics::CGRequestListenEventAccess() };
    if granted {
        InputMonitoringStatus::Granted
    } else {
        InputMonitoringStatus::Denied
    }
}

#[cfg(target_os = "macos")]
pub fn open_input_monitoring_settings() {
    // Open the Input Monitoring pane via the standard System Settings URL.
    // Use `open(1)` to avoid pulling in AppKit here.
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
    if let Err(err) = std::process::Command::new("open").arg(url).spawn() {
        tracing::warn!(target: TARGET, error = %err, "failed to open Input Monitoring settings");
    }
}
```

> **objc2-core-graphics API note:** `CGPreflightListenEventAccess`/`CGRequestListenEventAccess` are exposed by `objc2-core-graphics 0.3` as `unsafe fn() -> bool`. If the published binding names or signatures differ on the macOS runner, adjust ONLY these two functions; the status enum, the URL, and all callers are stable. Verify with `cargo doc -p objc2-core-graphics` on macOS. If the bindings are absent in 0.3, declare them in a local `extern "C"` block (they live in the `CoreGraphics` framework, already linked transitively) — keep that block in `permission.rs` with a `// SAFETY` note.

- [ ] **Step 5: Run tests + verify**

Run: `rtk cargo test -p rollshot-macos-input permission`
Expected: PASS (on Linux: 2 tests; on macOS: the `non_macos` test is cfg-excluded, 1 test).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-macos-input/src/permission.rs
rtk git commit -m "feat(macos-input): add Input Monitoring permission API (ListenEvent, not Accessibility)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: `MacosInputSource` — CGEventTap + CFRunLoop thread

**Files:**
- Create/replace: `crates/rollshot-macos-input/src/source.rs`
- Test: same file (`#[cfg(test)]`)

This is the unsafe FFI glue, isolated behind a safe `SemanticInputSource` impl. Architecture (from the verified `CrossMacro` approach, reimplemented — not copied): a dedicated thread owns a `CFRunLoop`; `start()` checks Input Monitoring, creates a listen-only HID tap with a C callback, adds it as a run-loop source, enables it, and runs the loop; the callback reduces each `CGEvent` to a `RawCgEvent`, classifies it, stamps it with ms-since-`start()`, and pushes to a shared queue; `TapDisabledByTimeout` re-enables the tap; `stop()` disables the tap, stops the run loop, joins the thread, and releases native objects. The unit test covers only the host-agnostic stub + `poll`/`stop`/`Send` contract; the FFI/run-loop path is verified by macOS `cargo check` + manual testing (spec §Manual Verification, §Platform Input Tests).

- [ ] **Step 1: Write the failing source tests**

Replace `src/source.rs` test block first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{CaptureRegion, SemanticInputSource};

    fn region() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 100, height: 80 }
    }

    #[test]
    fn unstarted_source_polls_empty_and_stops_cleanly() {
        let mut src = MacosInputSource::new();
        assert!(src.poll().is_empty());
        src.stop();
        assert!(src.poll().is_empty());
    }

    #[test]
    fn source_is_send_and_object_safe() {
        fn assert_send<T: Send>() {}
        assert_send::<MacosInputSource>();
        let _boxed: Box<dyn SemanticInputSource> = Box::new(MacosInputSource::new());
    }

    #[test]
    fn shared_queue_is_bounded_and_drops_oldest() {
        let shared = Shared::default();
        for i in 0..(MAX_QUEUED as u64 + 10) {
            shared.push(rollshot_action::TimedSemanticAction {
                action: rollshot_action::SemanticAction::TypingActivity,
                at_ms: i,
            });
        }
        let q = shared.queue.lock().unwrap();
        assert_eq!(q.len(), MAX_QUEUED);
        assert_eq!(q.front().unwrap().at_ms, 10, "the 10 oldest are dropped");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_start_degrades_to_source_start_failed() {
        let mut src = MacosInputSource::new();
        assert_eq!(src.start(region()).unwrap_err(), rollshot_action::DegradedReason::SourceStartFailed);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-macos-input source`
Expected: FAIL — `MacosInputSource` not defined.

- [ ] **Step 3: Implement the host-agnostic skeleton + shared queue**

Add above the test module:

```rust
//! `MacosInputSource`: a listen-only `CGEventTap` on a dedicated CFRunLoop
//! thread, behind the `SemanticInputSource` trait. All CoreGraphics /
//! CoreFoundation FFI is isolated here with `// SAFETY` notes; the public API is
//! safe. On non-macOS hosts `start` returns `DegradedReason::SourceStartFailed`.
//! Reimplements the CrossMacro approach (GPLv3 learning reference) without
//! copying its source.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticInputSource, TimedSemanticAction,
};

const TARGET: &str = "rollshot::action::macos_input";

/// Hard cap on buffered actions (drop-oldest), mirroring the Linux source so a
/// stalled consumer cannot grow memory without bound.
const MAX_QUEUED: usize = 4096;

#[derive(Default)]
struct Shared {
    queue: Mutex<std::collections::VecDeque<TimedSemanticAction>>,
}

impl Shared {
    fn push(&self, ev: TimedSemanticAction) {
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_QUEUED {
                q.pop_front();
            }
            q.push_back(ev);
        }
    }
}

pub struct MacosInputSource {
    shared: Arc<Shared>,
    started_at: Option<Instant>,
    #[cfg(target_os = "macos")]
    runloop: Option<macos::RunLoopHandle>,
    thread: Option<JoinHandle<()>>,
}

impl MacosInputSource {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            started_at: None,
            #[cfg(target_os = "macos")]
            runloop: None,
            thread: None,
        }
    }
}

impl Default for MacosInputSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticInputSource for MacosInputSource {
    fn start(&mut self, region: CaptureRegion) -> Result<InputCapability, DegradedReason> {
        self.started_at = Some(Instant::now());
        self.start_platform(region)
    }

    fn poll(&mut self) -> Vec<TimedSemanticAction> {
        match self.shared.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn stop(&mut self) {
        self.stop_platform();
    }
}

#[cfg(not(target_os = "macos"))]
impl MacosInputSource {
    fn start_platform(&mut self, _region: CaptureRegion) -> Result<InputCapability, DegradedReason> {
        tracing::debug!(target: TARGET, "CGEventTap unavailable on this platform");
        Err(DegradedReason::SourceStartFailed)
    }

    fn stop_platform(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
```

- [ ] **Step 4: Implement the macOS FFI module (`mod macos`) and `start_platform`/`stop_platform`**

Add the macOS-gated module. The C callback must be a free `extern "C"` function; it receives a pointer to the shared queue + start instant via the tap's `user_info`. Keep every `unsafe` block small with a `// SAFETY` note.

```rust
#[cfg(target_os = "macos")]
impl MacosInputSource {
    fn start_platform(&mut self, _region: CaptureRegion) -> Result<InputCapability, DegradedReason> {
        // Permission gate: Input Monitoring is required for a listen-only tap.
        if !matches!(
            crate::permission::input_monitoring_status(),
            crate::permission::InputMonitoringStatus::Granted
        ) {
            // Prompt once; if still not granted, degrade to visual-only.
            if !matches!(
                crate::permission::request_input_monitoring(),
                crate::permission::InputMonitoringStatus::Granted
            ) {
                tracing::warn!(target: TARGET, "Input Monitoring not granted");
                return Err(DegradedReason::PermissionDenied);
            }
        }

        let shared = Arc::clone(&self.shared);
        let started_at = self.started_at.expect("start_platform after stamping start");
        let (tx, rx) = std::sync::mpsc::channel::<Result<macos::RunLoopHandle, DegradedReason>>();

        let handle = std::thread::Builder::new()
            .name("rollshot-cgtap".into())
            .spawn(move || macos::run_tap_thread(shared, started_at, tx))
            .map_err(|_| DegradedReason::SourceStartFailed)?;
        self.thread = Some(handle);

        // The thread reports tap-creation success/failure before running the
        // loop, so `start` returns a definite capability.
        match rx.recv() {
            Ok(Ok(runloop)) => {
                self.runloop = Some(runloop);
                tracing::info!(target: TARGET, "CGEventTap started");
                Ok(InputCapability::SemanticEvents)
            }
            Ok(Err(reason)) => {
                if let Some(handle) = self.thread.take() {
                    let _ = handle.join();
                }
                Err(reason)
            }
            Err(_) => Err(DegradedReason::SourceStartFailed),
        }
    }

    fn stop_platform(&mut self) {
        if let Some(runloop) = self.runloop.take() {
            // SAFETY: `runloop` is the run loop the tap thread is blocked in;
            // stopping it unblocks `CFRunLoopRun` so the thread exits and frees
            // its tap/source. The handle is only ever used here, once.
            unsafe { runloop.stop() };
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        tracing::debug!(target: TARGET, "CGEventTap stopped");
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use std::time::Instant;

    use rollshot_action::{DegradedReason, TimedSemanticAction};

    use crate::classify::{classify_cg, RawCgEvent, RawCgKind};

    use objc2_core_foundation::{
        CFMachPort, CFRunLoop, CFRunLoopSource, CFRetained,
    };
    use objc2_core_graphics::{
        CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType,
    };

    use super::{Shared, TARGET};

    /// A `Send` handle to the tap thread's run loop, used only to stop it.
    pub struct RunLoopHandle(CFRetained<CFRunLoop>);
    // SAFETY: `CFRunLoop` is internally thread-safe for `CFRunLoopStop`, which
    // is the only operation performed through this handle, from `stop_platform`
    // on the owning thread after `start` returned. We never deref it elsewhere.
    unsafe impl Send for RunLoopHandle {}

    impl RunLoopHandle {
        /// # Safety
        /// Caller guarantees the referenced run loop is the tap thread's loop.
        pub unsafe fn stop(&self) {
            CFRunLoop::stop(Some(&self.0));
        }
    }

    /// Context passed to the C callback via the tap's `user_info` pointer.
    /// `tap` is set once, right after creation, so the callback can re-enable
    /// the tap on `TapDisabledByTimeout`. `CallbackCtx` is created and accessed
    /// only on the tap thread, so a single-threaded `OnceCell` is sufficient.
    struct CallbackCtx {
        shared: Arc<Shared>,
        started_at: Instant,
        tap: std::cell::OnceCell<CFRetained<CFMachPort>>,
    }

    /// Reduce a live `CGEvent` to the pure-core `RawCgEvent`.
    ///
    /// # Safety
    /// `event` must be a valid `CGEvent` for the duration of this call (it is,
    /// inside the tap callback).
    unsafe fn reduce(kind: RawCgKind, event: *mut CGEvent) -> RawCgEvent {
        let button_number = if matches!(kind, RawCgKind::OtherMouseDown) {
            CGEvent::integer_value_field(
                Some(&*event),
                CGEventField::MouseEventButtonNumber,
            )
        } else {
            0
        };
        let keycode = if matches!(kind, RawCgKind::KeyDown) {
            CGEvent::integer_value_field(Some(&*event), CGEventField::KeyboardEventKeycode)
        } else {
            0
        };
        RawCgEvent { kind, button_number, keycode }
    }

    fn kind_of(event_type: CGEventType) -> RawCgKind {
        match event_type {
            CGEventType::LeftMouseDown => RawCgKind::LeftMouseDown,
            CGEventType::RightMouseDown => RawCgKind::RightMouseDown,
            CGEventType::OtherMouseDown => RawCgKind::OtherMouseDown,
            CGEventType::ScrollWheel => RawCgKind::ScrollWheel,
            CGEventType::KeyDown => RawCgKind::KeyDown,
            _ => RawCgKind::Other,
        }
    }

    /// The tap callback. Listen-only, so the returned event pointer is ignored
    /// by the system; we return it unchanged. Declared `extern "C"` (NOT
    /// `"C-unwind"`): a panic unwinding through the CoreFoundation caller is UB,
    /// so the body is kept panic-free (`if let Ok(..)`, no `unwrap`) and `"C"`
    /// makes any unexpected unwind abort rather than cross the FFI boundary.
    ///
    /// # Safety
    /// `user_info` is the `*mut CallbackCtx` we passed to `CGEventTapCreate`,
    /// valid for the tap's lifetime; `event` is a live `CGEvent`.
    unsafe extern "C" fn tap_callback(
        _proxy: *mut std::ffi::c_void,
        event_type: CGEventType,
        event: *mut CGEvent,
        user_info: *mut std::ffi::c_void,
    ) -> *mut CGEvent {
        let ctx = &*(user_info as *const CallbackCtx);

        // Re-enable after an inactivity timeout (the OS disables the tap). The
        // spec requires this; the tap handle was stored into ctx after creation.
        if matches!(event_type, CGEventType::TapDisabledByTimeout) {
            if let Some(tap) = ctx.tap.get() {
                // SAFETY: `tap` is the live mach port for this very tap.
                CGEvent::tap_enable(tap, true);
            }
            tracing::debug!(target: TARGET, "tap re-enabled after timeout");
            return event;
        }

        let kind = kind_of(event_type);
        if !matches!(kind, RawCgKind::Other) {
            let raw = reduce(kind, event);
            if let Some(action) = classify_cg(raw) {
                let at_ms = ctx.started_at.elapsed().as_millis() as u64;
                ctx.shared.push(TimedSemanticAction { action, at_ms });
            }
        }
        event
    }

    /// Event mask: mouse downs, scroll, key down. (Key up / flags-changed are
    /// intentionally excluded — they classify to nothing.)
    fn event_mask() -> u64 {
        let bit = |t: CGEventType| 1u64 << (t.0 as u64);
        bit(CGEventType::LeftMouseDown)
            | bit(CGEventType::RightMouseDown)
            | bit(CGEventType::OtherMouseDown)
            | bit(CGEventType::ScrollWheel)
            | bit(CGEventType::KeyDown)
    }

    /// Owns the run loop for the session. Creates the tap, reports
    /// success/failure through `tx`, then blocks in `CFRunLoopRun` until
    /// `RunLoopHandle::stop` is called.
    pub fn run_tap_thread(
        shared: Arc<Shared>,
        started_at: Instant,
        tx: Sender<Result<RunLoopHandle, DegradedReason>>,
    ) {
        let ctx = Box::into_raw(Box::new(CallbackCtx {
            shared,
            started_at,
            tap: std::cell::OnceCell::new(),
        }));

        // SAFETY: standard CGEventTapCreate for a listen-only HID tap. `ctx` is
        // a valid pointer for the tap's lifetime; we free it after the loop.
        let tap = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::HIDEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                event_mask(),
                Some(tap_callback),
                ctx as *mut std::ffi::c_void,
            )
        };
        let Some(tap) = tap else {
            // Null tap: free ctx, report failure, do not run a loop.
            // SAFETY: ctx came from Box::into_raw and is not used after this.
            unsafe { drop(Box::from_raw(ctx)) };
            let _ = tx.send(Err(DegradedReason::SourceStartFailed));
            return;
        };

        // Give the callback a handle to its own tap so it can re-enable on
        // timeout. SAFETY: ctx is live; we are still the only thread touching it
        // (the run loop has not started yet).
        unsafe {
            let _ = (*ctx).tap.set(tap.clone());
        }

        // SAFETY: create a run-loop source from the tap mach port and add it to
        // this thread's run loop in common modes, then enable the tap.
        let run_loop = unsafe {
            let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
                .expect("run loop source from tap mach port");
            let run_loop = CFRunLoop::current().expect("current run loop");
            CFRunLoop::add_source(
                Some(&run_loop),
                Some(&source),
                objc2_core_foundation::kCFRunLoopCommonModes,
            );
            CGEvent::tap_enable(&tap, true);
            keep_alive(source); // keep the source retained for the loop's life
            run_loop
        };

        if tx.send(Ok(RunLoopHandle(run_loop.clone()))).is_err() {
            return;
        }

        // SAFETY: blocks until CFRunLoopStop is called via RunLoopHandle::stop.
        unsafe { CFRunLoop::run() };

        // Teardown: disable the tap and free the callback context.
        // SAFETY: tap is still valid; ctx came from Box::into_raw.
        unsafe {
            CGEvent::tap_enable(&tap, false);
            drop(Box::from_raw(ctx));
        }
        tracing::debug!(target: TARGET, "tap thread exited");
    }

    /// Hold a retained CF object alive until the thread ends. The run-loop
    /// source must outlive the loop; leaking it for the session is acceptable
    /// because the thread (and process intent) is short-lived per recording.
    fn keep_alive(source: CFRetained<CFRunLoopSource>) {
        std::mem::forget(source);
    }
}
```

> **objc2-core-graphics / objc2-core-foundation API note (IMPORTANT — macOS-only, manually verified):** The exact method names in `objc2-core-graphics 0.3` / `objc2-core-foundation 0.3` may differ from the calls above (e.g. `CGEvent::tap_create` vs a free `CGEventTapCreate` fn, `CFRunLoop::run`/`current`/`stop` vs free functions, `integer_value_field` vs `CGEventGetIntegerValueField`, `tap_enable` vs `CGEventTapEnable`, and whether `CFMachPort::new_run_loop_source` exists). The executor MUST run `cargo doc -p objc2-core-graphics -p objc2-core-foundation` on the macOS runner and adjust these FFI call sites to the published bindings. **Do not change** `classify.rs`, `reduce`'s field choices (`MouseEventButtonNumber`, `KeyboardEventKeycode`), the event mask membership, the `ListenOnly`/`HIDEventTap`/`HeadInsertEventTap` selection, or the timeout re-enable intent — those are the behavioral contract. If a binding is missing entirely, declare a minimal `extern "C"` for it with a `// SAFETY` note (the symbols live in the CoreGraphics/CoreFoundation frameworks, already linked transitively via objc2). The `TapDisabledByTimeout` re-enable is implemented above (the tap handle is stored in `CallbackCtx.tap` after creation and `tap_enable(tap, true)` runs in the timeout branch); confirm `CFRetained<CFMachPort>` and `Clone` for it match the published `objc2-core-foundation` types during macOS verification, but do NOT drop the re-enable — it is spec-required.

- [ ] **Step 5: Run tests + verify**

Run: `rtk cargo test -p rollshot-macos-input`
Expected: PASS (classify + permission + source unit tests; on Linux the `non_macos` tests run and the `macos` mod is cfg-excluded).

Run: `rtk cargo clippy -p rollshot-macos-input --all-targets -- -D warnings`
Expected: no warnings (on Linux; the macOS FFI is `cargo check`ed on the macOS runner in Task 12).

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-macos-input/src/source.rs
rtk git commit -m "feat(macos-input): add listen-only CGEventTap source behind SemanticInputSource

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# Part 3 — App wiring (`action-guide` feature)

## Task 8: Add the `action-guide` feature + gated deps to `rollshot-app`

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`

- [ ] **Step 1: Add the feature and feature/target-gated dependencies**

The platform input crates are pulled in only under the feature AND only on their target OS. `rollshot-action` is pulled in by the feature (any OS). Add to `crates/rollshot-app/Cargo.toml`:

```toml
[features]
action-guide = ["dep:rollshot-action", "dep:rollshot-linux-input", "dep:rollshot-macos-input"]
```

In `[dependencies]`, make `rollshot-action` optional:

```toml
rollshot-action = { path = "../rollshot-action", optional = true }
```

Add a Linux-target section entry (extend the existing `[target.'cfg(target_os = "linux")'.dependencies]`):

```toml
rollshot-linux-input = { path = "../rollshot-linux-input", optional = true }
```

Add a macOS-target entry (extend the existing `[target.'cfg(target_os = "macos")'.dependencies]`):

```toml
rollshot-macos-input = { path = "../rollshot-macos-input", optional = true }
```

> Modern Cargo (workspace MSRV is 1.85) accepts `dep:<name>` in a `[features]` entry where `<name>` is an `optional = true` dependency declared only in a `[target.…]` table: the dependency activates only on the matching target, and the `dep:` reference is a no-op on other targets. The factory (Task 9) uses `#[cfg]` to pick the right crate. **Both feature states are explicitly verified on the current host in Step 2, and CI (Task 12) verifies the feature on both Linux and macOS** — so a cross-OS resolution problem is caught immediately, not at runtime. If (and only if) `cargo check --features action-guide` on either host reports "feature `action-guide` includes `dep:rollshot-…-input` which is not … a dependency", the localized fix is to split the feature list so each platform dep is referenced from a tiny per-target feature, or drop the platform `dep:` from `action-guide` and `#[cfg]`-gate the factory's `use` of the always-built workspace member directly. Do not pre-emptively apply the fallback — try the simple form first.

- [ ] **Step 2: Verify both feature states compile**

Run: `rtk cargo check -p rollshot-app`
Expected: PASS (feature off — no new deps linked).

Run: `rtk cargo check -p rollshot-app --features action-guide`
Expected: PASS (feature on — `rollshot-action` + the host's input crate linked). No new code yet, so no behavior.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-app/Cargo.toml
rtk git commit -m "build(app): add action-guide feature gating rollshot-action + platform input deps

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: `action_input` module — factory, advisory, `ActionInputSession`

**Files:**
- Create: `crates/rollshot-app/src/action_input.rs`
- Modify: `crates/rollshot-app/src/main.rs` (feature-gated `mod` decl)
- Test: `crates/rollshot-app/src/action_input.rs` (`#[cfg(test)]`)

This is the heart of the app wiring and is fully unit-testable via fake sources — no platform devices needed.

- [ ] **Step 1: Declare the module behind the feature in `main.rs`**

Add near the other `mod` declarations in `crates/rollshot-app/src/main.rs`:

```rust
#[cfg(feature = "action-guide")]
mod action_input;
```

- [ ] **Step 2: Write the failing `action_input` tests**

Create `crates/rollshot-app/src/action_input.rs` with the test block first. The fakes implement the real trait, so they exercise the exact fallback/forward logic the future recording flow will rely on:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{
        CaptureRegion, DegradedReason, InputCapability, MouseButton, SemanticAction,
        SemanticInputSource, TimedSemanticAction,
    };

    fn region() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 100, height: 80 }
    }

    /// A fake that fails to start with a chosen reason.
    struct FailingSource(DegradedReason);
    impl SemanticInputSource for FailingSource {
        fn start(&mut self, _r: CaptureRegion) -> Result<InputCapability, DegradedReason> {
            Err(self.0)
        }
        fn poll(&mut self) -> Vec<TimedSemanticAction> { Vec::new() }
        fn stop(&mut self) {}
    }

    /// A fake that starts SemanticEvents and yields one queued action once.
    #[derive(Default)]
    struct OneClickSource { drained: bool }
    impl SemanticInputSource for OneClickSource {
        fn start(&mut self, _r: CaptureRegion) -> Result<InputCapability, DegradedReason> {
            Ok(InputCapability::SemanticEvents)
        }
        fn poll(&mut self) -> Vec<TimedSemanticAction> {
            if self.drained {
                Vec::new()
            } else {
                self.drained = true;
                vec![TimedSemanticAction {
                    action: SemanticAction::Click { button: MouseButton::Left, position: None },
                    at_ms: 10,
                }]
            }
        }
        fn stop(&mut self) {}
    }

    #[test]
    fn start_failure_falls_back_to_visual_only_with_reason() {
        let mut session = ActionInputSession::new(Box::new(FailingSource(DegradedReason::PermissionDenied)));
        let cap = session.start(region());
        assert_eq!(cap, InputCapability::VisualOnly { reason: DegradedReason::PermissionDenied });
        assert_eq!(session.capability(), cap);
        // A degraded session still polls (the swapped VisualOnlySource yields nothing).
        // Build a recorder to forward into.
        let mut recorder = test_recorder();
        session.poll_into(&mut recorder); // must not panic
        session.stop();
    }

    #[test]
    fn successful_start_reports_semantic_events_and_forwards_actions() {
        let mut session = ActionInputSession::new(Box::<OneClickSource>::default());
        assert_eq!(session.start(region()), InputCapability::SemanticEvents);
        let mut recorder = test_recorder();
        // First poll forwards the one click; second is a no-op.
        session.poll_into(&mut recorder);
        session.poll_into(&mut recorder);
        session.stop();
        // The recorder consumed the event without panicking; detailed candidate
        // assertions belong to rollshot-action's own tests.
    }

    #[test]
    fn advisory_text_is_platform_appropriate_and_non_fatal() {
        let linux = degraded_advisory(DegradedReason::PermissionDenied);
        assert!(linux.to_lowercase().contains("visual-only"));
        // The macOS-vs-Linux split is chosen at compile time; just assert the
        // string is non-empty and mentions the visual-only fallback.
        assert!(!degraded_advisory(DegradedReason::NoInputDevice).is_empty());
        assert!(!degraded_advisory(DegradedReason::SourceStartFailed).is_empty());
        assert!(!degraded_advisory(DegradedReason::RuntimeFailure).is_empty());
    }

    fn test_recorder() -> rollshot_action::ActionRecorder {
        rollshot_action::ActionRecorder::new(
            region(),
            rollshot_action::StoreConfig::default(),
            rollshot_action::DetectorConfig::default(),
        )
    }
}
```

> **Verified P0a signatures (code-checked, exact):** `ActionRecorder::new(region: CaptureRegion, store: StoreConfig, det: DetectorConfig)` (`crates/rollshot-action/src/recorder.rs:39`); both `StoreConfig` (`frame_store.rs:30`) and `DetectorConfig` (`detector.rs:37`) implement `Default`. The `test_recorder()` helper above is correct as written. Do not change `rollshot-action`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide action_input`
Expected: FAIL — `ActionInputSession`/`degraded_advisory`/`create_input_source` not defined.

- [ ] **Step 4: Implement `action_input.rs`**

Add above the test module:

```rust
//! Action Guide input wiring: pick the platform semantic-input source, run its
//! start/poll/stop lifecycle, degrade to visual-only on start failure, and
//! forward privacy-filtered actions into the `rollshot-action` recorder. This
//! is the reusable seam the future Action Guide recording lifecycle calls; P0b
//! exercises it through the `action-guide` CLI probe. (See the plan's Scope
//! Boundary.)

use rollshot_action::{
    ActionRecorder, CaptureRegion, DegradedReason, InputCapability, SemanticInputSource,
    VisualOnlySource,
};

const TARGET: &str = "rollshot::action::app_input";

/// Construct the platform-appropriate semantic input source. On unsupported
/// hosts (or when no platform source is compiled in) this is a
/// `VisualOnlySource` reporting `SourceStartFailed`.
pub fn create_input_source() -> Box<dyn SemanticInputSource> {
    #[cfg(target_os = "linux")]
    {
        Box::new(rollshot_linux_input::EvdevInputSource::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(rollshot_macos_input::MacosInputSource::new())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed))
    }
}

/// Persistent advisory shown while recording/reviewing in visual-only mode.
/// Non-fatal: recording, detection, review, and export remain available
/// (spec §Recording State And Warning).
pub fn degraded_advisory(_reason: DegradedReason) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Input Monitoring is unavailable. Using visual-only step detection. \
         Open System Settings to grant Input Monitoring."
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Input events unavailable. Using visual-only step detection. See the \
         README to grant temporary input-device access."
    }
}

/// Owns the active input source and the resolved capability for one recording.
pub struct ActionInputSession {
    source: Box<dyn SemanticInputSource>,
    capability: InputCapability,
}

impl ActionInputSession {
    pub fn new(source: Box<dyn SemanticInputSource>) -> Self {
        Self {
            source,
            // Until `start`, treat as not-yet-started visual-only.
            capability: InputCapability::VisualOnly { reason: DegradedReason::SourceStartFailed },
        }
    }

    /// Start observing. On the source's `Err(reason)`, swap to a started
    /// `VisualOnlySource{reason}` so recording continues (spec §Session
    /// Lifecycle: semantic-input failure stays Recording, capability=VisualOnly).
    pub fn start(&mut self, region: CaptureRegion) -> InputCapability {
        match self.source.start(region) {
            Ok(cap) => {
                tracing::info!(target: TARGET, ?cap, "input source started");
                self.capability = cap;
            }
            Err(reason) => {
                tracing::warn!(target: TARGET, ?reason, "input source degraded to visual-only");
                let mut fallback = VisualOnlySource::new(reason);
                // VisualOnlySource::start never errors.
                let cap = fallback
                    .start(region)
                    .unwrap_or(InputCapability::VisualOnly { reason });
                self.source = Box::new(fallback);
                self.capability = cap;
            }
        }
        self.capability
    }

    pub fn capability(&self) -> InputCapability {
        self.capability
    }

    /// Drain the source and forward each action into the recorder.
    pub fn poll_into(&mut self, recorder: &mut ActionRecorder) {
        for action in self.source.poll() {
            recorder.ingest_event(action);
        }
    }

    pub fn stop(&mut self) {
        self.source.stop();
    }
}
```

> **Verified P0a signature (code-checked, exact):** `ActionRecorder::ingest_event(&mut self, ev: TimedSemanticAction)` (`crates/rollshot-action/src/recorder.rs:70`). The `poll_into` loop above is correct as written.

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-app --features action-guide action_input`
Expected: PASS (3 tests).

Run: `rtk cargo clippy -p rollshot-app --features action-guide --all-targets -- -D warnings`
Expected: no warnings. (`create_input_source` is still unused until Task 10/11 — if clippy flags it as dead code, that is resolved by the probe entry in Task 11; if you commit Task 9 alone, add `#[allow(dead_code)]` on `create_input_source` ONLY, with a `// removed in Task 11` comment, then delete the allow in Task 11.)

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/action_input.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): add action input factory, advisory, and ActionInputSession controller

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: App probe launch mode

**Files:**
- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/main.rs`

The probe is the reachable, manually-verifiable host: start the session for a placeholder full-region, log capability + advisory, poll briefly into a throwaway recorder, then stop. It needs no iced window. This makes the Task 9 wiring non-dead-code and gives the spec's manual-verification entry.

- [ ] **Step 1: Add a feature-gated launch variant**

In `crates/rollshot-app/src/launch.rs`, locate the `LaunchMode` enum (currently `Capture(InteractiveLaunchOptions)`). Add a feature-gated variant:

```rust
pub enum LaunchMode {
    Capture(rollshot_capture::InteractiveLaunchOptions),
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
}
```

`parse_launch_args` (verified at `crates/rollshot-app/src/launch.rs:47`) is generic over a consuming iterator: it pulls the program name, then a single `flag`, and errors if `flag != "--capture"`. Insert the probe check **right after `flag` is read and before the `--capture` comparison** (so the new flag is recognized, not rejected as "unknown argument"). The exact existing lines are:

```rust
    if flag != "--capture" {
        return Err(format!("unknown rollshot-app argument '{flag}'"));
    }
```

Change to:

```rust
    #[cfg(feature = "action-guide")]
    if flag == "--action-guide-probe" {
        return Ok(LaunchMode::ActionGuideProbe);
    }

    if flag != "--capture" {
        return Err(format!("unknown rollshot-app argument '{flag}'"));
    }
```

- [ ] **Step 2: Handle the variant in `run`**

In `crates/rollshot-app/src/main.rs`, the `run` function matches `LaunchMode`. Add a feature-gated arm:

```rust
match launch_mode {
    LaunchMode::Capture(options) => {
        // ... existing ...
        run_iced_capture(options)
    }
    #[cfg(feature = "action-guide")]
    LaunchMode::ActionGuideProbe => run_action_guide_probe(),
}
```

Add the probe function (feature-gated) below `run`:

```rust
#[cfg(feature = "action-guide")]
fn run_action_guide_probe() -> Result<(), String> {
    use crate::action_input::{create_input_source, degraded_advisory, ActionInputSession};
    use rollshot_action::{ActionRecorder, CaptureRegion, DetectorConfig, InputCapability, StoreConfig};

    // P0b probe: no overlay region picker yet (deferred to the app-integration
    // plan). Observe the full virtual region as a placeholder.
    let region = CaptureRegion { x: 0, y: 0, width: 1920, height: 1080 };
    let mut session = ActionInputSession::new(create_input_source());
    let capability = session.start(region);

    match capability {
        InputCapability::SemanticEvents => {
            tracing::info!(target: "rollshot::action::probe", "semantic input enabled");
            println!("Action Guide input probe: Semantic input enabled.");
        }
        InputCapability::VisualOnly { reason } => {
            tracing::warn!(target: "rollshot::action::probe", ?reason, "visual-only");
            println!("Action Guide input probe: Visual-only detection.");
            println!("{}", degraded_advisory(reason));
        }
    }

    // Poll for ~3 seconds into a throwaway recorder so semantic events are
    // observed only during this active probe, then stop.
    let mut recorder = ActionRecorder::new(region, StoreConfig::default(), DetectorConfig::default());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        session.poll_into(&mut recorder);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    session.stop();
    println!("Action Guide input probe finished.");
    Ok(())
}
```

> **Verified (code-checked):** `LaunchMode::Capture(InteractiveLaunchOptions)` (`launch.rs:5`), `pub fn parse_launch_args<I, S>(...) -> Result<LaunchMode, String>` (`launch.rs:47`), `fn run(args: Vec<String>, file_logging: bool)` with `match launch_mode { LaunchMode::Capture(options) => … }` (`main.rs:58,61`), and both `StoreConfig`/`DetectorConfig: Default`. The arms above are correct as written. Note `LaunchMode` derives `PartialEq, Eq` — the new unit variant is fine.

- [ ] **Step 3: Verify both feature states compile and the off-build exposes nothing**

Run: `rtk cargo build -p rollshot-app`
Expected: PASS (feature off — no `ActionGuideProbe`, no `action_input` module compiled).

Run: `rtk cargo build -p rollshot-app --features action-guide`
Expected: PASS (feature on — probe reachable, `create_input_source` now used so no dead-code warning).

Run: `rtk cargo clippy -p rollshot-app --features action-guide --all-targets -- -D warnings`
Expected: no warnings. (Remove the temporary `#[allow(dead_code)]` from Task 9 Step 5 now if you added it.)

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): add action-guide input probe launch mode (reachable host for P0b wiring)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: CLI `action-guide` subcommand (feature-gated)

**Files:**
- Modify: `crates/rollshot-cli/Cargo.toml`
- Modify: `crates/rollshot-cli/src/args.rs`
- Modify: `crates/rollshot-cli/src/lib.rs`
- Create: `crates/rollshot-cli/src/cmd_action_guide.rs`

The spec requires `rollshot action-guide` to exist only when the feature is built and to expose nothing otherwise. For P0b it launches the app's probe.

- [ ] **Step 1: Add the feature to the CLI manifest**

In `crates/rollshot-cli/Cargo.toml`, add:

```toml
[features]
action-guide = []
```

(The CLI launches the app as a child process / via its existing launch path, so it does not need a direct dep on the platform crates. Confirm how the CLI currently launches `rollshot-app` — if it spawns the app binary, the subcommand passes `--action-guide-probe`; if it calls an app entry directly, route accordingly.)

- [ ] **Step 2: Add the feature-gated subcommand variant**

In `crates/rollshot-cli/src/args.rs`, add to the `Command` enum:

```rust
    /// Record a desktop workflow into an Action Guide (P0b: input-capability probe).
    #[cfg(feature = "action-guide")]
    ActionGuide(ActionGuideArgs),
```

And define the args struct near the other `*Args`:

```rust
#[cfg(feature = "action-guide")]
#[derive(Debug, clap::Args)]
pub struct ActionGuideArgs {}
```

- [ ] **Step 3: Route it**

In `crates/rollshot-cli/src/lib.rs`, the routing is `match &cli.command { Command::Capture(a) => cmd_capture::run(a), … }` (verified at `lib.rs:29-32`). Add a feature-gated arm:

```rust
        #[cfg(feature = "action-guide")]
        Command::ActionGuide(a) => cmd_action_guide::run(a),
```

Add the module decl near the other `pub mod cmd_*` lines (the existing modules are declared `pub mod`; match that style):

```rust
#[cfg(feature = "action-guide")]
pub mod cmd_action_guide;
```

- [ ] **Step 4: Expose the app-binary resolver (the CLI and app are SEPARATE binaries)**

Verified facts: the CLI binary is `rollshot` and the GUI is a separate binary `rollshot-app` (`crates/rollshot-cli/Cargo.toml` / `rollshot-app/Cargo.toml` `[[bin]]` names). The CLI already resolves and spawns the app via `crates/rollshot-cli/src/cmd_capture_launcher.rs::resolve_app_binary()` (currently private `fn` at `:117`, using `ROLLSHOT_APP` env or a sibling-of-`current_exe` lookup). **Do NOT spawn `current_exe()` — that would re-exec the CLI, not the GUI.** Reuse the existing resolver.

In `crates/rollshot-cli/src/cmd_capture_launcher.rs`, change the resolver's visibility so the new command can reuse it:

```rust
pub(crate) fn resolve_app_binary() -> Result<PathBuf, CliError> {
```

- [ ] **Step 5: Implement the command**

Create `crates/rollshot-cli/src/cmd_action_guide.rs`. `CliError::new(impl Into<String>, i32)` is the verified constructor (`crates/rollshot-cli/src/cli_error.rs`, used as `CliError::new("…", 1)` throughout `cmd_capture.rs`). P0b targets Linux + macOS only, so the probe launcher does not need the Windows `cmd.exe` wrapper that `cmd_capture_launcher::run` uses.

```rust
//! `rollshot action-guide` — launch the Action Guide input-capability probe
//! (P0b). Spawns the separate `rollshot-app` GUI binary in probe mode. Replaced
//! by the full overlay → record → review → export flow in the app-integration
//! plan.

use crate::args::ActionGuideArgs;
use crate::cli_error::CliError;

pub fn run(_args: &ActionGuideArgs) -> Result<String, CliError> {
    // The app binary must be built with `--features action-guide`, or it will
    // reject `--action-guide-probe` with a clear "unknown argument" error.
    let app = crate::cmd_capture_launcher::resolve_app_binary()?;
    let status = std::process::Command::new(&app)
        .arg("--action-guide-probe")
        .status()
        .map_err(|e| CliError::new(format!("failed to launch {}: {e}", app.display()), 1))?;

    if status.success() {
        Ok("action guide input probe completed".to_string())
    } else {
        Err(CliError::new("action guide input probe failed", 1))
    }
}
```

- [ ] **Step 6: Verify both feature states**

Run: `rtk cargo build -p rollshot-cli`
Expected: PASS (feature off — no `action-guide` subcommand; `rollshot --help` lists only capture/probe/stitch-folder).

Run: `rtk cargo build -p rollshot-cli --features action-guide`
Expected: PASS (feature on — `action-guide` subcommand present).

Run: `rtk cargo clippy -p rollshot-cli --features action-guide --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-cli/
rtk git commit -m "feat(cli): add feature-gated action-guide subcommand launching the input probe

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# Part 4 — CI, docs, verification

## Task 12: Extend CI for the new crates + the `action-guide` feature

**Files:**
- Modify: `.github/workflows/ci.yml`

The spec requires each increment to build, `fmt --check`, clippy (`-D warnings`), and test the `action-guide` feature on both Linux and macOS, while a feature-off build still compiles with no new command. The new crates are workspace members so the default `cargo test --workspace` already builds/tests them; this task adds the feature-on lane and the macOS `cargo check` entries.

- [ ] **Step 1: Add a feature-on clippy + test step (both OSes)**

In `.github/workflows/ci.yml`, after the existing `cargo test --workspace` step (around line 43), add:

```yaml
      - name: Clippy (action-guide feature)
        run: cargo clippy --workspace --all-targets --features rollshot-cli/action-guide,rollshot-app/action-guide -- -D warnings

      - name: Test (action-guide feature)
        run: cargo test --workspace --features rollshot-cli/action-guide,rollshot-app/action-guide
```

> The default `test`/`clippy` steps (no `--features`) already cover the feature-off build and prove "no new command exposed." These new steps run on the existing `[ubuntu-24.04, macos-14]` matrix, satisfying "both Linux and macOS hosts."

- [ ] **Step 2: Add the new crates to the macOS check list**

In the macOS-only `cargo check` block (around line 45-52), add:

```yaml
          cargo check -p rollshot-macos-input --all-targets
          cargo check -p rollshot-linux-input --all-targets
```

(`rollshot-macos-input`'s FFI path compiles only here; `rollshot-linux-input` is checked as a stub on macOS, confirming the cross-OS stub compiles.)

- [ ] **Step 3: Verify the tracing-target check still passes**

The CI runs `./scripts/check-tracing-targets.sh`. All new tracing events use stable explicit `rollshot::action::*` targets (`linux_input`, `macos_input`, `app_input`, `probe`). 

Run: `rtk ./scripts/check-tracing-targets.sh`
Expected: PASS (no bare-target events introduced).

- [ ] **Step 4: Verify the full feature-on workspace locally (Linux)**

Run: `rtk cargo test --workspace --features rollshot-cli/action-guide,rollshot-app/action-guide`
Expected: PASS.

Run: `rtk cargo clippy --workspace --all-targets --features rollshot-cli/action-guide,rollshot-app/action-guide -- -D warnings`
Expected: no warnings.

Run: `rtk cargo fmt --check`
Expected: no diff.

- [ ] **Step 5: Commit**

```bash
rtk git add .github/workflows/ci.yml
rtk git commit -m "ci: build/clippy/test action-guide feature and macOS input crate on both hosts

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: README — evdev ACL setup + macOS Input Monitoring

**Files:**
- Modify: `README.md`

The Linux source relies on user-granted, temporary read access to `/dev/input/event*`. The spec (§Linux, §Privacy And Security) mandates documenting identification, `setfacl` grant, verification, removal, the security consequence, and the reboot/device-recreation caveat. Rollshot never invokes `sudo`/`pkexec`/Polkit.

- [ ] **Step 1: Add an "Action Guide input access" section to `README.md`**

Add a new section (place it near other platform-setup notes). Use this content verbatim:

````markdown
## Action Guide input access (optional)

Action Guide works in **visual-only** mode out of the box. Granting temporary
input-device access upgrades it to **semantic** detection (clicks, scroll,
typing, Enter/Tab improve step timing). Input is observed **only** while an
Action Guide recording is active, and Rollshot never persists raw key codes,
typed text, device names, or device paths.

### Linux (KDE Wayland and others)

Rollshot reads kernel input devices directly via evdev; it does **not** use
`sudo`, `pkexec`, Polkit, or a privileged daemon. You grant your own user
temporary read access with an ACL.

> ⚠️ **Security warning:** read access to `/dev/input/event*` lets *any* process
> running as your user observe **all** keyboard and pointer activity, including
> passwords typed into other applications. Grant it only while you need it and
> remove it afterward. ACLs may disappear after a reboot or when a device is
> recreated (e.g. replugging a keyboard), and may need to be reapplied.

1. **Identify your input devices:**

   ```bash
   cat /proc/bus/input/devices   # find your keyboard/mouse "Handlers=... eventN"
   # or:
   ls -l /dev/input/by-id/
   ```

2. **Grant your user temporary read access** (replace `eventN`):

   ```bash
   sudo setfacl -m u:$USER:r /dev/input/eventN
   ```

3. **Verify access:**

   ```bash
   getfacl /dev/input/eventN     # should list user:<you>:r--
   # quick read test (Ctrl-C to stop):
   head -c 1 /dev/input/eventN >/dev/null && echo "readable"
   ```

4. **Remove the ACL when done:**

   ```bash
   sudo setfacl -x u:$USER /dev/input/eventN
   ```

If no device is readable, Action Guide stays in visual-only mode and shows a
persistent advisory — recording, detection, review, and export still work.

### macOS

Semantic input uses **Input Monitoring** (System Settings → Privacy & Security →
Input Monitoring). Rollshot requests it just-in-time when an Action Guide
recording starts; it never requests Accessibility or input injection. If you
deny it, Action Guide stays in visual-only mode with an advisory and an **Open
System Settings** shortcut. macOS may require restarting Rollshot before a newly
granted permission takes effect.

**Screen Recording** permission is separate and **required** to capture frames —
denying it is a capture failure, not a visual-only degradation.
````

- [ ] **Step 2: Commit**

```bash
rtk git add README.md
rtk git commit -m "docs(readme): document Action Guide evdev ACL setup and macOS Input Monitoring

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 14: Manual platform verification (no code; record results)

Native permission behavior cannot be reliably tested in CI (spec §Platform Input Tests: "Native permission behavior requires manual platform verification"). Perform these on real hardware and note the outcome in the PR description.

- [ ] **Linux — visual-only path (no ACL):** with no input ACL granted, run `rollshot action-guide` (feature build). Verify the probe prints "Visual-only detection." plus the README advisory, and exits cleanly. Confirm `tracing` logs show `DegradedReason::PermissionDenied` or `NoInputDevice` (privacy-safe — no key values).

- [ ] **Linux — semantic path (with ACL):** grant the documented `setfacl` ACL on your keyboard+mouse event devices, rerun the probe, and within the 3s window click, scroll, type, and press Enter/Tab. Verify it prints "Semantic input enabled." and logs a non-zero observed-event count with **no** key values, text, or device names in the logs. Then remove the ACL with the documented command and confirm it degrades again.

- [ ] **macOS — Screen Recording vs Input Monitoring independence:** confirm (via the eventual capture path / existing capture) that Screen Recording denial is fatal while Input Monitoring denial only degrades the probe to visual-only.

- [ ] **macOS — Input Monitoring grant:** with Input Monitoring denied, run the probe; verify the visual-only advisory + that `Open System Settings` opens the Input Monitoring pane. Grant it (restart Rollshot if prompted), rerun, and verify "Semantic input enabled." and that events are observed **only** during the active 3s probe window. Verify the tap re-enables after an idle period > 5s (TapDisabledByTimeout path) by idling mid-probe then acting.

- [ ] **Both — privacy audit:** inspect the probe's `tracing` output and confirm it never contains typed text, raw key codes, click coordinates, device names, or device paths (only capability, source category, reason, counts, lifecycle).

---

## Self-Review (completed during planning)

**Spec coverage:**
- §Implementation Increments P0b (two crates, each implements P0a trait, wired into app, no `rollshot-action` change) → Tasks 1-11. ✅ App-wiring boundary explicitly scoped (Scope Boundary section).
- §`rollshot-linux-input` (evdev discovery, read-only readers, semantic classification, no device metadata, drop to visual-only) → Tasks 2-3. ✅
- §`rollshot-macos-input` (unsafe-isolation, listen-only tap, permission status/request/open-settings, explicit failure reasons, no raw handles/injection) → Tasks 4-7. ✅
- §macOS (HIDEventTap/HeadInsert/ListenOnly, dedicated CFRunLoop thread, mask, no Unicode, re-enable on timeout, teardown order, null-tap → visual-only, ListenEvent ≠ Accessibility) → Tasks 6-7. ✅
- §Privacy And Security (semantic classification boundary, no raw codes/text/device identity, diagnostics record only capability/category/reason/counts/outcome) → enforced in classify cores + tracing-target choices; Task 14 audit. ✅
- §Platform Input Tests (evdev classification w/o metadata; no-device/permission-denied/reader-failure → visual-only; macOS permission API distinguishes ListenEvent; tap callbacks classify w/o Unicode; tap timeout re-enabled; null-tap/runtime → visual-only) → Tasks 2,3,5,6,7 unit tests + Task 14 manual. ✅
- §Recording State And Warning (Semantic vs Visual-only label; persistent platform-specific advisory; non-fatal) → `degraded_advisory` + probe output (Tasks 9-10). ✅
- §Failure Handling (typed errors; semantic-input start failure → `DegradedReason`, non-fatal) → `start()` returns `Result<_, DegradedReason>`; `ActionInputSession` fallback. ✅
- CI extension (build/fmt/clippy/test the feature on both hosts; feature-off compiles, no new command; macOS unsafe crate added) → Task 12. ✅
- README Linux ACL (identify/grant/verify/remove/security/reboot caveat; no sudo/Polkit) + macOS Input Monitoring → Task 13. ✅

**Known deferrals (stated, not gaps):** `Workflow::ActionGuide` + overlay routing + `is_supported()` rejection; `SendFrameStream` lift + frame-reader; Timeline Workspace/export UI; mid-session `RuntimeFailure` capability downgrade (needs a trait change the spec forbids in P0b); macOS absolute click position. All belong to the app-integration plan.

**Type consistency:** `SemanticInputSource::{start,poll,stop}`, `TimedSemanticAction`, `SemanticAction`/`MouseButton`/`SemanticKey`, `CaptureRegion`, `InputCapability`, `DegradedReason` used verbatim from the verified `rollshot-action` source. `ActionRecorder::new`/`ingest_event`, `StoreConfig`/`DetectorConfig` flagged with explicit "confirm against P0a source" checks in Tasks 9-10 (the one place this plan touches P0a APIs it did not define).

**Placeholder scan:** FFI call sites in Tasks 6-7 carry explicit "verify against the published `objc2-core-graphics`/`objc2-core-foundation`/`evdev` bindings on the target host and adjust ONLY this function" notes — these are genuine API-version unknowns for an unsafe-isolation crate whose FFI is manually verified on macOS, not vague placeholders; the behavioral contract (event mask, tap options, field choices, classification) is fully specified and CI-tested via the pure cores.

---

## Eng Review Notes (applied 2026-06-15)

### Fixes applied during review

| # | Issue | Where | Fix |
|---|-------|-------|-----|
| 1 | Reader threads kept observing input after `stop()` (privacy violation + zombie threads + unbounded queue) | Task 3 | Non-blocking evdev reads + poll-sleep loop checking `stop`; `stop()` now **joins** threads; bounded drop-oldest queue. New Design Decision 7. |
| 2 | `current_exe()` spawn would re-exec the **CLI**, not the GUI (separate binaries `rollshot` vs `rollshot-app`) | Task 11 | Reuse `cmd_capture_launcher::resolve_app_binary()` (bumped to `pub(crate)`); spawn `rollshot-app --action-guide-probe`. |
| 3 | Wrong `CliError` API (`::from`) | Task 11 | Use verified `CliError::new(msg, 1)` + `use crate::cli_error::CliError`. |
| 4 | `parse_launch_args` edit didn't match the real consuming-iterator code | Task 10 | Concrete edit inserting the probe check after `flag` is read, before the `--capture` comparison. |
| 5 | `extern "C-unwind"` tap callback = UB on unwind through CoreFoundation | Task 7 | `extern "C"` (abort-on-unwind) + panic-free body. |
| 6 | `TapDisabledByTimeout` re-enable was punted to "verification" though spec requires it | Task 7 | Tap handle stored in `CallbackCtx.tap` after creation; callback re-enables in the timeout branch. |
| 7 | Unbounded input queue (spec mandates fixed bounds) | Tasks 3, 7 | `MAX_QUEUED = 4096` drop-oldest `VecDeque`; unit tests added in both crates. |
| 8 | "Confirm against P0a" notes left as unknowns | Tasks 9, 10 | All P0a signatures code-verified and made definitive (`ActionRecorder::new`/`ingest_event`, `StoreConfig`/`DetectorConfig: Default`). |
| 9 | Cross-OS feature/target-dep resolution risk under-specified | Task 8 | Strengthened note + concrete fallback, with the cross-OS check pinned to CI (Task 12). |

### What already exists (reused, not rebuilt)

- `rollshot_action::SemanticInputSource` + `VisualOnlySource` + all models (P0a, #46) — implemented/consumed, never duplicated.
- `cmd_capture_launcher::resolve_app_binary()` (`ROLLSHOT_APP` env + sibling-of-exe lookup, with tests) — reused by the probe command instead of a new resolver.
- `rollshot-macos-oneshot`'s unsafe-isolation pattern (`[lints.rust] unsafe_code = "allow"`, non-platform stub, `#[ignore]` live test) — mirrored by `rollshot-macos-input`.
- The existing `[ubuntu-24.04, macos-14]` CI matrix and macOS `cargo check` block — extended, not replaced.

(The deferred app-integration surface — `Workflow::ActionGuide`, overlay region-only path, `SendFrameStream` lift, Timeline Workspace — is enumerated in the **Scope Boundary** table above; that doubles as the "NOT in scope" list.)

### Failure-mode coverage

| New codepath | Realistic failure | Test | Error handling | User sees |
|--------------|-------------------|------|----------------|-----------|
| evdev `start` | no readable device / ACL missing | Task 3 stub + Task 14 manual | `Err(NoInputDevice/PermissionDenied)` → `VisualOnly` | advisory (non-fatal) |
| evdev reader | thread dies mid-session | — (documented limitation) | logged; thread exits; poll empties | semantic events silently stop (non-fatal, degraded) |
| CGEventTap `start` | permission denied / null tap | Task 14 manual | `Err(PermissionDenied/SourceStartFailed)` → `VisualOnly` | advisory + Open Settings |
| CGEventTap runtime | `TapDisabledByTimeout` | Task 14 manual (idle) | callback re-enables tap | uninterrupted |
| input queue | consumer stalls | Tasks 3, 7 unit | `MAX_QUEUED` drop-oldest | bounded memory |
| `ActionInputSession::start` | source `Err` | Task 9 unit | swap to `VisualOnlySource{reason}` | advisory |
| CLI probe | app binary missing | launcher tests | `resolve_app_binary` → `CliError` | clear error |
| CLI probe | app built without feature | — | app rejects flag → non-zero exit | "probe failed" + app's "unknown argument" |

**Accepted (non-critical) limitation:** mid-session evdev/tap reader death has no test and no live capability-downgrade — it is logged, non-fatal, and explicitly out of P0b scope (surfacing it needs a `SemanticInputSource` trait change the spec forbids in P0b). Owned by the app-integration plan. Not a silent data-loss gap.

### Parallelization (for subagent-driven execution)

**Task 1 modifies the root `Cargo.toml` `members` list (adds BOTH crates) → it serializes everything and must land first.**

| Task | Modules touched | Depends on |
|------|-----------------|------------|
| 1 | root `Cargo.toml`, `crates/rollshot-linux-input/` | — |
| 2, 3 | `crates/rollshot-linux-input/` | 1 (→2→3) |
| 4, 5, 6, 7 | `crates/rollshot-macos-input/` | 1 (→4→5→6→7) |
| 8 | `crates/rollshot-app/Cargo.toml` | 3, 7 |
| 9, 10 | `crates/rollshot-app/src/` | 8 (→9→10) |
| 11 | `crates/rollshot-cli/` | 10 |
| 12 | `.github/workflows/ci.yml` | all code tasks |
| 13 | `README.md` | — |
| 14 | (manual, no files) | all |

**Lanes:**
- `Lane A: Task 2 → Task 3` (sequential, both `rollshot-linux-input/`)
- `Lane B: Task 4 → Task 5 → Task 6 → Task 7` (sequential, all `rollshot-macos-input/`)
- `Lane C: Task 13` (independent docs)

**Execution order:** Task 1 alone (root manifest). → Launch **Lane A ∥ Lane B ∥ Lane C** in parallel worktrees (no shared modules → no conflicts). → Merge. → Task 8 → Task 9 → Task 10 → Task 11 (sequential; 9/10 share `main.rs`). → Task 12. → Task 14 (manual, on real hardware).

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-15-action-guide-p0b-platform-input.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration. Note: Tasks 6-7 (macOS FFI) and Task 14 (manual) need a macOS host for full verification; on a Linux dev host they land with the pure cores tested + the stub paths green, and the FFI is confirmed on the macOS CI runner / by you.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints for review.

Which approach?
