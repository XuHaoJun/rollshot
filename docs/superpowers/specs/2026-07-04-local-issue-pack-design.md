# Local Issue Pack Design

Date: 2026-07-04
Status: Approved design
Branch: `feat/local-issue-pack`

## Purpose

Local Issue Pack turns a screenshot, long screenshot, or Action Guide session into
a local, reviewable, redaction-gated bug evidence bundle. The first release
produces a filesystem artifact that can be pasted into or attached to GitHub
Issues, Jira, Linear, Slack, email, or an internal tracker without making
Rollshot a cloud bug-reporting service.

The feature implements the first-release scope from
`docs/ideas/2026-07-04-rollshot-local-issue-pack.md`. The deferred scope from
that idea remains out of scope: no hosted issue pages, tracker API writes,
browser console/network/DOM capture, session replay backend, team inbox, or
AI-generated full bug-report narrative.

## Product Scope

Include:

- `Export Issue Pack` from Result Workspace for a screenshot or long screenshot.
- `Export Issue Pack` from Action Guide Review for a reviewed session.
- Folder export.
- ZIP export after the folder export succeeds.
- Deterministic GitHub-flavored Markdown `issue.md`.
- `manifest.json` with schema version, creation timestamp, Rollshot version,
  platform metadata, export mode, redaction status, OCR summary, and asset list.
- Final flattened safe screenshot export for Result Workspace packs.
- Reuse of Action Guide `steps.md`, `session.json`, and `keyframes/*.png` when
  an Action Guide session exists.
- Optional `guide.gif` when GIF generation succeeds.
- OCR snippets when OCR visible text data is available.
- Explicit safe/original language consistent with secure redaction sharing.

Conservative defaults:

- English template only.
- GitHub-flavored Markdown.
- Folder export is required; ZIP export is also exposed.
- Action Guide steps are embedded into `issue.md` and the existing
  `action-guide/steps.md` is included.
- Result Workspace packs require a final image.
- Action Guide-only packs are allowed when reviewed steps and keyframes exist.
- Non-sensitive platform metadata is included by default.
- GUI first; no CLI export path in the first release.

## Architecture

Implement the feature as an app-level composition layer:

```text
crates/rollshot-app/src/issue_pack.rs
```

The module coordinates existing app-level state and export helpers. It does not
introduce a new core crate in the first release.

Inputs:

- `ResultWorkspace` document state:
  - full-resolution `ImageDocument`
  - flattened safe output
  - source/export path identity
  - secure redaction state
  - OCR visible text state when the `ocr` feature is enabled
- `TimelineWorkspace` Action Guide state when the `action-guide` feature is
  enabled:
  - reviewed guide titles
  - selected keyframes
  - retained frame store
  - capture region
  - input capability and source kind
- Rollshot version, platform, timestamp, and export destination.

Outputs:

```text
rollshot-issue-pack-YYYY-MM-DD-HHMM/
  issue.md
  manifest.json
  images/
    final-redacted.png
  action-guide/
    steps.md
    session.json
    keyframes/
      001.png
      002.png
      003.png
    guide.gif
```

`images/final-redacted.png` is present for Result Workspace packs and is a
flattened document output. The `action-guide/` directory is present for Action
Guide packs or combined packs. `guide.gif` is included only when GIF export
succeeds. Action Guide keyframes are reviewed evidence images; the first release
does not describe them as redacted unless a later redaction pipeline actually
processes them.

Later, if CLI export becomes important, the pure export model and Markdown /
manifest renderer can be moved into a small reusable crate. The first release
keeps the boundary in `rollshot-app` because the required state already lives
there and is partly feature-gated.

## Data Model

The app-level model should be explicit about required and optional assets:

```rust
pub struct IssuePackInput {
    pub title: Option<String>,
    pub created_at: DateTime<Local>,
    pub rollshot_version: String,
    pub platform: PlatformInfo,
    pub final_image: Option<SafeImageAsset>,
    pub action_guide: Option<ActionGuideIssueAssets>,
    pub ocr_snippets: Vec<OcrSnippet>,
    pub redaction: RedactionSummary,
}

pub struct SafeImageAsset {
    pub file_name: String,
    pub pixels: RgbaImage,
    pub redaction_state: RedactionState,
    pub derived_from_original: bool,
}

pub struct ActionGuideIssueAssets {
    pub guide: Guide,
    pub store: FrameStore,
    pub region: CaptureRegion,
    pub capability: InputCapability,
    pub source_kind: InputSourceKind,
    pub include_gif: bool,
}

pub struct IssuePackExportResult {
    pub directory: PathBuf,
    pub markdown_path: PathBuf,
    pub manifest_path: PathBuf,
    pub zip_path: Option<PathBuf>,
    pub warnings: Vec<IssuePackWarning>,
}
```

The exact Rust names can change during implementation, but the behavior should
preserve these boundaries:

- The renderer receives a prepared model and does not inspect UI state.
- UI code is responsible for asking the user to review redactions before export.
- Export code writes only relative links in Markdown and manifest asset paths.
- Optional asset failures become warnings when the pack remains valid.

## Manifest

`manifest.json` uses `schema_version: 1`.

Required top-level fields:

```json
{
  "schema_version": 1,
  "created_at": "2026-07-04T15:30:00+08:00",
  "rollshot_version": "0.0.0-dev",
  "export_mode": "local_issue_pack",
  "platform": {
    "os": "linux",
    "arch": "x86_64"
  },
  "redaction": {
    "review_required": true,
    "review_completed": true,
    "result_workspace_images_are_flattened": true,
    "original_pixels_included": false,
    "redaction_count": 0
  },
  "assets": [
    { "kind": "issue_markdown", "path": "issue.md" },
    { "kind": "manifest", "path": "manifest.json" }
  ],
  "ocr": {
    "included": false,
    "snippet_count": 0
  },
  "warnings": []
}
```

The manifest must accurately list every file included in the exported folder or
ZIP. It must not claim that Rollshot found every sensitive region. If no
redactions exist, `redaction_count` is `0`, `review_completed` records the user
confirmation, and user-facing text says the image was reviewed, not that it is
sensitive-free. Action Guide keyframes must be listed as reviewed keyframe
assets, not redacted assets, unless they are produced by a future redaction
pipeline.

## Markdown

`issue.md` is the primary artifact. It uses relative links so the folder can be
moved or zipped without breaking references.

Screenshot or long-screenshot pack:

```markdown
# Bug Report

## Summary

[Write a short summary]

## Steps to reproduce

[Write the steps to reproduce]

## Actual result

The UI reached this state:

![](images/final-redacted.png)

## Expected result

[Write what should have happened]

## OCR snippets

- Example visible text

## Environment

- OS: Linux
- Rollshot version: 0.0.0-dev

## Attachments

- `manifest.json`
```

Action Guide pack:

```markdown
# Bug Report

## Summary

[Write a short summary]

## Steps to reproduce

1. Open Settings

   ![](action-guide/keyframes/001.png)

2. Click Save

   ![](action-guide/keyframes/002.png)

## Actual result

[Describe what happened]

## Expected result

[Write what should have happened]

## Attachments

- `action-guide/steps.md`
- `action-guide/session.json`
- `manifest.json`
```

Combined packs include both embedded Action Guide steps and the final image in
Actual result.

OCR snippets are optional. Missing OCR does not block export and should be
omitted rather than replaced with an error placeholder.

## Data Flow

Result Workspace flow:

1. User clicks `Export Issue Pack`.
2. App blocks export if Smart Redaction workbench has pending candidates, using
   the same policy as safe copy/save.
3. App opens a compact Issue Pack review panel.
4. User confirms redaction review.
5. User chooses folder export or ZIP export.
6. App builds a flattened safe image from the current document.
7. App writes the pack into a temporary sibling directory.
8. App swaps the temporary directory into place only after required files have
   been written.
9. If ZIP was selected, app packages the completed folder.
10. App shows success with the exported path, or a persistent error.

Action Guide flow:

1. User reviews, renames, deletes, or changes keyframes in Timeline Workspace.
2. User clicks `Export Issue Pack`.
3. App opens the same compact review panel, adapted for Action Guide assets.
4. User confirms evidence review. For Action Guide-only packs, this confirms
   that reviewed keyframes are acceptable to share; it does not claim keyframes
   were automatically redacted.
5. App writes the pack into a temporary sibling directory.
6. App calls the existing guide export path for `action-guide/steps.md`,
   `session.json`, and `keyframes/*.png`.
7. App attempts `guide.gif` if requested.
8. App renders `issue.md` with embedded steps and relative image links.
9. App writes `manifest.json`, swaps the temp directory into place, and then
   optionally writes the ZIP.

The implementation should avoid writing partial final output on blocking
failures. Temporary directories may be removed best-effort after errors.

## Failure Semantics

Terminal states:

- Exported.
- Exported with warnings.
- Cancelled by user.
- Blocked because redaction review was not confirmed.
- Failed to write required files.

Blocking failures:

- User cancels before write.
- Redaction review is not confirmed.
- Output folder cannot be created.
- `issue.md` cannot be written.
- `manifest.json` cannot be written.
- Required final image cannot be written for Result Workspace export.
- Action Guide-only export has no reviewed steps/keyframes.

Non-blocking optional failures:

- OCR unavailable, empty, or feature-disabled.
- GIF export fails.
- Optional Action Guide attachment fails when the pack still has a valid final
  screenshot and `issue.md`.

Warnings are surfaced in the UI and recorded in `manifest.json`.

## Safety Policy

The export must follow secure sharing semantics:

- Result Workspace screenshots are flattened outputs.
- Retained original capture files are not included by default.
- Result Workspace safe image export must not overwrite the retained original.
- The Issue Pack review panel must require an explicit redaction review
  confirmation before writing files.
- If redactions exist, the UI says Result Workspace images will be flattened and
  retained originals will not be included.
- If no redactions exist, the UI says: "No redactions are currently applied.
  Review the image before sharing."
- No UI copy, manifest field, or Markdown text claims that all sensitive
  information has been detected.

Action Guide keyframes are reviewed evidence images. The first release does not
attempt to redact Action Guide keyframes automatically. The review confirmation
and manifest make that explicit.

## UI

Result Workspace toolbar:

- Add `Export Issue Pack` near existing copy/save/reveal controls.
- Reuse existing inline message behavior for success, warning, and error states.

Action Guide Review toolbar:

- Add `Export Issue Pack` beside `Export Guide` and `Export GIF`.

Review panel:

```text
Issue Pack Export

Included:
  issue.md
  manifest.json
  final flattened screenshot
  3 Action Guide steps
  OCR snippets

Safety:
  Result Workspace images will be flattened.
  Retained originals will not be included.
  Review redactions before export.

[Review Redactions] [Export Folder] [Export ZIP] [Cancel]
```

For Action Guide-only packs, replace final screenshot text with reviewed
keyframes and clarify that keyframes should be checked before sharing and are
not automatically redacted.

No first-release wizard is added. Existing Result Workspace and Timeline
Workspace remain the editing/review surfaces; Issue Pack is the final packaging
action.

## Testing

Core tests in `rollshot-app::issue_pack`:

- Deterministic folder names and relative Markdown links.
- `issue.md` renders screenshot-only, Action Guide-only, and combined packs.
- `manifest.json` lists every included asset accurately.
- Redaction status is represented without overclaiming safety.
- Missing OCR does not block export.
- Optional GIF failure does not destroy a valid pack.
- Folder export rolls back temporary output on blocking failures.
- ZIP contains the same relative layout as folder export.

Integration/update tests:

- Result Workspace export blocks pending Smart Redaction candidates, matching
  existing copy/save behavior.
- Result Workspace Issue Pack image uses flattened pixels.
- Action Guide Issue Pack export uses reviewed titles and selected keyframes.
- Cancel path writes nothing.

Verification commands:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-action
rtk cargo fmt --check
```

Run clippy when implementation touches shared UI/update paths enough to justify
the extra cost:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

## Open Follow-Up

Future work may move the pure renderer and manifest model into a reusable crate
if CLI export becomes important. That is intentionally not part of the first
release.
