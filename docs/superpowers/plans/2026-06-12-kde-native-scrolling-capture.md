# KDE Native Scrolling Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a KDE Wayland native scrolling capture path that targets the active output without a portal picker and automatically falls back to the existing portal path only when the requested backend is `auto`.

**Architecture:** Add a strict `linux-kwin` streaming backend backed by KWin's `zkde_screencast_unstable_v1` protocol and the existing Linux PipeWire consumer. Preserve `auto` intent with a Linux auto backend that owns native-to-portal startup fallback, while the Linux iced overlay acquires the active-output KWin one-shot first so it can display a frozen selection background and target the same output. Explicit `linux-kwin` and `linux-portal` requests never fall back.

**Tech Stack:** Rust 1.85, `wayland-client`/`wayland-scanner`, KWin `ScreenShot2`, `zkde_screencast_unstable_v1`, PipeWire, iced layer-shell, tracing, Cargo tests.

**Approved spec:** `docs/superpowers/specs/2026-06-12-kde-native-scrolling-capture-design.md`

---

## File Structure

### New files

- `crates/rollshot-capture/protocols/zkde-screencast-unstable-v1.xml`
  - Vendored LGPL-2.1-or-later KDE protocol definition used to generate Rust client bindings.
- `crates/rollshot-capture/src/linux/kwin_screencast.rs`
  - Owns the KWin Wayland connection, output-name matching, protocol events, timeout, and stream-session lifetime.
- `crates/rollshot-capture/src/linux/auto.rs`
  - Owns KDE detection, fallback eligibility, strict native-first startup, combined failure reporting, and fallback diagnostics.
- `crates/rollshot-capture/tests/linux_kwin_smoke.rs`
  - Ignored live KDE test for explicit `linux-kwin` capture.

### Modified files

- `Cargo.toml`
  - Adds pinned workspace Wayland client/scanner dependencies.
- `crates/rollshot-capture/Cargo.toml`
  - Enables Wayland protocol generation on Linux.
- `crates/rollshot-capture/src/types.rs`
  - Adds `target_output_name` to capture options.
- `crates/rollshot-capture/src/diagnostics.rs`
  - Adds stable `rollshot::capture::linux::kwin` target.
- `crates/rollshot-capture/src/linux/pipewire.rs`
  - Shares frame handling between portal-FD and native PipeWire connections.
- `crates/rollshot-capture/src/linux/mod.rs`
  - Exposes strict KWin and auto Linux backends.
- `crates/rollshot-capture/src/backend.rs`
  - Preserves `auto` intent on Linux, adds `linux-kwin`, and creates the new backends.
- `crates/rollshot-capture/src/lib.rs`
  - Exports Linux KWin/auto types required by CLI, probe, and overlay.
- `crates/rollshot-cli/src/args.rs`
  - Accepts explicit `linux-kwin`.
- `crates/rollshot-cli/src/cmd_capture.rs`
  - Treats KWin/auto regions as full-source capture.
- `crates/rollshot-cli/src/cmd_probe.rs`
  - Reports KDE-native availability separately from portal availability.
- `crates/rollshot-cli/tests/probe_cli.rs`
  - Verifies Linux probe shape includes the KWin backend.
- `crates/rollshot-iced-overlay/src/driver.rs`
  - Passes a target output name into streaming capture.
- `crates/rollshot-iced-overlay/src/linux_runner.rs`
  - Acquires the KWin frozen active-output image before native/auto scrolling startup, carries it with the driver, and targets the overlay output.
- `crates/rollshot-iced-overlay/src/macos_capture.rs`
  - Call-site-only update for the new `Driver::start_capture` parameter (passes `None`; macOS behavior unchanged).
- `packaging/linux/dev.rollshot.io.desktop`
  - Declares `zkde_screencast_unstable_v1`.
- `README.md`
  - Replaces screenshot-only KDE permission instructions with native capture install, verification, and fallback instructions.

---

### Task 1: Preserve Linux Backend Intent and Target Output

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/backend.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-cli/src/args.rs`
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`

- [ ] **Step 1: Write failing backend and option tests**

In `crates/rollshot-capture/src/backend.rs`, extend the existing tests:

```rust
#[test]
fn from_cli_flag_preserves_linux_auto_intent() {
    assert_eq!(
        backend_for_flag("auto", "linux", Some("wayland")).unwrap(),
        BackendKind::LinuxAuto
    );
}

#[test]
fn from_cli_flag_accepts_linux_kwin() {
    assert_eq!(
        BackendKind::from_cli_flag("linux-kwin").unwrap(),
        BackendKind::LinuxKwinPipeWire
    );
}

#[test]
fn explicit_linux_backends_round_trip() {
    assert_eq!(
        backend_for_flag(BackendKind::LinuxAuto.as_flag(), "linux", Some("wayland")).unwrap(),
        BackendKind::LinuxAuto
    );
    assert_eq!(
        BackendKind::from_cli_flag(BackendKind::LinuxKwinPipeWire.as_flag()).unwrap(),
        BackendKind::LinuxKwinPipeWire
    );
}
```

In `crates/rollshot-capture/src/types.rs`, add:

```rust
#[test]
fn capture_options_default_has_no_target_output_name() {
    assert_eq!(CaptureOptions::default().target_output_name, None);
}
```

- [ ] **Step 2: Run the focused tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-capture backend::tests
rtk cargo test -p rollshot-capture types::tests
```

(`cargo test` accepts only one positional test filter per invocation.)

Expected: FAIL because `LinuxAuto`, `LinuxKwinPipeWire`, `backend_for_flag`, and `target_output_name` do not exist.

- [ ] **Step 3: Add the minimal backend intent and target-output types**

Add to `CaptureOptions`:

```rust
/// Wayland output name selected by a platform host. Linux KWin uses this to
/// bind the same output as the selection overlay. Other backends ignore it.
pub target_output_name: Option<String>,
```

Initialize it to `None` in `Default` and every existing `CaptureOptions` literal.

Extend `BackendKind`:

```rust
pub enum BackendKind {
    Fixture,
    LinuxAuto,
    LinuxKwinPipeWire,
    LinuxPortalPipeWire,
    MacosScreenCaptureKit,
    Unsupported,
}
```

Add a pure helper that preserves `auto` intent:

```rust
pub fn backend_for_flag(
    flag: &str,
    os: &str,
    session_type: Option<&str>,
) -> Result<BackendKind, CaptureError> {
    match flag {
        "auto" if os == "linux" && session_type == Some("wayland") => Ok(BackendKind::LinuxAuto),
        "auto" => Ok(default_backend_for(os, session_type)),
        "linux-kwin" => Ok(BackendKind::LinuxKwinPipeWire),
        "linux-portal" => Ok(BackendKind::LinuxPortalPipeWire),
        "fixture" => Ok(BackendKind::Fixture),
        "macos-sck" => Ok(BackendKind::MacosScreenCaptureKit),
        other => Err(CaptureError::InvalidConfig {
            message: format!(
                "unknown backend '{other}'; expected one of: auto, fixture, linux-kwin, linux-portal, macos-sck"
            ),
        }),
    }
}
```

Make `BackendKind::from_cli_flag` delegate to `backend_for_flag` with the real
environment. Change `default_backend_for("linux", Some("wayland"))` to
`LinuxAuto`, so probe output accurately reports that the product default owns
native-first fallback. `LinuxAuto::as_flag()` returns `"auto"`;
`LinuxKwinPipeWire::as_flag()` returns `"linux-kwin"`.

Keep this commit shippable on its own:

- `BackendKind::create` matches exhaustively, so the new variants need arms
  now: `LinuxAuto` creates the existing Linux portal backend (current behavior,
  replaced by the real auto backend in Task 5), and `LinuxKwinPipeWire`
  returns `CaptureError::Unsupported` with a "not yet wired" message until
  Task 5. Without these arms Task 1 does not compile.
- The probe report's `default_backend` string for Linux Wayland changes from
  `"linux-portal"` to `"auto"`; update any existing probe assertions that
  depend on the old value.

Update CLI accepted backend values and make `parse_region("auto", ...)` resolve `PortalPicker` only for explicit `LinuxPortalPipeWire`; `LinuxAuto` and `LinuxKwinPipeWire` use `FullSource`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-capture backend::tests
rtk cargo test -p rollshot-capture types::tests
rtk cargo test -p rollshot-cli --test capture_fixture
rtk cargo test -p rollshot-cli --test probe_cli
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/types.rs crates/rollshot-capture/src/backend.rs crates/rollshot-capture/src/lib.rs crates/rollshot-cli/src/args.rs crates/rollshot-cli/src/cmd_capture.rs
rtk git commit -m "feat(capture): preserve Linux auto backend intent"
```

---

### Task 2: Add Generated KWin Screencast Protocol Bindings

**Files:**
- Create: `crates/rollshot-capture/protocols/zkde-screencast-unstable-v1.xml`
- Create: `crates/rollshot-capture/src/linux/kwin_screencast.rs`
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-capture/Cargo.toml`
- Modify: `crates/rollshot-capture/src/linux/mod.rs`
- Modify: `crates/rollshot-capture/src/diagnostics.rs`

- [ ] **Step 1: Add the vendored protocol and dependency test**

Copy the current KDE protocol XML from:

```text
https://invent.kde.org/libraries/plasma-wayland-protocols/-/raw/master/src/protocols/zkde-screencast-unstable-v1.xml
```

Preserve its copyright and `SPDX-License-Identifier: LGPL-2.1-or-later`.

Add this test in the new `kwin_screencast.rs`:

```rust
#[test]
fn protocol_version_supports_output_streaming() {
    assert!(MAX_SUPPORTED_VERSION >= 1);
}

#[test]
fn created_event_produces_node_id() {
    let state = StreamOutcome::default();
    assert_eq!(state.apply(StreamEvent::Created(42)), StreamOutcome::Created(42));
}

#[test]
fn failed_event_preserves_message() {
    let state = StreamOutcome::default();
    assert_eq!(
        state.apply(StreamEvent::Failed("denied".to_string())),
        StreamOutcome::Failed("denied".to_string())
    );
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-capture kwin_screencast
```

Expected: FAIL because the module and protocol types do not exist.

- [ ] **Step 3: Add pinned Wayland dependencies and generated bindings**

In workspace dependencies:

```toml
wayland-client = "0.31.14"
wayland-scanner = "0.31.10"
```

Add both under Linux target dependencies for `rollshot-capture`.

In `kwin_screencast.rs`, generate the protocol:

```rust
pub mod protocol {
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/zkde-screencast-unstable-v1.xml");
    }

    use self::__interfaces::*;
    use wayland_client::protocol::*;
    wayland_scanner::generate_client_code!("protocols/zkde-screencast-unstable-v1.xml");
}
```

Add stable diagnostics target:

```rust
pub(crate) const TARGET_LINUX_KWIN: &str = "rollshot::capture::linux::kwin";
```

Define the testable event reducer:

```rust
const MAX_SUPPORTED_VERSION: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamEvent {
    Created(u32),
    Failed(String),
    Closed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum StreamOutcome {
    #[default]
    Pending,
    Created(u32),
    Failed(String),
    Closed,
}
```

Expose `pub mod kwin_screencast;` from `linux/mod.rs`.

- [ ] **Step 4: Run focused tests and compile generated code**

Run:

```bash
rtk cargo test -p rollshot-capture kwin_screencast
rtk cargo check -p rollshot-capture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-capture/Cargo.toml crates/rollshot-capture/protocols/zkde-screencast-unstable-v1.xml crates/rollshot-capture/src/linux/kwin_screencast.rs crates/rollshot-capture/src/linux/mod.rs crates/rollshot-capture/src/diagnostics.rs
rtk git commit -m "feat(capture): add KWin screencast protocol bindings"
```

---

### Task 3: Implement the KWin Output Stream Session

**Files:**
- Modify: `crates/rollshot-capture/src/linux/kwin_screencast.rs`

- [ ] **Step 1: Write failing mapping and lifecycle tests**

Add pure tests around output selection and error mapping:

```rust
#[test]
fn matching_output_name_selects_exact_output() {
    let outputs = vec![
        OutputInfo { registry_name: 7, name: Some("DP-1".into()) },
        OutputInfo { registry_name: 9, name: Some("eDP-1".into()) },
    ];
    assert_eq!(select_output(&outputs, "eDP-1").unwrap().registry_name, 9);
}

#[test]
fn missing_output_name_is_mapping_error() {
    let err = select_output(&[], "eDP-1").unwrap_err();
    assert!(matches!(err, CaptureError::Mapping { .. }));
}

#[test]
fn failed_event_maps_to_permission_denied_when_authorization_is_rejected() {
    let err = map_stream_failure("Client is not authorized");
    assert!(matches!(err, CaptureError::PermissionDenied { .. }));
}

#[test]
fn timeout_is_fallback_eligible_capture_timeout() {
    let err = stream_timeout("stream_output");
    assert!(matches!(err, CaptureError::Timeout { .. }));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-capture kwin_screencast
```

Expected: FAIL for missing selection and mapping functions.

- [ ] **Step 3: Implement the bounded Wayland session**

Implement:

```rust
pub struct KwinScreencastSession {
    node_id: u32,
    connection: wayland_client::Connection,
    stream: protocol::zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1,
}

impl KwinScreencastSession {
    pub fn node_id(&self) -> u32 {
        self.node_id
    }
}

pub trait KwinScreencastClient: Send {
    fn start_output(
        &self,
        output_name: &str,
        show_cursor: bool,
    ) -> Result<KwinScreencastSession, CaptureError>;
}
```

The real client must:

1. connect with `wayland_client::Connection::connect_to_env()`;
2. use `registry_queue_init`;
3. bind `zkde_screencast_unstable_v1` at `min(advertised, MAX_SUPPORTED_VERSION)`;
4. collect `wl_output` names and select an exact `output_name` match;
5. call `stream_output` with hidden or embedded cursor mode;
6. dispatch with a bounded 5-second deadline until `created`, `failed`, or `closed`;
7. keep the Wayland connection and stream proxy alive in `KwinScreencastSession`.

Use structured events at `TARGET_LINUX_KWIN`; do not log output contents or frame pixels.

- [ ] **Step 4: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-capture kwin_screencast
rtk cargo clippy -p rollshot-capture --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/linux/kwin_screencast.rs
rtk git commit -m "feat(capture): create KWin output screencast sessions"
```

---

### Task 4: Share PipeWire Processing Between Portal and Native Connections

**Files:**
- Modify: `crates/rollshot-capture/src/linux/pipewire.rs`

- [ ] **Step 1: Write failing connection-mode and metadata tests**

Add:

```rust
#[test]
fn native_stream_records_linux_kwin_backend_name() {
    let options = CaptureOptions::default();
    let metadata = frame_metadata_for_backend("linux-kwin", &options, make_meta(10, 20));
    assert_eq!(metadata.backend, "linux-kwin");
}

#[test]
fn portal_stream_records_linux_portal_backend_name() {
    let options = CaptureOptions::default();
    let metadata = frame_metadata_for_backend("linux-portal", &options, make_meta(10, 20));
    assert_eq!(metadata.backend, "linux-portal");
}

#[test]
fn connection_mode_distinguishes_default_and_portal_remote() {
    assert!(matches!(PipeWireRemote::Default, PipeWireRemote::Default));
    assert!(matches!(fake_portal_remote(), PipeWireRemote::PortalFd(_)));
}

#[test]
fn source_session_outlives_pipewire_consumer() {
    let drops = DropOrder::default();
    {
        let _stream = fake_stream_with(drops.tracked_pipewire(), drops.tracked_session());
    }
    assert_eq!(drops.order(), vec!["pipewire", "session"]);
}
```

The drop-order test covers the spec's teardown risk: the KWin stream session
must stay alive until the PipeWire consumer has stopped.

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-capture linux::pipewire::tests
```

Expected: FAIL because the shared remote and backend-label helpers do not exist.

- [ ] **Step 3: Refactor connection setup without changing frame processing**

Introduce:

```rust
enum PipeWireRemote {
    Default,
    PortalFd(std::os::fd::OwnedFd),
}

pub struct LinuxPipeWireFrameStream<R> {
    pipewire: connection::PipeWireConnection,
    _source_session: R,
    queue: Arc<FrameQueue>,
}
```

Field order is load-bearing: Rust drops fields in declaration order, so
`pipewire` must be declared before `_source_session` to stop the consumer
before releasing the KWin (or portal) session. State this in a comment on the
struct and keep the Step 1 drop-order test green.

Change `PipeWireConnection::connect_fd` into:

```rust
fn connect(
    remote: PipeWireRemote,
    node_id: u32,
    backend_name: &'static str,
    options: CaptureOptions,
    queue: Arc<FrameQueue>,
) -> Result<Self, CaptureError>
```

Use:

```rust
let core = match remote {
    PipeWireRemote::Default => context.connect_rc(None),
    PipeWireRemote::PortalFd(fd) => context.connect_fd_rc(dup_pipewire_fd(fd.as_fd())?, None),
};
```

Store `backend_name` in `StreamUserData` and use it when constructing
`FrameMetadata`. Keep `LinuxPortalFrameStream` as a type alias or thin wrapper
so existing portal tests remain readable. Add a native constructor accepting a
`KwinScreencastSession`.

- [ ] **Step 4: Run portal and PipeWire regression tests**

Run:

```bash
rtk cargo test -p rollshot-capture linux::pipewire
rtk cargo test -p rollshot-capture linux::tests
```

Expected: PASS with existing portal tests unchanged in behavior.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/linux/pipewire.rs
rtk git commit -m "refactor(capture): share Linux PipeWire stream processing"
```

---

### Task 5: Add Strict KWin and Native-First Auto Backends

**Files:**
- Create: `crates/rollshot-capture/src/linux/auto.rs`
- Modify: `crates/rollshot-capture/src/linux/mod.rs`
- Modify: `crates/rollshot-capture/src/backend.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Write failing strict-backend and fallback tests**

Use injected backend factories in `linux/auto.rs`:

```rust
#[test]
fn native_success_skips_portal() {
    let calls = Calls::default();
    let mut backend = test_auto_backend(native_ok(&calls), portal_ok(&calls));
    backend.start(targeted_options("eDP-1")).unwrap();
    assert_eq!(calls.native(), 1);
    assert_eq!(calls.portal(), 0);
}

#[test]
fn eligible_native_failure_starts_portal_once() {
    let calls = Calls::default();
    let mut backend = test_auto_backend(
        native_err(&calls, CaptureError::PermissionDenied { message: "denied".into() }),
        portal_ok(&calls),
    );
    backend.start(targeted_options("eDP-1")).unwrap();
    assert_eq!(calls.portal(), 1);
}

#[test]
fn user_cancelled_never_falls_back() {
    let calls = Calls::default();
    let mut backend = test_auto_backend(native_err(&calls, CaptureError::UserCancelled), portal_ok(&calls));
    assert!(matches!(backend.start(targeted_options("eDP-1")), Err(CaptureError::UserCancelled)));
    assert_eq!(calls.portal(), 0);
}

#[test]
fn explicit_kwin_backend_never_constructs_portal() {
    let mut backend = LinuxKwinBackend::with_client(failing_kwin_client());
    assert!(backend.start(targeted_options("eDP-1")).is_err());
}

#[test]
fn both_failures_preserve_native_and_portal_context() {
    let err = combine_fallback_errors(native_failure(), portal_failure());
    let text = err.to_string();
    assert!(text.contains("native"));
    assert!(text.contains("portal"));
}

#[test]
fn native_runtime_stream_error_does_not_construct_portal() {
    let calls = Calls::default();
    let mut backend = test_auto_backend(native_stream_that_fails_on_next_frame(&calls), portal_ok(&calls));
    let mut stream = backend.start(targeted_options("eDP-1")).unwrap();
    assert!(stream.next_frame().is_err());
    assert_eq!(calls.portal(), 0);
}

#[test]
fn explicit_kwin_backend_resolves_active_output_when_target_is_missing() {
    let mut backend = test_kwin_backend(recording_kwin_client(), active_output_resolver("eDP-1"));
    backend.start(CaptureOptions::default()).unwrap();
    assert_eq!(started_output_name(&backend), Some("eDP-1"));
}

#[test]
fn user_cancelled_and_invalid_config_are_not_fallback_eligible() {
    assert!(!is_fallback_eligible(&CaptureError::UserCancelled));
    assert!(!is_fallback_eligible(&CaptureError::InvalidConfig {
        message: "bad".into(),
    }));
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-capture linux::auto
```

Expected: FAIL because the auto and strict KWin backends do not exist.

- [ ] **Step 3: Implement `LinuxKwinBackend`**

In `linux/mod.rs`, add a strict backend:

```rust
pub struct LinuxKwinBackend<C = RealKwinScreencastClient> {
    client: C,
}
```

Its `start` method must:

- require Wayland and KDE;
- use `options.target_output_name` when the overlay host already resolved it;
- otherwise call the existing strict KWin active-screen one-shot path to
  resolve the active output name without invoking the portal (inject the
  active-output resolver for tests alongside the screencast client);
- call `client.start_output(output_name, options.show_cursor)`;
- connect a native Linux PipeWire frame stream;
- return errors unchanged;
- never instantiate a portal backend.

- [ ] **Step 4: Implement `LinuxAutoBackend` and fallback classifier**

In `linux/auto.rs`, define:

```rust
pub fn is_fallback_eligible(error: &CaptureError) -> bool {
    matches!(
        error,
        CaptureError::Unsupported { .. }
            | CaptureError::PermissionDenied { .. }
            | CaptureError::Timeout { .. }
            | CaptureError::Mapping { .. }
            | CaptureError::Backend(_)
    )
}
```

`LinuxAutoBackend::start` must:

- on KDE Wayland, try strict KWin first; the strict backend resolves a missing
  active output or reuses a caller-provided `target_output_name`;
- on non-KDE Wayland, use portal directly;
- on eligible native startup failure, emit:

```rust
tracing::warn!(
    target: TARGET_LINUX_KWIN,
    reason = fallback_reason(&native_error),
    fallback = "linux-portal",
    error = %native_error,
    "KWin native capture unavailable; falling back to portal"
);
```

- never fallback on `UserCancelled` or `InvalidConfig`;
- combine native and portal failures if fallback also fails;
- pass the caller's `CaptureOptions` to the portal leg unchanged. This is a
  deliberate semantics change for the CLI: `auto` now resolves region
  `FullSource` (Task 1), so a portal fallback captures the full source instead
  of honoring a portal-picked region. Task 8 documents it. The overlay
  scrolling path already uses `FullSource` and is unaffected.

Add this startup flow as a doc comment on `LinuxAutoBackend`:

```text
auto (Linux Wayland)
        │
   KDE detected? ──no──► start linux-portal
        │ yes
   strict linux-kwin startup
        │
   ok? ──yes──► native stream
        │ no
   fallback-eligible? ──no──► return native error
        │ yes
   warn (target rollshot::capture::linux::kwin)
        │
   start linux-portal ──err──► combined native+portal error
```

Wire `BackendKind::LinuxAuto` and `BackendKind::LinuxKwinPipeWire` into
`BackendKind::create`, replacing the Task 1 stub arms.

- [ ] **Step 5: Run focused and capture-crate tests**

Run:

```bash
rtk cargo test -p rollshot-capture linux::auto
rtk cargo test -p rollshot-capture backend::tests
rtk cargo test -p rollshot-capture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-capture/src/linux/auto.rs crates/rollshot-capture/src/linux/mod.rs crates/rollshot-capture/src/backend.rs crates/rollshot-capture/src/lib.rs
rtk git commit -m "feat(capture): add KDE native-first Linux backend"
```

---

### Task 6: Target the KDE Scrolling Overlay with a Frozen Active-Output Image

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs` (call-site only)

- [ ] **Step 1: Write failing scrolling-resource tests**

Extend `linux_runner.rs` tests:

```rust
#[test]
fn kwin_scrolling_resource_targets_frozen_output() {
    let resource = CaptureResource::Streaming {
        driver: fake_driver(Some("DP-2")),
        frozen: Some(fake_one_shot_capture_for("DP-2")),
    };
    assert_eq!(start_mode_for(&resource), StartMode::TargetScreen("DP-2".into()));
}

#[test]
fn portal_scrolling_resource_uses_active_start_mode_without_frozen_image() {
    let resource = CaptureResource::Streaming {
        driver: fake_driver(None),
        frozen: None,
    };
    assert_eq!(start_mode_for(&resource), StartMode::Active);
}

#[test]
fn auto_kwin_one_shot_failure_uses_portal_stream_without_frozen_image() {
    let result = acquire_scrolling_resource(
        &auto_config(),
        &factories_with_failed_kwin_one_shot_and_portal_driver(),
    ).unwrap();
    assert!(matches!(result, CaptureResource::Streaming { frozen: None, .. }));
}

#[test]
fn explicit_kwin_one_shot_failure_returns_error_without_portal() {
    let result = acquire_scrolling_resource(
        &kwin_config(),
        &factories_with_failed_kwin_one_shot_and_portal_driver(),
    );
    assert!(result.is_err());
}

#[test]
fn frozen_handle_exists_only_for_kwin_streaming_resources() {
    let kwin = CaptureResource::Streaming {
        driver: fake_driver(Some("DP-2")),
        frozen: Some(fake_one_shot_capture_for("DP-2")),
    };
    let portal = CaptureResource::Streaming { driver: fake_driver(None), frozen: None };
    assert!(frozen_handle_for(&kwin).is_some());
    assert!(frozen_handle_for(&portal).is_none());
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner::tests
```

Expected: FAIL because streaming resources do not carry frozen captures or target output names.

- [ ] **Step 3: Pass target output names through `Driver`**

Change `Driver::start_capture` to accept both platform target forms:

```rust
pub fn start_capture(
    backend: &str,
    fps: u32,
    show_cursor: bool,
    target_display_id: Option<u32>,
    target_output_name: Option<String>,
    preview_tx: UnboundedSender<LiveOverlayEvent>,
) -> Result<Self, String>
```

and place both values in `CaptureOptions`. Add this field to `Driver`:

```rust
target_output_name: Option<String>,
```

Expose:

```rust
pub fn target_output_name(&self) -> Option<&str> {
    self.target_output_name.as_deref()
}
```

Keep the existing `target_display_id` field and macOS behavior unchanged.
`target_output_name` is an additional Linux-oriented field; do not change the
macOS runner's capture semantics. The signature change does require a
mechanical edit to the `Driver::start_capture` call site in
`macos_capture.rs`: pass `None` for `target_output_name`. That is the only
macOS change in this plan.

- [ ] **Step 4: Carry frozen background with scrolling resources**

Change:

```rust
CaptureResource::Streaming(Driver)
```

to:

```rust
CaptureResource::Streaming {
    driver: Driver,
    frozen: Option<rollshot_capture::OneShotCapture>,
}
```

Extract pure helpers:

```rust
fn start_mode_for(resource: &CaptureResource) -> StartMode
fn frozen_handle_for(resource: &CaptureResource) -> Option<iced::widget::image::Handle>
```

For KDE `auto` or explicit `linux-kwin` scrolling startup:

1. call `OneShotBackendKind::LinuxKwin.capture_once(show_cursor)`;
2. pass its `output_name` into `Driver::start_capture`;
3. retain the one-shot as the frozen background;
4. for `auto`, if the one-shot fails with a fallback-eligible error, start
   explicit `linux-portal` and return `frozen: None`;
5. for explicit `linux-kwin`, return the one-shot/native error unchanged.

The streaming driver still starts and receives its first frame before
`application(...)` opens the overlay. Selection frames are ignored until
`BeginStitch`, matching the existing Driver lifecycle.

- [ ] **Step 5: Run overlay and macOS regression tests**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner::tests
rtk cargo test -p rollshot-iced-overlay driver::tests
rtk cargo check -p rollshot-iced-overlay
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/driver.rs crates/rollshot-iced-overlay/src/linux_runner.rs
rtk git commit -m "feat(overlay): target KDE scrolling capture output"
```

---

### Task 7: Add CLI Probe Coverage and a Live KWin Smoke Test

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_probe.rs`
- Modify: `crates/rollshot-cli/tests/probe_cli.rs`
- Create: `crates/rollshot-capture/tests/linux_kwin_smoke.rs`

- [ ] **Step 1: Write failing probe and smoke-test assertions**

In `probe_cli.rs`, add Linux-only assertions:

```rust
#[cfg(target_os = "linux")]
#[test]
fn probe_json_lists_kwin_and_portal_backends() {
    let report = run_probe_json();
    let names = backend_names(&report);
    assert!(names.contains(&"linux-kwin"));
    assert!(names.contains(&"linux-portal"));
}
```

Create the ignored smoke test:

```rust
#![cfg(target_os = "linux")]

#[test]
#[ignore = "requires installed Rollshot desktop entry and live KDE Wayland session"]
fn captures_linux_kwin_frames() {
    if std::env::var("ROLLSHOT_REAL_KWIN_CAPTURE").as_deref() != Ok("1") {
        eprintln!("set ROLLSHOT_REAL_KWIN_CAPTURE=1 to run real KWin capture");
        return;
    }

    let one_shot = rollshot_capture::OneShotBackendKind::LinuxKwin
        .capture_once(false)
        .expect("capture active KWin output");
    let output = one_shot.target_display().output_name.clone().expect("output name");

    let mut backend = rollshot_capture::LinuxKwinBackend::new();
    let mut options = rollshot_capture::CaptureOptions::default();
    options.target_output_name = Some(output);
    let mut stream = backend.start(options).expect("start KWin stream");
    let frame = stream.next_frame().expect("first KWin frame");
    assert_eq!(frame.metadata.backend, "linux-kwin");
}
```

- [ ] **Step 2: Run tests and verify probe failure**

Run:

```bash
rtk cargo test -p rollshot-cli --test probe_cli
rtk cargo test -p rollshot-capture --test linux_kwin_smoke
```

Expected: probe test FAIL until `linux-kwin` is reported; ignored smoke test compiles.

- [ ] **Step 3: Report KWin availability**

Add `LinuxKwinBackend::probe()` that reports:

- Wayland session status;
- KDE desktop detection;
- whether `zkde_screencast_unstable_v1` is advertised;
- a concise message explaining that installed desktop-entry authorization is
  still required for a real stream.

In `cmd_probe.rs`, append KWin probe before portal probe on Linux.

- [ ] **Step 4: Run CLI and capture tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test probe_cli
rtk cargo test -p rollshot-capture --test linux_kwin_smoke
rtk cargo test -p rollshot-cli
```

Expected: PASS; live KWin smoke remains ignored.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-cli/src/cmd_probe.rs crates/rollshot-cli/tests/probe_cli.rs crates/rollshot-capture/tests/linux_kwin_smoke.rs
rtk git commit -m "test(capture): cover KDE native backend diagnostics"
```

---

### Task 8: Document and Package KDE Native Authorization

**Files:**
- Modify: `packaging/linux/dev.rollshot.io.desktop`
- Modify: `README.md`

- [ ] **Step 1: Add failing packaging checks**

Run:

```bash
rtk rg -n '^X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1$' packaging/linux/dev.rollshot.io.desktop
rtk rg -n 'KDE Native Capture Permission' README.md
rtk rg -n 'linux-kwin' README.md
```

Run each check separately — an OR-pattern (`a|b|c`) passes if any one phrase
already exists in the file (e.g. README may already mention "fallback").

Expected: each command FAILs because the desktop declaration and new
documentation do not exist.

- [ ] **Step 2: Add the desktop entry authorization**

Add:

```ini
X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1
```

Keep the existing absolute `Exec=/usr/bin/rollshot-app` and
`X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`.

- [ ] **Step 3: Replace the README KDE permission section**

Rename `KDE Normal Screenshot Permission` to `KDE Native Capture Permission`
and explicitly document:

- both required desktop entry keys;
- system install and local install commands;
- `Exec` must canonicalize to the running binary;
- launching from the menu is not required;
- `cargo run`/`target/...` normally causes `auto` to fall back to portal;
- installed `auto` scrolling capture should not show a picker;
- explicit `linux-portal` always tests the picker path;
- explicit `linux-kwin` diagnoses native authorization without fallback;
- under `auto`, a portal fallback captures the full source and crops locally;
  it does not honor a portal-picked region.

Include these verification commands:

```bash
~/.local/bin/rollshot-app --capture '{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
~/.local/bin/rollshot-app --capture '{"backend":"linux-kwin","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
~/.local/bin/rollshot-app --capture '{"backend":"linux-portal","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
```

- [ ] **Step 4: Verify documentation checks**

Run:

```bash
rtk rg -n '^X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2$' packaging/linux/dev.rollshot.io.desktop
rtk rg -n '^X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1$' packaging/linux/dev.rollshot.io.desktop
rtk rg -n 'KDE Native Capture Permission' README.md
rtk rg -n 'linux-kwin' README.md
rtk rg -n 'linux-portal' README.md
rtk git diff --check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add packaging/linux/dev.rollshot.io.desktop README.md
rtk git commit -m "docs: explain KDE native capture installation"
```

---

### Task 9: Full Verification and KDE Runtime Matrix

**Files:**
- No planned source changes; fix only failures directly caused by this feature.

- [ ] **Step 1: Run capture and overlay test suites**

Run:

```bash
rtk cargo test -p rollshot-capture
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-cli
```

Expected: PASS.

- [ ] **Step 2: Run workspace formatting and linting**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Build and install the matching KDE test binary**

Run:

```bash
rtk cargo build --release -p rollshot-app
rtk install -Dm755 target/release/rollshot-app ~/.local/bin/rollshot-app
```

Write the transformed desktop entry to
`~/.local/share/applications/dev.rollshot.io.desktop`, then refresh:

```bash
rtk mkdir -p ~/.local/share/applications
rtk sh -c 'sed "s|^Exec=.*|Exec=$HOME/.local/bin/rollshot-app|" packaging/linux/dev.rollshot.io.desktop > "$HOME/.local/share/applications/dev.rollshot.io.desktop"'
rtk update-desktop-database ~/.local/share/applications
```

Expected: installed desktop entry `Exec` matches
`~/.local/bin/rollshot-app`.

- [ ] **Step 4: Run live explicit KWin smoke test**

Run inside the active KDE Wayland session:

```bash
rtk env ROLLSHOT_REAL_KWIN_CAPTURE=1 cargo test -p rollshot-capture --test linux_kwin_smoke -- --ignored --nocapture
```

Expected: PASS and first frame metadata reports `linux-kwin`.

- [ ] **Step 5: Verify native-first and explicit backend behavior**

Run the installed binary:

```bash
rtk ~/.local/bin/rollshot-app --capture '{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
rtk ~/.local/bin/rollshot-app --capture '{"backend":"linux-kwin","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
rtk ~/.local/bin/rollshot-app --capture '{"backend":"linux-portal","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
```

Expected:

- `auto`: opens on active output without portal picker;
- `linux-kwin`: opens on active output without portal picker;
- `linux-portal`: opens portal picker.

- [ ] **Step 6: Verify automatic fallback**

Temporarily run a binary whose path does not match the installed desktop entry:

```bash
rtk target/release/rollshot-app --capture '{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
```

Expected: native authorization/startup fails before overlay, a structured
`rollshot::capture::linux::kwin` fallback warning is emitted, and the portal
picker opens.

Then run:

```bash
rtk target/release/rollshot-app --capture '{"backend":"linux-kwin","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
```

Expected: reports native failure and does not open portal picker.

- [ ] **Step 7: Verify display and scale matrix**

Manually verify:

- single monitor at integer scale;
- multi-monitor with active output changed between runs;
- fractional-scale active output;
- cursor disabled and enabled;
- Finish, Cancel, capture-miss warning, live preview, and result workspace.

Expected: selection background, overlay output, stream output, and crop mapping
refer to the same active output in every native run.

- [ ] **Step 8: Confirm the implementation branch is clean**

Run:

```bash
rtk git status --short --branch
```

Expected: the feature branch has no uncommitted changes. If an earlier
verification step exposed a defect, return to the task that owns that behavior,
add a reproducing test there, implement the fix, rerun that task's exact
verification commands, and then rerun Task 9 from Step 1.
