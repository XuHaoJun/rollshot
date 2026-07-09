# Rollshot PRD: Action Guide Storyboard & Agent Annotation Umbrella Spec

- **Status:** Draft / implementation-oriented
- **Date:** 2026-07-09
- **Author:** ChatGPT
- **Area:** Action Guide, Storyboard Export, Issue Pack, Annotation, Agent-assisted review
- **Primary code areas:** `rollshot-action`, `rollshot-app`, `rollshot-image-document`, `rollshot-edit-proposal`
- **Related docs:**
  - `docs/ideas/2026-07-07-rollshot-storyboard-export-prd.md`
  - `docs/superpowers/plans/2026-07-07-storyboard-export.md`
  - `docs/superpowers/specs/2026-07-04-local-issue-pack-design.md`
  - `docs/superpowers/specs/2026-06-11-long-shot-callouts-image-document-design.md`
  - `docs/superpowers/plans/2026-06-20-edit-proposal-foundation.md`

---

## 1. Summary

Rollshot already has **Action Guide** review and a first Storyboard Export implementation. The next product opportunity is to turn Action Guide output into a better communication artifact:

> Reviewed workflow steps become a share-ready visual storyboard, optionally enriched with captions, lightweight annotations, and agent-proposed explanations.

This PRD is an **umbrella spec**. It intentionally covers multiple related capabilities, but it is biased toward fast incremental implementation. The recommended path is not to build a full annotation editor immediately. Instead:

1. **Ship Storyboard as a first-class share artifact**.
2. **Include Storyboard in Issue Pack**.
3. **Add preview and simple layout controls**.
4. **Add per-step captions**.
5. **Add lightweight per-step annotation by reusing existing `ImageDocument` infrastructure**.
6. **Allow agent proposals for captions and annotations after manual UX exists**.

---

## 2. Naming and terminology

### Recommended product name

**Action Guide Storyboard**

### Recommended UI labels

- `Export Storyboard`
- `Copy Storyboard`
- `Preview Storyboard`
- `Include Storyboard in Bug Report`
- `Suggest Captions`
- `Suggest Callouts`

### Recommended Chinese terms

- **Action Guide Storyboard**: 操作流程故事板
- **Storyboard Export**: 故事板匯出 / 流程故事板匯出
- **Visual Step Summary**: 視覺步驟摘要
- **Step Card**: 步驟卡片
- **Keyframe Contact Sheet**: 關鍵影格縮圖表 / 聯絡表

### Why not call it “大圖” in product UI?

「大圖」是 useful shorthand, but it is not precise. It does not explain whether the output is a collage, long screenshot, stitched image, or formatted workflow summary.

### Why avoid “stitching” here?

Rollshot already has scrolling screenshot stitching. Calling this feature “keyframe stitching” can confuse two different concepts:

- **Scrolling capture stitching**: reconstructs one continuous page/surface from overlapping frames.
- **Action Guide Storyboard**: lays out reviewed keyframes as separate step cards in one visual summary.

### When to use “contact sheet”?

Use **contact sheet** only for grid or compact layouts, where many thumbnails are arranged for scanning. For the primary user flow, **Storyboard** is better because the artifact is chronological and explanatory.

---

## 3. Current state from `rollshot-main.zip`

### 3.1 Existing Storyboard Export V1

The main zip already includes a headless storyboard renderer:

- `crates/rollshot-action/src/storyboard.rs`
- Public API exports in `crates/rollshot-action/src/lib.rs`
- Timeline Workspace button and update wiring:
  - `crates/rollshot-app/src/timeline_workspace/view.rs`
  - `crates/rollshot-app/src/timeline_workspace/update.rs`

Current V1 behavior:

- Exports one PNG.
- Uses reviewed `Guide` steps and retained `FrameStore` keyframes.
- Renders vertical single-column cards.
- Includes `Step N - title` labels when `show_titles = true`.
- Defaults include `max_width = 1200`, `max_canvas_pixels = 24_000_000`, card padding, spacing, and title display.
- Keeps Timeline Workspace open after Storyboard/GIF/MP4-style exports.

### 3.2 Current Action Guide editing capabilities

Action Guide review currently supports:

- Select step.
- Edit step title.
- Delete step.
- Replace selected step keyframe from nearby retained frames.
- Export Guide / GIF / Storyboard / MP4 / Bug Report.

It does **not** currently support:

- Per-frame annotation editing.
- Per-step caption separate from title.
- Storyboard preview before export.
- Layout mode selection.
- Agent-generated annotation acceptance inside Action Guide.

Relevant model:

- `crates/rollshot-action/src/models.rs`
  - `GuideStep { index, title, kind, reason, at_ms, keyframe, nearby, source }`
- `crates/rollshot-action/src/guide.rs`
  - `rename`, `delete`, `replace_keyframe`

### 3.3 Existing annotation infrastructure

Rollshot already has a non-destructive annotation graph in `rollshot-image-document`:

- `NumberCallout`
- `TextNote`
- `OpaqueRedaction`
- `ImageDocument::flatten()`
- Undo/redo history.
- Deterministic rasterization.

Relevant files:

- `crates/rollshot-image-document/src/annotation.rs`
- `crates/rollshot-image-document/src/document.rs`
- `crates/rollshot-image-document/src/flatten.rs`
- `crates/rollshot-image-document/src/edit_op.rs`

### 3.4 Existing agent proposal infrastructure

Rollshot already has proposal types that can represent annotation changes:

- `ProposedEdit::AddRedaction`
- `ProposedEdit::AddTextNote`
- `ProposedEdit::AddNumberCallout`
- `ProvenanceSource::Agent { run_id }`
- `ProposedEdit::to_edit_op()`

Relevant file:

- `crates/rollshot-edit-proposal/src/proposal.rs`

### 3.5 Current Issue Pack capabilities

Local Issue Pack already supports Action Guide export assets:

- `issue.md`
- `manifest.json`
- `action-guide/steps.md`
- `action-guide/session.json`
- `action-guide/keyframes/*.png`
- Optional `action-guide/guide.gif`

It does **not** currently include:

- `action-guide/storyboard.png`
- Storyboard asset entry in manifest.
- Storyboard preview/link in `issue.md`.

Relevant file:

- `crates/rollshot-app/src/issue_pack.rs`

---

## 4. Problem statement

Rollshot can already generate Action Guide artifacts, but the highest-frequency communication use case is still under-served:

> “I want to show a few key steps to another person immediately, with enough context that they understand what to look at.”

Current exports each solve only part of this:

| Artifact | Strength | Weakness |
|---|---|---|
| Guide folder | Complete and structured | Too heavy for quick Slack / Linear / GitHub comment sharing |
| GIF | Easy motion preview | Hard to pause/read, less suitable for detailed bug reports |
| MP4 | Higher-quality motion summary | Requires playback; heavier artifact |
| Storyboard V1 | Static, readable, chat-friendly | No preview, no Issue Pack integration, no captions beyond title, no annotation layer |
| Keyframes | Accurate evidence | Too many separate files; no visual hierarchy |

The product gap is not “can Rollshot export one big image?” anymore. The gap is:

> Can Rollshot produce a reviewable, explainable, share-ready workflow artifact without forcing users into a full image editor?

---

## 5. Target users

### 5.1 Developer / engineer

Wants to show a bug reproduction flow, UI state transition, or tool workflow to a teammate quickly.

### 5.2 QA

Wants to attach clear reproduction evidence to a bug report without manually stitching screenshots.

### 5.3 PM / designer

Wants to explain a UX flow, onboarding path, edge case, or design feedback with concrete screens.

### 5.4 Support / internal ops

Wants to produce lightweight how-to or troubleshooting visuals.

### 5.5 AI-assisted workflow user

Wants the tool to infer what matters in each step, then review/approve before sharing.

---

## 6. Product principles

1. **Reviewed steps are the source of truth.** Storyboard must reflect what the user kept, renamed, deleted, or keyframe-replaced.
2. **Do not turn Action Guide review into a full design tool too early.** Keep editing lightweight.
3. **Manual first, agent second.** Agent suggestions should map onto user-reviewable primitives.
4. **Shareability beats configurability.** Default output should be good without options.
5. **Privacy and evidence review stay explicit.** Action Guide keyframes are reviewed evidence images, not automatically redacted outputs.
6. **Reuse existing annotation primitives.** Avoid creating a second annotation model unless existing primitives are insufficient.

---

## 7. Goals and non-goals

### Goals

1. Make Storyboard a first-class Action Guide export artifact.
2. Let users share Action Guide steps as a single visual summary.
3. Add Issue Pack integration so bug reports have a visual summary image.
4. Add preview so users know what will be exported.
5. Add captions and lightweight annotations without overbuilding.
6. Enable agent-suggested captions/callouts through reviewable proposals.

### Non-goals for the umbrella MVP

- No hosted cloud sharing.
- No tracker API integration.
- No full Figma-like editor.
- No arbitrary drag-and-drop storyboard layout.
- No automatic redaction claim for Action Guide keyframes.
- No PDF/HTML export in the first implementation wave.
- No agent action that silently modifies user evidence without review.

---

## 8. Opportunity framing using PM discovery structure

The referenced PM Skills product-discovery workflow for existing products emphasizes multi-perspective ideation across PM, Designer, and Engineer viewpoints. Applying that here:

### PM view

Make Storyboard the fastest path from recorded workflow to shareable artifact.

### Designer view

Make each step visually understandable: title, caption, keyframe, optional highlight/callout.

### Engineer view

Reuse the existing `Guide`, `FrameStore`, `ImageDocument`, and `ProposedEdit` systems instead of creating parallel pipelines.

---

## 9. Recommended phased scope

## Phase P0 — Baseline already exists: Storyboard Export V1

**Status:** Implemented in main zip.

### Current scope

- Timeline button: `Export Storyboard`.
- Save dialog for `.png`.
- Single-column vertical PNG renderer.
- Step label + current step title.
- Keyframe scaling and canvas size guard.
- Typed `StoryboardError`.

### Keep as foundation

Do not remove this. Treat it as the rendering primitive for later phases.

---

## Phase P1 — Fast implementation: Storyboard as first-class Issue Pack artifact

**Priority:** Highest quick win.

### User story

As a user exporting a bug report from Action Guide, I want the issue pack to include a single visual summary image so the recipient can understand the reproduction flow before opening individual keyframes.

### Product behavior

When exporting an Action Guide Issue Pack:

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
    guide.gif              optional
```

### `issue.md` behavior

Under “Steps to reproduce”, include the storyboard before individual step images or in an “Overview” block:

```md
## Steps to reproduce

Overview:

![](action-guide/storyboard.png)

1. Open Settings

   ![](action-guide/keyframes/001.png)

2. Click Save

   ![](action-guide/keyframes/002.png)
```

### Manifest behavior

Add asset kind:

```json
{
  "kind": "action_storyboard",
  "path": "action-guide/storyboard.png"
}
```

### Default rule

- Include Storyboard by default when Action Guide has at least one reviewed step.
- If Storyboard export fails but Guide assets are valid, export Issue Pack with a warning.

### Warning code

```text
storyboard_export_failed
```

### Acceptance criteria

- Action Guide-only Issue Pack includes `action-guide/storyboard.png`.
- Combined screenshot + Action Guide Issue Pack includes `action-guide/storyboard.png`.
- `issue.md` references the storyboard via a relative path.
- `manifest.json` includes `action_storyboard` only when the file exists.
- Failure to generate Storyboard does not block Issue Pack if keyframes and `steps.md` remain valid.
- Warning is visible in result banner and manifest warnings.

### Implementation notes

Modify:

- `crates/rollshot-app/src/issue_pack.rs`

Potential changes:

```rust
pub(crate) struct ActionGuideIssueAssets {
    pub steps: Vec<IssuePackStep>,
    pub include_gif: bool,
    pub include_storyboard: bool,
}
```

or keep `include_storyboard` internal and always true for now.

In `build_folder(...)`, after `export_guide(...)` succeeds:

```rust
let storyboard_path = tmp_dir.join("action-guide/storyboard.png");
if let Err(error) = rollshot_action::export_storyboard(
    action.guide,
    action.store,
    rollshot_action::StoryboardOptions::default(),
    &storyboard_path,
) {
    warnings.push(IssuePackWarning {
        code: "storyboard_export_failed".to_string(),
        message: format!("Storyboard export failed: {error}"),
    });
}
```

Then detect existence before rendering manifest assets.

### Why this first?

It reuses existing exporter and Issue Pack architecture. It creates immediate user value without building new UI.

---

## Phase P2 — Fast/medium implementation: Storyboard preview before export

**Priority:** High, after P1.

### User story

As a user reviewing an Action Guide, I want to preview the storyboard before export so I can decide whether to delete steps, rename steps, or choose better keyframes.

### Product behavior

Add a `Preview Storyboard` or preview panel in Timeline Workspace.

Two possible UX options:

### Option A: Modal preview

- Button: `Preview Storyboard`.
- Modal shows generated preview image scaled to fit.
- Actions:
  - `Export PNG`
  - `Copy Image` if supported
  - `Close`

### Option B: Right-side tab/panel

- Timeline detail panel can toggle between:
  - `Step`
  - `Storyboard`
- Storyboard preview updates when step title/keyframe/order changes.

### Recommended quick implementation

Use **Option A: modal preview** first.

Why:

- Avoids restructuring Timeline Workspace layout.
- Keeps preview generation explicit.
- Less risk of performance regressions.

### Technical approach

Add an in-memory render function to `rollshot-action`:

```rust
pub fn render_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError>
```

Where:

```rust
pub struct StoryboardRenderResult {
    pub image: RgbaImage,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}
```

Then `export_storyboard(...)` becomes:

```rust
let rendered = render_storyboard(guide, store, opts)?;
write_png_atomic(out_path, &rendered.image)?;
```

### Preview optimization

For preview, use smaller options:

```rust
StoryboardOptions {
    max_width: 800,
    max_canvas_pixels: 12_000_000,
    ..Default::default()
}
```

### Acceptance criteria

- Preview shows the same step order/titles/keyframes as export.
- Preview does not mutate guide state.
- Empty guide shows a clear empty state.
- Missing keyframe shows recoverable error.
- Preview generation does not close workspace.
- After renaming/replacing a keyframe, re-opening preview reflects changes.

### Deferred

- Truly live preview on every edit.
- Layout options.
- Preview caching.

---

## Phase P3 — Fast/medium implementation: Step captions separate from titles

**Priority:** Medium-high.

### Problem

`GuideStep.title` is currently both:

1. A short step label used in the timeline list.
2. The only text rendered into Storyboard.

That forces a tradeoff:

- Short titles are good for navigation but under-explain the step.
- Long titles explain better but clutter the timeline and card label.

### User story

As a user, I want to add a one-line caption to a step so the exported Storyboard explains what matters without making the timeline title too long.

### Product behavior

Each step may have:

- **Title**: short action label, shown in timeline and label.
- **Caption**: optional explanatory sentence, shown in Storyboard/Issue Pack.

Example:

```text
Title: Click Save
Caption: The settings dialog closes but the change is not persisted.
```

### UI behavior

In selected step detail panel:

```text
[ keyframe preview ]
Title
[ Click Save                       ]
Caption
[ The settings dialog closes but...]
[Delete step]
```

### Storyboard rendering

Card text hierarchy:

```text
Step 2 — Click Save
The settings dialog closes but the change is not persisted.
[ image ]
```

### Data model options

#### Option A: Add `caption: String` to `GuideStep`

Fastest for runtime model:

```rust
pub struct GuideStep {
    pub index: usize,
    pub title: String,
    pub caption: String,
    ...
}
```

Pros:

- Simple UI wiring.
- Simple export.

Cons:

- Requires touching core model and tests.
- Need manifest compatibility decisions.

#### Option B: Keep `GuideStep` unchanged and store presentation metadata in app state

```rust
pub struct StepPresentation {
    pub source: CandidateId,
    pub caption: String,
}
```

Pros:

- Less invasive to `rollshot-action`.
- Avoids changing core guide semantics too early.

Cons:

- Export APIs need an overlay/presentation input.
- Captions are app-owned unless persisted explicitly.

### Recommended implementation

For fast implementation, use **Option A** if Action Guide sessions are not yet imported from older persisted JSON as a stable public format.

If compatibility matters, use Option B.

Given current `export.rs` serializes `SessionManifest` rather than serializing `GuideStep` directly, adding a caption to exported manifest can be controlled manually and made backward-compatible by adding an optional field:

```rust
pub struct ManifestStep {
    pub index: usize,
    pub title: String,
    pub caption: Option<String>,
    ...
}
```

### Acceptance criteria

- User can edit a caption for the selected step.
- Caption appears in Storyboard export.
- Caption appears in Issue Pack `issue.md` when present.
- Empty caption is omitted from output.
- Timeline list remains compact and primarily title-based.
- Existing title rename behavior is unchanged.

---

## Phase P4 — Strategic but bounded: Per-step lightweight annotations

**Priority:** Strategic bet, not first quick win.

### Product stance

Do **not** build a full annotation editor inside Action Guide immediately.

Instead, support a small set of step-level visual edits that directly improve communication:

1. Number callout.
2. Text note.
3. Opaque redaction.
4. Highlight rectangle, if added later.

### User story

As a user, I want to mark the relevant part of a keyframe so the recipient knows what to look at in the Storyboard.

### Recommended UX

Add an `Annotate Step` action in the selected step detail panel.

Flow:

1. User selects a step.
2. User clicks `Annotate Step`.
3. Rollshot opens a lightweight annotation mode for that keyframe.
4. User adds callout/text/redaction.
5. User returns to Timeline Workspace.
6. Storyboard export uses the annotated keyframe image.

### Why separate annotation mode?

- Avoids cramming toolbars into Timeline Workspace.
- Reuses mental model from Result Workspace annotation.
- Keeps Timeline Workspace focused on step review.

### Data model proposal

Use a per-step annotation document keyed by stable `GuideStep.source`:

```rust
pub struct ActionGuideStepPresentation {
    pub source: CandidateId,
    pub keyframe: FrameId,
    pub caption: Option<String>,
    pub annotations: Vec<Annotation>,
}
```

For runtime/editor state:

```rust
pub struct ActionGuidePresentationState {
    pub steps: BTreeMap<CandidateId, ActionGuideStepPresentation>,
}
```

### Keyframe replacement behavior

Annotations are image-coordinate-dependent. If the user replaces a step keyframe:

Recommended V1 behavior:

- Keep caption.
- Mark annotations for that step as stale or clear them.
- Show a non-blocking banner:

```text
Step annotations were cleared because the keyframe changed.
```

Simpler MVP:

- Clear annotations on keyframe replacement.

### Rendering behavior

Storyboard renderer should support either raw keyframes or pre-flattened keyframes.

Fastest route:

- App prepares an `ImageDocument` per annotated step.
- Calls `flatten()`.
- Sends flattened images to a new storyboard input model.

Better route:

Add a more generic renderer API:

```rust
pub struct StoryboardStep<'a> {
    pub index: usize,
    pub title: &'a str,
    pub caption: Option<&'a str>,
    pub image: &'a RgbaImage,
}

pub fn render_storyboard_steps(
    steps: &[StoryboardStep<'_>],
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError>
```

Then existing `Guide + FrameStore` export becomes a thin adapter.

### Acceptance criteria

- User can add at least one number callout to a selected step.
- Exported Storyboard includes the flattened callout.
- Original retained keyframe pixels are not mutated.
- Annotation changes can be undone/redone inside annotation mode if reusing `ImageDocument`.
- Replacing the keyframe handles stale annotations safely.
- Issue Pack can include annotated Storyboard while still including original reviewed keyframes.

### Important privacy language

If annotations include redactions, the Storyboard may be redaction-flattened, but the Issue Pack may still include original keyframes unless those are also redacted. UI copy must not imply the entire Issue Pack is safe-redacted unless all included assets are safe.

---

## Phase P5 — Agent-assisted captions and annotation proposals

**Priority:** After manual caption/annotation primitives exist.

### Product stance

Agent output must be a proposal, not an automatic mutation.

### User story A: Caption suggestion

As a user, I want Rollshot to suggest clear captions for each step so I can quickly turn a recording into a readable guide.

### User story B: Annotation suggestion

As a user, I want Rollshot to suggest where to add a callout or redaction so I can review and accept the useful suggestions.

### Suggestion types

#### Caption proposal

```rust
pub enum ActionGuideProposal {
    UpdateStepTitle {
        source: CandidateId,
        title: String,
    },
    UpdateStepCaption {
        source: CandidateId,
        caption: String,
    },
}
```

#### Annotation proposal

Reuse `rollshot-edit-proposal` primitives where possible:

```rust
ProposedEdit::AddTextNote { position, text }
ProposedEdit::AddNumberCallout { tip, bubble }
ProposedEdit::AddRedaction { bounds }
```

Need wrapper to associate a proposal with an Action Guide step:

```rust
pub struct StepEditProposal {
    pub step_source: CandidateId,
    pub base_keyframe: FrameId,
    pub proposal: EditProposal,
}
```

### Agent UX

In Timeline Workspace:

- Button: `Suggest Captions`
- Optional later: `Suggest Callouts`
- Proposal panel:
  - Shows step-by-step suggestions.
  - `Accept`, `Reject`, `Accept All`.
  - Low confidence suggestions are collapsed or marked.

### Safety rules

- Agent must never silently redact or annotate without review.
- Redaction suggestions must be explicitly accepted.
- If base keyframe changed after proposal generation, proposal is stale and cannot be applied.
- Prompt/agent provenance should not be exported into user artifacts unless explicitly useful.

### Acceptance criteria

- Agent can propose captions for all steps.
- User can accept/reject each caption.
- Accepted captions appear in Storyboard and Issue Pack.
- If a step is deleted, its pending proposals disappear.
- If keyframe changes, annotation proposals for that step become stale.
- Agent proposals include `ProvenanceSource::Agent { run_id }` or equivalent.

---

## 10. Umbrella user flows

### Flow A: Fast share to Slack

1. User records Action Guide.
2. User reviews timeline, deletes noisy steps.
3. User renames steps.
4. User clicks `Export Storyboard` or `Copy Storyboard`.
5. User shares one image.

### Flow B: Bug report with visual summary

1. User records bug reproduction.
2. User reviews keyframes.
3. User clicks `Export Bug Report...`.
4. User confirms evidence review.
5. Issue Pack includes `storyboard.png`, `steps.md`, `session.json`, keyframes, optional GIF.

### Flow C: Preview before export

1. User records flow.
2. User clicks `Preview Storyboard`.
3. User sees too many steps.
4. User deletes noisy steps / changes titles.
5. User exports final Storyboard.

### Flow D: Caption-enhanced storyboard

1. User selects a step.
2. User edits title and caption.
3. Storyboard output shows concise label + explanatory caption.

### Flow E: Annotated storyboard

1. User selects a step.
2. User clicks `Annotate Step`.
3. User adds number callout/text note/redaction.
4. Storyboard output uses flattened annotated keyframe.

### Flow F: Agent-assisted communication

1. User clicks `Suggest Captions`.
2. Agent proposes titles/captions.
3. User accepts useful proposals.
4. User optionally clicks `Suggest Callouts`.
5. User accepts/rejects visual annotations.
6. User exports Storyboard or Issue Pack.

---

## 11. Functional requirements

### 11.1 Storyboard rendering

- FR-SB-1: Renderer must support the current `Guide + FrameStore` source.
- FR-SB-2: Renderer should expose an in-memory render path for preview.
- FR-SB-3: Renderer must support title and optional caption.
- FR-SB-4: Renderer must preserve step order from reviewed guide.
- FR-SB-5: Renderer must enforce canvas pixel limits.
- FR-SB-6: Renderer must fail recoverably on empty guide or missing keyframe.
- FR-SB-7: Renderer must not mutate guide or frame store.

### 11.2 Issue Pack integration

- FR-IP-1: Action Guide Issue Pack should include `action-guide/storyboard.png` by default.
- FR-IP-2: `issue.md` should include a relative link to the storyboard when present.
- FR-IP-3: `manifest.json` should include an `action_storyboard` asset when present.
- FR-IP-4: Storyboard export failure should become a warning if other required assets succeed.
- FR-IP-5: Evidence review copy must clarify that Action Guide keyframes are reviewed evidence, not automatically redacted.

### 11.3 Captions

- FR-CAP-1: User can edit a caption for each retained step.
- FR-CAP-2: Empty captions are omitted from output.
- FR-CAP-3: Caption must appear in Storyboard and Issue Pack Markdown.
- FR-CAP-4: Captions must be preserved when keyframe is replaced.
- FR-CAP-5: Captions must be deleted when the step is deleted.

### 11.4 Lightweight annotations

- FR-ANN-1: User can add at least one callout primitive to a step.
- FR-ANN-2: User can add text note and opaque redaction if reusing existing `ImageDocument` UI allows it.
- FR-ANN-3: Annotations are non-destructive and flatten only for export/preview.
- FR-ANN-4: Annotation coordinates are tied to a step keyframe.
- FR-ANN-5: Replacing keyframe must clear or invalidate existing annotations.
- FR-ANN-6: Storyboard preview/export must use flattened annotated images.

### 11.5 Agent proposals

- FR-AG-1: Agent suggestions must be reviewable.
- FR-AG-2: Caption suggestions can update title/caption only after user acceptance.
- FR-AG-3: Annotation suggestions must map to existing edit primitives.
- FR-AG-4: Stale proposals must be rejected if base step/keyframe changed.
- FR-AG-5: Agent provenance must be stored internally for proposal review/debugging.

---

## 12. Non-functional requirements

### NFR-1: Determinism

Same input guide, frame store, presentation metadata, and options should render the same Storyboard.

### NFR-2: Responsiveness

- Export of 3–8 steps should feel immediate.
- Preview should use smaller render options or caching.
- Avoid rendering full 1200px storyboard on every keystroke.

### NFR-3: Memory safety

- Enforce pixel limits.
- Avoid unbounded copies of all full-size keyframes.
- Flatten only needed annotated steps.

### NFR-4: Backward compatibility

- Existing Guide/GIF/MP4 exports must remain unchanged.
- Existing Issue Pack exports without Action Guide must remain unchanged.
- If adding new manifest fields, prefer optional fields.

### NFR-5: Testability

- Renderer should be headless and unit-testable.
- Issue Pack integration should be tested without GUI.
- Agent proposal acceptance should be tested independently from actual LLM runtime.

---

## 13. UX design details

### 13.1 Timeline header

Current header already has:

```text
Discard | Export GIF | Export Storyboard | Export MP4 | Export Guide | Export Bug Report...
```

Near-term recommendation:

```text
Discard | Preview Storyboard | Export Storyboard | Export MP4 | Export GIF | Export Guide | Export Bug Report...
```

But avoid too many buttons if header is crowded. Alternative:

```text
Discard | Export... | Bug Report...
```

Where `Export...` opens a small menu:

- Storyboard PNG
- GIF
- MP4
- Guide Folder

### 13.2 Selected step panel

Current:

```text
[keyframe]
Step title input
Delete step
```

After captions:

```text
[keyframe]
Title
[ Click Save ]
Caption
[ Explain what matters in this step ]
[Annotate Step] [Delete Step]
```

### 13.3 Preview modal

```text
Preview Storyboard

[ scrollable preview image ]

[Export PNG] [Copy Image] [Close]
```

For quick implementation, `Copy Image` can be hidden until clipboard support is ready.

### 13.4 Agent proposal panel

```text
Suggested captions

Step 1
Current: Click
Suggested: Open Settings
Caption: The user opens the settings screen from the toolbar.
[Accept] [Reject]

Step 2
...

[Accept All] [Reject All]
```

---

## 14. Technical design

### 14.1 Refactor Storyboard renderer into two layers

Current exporter writes directly to file. Add an in-memory render layer:

```rust
pub struct StoryboardRenderResult {
    pub image: RgbaImage,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}

pub fn render_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError>;

pub fn export_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
    out_path: &Path,
) -> Result<StoryboardExportResult, StoryboardError>;
```

### 14.2 Introduce generic storyboard step input

To support captions and annotations without bloating `Guide`, add generic renderer input:

```rust
pub struct StoryboardStep<'a> {
    pub index: usize,
    pub title: &'a str,
    pub caption: Option<&'a str>,
    pub image: &'a RgbaImage,
}

pub fn render_storyboard_steps(
    steps: &[StoryboardStep<'_>],
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError>;
```

Then adapters:

```rust
render_storyboard(guide, store, opts)
render_storyboard_with_presentation(guide, store, presentation, opts)
```

### 14.3 Presentation metadata

Add app-owned or core-owned presentation state:

```rust
pub struct StepPresentation {
    pub source: CandidateId,
    pub keyframe: FrameId,
    pub caption: Option<String>,
    pub annotations: Vec<Annotation>,
}
```

For fast implementation, app-owned state is acceptable.

### 14.4 Annotation flattening

For annotated step rendering:

```rust
let mut doc = ImageDocument::new(raw_keyframe.clone());
doc.apply_batch(annotation_ops)?;
let flattened = doc.flatten();
```

Or construct an `ImageDocument` when user enters annotation mode and store its annotations.

### 14.5 Issue Pack Storyboard integration

Modify `manifest_assets(...)` to conditionally include Storyboard:

```rust
if include_storyboard {
    assets.push(AssetEntry {
        kind: "action_storyboard".to_string(),
        path: "action-guide/storyboard.png".to_string(),
    });
}
```

Modify `render_issue_markdown(...)`:

```rust
if action.storyboard_path.is_some() {
    md.push_str("Overview:\n\n");
    md.push_str("![](action-guide/storyboard.png)\n\n");
}
```

### 14.6 Agent proposal integration

Caption proposals should be separate from image-document edit proposals because `ImageDocument` does not know about guide titles/captions.

Annotation proposals can reuse `EditProposal`, wrapped with step identity:

```rust
pub struct ActionGuideAnnotationProposal {
    pub step_source: CandidateId,
    pub base_keyframe: FrameId,
    pub edit_proposal: EditProposal,
}
```

Apply only if:

- Step still exists.
- Step source matches.
- Current keyframe equals `base_keyframe`.
- Proposal base state matches the annotation document state, if using `ImageDocument` state IDs.

---

## 15. Implementation plan optimized for fast shipping

## Task 1 — Include Storyboard in Issue Pack

Files:

- Modify `crates/rollshot-app/src/issue_pack.rs`
- Add/modify tests in same file

Steps:

1. Add `include_storyboard` or `storyboard_path` to `ActionGuideIssueAssets`.
2. In Action Guide folder build, call `rollshot_action::export_storyboard(...)` after `export_guide(...)`.
3. Add warning on failure.
4. Update `render_issue_markdown(...)` to include overview image only when available.
5. Update `manifest_assets(...)` to include `action_storyboard` only when present.
6. Add tests:
   - manifest includes storyboard
   - markdown includes storyboard relative path
   - storyboard failure warning does not block export

Suggested commit:

```text
feat(issue-pack): include action guide storyboard
```

---

## Task 2 — Add in-memory Storyboard render API

Files:

- Modify `crates/rollshot-action/src/storyboard.rs`
- Modify `crates/rollshot-action/src/lib.rs`
- Add tests in `storyboard.rs`

Steps:

1. Extract current layout/raster code from `export_storyboard` into `render_storyboard`.
2. Keep `export_storyboard` as file-write wrapper.
3. Add `StoryboardRenderResult`.
4. Add tests for render result dimensions and step count.

Suggested commit:

```text
refactor(action): expose storyboard render result
```

---

## Task 3 — Add Storyboard preview modal

Files:

- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`
- Modify `TimelineWorkspace` state definition file

Steps:

1. Add state field for preview modal:

```rust
pub storyboard_preview: Option<StoryboardPreviewState>
```

2. Add messages:

```rust
PreviewStoryboardRequested
PreviewStoryboardRendered(Result<StoryboardPreview, String>)
PreviewStoryboardClosed
```

3. Generate preview through `render_storyboard` with smaller options.
4. Show modal with scrollable image.
5. Keep export button available.

Suggested commit:

```text
feat(action): preview storyboard before export
```

---

## Task 4 — Add manual step captions

Files:

- Modify `crates/rollshot-action/src/models.rs`
- Modify `crates/rollshot-action/src/guide.rs`
- Modify `crates/rollshot-action/src/export.rs`
- Modify `crates/rollshot-action/src/storyboard.rs`
- Modify Timeline Workspace update/view files
- Modify Issue Pack markdown render

Steps:

1. Add `caption: String` or app-owned caption state.
2. Add `Guide::set_caption(index, caption)` if core-owned.
3. Add caption input in detail panel.
4. Render caption under title in Storyboard.
5. Include caption in `steps.md`, `session.json`, and Issue Pack markdown.
6. Add tests.

Suggested commit:

```text
feat(action): add step captions to storyboard
```

---

## Task 5 — Add lightweight per-step annotation MVP

Files likely touched:

- `crates/rollshot-app/src/timeline_workspace/*`
- `crates/rollshot-image-document/*` only if missing public API
- `crates/rollshot-action/src/storyboard.rs` for generic step renderer

MVP scope:

- Support number callout first.
- Defer text/redaction UI unless reuse is easy.
- Flatten annotation into storyboard preview/export.

Steps:

1. Add generic `render_storyboard_steps` API.
2. Add `StepPresentation` state keyed by `CandidateId`.
3. Add `Annotate Step` entry point.
4. Reuse existing annotation document/editor if possible.
5. On keyframe replacement, clear annotations for that step.
6. Use flattened image in Storyboard.

Suggested commit:

```text
feat(action): annotate storyboard steps
```

---

## Task 6 — Add agent caption proposals

Files likely touched:

- `rollshot-automation` / agent frontend depending on current architecture
- `rollshot-edit-proposal` or new `rollshot-action-proposal`
- Timeline Workspace proposal UI

MVP scope:

- Only propose titles/captions.
- No visual annotation proposal yet.

Suggested commit:

```text
feat(action): suggest step captions
```

---

## 16. Acceptance criteria by milestone

### P1 acceptance

- [ ] Issue Pack includes `action-guide/storyboard.png` for Action Guide packs.
- [ ] `issue.md` references Storyboard overview using relative path.
- [ ] `manifest.json` includes `action_storyboard` only if file exists.
- [ ] Storyboard failure becomes warning, not fatal, when Guide export succeeds.
- [ ] Existing screenshot-only Issue Pack tests still pass.

### P2 acceptance

- [ ] User can preview Storyboard from Timeline Workspace.
- [ ] Preview reflects current reviewed steps.
- [ ] Preview reflects renamed titles and replaced keyframes.
- [ ] Preview failure is recoverable.
- [ ] Workspace remains open.

### P3 acceptance

- [ ] User can edit caption per step.
- [ ] Captions appear in Storyboard.
- [ ] Captions appear in Issue Pack Markdown.
- [ ] Empty captions are omitted.
- [ ] Timeline list remains title-only.

### P4 acceptance

- [ ] User can add a number callout to a selected step.
- [ ] Storyboard export includes flattened callout.
- [ ] Original retained keyframe is unchanged.
- [ ] Keyframe replacement safely clears/stales annotations.

### P5 acceptance

- [ ] Agent can propose captions.
- [ ] User can accept/reject per step.
- [ ] Accepted captions are normal user-editable captions afterward.
- [ ] Stale proposals cannot apply.

---

## 17. Risks and mitigations

### Risk 1: Action Guide workspace becomes too heavy

Mitigation:

- Keep annotation mode separate.
- Start with captions and Issue Pack integration.
- Do not add a full toolbar to Timeline Workspace immediately.

### Risk 2: “Redaction” in Storyboard creates false safety assumptions

Mitigation:

- Explicitly label Action Guide keyframes as reviewed evidence.
- If Issue Pack includes original keyframes, do not claim the pack is fully redacted.
- Later add “redacted keyframe export” as a separate feature.

### Risk 3: Storyboard output becomes too tall

Mitigation:

- Keep max canvas pixels.
- Later add compact/grid mode.
- Preview will help users delete noisy steps.

### Risk 4: Agent annotations are wrong

Mitigation:

- Agent proposals are never auto-applied.
- Show confidence/rationale.
- Require accept/reject.

### Risk 5: Caption model migration creates churn

Mitigation:

- Use optional manifest fields.
- Consider app-owned presentation metadata first if persistence is uncertain.

---

## 18. Metrics and qualitative validation

If telemetry exists later:

- Storyboard export count.
- Storyboard included in Issue Pack count.
- GIF/MP4/Storyboard relative usage.
- Average step count per Storyboard.
- Export failure categories.
- Preview opened → export conversion.
- Caption edit rate.
- Agent suggestion accept/reject rate.

Without telemetry:

- Dogfood by sharing Storyboards in Slack/issue discussions.
- Track whether recipients ask fewer clarification questions.
- Track whether users request annotation before or after captions.

---

## 19. Open questions

1. Should `caption` live in `GuideStep` or in app-owned presentation metadata?
2. Should Storyboard be included in Issue Pack by default with no checkbox?
3. Should `Copy Storyboard` be prioritized before preview?
4. Should annotated Storyboard export include original keyframes in Issue Pack, or annotated keyframes too?
5. Should agent captioning require OCR first, or can it start with visual/keyframe + current title context?
6. Should Storyboard support compact/contact-sheet layout before annotations?

Recommended answers for fast implementation:

1. Add caption to `GuideStep` if no import compatibility constraint; otherwise app-owned.
2. Yes, include Storyboard by default.
3. Defer copy if clipboard image support is platform-specific; do preview first.
4. Keep original keyframes; include annotated Storyboard only. Be explicit in copy.
5. Start without OCR if agent visual input exists; otherwise use title/kind/reason only for draft captions.
6. Defer compact layout until users complain output is too tall.

---

## 20. Final recommendation

The best near-term path is:

1. **P1: Include Storyboard in Issue Pack.** This is the fastest meaningful product improvement because the renderer already exists.
2. **P2: Add Storyboard preview.** This improves trust and reduces blind exports.
3. **P3: Add captions.** This is the lowest-risk communication upgrade and prepares for agent assistance.
4. **P4: Add lightweight step annotations.** Reuse `ImageDocument`; avoid a second annotation system.
5. **P5: Add agent proposals.** Agent should propose captions/callouts only after users can manually review/edit those concepts.

In product language, the feature line should be framed as:

> **Action Guide Storyboard turns reviewed workflow keyframes into a share-ready visual step summary.**

This is stronger and less ambiguous than “大圖”, and it avoids confusing Storyboard layout with Rollshot’s existing scrolling screenshot stitching.

