# Capture Workflow × Scope Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the conflated `CaptureMode { Scrolling, Region, Fullscreen }` with two orthogonal types — `Workflow { Screenshot, Scrolling }` and `CaptureScope { Region, Fullscreen }`, bundled as `CaptureRequest` — without changing any runtime behavior.

**Architecture:** Parallel-change refactor. Task 1 adds the new types plus a temporary `From<CaptureMode>` / `to_legacy_mode()` bridge (additive, green). Tasks 2–3 migrate consumers crate-by-crate, converting at the not-yet-migrated boundary so every commit compiles and all tests pass. Task 4 deletes `CaptureMode` and the bridge. The three existing capture flows map 1:1 to the three supported `CaptureRequest` combinations; `Scrolling × Fullscreen` is expressible but rejected by `is_supported()` and never constructed.

**Tech Stack:** Rust (workspace), serde, iced (overlay), `cargo test` / `clippy` / `fmt`.

**Spec:** `docs/superpowers/specs/2026-06-15-capture-workflow-scope-refactor-design.md`

---

## File Structure

- Modify: `crates/rollshot-capture/src/types.rs` — add `Workflow`, `CaptureScope`, `CaptureRequest` + bridge; later migrate `InteractiveLaunchOptions`; rewrite serde tests; finally delete `CaptureMode`.
- Modify: `crates/rollshot-capture/src/lib.rs` — re-export new types; later drop `CaptureMode` re-export.
- Modify: `crates/rollshot-iced-overlay/src/lib.rs` — `OverlayConfig.initial_mode` → `request`.
- Modify: `crates/rollshot-iced-overlay/src/toolbar.rs` — `active_mode: CaptureMode` → `active_workflow: Workflow`.
- Modify: `crates/rollshot-iced-overlay/src/workspace.rs` — `active_mode: CaptureMode` → `Workflow`.
- Modify: `crates/rollshot-iced-overlay/src/app.rs` — `state.mode` → `state.workflow`; `ActivateMode` → `ActivateWorkflow`; drop `Fullscreen` unreachable arms.
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs` — scope-based routing; `acquire_resource` matches workflow.
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs` — mirror of linux_runner (macOS path).
- Modify: `crates/rollshot-iced-overlay/src/fullscreen.rs` — `initial_mode` → `request`.
- Modify: `crates/rollshot-iced-overlay/src/bin/capture_overlay.rs` — fixture config.
- Modify: `crates/rollshot-app/src/macos_product.rs` — `initial_capture_path` → `f(scope)`; OverlayConfig construction.
- Modify: `crates/rollshot-app/src/launch.rs` — any `CaptureMode` references.
- Modify: `crates/rollshot-cli/src/cmd_capture_launcher.rs` — build `CaptureRequest`.

**Mechanical-site convention:** Many call sites are rote substitutions. To keep them uniform, Task 1 adds the constructors `CaptureRequest::screenshot_region()`, `screenshot_fullscreen()`, `scrolling_region()`. The substitution rule everywhere is:

| Old | New |
|-----|-----|
| `CaptureMode::Region` | `CaptureRequest::screenshot_region()` (value) / `Workflow::Screenshot` (in-overlay match arm) |
| `CaptureMode::Fullscreen` | `CaptureRequest::screenshot_fullscreen()` / scope check |
| `CaptureMode::Scrolling` | `CaptureRequest::scrolling_region()` / `Workflow::Scrolling` |

Use `cargo build` after each task as the authoritative worklist of remaining sites.

---

## Task 1: New types + bridge (additive, green)

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Test: `crates/rollshot-capture/src/types.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `crates/rollshot-capture/src/types.rs`:

```rust
use super::{CaptureRequest, CaptureScope, Workflow};

#[test]
fn capture_request_default_is_scrolling_region() {
    let r = CaptureRequest::default();
    assert_eq!(r.workflow, Workflow::Scrolling);
    assert_eq!(r.scope, CaptureScope::Region);
}

#[test]
fn needs_overlay_matches_region_scope() {
    assert!(CaptureRequest::screenshot_region().needs_overlay());
    assert!(CaptureRequest::scrolling_region().needs_overlay());
    assert!(!CaptureRequest::screenshot_fullscreen().needs_overlay());
}

#[test]
fn is_supported_rejects_scrolling_fullscreen() {
    let bad = CaptureRequest { workflow: Workflow::Scrolling, scope: CaptureScope::Fullscreen };
    assert!(!bad.is_supported());
    for r in [
        CaptureRequest::screenshot_region(),
        CaptureRequest::screenshot_fullscreen(),
        CaptureRequest::scrolling_region(),
    ] {
        assert!(r.is_supported());
    }
}

#[test]
fn legacy_capture_mode_maps_to_request() {
    use super::CaptureMode;
    assert_eq!(CaptureRequest::from(CaptureMode::Region), CaptureRequest::screenshot_region());
    assert_eq!(CaptureRequest::from(CaptureMode::Fullscreen), CaptureRequest::screenshot_fullscreen());
    assert_eq!(CaptureRequest::from(CaptureMode::Scrolling), CaptureRequest::scrolling_region());
}

#[test]
fn request_round_trips_through_legacy_mode() {
    use super::CaptureMode;
    for m in [CaptureMode::Region, CaptureMode::Fullscreen, CaptureMode::Scrolling] {
        assert_eq!(CaptureRequest::from(m).to_legacy_mode(), m);
    }
}

#[test]
fn capture_request_serde_round_trip() {
    let r = CaptureRequest::scrolling_region();
    let json = serde_json::to_string(&r).unwrap();
    assert_eq!(json, r#"{"workflow":"scrolling","scope":"region"}"#);
    assert_eq!(serde_json::from_str::<CaptureRequest>(&json).unwrap(), r);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-capture types::`
Expected: FAIL — `cannot find type CaptureRequest` / `Workflow` / `CaptureScope`.

- [ ] **Step 3: Add the new types + bridge**

Add to `crates/rollshot-capture/src/types.rs` (next to `CaptureMode`):

```rust
/// WHAT we do with the captured frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Workflow {
    Screenshot,
    #[default]
    Scrolling,
    // ActionGuide is added later by the Action Guide plan.
}

/// WHAT AREA we capture. Resolves down to the backend `RegionMode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureScope {
    #[default]
    Region,
    Fullscreen,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CaptureRequest {
    pub workflow: Workflow,
    pub scope: CaptureScope,
}

impl CaptureRequest {
    pub const fn screenshot_region() -> Self {
        Self { workflow: Workflow::Screenshot, scope: CaptureScope::Region }
    }
    pub const fn screenshot_fullscreen() -> Self {
        Self { workflow: Workflow::Screenshot, scope: CaptureScope::Fullscreen }
    }
    pub const fn scrolling_region() -> Self {
        Self { workflow: Workflow::Scrolling, scope: CaptureScope::Region }
    }

    /// Region scope uses the selection overlay; Fullscreen captures directly.
    pub fn needs_overlay(&self) -> bool {
        matches!(self.scope, CaptureScope::Region)
    }

    /// `Scrolling × Fullscreen` is expressible but not wired in this refactor.
    pub fn is_supported(&self) -> bool {
        !matches!(
            (self.workflow, self.scope),
            (Workflow::Scrolling, CaptureScope::Fullscreen)
        )
    }
}

// --- Temporary migration bridge. Removed in Task 4. ---
impl From<CaptureMode> for CaptureRequest {
    fn from(mode: CaptureMode) -> Self {
        match mode {
            CaptureMode::Region => Self::screenshot_region(),
            CaptureMode::Fullscreen => Self::screenshot_fullscreen(),
            CaptureMode::Scrolling => Self::scrolling_region(),
        }
    }
}

impl CaptureRequest {
    /// Bridge for incremental migration; removed once `CaptureMode` is gone.
    pub fn to_legacy_mode(self) -> CaptureMode {
        match (self.workflow, self.scope) {
            (Workflow::Screenshot, CaptureScope::Region) => CaptureMode::Region,
            (Workflow::Screenshot, CaptureScope::Fullscreen) => CaptureMode::Fullscreen,
            (Workflow::Scrolling, _) => CaptureMode::Scrolling,
        }
    }
}
```

- [ ] **Step 4: Re-export from the crate root**

In `crates/rollshot-capture/src/lib.rs`, add the new names to the existing `pub use types::{...}` line (which already re-exports `CaptureMode`):

```rust
pub use types::{CaptureMode, CaptureRequest, CaptureScope, Workflow};
```

(Keep all other names already re-exported on that line.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-capture types::`
Expected: PASS (all six new tests).

- [ ] **Step 6: Verify the whole crate still builds**

Run: `rtk cargo build -p rollshot-capture`
Expected: success (the additions are unused by callers — that is fine).

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-capture/src/types.rs crates/rollshot-capture/src/lib.rs
rtk git commit -m "feat(capture): add Workflow x CaptureScope types with CaptureMode bridge

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Migrate the overlay crate + app overlay sites (green via bridge)

Migrate `rollshot-iced-overlay` and `rollshot-app`'s overlay wiring to `CaptureRequest`/`Workflow`/`CaptureScope`. The launch payload (`InteractiveLaunchOptions.initial_mode: CaptureMode`) is unchanged in this task; `rollshot-app` converts it with `.into()` at the `OverlayConfig` boundary, so the whole workspace stays green.

**This task touches both platform paths (AGENTS.md §8):** Linux (`linux_runner.rs`) and macOS (`macos_capture.rs`, `macos_product.rs`). Apply the same transformation to both and verify both compile.

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/workspace.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/toolbar.rs`
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs`
- Modify: `crates/rollshot-iced-overlay/src/fullscreen.rs`
- Modify: `crates/rollshot-iced-overlay/src/bin/capture_overlay.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/launch.rs`

- [ ] **Step 1: Change `OverlayConfig` to carry a `CaptureRequest`**

In `crates/rollshot-iced-overlay/src/lib.rs`, change the field:

```rust
pub struct OverlayConfig {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
    pub request: rollshot_capture::CaptureRequest, // was: initial_mode: CaptureMode
    pub target_output_name: Option<String>,
}
```

- [ ] **Step 2: Decouple acquisition routing in `linux_runner.rs`**

In `crates/rollshot-iced-overlay/src/linux_runner.rs`:

- `run_initial_path` (~line 581): replace `if config.initial_mode == CaptureMode::Fullscreen` with `if config.request.scope == rollshot_capture::CaptureScope::Fullscreen`.
- `run_overlay_session` (~line 592): replace the same comparison; keep the existing `OverlayError::Overlay("fullscreen must not reach the overlay runner")`.
- `acquire_resource` (~lines 109–129): change the signature and body to match on `Workflow` (no `Fullscreen` arm — fullscreen is handled by the scope routing above, so the previous `unreachable!` arm is deleted):

```rust
pub(crate) fn acquire_resource(
    request: rollshot_capture::CaptureRequest,
    config: &OverlayConfig,
    factories: &ResourceFactories,
) -> Result<Option<CaptureResource>, OverlayError> {
    use rollshot_capture::Workflow;
    match request.workflow {
        Workflow::Scrolling => acquire_scrolling_resource(config, factories),
        Workflow::Screenshot => {
            tracing::debug!(target: TARGET_OVERLAY, "acquiring one-shot capture resource");
            let capture = match (factories.one_shot)(config.show_cursor) {
                Ok(c) => c,
                Err(rollshot_capture::CaptureError::UserCancelled) => return Ok(None),
                Err(e) => return Err(OverlayError::Capture(e.to_string())),
            };
            Ok(Some(CaptureResource::OneShot(capture)))
        }
    }
}
```

- Update the call site (~line 617) to `acquire_resource(config.request, &config, &factories)?`.
- The `CAPTURE_MODE` global (~line 638) becomes a `Workflow`. Rename it to `CAPTURE_WORKFLOW: Mutex<Option<rollshot_capture::Workflow>>` and store `Some(config.request.workflow)`. Update its other readers (toolbar active-state) accordingly.
- Runtime mode-switch (~line 416, `initial_mode: new_mode`): rebuild the config preserving scope — `request: rollshot_capture::CaptureRequest { workflow: new_workflow, scope: config.request.scope }`.

- [ ] **Step 3: Mirror the routing change in `macos_capture.rs` (macOS path)**

In `crates/rollshot-iced-overlay/src/macos_capture.rs`, apply the same shape:

- The inner capture/acquire fn (`mode: CaptureMode`, ~line 73) → take `workflow: Workflow` and match `Screenshot`/`Scrolling`; delete the `Fullscreen` arm (it is the same defensive `unreachable!`/bypass as Linux — verify and remove).
- `acquire_resource(config.initial_mode, ...)` (~line 245) → `acquire_resource(config.request.workflow, ...)`.
- Struct fields `mode: config.initial_mode` (~264) and `capture_mode: Some(config.initial_mode)` (~283) → store `Workflow` (`config.request.workflow`); rename the fields to `workflow` / `active_workflow` for clarity.
- The `if self.capture_mode != Some(CaptureMode::Region)` guard (~501) → `if self.active_workflow != Some(Workflow::Screenshot)` (Region screenshot ⇒ Screenshot workflow).
- `activate_mode(new_mode: CaptureMode)` (~767) → `activate_workflow(new_workflow: Workflow)`; line 779 `initial_mode: new_mode` → `request: CaptureRequest { workflow: new_workflow, scope: self.scope_or_region() }` (scope stays `Region` in the overlay); line 787 `acquire_resource(new_mode, ...)` → `acquire_resource(new_workflow, ...)`.
- `OverlayEffect::ActivateMode(new_mode)` (~722) → `ActivateWorkflow(new_workflow)` (see Step 5).

- [ ] **Step 4: Migrate overlay state in `app.rs` and `workspace.rs`**

In `crates/rollshot-iced-overlay/src/workspace.rs`: rename `active_mode: CaptureMode` → `active_workflow: Workflow`, `new(mode: CaptureMode)` → `new(workflow: Workflow)`, `active_mode()` → `active_workflow()`, `activate_mode(mode)` → `activate_workflow(workflow)`. Update the `WorkspaceMessage::ActivateMode(CaptureMode)` variant (~line 14) → `ActivateWorkflow(Workflow)`.

In `crates/rollshot-iced-overlay/src/app.rs`:
- `state.mode: CaptureMode` (~line 90) → `state.workflow: Workflow`; constructor defaults (`mode: CaptureMode::Scrolling`, `WorkspaceState::new(CaptureMode::Scrolling)`) → `Workflow::Scrolling`.
- `OverlayMessage::ActivateMode(CaptureMode)` (lines 48, 61) → `ActivateWorkflow(Workflow)`.
- Every `match state.mode { Region => A, Scrolling => B, Fullscreen => unreachable!() }` (lines 665, 710, 767; and the `(Some(handle), CaptureMode::Region)` match at 458) becomes a two-arm match on `state.workflow`: `Workflow::Screenshot => A`, `Workflow::Scrolling => B`. **Delete the `Fullscreen` `unreachable!` arms** — `Workflow` has no `Fullscreen`, so the match is exhaustive without them.
- `state.mode == CaptureMode::Region` (~740) → `state.workflow == Workflow::Screenshot`.
- The `ActivateMode` handler (~781): `state.workflow = workflow; state.workspace.activate_workflow(workflow);` and the region/passthrough match (`Scrolling => ToolbarOnly`, `Region => None`, `Fullscreen => ...`) → `Workflow::Scrolling => ToolbarOnly`, `Workflow::Screenshot => None` (drop the Fullscreen arm).
- Map toolbar actions to workflows where `ToolbarAction` is handled: `RegionMode => Workflow::Screenshot`, `ScrollingMode => Workflow::Scrolling`.

- [ ] **Step 5: Update the toolbar to highlight by workflow**

In `crates/rollshot-iced-overlay/src/toolbar.rs`, change `active_mode: CaptureMode` → `active_workflow: Workflow` in both `action_style_fn` (~84) and `render_toolbar` (~125), and update the active-match:

```rust
let is_active = matches!(
    (action, active_workflow),
    (ToolbarAction::RegionMode, Workflow::Screenshot)
        | (ToolbarAction::ScrollingMode, Workflow::Scrolling)
);
```

Keep `ToolbarAction` variant names, labels (📷 / 📜), and tooltips ("Region Mode" / "Scrolling Mode") **unchanged** — no UX change.

- [ ] **Step 6: Update `fullscreen.rs`, the `capture_overlay` bin, and remaining fixtures**

- `crates/rollshot-iced-overlay/src/fullscreen.rs` (~line 87, `initial_mode: mode`): build the `OverlayConfig` with `request: CaptureRequest::screenshot_fullscreen()` (fullscreen is always the Screenshot workflow here).
- `crates/rollshot-iced-overlay/src/bin/capture_overlay.rs` (~line 22): `initial_mode: CaptureMode::Scrolling` → `request: CaptureRequest::scrolling_region()`.
- All `initial_mode: CaptureMode::Scrolling` test fixtures in `linux_runner.rs` (lines 731, 1252, 1263, 1311) → `request: CaptureRequest::scrolling_region()`; the fullscreen routing test (~1432–1434, `config.initial_mode = CaptureMode::Fullscreen`) → `config.request = CaptureRequest::screenshot_fullscreen()`.
- `macos_capture.rs` test fixtures (lines 1074, 1084, 1102): `Some(CaptureMode::Region)` → `Some(Workflow::Screenshot)`; `c.overlay.mode = CaptureMode::Region` → `c.overlay.workflow = Workflow::Screenshot`; `CaptureMode::Scrolling` → `Workflow::Scrolling`.

- [ ] **Step 7: Convert at the `rollshot-app` boundary (keep the launch payload on `CaptureMode`)**

In `crates/rollshot-app/src/macos_product.rs`:
- `initial_capture_path(mode: CaptureMode)` (~53) → take the scope: `fn initial_capture_path(scope: CaptureScope) -> InitialCapturePath { match scope { CaptureScope::Fullscreen => InitialCapturePath::Fullscreen, CaptureScope::Region => InitialCapturePath::Overlay } }`. Update its callers: `initial_capture_path(config.request.scope)` (~122) and the two unit-test calls (~759, 763) → `initial_capture_path(CaptureScope::Fullscreen)` / `(CaptureScope::Region)`.
- OverlayConfig construction (~632–636, `initial_mode: CaptureMode::Region`) → `request: CaptureRequest::screenshot_region()`.
- Wherever `rollshot-app` builds an `OverlayConfig` from the parsed `InteractiveLaunchOptions`, set `request: options.initial_mode.into()` (the `From<CaptureMode>` bridge). `cargo build` will point to the exact construction site(s).

In `crates/rollshot-app/src/launch.rs`: update any remaining `CaptureMode` references the compiler flags (assertions/imports) to read through the bridge or the new types as appropriate.

- [ ] **Step 8: Build the workspace and find any missed sites**

Run: `rtk cargo build --workspace`
Expected: success. If it fails, the errors are the remaining `CaptureMode` sites in the overlay/app layer — apply the substitution rule from the File Structure table and rebuild until green.

- [ ] **Step 9: Run the affected test suites (both platforms' shared tests)**

Run: `rtk cargo test -p rollshot-iced-overlay -p rollshot-app`
Expected: PASS (behavior unchanged; the fullscreen-routing and toolbar/workspace tests still pass with the new types).

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rollshot-iced-overlay crates/rollshot-app
rtk git commit -m "refactor(overlay): drive capture by Workflow x CaptureScope

Decouple overlay-vs-direct routing onto CaptureScope; the overlay now
carries Workflow (Screenshot/Scrolling), dropping the defensive Fullscreen
unreachable arms. Launch payload still on CaptureMode, bridged at the app
boundary. Behavior unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Migrate the launch payload + CLI + serde tests (green)

Flip `InteractiveLaunchOptions` to carry a `CaptureRequest`, update the CLI builder, rewrite the serde tests for the new shape, and remove the `.into()` bridge at the app boundary.

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-cli/src/cmd_capture_launcher.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs` (and any other OverlayConfig construction)

- [ ] **Step 1: Rewrite the serde tests for the new payload shape**

In `crates/rollshot-capture/src/types.rs` tests: delete `capture_modes_round_trip_with_current_names`, `legacy_screenshot_mode_deserializes_as_region`, and the `CaptureMode`-based `interactive_launch_options_round_trip_json` / `interactive_launch_options_ignore_obsolete_field` / `fps_change_does_not_affect_initial_mode`. Replace with:

```rust
#[test]
fn interactive_launch_options_round_trip_json() {
    let options = InteractiveLaunchOptions {
        backend: "linux-portal".to_string(),
        fps: 7,
        show_cursor: true,
        initial_request: CaptureRequest::screenshot_region(),
    };
    let json = serde_json::to_string(&options).expect("serialize");
    assert!(json.contains(r#""initial_request":{"workflow":"screenshot","scope":"region"}"#), "json = {json}");
    let decoded: InteractiveLaunchOptions = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, options);
}

#[test]
fn interactive_launch_options_default_initial_request() {
    let json = r#"{"backend":"auto","fps":5,"show_cursor":false}"#;
    let decoded: InteractiveLaunchOptions = serde_json::from_str(json).expect("deserialize");
    assert_eq!(decoded.initial_request, CaptureRequest::scrolling_region());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-capture types::interactive_launch`
Expected: FAIL — `InteractiveLaunchOptions` has no field `initial_request`.

- [ ] **Step 3: Migrate the payload field**

In `crates/rollshot-capture/src/types.rs`, change `InteractiveLaunchOptions`:

```rust
pub struct InteractiveLaunchOptions {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
    #[serde(default)]
    pub initial_request: CaptureRequest,
}
```

And update `default_capture()`:

```rust
pub fn default_capture() -> Self {
    Self {
        backend: "auto".to_string(),
        fps: 5,
        show_cursor: false,
        initial_request: CaptureRequest::scrolling_region(),
    }
}
```

- [ ] **Step 4: Update the CLI builder**

In `crates/rollshot-cli/src/cmd_capture_launcher.rs`, `launch_options` (~40):

```rust
fn launch_options(args: &CaptureArgs) -> InteractiveLaunchOptions {
    InteractiveLaunchOptions {
        backend: args.backend.clone(),
        fps: args.fps,
        show_cursor: args.show_cursor,
        initial_request: rollshot_capture::CaptureRequest::scrolling_region(),
    }
}
```

Update the CLI test asserting the field (`launch_options_keep_only_interactive_fields` ~225) to compare `initial_request` against `CaptureRequest::scrolling_region()`.

- [ ] **Step 5: Remove the bridge at the app boundary**

In `crates/rollshot-app` wherever `OverlayConfig` is built from the payload, replace `request: options.initial_mode.into()` with `request: options.initial_request` (direct, no conversion). `cargo build -p rollshot-app` flags the site(s).

- [ ] **Step 6: Build + test the payload path**

Run: `rtk cargo test -p rollshot-capture -p rollshot-cli -p rollshot-app`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-capture crates/rollshot-cli crates/rollshot-app
rtk git commit -m "refactor(capture): launch payload carries CaptureRequest

initial_mode: CaptureMode -> initial_request: CaptureRequest; drop the legacy
\"screenshot\" alias (ephemeral same-version payload). Bridge at the app
boundary removed.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Delete `CaptureMode` and the bridge (green)

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Confirm there are no non-bridge references left**

Run: `rtk grep -rn "CaptureMode" crates/ --include="*.rs"`
Expected: only the `CaptureMode` definition, `From<CaptureMode> for CaptureRequest`, and `to_legacy_mode` (all in `types.rs`), plus its `lib.rs` re-export. If anything else appears, migrate it with the substitution rule before continuing.

- [ ] **Step 2: Delete the type and the bridge**

In `crates/rollshot-capture/src/types.rs`: delete the `CaptureMode` enum, its `#[cfg(test)]` round-trip tests (already replaced in Task 3), the `impl From<CaptureMode> for CaptureRequest`, and `CaptureRequest::to_legacy_mode`. Delete the `legacy_capture_mode_maps_to_request` and `request_round_trips_through_legacy_mode` tests added in Task 1 (they test the now-deleted bridge).

In `crates/rollshot-capture/src/lib.rs`: remove `CaptureMode` from the `pub use types::{...}` re-export.

- [ ] **Step 3: Full workspace verification**

Run: `rtk cargo build --workspace`
Expected: success (no remaining references).

Run: `rtk cargo test --workspace`
Expected: PASS.

Run: `rtk cargo fmt --check`
Expected: clean.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-capture
rtk git commit -m "refactor(capture): remove CaptureMode in favor of Workflow x CaptureScope

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Type model (`Workflow`, `CaptureScope`, `CaptureRequest`, `needs_overlay`, `is_supported`) → Task 1.
- Old→new 1:1 mapping → Task 1 (`From`) + Task 2/3 construction sites.
- Acquisition path = f(scope) → Task 2 Step 2 (linux), Step 3 (macOS), Step 7 (`initial_capture_path`).
- Scope → backend `RegionMode` → unchanged downstream (existing `RegionMode` construction preserved); no new code needed (documented in spec).
- Toolbar rewiring + unchanged labels → Task 2 Steps 4–5.
- Serialization (ephemeral payload, drop legacy alias) → Task 3.
- Testing (rewritten serde, mapping, `needs_overlay`, `is_supported`, existing green) → Tasks 1, 3, and the build/test steps.
- Sequencing (refactor lands first) → this plan; Action Guide plan follows.
- `is_supported()` rejects `Scrolling × Fullscreen` → Task 1 test.

**Placeholder scan:** No TBD/TODO. Mechanical site-updates are specified by an explicit substitution table + `cargo build` worklist, not hand-waved logic; every logic change (routing, highlight, message mapping, serde) has complete code.

**Type consistency:** `CaptureRequest` / `Workflow` / `CaptureScope`, constructors `screenshot_region()` / `screenshot_fullscreen()` / `scrolling_region()`, `needs_overlay()`, `is_supported()`, `to_legacy_mode()`, and field names (`request`, `initial_request`, `active_workflow`, `state.workflow`) are used consistently across all tasks.

## Notes

- Branch: land this on `refactor/capture-workflow-scope` (per the spec's sequencing). The Action Guide plan (P0a) builds `Workflow::ActionGuide` on top afterward.
- Platform split (AGENTS.md §8): Task 2 explicitly migrates both the Linux (`linux_runner.rs`) and macOS (`macos_capture.rs`, `macos_product.rs`) paths; every build/test step must pass before commit.

## Handoff: Action Guide spec update (deferred to post-refactor)

Removing `CaptureMode` makes several passages in
`docs/superpowers/specs/2026-06-15-action-guide-capture-design.md` stale.
**By decision, do NOT edit the Action Guide spec as part of this refactor.**
Update it *after* this refactor lands and is verified — as the first step of the
Action Guide P0a plan. The required edits (all are framing/vocabulary, not
behavior):

1. **"Starting A Recording"** — the paragraph beginning *"Action Guide does not
   become a `CaptureMode` variant … a separate workflow flag, analogous to KDE
   Spectacle's `videoMode`"*: replace with *"Action Guide is a first-class
   `Workflow::ActionGuide` variant (alongside `Screenshot`/`Scrolling`), always
   at `CaptureScope::Region`."* Drop the `videoMode`-flag analogy — it was a
   workaround for the conflated enum that no longer exists.

2. **"Toolbar Entry And Recording Controls" → Entry button** — *"`ToolbarAction::ActionGuide`
   … It is an action, not a `CaptureMode`"*: restate as *"activates
   `Workflow::ActionGuide` via the toolbar's `ActivateWorkflow(Workflow)`
   message"* (this refactor already makes the toolbar a workflow-switcher).

3. **Non-Goals** — *"Action Guide is not a `CaptureMode`; the three
   image-acquisition modes (Region, Scrolling, Fullscreen) are unchanged"*:
   restate as *"Action Guide is `Workflow::ActionGuide`, always
   `CaptureScope::Region`; the `Screenshot`/`Scrolling` workflows and the
   `Region`/`Fullscreen` scopes are unchanged."* The region-only rationale
   still holds — and the AG plan should extend `CaptureRequest::is_supported()`
   to also reject `ActionGuide × Fullscreen` (mirroring `Scrolling × Fullscreen`).

4. **Acceptance Criteria** — *"Existing Region, Scrolling, Fullscreen … behavior
   remains unchanged"*: restate in the workflow × scope vocabulary.

Nothing else in the Action Guide spec depends on `CaptureMode`; the
`rollshot-action`, frame-pipeline, detection, privacy, and export sections are
unaffected.
