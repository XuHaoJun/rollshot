# Rollshot Local Issue Pack

**Date:** 2026-07-04  
**Status:** Idea recommended for MVP validation; not an implementation spec  
**Related:**

- `docs/feature-discovery/2026-06-14-action-guide-capture-roadmap.md`
- `docs/ideas/2026-06-14-smart-redaction-presets.md`
- `docs/ideas/2026-06-22-smart-redaction-auto-detection-architecture.md`
- `docs/superpowers/plans/2026-06-13-secure-redaction-sharing.md`

## Summary

Rollshot should consider a new workflow-level feature: **Local Issue Pack**.

The idea is to turn a screenshot, long screenshot, or Action Guide session into a local, reviewable, redaction-gated bug evidence bundle that can be pasted into GitHub Issues, Jira, Linear, Slack, email, or an internal tracker without requiring Rollshot to become a cloud bug-reporting SaaS.

```text
capture / long capture / Action Guide
        ↓
review steps + screenshot evidence
        ↓
OCR snippets and optional annotations
        ↓
redaction review gate
        ↓
Markdown + images + manifest.json + optional GIF / ZIP
```

The product thesis is:

> Rollshot should not merely capture screen pixels. It should capture reproducible visual context.

This is worth exploring because Rollshot already has several pieces that can be composed into this workflow: Action Guide export, keyframes, Markdown, long screenshot, OCR direction, opaque redaction, Smart Redaction presets, and local-first safety semantics.

## Recommendation

Build a narrow MVP.

Do **not** build a full Jam / Marker.io competitor first. Do **not** start with cloud issue pages, Jira/GitHub API writes, browser console logs, network logs, or session replay infrastructure.

Instead, build:

```text
Local Issue Pack = local Markdown/ZIP artifact + redacted screenshots + steps to reproduce
```

Recommended priority: **8/10 after Action Guide MVP and redaction export are stable**.

This should be treated as Rollshot's first workflow-level product feature, not as another annotation tool.

## Competitive Research

### Traditional screenshot tools are already crowded

| Product | Relevant capabilities | Implication for Rollshot |
|---|---|---|
| CleanShot X | Screenshot, screen recording, annotation, cloud upload, scrolling capture | Screenshot + annotation + OCR/cloud sharing alone is not differentiated. |
| Snagit | Screenshot, scrolling capture, screen recording, arrows/text/highlights, how-to guides, feedback, process documentation | Snagit already owns the polished “capture and explain” category. |
| ShareX | Free/open-source screen capture, recording, file sharing, GIF recording, many upload destinations | Power-user capture/export is already mature on Windows. |
| Shottr | Lightweight Mac screenshots, scrolling screenshots, pixelate/remove sensitive information, OCR/QR, combine screenshots | Local screenshot utilities already include OCR and redaction-like tools. |
| Zight | Screenshot, video, webcam, GIF, annotation, editing, instant sharing links | Visual async communication is already a mature SaaS category. |

Conclusion: **Rollshot should not compete as “another screenshot + annotation + OCR + share link” product.**

### Bug-reporting tools already own cloud debugging context

| Product | Relevant capabilities | Implication for Rollshot |
|---|---|---|
| Jam | One-click bug reports, device/browser metadata, console logs, network logs, repro steps, backend tracing | Competing on browser technical context would require a much broader product surface. |
| Marker.io | Website feedback, annotated screenshots, browser data, console logs, session replays, Jira/Trello/Asana/GitHub integrations | Website feedback + issue tracker integration is already a mature vertical. |
| Bugzy / similar tools | Automated technical context such as console, network, browser, OS, environment | Technical browser-debug context is not Rollshot's strongest initial wedge. |
| AppRemark / mobile in-app feedback tools | Shake-to-capture, annotated screenshots, forms, device metadata, dashboard | Mobile SDK-style bug reporting is a different product category. |

Conclusion: **Rollshot should not start by chasing cloud bug-reporting platforms.** The winning path is not “Jam, but worse and local.”

### Step-by-step documentation tools validate the Action Guide direction

| Product | Relevant capabilities | Implication for Rollshot |
|---|---|---|
| Scribe | Automatically generated step-by-step guides with screenshots, text, and cursor clicks | Step-based visual documentation is a proven workflow format. |
| Tango | Click through workflows and automatically create how-to guides with screenshots, highlights, and descriptions | Action Guide is aligned with a known user need, but Rollshot can specialize it for bug evidence instead of SOP/training docs. |

Conclusion: **Action Guide can become “steps to reproduce,” not just “how-to documentation.”** This is a strong bridge from existing Rollshot direction to bug-reporting value.

### Small but important signal: Markdown/ZIP bug report apps exist

DebugShare and Bug Report Recorder on iOS are especially relevant because they validate a non-cloud, artifact-based bug-reporting workflow:

- DebugShare describes screenshot memo management for developers and QA, with OCR auto-tagging and Markdown/ZIP export.
- Bug Report Recorder describes local screen recording, auto-generated steps to reproduce, device metadata, and Markdown/HTML/archive export, with no account or cloud upload.

Conclusion: **Markdown/ZIP bug evidence packs are a real workflow**, not just a speculative Rollshot idea. However, current examples are mobile-centric; Rollshot can differentiate with desktop-cross-app capture, long screenshot, Action Guide keyframes, and redaction review.

## Differentiated Positioning

The strongest positioning is:

> Rollshot Local Issue Pack is a local-first evidence bundle for desktop bugs, with redaction review before anything leaves the machine.

This gives Rollshot a narrow but defensible wedge:

1. **Local-first artifact**  
   Most bug-report products push toward cloud issue pages, hosted recordings, team inboxes, or tracker integrations. Rollshot can start with a filesystem artifact that works in any tool.

2. **Desktop-cross-app capture**  
   Browser feedback tools are strong for websites, but less natural for native apps, Electron apps, IDEs, terminals, OS settings, design tools, and internal desktop workflows.

3. **Redaction-gated export**  
   Traditional screenshot tools provide blur/redaction tools, but Rollshot can make redaction review a required export stage for Issue Pack generation. This fits the existing secure redaction direction.

4. **Action Guide becomes steps to reproduce**  
   The existing Action Guide direction can be repurposed into bug-report evidence. Instead of asking users to manually write steps, Rollshot can produce an editable draft.

5. **Markdown-native output**  
   A local `issue.md` with relative image links is simple, inspectable, versionable, and works in GitHub, GitLab, Linear comments, Slack snippets, email, and internal docs.

## Product Definition

### User Story

As a developer, QA engineer, designer, or power user, I want to reproduce a visual bug once and export a clean evidence bundle, so that I can share the issue without manually collecting screenshots, writing every step, or leaking sensitive information.

### First-Release User Flow

```text
1. User starts Record Action or captures a final screenshot / long screenshot.
2. User reproduces the issue.
3. Rollshot generates candidate steps and keyframes when an Action Guide exists.
4. Rollshot opens a review workspace.
5. User edits step titles, deletes wrong steps, and chooses final evidence image.
6. Rollshot shows OCR snippets if available.
7. Rollshot requires redaction review before export.
8. User exports a local folder or ZIP.
9. User pastes issue.md into GitHub/Jira/Linear or attaches the ZIP.
```

### Example Export

```text
rollshot-issue-pack-2026-07-04-1530/
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

### Example `issue.md`

```markdown
# Bug Report

## Summary

[Write a short summary]

## Steps to reproduce

1. Open Settings  
   ![](action-guide/keyframes/001.png)

2. Click Save  
   ![](action-guide/keyframes/002.png)

3. Error message appears  
   ![](action-guide/keyframes/003.png)

## Actual result

The UI reached this state:

![](images/final-redacted.png)

## Expected result

[Write what should have happened]

## OCR snippets

- Failed to save settings
- Network request returned 500

## Environment

- OS: [optional]
- App/window: [optional]
- Rollshot version: [auto-filled if available]

## Attachments

- `action-guide/guide.gif`
- `action-guide/session.json`
- `manifest.json`
```

## Product Decisions

- The first release exports a **local folder or ZIP**, not a hosted page.
- `issue.md` is the primary artifact. GIF is optional and secondary.
- Redacted images are the default shareable images.
- Original unredacted pixels are not included by default.
- If originals are retained locally, `manifest.json` must clearly distinguish local source assets from exported safe assets.
- The user must review redactions before Issue Pack export.
- OCR snippets are optional. Missing OCR must not block export.
- Action Guide steps are optional. A plain screenshot or long screenshot can still become an Issue Pack.
- LLM generation is deferred. The MVP can use deterministic templates and user-editable placeholders.
- Any future AI mode must be visibly distinct from local-only mode.

## First-Release Scope

Include:

- `Export Issue Pack` command.
- Folder export.
- Optional ZIP export if trivial after folder export.
- Deterministic `issue.md` template.
- `manifest.json` with source metadata, Rollshot version, export timestamp, asset list, and redaction status.
- Final redacted screenshot export.
- Reuse of Action Guide `steps.md`, `keyframes/*.png`, and `session.json` when available.
- Optional `guide.gif` when Action Guide GIF export exists.
- OCR snippets when OCR document data exists.
- Clear safe/original language borrowed from the secure redaction sharing policy.

Defer:

- GitHub/Jira/Linear API integration.
- Hosted cloud issue pages.
- Browser console logs.
- Network request capture.
- DOM capture.
- Session replay backend.
- Team inbox.
- Automatic issue creation.
- AI-generated full bug report narrative.
- Claiming that all sensitive information has been found.

## Architecture Direction

Implement this first as an app-level composition layer, not a new core engine.

Suggested file:

```text
crates/rollshot-app/src/issue_pack.rs
```

Reason: Issue Pack needs to coordinate existing app-level state:

- Result Workspace document.
- Safe redaction/export policy.
- Action Guide session export.
- OCR snippets.
- File save dialogs and path policy.
- User-facing export messages.

Later, if CLI export becomes important, the pure export model can be moved into a small crate.

### Data Model Sketch

```rust
pub struct IssuePackInput {
    pub title: Option<String>,
    pub created_at: Timestamp,
    pub rollshot_version: String,
    pub platform: PlatformInfo,
    pub final_image: Option<SafeImageAsset>,
    pub action_guide: Option<ActionGuideIssueAssets>,
    pub ocr_snippets: Vec<OcrSnippet>,
    pub redaction_summary: RedactionSummary,
}

pub struct ActionGuideIssueAssets {
    pub steps_markdown: PathBuf,
    pub session_json: PathBuf,
    pub keyframes_dir: PathBuf,
    pub gif: Option<PathBuf>,
}

pub struct SafeImageAsset {
    pub path: PathBuf,
    pub redaction_state: RedactionState,
    pub derived_from_original: bool,
}

pub struct IssuePackExportResult {
    pub directory: PathBuf,
    pub markdown_path: PathBuf,
    pub manifest_path: PathBuf,
}
```

### Manifest Sketch

```json
{
  "schema_version": 1,
  "created_at": "2026-07-04T15:30:00+08:00",
  "rollshot_version": "0.0.0-dev",
  "export_mode": "local_issue_pack",
  "redaction": {
    "review_required": true,
    "review_completed": true,
    "exported_images_are_flattened": true,
    "original_pixels_included": false
  },
  "assets": [
    { "kind": "issue_markdown", "path": "issue.md" },
    { "kind": "final_redacted_image", "path": "images/final-redacted.png" },
    { "kind": "action_steps", "path": "action-guide/steps.md" },
    { "kind": "keyframe", "path": "action-guide/keyframes/001.png" }
  ],
  "ocr": {
    "included": true,
    "snippet_count": 2
  }
}
```

## UI Direction

Minimal UI is enough:

```text
Result Workspace
  [Copy Safe Image] [Save Safe Image As] [Export Issue Pack]

Action Guide Review
  [Export Guide] [Export GIF] [Export Issue Pack]
```

Before export, show a compact review panel:

```text
Issue Pack Export

Included:
  ✓ issue.md
  ✓ final redacted screenshot
  ✓ 3 Action Guide steps
  ✓ OCR snippets
  ✓ manifest.json

Safety:
  ✓ Redacted export image will be flattened
  ✓ Unredacted original will not be included
  ! Review redactions before export

[Review Redactions] [Export Local Folder] [Cancel]
```

If no redactions exist, the safety section should not imply that the screenshot is sensitive-free. It should say:

```text
No redactions are currently applied. Review the image before sharing.
```

## Failure Semantics

Issue Pack export should end in explicit states:

- Exported.
- Cancelled by user.
- Blocked because redaction review is required.
- Failed to write files.
- Failed to include optional Action Guide assets.
- Failed to include optional OCR snippets.

Failure to include optional assets should not destroy the whole export unless the primary `issue.md` or final image cannot be written.

No state should claim that all sensitive information has been found.

## Why This Is Worth Doing

This idea is worth doing because it composes existing Rollshot directions into a sharper product promise:

```text
Current Rollshot value:
  capture screenshots and edit/share them safely

Stronger Rollshot value:
  capture the visual context needed to explain and reproduce a problem
```

It is especially strong for:

- Desktop app bug reports.
- Native app UI regressions.
- Electron/internal-tool bugs.
- CLI/terminal/IDE visual bugs.
- Long scrolling UI bugs.
- Open-source issue reports where Markdown is the default collaboration format.
- Teams that cannot upload recordings/screenshots to third-party cloud tools.

The feature also creates a natural reason for several existing Rollshot investments to coexist:

- Action Guide supplies steps.
- Long screenshot supplies final state context.
- OCR supplies error text snippets.
- Smart Redaction supplies candidate sensitive regions.
- Secure sharing supplies safe export semantics.

## Why This Might Not Be Worth Doing

Do not pursue this if the intended product direction is only a lightweight personal screenshot utility.

This feature adds workflow complexity and could distract from capture reliability if started too early. It also risks scope creep into bug-reporting SaaS territory.

The idea becomes unattractive if the scope expands to:

- Team dashboards.
- Hosted issue pages.
- Tracker sync.
- Browser log SDKs.
- Video session replay.
- AI debugging agents.

Those are valid products, but they are not Rollshot's likely best wedge.

## MVP Acceptance Criteria

- User can export an Issue Pack from a single screenshot or long screenshot.
- User can export an Issue Pack from an Action Guide session.
- `issue.md` renders correctly with relative image links.
- Export never includes unredacted original pixels unless an explicit future advanced option is added.
- Exported redacted image is flattened.
- `manifest.json` accurately lists included files.
- Missing OCR does not block export.
- Missing Action Guide does not block export.
- The user can cancel before writing files.
- Exported folder is understandable without opening Rollshot.

## Suggested Implementation Order

1. Add a pure `issue_pack` export model and Markdown renderer.
2. Add tests for deterministic paths and Markdown links.
3. Export from a static fixture into a temp directory.
4. Wire final safe screenshot export through existing Result Workspace policy.
5. Generate `manifest.json`.
6. Add `Export Issue Pack` button to Result Workspace.
7. Reuse Action Guide assets when a session exists.
8. Add optional ZIP packaging.
9. Add OCR snippets when the OCR document model is ready.
10. Add review UI polish.

## Open Decisions

1. Should the first export be folder-only, ZIP-only, or both?
2. Should `issue.md` use GitHub-flavored Markdown defaults?
3. Should default language follow OS locale or always English?
4. Should Action Guide steps be embedded directly into `issue.md`, or linked via `action-guide/steps.md`?
5. Should the final image be required, or can an Action Guide-only pack exist?
6. Should Rollshot include non-sensitive platform metadata by default?
7. Should Issue Pack export live only in GUI first, or should there be a CLI export path for Action Guide sessions?

## References

Competitive research sources reviewed on 2026-07-04:

- CleanShot X — https://cleanshot.com/
- Snagit — https://www.techsmith.com/snagit/
- ShareX — https://getsharex.com/
- Shottr — https://shottr.cc/
- Zight — https://zight.com/
- Jam — https://jam.dev/
- Marker.io features — https://marker.io/features
- Marker.io issue page docs — https://help.marker.io/en/articles/6559060-issue-page
- Marker.io web app integration docs — https://help.marker.io/en/articles/5546520-how-to-integrate-marker-io-into-your-web-app
- Scribe 101 — https://support.scribehow.com/hc/en-us/articles/8951146003741-New-User-Guide-Scribe-101
- Scribe step-by-step guide generator — https://scribe.com/tools/step-by-step-guide-generator
- Tango — https://www.tango.ai/
- DebugShare App Store listing — https://apps.apple.com/ch/app/debugshare-screenshot-memo/id6767061742?l=en-GB
- Bug Report Recorder App Store listing — https://apps.apple.com/tw/app/bug-report-recorder/id6758620545
- ImageR: Enhancing Bug Report Clarity by Screenshots — https://arxiv.org/abs/2505.01925
- Can You Mimic Me? Exploring the Use of Android Record & Replay Tools in Debugging — https://arxiv.org/abs/2504.20237
