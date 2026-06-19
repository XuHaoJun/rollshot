# Thin Daemon macOS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the macOS adapter for `rollshot-app daemon` — a status-bar item with `Capture Region` / `Quit Rollshot` and a configurable `Command+Shift+6` global shortcut — that drives the already-shipped shared daemon core and spawns region-capture child processes, with the same single-instance, busy-trigger, configuration-fallback, and quit-termination behavior the Linux slice already enforces.

**Architecture:** Reuse the platform-neutral daemon code that the Linux slice already landed (`core.rs` state machine, `instance.rs` file lock, `config.rs` loader, `process.rs` POSIX process-group launcher) without behavioral change. Add a macOS adapter under `crates/rollshot-app/src/daemon/macos*` that owns a native main-thread event loop (winit 0.30, already in the tree via iced), hosts a `tray-icon` status item and a `global-hotkey` registration, and funnels their callbacks plus watcher-thread `CaptureExited` events into the shared `DaemonCore` through a `winit::EventLoopProxy<DaemonEvent>`. Capture still runs as a separate `rollshot-app capture --workflow screenshot --scope region` process, preserving the existing macOS iced product event loop.

**Tech Stack:** Rust 2021, clap, serde/toml, fs4, std channels/threads, `nix` POSIX signals, winit 0.30 (`ApplicationHandler` + `EventLoopProxy`, `Accessory` activation policy), `tray-icon` 0.24, `global-hotkey` 0.8, tracing.

## Global Constraints

These are copied verbatim from the spec (`docs/superpowers/specs/2026-06-19-thin-daemon-design.md`) and apply to every task below.

- The same binary provides both modes; `rollshot-app` with no subcommand keeps its current meaning (one-shot scrolling region capture) and does **not** start the daemon.
- The daemon always launches exactly this logical capture request: `rollshot-app capture --workflow screenshot --scope region`, resolving the executable with `std::env::current_exe()` (never `PATH`).
- The tray menu exposes exactly two product actions: `Capture Region` and `Quit Rollshot`.
- macOS default hotkey is `Command+Shift+6`. Configuration is read once at startup from `<config-dir>/rollshot/config.toml`; on absent file use the platform default with no warning; on unreadable/invalid-TOML/invalid-hotkey, log a warning and fall back to the platform default.
- Permit at most one daemon and one active capture child. A second daemon logs an info event and exits successfully without creating a tray or registering a shortcut.
- Tray initialization failure is fatal. Global-shortcut initialization or binding failure is non-fatal → tray-only operation. Capture spawn failure and capture non-zero exit are both non-fatal; neither terminates the daemon.
- On quit: request termination of the active capture process group, wait up to two seconds, force-terminate if still running, tear down shortcut and tray resources, release the single-instance guard, and exit.
- Runtime diagnostics use stable explicit targets under `rollshot::daemon::*` with structured fields. Diagnostics never log raw key events; they may log the configured shortcut description.
- The configuration model owns a platform-neutral shortcut representation; the macOS adapter is responsible for translating it into the native `global-hotkey` registration format.
- The macOS adapter emits the same `CaptureRegion` and `Quit` semantic events as Linux and uses the same shared `DaemonCore` and child-process contract. The daemon process never starts an iced capture runner itself.

---

## Scope

- **In:** the deferred macOS slice (delivery-sequence step 4 of the spec) — the `tray-icon` status item, the `global-hotkey` registration, the native main-thread event loop, the macOS `run_primary` wiring, and the macOS reuse of the shared core/instance/config/process modules.
- **Out:** Linux (already shipped), X11/GNOME/other Linux desktops, autostart/launch-at-login, a settings window or hotkey editor, live config reload, multiple shortcut actions, and any change to the existing macOS capture / Result Workspace flow.
- This plan touches **macOS-only** code paths plus three cross-platform files (`config.rs`, `mod.rs`, `Cargo.toml`). The Linux adapter (`daemon/linux*`) is **not** edited. Per AGENTS.md §8 the daemon is not part of the shared capture-UI/overlay surface, so no overlay/crop/coordinate paths are affected on either platform; the existing macOS capture flow is reached only as an unchanged child process.

## File map

- `Cargo.toml`: no change expected (workspace deps already include `fs4`, `nix`; `winit`/`tray-icon`/`global-hotkey` are package-local). Verify the `[workspace.dependencies]` `nix` entry exists (it does — used by the Linux slice).
- `crates/rollshot-app/Cargo.toml`: add `winit`, `tray-icon`, `global-hotkey`, and `nix` to the macOS target dependency block.
- `crates/rollshot-app/src/daemon/mod.rs`: widen the `process` module gate to macOS, register `macos` module, add the macOS `run_primary`, and narrow the unimplemented fallback to non-Linux-non-macOS.
- `crates/rollshot-app/src/daemon/config.rs`: expose the platform-neutral shortcut parts (`Modifier` public + `modifiers()`/`key()` accessors) so the macOS adapter can translate them. No parsing/behavior change.
- `crates/rollshot-app/src/daemon/macos.rs`: macOS adapter composition — the winit event loop, `ApplicationHandler`, proxy/forwarder wiring, fatal-tray handling, and tray-only fallback.
- `crates/rollshot-app/src/daemon/macos/tray.rs`: `tray-icon` status item + two menu items, and the pure menu-id → `DaemonEvent` mapping.
- `crates/rollshot-app/src/daemon/macos/shortcut.rs`: `global-hotkey` registration and the pure `Shortcut` → `global_hotkey::hotkey::HotKey` translation.
- `README.md`: macOS daemon usage, configuration path, and limitations.
- `AGENTS.md`: project-map note that the macOS daemon adapter is wired (mirrors the Linux entry).
- `docs/superpowers/specs/2026-06-19-thin-daemon-design.md`: no edits; it is the approved source of truth.

## Engineering review lock-in

### Step 0: scope challenge

- Every task contributes to the macOS daemon slice or is the minimum reuse change needed to compile the shared core on macOS. No speculative abstraction is added: the adapter mirrors the existing `linux.rs`/`linux/tray.rs`/`linux/shortcut.rs` shape.
- Complexity budget: 2 net-new source files (`macos.rs` is the third but parallels `linux.rs`), 3 modified cross-platform files, 2 doc files. Well under the review thresholds.
- No new binary, no new crate, no release-packaging change. `rollshot-app daemon` on macOS is a new mode of the existing packaged binary, exactly as on Linux.
- The shared `DaemonCore`, single-instance guard, configuration loader, and child-process launcher are reused unchanged, so the macOS slice cannot drift from Linux product behavior.

### What already exists (reuse decisions)

| Existing code or flow | Reuse decision |
|---|---|
| `LaunchCli -> resolve_launch_mode -> run` and `LaunchMode::Daemon` (`launch.rs`, `main.rs:78`) | Already routes `daemon` to `daemon::run()` on all targets. **No change** — the macOS branch is added inside `daemon::run_primary`. |
| `daemon::run` / `run_if_primary` (`daemon/mod.rs:9`) | Acquires the lock and dispatches to `run_primary`. **No change**; add a `#[cfg(target_os = "macos")]` `run_primary`. |
| `DaemonCore` state machine + `DaemonEvent` (`daemon/core.rs`) | **No change.** Idle↔Capturing↔Exiting, monotonic `CaptureId`, busy-trigger ignore, spawn-failure-to-idle, quit-terminates-then-exits, and `Drop` backstop all apply verbatim. |
| `InstanceGuard` fs4 file lock (`daemon/instance.rs`) | **No change.** `fs4::FileExt::try_lock` is cross-platform; `lock_path()` already resolves `<config-dir>/rollshot/daemon.lock`, which on macOS is `~/Library/Application Support/rollshot/daemon.lock`. |
| Config loader + `Platform::Macos` default (`daemon/config.rs`) | Reuse; `default_for(Platform::Macos)` already yields `Command+Shift+6` and `config_path()` already resolves the macOS Application Support path via `dirs::config_dir()`. Add only public accessors for the adapter (Task 2). |
| `CurrentExeLauncher` + `spawn_watcher` + `ProcessGroupCapture` + `terminate_with` (`daemon/process.rs`) | **No code change** — all POSIX (`CommandExt::process_group`, `nix::killpg`, SIGTERM→2s→SIGKILL) and valid on macOS. Only the `mod.rs` cfg gate and the `nix` macOS dependency are added (Task 2). |
| Linux adapter shape (`daemon/linux.rs`, `linux/tray.rs`, `linux/shortcut.rs`) | Used as the **structural template** for the macOS adapter; not edited. |
| winit 0.30.13 already in the dependency tree (via iced) | Reuse for the macOS daemon event loop; iced and the daemon run in **separate processes**, so there is never a second event loop in one process. |

### Runtime data flow (macOS)

```text
 NSStatusItem menu (tray-icon) ──MenuEvent──┐
                                            │ map id -> DaemonEvent
 Cmd+Shift+6 (global-hotkey) ──HotKeyEvent──┤ (Pressed -> CaptureRegion)
                                            v
                              proxy.send_event(DaemonEvent)        capture watcher thread
                                            │                              │
                                            │                       CaptureExited(id,success)
                                            │                              v
                                            │                     mpsc::Sender<DaemonEvent>
                                            │                              │
                                            │                     forwarder thread: rx -> proxy
                                            v                              │
        ┌──────────────────────────────────────────────────────────────────┐
        │ winit main-thread loop: ApplicationHandler::user_event(DaemonEvent)│
        │   -> DaemonCore::handle(event)                                     │
        │        Idle <-> Capturing -> (LoopAction::Exit => event_loop.exit())│
        └──────────────────────────────────────────────────────────────────┘
                                            │ CaptureRegion (Idle)
                                            v
                        current_exe() capture --workflow screenshot --scope region
                        (own process group; Quit => SIGTERM, wait 2s, SIGKILL)
```

The only structural difference from Linux: Linux blocks the main thread on `receiver.recv()`; macOS must run a native main-thread loop because `tray-icon`/`global-hotkey` deliver their callbacks on the NSApplication run loop. The `EventLoopProxy` is the single bridge into that loop. The shared `DaemonCore` is identical and remains the sole owner of product behavior.

### Ownership and shutdown order

```text
InstanceGuard (file lock)
  └── macos::run owns:
       ├── EventLoop + EventLoopProxy (main thread)
       ├── forwarder thread (mpsc Receiver -> proxy)
       └── DaemonApp:
            ├── DaemonCore  ── ProcessGroupCapture (active capture child)
            ├── TrayGuard   (tray-icon NSStatusItem + menu; Drop clears MenuEvent handler)
            └── ShortcutGuard (global-hotkey manager; Drop clears handler + unregisters)

Quit (menu or future signal):
DaemonCore::handle(Quit) terminates/reaps the capture process group (SIGTERM, 2s, SIGKILL)
  -> returns LoopAction::Exit -> event_loop.exit()
  -> run_app returns; DaemonApp drops:
       ShortcutGuard::drop (clear handler, unregister hotkey)
       -> TrayGuard::drop (clear handler, remove status item)
       -> forwarder thread ends (mpsc sender dropped with DaemonCore)
  -> InstanceGuard drops, releasing the file lock
```

This preserves the spec's quit ordering: capture is terminated **before** the shortcut and tray are torn down (the termination happens inside `DaemonCore::handle(Quit)`), and the single-instance guard is released last. The `DaemonCore::Drop` impl is the backstop for any non-`Quit` exit path (fatal tray error), guaranteeing no orphaned capture group.

### API assumptions verified (and what the Task 1 spike must confirm)

- **POSIX process group on macOS** — `std::os::unix::process::CommandExt::process_group` and `nix::sys::signal::killpg` are valid on macOS with the same semantics the Linux slice relies on; the existing `process.rs` unit tests (which use `sh -c`) will run on macOS once the cfg gate is widened.
- **macOS config directory** — `dirs::config_dir()` on macOS returns `~/Library/Application Support`, so the spec's "Application Support" location is satisfied by the existing `config_path()`/`lock_path()` with no change.
- **`global-hotkey` does not need Accessibility permission** — on macOS it registers via Carbon `RegisterEventHotKey`, not a `CGEventTap`, so no TCC/Accessibility prompt is expected (unlike the Action Guide input source). **Spike confirms** the hotkey fires from a non-bundled binary.
- **Windowless winit on macOS** — a winit 0.30 `EventLoop` with `ActivationPolicy::Accessory` and no windows stays alive in `ControlFlow::Wait`, delivers `EventLoopProxy` user events, hosts an `NSStatusItem`, and `run_app` **returns** after `ActiveEventLoop::exit()`. **Spike confirms** all four behaviors against the pinned versions.
- **`tray-icon` / `global-hotkey` event-handler signatures** — `MenuEvent::set_event_handler` and `GlobalHotKeyEvent::set_event_handler` accept a `Send + Sync` closure; `EventLoopProxy<DaemonEvent>` is `Send + Sync`. **Spike captures** the exact signatures (closure vs. boxed) for the pinned versions so Tasks 3–5 compile first time.

### NOT in scope

- Linux daemon changes — the Linux adapter is frozen for this slice.
- A bundled `.app`, Info.plist, code-signing, or notarization — the daemon runs as a mode of the existing binary; `Accessory` activation policy is set at runtime, not via a bundle.
- Other Linux desktops, X11, autostart, settings UI, live reload, or multiple shortcut actions.
- Any change to the existing macOS capture overlay, Result Workspace, thumbnail, or save-handoff flow — those are reached only as an unchanged child process.

### Pre-flight: macOS spike gate

Tasks 2–6 below contain concrete, version-pinned code. They assume the Task 1 spike confirmed the runtime behaviors and exact crate APIs listed above. **If the spike reveals a different `set_event_handler` shape, a winit lifecycle difference, or a windowless-loop limitation, update the concrete code in Tasks 3–5 to match before proceeding** — the spike's notes are authoritative over this document for those specific API shapes.

---

### Task 1: macOS integration spike (go/no-go gate)

**This is a throwaway feasibility spike, not TDD.** It must run on macOS hardware. Its deliverable is a confirmed go/no-go decision plus the exact pinned versions and event-handler signatures that Tasks 3–5 depend on. Per AGENTS.md, this resolves the platform/integration unknowns that cannot be settled by reading code.

**Files:**
- Create (throwaway): `crates/rollshot-app/examples/macos_daemon_spike.rs` (delete or leave uncommitted after the gate; it is not part of the shipped daemon).

- [ ] **Step 1: Add the spike dependencies temporarily**

In `crates/rollshot-app/Cargo.toml`, under `[target.'cfg(target_os = "macos")'.dependencies]`, add:

```toml
winit = "0.30"
tray-icon = "0.24"
global-hotkey = "0.8"
```

- [ ] **Step 2: Write a minimal windowless tray + hotkey harness**

Create `crates/rollshot-app/examples/macos_daemon_spike.rs`:

```rust
//! Throwaway spike (delete after the macOS daemon plan's Task 1 gate).
//! Confirms: windowless winit loop stays alive under Accessory policy,
//! NSStatusItem appears, menu clicks + Cmd+Shift+6 fire, and exit() returns.
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::TrayIconBuilder;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

#[derive(Debug)]
enum Ev {
    Capture,
    Quit,
}

struct App {
    proxy: EventLoopProxy<Ev>,
    started: bool,
    _tray: Option<tray_icon::TrayIcon>,
    _mgr: Option<GlobalHotKeyManager>,
}

impl ApplicationHandler<Ev> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        el.set_control_flow(ControlFlow::Wait);

        let menu = Menu::new();
        menu.append(&MenuItem::with_id(MenuId::new("capture-region"), "Capture Region", true, None)).unwrap();
        menu.append(&MenuItem::with_id(MenuId::new("quit-rollshot"), "Quit Rollshot", true, None)).unwrap();
        let proxy = self.proxy.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            match e.id.as_ref() {
                "capture-region" => { let _ = proxy.send_event(Ev::Capture); }
                "quit-rollshot" => { let _ = proxy.send_event(Ev::Quit); }
                _ => {}
            }
        }));
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_title("Rollshot")
            .with_tooltip("Rollshot")
            .build()
            .expect("tray builds");
        self._tray = Some(tray);

        let mgr = GlobalHotKeyManager::new().expect("hotkey manager");
        let hk = HotKey::new(Some(Modifiers::META | Modifiers::SHIFT), Code::Digit6);
        mgr.register(hk).expect("register Cmd+Shift+6");
        let id = hk.id();
        let proxy = self.proxy.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            if e.id() == id && e.state() == HotKeyState::Pressed {
                let _ = proxy.send_event(Ev::Capture);
            }
        }));
        self._mgr = Some(mgr);
        eprintln!("spike: tray + hotkey ready; click menu or press Cmd+Shift+6");
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: Ev) {
        eprintln!("spike: got {ev:?}");
        if matches!(ev, Ev::Quit) {
            el.exit();
        }
    }

    fn window_event(
        &mut self,
        _el: &ActiveEventLoop,
        _id: winit::window::WindowId,
        _ev: winit::event::WindowEvent,
    ) {
    }
}

fn main() {
    let event_loop = EventLoop::<Ev>::with_user_event()
        .with_activation_policy(ActivationPolicy::Accessory)
        .build()
        .expect("event loop builds");
    let proxy = event_loop.create_proxy();
    let mut app = App { proxy, started: false, _tray: None, _mgr: None };
    event_loop.run_app(&mut app).expect("loop runs");
    eprintln!("spike: run_app returned after exit() — clean shutdown path works");
}
```

- [ ] **Step 3: Run and observe**

Run on macOS: `rtk cargo run -p rollshot-app --example macos_daemon_spike`

Confirm and record each result (this is the gate):
- A menu-bar status item titled "Rollshot" appears, with no Dock icon (Accessory policy).
- The menu shows exactly `Capture Region` and `Quit Rollshot`.
- Clicking `Capture Region` prints `spike: got Capture`; pressing `Cmd+Shift+6` prints `spike: got Capture` **with no Accessibility permission prompt**.
- The loop stays alive while idle (no early exit with zero windows).
- Clicking `Quit Rollshot` prints `spike: got Quit`, then `spike: run_app returned after exit()` — i.e. `run_app` returns rather than `process::exit`-ing.

- [ ] **Step 4: Capture the API facts and decide**

Record in the task's review notes: the exact `MenuEvent::set_event_handler` / `GlobalHotKeyEvent::set_event_handler` signatures the compiler accepted (closure vs. `Box<dyn …>`), the `HotKey` field/accessor names used (`mods`/`key` vs. methods, `id()`, `state()`), and any winit lifecycle quirk (e.g. tray must be built in `resumed` vs. `new_events`). **Go** if all Step 3 observations hold. If any fail, document the deviation; Tasks 3–5 must be adjusted to the working pattern before continuing.

- [ ] **Step 5: Remove the throwaway example**

```bash
rm crates/rollshot-app/examples/macos_daemon_spike.rs
```

Leave the three dependency lines from Step 1 in place — Task 2 formalizes them. Do not commit the example file. (No commit for this task; it is a gate, not a deliverable change. If the spike dependencies were added to `Cargo.toml`, they carry into Task 2.)

---

### Task 2: Enable the shared daemon core on macOS

Make the already-shipped platform-neutral daemon modules compile and run on macOS, and expose the platform-neutral shortcut parts the adapter needs. No product behavior changes.

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`
- Modify: `crates/rollshot-app/src/daemon/config.rs`

**Interfaces:**
- Consumes: `DaemonConfig`, `Shortcut`, `Platform::Macos`, `config_path`, `load_from` (existing); `CurrentExeLauncher`, `DaemonCore` (existing).
- Produces (for Tasks 3–5):
  - `pub enum Modifier { Control, Alt, Shift, Command, Super }` (now public) in `daemon/config.rs`.
  - `impl Shortcut { pub fn modifiers(&self) -> &[Modifier]; pub fn key(&self) -> &str }`.
  - `crate::daemon::process` available under `#[cfg(any(target_os = "linux", target_os = "macos"))]`, exporting `CurrentExeLauncher` on macOS.

- [ ] **Step 1: Add the macOS daemon dependencies**

In `crates/rollshot-app/Cargo.toml`, extend the existing `[target.'cfg(target_os = "macos")'.dependencies]` block (keep the existing entries) with:

```toml
winit = "0.30"
tray-icon = "0.24"
global-hotkey = "0.8"
nix = { workspace = true }
```

Rationale: `nix` is currently only under the Linux target block, but `process.rs` (now compiled on macOS) uses `nix::sys::signal`. `winit`/`tray-icon`/`global-hotkey` back the macOS adapter (Tasks 3–5). All are macOS-gated, so Linux builds are unaffected.

- [ ] **Step 2: Write the failing config-accessor test**

Add to the `tests` module in `crates/rollshot-app/src/daemon/config.rs`:

```rust
    #[test]
    fn shortcut_exposes_modifiers_and_key_for_adapters() {
        let shortcut: Shortcut = "Command+Shift+6".parse().unwrap();
        assert_eq!(shortcut.key(), "6");
        assert!(shortcut.modifiers().contains(&Modifier::Command));
        assert!(shortcut.modifiers().contains(&Modifier::Shift));
        assert!(!shortcut.modifiers().contains(&Modifier::Control));
    }
```

- [ ] **Step 3: Run it and confirm it fails to compile**

Run: `rtk cargo test -p rollshot-app --lib daemon::config::tests::shortcut_exposes_modifiers_and_key_for_adapters`
Expected: FAIL — `Modifier` is private and `modifiers()`/`key()` do not exist.

- [ ] **Step 4: Make `Modifier` public and add the accessors**

In `crates/rollshot-app/src/daemon/config.rs`, change the `Modifier` enum visibility:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Control,
    Alt,
    Shift,
    Command,
    Super,
}
```

Add accessors to the existing `impl Shortcut` block (next to `portal_trigger`):

```rust
    /// Platform-neutral modifiers, for adapters that translate into a native
    /// registration format (the macOS `global-hotkey` adapter).
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    /// The single base key, normalized: one lowercase ASCII letter/digit, or
    /// `F1`..`F24`.
    pub fn key(&self) -> &str {
        &self.key
    }
```

- [ ] **Step 5: Run the config test and confirm it passes**

Run: `rtk cargo test -p rollshot-app --lib daemon::config::tests::shortcut_exposes_modifiers_and_key_for_adapters`
Expected: PASS.

- [ ] **Step 6: Widen the `process` module gate and register the `macos` module**

In `crates/rollshot-app/src/daemon/mod.rs`, change the module declarations:

```rust
pub mod config;
pub mod core;
pub mod instance;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod process;
```

**Do not** change the existing `#[cfg(not(target_os = "linux"))]` fallback `run_primary` in this task. At the end of Task 2, macOS still uses that "not implemented" fallback at runtime (the adapter isn't wired until Task 5), but it must keep compiling. Task 5 narrows the fallback to `not(any(linux, macos))` at the same moment it adds the real macOS `run_primary`, avoiding both a missing and a duplicate definition.

To make `#[cfg(target_os = "macos")] pub mod macos;` resolve, create `crates/rollshot-app/src/daemon/macos.rs` now as a placeholder containing only:

```rust
//! macOS daemon adapter (built in Tasks 3-5).
```

Task 3 adds the `tray` submodule, Task 4 the `shortcut` submodule, Task 5 the loop. The placeholder keeps macOS compiling at every task boundary.

- [ ] **Step 7: Verify the shared core compiles and tests pass on macOS**

Run on macOS: `rtk cargo test -p rollshot-app --lib daemon::`
Expected: PASS — including the existing `daemon::process::tests::*` (process-group termination, watcher exit) now running on macOS, the `daemon::core::tests::*` state-machine tests, the `daemon::instance::tests::*` lock tests, and the new config accessor test.

On Linux, run the same command to confirm no regression:
Run: `rtk cargo test -p rollshot-app --lib daemon::`
Expected: PASS (macOS module is cfg-gated out; `process`/`core`/`instance`/`config` unchanged behavior).

- [ ] **Step 8: Commit**

```bash
git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/daemon/mod.rs crates/rollshot-app/src/daemon/config.rs crates/rollshot-app/src/daemon/macos.rs
git commit -m "feat(daemon): enable shared core on macOS"
```

---

### Task 3: macOS tray status item

Add the `tray-icon` status item with the two product menu items and the pure menu-id → `DaemonEvent` mapping. The status item appears in the menu bar; clicking an item posts the corresponding semantic event through the event-loop proxy.

**Files:**
- Modify: `crates/rollshot-app/src/daemon/macos.rs` (register the `tray` submodule)
- Create: `crates/rollshot-app/src/daemon/macos/tray.rs`

**Interfaces:**
- Consumes: `crate::daemon::core::DaemonEvent`; `winit::event_loop::EventLoopProxy`.
- Produces (for Task 5):
  - `pub(crate) fn daemon_event_for(id: &str) -> Option<DaemonEvent>`
  - `pub(crate) struct TrayGuard` with `pub(crate) fn start(proxy: EventLoopProxy<DaemonEvent>) -> Result<TrayGuard, String>` and a `Drop` that clears the global `MenuEvent` handler and removes the status item.
  - `pub(crate) const CAPTURE_ID: &str` / `pub(crate) const QUIT_ID: &str`.

- [ ] **Step 1: Register the tray submodule**

In `crates/rollshot-app/src/daemon/macos.rs`, replace the placeholder doc comment's body so the file begins:

```rust
//! macOS daemon adapter.
pub(crate) mod tray;
```

- [ ] **Step 2: Write the failing menu-mapping test**

Create `crates/rollshot-app/src/daemon/macos/tray.rs` with only the mapping function and its test first (the native `TrayGuard` is added after the test fails):

```rust
use crate::daemon::core::DaemonEvent;

pub(crate) const CAPTURE_ID: &str = "capture-region";
pub(crate) const QUIT_ID: &str = "quit-rollshot";

/// Map a tray menu item id to the daemon semantic event it triggers. Unknown
/// ids are ignored so a stray menu event can never drive product behavior.
pub(crate) fn daemon_event_for(id: &str) -> Option<DaemonEvent> {
    match id {
        CAPTURE_ID => Some(DaemonEvent::CaptureRegion),
        QUIT_ID => Some(DaemonEvent::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_daemon_events() {
        assert!(matches!(
            daemon_event_for(CAPTURE_ID),
            Some(DaemonEvent::CaptureRegion)
        ));
        assert!(matches!(daemon_event_for(QUIT_ID), Some(DaemonEvent::Quit)));
        assert!(daemon_event_for("unknown").is_none());
    }
}
```

- [ ] **Step 3: Run it and confirm it passes**

Run on macOS: `rtk cargo test -p rollshot-app --lib daemon::macos::tray::tests::menu_ids_map_to_daemon_events`
Expected: PASS (this pure mapping is the unit-testable core of the tray; the native status item is verified manually in Task 6).

- [ ] **Step 4: Add the native `TrayGuard`**

Append to `crates/rollshot-app/src/daemon/macos/tray.rs`, above the `#[cfg(test)]` module:

```rust
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use winit::event_loop::EventLoopProxy;

/// Owns the macOS status item and its menu for the daemon lifetime. The menu
/// exposes exactly the two product actions. `MenuEvent` is a process-global
/// handler, so `Drop` clears it to avoid a stale closure outliving the daemon.
pub(crate) struct TrayGuard {
    _tray: TrayIcon,
}

impl TrayGuard {
    pub(crate) fn start(proxy: EventLoopProxy<DaemonEvent>) -> Result<Self, String> {
        let menu = Menu::new();
        let capture = MenuItem::with_id(MenuId::new(CAPTURE_ID), "Capture Region", true, None);
        let quit = MenuItem::with_id(MenuId::new(QUIT_ID), "Quit Rollshot", true, None);
        menu.append(&capture)
            .map_err(|error| format!("failed to build tray menu: {error}"))?;
        menu.append(&quit)
            .map_err(|error| format!("failed to build tray menu: {error}"))?;

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(daemon_event) = daemon_event_for(event.id.as_ref()) {
                let _ = proxy.send_event(daemon_event);
            }
        }));

        // Title-only status item: no embedded icon asset is required and the
        // item stays visible in the menu bar on macOS.
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_title("Rollshot")
            .with_tooltip("Rollshot")
            .build()
            .map_err(|error| format!("failed to create macOS tray icon: {error}"))?;

        Ok(Self { _tray: tray })
    }
}

impl Drop for TrayGuard {
    fn drop(&mut self) {
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    }
}
```

> If the Task 1 spike recorded a different `set_event_handler` shape (e.g. a boxed `Box<dyn Fn(MenuEvent) + Send + Sync>` argument, or a different `None` turbofish), use the spike's exact form here and in Task 4.

- [ ] **Step 5: Confirm the package still compiles and the mapping test still passes**

Run on macOS: `rtk cargo test -p rollshot-app --lib daemon::macos::tray::`
Expected: PASS. (The `TrayGuard` itself is exercised manually in Task 6; this step verifies it compiles against the pinned `tray-icon`.)

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-app/src/daemon/macos.rs crates/rollshot-app/src/daemon/macos/tray.rs
git commit -m "feat(daemon): add macOS tray status item"
```

---

### Task 4: macOS global shortcut

Register `Command+Shift+6` (or the configured shortcut) through `global-hotkey`, translating the platform-neutral `Shortcut` into a native `HotKey`. Activation posts `CaptureRegion` through the proxy. Registration failure is non-fatal (Task 5 degrades to tray-only).

**Files:**
- Modify: `crates/rollshot-app/src/daemon/macos.rs` (register the `shortcut` submodule)
- Create: `crates/rollshot-app/src/daemon/macos/shortcut.rs`

**Interfaces:**
- Consumes: `crate::daemon::config::{Modifier, Shortcut}` (public from Task 2); `crate::daemon::core::DaemonEvent`; `winit::event_loop::EventLoopProxy`.
- Produces (for Task 5):
  - `pub(crate) fn to_hotkey(shortcut: &Shortcut) -> Result<global_hotkey::hotkey::HotKey, String>`
  - `pub(crate) struct ShortcutGuard` with `pub(crate) fn start(proxy: EventLoopProxy<DaemonEvent>, shortcut: &Shortcut) -> Result<ShortcutGuard, String>` and a `Drop` that clears the global hotkey handler and unregisters the hotkey.

- [ ] **Step 1: Register the shortcut submodule**

In `crates/rollshot-app/src/daemon/macos.rs`, add below the `tray` line:

```rust
pub(crate) mod shortcut;
```

- [ ] **Step 2: Write the failing translation tests**

Create `crates/rollshot-app/src/daemon/macos/shortcut.rs` with the translation and tests first:

```rust
use crate::daemon::config::{Modifier, Shortcut};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};

/// Translate the platform-neutral shortcut into a `global-hotkey` `HotKey`.
/// `Command` and `Super` both map to the macOS `META` (Command) modifier.
pub(crate) fn to_hotkey(shortcut: &Shortcut) -> Result<HotKey, String> {
    let mut modifiers = Modifiers::empty();
    for modifier in shortcut.modifiers() {
        modifiers |= match modifier {
            Modifier::Control => Modifiers::CONTROL,
            Modifier::Alt => Modifiers::ALT,
            Modifier::Shift => Modifiers::SHIFT,
            Modifier::Command | Modifier::Super => Modifiers::META,
        };
    }
    let code = key_to_code(shortcut.key())?;
    Ok(HotKey::new(Some(modifiers), code))
}

/// Map the normalized base key (one lowercase ASCII letter/digit, or `F1`..`F24`)
/// to a W3C UI-Events `Code` via its canonical name.
fn key_to_code(key: &str) -> Result<Code, String> {
    let name = if key.starts_with('F') && key.len() >= 2 && key[1..].chars().all(|c| c.is_ascii_digit())
    {
        key.to_string()
    } else if key.len() == 1 {
        let ch = key.chars().next().expect("len == 1");
        if ch.is_ascii_digit() {
            format!("Digit{ch}")
        } else {
            format!("Key{}", ch.to_ascii_uppercase())
        }
    } else {
        return Err(format!("unsupported shortcut key: {key}"));
    };
    name.parse::<Code>()
        .map_err(|_| format!("unsupported shortcut key: {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_macos_default_to_command_shift_digit6() {
        let shortcut: Shortcut = "Command+Shift+6".parse().unwrap();
        let hotkey = to_hotkey(&shortcut).unwrap();
        assert_eq!(hotkey.mods, Modifiers::META | Modifiers::SHIFT);
        assert_eq!(hotkey.key, Code::Digit6);
    }

    #[test]
    fn translates_letter_and_function_keys() {
        assert_eq!(
            to_hotkey(&"Command+A".parse().unwrap()).unwrap().key,
            Code::KeyA
        );
        assert_eq!(
            to_hotkey(&"Command+F6".parse().unwrap()).unwrap().key,
            Code::F6
        );
    }

    #[test]
    fn super_maps_to_command_meta() {
        let hotkey = to_hotkey(&"Super+Shift+6".parse().unwrap()).unwrap();
        assert_eq!(hotkey.mods, Modifiers::META | Modifiers::SHIFT);
    }
}
```

- [ ] **Step 3: Run the translation tests and confirm they pass**

Run on macOS: `rtk cargo test -p rollshot-app --lib daemon::macos::shortcut::tests`
Expected: PASS.

> If the spike recorded that `HotKey` exposes `mods`/`key` via methods rather than public fields, adjust the assertions (e.g. `hotkey.key()`); keep the test asserting `META | SHIFT` and `Digit6`.

- [ ] **Step 4: Add the native `ShortcutGuard`**

Append to `crates/rollshot-app/src/daemon/macos/shortcut.rs`, above the `#[cfg(test)]` module:

```rust
use crate::daemon::core::DaemonEvent;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use winit::event_loop::EventLoopProxy;

/// Owns the `global-hotkey` manager and the registered capture hotkey for the
/// daemon lifetime. `GlobalHotKeyEvent` is a process-global handler, so `Drop`
/// clears it and unregisters the hotkey.
pub(crate) struct ShortcutGuard {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
}

impl ShortcutGuard {
    pub(crate) fn start(
        proxy: EventLoopProxy<DaemonEvent>,
        shortcut: &Shortcut,
    ) -> Result<Self, String> {
        let hotkey = to_hotkey(shortcut)?;
        let manager = GlobalHotKeyManager::new()
            .map_err(|error| format!("failed to initialize global hotkey manager: {error}"))?;
        manager
            .register(hotkey)
            .map_err(|error| format!("failed to register capture hotkey: {error}"))?;

        let registered_id = hotkey.id();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.id() == registered_id && event.state() == HotKeyState::Pressed {
                let _ = proxy.send_event(DaemonEvent::CaptureRegion);
            }
        }));

        Ok(Self { manager, hotkey })
    }
}

impl Drop for ShortcutGuard {
    fn drop(&mut self) {
        GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
        let _ = self.manager.unregister(self.hotkey);
    }
}
```

- [ ] **Step 5: Confirm the package compiles on macOS**

Run on macOS: `rtk cargo test -p rollshot-app --lib daemon::macos::shortcut::`
Expected: PASS (translation tests; live registration is verified manually in Task 6).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-app/src/daemon/macos.rs crates/rollshot-app/src/daemon/macos/shortcut.rs
git commit -m "feat(daemon): register macOS global shortcut"
```

---

### Task 5: Wire the macOS daemon event loop end to end

Compose the adapter: build the winit event loop with `Accessory` policy, create the proxy and the watcher→proxy forwarder, start the tray (fatal on failure) and the shortcut (non-fatal), and drive the shared `DaemonCore` from `user_event`. Add the macOS `run_primary`.

**Files:**
- Modify: `crates/rollshot-app/src/daemon/macos.rs` (adapter composition + event loop)
- Modify: `crates/rollshot-app/src/daemon/mod.rs` (macOS `run_primary`)

**Interfaces:**
- Consumes: `tray::TrayGuard`, `shortcut::ShortcutGuard` (Tasks 3–4); `DaemonCore`, `DaemonEvent`, `LoopAction` (`core.rs`); `CurrentExeLauncher` (`process.rs`); `DaemonConfig` (`config.rs`).
- Produces: `pub fn macos::run(core: DaemonCore<CurrentExeLauncher>, capture_exits: std::sync::mpsc::Receiver<DaemonEvent>, config: &DaemonConfig) -> Result<(), String>`; and a `#[cfg(target_os = "macos")]` `run_primary` calling it.

- [ ] **Step 1: Write a failing test for the tray-only degrade decision**

The composition's startup policy (tray fatal, shortcut non-fatal) is the unit-testable seam, mirroring the Linux `start_parts` test. Add this pure helper and test to `crates/rollshot-app/src/daemon/macos.rs` (below the module declarations):

```rust
/// Startup policy shared by the real loop and tests: the tray is required; the
/// shortcut is best-effort. Returns the tray result and an optional shortcut,
/// logging when the shortcut degrades to tray-only.
fn start_parts<T, S>(
    start_tray: impl FnOnce() -> Result<T, String>,
    start_shortcut: impl FnOnce() -> Result<S, String>,
) -> Result<(T, Option<S>), String> {
    let tray = start_tray()?;
    let shortcut = match start_shortcut() {
        Ok(shortcut) => Some(shortcut),
        Err(error) => {
            tracing::warn!(
                target: "rollshot::daemon::shortcut",
                %error,
                "global shortcut unavailable; continuing with tray only"
            );
            None
        }
    };
    Ok((tray, shortcut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_failure_aborts_platform_startup() {
        assert!(start_parts::<(), ()>(|| Err("no tray".into()), || Ok(())).is_err());
    }

    #[test]
    fn shortcut_failure_keeps_tray_alive() {
        let (tray, shortcut) = start_parts(|| Ok(7), || Err::<(), _>("denied".into())).unwrap();
        assert_eq!(tray, 7);
        assert!(shortcut.is_none());
    }
}
```

- [ ] **Step 2: Run the tests and confirm they pass**

Run on macOS: `rtk cargo test -p rollshot-app --lib daemon::macos::tests`
Expected: PASS.

- [ ] **Step 3: Add the event loop and `run` composition**

Add to `crates/rollshot-app/src/daemon/macos.rs` (the full module head plus the loop). The final top of the file is:

```rust
//! macOS daemon adapter.
pub(crate) mod shortcut;
pub(crate) mod tray;

use crate::daemon::config::{DaemonConfig, Shortcut};
use crate::daemon::core::{DaemonCore, DaemonEvent, LoopAction};
use crate::daemon::process::CurrentExeLauncher;
use std::sync::mpsc::Receiver;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::window::WindowId;
```

Then add the `start_parts` helper from Step 1 (keep it), and below it:

```rust
/// The daemon's winit application. Holds the shared core plus the platform
/// guards; the guards are created in `resumed` (after the NSApplication has
/// launched) and dropped when the loop exits.
struct DaemonApp {
    core: DaemonCore<CurrentExeLauncher>,
    hotkey: Shortcut,
    proxy: EventLoopProxy<DaemonEvent>,
    tray: Option<tray::TrayGuard>,
    shortcut: Option<shortcut::ShortcutGuard>,
    started: bool,
    startup_error: Option<String>,
}

impl ApplicationHandler<DaemonEvent> for DaemonApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        event_loop.set_control_flow(ControlFlow::Wait);

        let proxy = self.proxy.clone();
        let hotkey = self.hotkey.clone();
        match start_parts(
            || tray::TrayGuard::start(proxy.clone()),
            || shortcut::ShortcutGuard::start(proxy.clone(), &hotkey),
        ) {
            Ok((tray, shortcut)) => {
                self.tray = Some(tray);
                self.shortcut = shortcut;
                tracing::info!(
                    target: "rollshot::daemon::core",
                    version = env!("CARGO_PKG_VERSION"),
                    os = std::env::consts::OS,
                    preferred_shortcut = %self.hotkey,
                    shortcut_active = self.shortcut.is_some(),
                    "Rollshot tray daemon ready"
                );
            }
            Err(error) => {
                // Tray init failure is fatal (spec): record it and exit; the
                // outer `run` surfaces it as an error.
                self.startup_error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DaemonEvent) {
        if self.core.handle(event) == LoopAction::Exit {
            event_loop.exit();
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {
        // The daemon owns no windows; capture runs as a child process.
    }
}

/// Run the macOS daemon on the main thread. Owns the winit loop and the
/// forwarder that bridges watcher-thread `CaptureExited` events into it. Returns
/// when the user quits (clean teardown via guard `Drop`) or with the fatal tray
/// error if startup failed.
pub fn run(
    core: DaemonCore<CurrentExeLauncher>,
    capture_exits: Receiver<DaemonEvent>,
    config: &DaemonConfig,
) -> Result<(), String> {
    let event_loop = EventLoop::<DaemonEvent>::with_user_event()
        .with_activation_policy(ActivationPolicy::Accessory)
        .build()
        .map_err(|error| format!("failed to build macOS event loop: {error}"))?;
    let proxy = event_loop.create_proxy();

    // Bridge the capture watcher thread (which sends `CaptureExited` over the
    // core's mpsc sender) into the main-thread loop. Detached; it ends when the
    // sender (owned by the core) drops or the loop closes.
    let forward = proxy.clone();
    std::thread::Builder::new()
        .name("rollshot-daemon-forward".into())
        .spawn(move || {
            while let Ok(event) = capture_exits.recv() {
                if forward.send_event(event).is_err() {
                    break;
                }
            }
        })
        .map_err(|error| format!("failed to start daemon forwarder thread: {error}"))?;

    let mut app = DaemonApp {
        core,
        hotkey: config.capture_region_hotkey.clone(),
        proxy,
        tray: None,
        shortcut: None,
        started: false,
        startup_error: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("macOS daemon event loop failed: {error}"))?;

    if let Some(error) = app.startup_error.take() {
        return Err(error);
    }
    Ok(())
}
```

> If the spike recorded that `run_app` does **not** return on macOS (older winit behavior), the teardown must instead happen inside `user_event` before `exit()` (explicitly drop the guards there) — record that in the task notes and adjust. With winit 0.30 `run_app`, the return path above is expected.

- [ ] **Step 4: Add the macOS `run_primary` and narrow the fallback**

In `crates/rollshot-app/src/daemon/mod.rs`, first narrow the existing unimplemented fallback so it no longer claims macOS (this and the new macOS `run_primary` must land together — otherwise macOS has either no `run_primary` or two):

```rust
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn run_primary(_instance: instance::InstanceGuard) -> Result<(), String> {
    Err("daemon mode is not implemented on this platform yet".into())
}
```

Then add the macOS `run_primary` (next to the Linux one):

```rust
#[cfg(target_os = "macos")]
fn run_primary(_instance: instance::InstanceGuard) -> Result<(), String> {
    let config_path = config::config_path()?;
    let loaded = config::load_from(&config_path, config::Platform::Macos);
    if let Some(warning) = loaded.warning {
        tracing::warn!(
            target: "rollshot::daemon::config",
            %warning,
            "using default daemon configuration"
        );
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to resolve Rollshot executable: {error}"))?;
    let (events, receiver) = std::sync::mpsc::channel();
    let launcher = process::CurrentExeLauncher::new(executable);
    let core = core::DaemonCore::new(launcher, events);

    macos::run(core, receiver, &loaded.config)
}
```

- [ ] **Step 5: Build and run the existing daemon unit tests on macOS**

Run on macOS: `rtk cargo test -p rollshot-app --lib daemon::`
Expected: PASS — all of `daemon::config`, `daemon::core`, `daemon::instance`, `daemon::process`, `daemon::macos::tray`, `daemon::macos::shortcut`, and `daemon::macos::tests`.

- [ ] **Step 6: Build the binary on macOS**

Run on macOS: `rtk cargo build -p rollshot-app`
Expected: builds cleanly (confirms `tray-icon`/`global-hotkey`/`winit` resolve with the workspace's existing `objc2` versions; if a duplicate `objc2` major causes a conflict, pin to the versions the spike confirmed).

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-app/src/daemon/macos.rs crates/rollshot-app/src/daemon/mod.rs
git commit -m "feat(daemon): wire macOS daemon event loop"
```

---

### Task 6: Document and verify the macOS daemon

Document macOS daemon usage and run the required repository checks plus the spec's deferred macOS manual verification.

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`

- [ ] **Step 1: Document macOS daemon usage in `README.md`**

Add a macOS subsection beside the existing KDE daemon documentation. Include:

```markdown
### System tray daemon (macOS)

Run `rollshot-app daemon` to start a menu-bar status item (no Dock icon). The
item's menu has two actions:

- **Capture Region** — opens region selection in Screenshot mode (switch to
  Scrolling from the overlay toolbar after selecting a crop).
- **Quit Rollshot** — terminates any active capture and exits the daemon.

The global shortcut defaults to **Command+Shift+6**. Override it in
`~/Library/Application Support/rollshot/config.toml`:

    [daemon]
    capture_region_hotkey = "Command+Shift+6"

If the shortcut cannot be registered (e.g. another app owns it), the daemon
logs a warning and keeps working through the menu. Starting a second daemon
exits immediately without a second menu item. The shortcut uses Carbon hotkey
registration and does not require Accessibility permission.
```

- [ ] **Step 2: Note the macOS adapter in `AGENTS.md`**

In the `crates/rollshot-app` project-map entry (§9), append a sentence noting that the daemon now has a macOS adapter (`daemon/macos*`, winit + `tray-icon` + `global-hotkey`) alongside the Linux KDE adapter, both driving the shared `daemon/core.rs`.

- [ ] **Step 3: Run the required repository checks**

Run on macOS (the macOS adapter only compiles there):

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all pass with no warnings. Also run `rtk cargo test` and `rtk cargo clippy --workspace --all-targets -- -D warnings` on Linux to confirm the cross-platform changes (config accessors, `process` cfg, `mod.rs`) did not regress the Linux build.

- [ ] **Step 4: Manual macOS verification (spec §"Deferred macOS verification" and §"User-facing behavior")**

On a macOS machine running `rollshot-app daemon`, confirm:
- The menu-bar status item appears with exactly `Capture Region` and `Quit Rollshot`; no Dock icon.
- `Capture Region` (menu) launches a region screenshot capture; selecting a crop and switching to Scrolling in the overlay toolbar completes a scrolling capture through the existing macOS capture / Result Workspace flow.
- `Command+Shift+6` triggers the same capture with no Accessibility prompt.
- Triggering repeatedly while a capture is active starts no second capture (busy-trigger ignored).
- Starting a second `rollshot-app daemon` exits successfully with no second status item (single instance).
- With the shortcut taken by another app (or an invalid `capture_region_hotkey` in config), the daemon starts tray-only and logs the warning; the menu still captures.
- `Quit Rollshot` while idle exits cleanly; `Quit Rollshot` while a capture is active terminates the capture process group (verify with Activity Monitor: no orphaned `rollshot-app capture` process) before the daemon exits.
- Running `rollshot-app` with no subcommand and `rollshot-app capture …` behave exactly as before (the daemon mode is additive).

- [ ] **Step 5: Commit**

```bash
git add README.md AGENTS.md
git commit -m "docs(daemon): document macOS tray daemon"
```

---

## Test strategy

### Automated (per the spec's test matrix; reused vs. new)

| Spec requirement | Coverage | Where |
|---|---|---|
| CLI parsing (`daemon` mode, capture options unchanged, no-subcommand unchanged) | Reused, unchanged | `launch.rs` tests (already passing on all targets) |
| Config: absent→default, valid override, unreadable/malformed/invalid→fallback+warning | Reused, unchanged | `daemon/config.rs` tests |
| Config: macOS default is `Command+Shift+6` | Reused | `daemon::config::tests::macos_default_keeps_command_first` |
| Config: adapter accessors expose modifiers/key | New (Task 2) | `daemon::config::tests::shortcut_exposes_modifiers_and_key_for_adapters` |
| State machine: idle→one capture, busy ignored, child-exit→idle, spawn-fail→idle, quit cleanup, quit-while-capturing terminates child | Reused, now also run on macOS | `daemon/core.rs` tests |
| Single instance: first succeeds, second reports running, drop allows reacquire | Reused, cross-platform | `daemon/instance.rs` tests |
| Child process: success, non-zero exit, spawn failure, graceful + forced termination, descendant cleanup | Reused, now also run on macOS | `daemon/process.rs` tests |
| Tray menu id → semantic event | New (Task 3) | `daemon::macos::tray::tests` |
| Shortcut translation (default, letters, function keys, Super→Command) | New (Task 4) | `daemon::macos::shortcut::tests` |
| Platform orchestration: tray failure aborts; shortcut failure preserves tray | New (Task 5) | `daemon::macos::tests` (`start_parts`) |

### Manual (macOS hardware required)

Live `NSStatusItem` visibility, real hotkey activation, single-instance behavior with a real lock, child-process termination, and compatibility with the existing macOS capture/Result-Workspace flow — covered by Task 6 Step 4. These mirror exactly the manual items the Linux slice deferred to a desktop session, because they require a logged-in macOS GUI session.

## Failure-mode audit

- **Tray init fails** → `resumed` records `startup_error` and calls `exit()`; `run` returns `Err`, the daemon exits non-zero; the instance guard drops and releases the lock. (Spec: fatal.)
- **Hotkey registration fails** (conflict, manager init) → `start_parts` logs a warning and yields `None`; the daemon runs tray-only. (Spec: non-fatal.)
- **Capture spawn fails** → `DaemonCore::handle(CaptureRegion)` logs the error and stays `Idle`. (Reused core; spec: non-fatal.)
- **Capture exits non-zero / cancelled** → watcher sends `CaptureExited`, forwarder → proxy → core returns to `Idle`. The daemon never exits on capture failure. (Reused core.)
- **Second daemon** → `run_if_primary` sees `AlreadyRunning`, logs info, returns `Ok` before any tray/loop is created. (Reused; macOS lock path is Application Support.)
- **Quit with active capture** → core terminates the process group (SIGTERM→2s→SIGKILL) inside `handle(Quit)`, then `exit()`; guards drop in order; lock released last. The `DaemonCore::Drop` backstop also fires on the fatal-tray path.
- **Forwarder thread** → detached; ends when the core's mpsc sender drops (on `DaemonApp` drop) or the proxy closes. No join needed; matches the Linux watcher-thread lifecycle.

## Task dependency and execution strategy

```text
Task 1 (spike gate, macOS) ── confirms versions + APIs
        │
        v
Task 2 (shared core on macOS) ── compiles core/process/config on macOS
        │
        ├──> Task 3 (tray)      ─┐
        └──> Task 4 (shortcut)  ─┤  (independent; can be done in either order)
                                 v
                          Task 5 (event loop wiring) ── needs Tasks 3 + 4
                                 │
                                 v
                          Task 6 (docs + verification)
```

Tasks 3 and 4 are independent leaf modules and may be implemented in either order or in parallel; both are consumed by Task 5. Every task except Task 1 ends with a committable, independently testable deliverable. Task 1 is a gate: do not start Task 3+ until its observations hold (Task 2 is safe regardless, as it is the cross-platform reuse change).

## Completion criteria

- `rollshot-app daemon` on macOS shows a menu-bar status item with `Capture Region` and `Quit Rollshot`, registers `Command+Shift+6`, and starts region-capture child processes that complete through the existing macOS flow.
- Single-instance, busy-trigger-ignored, configuration-fallback, tray-fatal/shortcut-non-fatal, and quit-terminates-child behaviors all hold (verified per the matrix and Task 6 Step 4).
- The shared `DaemonCore`, `InstanceGuard`, config loader, and `CurrentExeLauncher` are reused with no behavioral change; the Linux adapter is untouched.
- `rtk cargo test`, `rtk cargo fmt --check`, and `rtk cargo clippy --workspace --all-targets -- -D warnings` pass on macOS, and the cross-platform changes do not regress the Linux build.
- `README.md` and `AGENTS.md` document the macOS daemon.
