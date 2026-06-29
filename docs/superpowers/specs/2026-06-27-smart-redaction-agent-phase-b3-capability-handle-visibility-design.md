# Smart Redaction Agent Phase B3 Capability Handle Visibility

**Date:** 2026-06-27
**Status:** Draft for user review

## Goal

Expose the current `AutomationInput.capability_handles` state to the Smart
Redaction authoring agent so the model can tell whether template-backed
capabilities are available before it writes JavaScript.

Phase B3 does not make template matching broadly usable. It only surfaces
handles that already exist in the run input and makes dry-run use the same
handle map that inspection reports. New template creation, persistence,
selection, naming, and review remain Phase F work.

## Product Assumption

The current product workbench has no template-handle lifecycle, so normal Smart
Redaction runs still start with an empty handle map. That empty state is useful
context: the agent should see `template_match` as unavailable with
`no_capability_handles` rather than trying `rollshot.templateMatch` blindly.

When a future caller or test harness constructs a run with handles, Phase B3
should expose those handles through `inspect_image_context` and pass the same
map into `dry_run`.

## Success Criteria

- `ToolContext` owns the run's capability handle map.
- `dry_run` passes `ToolContext.capability_handles` into
  `AutomationInput.capability_handles`.
- `inspect_image_context` returns bounded capability handle metadata.
- `inspect_image_context.capabilities.template_match` is:
  - `unavailable` with `no_capability_handles` when the map is empty,
  - `available` when at least one handle is present.
- Product Smart Redaction runs still pass an empty handle map until Phase F
  adds a lifecycle.
- The Smart Redaction prompt tells the model to inspect handles before using
  `rollshot.templateMatch`.
- No raw template pixels, previews, or persisted template assets are exposed.

## Scope

### In Scope

- Add capability handle metadata to agent inspection context/results.
- Add a `capability_handles` argument to `ToolContext::new`.
- Update all current `ToolContext::new` call sites.
- Keep product workbench runs on an empty handle map.
- Make dry-run and inspection share the same handle source.
- Update focused agent/workbench tests and prompt contract tests.

### Out of Scope

- Template asset creation.
- Template handle persistence.
- Template-match inspection tools.
- Preparing `RealAutomationHost::prepare_template_match` in product runs.
- UI for selecting or naming template handles.
- Any change to JavaScript validation or the templateMatch capability schema.

## Design

### 1. Run-Level Capability Handles

`rollshot_automation::AutomationInput` already contains:

```rust
pub capability_handles: BTreeMap<String, String>,
```

Phase B3 makes `rollshot-agent` preserve this map at the run context level:

```rust
pub struct ToolContext {
    pub capability_handles: BTreeMap<String, String>,
    ...
}
```

`ToolContext::new` should accept a `BTreeMap<String, String>` argument. The
workbench passes `BTreeMap::new()` for now. Tests can pass a populated map to
prove the path is real without inventing product persistence.

### 2. Inspection Result Shape

Add a bounded serializable handle summary:

```rust
pub struct CapabilityHandleSummary {
    pub name: String,
    pub handle: String,
    pub capability: String,
}
```

The map key becomes `name`, the map value becomes `handle`, and the capability
is `"template_match"` in this phase. The result should be deterministic by
using the existing `BTreeMap` ordering and should cap the list to a small fixed
limit, for example 16 entries. If more entries exist, the result should include
the first 16 and still report `template_match` as `available`; Phase B3 does not
need pagination.

`inspect_image_context` should add:

```json
"capability_handles": [
  { "name": "logo", "handle": "tpl-logo-v1", "capability": "template_match" }
]
```

This is metadata only. It does not expose template bytes, previews, source
image crops, or match results.

### 3. Capability Availability

`inspect_image_context.capabilities.template_match` should be computed from the
run handle map:

- empty map: `{"status":"unavailable","reason":"no_capability_handles"}`,
- non-empty map: `{"status":"available","reason":null}`.

`layout` remains unavailable. `ocr` and `region_features` keep the B1/B2
availability behavior.

### 4. Dry-Run Consistency

`DryRunTool` currently creates `AutomationInput` with an empty
`capability_handles` map. Phase B3 should replace that with
`self.ctx.capability_handles.clone()`.

This keeps pre-authoring inspection and JavaScript execution aligned. If
`inspect_image_context` says a handle exists, `input.capabilityHandles` in
dry-run sees the same handle.

Phase B3 does not prepare template-match results in `RealAutomationHost`, so a
script that calls `rollshot.templateMatch` can still receive the current host
error unless a test harness has also prepared/faked template results. That is
acceptable: B3 exposes handle visibility, not template execution lifecycle.

### 5. Product Workbench

The workbench should add a small local helper:

```rust
fn product_capability_handles() -> BTreeMap<String, String> {
    BTreeMap::new()
}
```

Use it when constructing `ToolContext`. This makes the intentional empty state
visible and creates a single future edit point for Phase F.

`authoring_inspection_context` does not need to receive a separate handle map if
`InspectImageContextTool` can derive handle summaries from `ToolContext`.
Keeping the source in one place avoids drift between inspection and dry-run.

### 6. Prompt Contract

Update the Smart Redaction authoring guide:

- tell the model to call `inspect_image_context` before using
  `rollshot.templateMatch`,
- tell it to use only handles listed in `capability_handles`,
- tell it not to invent template handles when the list is empty,
- keep template creation/persistence out of scope.

## Error Handling

- Empty handle map is not an error; it is a structured unavailable state.
- Oversized handle maps are bounded in inspection output.
- `dry_run` remains responsible for surfacing execution errors from
  `rollshot.templateMatch`.
- Unknown or stale handles are not resolved in B3 because no product handle
  lifecycle exists yet.

## Testing

### Agent Tool Tests

- `inspect_image_context` returns an empty `capability_handles` list by
  default.
- `inspect_image_context` returns handle summaries from a populated
  `ToolContext`.
- `inspect_image_context.capabilities.template_match` is unavailable when the
  map is empty.
- `inspect_image_context.capabilities.template_match` is available when the map
  is non-empty.
- `dry_run` passes capability handles into `AutomationInput`; use a source that
  reads `input.capabilityHandles.logo` and returns a candidate only when it is
  present.

### Workbench Tests

- Product workbench test context constructs `ToolContext` with an empty handle
  map.
- Product `inspect_image_context` still reports `template_match` unavailable
  with `no_capability_handles`.

### Prompt Tests

- Provider request tests assert the prompt tells the model to inspect
  `capability_handles` before `rollshot.templateMatch`.
- Existing OCR and region-feature prompt assertions remain unchanged.

## Verification Commands

```bash
rtk cargo test -p rollshot-agent inspect_image_context
rtk cargo test -p rollshot-agent dry_run
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo fmt --check
```

## Risks

- The model may overinterpret a visible handle as proof template matching is
  prepared. The prompt and availability text must be explicit that handles are
  necessary but not a persistence lifecycle.
- A future Phase F implementation could add sensitive template labels. B3
  should keep handle summaries minimal and avoid previews or raw pixels.
- If inspection and dry-run use different handle sources, the model will write
  scripts that cannot reproduce inspected context. `ToolContext` must remain the
  single source for this phase.

## Deferred Work

- Template asset lifecycle and persistence.
- Product UI for creating template handles.
- Template-match inspection.
- Preparing template-match results in product authoring runs.
- User-facing explanations for missing or stale persisted templates.
