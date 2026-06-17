# Action Guide Fullscreen Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `rollshot-app --action-guide --fullscreen` (Linux/KDE) that records the whole display, is stopped by clicking a temporary system-tray icon, and opens the existing Action Guide Timeline Workspace.

**Architecture:** A new **headless** runner `run_action_guide_fullscreen` in `rollshot-iced-overlay` (no layer-shell overlay, no iced `Application`). It reuses the existing scrolling acquisition path (`acquire_resource(Workflow::ActionGuide, ..)` + `real_factories()`) to get a streaming `Driver`, runs the existing `begin_action_recording`/`finalize_action` against the full-display region, and blocks on a tray-click channel. The tray is a `ksni` StatusNotifierItem (KDE-native SNI, no GTK, no second event loop) behind a `RecordingTray` trait seam so CI can unit-test orchestration with a fake.

**Tech Stack:** Rust, iced 0.14 (not used by this path directly), `ksni` (SNI tray), `notify-rust` (Freedesktop notifications), `zbus` 4.4 (SNI-host probe), existing `rollshot-action` recorder.

## Global Constraints

- Linux-only feature. macOS/Windows out of scope. All new code is gated behind `#[cfg(all(target_os = "linux", feature = "action-guide"))]` to match existing Action Guide gating.
- `rollshot-iced-overlay` sets `unsafe_code = "deny"` — all new deps/code must be safe.
- All runtime diagnostics use `tracing` with an explicit `rollshot::*` target and structured fields. No `println!`/`eprintln!`/`dbg!` (except pre-subscriber/intentional stderr).
- `--action-guide` alone keeps existing region behavior (`fullscreen = false`). Only `--action-guide --fullscreen` enables this path.
- Tray icon lifecycle is bound to the recording session; destroyed on every exit path (success, error, cancel) via RAII `Drop`.
- The tray is the only stop mechanism in P0. If no SNI host exists, return an error **before** acquiring any capture resource.
- Verification per task: `rtk cargo test -p <crate> --features action-guide`, `rtk cargo fmt --check`, and `rtk cargo clippy -p <crate> --all-targets --features action-guide -- -D warnings`.
- Spec of record: `docs/superpowers/specs/2026-06-17-action-guide-fullscreen-design.md`.

---

## File Structure

- Modify: `crates/rollshot-capture/src/types.rs` — add `CaptureRequest::action_guide_fullscreen()`; allow `(ActionGuide, Fullscreen)` in `is_supported()`; tests.
- Modify: `crates/rollshot-cli/src/args.rs` — add `fullscreen: bool` to `ActionGuideArgs`.
- Modify: `crates/rollshot-cli/src/cmd_action_guide.rs` — forward `--fullscreen` to `rollshot-app`.
- Modify: `crates/rollshot-app/src/launch.rs` — `LaunchMode::ActionGuide { fullscreen: bool }`; parse `--action-guide --fullscreen`.
- Modify: `crates/rollshot-app/src/main.rs` — branch `run_action_guide_record` on `fullscreen`.
- Create: `crates/rollshot-iced-overlay/src/recording_tray.rs` — `RecordingTray` trait, `ksni` impl, SNI-host probe, notification helper.
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs` — `run_action_guide_fullscreen` orchestration (generic inner + real wiring); reuse `acquire_resource`.
- Modify: `crates/rollshot-iced-overlay/src/lib.rs` — declare module; re-export `run_action_guide_fullscreen`.
- Modify: `crates/rollshot-iced-overlay/Cargo.toml` — add `ksni`, `notify-rust`, `zbus` under the Linux + action-guide gate.

---

## Task 1: Spike — pin `ksni`/`zbus`/`notify-rust` versions + API (headless, autonomous)

This is an **investigation task** (no TDD) that runs fully on a **headless Linux server — no desktop required**. It pins the exact dependency versions and API signatures so Tasks 5/6 compile first time, and it confirms the one genuinely environment-independent risk: that `ksni` and the workspace `zbus = "4.4"` agree on a single `zbus` major version. The runtime behaviors that *do* need a real KDE desktop (live tray, live capture) are deliberately **not** here — they are batched into the final **Task 8 (manual, requires KDE)**.

> Why split: this repo's host is a headless TTY (no Wayland/portal/pipewire/SNI host). The dependency/API/compile risks are resolvable here and now; only live-runtime behavior must wait for a desktop. See Task 8.

**Files:**
- Create (throwaway, do NOT commit): `crates/rollshot-iced-overlay/examples/spike_tray_api.rs`

- [ ] **Step 1: Resolve versions and check the `zbus` alignment (the critical risk).**

Temporarily add `ksni`, `notify-rust`, and `zbus = { workspace = true }` to the crate (as Task 5 will), then resolve and inspect the dependency tree:

```bash
rtk cargo add ksni notify-rust --package rollshot-iced-overlay
rtk cargo tree -p rollshot-iced-overlay -i zbus
```
Expected: a SINGLE `zbus` major version in the tree. If `ksni` pulls an older `zbus` (e.g. 3.x) alongside the workspace 4.4, that is the blocker to resolve NOW: pick a `ksni` version built on `zbus` 4, or plan to run the SNI-host probe through `ksni`'s re-exported `zbus` instead of the workspace dep. Record the chosen `ksni` version.

- [ ] **Step 2: Compile-check the real tray/probe/notify code (no execution needed).**

Write a throwaway example containing the exact code Task 5 will ship — the `ksni::Tray` impl, the `zbus::blocking` host probe, and the `notify_rust` call — and **build** it (do not run). This proves the API signatures are correct against the resolved versions, which is the whole point of pinning:

```rust
// examples/spike_tray_api.rs (throwaway) — BUILD ONLY, never run here.
// Build: cargo build -p rollshot-iced-overlay --example spike_tray_api --features action-guide
// Paste the Task 5 Step 6 bodies of: RecordingItem (ksni::Tray impl),
// sni_host_available (zbus::blocking probe), notify_recording_started.
// Goal: `cargo build` succeeds => signatures are correct for the pinned versions.
fn main() {}
```

Run: `rtk cargo build -p rollshot-iced-overlay --example spike_tray_api --features action-guide`
Expected: PASS = compiles. Any error here means the `ksni::Tray` method set, `TrayService`/`Handle` names, `zbus::blocking::Proxy` signature, or `notify_rust::Hint` variant differs from the plan — **fix the plan's Task 5 code to match before proceeding**.

- [ ] **Step 3: Smoke-test the negative SNI path (server-doable).**

The headless server has a session bus but no SNI host, so `sni_host_available()` should return `false` cleanly (no panic). Add a `println!` of the result to the example `main`, run it once, confirm `false`:

Run: `rtk cargo run -p rollshot-iced-overlay --example spike_tray_api --features action-guide`
Expected: prints `false` (or logs "no registered StatusNotifierHost"), exits cleanly. This validates the "no tray → return error before capture" hard requirement's detection logic on a real (host-less) bus.

- [ ] **Step 4: Delete the spike example.**

```bash
rm crates/rollshot-iced-overlay/examples/spike_tray_api.rs
rtk git checkout crates/rollshot-iced-overlay/Cargo.toml   # undo the temp `cargo add`
```
No commit (spike is throwaway). Carry the pinned versions + confirmed signatures into Task 5.

> **Deferred to Task 8 (needs KDE desktop):** that the headless `Driver` actually acquires a fullscreen stream off the iced loop, and that the tray appears + click works on a live SNI host. Task 6 is therefore written against the documented assumption that the headless `Driver` works; if Task 8 disproves it, Task 6's acquisition gains a `pollster::block_on` wrapper (noted inline in Task 6).

---

## Task 2: Capture request — `action_guide_fullscreen()` + `is_supported`

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs` (constructors ~line 84; `is_supported` ~line 98; tests ~line 202)

**Interfaces:**
- Produces: `CaptureRequest::action_guide_fullscreen() -> CaptureRequest` (const); `(ActionGuide, Fullscreen)` now returns `true` from `is_supported()`; `needs_overlay()` returns `false` for it (already true via existing logic — locked by a regression test).

- [ ] **Step 1: Write the failing tests.**

Add to the `#[cfg(test)] mod tests` in `crates/rollshot-capture/src/types.rs`. Note the existing test module imports `use super::{CaptureOptions, CaptureRequest, InteractiveLaunchOptions};` — extend the import to include `CaptureScope, Workflow`.

```rust
#[test]
fn action_guide_fullscreen_request_shape() {
    let req = CaptureRequest::action_guide_fullscreen();
    assert_eq!(req.workflow, Workflow::ActionGuide);
    assert_eq!(req.scope, CaptureScope::Fullscreen);
}

#[test]
fn action_guide_fullscreen_is_supported() {
    assert!(CaptureRequest::action_guide_fullscreen().is_supported());
}

#[test]
fn action_guide_fullscreen_needs_no_overlay() {
    assert!(!CaptureRequest::action_guide_fullscreen().needs_overlay());
}

#[test]
fn scrolling_fullscreen_still_unsupported() {
    let req = CaptureRequest {
        workflow: Workflow::Scrolling,
        scope: CaptureScope::Fullscreen,
    };
    assert!(!req.is_supported());
}
```

- [ ] **Step 2: Run tests to verify they fail.**

Run: `rtk cargo test -p rollshot-capture action_guide_fullscreen`
Expected: FAIL — `no function or associated item named action_guide_fullscreen` (and `is_supported` assertion fails once the constructor exists).

- [ ] **Step 3: Add the constructor.**

In `impl CaptureRequest`, after `action_guide_region()` (line 84-89):

```rust
    pub const fn action_guide_fullscreen() -> Self {
        Self {
            workflow: Workflow::ActionGuide,
            scope: CaptureScope::Fullscreen,
        }
    }
```

- [ ] **Step 4: Allow the combination in `is_supported()`.**

Change `is_supported()` (line 98-104) to drop `ActionGuide × Fullscreen` from the unsupported set, and update the doc comment:

```rust
    /// `Scrolling × Fullscreen` is expressible but not wired. `ActionGuide ×
    /// Fullscreen` is supported via the headless fullscreen runner.
    pub fn is_supported(&self) -> bool {
        !matches!(
            (self.workflow, self.scope),
            (Workflow::Scrolling, CaptureScope::Fullscreen)
        )
    }
```

- [ ] **Step 5: Run tests to verify they pass.**

Run: `rtk cargo test -p rollshot-capture && rtk cargo fmt --check`
Expected: PASS (all four new tests + existing).

- [ ] **Step 6: Commit.**

```bash
git add crates/rollshot-capture/src/types.rs
git commit -m "feat(capture): add action_guide_fullscreen request"
```

---

## Task 3: CLI — forward `--fullscreen`

**Files:**
- Modify: `crates/rollshot-cli/src/args.rs:95-97` (`ActionGuideArgs`)
- Modify: `crates/rollshot-cli/src/cmd_action_guide.rs`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `ActionGuideArgs { fullscreen: bool }`; `cmd_action_guide::run` appends `--fullscreen` to the `rollshot-app` invocation when set.

- [ ] **Step 1: Write the failing test for arg construction.**

Add a test module to `crates/rollshot-cli/src/cmd_action_guide.rs` that asserts the extra args helper (introduced in Step 3) produces the right flag list:

```rust
#[cfg(test)]
mod tests {
    use super::extra_args;
    use crate::args::ActionGuideArgs;

    #[test]
    fn region_mode_passes_only_action_guide() {
        let args = ActionGuideArgs { fullscreen: false };
        assert_eq!(extra_args(&args), vec!["--action-guide".to_string()]);
    }

    #[test]
    fn fullscreen_mode_appends_flag() {
        let args = ActionGuideArgs { fullscreen: true };
        assert_eq!(
            extra_args(&args),
            vec!["--action-guide".to_string(), "--fullscreen".to_string()]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails.**

Run: `rtk cargo test -p rollshot-cli --features action-guide extra_args`
Expected: FAIL — `cannot find function extra_args` and `ActionGuideArgs` has no field `fullscreen`.

- [ ] **Step 3: Add the field and the helper, wire the command.**

In `crates/rollshot-cli/src/args.rs` (lines 95-97):

```rust
#[cfg(feature = "action-guide")]
#[derive(Debug, clap::Args)]
pub struct ActionGuideArgs {
    /// Record the whole display instead of selecting a region. The recording is
    /// stopped by clicking the temporary system-tray icon (Linux/KDE only).
    #[arg(long, default_value_t = false)]
    pub fullscreen: bool,
}
```

Rewrite `crates/rollshot-cli/src/cmd_action_guide.rs`:

```rust
//! `rollshot action-guide` — launch the Action Guide recording overlay.
//! Spawns the separate `rollshot-app` GUI binary in record mode.

use crate::args::ActionGuideArgs;
use crate::cli_error::CliError;

/// Flags forwarded to the `rollshot-app` binary for this invocation.
fn extra_args(args: &ActionGuideArgs) -> Vec<String> {
    let mut out = vec!["--action-guide".to_string()];
    if args.fullscreen {
        out.push("--fullscreen".to_string());
    }
    out
}

pub fn run(args: &ActionGuideArgs) -> Result<String, CliError> {
    let app = crate::cmd_capture_launcher::resolve_app_binary()?;
    let status = std::process::Command::new(&app)
        .args(extra_args(args))
        .status()
        .map_err(|e| CliError::new(format!("failed to launch {}: {e}", app.display()), 1))?;

    if status.success() {
        Ok("action guide recording completed".to_string())
    } else {
        Err(CliError::new("action guide recording failed", 1))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass.**

Run: `rtk cargo test -p rollshot-cli --features action-guide && rtk cargo fmt --check`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/rollshot-cli/src/args.rs crates/rollshot-cli/src/cmd_action_guide.rs
git commit -m "feat(cli): forward --fullscreen to action-guide"
```

---

## Task 4: App launch parsing — `LaunchMode::ActionGuide { fullscreen }`

**Files:**
- Modify: `crates/rollshot-app/src/launch.rs` (enum line 9-10; parse line 65-68; existing tests reference the variant at lines 132-133, 152-153)

**Interfaces:**
- Produces: `LaunchMode::ActionGuide { fullscreen: bool }`; `parse_launch_args` accepts `--action-guide` (→ `fullscreen: false`) and `--action-guide --fullscreen` (→ `fullscreen: true`).
- Consumes: nothing from other tasks (the variant is self-contained).

- [ ] **Step 1: Write the failing tests.**

Add to `crates/rollshot-app/src/launch.rs` `mod tests`:

```rust
    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_without_fullscreen() {
        let mode = parse_launch_args(["rollshot-app", "--action-guide"]).expect("parse");
        assert!(matches!(mode, LaunchMode::ActionGuide { fullscreen: false }));
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_with_fullscreen() {
        let mode = parse_launch_args(["rollshot-app", "--action-guide", "--fullscreen"])
            .expect("parse");
        assert!(matches!(mode, LaunchMode::ActionGuide { fullscreen: true }));
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_rejects_unknown_trailing_flag() {
        let err = parse_launch_args(["rollshot-app", "--action-guide", "--bogus"])
            .expect_err("unknown trailing flag");
        assert!(err.contains("unknown"), "err = {err}");
    }
```

- [ ] **Step 2: Run tests to verify they fail.**

Run: `rtk cargo test -p rollshot-app --features action-guide action_guide_`
Expected: FAIL — variant `ActionGuide` is a unit variant (no `fullscreen` field), pattern match errors.

- [ ] **Step 3: Add the field to the enum.**

In `crates/rollshot-app/src/launch.rs` (lines 9-10):

```rust
    #[cfg(feature = "action-guide")]
    ActionGuide { fullscreen: bool },
```

- [ ] **Step 4: Parse the flag.**

Replace the existing `--action-guide` branch (lines 65-68) with one that consumes an optional trailing `--fullscreen` and rejects anything else:

```rust
    #[cfg(feature = "action-guide")]
    if flag == "--action-guide" {
        let fullscreen = match args.next() {
            None => false,
            Some(next) if next == "--fullscreen" => true,
            Some(other) => {
                return Err(format!("unknown argument after --action-guide: '{other}'"));
            }
        };
        if let Some(extra) = args.next() {
            return Err(format!("unexpected argument after --fullscreen: '{extra}'"));
        }
        return Ok(LaunchMode::ActionGuide { fullscreen });
    }
```

- [ ] **Step 5: Fix the two existing `unreachable!` match arms.**

The existing tests at lines 130-133 and 151-154 match `LaunchMode::ActionGuide =>`. Update both unit-variant arms to the struct form:

```rust
            #[cfg(feature = "action-guide")]
            LaunchMode::ActionGuide { .. } => unreachable!("test expects Capture mode"),
```

- [ ] **Step 6: Run tests to verify they pass.**

Run: `rtk cargo test -p rollshot-app --features action-guide && rtk cargo fmt --check`
Expected: PASS.

- [ ] **Step 7: Commit.**

```bash
git add crates/rollshot-app/src/launch.rs
git commit -m "feat(app): parse --action-guide --fullscreen launch flag"
```

---

## Task 5: Overlay — `RecordingTray` seam, SNI-host probe, notification helper

**Files:**
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Create: `crates/rollshot-iced-overlay/src/recording_tray.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs` (declare module ~line 62-64 area)

> Use the exact `ksni`/`zbus` versions and API signatures pinned by the Task 1 spike. The code below is written against `ksni` 0.2 with a dedicated-thread spawn; if the spike found different signatures, adapt the `ksni::Tray` impl and spawn call accordingly (the `RecordingTray` trait and tests stay the same).

**Interfaces:**
- Produces:
  - `pub(crate) trait RecordingTray: Send { fn wait_for_finish(&self); }` (concrete impls tear down on `Drop`).
  - `pub(crate) fn sni_host_available() -> bool`
  - `pub(crate) fn create_recording_tray() -> Result<Box<dyn RecordingTray>, OverlayError>`
  - `pub(crate) fn notify_recording_started()` (best-effort; never errors out of the function).

- [ ] **Step 1: Add dependencies.**

In `crates/rollshot-iced-overlay/Cargo.toml`, add a Linux-gated optional block and extend the `action-guide` feature. After the existing `[target.'cfg(target_os = "linux")'.dependencies]` block (lines 18-20):

```toml
[target.'cfg(target_os = "linux")'.dependencies.ksni]
version = "0.2"
optional = true

[target.'cfg(target_os = "linux")'.dependencies.notify-rust]
version = "4"
optional = true

[target.'cfg(target_os = "linux")'.dependencies.zbus]
workspace = true
optional = true
```

Update the feature (line 16):

```toml
action-guide = ["dep:rollshot-action", "dep:ksni", "dep:notify-rust", "dep:zbus"]
```

> If the spike showed `ksni` requires its own `zbus`, drop the `dep:zbus` line and route `sni_host_available` through `ksni`'s re-export instead.

- [ ] **Step 2: Write the failing tests (fake tray + cleanup semantics).**

Create `crates/rollshot-iced-overlay/src/recording_tray.rs` with ONLY the trait and tests first:

```rust
//! Temporary system-tray (SNI) item + notification used by the headless
//! fullscreen Action Guide runner to signal "finish recording".
//!
//! The runner is generic over [`RecordingTray`] so orchestration (ordering and
//! cleanup) is unit-tested on CI with a fake — CI has no SNI host or DBus.

use crate::OverlayError;

/// A temporary recording tray item. The concrete impl tears the item down on
/// `Drop`, so the runner gets RAII cleanup on every exit path.
pub(crate) trait RecordingTray: Send {
    /// Block the calling thread until the user activates (clicks) the tray item.
    fn wait_for_finish(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct FakeTray {
        dropped: Arc<AtomicBool>,
        waited: Arc<AtomicBool>,
    }
    impl RecordingTray for FakeTray {
        fn wait_for_finish(&self) {
            self.waited.store(true, Ordering::SeqCst);
        }
    }
    impl Drop for FakeTray {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn fake_tray_drops_and_waits() {
        let dropped = Arc::new(AtomicBool::new(false));
        let waited = Arc::new(AtomicBool::new(false));
        {
            let tray: Box<dyn RecordingTray> = Box::new(FakeTray {
                dropped: dropped.clone(),
                waited: waited.clone(),
            });
            tray.wait_for_finish();
            assert!(waited.load(Ordering::SeqCst));
            assert!(!dropped.load(Ordering::SeqCst), "not dropped until scope end");
        }
        assert!(dropped.load(Ordering::SeqCst), "dropped at scope end");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail (compile-then-pass gate).**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide recording_tray`
Expected: FAIL — `recording_tray` module not declared in `lib.rs` yet (unresolved module).

- [ ] **Step 4: Declare the module.**

In `crates/rollshot-iced-overlay/src/lib.rs`, alongside the other Linux modules (near line 63):

```rust
#[cfg(all(target_os = "linux", feature = "action-guide"))]
mod recording_tray;
```

- [ ] **Step 5: Run the module test to confirm GREEN.**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide recording_tray`
Expected: PASS (`fake_tray_drops_and_waits`).

- [ ] **Step 6: Implement the real `ksni` tray, SNI probe, and notification.**

Append to `crates/rollshot-iced-overlay/src/recording_tray.rs` (above the `#[cfg(test)]` module). Diagnostics use `tracing` with target `rollshot::overlay::tray`:

```rust
use std::sync::mpsc::{Receiver, Sender};

const TARGET_TRAY: &str = "rollshot::overlay::tray";

/// Returns true if a StatusNotifierWatcher with a registered host is present on
/// the session bus (KDE Plasma always has one). Used as a hard precondition so
/// the runner errors out *before* acquiring any capture resource.
pub(crate) fn sni_host_available() -> bool {
    use zbus::blocking::{Connection, Proxy};
    let Ok(conn) = Connection::session() else {
        tracing::warn!(target: TARGET_TRAY, "no session bus; treating tray as unavailable");
        return false;
    };
    for service in [
        "org.kde.StatusNotifierWatcher",
        "org.freedesktop.StatusNotifierWatcher",
    ] {
        let Ok(proxy) = Proxy::new(&conn, service, "/StatusNotifierWatcher", service) else {
            continue;
        };
        if let Ok(true) = proxy.get_property::<bool>("IsStatusNotifierHostRegistered") {
            tracing::debug!(target: TARGET_TRAY, service, "SNI host registered");
            return true;
        }
    }
    tracing::warn!(target: TARGET_TRAY, "no registered StatusNotifierHost found");
    false
}

/// The ksni-backed tray item. `activate` (click) fires the finish channel.
struct RecordingItem {
    finish_tx: Sender<()>,
}

impl ksni::Tray for RecordingItem {
    fn id(&self) -> String {
        "rollshot-recording".into()
    }
    fn title(&self) -> String {
        "Rollshot is recording".into()
    }
    fn icon_name(&self) -> String {
        "media-record".into()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Rollshot is recording — click to finish".into(),
            description: String::new(),
            icon_name: "media-record".into(),
            icon_pixmap: Vec::new(),
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        tracing::info!(target: TARGET_TRAY, "tray activated; finishing recording");
        let _ = self.finish_tx.send(());
    }
}

/// Owns the spawned ksni service; shuts it down on Drop (RAII cleanup).
struct KsniTray {
    finish_rx: Receiver<()>,
    handle: ksni::Handle<RecordingItem>,
}

impl RecordingTray for KsniTray {
    fn wait_for_finish(&self) {
        // Blocks until `activate` sends, or the channel closes (service died).
        let _ = self.finish_rx.recv();
    }
}

impl Drop for KsniTray {
    fn drop(&mut self) {
        tracing::debug!(target: TARGET_TRAY, "tearing down tray item");
        self.handle.shutdown();
    }
}

/// Create and register the temporary recording tray item.
pub(crate) fn create_recording_tray() -> Result<Box<dyn RecordingTray>, OverlayError> {
    if !sni_host_available() {
        return Err(OverlayError::Capture(
            "Fullscreen Action Guide requires a system tray. \
             This environment does not support tray icons."
                .to_string(),
        ));
    }
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let service = ksni::TrayService::new(RecordingItem { finish_tx });
    let handle = service.handle();
    service.spawn();
    tracing::info!(target: TARGET_TRAY, "recording tray item registered");
    Ok(Box::new(KsniTray { finish_rx, handle }))
}

/// Best-effort transient "recording started" notification. Never aborts
/// recording: a failure is logged and swallowed.
pub(crate) fn notify_recording_started() {
    use notify_rust::{Hint, Notification, Timeout};
    let result = Notification::new()
        .summary("Rollshot is recording")
        .body("Click the tray icon to finish recording.")
        .icon("media-record")
        .hint(Hint::Transient(true))
        .timeout(Timeout::Milliseconds(4000))
        .show();
    if let Err(err) = result {
        tracing::warn!(target: TARGET_TRAY, %err, "recording notification failed (continuing)");
    }
}
```

> Verify against the spike: exact `ksni::Tray` method set, `TrayService::spawn`/`Handle::shutdown` names, and the `notify_rust::Hint::Transient` variant (the hint API has shifted across `notify-rust` releases). Adjust signatures if needed — the trait and tests are unaffected.

- [ ] **Step 7: Verify build + clippy + the fake-tray test still pass.**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide recording_tray && rtk cargo clippy -p rollshot-iced-overlay --all-targets --features action-guide -- -D warnings && rtk cargo fmt --check`
Expected: PASS. (The real `ksni`/`zbus`/`notify-rust` paths are compiled but not unit-tested — they need a live DBus session, covered by the manual **Task 8**.)

- [ ] **Step 8: Commit.**

```bash
git add crates/rollshot-iced-overlay/Cargo.toml crates/rollshot-iced-overlay/src/recording_tray.rs crates/rollshot-iced-overlay/src/lib.rs
git commit -m "feat(overlay): add RecordingTray seam, SNI probe, notification helper"
```

---

## Task 6: Overlay — `run_action_guide_fullscreen` headless runner

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs` (public re-export near line 92-105)

**Interfaces:**
- Consumes: `CaptureRequest::action_guide_fullscreen()` (Task 2); `RecordingTray`, `create_recording_tray`, `notify_recording_started` (Task 5); existing `acquire_resource`, `real_factories`, `CaptureResource`, `Driver::source_size`, `Driver::begin_action_recording`, `Driver::finalize_action`.
- Produces: `pub fn run_action_guide_fullscreen(config, input_source) -> Result<Option<(Recording, InputCapability, CaptureRegion)>, OverlayError>` (re-exported from `lib.rs`).

The orchestration is split into a generic inner function (testable on CI with closures, mirroring the existing `run_initial_path` pattern in this file) and a thin real wrapper.

```text
run_action_guide_fullscreen(config, input_source)        [thin wrapper]
  └─ orchestrate_fullscreen(make_tray, notify, record)   [generic, unit-tested]
        1. tray = make_tray()?      ← SNI-host-absent errors HERE, before capture
        2. notify()                 ← best-effort
        3. result = record(&tray)   ← acquire → begin → tray.wait_for_finish() → finalize
        4. (tray dropped on return, every path)
```

- [ ] **Step 1: Write the failing orchestration tests.**

Add a test module near the other `linux_runner` tests. These exercise ordering and cleanup with fakes — no real tray/capture:

```rust
#[cfg(all(test, feature = "action-guide"))]
mod fullscreen_orchestration_tests {
    use super::orchestrate_fullscreen;
    use crate::recording_tray::RecordingTray;
    use crate::OverlayError;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct FakeTray {
        dropped: Arc<AtomicBool>,
    }
    impl RecordingTray for FakeTray {
        fn wait_for_finish(&self) {}
    }
    impl Drop for FakeTray {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn tray_failure_skips_capture_and_notify() {
        let notified = Arc::new(AtomicBool::new(false));
        let recorded = Arc::new(AtomicBool::new(false));
        let n = notified.clone();
        let r = recorded.clone();
        let out: Result<Option<()>, OverlayError> = orchestrate_fullscreen(
            || Err(OverlayError::Capture("no tray".into())),
            || n.store(true, Ordering::SeqCst),
            |_tray| {
                r.store(true, Ordering::SeqCst);
                Ok(Some(()))
            },
        );
        assert!(matches!(out, Err(OverlayError::Capture(_))));
        assert!(!notified.load(Ordering::SeqCst), "notify must not run");
        assert!(!recorded.load(Ordering::SeqCst), "capture must not run");
    }

    #[test]
    fn record_error_still_drops_tray() {
        let dropped = Arc::new(AtomicBool::new(false));
        let d = dropped.clone();
        let out: Result<Option<()>, OverlayError> = orchestrate_fullscreen(
            move || Ok(FakeTray { dropped: d.clone() }),
            || {},
            |_tray| Err(OverlayError::Capture("stream failed".into())),
        );
        assert!(matches!(out, Err(OverlayError::Capture(_))));
        assert!(dropped.load(Ordering::SeqCst), "tray dropped on error path");
    }

    #[test]
    fn happy_path_returns_result_and_drops_tray() {
        let dropped = Arc::new(AtomicBool::new(false));
        let d = dropped.clone();
        let out: Result<Option<u8>, OverlayError> = orchestrate_fullscreen(
            move || Ok(FakeTray { dropped: d.clone() }),
            || {},
            |_tray| Ok(Some(7u8)),
        );
        assert_eq!(out.unwrap(), Some(7u8));
        assert!(dropped.load(Ordering::SeqCst), "tray dropped on success path");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail.**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide fullscreen_orchestration`
Expected: FAIL — `cannot find function orchestrate_fullscreen`.

- [ ] **Step 3: Implement the generic orchestration + real wrapper.**

Add to `crates/rollshot-iced-overlay/src/linux_runner.rs` (gated `#[cfg(feature = "action-guide")]`). The generic fn carries the ordering/cleanup contract; the wrapper supplies real implementations.

```rust
/// Generic fullscreen Action Guide orchestration. Ordering contract:
/// `make_tray` runs first (so an SNI-host-absent error happens before any
/// capture resource is acquired); `notify` is best-effort; `record` does the
/// acquire → begin → wait → finalize work. The tray is dropped on every exit
/// path (RAII), giving guaranteed cleanup. Generic + closure-based so CI can
/// unit-test it with fakes (mirrors `run_initial_path`).
#[cfg(feature = "action-guide")]
fn orchestrate_fullscreen<Tray, R>(
    make_tray: impl FnOnce() -> Result<Tray, OverlayError>,
    notify: impl FnOnce(),
    record: impl FnOnce(&Tray) -> Result<Option<R>, OverlayError>,
) -> Result<Option<R>, OverlayError>
where
    Tray: crate::recording_tray::RecordingTray,
{
    let tray = make_tray()?;
    notify();
    record(&tray)
    // `tray` dropped here on all paths.
}

/// Headless fullscreen Action Guide runner. No layer-shell overlay, no iced
/// Application: owns the `Driver` locally and blocks on the tray-click channel.
#[cfg(feature = "action-guide")]
pub fn run_action_guide_fullscreen(
    config: OverlayConfig,
    input_source: Box<dyn rollshot_action::SemanticInputSource>,
) -> Result<
    Option<(
        rollshot_action::Recording,
        rollshot_action::InputCapability,
        rollshot_action::CaptureRegion,
    )>,
    OverlayError,
> {
    if !config.request.is_supported() {
        return Err(OverlayError::Capture(
            "unsupported capture request".to_string(),
        ));
    }
    tracing::info!(target: TARGET_OVERLAY, "fullscreen action guide starting (headless)");

    orchestrate_fullscreen(
        crate::recording_tray::create_recording_tray,
        crate::recording_tray::notify_recording_started,
        move |_tray| {
            // Reuse the exact scrolling acquisition (KWin probe → output-bound
            // stream, or portal with auto-fallback). Returns the live Driver.
            let factories = real_factories();
            let resource = acquire_resource(Workflow::ActionGuide, &config, &factories)?;
            let mut driver = match resource {
                // Portal picker dismissed → no resource → cancel cleanly.
                None => {
                    tracing::info!(target: TARGET_OVERLAY, "capture cancelled before recording");
                    return Ok(None);
                }
                Some(CaptureResource::Streaming { driver, .. }) => driver,
                Some(CaptureResource::OneShot(_)) => {
                    return Err(OverlayError::Capture(
                        "fullscreen action guide expected a streaming resource".to_string(),
                    ));
                }
            };

            let size = driver.source_size();
            let region = rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            };
            tracing::info!(
                target: TARGET_OVERLAY,
                width = size.width,
                height = size.height,
                "recording full display"
            );

            let _capability = driver.begin_action_recording(region, input_source);

            // Block until the user clicks the tray icon.
            _tray.wait_for_finish();

            let (recording, capability) = driver
                .finalize_action()
                .map_err(OverlayError::Capture)?;
            Ok(Some((recording, capability, region)))
        },
    )
}
```

> Note: `acquire_resource`, `real_factories`, `CaptureResource`, and `Workflow` are already in scope in `linux_runner.rs`. If the Task 1 spike found that portal stream setup needs its own executor, wrap the `acquire_resource` line in `pollster::block_on(async { .. })` per the spike's finding (add `pollster` as a Linux+action-guide dep in that case).

- [ ] **Step 4: Run orchestration tests to verify GREEN.**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide fullscreen_orchestration`
Expected: PASS (all three orchestration tests).

- [ ] **Step 5: Re-export the public runner from `lib.rs`.**

In `crates/rollshot-iced-overlay/src/lib.rs`, after `run_action_guide` (line 92-105):

```rust
#[cfg(all(target_os = "linux", feature = "action-guide"))]
pub fn run_action_guide_fullscreen(
    config: OverlayConfig,
    input_source: Box<dyn rollshot_action::SemanticInputSource>,
) -> Result<
    Option<(
        rollshot_action::Recording,
        rollshot_action::InputCapability,
        rollshot_action::CaptureRegion,
    )>,
    OverlayError,
> {
    linux_runner::run_action_guide_fullscreen(config, input_source)
}
```

- [ ] **Step 6: Verify build, clippy, full crate tests.**

Run: `rtk cargo test -p rollshot-iced-overlay --features action-guide && rtk cargo clippy -p rollshot-iced-overlay --all-targets --features action-guide -- -D warnings && rtk cargo fmt --check`
Expected: PASS.

- [ ] **Step 7: Commit.**

```bash
git add crates/rollshot-iced-overlay/src/linux_runner.rs crates/rollshot-iced-overlay/src/lib.rs
git commit -m "feat(overlay): add headless run_action_guide_fullscreen runner"
```

---

## Task 7: App wiring + docs (code only — no desktop needed)

All steps here build/test/commit on the headless server. The end-to-end **runtime** verification lives in **Task 8** (requires a KDE desktop).

**Files:**
- Modify: `crates/rollshot-app/src/main.rs` (`run_action_guide_record`, lines 134-163; call site that matches `LaunchMode::ActionGuide`)
- Modify: `README.md` (Action Guide usage note)

**Interfaces:**
- Consumes: `LaunchMode::ActionGuide { fullscreen }` (Task 4); `CaptureRequest::action_guide_fullscreen()` (Task 2); `rollshot_iced_overlay::run_action_guide_fullscreen` (Task 6).

- [ ] **Step 1: Branch `run_action_guide_record` on `fullscreen`.**

Update the Linux `run_action_guide_record` in `crates/rollshot-app/src/main.rs` (lines 134-155) to take the flag and select the runner. Change the signature to `fn run_action_guide_record(fullscreen: bool)` and:

```rust
#[cfg(all(feature = "action-guide", target_os = "linux"))]
fn run_action_guide_record(fullscreen: bool) -> Result<(), String> {
    use rollshot_capture::CaptureRequest;
    let request = if fullscreen {
        CaptureRequest::action_guide_fullscreen()
    } else {
        CaptureRequest::action_guide_region()
    };
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: "auto".to_string(),
        fps: 5,
        show_cursor: false,
        request,
        target_output_name: None,
    };
    let source = crate::action_input::create_input_source();
    let outcome = if fullscreen {
        rollshot_iced_overlay::run_action_guide_fullscreen(config, source)
    } else {
        rollshot_iced_overlay::run_action_guide(config, source)
    }
    .map_err(|e| e.to_string())?;
    match outcome {
        Some((recording, capability, region)) => {
            let source_kind = crate::timeline_workspace::source_kind_for(
                capability,
                crate::storage::Platform::Linux,
            );
            crate::timeline_workspace::run(recording, region, capability, source_kind)
        }
        None => Ok(()),
    }
}
```

Update the macOS stub signature to match: `fn run_action_guide_record(_fullscreen: bool) -> Result<(), String>` (lines 157-163).

- [ ] **Step 2: Update the call site.**

Find where `LaunchMode::ActionGuide` is matched (the dispatch that calls `run_action_guide_record()`), and pass the flag:

```rust
        #[cfg(feature = "action-guide")]
        LaunchMode::ActionGuide { fullscreen } => run_action_guide_record(fullscreen),
```

- [ ] **Step 3: Build the whole workspace.**

Run: `rtk cargo build -p rollshot-app --features action-guide && rtk cargo test -p rollshot-app --features action-guide && rtk cargo fmt --check`
Expected: PASS — compiles and existing app tests pass.

- [ ] **Step 4: Workspace clippy.**

Run: `rtk cargo clippy --workspace --all-targets --features action-guide -- -D warnings`
Expected: PASS (no warnings).

- [ ] **Step 5: Add a README usage note.**

In `README.md`, under the Action Guide section, add:

```markdown
- Fullscreen recording (Linux/KDE): `rollshot action-guide --fullscreen`
  records the whole display. Click the temporary system-tray icon to finish.
  Requires a system tray (StatusNotifierItem host); KDE Plasma provides one.
```

- [ ] **Step 6: Commit.**

```bash
git add crates/rollshot-app/src/main.rs README.md
git commit -m "feat(app): wire fullscreen action guide runner + docs"
```

At this point everything that can be built and unit-tested on a headless server is **done and committed**. The feature is code-complete but **not yet runtime-verified** — proceed to Task 8 only on a KDE desktop.

---

## ⚠️ Task 8: Manual verification on real KDE Wayland — REQUIRES A HUMAN + KDE DESKTOP

> **🚦 STOP — do NOT attempt this on the headless server.** Everything below needs a live KDE Plasma Wayland session (compositor + `xdg-desktop-portal` + PipeWire + an SNI tray host). These are the checks that could not run during development. **The agent should not run this task; it must hand off to the user** with the checklist below. None of Tasks 1–7's commits are considered runtime-verified until this passes.

**Why deferred:** the development host is a headless TTY with no desktop stack, so live tray, live screen capture, and the headless-`Driver` feasibility question (the one unknown the spec flagged) can only be confirmed on a real KDE machine.

**Prerequisites for the human:** a KDE Plasma Wayland session, this branch checked out, `cargo` toolchain available.

- [ ] **Step 1: Build on the KDE machine.**

Run: `cargo build -p rollshot-app --features action-guide`
Expected: PASS.

- [ ] **Step 2: Headless `Driver` feasibility (the flagged unknown).**

Run: `cargo run -p rollshot-app --features action-guide -- --action-guide --fullscreen`
Expected: capture actually starts (portal screen-picker may appear on portal backends — pick a screen). **If it hangs or panics during stream acquisition**, the headless `Driver` assumption is wrong: apply the `pollster::block_on` wrapper noted in Task 6 Step 3 around the `acquire_resource` call, then re-test. Record the outcome.

- [ ] **Step 3: Tray + notification + stop (happy path).**

With the command from Step 2 running:
- Tray icon appears in the system tray.
- A transient notification "Rollshot is recording" shows.
- Perform a few clicks/scrolls, then click the tray icon.
- Expected: recording stops, the tray icon disappears, and the Action Guide Timeline Workspace opens with detected steps for the full display.

- [ ] **Step 4: Region regression.**

Run: `cargo run -p rollshot-app --features action-guide -- --action-guide`
Expected: the region-selection overlay still appears and behaves exactly as before (no tray, no fullscreen behavior).

- [ ] **Step 5: No-tray-host negative path.**

On a session with no SNI host (e.g. a bare compositor, or GNOME without the AppIndicator extension), run the fullscreen command.
Expected: it exits immediately with "Fullscreen Action Guide requires a system tray…" and **never starts capture** (no portal picker, no frames).

- [ ] **Step 6: Report results.**

Record pass/fail for each step (and any `pollster` fix applied in Step 2) back to the team / in the PR. If Step 2 required the `pollster` change, that is a code change to commit on this branch.

---

## Self-Review

**Spec coverage:**
- Launch `--action-guide --fullscreen` + CLI forward → Tasks 3, 4, 7. ✓
- `action_guide_fullscreen()` + `is_supported` + `needs_overlay` → Task 2. ✓
- Headless runner: tray check → tray → notify → acquire stream → record → wait click → finalize → return → workspace → Tasks 5, 6, 7. ✓
- SNI tray (ksni), transient notification (notify-rust) → Task 5. ✓
- "No SNI host" hard error before capture → Task 5 (`create_recording_tray`) + Task 6 ordering test. ✓
- Tray destroyed on all paths (RAII) → Task 5 `Drop` + Task 6 cleanup tests. ✓
- Portal-picker-dismissed → `Ok(None)` → Task 6 (`None` arm). ✓
- Full-display region from driver source size → Task 6. ✓
- Reuse Timeline Workspace handoff → Task 7. ✓
- Unit tests (capture/launch/cli) → Tasks 2, 3, 4. ✓
- Module tests (tray lifecycle, host-absent, cleanup, notification best-effort) → Tasks 5, 6. ✓
- Dependency/API/version risk (ksni/zbus alignment) → Task 1 spike (headless, autonomous). ✓
- Runtime feasibility (headless Driver) + live tray/notification + end-to-end + regression + no-host negative → **Task 8 (manual, requires KDE desktop)**. ✓

**Placeholder scan:** No `TODO`/`TBD`/"add error handling"/"similar to". The only deferred specifics are exact `ksni`/`zbus`/`notify-rust` signatures, explicitly gated behind the Task 1 spike (which compile-checks the real code on the server) with concrete fallback instructions — not placeholders.

**Headless-host note:** the development host is a headless TTY (no Wayland/portal/pipewire/SNI host). Tasks 1–7 are fully doable there (build + unit tests, which use fakes for tray/capture). All live-runtime checks are isolated in Task 8 and explicitly handed to the user.

**Type consistency:** `RecordingTray::wait_for_finish` used identically in Tasks 5/6. `orchestrate_fullscreen(make_tray, notify, record)` signature matches its tests and call site. `run_action_guide_fullscreen` signature identical in `linux_runner.rs` and `lib.rs`. `run_action_guide_record(fullscreen: bool)` matches its call site. `CaptureResource::Streaming { driver, .. }` matches the existing enum.

---

## Execution / Parallelization

Tasks span four crates with clean boundaries — good parallelism after the spike.

| Task | Modules touched | Env | Depends on |
|------|-----------------|-----|------------|
| T1: Spike (deps/API/version) | `crates/rollshot-iced-overlay/` (throwaway example) | headless server ✓ | — |
| T2: capture request | `crates/rollshot-capture/` | headless server ✓ | — |
| T3: cli flag | `crates/rollshot-cli/` | headless server ✓ | — |
| T4: app launch parse | `crates/rollshot-app/` | headless server ✓ | — |
| T5: tray seam | `crates/rollshot-iced-overlay/` | headless server ✓ | T1 (findings) |
| T6: headless runner | `crates/rollshot-iced-overlay/` | headless server ✓ | T2, T5 |
| T7: app wiring + docs | `crates/rollshot-app/`, `README.md` | headless server ✓ | T2, T4, T6 |
| **T8: manual verification** | runtime only (no code, unless `pollster` fix) | **KDE desktop ⚠️ human** | T7 |

**Lanes:**
- Lane A (capture): T2
- Lane B (cli): T3
- Lane C (app): T4 → T7 (sequential, same crate; T7 also waits on T6)
- Lane D (overlay): T5 → T6 (sequential, same crate)

**Execution order:** Run **T1 spike first** (informs Lane D). Then launch **T2, T3, T4, T5 in parallel**. Then **T6** (after T2 + T5). Then **T7** (after T2 + T4 + T6). **T1–T7 all run autonomously on the headless server.** Finally **T8 is handed to the user** to run on a KDE desktop — the agent stops at the end of T7.

**Conflict flags:** T5 and T6 both touch `crates/rollshot-iced-overlay/` → same lane, sequential (no parallel conflict). No task modifies the root `Cargo.toml` `members` list (only crate-level `Cargo.toml` deps in T5), so nothing serializes the whole workspace.
