# Capture Workflow × Scope Refactor Design

## Summary

Split the conflated `CaptureMode` enum into two orthogonal axes — a capture
**workflow** (what we do with the frames) and a capture **scope** (what area we
capture). This is a pure structural, behavior-preserving refactor: the three
existing capture behaviors are unchanged, and it lands before the Action Guide
feature so that feature can add a new workflow on a clean base.

```text
   CaptureMode { Scrolling, Region, Fullscreen }     -- conflates two axes
                          |
                          v
   Workflow { Screenshot, Scrolling }   ×   CaptureScope { Region, Fullscreen }
```

## Motivation

Today `CaptureMode { Scrolling, Region, Fullscreen }` mixes two unrelated
concepts:

- `Scrolling` answers **what workflow** (scroll + stitch into a long image).
- `Region` / `Fullscreen` answer **what scope** (a selected rect vs the whole
  source).

Consequences of the conflation:

- "Scrolling fullscreen" and "Action Guide fullscreen" have no representable
  home.
- `Fullscreen` smuggles in a *second* meaning — "skip the region-selection
  overlay" — so the overlay-vs-bypass decision is tangled into the workflow
  branch (`match mode { Fullscreen => bypass, Scrolling | Region => overlay }`,
  plus an `unreachable!` for fullscreen inside the overlay).
- The scope axis already exists one layer down: `rollshot-capture` has
  `RegionMode { Manual(Region), PortalPicker, FullSource }`. The UI layer is not
  missing the scope concept — it is duplicating it badly by fusing it with the
  workflow.

This refactor surfaces the scope axis consistently at the UI/workflow layer and
aligns it with the backend `RegionMode` that already exists.

## Goals

- Replace `CaptureMode` with an orthogonal `Workflow` × `CaptureScope` model.
- Preserve all current runtime behavior: the three existing capture flows work
  identically.
- Decouple the region-selection-overlay-vs-direct decision so it depends only on
  scope.
- Keep every existing test green (with mechanical type updates) as the proof of
  behavior preservation.
- Leave a clean base for the Action Guide feature to add `Workflow::ActionGuide`.

## Non-Goals

- Adding `Workflow::ActionGuide` — that is added by the Action Guide plan (P0a)
  on top of this refactor.
- Wiring any newly-expressible combination (notably `Scrolling × Fullscreen`).
  It becomes type-expressible but stays unsupported until a feature wants it.
- Any UX change: toolbar labels, icons, and behavior are unchanged.
- Adding a full-fidelity video recording workflow (mp4/webm). That is a separate
  future `Workflow`, not part of this refactor.
- Moving the types to a higher-level crate (see Type Placement).

## Type Model

The new types live in `rollshot-capture::types`, where `CaptureMode` lives
today (minimal churn — see Type Placement).

```rust
/// WHAT we do with the captured frames.
pub enum Workflow {
    Screenshot, // one-shot single image
    Scrolling,  // scroll + stitch into a long image
    // ActionGuide is added later by the Action Guide plan.
}

/// WHAT AREA we capture. Resolves down to the backend `RegionMode`.
pub enum CaptureScope {
    Region,     // a user-selected rectangle (overlay or portal picker)
    Fullscreen, // the whole source
}

pub struct CaptureRequest {
    pub workflow: Workflow,
    pub scope: CaptureScope,
}

impl CaptureRequest {
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
```

Both enums derive the traits `CaptureMode` derives today (`Debug, Clone, Copy,
PartialEq, Eq, Serialize, Deserialize`), plus a `Default` for `Workflow`
(`Scrolling`) and `CaptureScope` (`Region`) to back `CaptureRequest`'s default
of `{ Scrolling, Region }`.

## Old → New Mapping (Behavior-Preserving Contract)

Exactly three combinations are constructed, mapping 1:1 to today's modes:

| Today `CaptureMode` | `CaptureRequest`           |
|---------------------|----------------------------|
| `Region`            | `{ Screenshot, Region }`   |
| `Fullscreen`        | `{ Screenshot, Fullscreen }` |
| `Scrolling`         | `{ Scrolling, Region }`    |

No other combination is built by the UI, CLI, or defaults. `Scrolling ×
Fullscreen` is rejected at construction by `is_supported()`.

## Acquisition Path Becomes `f(scope)`

The central cleanup: the overlay-vs-direct decision depends only on scope, and
the workflow only decides what happens to the captured frames.

```text
CaptureRequest { workflow, scope }
   |
   +-- scope == Fullscreen --> capture the whole source DIRECTLY (no overlay)
   |                             +- Screenshot -> one-shot PNG
   |                             +- Scrolling  -> (unwired)
   |
   +-- scope == Region ------> region-selection OVERLAY
                                 +- Screenshot  -> one-shot crop
                                 +- Scrolling   -> scroll + stitch
                                 +- ActionGuide -> (added by the AG plan)
```

Specifically:

- macOS `initial_capture_path` becomes `match scope { Fullscreen => Fullscreen,
  Region => Overlay }` — independent of workflow.
- The Linux fullscreen routing keys off `scope == Fullscreen` (fullscreen is
  routed before the overlay activates, as today).
- The in-overlay `acquire_resource` matches on `workflow`: `Screenshot` →
  one-shot resource, `Scrolling` → scrolling resource. The
  `unreachable!("fullscreen is routed before active overlay state")` remains
  valid because fullscreen never enters the overlay.

## Scope → Backend `RegionMode`

The UI scope resolves to the existing backend region spec:

- `Fullscreen` → `RegionMode::FullSource`
- `Region` → `RegionMode::Manual(rect)` after manual overlay selection, or
  `RegionMode::PortalPicker` on the Linux portal backend.

## Toolbar Rewiring

The overlay toolbar's `RegionMode` (📷) and `ScrollingMode` (📜) buttons are
in-overlay, i.e. always `scope = Region`; they actually toggle the **workflow**.
So:

- `Message::ActivateMode(CaptureMode)` → `ActivateWorkflow(Workflow)`, where 📷
  selects `Screenshot` and 📜 selects `Scrolling`.
- Visible labels and icons are unchanged in this refactor (no UX change). Only
  the internal type is corrected.

This leaves the toolbar as a clean workflow-switcher, so the Action Guide plan
can add its 🎬 button as another workflow with no further restructuring.

## Blast Radius

`CaptureMode` is referenced in ~128 places across 13 files. The change is wide
but mechanical (a type split, not a logic change):

- `rollshot-capture`: `src/types.rs` (new types + `RegionMode` mapping),
  `src/lib.rs` (re-exports).
- `rollshot-cli`: `src/cmd_capture_launcher.rs` (build `CaptureRequest`; reject
  unsupported combinations via `is_supported()`).
- `rollshot-app`: `src/launch.rs`, `src/macos_product.rs`
  (`initial_capture_path` → `f(scope)`).
- `rollshot-iced-overlay`: `src/linux_runner.rs`, `src/macos_capture.rs`,
  `src/fullscreen.rs`, `src/toolbar.rs`, `src/workspace.rs`, `src/app.rs`,
  `src/lib.rs`, `src/bin/capture_overlay.rs`.

## Serialization And Compatibility

`CaptureMode` appears only in the ephemeral CLI→app launch JSON
(`InteractiveLaunchOptions.initial_mode`) and in in-memory state — it is **not**
persisted to settings or storage (verified against the usage map). Therefore:

- `InteractiveLaunchOptions.initial_mode: CaptureMode` becomes
  `initial_request: CaptureRequest` (defaulting to `{ Scrolling, Region }`).
- The launch-payload round-trip tests are rewritten for the new shape.
- The legacy `#[serde(alias = "screenshot")]` mapping for `Region` is dropped —
  no stored data depends on it, and the CLI and app are always the same version.
- No migration is required.

## Testing

- Rewrite the `CaptureMode` serde round-trip tests for `Workflow`,
  `CaptureScope`, and `CaptureRequest`.
- Add a mapping test asserting each old mode corresponds to its
  `CaptureRequest` pair.
- Add `needs_overlay() == (scope == Region)`.
- Add `is_supported()` rejects `Scrolling × Fullscreen` and accepts the three
  supported combinations.
- All existing capture, overlay, launch, and macOS-product tests stay green with
  mechanical type updates — this is the behavior-preservation proof.
- Verify with `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, and `cargo test` across the workspace.

## Type Placement

The new types live in `rollshot-capture::types` alongside the current
`CaptureMode`, minimizing churn. Note that `Workflow` is arguably an app-level
concept (scrolling stitching lives in `rollshot-core`, Action Guide in
`rollshot-action` + app), so a future move to a higher-level location is
defensible — but it is out of scope here to keep the diff focused.

## Sequencing

1. This refactor lands first, fully green and behavior-preserving, on its own
   branch (`refactor/capture-workflow-scope`).
2. The Action Guide plan (P0a) then adds `Workflow::ActionGuide` and the 🎬
   toolbar entry on the clean base.
3. The Action Guide spec's "Action Guide is not a `CaptureMode` / separate
   workflow flag (Spectacle `videoMode` analogy)" framing is updated to
   "first-class `Workflow::ActionGuide`."
4. Any newly-expressible combination (e.g. `Scrolling × Fullscreen`) is a
   separate, additive feature — not part of this refactor.

## Acceptance Criteria

- `CaptureMode` is fully replaced by `Workflow` × `CaptureScope` /
  `CaptureRequest` across the workspace.
- The three existing capture flows (region screenshot, fullscreen screenshot,
  scrolling) behave identically to before.
- The overlay-vs-direct decision depends only on `CaptureScope`.
- `is_supported()` rejects `Scrolling × Fullscreen`; the UI/CLI never construct
  it.
- `cargo fmt --check`, `clippy -D warnings`, and the full test suite pass.
- No new workflow, no new wired combination, and no UX change ship in this
  refactor.
