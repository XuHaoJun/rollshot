# Action Guide Storyboard Issue Pack Design

Date: 2026-07-09
Status: Approved design
Branch: `feat/action-guide-storyboard-issue-pack`

## Purpose

Action Guide Storyboard Issue Pack integration makes the existing Storyboard PNG
part of Local Issue Pack exports. The goal is to give recipients one visual
overview of the reviewed workflow before they inspect individual keyframes.

This is the P1 slice from
`docs/ideas/2026-07-09-rollshot-action-guide-storyboard-umbrella-prd.md`.
It intentionally does not add Storyboard preview, layout controls, captions,
per-step annotations, copy-to-clipboard, or agent suggestions. Those remain
separate follow-up designs.

## Product Scope

Include:

- Generate `action-guide/storyboard.png` by default for Issue Packs that include
  a reviewed Action Guide.
- Show the Storyboard as an Overview image near the top of `issue.md` when the
  PNG exists.
- Add an `action_storyboard` asset entry to `manifest.json` when the PNG exists.
- Treat Storyboard generation as optional: if it fails after the guide export
  succeeds, keep the Issue Pack valid and record a warning.
- Preserve existing screenshot-only, Action Guide-only, combined, folder, ZIP,
  GIF, `steps.md`, `session.json`, and keyframe behavior.

Out of scope:

- No user-facing checkbox or setting for Storyboard inclusion.
- No changes to the Timeline Workspace header or export dialog.
- No changes to `rollshot-action` renderer APIs.
- No caption or annotation model changes.
- No claim that Action Guide keyframes are redacted or sensitive-free.

## User Experience

When a user exports a Bug Report from Action Guide, the folder layout becomes:

```text
rollshot-issue-pack-YYYY-MM-DD-HHMM/
  issue.md
  manifest.json
  action-guide/
    steps.md
    session.json
    storyboard.png
    keyframes/
      001.png
      002.png
    guide.gif
```

`guide.gif` remains optional and appears only when GIF export succeeds.
`storyboard.png` appears when Storyboard export succeeds.

In `issue.md`, the Action Guide section includes the overview before individual
step images:

```md
## Steps to reproduce

Overview:

![](action-guide/storyboard.png)

1. Open Settings

   ![](action-guide/keyframes/001.png)
```

If Storyboard export fails, the Overview block is omitted. The individual step
images remain, and the warning is returned to the UI through the existing Issue
Pack warning path.

## Architecture

Keep the integration in the existing app-level Issue Pack module:

```text
crates/rollshot-app/src/issue_pack.rs
```

The existing `rollshot_action::export_storyboard(...)` API is sufficient for
this slice. The Issue Pack builder should call it after
`rollshot_action::export_guide(...)` succeeds and before Markdown and manifest
files are rendered.

The export order is:

1. Validate Issue Pack input.
2. Write final screenshot image when present.
3. Export Action Guide assets when an Action Guide source is present.
4. Attempt Storyboard export to `action-guide/storyboard.png`.
5. Attempt GIF export when requested.
6. Render `issue.md`.
7. Render `manifest.json`.
8. Swap the temp folder into place, then optionally ZIP it.

Storyboard failure must not mask a valid guide export. `export_guide(...)`
remains required for Action Guide packs because it writes `steps.md`,
`session.json`, and keyframes. If `export_guide(...)` fails, the whole Issue
Pack export still fails and rolls back temp output.

## Data Model

Do not add a user-configurable `include_storyboard` field in this slice.
Storyboard inclusion is the default behavior for Action Guide Issue Packs.

Use a local export-build flag, matching the existing GIF handling:

```rust
let include_storyboard = tmp_dir.join("action-guide/storyboard.png").exists();
```

Thread that flag into the render helpers:

```rust
render_issue_markdown(input, include_storyboard)
render_manifest_json(input, warnings, include_gif, include_storyboard)
manifest_assets(input, include_gif, include_storyboard)
```

Do not add Storyboard state to `ActionGuideIssueAssets` in this slice. That
model describes requested/known Action Guide evidence before export; the
Storyboard is an optional derived asset whose presence is known only after the
export attempt.

The public behavior is:

- `action_storyboard` appears in the manifest only if
  `action-guide/storyboard.png` exists.
- The Markdown Overview block appears only if `action-guide/storyboard.png`
  exists.
- Missing Storyboard never causes Markdown or manifest to reference a missing
  file.

## Warning Behavior

Add warning code:

```text
storyboard_export_failed
```

The warning message should include the renderer error text, for example:

```text
Storyboard export failed: canvas too large
```

The warning is recorded in:

- `IssuePackExportResult.warnings`
- `manifest.json` warnings

This matches the existing optional GIF warning behavior.

## Manifest

When Storyboard exists, add:

```json
{
  "kind": "action_storyboard",
  "path": "action-guide/storyboard.png"
}
```

The asset should be listed near the other Action Guide assets. Exact ordering is
not user-visible, but tests should pin a deterministic order to avoid churn.

Do not alter existing asset kinds:

- `action_steps`
- `action_session`
- `action_keyframe`
- `action_gif`

## Privacy And Evidence Language

This slice does not change redaction semantics. `storyboard.png` is composed
from reviewed Action Guide keyframes. It should be treated as reviewed evidence,
not as redacted output.

The Issue Pack must not claim:

- all sensitive content was detected,
- Action Guide keyframes are redacted,
- the whole pack is sensitive-free.

If future work adds redacted or annotated Storyboards, that must be designed as
a separate export semantics change.

## Testing

Add focused tests in `crates/rollshot-app/src/issue_pack.rs`.

Required tests:

- Markdown includes the Storyboard Overview block when an Action Guide asset
  says Storyboard is present.
- Markdown omits the Overview block when Storyboard is absent.
- Manifest assets include `action_storyboard` only when Storyboard is present.
- Feature-gated Action Guide export writes
  `action-guide/storyboard.png` for a normal reviewed guide.
- Existing screenshot-only Issue Pack tests still pass.
- Existing Action Guide keyframe and GIF manifest behavior remains unchanged.

Optional test:

- If a Storyboard failure can be induced cleanly without brittle filesystem
  permissions or platform assumptions, verify the export succeeds, the warning
  code is `storyboard_export_failed`, and Markdown/manifest omit Storyboard.

If the optional failure test would require brittle filesystem tricks, keep the
failure behavior small and directly adjacent to the existing GIF warning path,
and rely on warning serialization tests plus normal successful export coverage.

## Acceptance Criteria

- Action Guide-only Issue Packs include `action-guide/storyboard.png` when
  Storyboard export succeeds.
- Combined screenshot plus Action Guide Issue Packs include
  `action-guide/storyboard.png` when Storyboard export succeeds.
- `issue.md` references Storyboard through the relative path
  `action-guide/storyboard.png` only when the file exists.
- `manifest.json` includes `action_storyboard` only when the file exists.
- Storyboard export failure records `storyboard_export_failed` and does not
  block the Issue Pack when `steps.md`, `session.json`, and keyframes were
  exported successfully.
- Existing screenshot-only Issue Pack behavior is unchanged.
- Existing Action Guide `steps.md`, `session.json`, keyframe, and optional GIF
  behavior is unchanged.
