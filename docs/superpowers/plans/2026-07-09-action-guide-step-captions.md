# Action Guide Step Captions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add optional one-line captions to reviewed Action Guide steps and include those captions in Storyboard, guide export, and Issue Pack output.

**Architecture:** Store captions on the headless `GuideStep` model so every export path reads the same reviewed step state. Keep the Timeline Workspace UI simple with one additional iced `text_input`, and keep Storyboard caption rendering deterministic by fitting long captions with ellipsis instead of introducing a text-wrapping engine.

**Tech Stack:** Rust, `rollshot-action`, `rollshot-app` with `action-guide`, iced 0.14 built-in widgets, `serde`, `image`, Cargo tests through `rtk`.

## Global Constraints

- This plan implements PRD Phase P3 only: step captions separate from step titles.
- P1 Issue Pack Storyboard integration is already present in `crates/rollshot-app/src/issue_pack.rs`.
- P2 Storyboard preview is already present in `crates/rollshot-action/src/storyboard.rs` and `crates/rollshot-app/src/timeline_workspace`.
- Captions are optional and empty captions are omitted from rendered/exported artifacts.
- Captions are preserved when a step keyframe is replaced.
- Captions disappear naturally when the step is deleted.
- Existing title rename behavior and timeline list labels remain title-based.
- Existing Guide/GIF/MP4/Storyboard exports without captions remain source-compatible.
- No per-step annotations, redactions, layout controls, copy-to-clipboard, or agent proposals in this phase.
- No new crates or GUI frameworks.
- iced UI changes use built-in widgets only; no custom widget or custom overlay.
- Runtime diagnostics in product paths must use `tracing` with explicit `rollshot::*` targets. This phase does not need new runtime diagnostics unless an implementation path adds recoverable error handling.

---

## File Structure

- Modify `crates/rollshot-action/src/models.rs`
  - Add `caption: String` to `GuideStep`.
- Modify `crates/rollshot-action/src/guide.rs`
  - Initialize captions to empty strings.
  - Add `Guide::set_caption(index, caption) -> bool`.
  - Test default, update, keyframe replacement, and deletion behavior.
- Modify `crates/rollshot-action/src/export.rs`
  - Add optional `caption` to `ManifestStep`.
  - Include non-empty captions in `steps.md`.
  - Serialize captions in `session.json` only when present.
- Modify `crates/rollshot-action/src/storyboard.rs`
  - Render caption text between the step label and keyframe image.
  - Fit long captions to card width with ellipsis.
  - Keep canvas limit checks authoritative.
- Modify `crates/rollshot-app/src/issue_pack.rs`
  - Add optional `caption` to `IssuePackStep`.
  - Thread captions from `ActionGuideIssueAssets::from_guide`.
  - Include captions in `issue.md` when present.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`
  - Add `Message::CaptionChanged(String)`.
  - Update the selected step caption.
  - Test caption edit and keyframe replacement preservation.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`
  - Add a caption input in the selected step detail panel.
  - Rely on existing view smoke tests.

No new files or crates are needed.

---

## Review Lock-In

### Scope Challenge

The smallest independently useful next phase is captions. P1 and P2 already made Storyboard shareable and previewable; captions make the artifact explain what matters without adding annotation/editor complexity. Captions also create the manual primitive that P5 agent-suggested captions can target.

### What Already Exists

- `GuideStep.title` already carries reviewed, user-edited step text through Timeline Workspace, Storyboard, guide export, and Issue Pack. This plan reuses that path for captions instead of adding app-only presentation state.
- `Guide::rename`, `Guide::delete`, and `Guide::replace_keyframe` already provide the mutation shape for reviewed steps. `Guide::set_caption` intentionally mirrors `rename`.
- `rollshot-action::export_guide(...)` already writes `steps.md`, `session.json`, and keyframes. This plan extends those artifacts without changing the export directory structure.
- `rollshot-action::render_storyboard(...)` already provides the in-memory path used by preview and export. This plan extends the existing card layout and keeps the renderer API source-compatible.
- `ActionGuideIssueAssets::from_guide(...)` already snapshots reviewed guide metadata for Issue Pack rendering. This plan adds caption mapping there rather than rebuilding Issue Pack state.
- Timeline Workspace already uses iced `text_input` for step titles. This plan uses the same built-in widget for captions.

### Data Model Choice

Use `GuideStep.caption: String`.

This is intentionally more direct than an app-only presentation map. Storyboard, portable guide export, Issue Pack, and Timeline Workspace all already consume `Guide`; storing captions there avoids parallel state and keeps reviewed steps the single source of truth. Persisted `session.json` compatibility is handled by making exported manifest captions optional.

### Data Flow

```text
Timeline detail panel
  text_input("Step caption")
          |
          v
Message::CaptionChanged(String)
          |
          v
Guide::set_caption(index, caption)
          |
          v
GuideStep.caption
   |              |                 |
   v              v                 v
steps.md +   Storyboard PNG    Issue Pack issue.md
session.json  preview/export    Bug Report export
```

### Text Rendering Choice

Use a single-line `text_input` in Timeline Workspace and ellipsis fitting in Storyboard. `rollshot-image-document::measure_block` does not soft-wrap; introducing a wrapping layout engine would be larger than P3 needs. The PRD calls captions one-line, so this matches the product scope.

### NOT in scope

- No multi-line caption editor.
- No rich text or Markdown captions.
- No Storyboard layout mode changes.
- No annotations, callouts, highlights, or redaction semantics.
- No agent-generated captions.
- No persistence/import of editable Timeline Workspace sessions beyond existing export artifacts.

### Test Coverage Table

```text
Task / behavior                                      Unit  Integration  UI smoke  Manual only
---------------------------------------------------  ----  -----------  --------  -----------
Task 1 / GuideStep defaults to empty caption          yes   no           no        no
Task 1 / set_caption updates selected step model      yes   no           no        no
Task 1 / keyframe replacement preserves caption       yes   no           no        no
Task 2 / steps.md includes non-empty caption          yes   yes          no        no
Task 2 / session.json optional caption field          yes   yes          no        no
Task 2 / old session JSON without caption loads       yes   yes          no        no
Task 3 / Storyboard height changes for caption        yes   no           no        no
Task 3 / long caption fits card width                 yes   no           no        no
Task 3 / whitespace-only caption omitted              yes   no           no        no
Task 4 / Issue Pack Markdown includes caption         yes   yes          no        no
Task 4 / empty caption omitted from Issue Pack        yes   no           no        no
Task 5 / Timeline update edits caption                yes   no           no        no
Task 5 / detail panel view builds with caption input  no    no           yes       no
Final / formatting and feature test suites            no    yes          no        no
```

### Test Diagram

```text
Task 1 model tests
  prove GuideStep.caption exists and survives keyframe replacement
          |
          v
Task 2 export tests
  prove guide artifacts serialize non-empty captions and load old sessions
          |
          v
Task 3 renderer tests
  prove Storyboard layout includes useful captions and omits empty captions
          |
          v
Task 4 Issue Pack tests          Task 5 Timeline tests
  prove Bug Report Markdown       prove user input mutates selected GuideStep
  uses the same reviewed state     and preview/export sees the same state
          \                       /
           v                     v
          Task 6 focused suites + fmt + clippy + manual smoke
```

### Failure Modes

```text
Codepath / risk                                      Test coverage                    Handling / user visibility
---------------------------------------------------  -------------------------------  ---------------------------------------------
Whitespace-only caption renders as visual noise      Task 3 whitespace-only test      Trim and omit; user sees no empty caption line
Old session JSON lacks caption field                 Task 2 deserialize old JSON      serde default -> None; no user-visible failure
Very long caption overflows Storyboard card          Task 3 long-caption fit test     Ellipsis fit; user sees truncated Storyboard text
Caption makes Storyboard exceed pixel limit          Existing Storyboard errors       StoryboardError::CanvasTooLarge surfaces in preview/export paths
Keyframe replacement drops unrelated metadata        Task 1 + Task 5 preservation     Caption stays on GuideStep; user sees caption unchanged
Step deletion leaves orphan caption                  Task 1 model ownership           Caption is removed with GuideStep; no hidden state remains
Issue Pack references a missing caption asset        Task 4 Markdown tests            Captions are inline text only; no new file asset is introduced
```

No critical gaps remain: every new silent-failure risk has either a test or uses an existing visible error path.

### Parallelization

Task 1 changes the shared model and must run first. After Task 1 lands, Task 2 and Task 3 share `crates/rollshot-action/` and should stay sequential in one lane. Task 4 and Task 5 touch separate `rollshot-app` modules and can run in parallel if the executor has isolated task branches/sessions; otherwise run sequentially in the listed order.

```text
Task                                           Modules touched                         Depends on
---------------------------------------------  --------------------------------------  ----------
Task 1: Guide model captions                   crates/rollshot-action/                 —
Task 2: Guide export captions                  crates/rollshot-action/                 Task 1
Task 3: Storyboard captions                    crates/rollshot-action/                 Task 1
Task 4: Issue Pack captions                    crates/rollshot-app/src/issue_pack.rs   Task 1
Task 5: Timeline caption editing               crates/rollshot-app/src/timeline_*      Task 1
Task 6: Verification                           workspace                               Tasks 1-5
```

Parallel lanes:

```text
Lane A: Task 1
Lane B: Task 2 -> Task 3 (sequential; both touch rollshot-action storyboard/export code)
Lane C: Task 4 (independent after Task 1)
Lane D: Task 5 (independent after Task 1)
Lane E: Task 6 after B + C + D merge
```

Conflict flags: Lanes C and D both compile `rollshot-app`, but they touch different files. Lanes B, C, and D all depend on the `GuideStep.caption` shape from Task 1, so do not start them before Task 1 is merged.

---

### Task 1: Add Captions To The Guide Model

**Files:**
- Modify: `crates/rollshot-action/src/models.rs`
- Modify: `crates/rollshot-action/src/guide.rs`
- Test: `crates/rollshot-action/src/guide.rs`

**Interfaces:**
- Consumes: existing `GuideStep`, `Guide::from_candidates`, `Guide::rename`, `Guide::replace_keyframe`, `Guide::delete`.
- Produces:
  - `GuideStep { caption: String, ... }`
  - `pub fn set_caption(&mut self, index: usize, caption: String) -> bool`

- [ ] **Step 1: Write failing guide model tests**

Add these tests to `#[cfg(test)] mod tests` in `crates/rollshot-action/src/guide.rs`:

```rust
    #[test]
    fn from_candidates_initializes_empty_captions() {
        let g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);

        assert_eq!(g.steps()[0].caption, "");
    }

    #[test]
    fn set_caption_persists_and_unknown_index_is_rejected() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);

        assert!(g.set_caption(
            1,
            "Settings close but the value is not saved.".to_string()
        ));
        assert_eq!(
            g.steps()[0].caption,
            "Settings close but the value is not saved."
        );
        assert!(!g.set_caption(99, "ignored".to_string()));
    }

    #[test]
    fn replace_keyframe_preserves_caption() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![4, 5, 6])]);

        assert!(g.set_caption(1, "The save action loses state.".to_string()));
        assert!(g.replace_keyframe(1, 6));

        assert_eq!(g.steps()[0].caption, "The save action loses state.");
        assert_eq!(g.steps()[0].keyframe, 6);
    }
```

- [ ] **Step 2: Run guide tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-action guide
```

Expected: FAIL to compile with errors equivalent to:

```text
no field `caption` on type `GuideStep`
no method named `set_caption` found for struct `Guide`
```

- [ ] **Step 3: Add the caption field and setter**

In `crates/rollshot-action/src/models.rs`, add `caption` directly after `title`:

```rust
pub struct GuideStep {
    pub index: usize,
    pub title: String,
    pub caption: String,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe: FrameId,
    pub nearby: Vec<FrameId>,
    pub source: CandidateId,
}
```

In `crates/rollshot-action/src/guide.rs`, initialize the field in `from_candidates`:

```rust
GuideStep {
    index: i + 1,
    title: default_title(c.kind).to_string(),
    caption: String::new(),
    kind: c.kind,
    reason: c.reason,
    at_ms: c.at_ms,
    keyframe: c.keyframe,
    nearby: c.nearby,
    source: c.id,
}
```

Add this method after `rename`:

```rust
    /// Set a step's optional Storyboard/Issue Pack caption. Returns false if
    /// `index` is unknown.
    pub fn set_caption(&mut self, index: usize, caption: String) -> bool {
        match self.steps.iter_mut().find(|s| s.index == index) {
            Some(step) => {
                step.caption = caption;
                true
            }
            None => false,
        }
    }
```

- [ ] **Step 4: Run guide tests to verify pass**

Run:

```bash
rtk cargo test -p rollshot-action guide
```

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-action/src/models.rs crates/rollshot-action/src/guide.rs
rtk git commit -m "feat(action): add guide step captions"
```

---

### Task 2: Export Captions In Guide Artifacts

**Files:**
- Modify: `crates/rollshot-action/src/export.rs`
- Test: `crates/rollshot-action/src/export.rs`

**Interfaces:**
- Consumes: `GuideStep.caption`.
- Produces:
  - `ManifestStep { caption: Option<String>, ... }`
  - `steps.md` includes non-empty caption text under the step title.
  - `session.json` includes `"caption"` only for non-empty captions.

- [ ] **Step 1: Write failing export test**

Add this test to `#[cfg(test)] mod tests` in `crates/rollshot-action/src/export.rs`:

```rust
    #[test]
    fn export_guide_includes_non_empty_step_caption() {
        let (mut guide, store) = one_step_recording();
        assert!(guide.set_caption(
            1,
            "The settings dialog closes, but the new value is not persisted.".to_string()
        ));
        let out = temp_dir("export-caption");

        let dir = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
            &out,
        )
        .expect("export succeeds");

        let md = std::fs::read_to_string(dir.join("steps.md")).unwrap();
        assert!(
            md.contains("The settings dialog closes, but the new value is not persisted."),
            "md = {md}"
        );

        let json = std::fs::read_to_string(dir.join("session.json")).unwrap();
        let parsed: SessionManifest = serde_json::from_str(&json).expect("manifest parses");
        assert_eq!(
            parsed.steps[0].caption.as_deref(),
            Some("The settings dialog closes, but the new value is not persisted.")
        );

        let _ = std::fs::remove_dir_all(&out);
    }
```

Also add this backward-compatibility test in the same module:

```rust
    #[test]
    fn session_manifest_deserializes_without_caption_field() {
        let json = r#"{
  "region": { "x": 0, "y": 0, "width": 8, "height": 8 },
  "input_source": "linux-evdev",
  "input_capability": "semantic-events",
  "steps": [
    {
      "index": 1,
      "title": "Click",
      "kind": "click",
      "reason": "click-confirmed",
      "at_ms": 0,
      "keyframe_file": "keyframes/001.png"
    }
  ]
}"#;

        let parsed: SessionManifest = serde_json::from_str(json).expect("manifest parses");

        assert_eq!(parsed.steps.len(), 1);
        assert_eq!(parsed.steps[0].caption, None);
    }
```

- [ ] **Step 2: Run export tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-action export_guide_includes_non_empty_step_caption
```

Expected: FAIL to compile with:

```text
no field `caption` on type `ManifestStep`
```

- [ ] **Step 3: Add optional caption to export manifest and Markdown**

In `ManifestStep`, add the optional field after `title`:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestStep {
    pub index: usize,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe_file: String,
}
```

Add this helper near `build(...)`:

```rust
fn non_empty_caption(caption: &str) -> Option<&str> {
    let caption = caption.trim();
    (!caption.is_empty()).then_some(caption)
}
```

In the `for` loop inside `build(...)`, replace the existing Markdown push and `ManifestStep` creation with:

```rust
        let caption = non_empty_caption(&step.caption);
        md.push_str(&format!("{n}. {}\n\n", step.title));
        if let Some(caption) = caption {
            md.push_str(&format!("   {caption}\n\n"));
        }
        md.push_str(&format!("   ![]({rel})\n\n"));
        steps.push(ManifestStep {
            index: step.index,
            title: step.title.clone(),
            caption: caption.map(str::to_string),
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            keyframe_file: rel,
        });
```

- [ ] **Step 4: Update existing manifest assertions**

In `session_json_has_capability_and_no_raw_input_fields`, keep the current assertions and add:

```rust
        assert_eq!(parsed.steps[0].caption, None);
        assert!(
            !json.contains("\"caption\""),
            "empty captions should be omitted: {json}"
        );
```

- [ ] **Step 5: Run export tests to verify pass**

Run:

```bash
rtk cargo test -p rollshot-action export
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-action/src/export.rs
rtk git commit -m "feat(action): export guide step captions"
```

---

### Task 3: Render Captions In Storyboard PNGs

**Files:**
- Modify: `crates/rollshot-action/src/storyboard.rs`
- Test: `crates/rollshot-action/src/storyboard.rs`

**Interfaces:**
- Consumes: `GuideStep.caption`.
- Produces:
  - Caption text drawn below `Step N - title`.
  - Long captions elided to fit `content_width`.
  - Existing `render_storyboard(...)` and `export_storyboard(...)` signatures unchanged.

- [ ] **Step 1: Write failing Storyboard tests**

Add these tests to `#[cfg(test)] mod tests` in `crates/rollshot-action/src/storyboard.rs`:

```rust
    #[test]
    fn captions_increase_storyboard_card_height() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 1);

        let without_caption = render_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render without caption");

        assert!(guide.set_caption(
            1,
            "The Save button closes the dialog without persisting the change.".to_string()
        ));
        let with_caption = render_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("render with caption");

        assert!(
            with_caption.height > without_caption.height,
            "caption should add text height"
        );
        assert_eq!(with_caption.step_count, 1);
    }

    #[test]
    fn long_captions_are_elided_to_fit_card_width() {
        let caption = fit_caption(
            "The settings dialog closes but the saved preference is not present after reopening the same panel",
            180.0,
        );

        assert!(caption.ends_with("..."), "caption = {caption}");
        assert!(
            measure_block(&caption, CAPTION_FONT_PX, false).0 <= 180.0,
            "caption should fit measured width: {caption}"
        );
    }

    #[test]
    fn whitespace_only_captions_are_omitted_from_storyboard_layout() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 1);
        let opts = StoryboardOptions {
            max_width: 320,
            max_canvas_pixels: 1_000_000,
            outer_padding: 12,
            card_spacing: 10,
            card_padding: 8,
            show_titles: true,
        };

        let without_caption = render_storyboard(&guide, &recording.store, opts.clone())
            .expect("render without caption");
        assert!(guide.set_caption(1, "    ".to_string()));
        let whitespace_caption = render_storyboard(&guide, &recording.store, opts)
            .expect("render with whitespace caption");

        assert_eq!(whitespace_caption.height, without_caption.height);
    }
```

- [ ] **Step 2: Run Storyboard tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: FAIL to compile with:

```text
cannot find value `CAPTION_FONT_PX` in this scope
cannot find function `fit_caption` in this scope
```

- [ ] **Step 3: Add caption constants and card fields**

In `crates/rollshot-action/src/storyboard.rs`, add constants near `LABEL_FONT_PX`:

```rust
const CAPTION_FONT_PX: f32 = 20.0;
const CAPTION_GAP: u32 = 8;
const CAPTION_COLOR: Rgba8 = Rgba8::new(71, 79, 92, 255);
```

Change `Card` to:

```rust
struct Card {
    label: String,
    caption: Option<String>,
    image: RgbaImage,
    height: u32,
}
```

- [ ] **Step 4: Include caption height in render calculation**

In the card-building loop inside `render_storyboard(...)`, replace the label/image height calculation with:

```rust
        let label = step_label(i + 1, &step.title, opts.show_titles);
        let label = fit_label(&label, content_width as f32);
        let (_, label_height) = measure_block(&label, LABEL_FONT_PX, true);
        let label_height = label_height.ceil() as u32;

        let caption = non_empty_caption(&step.caption)
            .map(|caption| fit_caption(caption, content_width as f32));
        let caption_height = caption
            .as_ref()
            .map(|caption| measure_block(caption, CAPTION_FONT_PX, false).1.ceil() as u32)
            .unwrap_or(0);
        let text_height = if caption.is_some() {
            label_height
                .checked_add(CAPTION_GAP)
                .and_then(|height| height.checked_add(caption_height))
                .ok_or(StoryboardError::CanvasTooLarge)?
        } else {
            label_height
        };
        let card_height = opts
            .card_padding
            .checked_mul(2)
            .and_then(|height| height.checked_add(text_height))
            .and_then(|height| height.checked_add(LABEL_GAP))
            .and_then(|height| height.checked_add(image.height()))
            .ok_or(StoryboardError::CanvasTooLarge)?;
        cards.push(Card {
            label,
            caption,
            image,
            height: card_height,
        });
```

- [ ] **Step 5: Draw caption text**

In the card drawing loop, replace the current label height advance with:

```rust
        let (_, label_height) = measure_block(&card.label, LABEL_FONT_PX, true);
        content_y += label_height.ceil() as u32;
        if let Some(caption) = &card.caption {
            content_y += CAPTION_GAP;
            draw_text_block(
                &mut canvas,
                ImagePoint::new(content_x as f32, content_y as f32),
                caption,
                CAPTION_FONT_PX,
                false,
                CAPTION_COLOR,
            );
            let (_, caption_height) = measure_block(caption, CAPTION_FONT_PX, false);
            content_y += caption_height.ceil() as u32;
        }
        content_y += LABEL_GAP;
```

- [ ] **Step 6: Add caption helpers**

Replace `fit_label(...)` with a generic helper and caption-specific wrapper:

```rust
fn fit_label(label: &str, max_width: f32) -> String {
    fit_text(label, max_width, LABEL_FONT_PX, true)
}

fn fit_caption(caption: &str, max_width: f32) -> String {
    fit_text(caption.trim(), max_width, CAPTION_FONT_PX, false)
}

fn fit_text(text: &str, max_width: f32, px: f32, bold: bool) -> String {
    if measure_block(text, px, bold).0 <= max_width {
        return text.to_string();
    }

    let ellipsis = "...";
    let text = text.trim_end();
    let mut fitted = String::new();
    for ch in text.chars() {
        let candidate = format!("{fitted}{ch}{ellipsis}");
        if measure_block(&candidate, px, bold).0 > max_width {
            break;
        }
        fitted.push(ch);
    }
    if fitted.is_empty() {
        ellipsis.to_string()
    } else {
        format!("{fitted}{ellipsis}")
    }
}

fn non_empty_caption(caption: &str) -> Option<&str> {
    let caption = caption.trim();
    (!caption.is_empty()).then_some(caption)
}
```

- [ ] **Step 7: Run Storyboard tests to verify pass**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: PASS.

- [ ] **Step 8: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-action/src/storyboard.rs
rtk git commit -m "feat(action): render storyboard captions"
```

---

### Task 4: Include Captions In Issue Pack Markdown

**Files:**
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

**Interfaces:**
- Consumes: `GuideStep.caption`.
- Produces:
  - `IssuePackStep { caption: Option<String>, ... }`
  - `ActionGuideIssueAssets::from_guide(...)` maps non-empty captions.
  - `render_issue_markdown(...)` prints caption text below the numbered step title.

- [ ] **Step 1: Write failing Issue Pack tests**

In `#[cfg(test)] mod tests` in `crates/rollshot-app/src/issue_pack.rs`, add:

```rust
    #[test]
    fn issue_markdown_includes_action_step_caption_when_present() {
        let mut input = action_guide_input_with_one_step(false);
        input.action_guide.as_mut().unwrap().steps[0].caption =
            Some("The dialog closes but the setting is not saved.".to_string());

        let md = render_issue_markdown(&input, true);

        assert!(
            md.contains("The dialog closes but the setting is not saved."),
            "md = {md}"
        );
        assert!(
            md.contains("1. Open Settings\n\n   The dialog closes but the setting is not saved.\n\n   ![](action-guide/keyframes/001.png)"),
            "md = {md}"
        );
    }

    #[test]
    fn issue_markdown_omits_empty_action_step_caption() {
        let mut input = action_guide_input_with_one_step(false);
        input.action_guide.as_mut().unwrap().steps[0].caption = None;

        let md = render_issue_markdown(&input, true);

        assert!(md.contains("1. Open Settings\n\n   ![](action-guide/keyframes/001.png)"));
    }
```

In the feature-gated `action_guide_tests` module, add:

```rust
    #[test]
    fn action_guide_issue_assets_maps_non_empty_captions() {
        let recording = recording();
        let mut guide = Guide::from_candidates(recording.candidates);
        assert!(guide.set_caption(1, "The value is lost after Save.".to_string()));

        let assets = ActionGuideIssueAssets::from_guide(&guide, false);

        assert_eq!(
            assets.steps[0].caption.as_deref(),
            Some("The value is lost after Save.")
        );
    }
```

- [ ] **Step 2: Run Issue Pack tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack
```

Expected: FAIL to compile with:

```text
no field `caption` on type `IssuePackStep`
```

- [ ] **Step 3: Add caption to IssuePackStep and map from Guide**

Change `IssuePackStep`:

```rust
pub(crate) struct IssuePackStep {
    pub index: usize,
    pub title: String,
    pub caption: Option<String>,
    pub keyframe_path: String,
}
```

Add a local helper near `ActionGuideIssueAssets::from_guide`:

```rust
fn non_empty_caption(caption: &str) -> Option<String> {
    let caption = caption.trim();
    (!caption.is_empty()).then(|| caption.to_string())
}
```

Change the `IssuePackStep` construction in `from_guide(...)`:

```rust
IssuePackStep {
    index: i + 1,
    title: step.title.clone(),
    caption: non_empty_caption(&step.caption),
    keyframe_path: format!("action-guide/keyframes/{:03}.png", i + 1),
}
```

Update existing `IssuePackStep` literals in `crates/rollshot-app/src/issue_pack.rs` by adding `caption: None,` next to `title` in these helpers/tests:

- `renders_action_guide_steps_and_omits_missing_ocr`
- `manifest_assets_list_every_expected_relative_path`
- `action_guide_input_with_one_step`

- [ ] **Step 4: Render captions in Issue Pack Markdown**

In `render_issue_markdown(...)`, replace the current per-step Markdown block with:

```rust
        for step in &action.steps {
            md.push_str(&format!("{}. {}\n\n", step.index, step.title));
            if let Some(caption) = &step.caption {
                md.push_str(&format!("   {caption}\n\n"));
            }
            md.push_str(&format!("   ![]({})\n\n", step.keyframe_path));
        }
```

- [ ] **Step 5: Run Issue Pack tests to verify pass**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack
```

Expected: PASS.

- [ ] **Step 6: Commit Task 4**

Run:

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(app): include action guide captions in issue packs"
```

---

### Task 5: Add Timeline Workspace Caption Editing

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**
- Consumes: `Guide::set_caption`.
- Produces:
  - `Message::CaptionChanged(String)`
  - Selected step detail panel with a caption `text_input`.

- [ ] **Step 1: Write failing update tests**

Add these tests near `title_changed_renames_selected_step` in `crates/rollshot-app/src/timeline_workspace/update.rs`:

```rust
    #[test]
    fn caption_changed_updates_selected_step() {
        let mut state = ws(synthetic_recording(2));

        let _ = update(
            &mut state,
            Message::CaptionChanged("The save action loses the selected value.".to_string()),
        );

        assert_eq!(
            state.selected_step().unwrap().caption,
            "The save action loses the selected value."
        );
    }

    #[test]
    fn replace_keyframe_preserves_selected_step_caption() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(
            &mut state,
            Message::CaptionChanged("The selected value is lost.".to_string()),
        );
        let step = state.selected_step().unwrap();
        let target = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();

        let _ = update(&mut state, Message::ReplaceKeyframe(target));

        assert_eq!(state.selected_step().unwrap().caption, "The selected value is lost.");
        assert_eq!(state.selected_step().unwrap().keyframe, target);
    }
```

- [ ] **Step 2: Run Timeline Workspace tests to verify failure**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
```

Expected: FAIL to compile with:

```text
no variant or associated item named `CaptionChanged` found for enum `Message`
```

- [ ] **Step 3: Add caption update message**

In `Message`, add the variant immediately after `TitleChanged(String)`:

```rust
    CaptionChanged(String),
```

In `update(...)`, add the match arm immediately after `Message::TitleChanged(title)`:

```rust
        Message::CaptionChanged(caption) => {
            if let Some(index) = state.selected {
                state.guide.set_caption(index, caption);
            }
            Task::none()
        }
```

- [ ] **Step 4: Add caption input to the detail panel**

In `detail_panel(...)` in `crates/rollshot-app/src/timeline_workspace/view.rs`, replace the two-line title/delete area with:

```rust
                text("Title").size(12),
                text_input("Step title", &step.title).on_input(Message::TitleChanged),
                text("Caption").size(12),
                text_input("Step caption", &step.caption).on_input(Message::CaptionChanged),
                button(text("Delete step"))
                    .on_press(Message::DeleteStep)
                    .style(button::danger),
```

Keep the surrounding `column![...]` structure and `.spacing(8)`.

- [ ] **Step 5: Run Timeline Workspace tests to verify pass**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
```

Expected: PASS, including the existing view smoke test.

- [ ] **Step 6: Commit Task 5**

Run:

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): edit action guide step captions"
```

---

### Task 6: End-To-End Verification

**Files:**
- No planned source edits unless verification exposes a defect.

**Interfaces:**
- Consumes all previous tasks.
- Produces a formatted, tested implementation branch.

- [ ] **Step 1: Run focused action tests**

Run:

```bash
rtk cargo test -p rollshot-action guide
rtk cargo test -p rollshot-action export
rtk cargo test -p rollshot-action storyboard
```

Expected: PASS for all three commands.

- [ ] **Step 2: Run focused app tests with Action Guide enabled**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
```

Expected: PASS.

- [ ] **Step 3: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 4: Run clippy because the change touches shared model and UI/export paths**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Manual product smoke**

Run the app with Action Guide enabled using the existing project workflow, record a short guide, enter a title and caption for one step, open Storyboard preview, export Storyboard, and export Bug Report.

Expected:

- Timeline list still shows compact titles.
- Selected detail panel shows editable Title and Caption inputs.
- Preview/exported Storyboard shows title plus caption.
- Issue Pack `issue.md` shows the caption under the relevant numbered step.
- Issue Pack `manifest.json` asset list is unchanged except for existing Storyboard assets from P1.

- [ ] **Step 6: Commit verification fixes if any**

If verification required source changes, commit them:

```bash
rtk git add crates/rollshot-action crates/rollshot-app
rtk git commit -m "fix(action): stabilize step caption exports"
```

If verification required no source changes, do not create an empty commit.

---

## Self-Review

### Spec Coverage

- FR-CAP-1 user can edit a caption: Task 5.
- FR-CAP-2 empty captions omitted: Task 2, Task 4.
- FR-CAP-3 caption appears in Storyboard and Issue Pack Markdown: Task 3, Task 4.
- FR-CAP-4 captions preserved when keyframe is replaced: Task 1, Task 5.
- FR-CAP-5 captions deleted when the step is deleted: Task 1 by storing caption on `GuideStep`.
- NFR-1 determinism: Task 3 uses deterministic text measurement and drawing.
- NFR-2 responsiveness: Task 3 keeps render on explicit preview/export paths and uses one-line fitting.
- NFR-4 backward compatibility: Task 2 makes manifest captions optional and skips empty serialization.
- NFR-5 testability: Tasks 1-5 add headless unit/integration tests.

### Placeholder Scan

No placeholders are intentionally left in this plan. All named functions, files, commands, and message variants are defined by this plan or exist in the current codebase.

### Type Consistency

- `GuideStep.caption` is a `String`.
- `Guide::set_caption(index, caption)` accepts `usize` and `String`, returns `bool`.
- Export/session and Issue Pack captions use `Option<String>` because serialized artifacts omit empty captions.
- UI message `Message::CaptionChanged(String)` mirrors `Message::TitleChanged(String)`.
