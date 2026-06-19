# Thin Daemon Linux KDE 6 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `rollshot-app daemon` with a persistent KDE Plasma 6 tray, one configurable XDG portal global shortcut, single-instance enforcement, and isolated region-capture child processes.

**Architecture:** Keep daemon orchestration inside `rollshot-app`, split into focused configuration, lock, child-process, state-machine, and Linux adapter modules. Linux uses `ksni` for the StatusNotifierItem tray and `ashpd` for the GlobalShortcuts portal; the daemon launches the current executable as a separate `capture --workflow screenshot --scope region` process and owns its Unix process group.

**Tech Stack:** Rust 2021, clap, serde/toml, fs4, std channels/threads, ksni, ashpd 0.9, tokio current-thread runtime, futures-util, nix signals, tracing.

---

## Scope

This plan implements:

- shared daemon core and configuration;
- Linux KDE Plasma 6 on Wayland;
- SNI tray with `Capture Region` and `Quit Rollshot`;
- XDG GlobalShortcuts portal binding;
- one active capture child at a time;
- graceful then forced child-process-group termination;
- documentation and automated verification.

This plan does not implement macOS daemon support, GNOME, X11 fallback,
autostart, live configuration reload, or a settings window. The approved design
for those boundaries remains in
`docs/superpowers/specs/2026-06-19-thin-daemon-design.md`.

## File map

- `Cargo.toml`: add workspace dependencies and the shared Linux desktop crate.
- `crates/rollshot-linux-desktop/Cargo.toml`: new Linux desktop integration
  helper crate.
- `crates/rollshot-linux-desktop/src/lib.rs`: shared SNI host detection.
- `crates/rollshot-iced-overlay/Cargo.toml`: consume the shared SNI helper for
  Action Guide.
- `crates/rollshot-iced-overlay/src/recording_tray.rs`: remove duplicated SNI
  host probing.
- `crates/rollshot-app/Cargo.toml`: daemon dependencies.
- `crates/rollshot-app/src/daemon/mod.rs`: startup ordering and daemon event
  loop.
- `crates/rollshot-app/src/daemon/config.rs`: config path, TOML loading,
  shortcut parsing, and portal trigger translation.
- `crates/rollshot-app/src/daemon/instance.rs`: exclusive daemon file lock.
- `crates/rollshot-app/src/daemon/core.rs`: platform-neutral state machine.
- `crates/rollshot-app/src/daemon/process.rs`: current-executable capture
  launcher, child watcher, and process-group termination.
- `crates/rollshot-app/src/daemon/linux.rs`: KDE adapter composition and
  tray-only fallback.
- `crates/rollshot-app/src/daemon/linux/tray.rs`: persistent ksni tray.
- `crates/rollshot-app/src/daemon/linux/shortcut.rs`: ashpd portal session and
  activation stream.
- `crates/rollshot-app/src/launch.rs`: `daemon` CLI mode.
- `crates/rollshot-app/src/main.rs`: dispatch daemon mode.
- `README.md`: KDE daemon usage, configuration, and limitations.
- `docs/superpowers/specs/2026-06-19-thin-daemon-design.md`: no edits; it is
  the approved source of truth for this implementation.

### Task 1: Parse daemon configuration and shortcut syntax

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/rollshot-app/Cargo.toml`
- Create: `crates/rollshot-app/src/daemon/mod.rs`
- Create: `crates/rollshot-app/src/daemon/config.rs`
- Modify: `crates/rollshot-app/src/main.rs`

- [ ] **Step 1: Add failing configuration tests**

Create `daemon/mod.rs` with `mod config;`, register `mod daemon;` in `main.rs`,
and add these tests in `daemon/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_file_uses_linux_default_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_from(&dir.path().join("config.toml"), Platform::Linux);

        assert_eq!(loaded.config.capture_region_hotkey.to_string(), "Alt+Shift+6");
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn valid_file_overrides_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[daemon]\ncapture_region_hotkey = \"Control+Alt+7\"\n",
        )
        .unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(
            loaded.config.capture_region_hotkey.to_string(),
            "Control+Alt+7"
        );
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn malformed_toml_falls_back_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[daemon\n").unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("parse"));
    }

    #[test]
    fn invalid_shortcut_falls_back_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[daemon]\ncapture_region_hotkey = \"Alt+Shift\"\n",
        )
        .unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("shortcut"));
    }

    #[test]
    fn unreadable_path_falls_back_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_from(dir.path(), Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("read"));
    }

    #[test]
    fn linux_portal_trigger_uses_xdg_modifier_names() {
        let shortcut: Shortcut = "Command+Control+Alt+Shift+6".parse().unwrap();
        assert_eq!(shortcut.portal_trigger(), "CTRL+ALT+SHIFT+LOGO+6");
    }

    #[test]
    fn macos_default_keeps_command_first() {
        assert_eq!(
            DaemonConfig::default_for(Platform::Macos)
                .capture_region_hotkey
                .to_string(),
            "Command+Shift+6"
        );
    }
}
```

- [ ] **Step 2: Run the focused tests and verify the red state**

Run:

```bash
rtk cargo test -p rollshot-app daemon::config::tests
```

Expected: compilation fails because `load_from`, `Platform`, `DaemonConfig`,
and `Shortcut` do not exist.

- [ ] **Step 3: Add the configuration dependencies**

Add to workspace dependencies:

```toml
toml = "1.1"
```

Add to `rollshot-app` dependencies:

```toml
serde = { workspace = true }
toml = { workspace = true }
```

- [ ] **Step 4: Implement the minimal configuration model**

Implement these concrete types and behavior in `daemon/config.rs`:

```rust
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Control,
    Alt,
    Shift,
    Command,
    Super,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    modifiers: Vec<Modifier>,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub capture_region_hotkey: Shortcut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub config: DaemonConfig,
    pub warning: Option<String>,
}

#[derive(Deserialize)]
struct ConfigFile {
    daemon: RawDaemonConfig,
}

#[derive(Deserialize)]
struct RawDaemonConfig {
    capture_region_hotkey: String,
}

impl DaemonConfig {
    pub fn default_for(platform: Platform) -> Self {
        let text = match platform {
            Platform::Linux => "Alt+Shift+6",
            Platform::Macos => "Command+Shift+6",
        };
        Self {
            capture_region_hotkey: text.parse().expect("platform default is valid"),
        }
    }
}

impl FromStr for Shortcut {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Vec::new();
        let mut key = None;

        for part in value.split('+').map(str::trim).filter(|part| !part.is_empty()) {
            let modifier = match part.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => Some(Modifier::Control),
                "alt" | "option" => Some(Modifier::Alt),
                "shift" => Some(Modifier::Shift),
                "command" | "cmd" => Some(Modifier::Command),
                "super" | "logo" => Some(Modifier::Super),
                _ => None,
            };

            if let Some(modifier) = modifier {
                if modifiers.contains(&modifier) {
                    return Err(format!("duplicate modifier: {part}"));
                }
                modifiers.push(modifier);
            } else if key.replace(part.to_string()).is_some() {
                return Err("shortcut must contain exactly one base key".into());
            }
        }

        let key = key.ok_or_else(|| "shortcut must contain one base key".to_string())?;
        if !key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return Err("shortcut base key must be alphanumeric or underscore".into());
        }
        if modifiers.contains(&Modifier::Command)
            && modifiers.contains(&Modifier::Super)
        {
            return Err("Command and Super name the same platform modifier".into());
        }
        Ok(Self { modifiers, key })
    }
}

impl Shortcut {
    pub fn portal_trigger(&self) -> String {
        let mut parts = Vec::new();
        for (modifier, name) in [
            (Modifier::Control, "CTRL"),
            (Modifier::Alt, "ALT"),
            (Modifier::Shift, "SHIFT"),
            (Modifier::Command, "LOGO"),
            (Modifier::Super, "LOGO"),
        ] {
            if self.modifiers.contains(&modifier) {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = self
            .modifiers
            .iter()
            .map(|modifier| match modifier {
                Modifier::Control => "Control",
                Modifier::Alt => "Alt",
                Modifier::Shift => "Shift",
                Modifier::Command => "Command",
                Modifier::Super => "Super",
            })
            .collect();
        parts.push(&self.key);
        write!(f, "{}", parts.join("+"))
    }
}

pub fn config_path() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join("rollshot").join("config.toml"))
        .ok_or_else(|| "platform configuration directory is unavailable".to_string())
}

pub fn load_from(path: &Path, platform: Platform) -> LoadedConfig {
    let fallback = DaemonConfig::default_for(platform);
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedConfig {
                config: fallback,
                warning: None,
            };
        }
        Err(error) => {
            return LoadedConfig {
                config: fallback,
                warning: Some(format!("failed to read daemon config: {error}")),
            };
        }
    };

    let raw: ConfigFile = match toml::from_str(&text) {
        Ok(raw) => raw,
        Err(error) => {
            return LoadedConfig {
                config: fallback,
                warning: Some(format!("failed to parse daemon config: {error}")),
            };
        }
    };

    match raw.daemon.capture_region_hotkey.parse() {
        Ok(capture_region_hotkey) => LoadedConfig {
            config: DaemonConfig {
                capture_region_hotkey,
            },
            warning: None,
        },
        Err(error) => LoadedConfig {
            config: fallback,
            warning: Some(format!("invalid daemon shortcut: {error}")),
        },
    }
}
```

Keep `Platform::Macos` because it is part of the approved shared configuration
contract; do not add a macOS daemon adapter in this plan.

- [ ] **Step 5: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app daemon::config::tests
```

Expected: all five configuration tests pass.

- [ ] **Step 6: Format and commit**

Run:

```bash
rtk cargo fmt --all
rtk git add Cargo.toml Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-app/src/main.rs crates/rollshot-app/src/daemon
rtk git commit -m "feat(daemon): add configuration model"
```

### Task 2: Enforce a single daemon instance

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`
- Create: `crates/rollshot-app/src/daemon/instance.rs`

- [ ] **Step 1: Write failing lock tests**

Add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_guard_reports_already_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let first = acquire_at(&path).unwrap();
        assert!(matches!(first, AcquireResult::Acquired(_)));
        let second = acquire_at(&path).unwrap();
        assert!(matches!(second, AcquireResult::AlreadyRunning));
    }

    #[test]
    fn dropping_guard_allows_reacquisition() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let guard = match acquire_at(&path).unwrap() {
            AcquireResult::Acquired(guard) => guard,
            AcquireResult::AlreadyRunning => panic!("first lock must succeed"),
        };
        drop(guard);

        assert!(matches!(
            acquire_at(&path).unwrap(),
            AcquireResult::Acquired(_)
        ));
    }
}
```

- [ ] **Step 2: Verify the tests fail**

Run:

```bash
rtk cargo test -p rollshot-app daemon::instance::tests
```

Expected: compilation fails because the instance module and lock types are
missing.

- [ ] **Step 3: Add `fs4` and implement the guard**

Add workspace dependency:

```toml
fs4 = "1.1"
```

Add app dependency:

```toml
fs4 = { workspace = true }
```

Implement:

```rust
use fs4::{FileExt, TryLockError};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

pub struct InstanceGuard {
    _file: File,
}

pub enum AcquireResult {
    Acquired(InstanceGuard),
    AlreadyRunning,
}

pub fn lock_path() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join("rollshot").join("daemon.lock"))
        .ok_or_else(|| "platform configuration directory is unavailable".to_string())
}

pub fn acquire_at(path: &Path) -> Result<AcquireResult, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create daemon state directory: {error}"))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("failed to open daemon lock: {error}"))?;

    match file.try_lock() {
        Ok(()) => Ok(AcquireResult::Acquired(InstanceGuard { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(AcquireResult::AlreadyRunning),
        Err(TryLockError::Error(error)) => {
            Err(format!("failed to acquire daemon lock: {error}"))
        }
    }
}
```

Declare `mod instance;` in `daemon/mod.rs`.

- [ ] **Step 4: Run the lock tests**

Run:

```bash
rtk cargo test -p rollshot-app daemon::instance::tests
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
rtk cargo fmt --all
rtk git add Cargo.toml Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-app/src/daemon
rtk git commit -m "feat(daemon): enforce single instance"
```

### Task 3: Build the daemon state machine with fake capture processes

**Files:**

- Modify: `crates/rollshot-app/src/daemon/mod.rs`
- Create: `crates/rollshot-app/src/daemon/core.rs`

- [ ] **Step 1: Write failing state-machine tests**

Define tests around these exact semantic events:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeState {
        launches: usize,
        terminations: usize,
    }

    struct FakeLauncher(Arc<Mutex<FakeState>>);
    struct FakeCapture(Arc<Mutex<FakeState>>);

    impl CaptureLauncher for FakeLauncher {
        fn launch(
            &mut self,
            _id: CaptureId,
            _events: std::sync::mpsc::Sender<DaemonEvent>,
        ) -> Result<Box<dyn ActiveCapture>, String> {
            self.0.lock().unwrap().launches += 1;
            Ok(Box::new(FakeCapture(self.0.clone())))
        }
    }

    impl ActiveCapture for FakeCapture {
        fn terminate(&mut self, _grace: Duration) -> Result<(), String> {
            self.0.lock().unwrap().terminations += 1;
            Ok(())
        }
    }

    fn core() -> (DaemonCore<FakeLauncher>, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let (events, _receiver) = std::sync::mpsc::channel();
        (
            DaemonCore::new(FakeLauncher(state.clone()), events),
            state,
        )
    }

    #[test]
    fn idle_capture_event_launches_one_child() {
        let (mut core, state) = core();
        assert_eq!(
            core.handle(DaemonEvent::CaptureRegion),
            LoopAction::Continue
        );
        assert_eq!(state.lock().unwrap().launches, 1);
        assert!(core.is_capturing());
    }

    #[test]
    fn trigger_while_capturing_is_ignored() {
        let (mut core, state) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureRegion);
        assert_eq!(state.lock().unwrap().launches, 1);
    }

    #[test]
    fn matching_child_exit_returns_to_idle() {
        let (mut core, _) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureExited {
            id: CaptureId(1),
            success: true,
        });
        assert!(!core.is_capturing());
    }

    #[test]
    fn nonzero_child_exit_also_returns_to_idle() {
        let (mut core, _) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureExited {
            id: CaptureId(1),
            success: false,
        });
        assert!(!core.is_capturing());
    }

    #[test]
    fn stale_child_exit_does_not_clear_current_capture() {
        let (mut core, _) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureExited {
            id: CaptureId(99),
            success: true,
        });
        assert!(core.is_capturing());
    }

    #[test]
    fn quit_terminates_active_capture_and_exits() {
        let (mut core, state) = core();
        core.handle(DaemonEvent::CaptureRegion);
        assert_eq!(core.handle(DaemonEvent::Quit), LoopAction::Exit);
        assert_eq!(state.lock().unwrap().terminations, 1);
    }

    #[test]
    fn dropping_core_terminates_active_capture() {
        let (mut core, state) = core();
        core.handle(DaemonEvent::CaptureRegion);
        drop(core);
        assert_eq!(state.lock().unwrap().terminations, 1);
    }
}
```

Add this failing-launch regression test:

```rust
struct FailingLauncher;

impl CaptureLauncher for FailingLauncher {
    fn launch(
        &mut self,
        _id: CaptureId,
        _events: std::sync::mpsc::Sender<DaemonEvent>,
    ) -> Result<Box<dyn ActiveCapture>, String> {
        Err("spawn failed".into())
    }
}

#[test]
fn spawn_failure_leaves_core_idle() {
    let (events, _receiver) = std::sync::mpsc::channel();
    let mut core = DaemonCore::new(FailingLauncher, events);

    assert_eq!(
        core.handle(DaemonEvent::CaptureRegion),
        LoopAction::Continue
    );
    assert!(!core.is_capturing());
}
```

- [ ] **Step 2: Verify red state**

Run:

```bash
rtk cargo test -p rollshot-app daemon::core::tests
```

Expected: compilation fails because the state-machine types are missing.

- [ ] **Step 3: Implement the minimal pure core**

Implement these public-in-module contracts:

```rust
use std::sync::mpsc::Sender;
use std::time::Duration;

const QUIT_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureId(pub u64);

#[derive(Debug)]
pub enum DaemonEvent {
    CaptureRegion,
    CaptureExited { id: CaptureId, success: bool },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    Continue,
    Exit,
}

pub trait ActiveCapture: Send {
    fn terminate(&mut self, grace: Duration) -> Result<(), String>;
}

pub trait CaptureLauncher {
    fn launch(
        &mut self,
        id: CaptureId,
        events: Sender<DaemonEvent>,
    ) -> Result<Box<dyn ActiveCapture>, String>;
}

struct RunningCapture {
    id: CaptureId,
    process: Box<dyn ActiveCapture>,
}

pub struct DaemonCore<L: CaptureLauncher> {
    launcher: L,
    events: Sender<DaemonEvent>,
    active: Option<RunningCapture>,
    next_id: u64,
}

impl<L: CaptureLauncher> DaemonCore<L> {
    pub fn new(launcher: L, events: Sender<DaemonEvent>) -> Self {
        Self {
            launcher,
            events,
            active: None,
            next_id: 1,
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.active.is_some()
    }

    pub fn handle(&mut self, event: DaemonEvent) -> LoopAction {
        match event {
            DaemonEvent::CaptureRegion if self.active.is_none() => {
                let id = CaptureId(self.next_id);
                self.next_id += 1;
                match self.launcher.launch(id, self.events.clone()) {
                    Ok(process) => {
                        self.active = Some(RunningCapture { id, process });
                    }
                    Err(error) => {
                        tracing::error!(
                            target: "rollshot::daemon::process",
                            %error,
                            "failed to start capture child"
                        );
                    }
                }
                LoopAction::Continue
            }
            DaemonEvent::CaptureRegion => {
                tracing::debug!(
                    target: "rollshot::daemon::core",
                    "capture trigger ignored while capture is active"
                );
                LoopAction::Continue
            }
            DaemonEvent::CaptureExited { id, success } => {
                if self.active.as_ref().is_some_and(|active| active.id == id) {
                    self.active = None;
                    tracing::info!(
                        target: "rollshot::daemon::process",
                        capture_id = id.0,
                        success,
                        "capture child exited"
                    );
                }
                LoopAction::Continue
            }
            DaemonEvent::Quit => {
                if let Some(mut active) = self.active.take() {
                    if let Err(error) = active.process.terminate(QUIT_GRACE) {
                        tracing::warn!(
                            target: "rollshot::daemon::process",
                            %error,
                            "capture child cleanup failed"
                        );
                    }
                }
                LoopAction::Exit
            }
        }
    }
}

impl<L: CaptureLauncher> Drop for DaemonCore<L> {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            if let Err(error) = active.process.terminate(QUIT_GRACE) {
                tracing::warn!(
                    target: "rollshot::daemon::process",
                    %error,
                    "capture child cleanup failed while daemon core dropped"
                );
            }
        }
    }
}
```

Declare `mod core;` in `daemon/mod.rs`.

- [ ] **Step 4: Run focused tests**

```bash
rtk cargo test -p rollshot-app daemon::core::tests
```

Expected: all state-machine tests pass.

- [ ] **Step 5: Commit**

```bash
rtk cargo fmt --all
rtk git add crates/rollshot-app/src/daemon
rtk git commit -m "feat(daemon): add capture state machine"
```

### Task 4: Launch and terminate the isolated capture process

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`
- Create: `crates/rollshot-app/src/daemon/process.rs`

- [ ] **Step 1: Add failing command-contract tests**

Keep command construction pure and testable:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_arguments_are_region_screenshot() {
        assert_eq!(
            capture_args(),
            [
                "capture",
                "--workflow",
                "screenshot",
                "--scope",
                "region"
            ]
        );
    }

    #[test]
    fn watcher_reports_exit_for_matching_capture_id() {
        let (tx, rx) = std::sync::mpsc::channel();
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .unwrap();
        let completed = std::sync::Arc::new((
            std::sync::Mutex::new(false),
            std::sync::Condvar::new(),
        ));
        spawn_watcher(CaptureId(7), child, completed, tx);

        assert!(matches!(
            rx.recv().unwrap(),
            DaemonEvent::CaptureExited {
                id: CaptureId(7),
                success: true
            }
        ));
    }

    #[test]
    fn graceful_completion_needs_only_sigterm() {
        let completed = std::sync::Arc::new((
            std::sync::Mutex::new(false),
            std::sync::Condvar::new(),
        ));
        let notify = completed.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let (lock, condition) = &*notify;
            *lock.lock().unwrap() = true;
            condition.notify_all();
        });
        let signals = std::sync::Mutex::new(Vec::new());

        terminate_with(
            &completed,
            std::time::Duration::from_secs(1),
            |signal| {
                signals.lock().unwrap().push(signal);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(*signals.lock().unwrap(), vec![Signal::SIGTERM]);
    }

    #[test]
    fn timeout_escalates_to_sigkill() {
        let completed = std::sync::Arc::new((
            std::sync::Mutex::new(false),
            std::sync::Condvar::new(),
        ));
        let signals = std::sync::Mutex::new(Vec::new());

        terminate_with(
            &completed,
            std::time::Duration::from_millis(1),
            |signal| {
                signals.lock().unwrap().push(signal);
                if signal == Signal::SIGKILL {
                    let (lock, condition) = &*completed;
                    *lock.lock().unwrap() = true;
                    condition.notify_all();
                }
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            *signals.lock().unwrap(),
            vec![Signal::SIGTERM, Signal::SIGKILL]
        );
    }
}
```

- [ ] **Step 2: Verify red state**

```bash
rtk cargo test -p rollshot-app daemon::process::tests
```

Expected: compilation fails because the process module does not exist.

- [ ] **Step 3: Enable safe Unix process-group signaling**

Extend the workspace `nix` features:

```toml
nix = { version = "0.29", features = ["fs", "poll", "signal"] }
```

Add to Linux app dependencies:

```toml
nix = { workspace = true }
```

- [ ] **Step 4: Implement the real launcher**

Implement `CurrentExeLauncher` with:

```rust
use crate::daemon::core::{
    ActiveCapture, CaptureId, CaptureLauncher, DaemonEvent,
};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::{mpsc::Sender, Arc, Condvar, Mutex};
use std::time::Duration;

pub struct CurrentExeLauncher {
    executable: std::path::PathBuf,
}

struct ProcessGroupCapture {
    pgid: Pid,
    completed: Arc<(Mutex<bool>, Condvar)>,
}

pub fn capture_args() -> [&'static str; 5] {
    ["capture", "--workflow", "screenshot", "--scope", "region"]
}

impl CurrentExeLauncher {
    pub fn new(executable: std::path::PathBuf) -> Self {
        Self { executable }
    }
}

impl CaptureLauncher for CurrentExeLauncher {
    fn launch(
        &mut self,
        id: CaptureId,
        events: Sender<DaemonEvent>,
    ) -> Result<Box<dyn ActiveCapture>, String> {
        let mut command = Command::new(&self.executable);
        command.args(capture_args()).process_group(0);
        let child = command
            .spawn()
            .map_err(|error| format!("failed to spawn capture child: {error}"))?;
        let pgid = Pid::from_raw(child.id() as i32);
        let completed = Arc::new((Mutex::new(false), Condvar::new()));
        spawn_watcher(id, child, completed.clone(), events);
        Ok(Box::new(ProcessGroupCapture { pgid, completed }))
    }
}

fn spawn_watcher(
    id: CaptureId,
    mut child: Child,
    completed: Arc<(Mutex<bool>, Condvar)>,
    events: Sender<DaemonEvent>,
) {
    std::thread::spawn(move || {
        let success = match child.wait() {
            Ok(status) => status.success(),
            Err(error) => {
                tracing::warn!(
                    target: "rollshot::daemon::process",
                    %error,
                    "failed to wait for capture child"
                );
                false
            }
        };
        let (lock, condition) = &*completed;
        *lock.lock().unwrap() = true;
        condition.notify_all();
        let _ = events.send(DaemonEvent::CaptureExited { id, success });
    });
}

impl ActiveCapture for ProcessGroupCapture {
    fn terminate(&mut self, grace: Duration) -> Result<(), String> {
        terminate_with(&self.completed, grace, |signal| {
            match killpg(self.pgid, signal) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
                Err(error) => Err(format!(
                    "failed to signal capture process group with {signal:?}: {error}"
                )),
            }
        })
    }
}

fn terminate_with(
    completed: &Arc<(Mutex<bool>, Condvar)>,
    grace: Duration,
    mut signal: impl FnMut(Signal) -> Result<(), String>,
) -> Result<(), String> {
    let (lock, condition) = &**completed;
    if *lock.lock().unwrap() {
        return Ok(());
    }
    signal(Signal::SIGTERM)?;
    let completed = lock.lock().unwrap();
    let (completed, _) = condition
        .wait_timeout_while(completed, grace, |completed| !*completed)
        .map_err(|_| "capture completion lock was poisoned".to_string())?;
    if *completed {
        return Ok(());
    }
    drop(completed);

    signal(Signal::SIGKILL)?;
    let (completed, _) = condition
        .wait_timeout_while(
            lock.lock().unwrap(),
            Duration::from_secs(1),
            |completed| !*completed,
        )
        .map_err(|_| "capture completion lock was poisoned".to_string())?;
    if *completed {
        Ok(())
    } else {
        Err("capture process group did not exit after SIGKILL".into())
    }
}
```

The watcher is the only owner that waits on `Child`; termination waits on the
condition variable. This prevents two threads from racing to reap the same
process. Declare this module as `#[cfg(target_os = "linux")] mod process;` so
the deferred macOS build does not compile Unix process-group code yet.

- [ ] **Step 5: Run process and core tests**

```bash
rtk cargo test -p rollshot-app daemon::process::tests
rtk cargo test -p rollshot-app daemon::core::tests
```

Expected: both suites pass.

- [ ] **Step 6: Commit**

```bash
rtk cargo fmt --all
rtk git add Cargo.toml Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-app/src/daemon
rtk git commit -m "feat(daemon): manage capture child process"
```

### Task 5: Share SNI host detection with Action Guide

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/rollshot-linux-desktop/Cargo.toml`
- Create: `crates/rollshot-linux-desktop/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Modify: `crates/rollshot-iced-overlay/src/recording_tray.rs`
- Modify: `crates/rollshot-app/Cargo.toml`

- [ ] **Step 1: Create the focused shared crate**

Add `crates/rollshot-linux-desktop` to workspace members. Its manifest is:

```toml
[package]
name = "rollshot-linux-desktop"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
tracing = { workspace = true }
zbus = { version = "5", features = ["blocking"] }

[lints]
workspace = true
```

Implement only SNI host detection in `src/lib.rs`:

```rust
const TARGET_SNI: &str = "rollshot::linux_desktop::sni";

pub fn sni_host_available() -> bool {
    use zbus::blocking::{Connection, Proxy};

    let Ok(connection) = Connection::session() else {
        tracing::warn!(target: TARGET_SNI, "session bus unavailable");
        return false;
    };
    for service in [
        "org.kde.StatusNotifierWatcher",
        "org.freedesktop.StatusNotifierWatcher",
    ] {
        let Ok(proxy) = Proxy::new(
            &connection,
            service,
            "/StatusNotifierWatcher",
            service,
        ) else {
            continue;
        };
        if let Ok(true) = proxy.get_property::<bool>("IsStatusNotifierHostRegistered") {
            tracing::debug!(target: TARGET_SNI, service, "SNI host registered");
            return true;
        }
    }
    tracing::warn!(target: TARGET_SNI, "registered SNI host not found");
    false
}
```

- [ ] **Step 2: Replace the Action Guide duplicate**

Add `rollshot-linux-desktop` as an optional Linux dependency of
`rollshot-iced-overlay`, include it in the `action-guide` feature, delete the
local `sni_host_available`, and change:

```rust
if !rollshot_linux_desktop::sni_host_available() {
    return Err(OverlayError::Capture(
        "Fullscreen Action Guide requires a system tray. \
         This environment does not support tray icons."
            .to_string(),
    ));
}
```

Remove `dep:zbus` from the overlay `action-guide` feature and remove its
now-unused optional Linux `zbus` dependency; the new shared crate owns that
D-Bus probing dependency.

Add `rollshot-linux-desktop` as a Linux dependency of `rollshot-app`.

- [ ] **Step 3: Verify Action Guide and default builds**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-iced-overlay --features action-guide
rtk cargo test -p rollshot-app
```

Expected: all pass; the existing recording tray behavior remains unchanged.

- [ ] **Step 4: Commit**

```bash
rtk cargo fmt --all
rtk git add Cargo.toml Cargo.lock crates/rollshot-linux-desktop crates/rollshot-iced-overlay crates/rollshot-app/Cargo.toml
rtk git commit -m "refactor(linux): share SNI host detection"
```

### Task 6: Implement the persistent KDE tray

**Files:**

- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`
- Create: `crates/rollshot-app/src/daemon/linux.rs`
- Create: `crates/rollshot-app/src/daemon/linux/tray.rs`

- [ ] **Step 1: Write callback-routing tests**

Build `DaemonTrayItem` independently of D-Bus and test its menu callbacks:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_menu_item_sends_capture_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx);
        item.activate_capture();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::CaptureRegion));
    }

    #[test]
    fn quit_menu_item_sends_quit_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx);
        item.activate_quit();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::Quit));
    }

    #[test]
    fn menu_contains_only_capture_and_quit() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let item = DaemonTrayItem::new(tx);
        let menu = ksni::Tray::menu(&item);
        let labels: Vec<&str> = menu
            .iter()
            .map(|item| match item {
                ksni::MenuItem::Standard(item) => item.label.as_str(),
                _ => panic!("daemon tray only uses standard items"),
            })
            .collect();
        assert_eq!(labels, ["Capture Region", "Quit Rollshot"]);
    }
}
```

Keep `activate_capture` and `activate_quit` as the tested methods and make the
menu closures call those methods. This verifies semantic routing without a
D-Bus session.

- [ ] **Step 2: Verify red state**

```bash
rtk cargo test -p rollshot-app daemon::linux::tray::tests
```

Expected: compilation fails because the Linux tray module is missing.

- [ ] **Step 3: Add ksni and implement the tray guard**

Add to Linux app dependencies:

```toml
ksni = { version = "0.3", features = ["blocking"] }
```

Implement:

```rust
use crate::daemon::core::DaemonEvent;
use std::sync::mpsc::Sender;

pub struct DaemonTrayItem {
    events: Sender<DaemonEvent>,
}

impl DaemonTrayItem {
    fn new(events: Sender<DaemonEvent>) -> Self {
        Self { events }
    }

    fn activate_capture(&mut self) {
        let _ = self.events.send(DaemonEvent::CaptureRegion);
    }

    fn activate_quit(&mut self) {
        let _ = self.events.send(DaemonEvent::Quit);
    }
}

impl ksni::Tray for DaemonTrayItem {
    fn id(&self) -> String {
        "rollshot-daemon".into()
    }

    fn title(&self) -> String {
        "Rollshot".into()
    }

    fn icon_name(&self) -> String {
        "camera-photo".into()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Capture Region".into(),
                icon_name: "camera-photo".into(),
                activate: Box::new(Self::activate_capture),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit Rollshot".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(Self::activate_quit),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct TrayGuard {
    handle: ksni::blocking::Handle<DaemonTrayItem>,
}

impl TrayGuard {
    pub fn start(events: Sender<DaemonEvent>) -> Result<Self, String> {
        if !rollshot_linux_desktop::sni_host_available() {
            return Err("KDE StatusNotifierHost is unavailable".into());
        }
        use ksni::blocking::TrayMethods;
        let handle = DaemonTrayItem::new(events)
            .spawn()
            .map_err(|error| format!("failed to register Rollshot tray: {error}"))?;
        Ok(Self { handle })
    }
}

impl Drop for TrayGuard {
    fn drop(&mut self) {
        self.handle.shutdown().wait();
    }
}
```

Declare `#[cfg(target_os = "linux")] mod linux;` and `mod tray;`.

- [ ] **Step 4: Run tray tests and package tests**

```bash
rtk cargo test -p rollshot-app daemon::linux::tray::tests
rtk cargo test -p rollshot-app
```

Expected: all pass without requiring an SNI host because unit tests do not call
`TrayGuard::start`.

- [ ] **Step 5: Commit**

```bash
rtk cargo fmt --all
rtk git add Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-app/src/daemon
rtk git commit -m "feat(daemon): add KDE tray menu"
```

### Task 7: Register the KDE global shortcut through the portal

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/daemon/linux.rs`
- Create: `crates/rollshot-app/src/daemon/linux/shortcut.rs`

- [ ] **Step 1: Write pure shortcut-routing tests**

Add tests for the portal-facing constants and activation filter:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_capture_region_id_routes_to_capture() {
        assert!(is_capture_shortcut("capture-region"));
        assert!(!is_capture_shortcut("other"));
    }

    #[test]
    fn preferred_trigger_comes_from_configured_shortcut() {
        let shortcut: Shortcut = "Alt+Shift+6".parse().unwrap();
        assert_eq!(preferred_trigger(&shortcut), "ALT+SHIFT+6");
    }
}
```

- [ ] **Step 2: Verify red state**

```bash
rtk cargo test -p rollshot-app daemon::linux::shortcut::tests
```

Expected: compilation fails because the shortcut module is missing.

- [ ] **Step 3: Add portal runtime dependencies**

Add workspace dependency:

```toml
futures-util = "0.3"
```

Extend workspace tokio features:

```toml
tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }
```

Add Linux app dependencies:

```toml
ashpd = { workspace = true }
futures-util = { workspace = true }
tokio = { workspace = true }
```

- [ ] **Step 4: Implement the portal thread and RAII shutdown**

Implement `ShortcutGuard::start` as a dedicated named thread. The thread owns a
current-thread tokio runtime and the complete portal session:

```rust
use crate::daemon::config::Shortcut;
use crate::daemon::core::DaemonEvent;

const SHORTCUT_ID: &str = "capture-region";

pub struct ShortcutGuard {
    shutdown: tokio::sync::watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub fn is_capture_shortcut(id: &str) -> bool {
    id == SHORTCUT_ID
}

pub fn preferred_trigger(shortcut: &Shortcut) -> String {
    shortcut.portal_trigger()
}

impl ShortcutGuard {
    pub fn start(
        events: std::sync::mpsc::Sender<DaemonEvent>,
        shortcut: &Shortcut,
    ) -> Result<Self, String> {
        let preferred = preferred_trigger(shortcut);
        let (shutdown, receiver) = tokio::sync::watch::channel(false);
        let thread = std::thread::Builder::new()
            .name("rollshot-global-shortcut".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        tracing::warn!(
                            target: "rollshot::daemon::shortcut",
                            %error,
                            "failed to create global shortcut runtime"
                        );
                        return;
                    }
                };
                if let Err(error) = runtime.block_on(run_portal(
                    events,
                    preferred,
                    receiver,
                )) {
                    tracing::warn!(
                        target: "rollshot::daemon::shortcut",
                        %error,
                        "global shortcut unavailable; tray remains active"
                    );
                }
            })
            .map_err(|error| format!("failed to start shortcut thread: {error}"))?;
        Ok(Self {
            shutdown,
            thread: Some(thread),
        })
    }
}

impl Drop for ShortcutGuard {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::warn!(
                    target: "rollshot::daemon::shortcut",
                    "global shortcut thread panicked during shutdown"
                );
            }
        }
    }
}
```

Define `run_portal` with this signature and sequence:

```rust
async fn run_portal(
    events: std::sync::mpsc::Sender<DaemonEvent>,
    preferred: String,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    let portal = ashpd::desktop::global_shortcuts::GlobalShortcuts::new()
        .await
        .map_err(|error| error.to_string())?;
    let session = portal
        .create_session()
        .await
        .map_err(|error| error.to_string())?;
    let shortcut = ashpd::desktop::global_shortcuts::NewShortcut::new(
        SHORTCUT_ID,
        "Capture a Rollshot region",
    )
    .preferred_trigger(Some(preferred.as_str()));
    let shortcuts = [shortcut];
    let parent = ashpd::WindowIdentifier::default();
    let request = tokio::select! {
        request = portal.bind_shortcuts(
            &session,
            &shortcuts,
            &parent,
        ) => {
            request.map_err(|error| error.to_string())?
        },
        changed = shutdown.changed() => {
            let _ = changed;
            session.close().await.map_err(|error| error.to_string())?;
            return Ok(());
        },
    };
    let response = request.response().map_err(|error| error.to_string())?;

    if !response
        .shortcuts()
        .iter()
        .any(|shortcut| shortcut.id() == SHORTCUT_ID)
    {
        return Err("portal did not bind capture-region".into());
    }

    let mut activated = portal
        .receive_activated()
        .await
        .map_err(|error| error.to_string())?;
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                let _ = session.close().await;
                return Ok(());
            },
            event = futures_util::StreamExt::next(&mut activated) => {
                let Some(event) = event else {
                    return Err("global shortcut activation stream closed".into());
                };
                if is_capture_shortcut(event.shortcut_id()) {
                    let _ = events.send(DaemonEvent::CaptureRegion);
                }
            },
        }
    }
}
```

The thread exits after logging and does not stop the daemon. Do not log raw key
events.

- [ ] **Step 5: Add platform startup fallback tests**

In `linux.rs`, implement a small injectable composition helper:

```rust
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
```

Test:

```rust
#[test]
fn tray_failure_aborts_platform_startup() {
    assert!(start_parts::<(), ()>(|| Err("no tray".into()), || Ok(())).is_err());
}

#[test]
fn shortcut_failure_keeps_tray_alive() {
    let (tray, shortcut) =
        start_parts(|| Ok(7), || Err::<(), _>("denied".into())).unwrap();
    assert_eq!(tray, 7);
    assert!(shortcut.is_none());
}
```

`LinuxPlatform::start` calls `TrayGuard::start` first, then
`ShortcutGuard::start`, and stores both guards.

```rust
use crate::daemon::config::DaemonConfig;
use crate::daemon::core::DaemonEvent;

pub struct LinuxPlatform {
    _tray: tray::TrayGuard,
    _shortcut: Option<shortcut::ShortcutGuard>,
}

impl LinuxPlatform {
    pub fn start(
        events: std::sync::mpsc::Sender<DaemonEvent>,
        config: &DaemonConfig,
    ) -> Result<Self, String> {
        let (tray, shortcut) = start_parts(
            || tray::TrayGuard::start(events.clone()),
            || shortcut::ShortcutGuard::start(
                events,
                &config.capture_region_hotkey,
            ),
        )?;
        Ok(Self {
            _tray: tray,
            _shortcut: shortcut,
        })
    }
}
```

- [ ] **Step 6: Run portal and platform tests**

```bash
rtk cargo test -p rollshot-app daemon::linux::shortcut::tests
rtk cargo test -p rollshot-app daemon::linux::tests
```

Expected: all pass without opening a portal dialog because tests cover pure
routing and injected startup results.

- [ ] **Step 7: Commit**

```bash
rtk cargo fmt --all
rtk git add Cargo.toml Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-app/src/daemon
rtk git commit -m "feat(daemon): bind KDE global shortcut"
```

### Task 8: Wire `rollshot-app daemon` end to end

**Files:**

- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`

- [ ] **Step 1: Add failing CLI tests**

Add:

```rust
#[test]
fn daemon_subcommand_selects_daemon_mode() {
    let mode = parse(&["rollshot-app", "daemon"]).expect("parse daemon");
    assert!(matches!(mode, LaunchMode::Daemon));
}

#[test]
fn no_subcommand_still_selects_default_capture() {
    let mode = parse(&["rollshot-app"]).expect("parse default");
    assert!(matches!(mode, LaunchMode::Capture(_)));
}
```

- [ ] **Step 2: Verify the CLI test fails**

```bash
rtk cargo test -p rollshot-app launch::tests::daemon_subcommand_selects_daemon_mode
```

Expected: parsing fails because `daemon` is not a known subcommand.

- [ ] **Step 3: Add launch dispatch**

Add:

```rust
// Insert in LaunchMode after Capture:
Daemon,

// Insert in LaunchCommand after Capture:
/// Run Rollshot in the system tray and listen for the capture shortcut.
Daemon,
```

Map `LaunchCommand::Daemon` to `LaunchMode::Daemon`. In `main.rs`, dispatch:

```rust
LaunchMode::Daemon => daemon::run(),
```

Do not change no-subcommand capture behavior.

- [ ] **Step 4: Implement startup ordering and event loop**

Implement `daemon::run`:

```rust
pub fn run() -> Result<(), String> {
    let lock_path = instance::lock_path()?;
    let acquired = instance::acquire_at(&lock_path)?;
    run_if_primary(acquired, run_primary)
}

fn run_if_primary(
    acquired: instance::AcquireResult,
    start: impl FnOnce(instance::InstanceGuard) -> Result<(), String>,
) -> Result<(), String> {
    match acquired {
        instance::AcquireResult::Acquired(guard) => start(guard),
        instance::AcquireResult::AlreadyRunning => {
            tracing::info!(
                target: "rollshot::daemon::instance",
                "Rollshot daemon is already running"
            );
            Ok(())
        }
    }
}

fn run_primary(_instance: instance::InstanceGuard) -> Result<(), String> {
    let config_path = config::config_path()?;
    let loaded = config::load_from(&config_path, config::Platform::Linux);
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
    let _platform = linux::LinuxPlatform::start(events.clone(), &loaded.config)?;
    let launcher = process::CurrentExeLauncher::new(executable);
    let mut core = core::DaemonCore::new(launcher, events);

    tracing::info!(
        target: "rollshot::daemon::core",
        "Rollshot daemon ready"
    );
    while let Ok(event) = receiver.recv() {
        if core.handle(event) == core::LoopAction::Exit {
            return Ok(());
        }
    }
    Err("daemon event channel closed unexpectedly".into())
}
```

Guard this Linux implementation with `#[cfg(target_os = "linux")]`. For other
targets, provide:

```rust
#[cfg(not(target_os = "linux"))]
pub fn run() -> Result<(), String> {
    Err("daemon mode is not implemented on this platform yet".into())
}
```

This is an explicit platform-slice boundary, not a claim of macOS support.

- [ ] **Step 5: Run CLI and daemon unit tests**

```bash
rtk cargo test -p rollshot-app launch::tests
rtk cargo test -p rollshot-app daemon::
```

Expected: all pass.

- [ ] **Step 6: Verify duplicate daemon behavior without a desktop session**

Add:

```rust
#[test]
fn existing_instance_exits_successfully_without_starting_platform() {
    let started = std::cell::Cell::new(false);

    let result = run_if_primary(instance::AcquireResult::AlreadyRunning, |_guard| {
        started.set(true);
        Ok(())
    });

    assert!(result.is_ok());
    assert!(!started.get());
}
```

Run:

```bash
rtk cargo test -p rollshot-app daemon::tests::existing_instance_exits_successfully
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
rtk cargo fmt --all
rtk git add crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs crates/rollshot-app/src/daemon
rtk git commit -m "feat(app): add daemon launch mode"
```

### Task 9: Document and verify Linux KDE 6

**Files:**

- Modify: `README.md`
- Modify: `AGENTS.md`

- [ ] **Step 1: Document daemon usage**

Add a concise README section containing:

````markdown
### System tray daemon (KDE Plasma 6)

Start the daemon:

```bash
rollshot-app daemon
```

The tray provides **Capture Region** and **Quit Rollshot**. The default KDE
global shortcut request is `Alt+Shift+6`; KDE may ask you to approve or replace
it. Region capture starts in Screenshot mode, and the capture toolbar can
switch the selected crop to Scrolling mode.

Optional configuration:

```toml
[daemon]
capture_region_hotkey = "Alt+Shift+6"
```

Save it as `$XDG_CONFIG_HOME/rollshot/config.toml` (normally
`~/.config/rollshot/config.toml`) and restart the daemon. The first release
targets KDE Plasma 6 on Wayland. If portal shortcut registration fails, the
tray remains usable.
````

Do not document autostart, GNOME, X11, or macOS daemon support.

- [ ] **Step 2: Update repository orientation**

Add `crates/rollshot-linux-desktop` to the `AGENTS.md` project map as the small
Linux desktop integration helper currently shared by the daemon and Action
Guide SNI paths.

- [ ] **Step 3: Run full automated verification**

Run:

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: every command exits 0 with no test failures, formatting differences,
or clippy warnings.

- [ ] **Step 4: Run KDE Plasma 6 manual verification**

On a KDE Plasma 6 Wayland desktop:

```bash
rtk cargo run -p rollshot-app -- daemon
```

Verify:

1. One Rollshot SNI tray item appears.
2. KDE presents or recognizes the `Alt+Shift+6` portal shortcut.
3. The shortcut and `Capture Region` menu item both open region Screenshot
   capture.
4. After selecting a crop, the toolbar can switch to Scrolling and finish.
5. Repeated shortcut presses during capture do not open another capture.
6. A second `rollshot-app daemon` exits successfully without a second tray.
7. Denying/removing the portal binding leaves the tray functional.
8. `Quit Rollshot` exits while idle.
9. `Quit Rollshot` during capture closes the capture child and daemon.

If the execution environment lacks KDE Plasma 6, record this as the only
remaining runtime-verification gap; do not claim KDE runtime verification.

- [ ] **Step 5: Review the final diff against the approved spec**

Run:

```bash
rtk git diff 15b1ad1 --stat
rtk git diff 15b1ad1 -- crates/rollshot-app crates/rollshot-linux-desktop crates/rollshot-iced-overlay README.md AGENTS.md
```

Confirm every changed line maps to the approved daemon design and no macOS
daemon implementation or unrelated refactor entered the diff.

- [ ] **Step 6: Commit documentation**

```bash
rtk git add README.md AGENTS.md
rtk git commit -m "docs(daemon): document KDE tray mode"
```

## Completion criteria

- `rollshot-app daemon` is accepted while no-argument behavior remains the
  existing one-shot capture.
- Only one daemon can own the lock and tray.
- KDE SNI tray offers only `Capture Region` and `Quit Rollshot`.
- XDG portal shortcut defaults to `ALT+SHIFT+6` and failure degrades to
  tray-only.
- One capture child runs at a time using the current executable.
- Quit terminates the capture process group before daemon resources drop.
- Config fallback behavior is covered by tests.
- State transitions and platform fallback are covered without requiring a GUI
  in CI.
- Full test, format, and clippy commands pass.
- KDE Plasma 6 manual results are recorded honestly.
- macOS remains explicitly deferred.
