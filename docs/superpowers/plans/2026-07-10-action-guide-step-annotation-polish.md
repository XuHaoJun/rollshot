# Action Guide Step Annotation Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the next Action Guide Storyboard phase by making per-step annotations useful beyond the current number-callout MVP: number callouts, text notes, opaque redactions, and undo/redo all render in preview/export/Issue Pack without mutating original keyframes.

**Architecture:** Keep `rollshot-image-document` as the annotation source of truth and keep `timeline_workspace::annotation` as the iced adapter. The Timeline Workspace already flattens per-step `ImageDocument`s into `StoryboardStep` images, so this phase only extends the annotation modal and its update/view tests.

**Tech Stack:** Rust, iced 0.14 built-in widgets plus Canvas, `rollshot-image-document::ImageDocument`, `rollshot_action::render_storyboard_steps`, existing `rtk cargo test` workflow.

## Global Constraints

- Follow `AGENTS.md`: prefix shell commands with `rtk`.
- Use `tracing` targets for retained runtime diagnostics; do not add `println!`, `eprintln!`, or `dbg!`.
- Keep changes surgical: no full annotation editor, no moving/resizing existing annotations, no agent callout proposals in this phase.
- Use standard iced widgets plus Canvas; do not introduce a custom `iced::advanced::Widget`.
- Original Action Guide keyframes remain reviewed evidence images; only Storyboard preview/export and Issue Pack `storyboard.png` use flattened annotated keyframes.
- Keyframe replacement keeps caption but clears annotations for that step, matching current behavior.

---

## File Structure

- Modify `crates/rollshot-app/src/timeline_workspace/annotation.rs`
  - Add annotation tool state, text/redaction drafts, Canvas rendering for all `RenderShape` variants, and focused tests for presentation/session behavior.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`
  - Route annotation canvas gestures by active tool, commit text/redaction operations, and wire undo/redo.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`
  - Add compact tool controls, text-note input, undo/redo buttons, and keep the existing modal/Canvas layout.
- Test only existing crates/modules; no new crate or dependency.

---

### Task 1: Extend Annotation Session State

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`

**Interfaces:**
- Consumes: `rollshot_image_document::{ImagePoint, ImageRect, ImageDocument}`
- Produces:
  - `pub(crate) enum AnnotationTool { Number, Text, Redaction }`
  - `pub(crate) enum AnnotationDraft { Number { tip, bubble }, Redaction { start, current } }`
  - `StepAnnotationSession { tool, text_note }`

- [ ] **Step 1: Write failing tests for defaults and tool state**

Add these tests to the existing `#[cfg(test)] mod tests` in `annotation.rs`:

```rust
#[test]
fn annotation_session_defaults_to_number_tool_with_empty_text() {
    let image = ::image::RgbaImage::from_pixel(16, 12, ::image::Rgba([0, 0, 0, 255]));
    let session = StepAnnotationSession::new(7, 3, &image);

    assert_eq!(session.tool, AnnotationTool::Number);
    assert_eq!(session.text_note, "");
    assert_eq!(session.width, 16);
    assert_eq!(session.height, 12);
    assert!(session.draft.is_none());
}

#[test]
fn redaction_draft_rect_normalizes_drag_direction() {
    let draft = AnnotationDraft::Redaction {
        start: ImagePoint::new(12.0, 9.0),
        current: ImagePoint::new(2.0, 3.0),
    };

    assert_eq!(
        draft.redaction_rect(),
        Some(rollshot_image_document::ImageRect {
            x: 2.0,
            y: 3.0,
            width: 10.0,
            height: 6.0,
        })
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide annotation_session_defaults_to_number_tool_with_empty_text
rtk cargo test -p rollshot-app --features action-guide redaction_draft_rect_normalizes_drag_direction
```

Expected: FAIL because `AnnotationTool`, `text_note`, and `redaction_rect` do not exist.

- [ ] **Step 3: Add tool and draft state**

In `annotation.rs`, replace the current `AnnotationDraft` with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnnotationTool {
    Number,
    Text,
    Redaction,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AnnotationDraft {
    Number { tip: ImagePoint, bubble: ImagePoint },
    Redaction { start: ImagePoint, current: ImagePoint },
}

impl AnnotationDraft {
    pub(crate) fn redaction_rect(&self) -> Option<rollshot_image_document::ImageRect> {
        match self {
            AnnotationDraft::Redaction { start, current } => {
                Some(rollshot_image_document::ImageRect::from_corners(*start, *current))
            }
            AnnotationDraft::Number { .. } => None,
        }
    }
}
```

Update `StepAnnotationSession`:

```rust
pub(crate) struct StepAnnotationSession {
    pub source: CandidateId,
    #[allow(dead_code)]
    pub keyframe: FrameId,
    pub handle: image::Handle,
    pub width: u32,
    pub height: u32,
    pub tool: AnnotationTool,
    pub text_note: String,
    pub draft: Option<AnnotationDraft>,
}
```

Update `StepAnnotationSession::new`:

```rust
Self {
    source,
    keyframe,
    handle: super::build_handle(image),
    width: image.width(),
    height: image.height(),
    tool: AnnotationTool::Number,
    text_note: String::new(),
    draft: None,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide annotation_session_defaults_to_number_tool_with_empty_text
rtk cargo test -p rollshot-app --features action-guide redaction_draft_rect_normalizes_drag_direction
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/annotation.rs
rtk git commit -m "feat(action): extend annotation session state"
```

---

### Task 2: Render All Annotation Shapes in the Live Canvas

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`

**Interfaces:**
- Consumes: `rollshot_image_document::annotation_shapes`
- Produces: live Canvas overlay parity for number callouts, text notes, opaque redactions, and redaction draft rectangles.

- [ ] **Step 1: Write the failing redaction-draft helper test**

Add this test to `annotation.rs`. It drives a small helper that the Canvas `draw` path will use to convert a draft into the same `Annotation` shape model as committed annotations:

```rust
#[test]
fn draft_annotation_converts_redaction_draft_to_opaque_redaction() {
    let document = ImageDocument::new(::image::RgbaImage::from_pixel(
        64,
        64,
        ::image::Rgba([10, 20, 30, 255]),
    ));
    let annotation = draft_annotation(
        &document,
        AnnotationDraft::Redaction {
            start: ImagePoint::new(12.0, 9.0),
            current: ImagePoint::new(2.0, 3.0),
        },
    )
    .expect("draft annotation");

    assert!(matches!(
        annotation,
        Annotation::OpaqueRedaction { bounds, .. }
            if bounds
                == (rollshot_image_document::ImageRect {
                    x: 2.0,
                    y: 3.0,
                    width: 10.0,
                    height: 6.0,
                })
    ));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-app --features action-guide draft_annotation_converts_redaction_draft_to_opaque_redaction`

Expected: FAIL because `draft_annotation` does not exist.

- [ ] **Step 3: Add the draft helper and draw rectangles/top-left labels**

Add this helper near `rgba`:

```rust
fn draft_annotation(document: &ImageDocument, draft: AnnotationDraft) -> Option<Annotation> {
    match draft {
        AnnotationDraft::Number { tip, bubble } => Some(Annotation::NumberCallout {
            id: AnnotationId(0),
            number: document.annotations().len() as u32 + 1,
            tip,
            bubble,
        }),
        AnnotationDraft::Redaction { .. } => draft.redaction_rect().map(|bounds| {
            Annotation::OpaqueRedaction {
                id: AnnotationId(0),
                bounds,
            }
        }),
    }
}
```

In `NumberAnnotationCanvas::draw_annotation`, replace the ignored branch:

```rust
RenderShape::Rect { .. } | RenderShape::Label { .. } => {}
```

with:

```rust
RenderShape::Rect { rect, color } => {
    let path = canvas::Path::rectangle(
        Point::new(rect.x * self.scale, rect.y * self.scale),
        iced::Size::new(rect.width * self.scale, rect.height * self.scale),
    );
    frame.fill(&path, rgba(color));
}
RenderShape::Label {
    anchor,
    anchor_kind: TextAnchor::TopLeft,
    content,
    px,
    bold,
    color,
} => {
    frame.fill_text(canvas::Text {
        content,
        position: Point::new(anchor.x * self.scale, anchor.y * self.scale),
        color: rgba(color),
        size: iced::Pixels(px * self.scale),
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        font: if bold {
            iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::with_name(rollshot_image_document::style::FONT_FAMILY_NAME)
            }
        } else {
            iced::Font::with_name(rollshot_image_document::style::FONT_FAMILY_NAME)
        },
        ..canvas::Text::default()
    });
}
```

In `draw`, replace the current number-only draft block with:

```rust
if let Some(draft) = self.draft.and_then(|draft| draft_annotation(self.document, draft)) {
    self.draw_annotation(&mut frame, &draft);
}
```

- [ ] **Step 4: Add the Canvas construction smoke test**

Add this test to `annotation.rs`; it cannot pixel-test iced Canvas headlessly, but it ensures the program accepts mixed committed annotations and a redaction draft without panicking during construction:

```rust
#[test]
fn number_annotation_canvas_accepts_mixed_annotations_and_redaction_draft() {
    let mut document = ImageDocument::new(::image::RgbaImage::from_pixel(
        64,
        64,
        ::image::Rgba([10, 20, 30, 255]),
    ));
    document.add_number_callout(ImagePoint::new(8.0, 8.0), ImagePoint::new(24.0, 24.0));
    document
        .add_text_note(ImagePoint::new(4.0, 40.0), "Check this label".to_string())
        .unwrap();
    document
        .add_redaction(rollshot_image_document::ImageRect {
            x: 32.0,
            y: 8.0,
            width: 16.0,
            height: 12.0,
        })
        .unwrap();

    let canvas = NumberAnnotationCanvas {
        document: &document,
        draft: Some(AnnotationDraft::Redaction {
            start: ImagePoint::new(1.0, 1.0),
            current: ImagePoint::new(12.0, 10.0),
        }),
        scale: 0.5,
    };

    assert_eq!(canvas.scale, 0.5);
}
```

- [ ] **Step 5: Run annotation module tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::annotation::tests`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/annotation.rs
rtk git commit -m "feat(action): render annotation canvas primitives"
```

---

### Task 3: Commit Text Notes, Redactions, and Undo/Redo from Update Logic

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/annotation.rs`

**Interfaces:**
- Consumes: `ImageDocument::{add_text_note, add_redaction, undo, redo}`
- Produces:
  - `Message::AnnotationToolChanged(AnnotationTool)`
  - `Message::AnnotationTextChanged(String)`
  - `Message::AnnotationUndo`
  - `Message::AnnotationRedo`

- [ ] **Step 1: Write failing update tests**

Add these tests to `update.rs`:

```rust
#[test]
fn annotation_text_tool_commits_text_note_on_click() {
    let mut state = ws(recording_from_frames());
    let _ = update(&mut state, Message::AnnotateStepRequested);
    let source = state.selected_step().unwrap().source;
    let _ = update(
        &mut state,
        Message::AnnotationToolChanged(super::annotation::AnnotationTool::Text),
    );
    let _ = update(
        &mut state,
        Message::AnnotationTextChanged("This label matters".to_string()),
    );
    let _ = update(
        &mut state,
        Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(5.0, 6.0)),
    );

    let doc = state.presentation.doc(source).unwrap();
    assert!(doc.document.annotations().iter().any(|annotation| {
        matches!(
            annotation,
            rollshot_image_document::Annotation::TextNote { text, .. }
                if text == "This label matters"
        )
    }));
}

#[test]
fn annotation_redaction_tool_commits_dragged_redaction() {
    let mut state = ws(recording_from_frames());
    let _ = update(&mut state, Message::AnnotateStepRequested);
    let source = state.selected_step().unwrap().source;
    let _ = update(
        &mut state,
        Message::AnnotationToolChanged(super::annotation::AnnotationTool::Redaction),
    );
    let _ = update(
        &mut state,
        Message::AnnotationCanvasPressed(rollshot_image_document::ImagePoint::new(4.0, 4.0)),
    );
    let _ = update(
        &mut state,
        Message::AnnotationCanvasMoved(rollshot_image_document::ImagePoint::new(18.0, 20.0)),
    );
    let _ = update(
        &mut state,
        Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(18.0, 20.0)),
    );

    let doc = state.presentation.doc(source).unwrap();
    assert!(doc.document.annotations().iter().any(|annotation| {
        matches!(
            annotation,
            rollshot_image_document::Annotation::OpaqueRedaction { bounds, .. }
                if bounds.width >= 14.0 && bounds.height >= 16.0
        )
    }));
}

#[test]
fn annotation_undo_and_redo_update_current_document() {
    let mut state = ws(recording_from_frames());
    let _ = update(&mut state, Message::AnnotateStepRequested);
    let source = state.selected_step().unwrap().source;
    let _ = update(
        &mut state,
        Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
    );
    assert_eq!(
        state.presentation.doc(source).unwrap().document.annotations().len(),
        1
    );

    let _ = update(&mut state, Message::AnnotationUndo);
    assert_eq!(
        state.presentation.doc(source).unwrap().document.annotations().len(),
        0
    );

    let _ = update(&mut state, Message::AnnotationRedo);
    assert_eq!(
        state.presentation.doc(source).unwrap().document.annotations().len(),
        1
    );
}

#[test]
fn empty_text_note_click_sets_message_without_committing_annotation() {
    let mut state = ws(recording_from_frames());
    let _ = update(&mut state, Message::AnnotateStepRequested);
    let source = state.selected_step().unwrap().source;
    let _ = update(
        &mut state,
        Message::AnnotationToolChanged(super::annotation::AnnotationTool::Text),
    );
    let _ = update(
        &mut state,
        Message::AnnotationTextChanged("   ".to_string()),
    );
    let _ = update(
        &mut state,
        Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(5.0, 6.0)),
    );

    assert_eq!(
        state.presentation.doc(source).unwrap().document.annotations().len(),
        0
    );
    assert!(state
        .message
        .as_ref()
        .is_some_and(|message| message.contains("Enter text")));
}

#[test]
fn zero_area_redaction_sets_message_without_committing_annotation() {
    let mut state = ws(recording_from_frames());
    let _ = update(&mut state, Message::AnnotateStepRequested);
    let source = state.selected_step().unwrap().source;
    let _ = update(
        &mut state,
        Message::AnnotationToolChanged(super::annotation::AnnotationTool::Redaction),
    );
    let _ = update(
        &mut state,
        Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
    );

    assert_eq!(
        state.presentation.doc(source).unwrap().document.annotations().len(),
        0
    );
    assert!(state
        .message
        .as_ref()
        .is_some_and(|message| message.contains("Redaction failed")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide annotation_`

Expected: FAIL because the new messages and behaviors do not exist.

- [ ] **Step 3: Add messages**

In `Message`, add:

```rust
AnnotationToolChanged(super::annotation::AnnotationTool),
AnnotationTextChanged(String),
AnnotationUndo,
AnnotationRedo,
```

- [ ] **Step 4: Route annotation gestures by active tool**

Replace the three annotation canvas match arms with tool-aware logic:

```rust
Message::AnnotationToolChanged(tool) => {
    if let Some(session) = &mut state.annotation_session {
        session.tool = tool;
        session.draft = None;
    }
    Task::none()
}
Message::AnnotationTextChanged(text) => {
    if let Some(session) = &mut state.annotation_session {
        session.text_note = text;
    }
    Task::none()
}
Message::AnnotationCanvasPressed(point) => {
    if let Some(session) = &mut state.annotation_session {
        let point = clamp_annotation_point(point, session.width, session.height);
        session.draft = match session.tool {
            super::annotation::AnnotationTool::Number => {
                Some(super::annotation::AnnotationDraft::Number {
                    tip: point,
                    bubble: point,
                })
            }
            super::annotation::AnnotationTool::Redaction => {
                Some(super::annotation::AnnotationDraft::Redaction {
                    start: point,
                    current: point,
                })
            }
            super::annotation::AnnotationTool::Text => None,
        };
    }
    Task::none()
}
Message::AnnotationCanvasMoved(point) => {
    if let Some(session) = &mut state.annotation_session {
        let point = clamp_annotation_point(point, session.width, session.height);
        match &mut session.draft {
            Some(super::annotation::AnnotationDraft::Number { bubble, .. }) => *bubble = point,
            Some(super::annotation::AnnotationDraft::Redaction { current, .. }) => *current = point,
            None => {}
        }
    }
    Task::none()
}
Message::AnnotationCanvasReleased(point) => {
    commit_annotation_release(state, point);
    Task::none()
}
Message::AnnotationUndo => {
    with_annotation_document(state, |doc| {
        doc.document.undo();
    });
    Task::none()
}
Message::AnnotationRedo => {
    with_annotation_document(state, |doc| {
        doc.document.redo();
    });
    Task::none()
}
```

Add helpers near `clamp_annotation_point`:

```rust
fn with_annotation_document(
    state: &mut TimelineWorkspace,
    f: impl FnOnce(&mut super::annotation::StepAnnotationDocument),
) {
    let Some(session) = state.annotation_session.as_ref() else {
        return;
    };
    let Some(step) = state
        .guide
        .steps()
        .iter()
        .find(|step| step.source == session.source)
        .cloned()
    else {
        return;
    };
    if let Some(doc) = state.presentation.document_for_step(&step, &state.store) {
        f(doc);
    }
}

fn commit_annotation_release(state: &mut TimelineWorkspace, point: ImagePoint) {
    let Some(session) = &mut state.annotation_session else {
        return;
    };
    let release = clamp_annotation_point(point, session.width, session.height);
    let source = session.source;
    let tool = session.tool;
    let draft = session.draft.take();
    let text_note = session.text_note.trim().to_string();
    let Some(step) = state
        .guide
        .steps()
        .iter()
        .find(|step| step.source == source)
        .cloned()
    else {
        state.annotation_session = None;
        state.message = Some("Annotation session closed because the step no longer exists.".to_string());
        return;
    };

    if tool == super::annotation::AnnotationTool::Text && text_note.is_empty() {
        state.message = Some("Enter text before placing a text note.".to_string());
        return;
    }

    let Some(doc) = state.presentation.document_for_step(&step, &state.store) else {
        return;
    };

    let error_message = match tool {
        super::annotation::AnnotationTool::Number => {
            let tip = match draft {
                Some(super::annotation::AnnotationDraft::Number { tip, .. }) => tip,
                _ => release,
            };
            doc.document.add_number_callout(tip, release);
            None
        }
        super::annotation::AnnotationTool::Text => {
            doc.document
                .add_text_note(release, text_note)
                .err()
                .map(|error| format!("Text note failed: {error}"))
        }
        super::annotation::AnnotationTool::Redaction => {
            let rect = match draft.and_then(|draft| draft.redaction_rect()) {
                Some(rect) => rect,
                None => rollshot_image_document::ImageRect::from_corners(release, release),
            };
            doc.document
                .add_redaction(rect)
                .err()
                .map(|error| format!("Redaction failed: {error}"))
        }
    };

    if let Some(message) = error_message {
        state.message = Some(message);
    }
}
```

- [ ] **Step 5: Run update tests**

Run: `rtk cargo test -p rollshot-app --features action-guide annotation_`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/annotation.rs
rtk git commit -m "feat(action): commit step text and redaction annotations"
```

---

### Task 4: Add Modal Controls for Tool Modes and Undo/Redo

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**
- Consumes: `AnnotationTool`, new `Message` variants
- Produces: user-facing controls inside the existing annotation modal

- [ ] **Step 1: Add a view smoke test for every tool state**

Extend `view_builds_for_selected_empty_and_discard_states` after the existing annotation modal check:

```rust
let _ = crate::timeline_workspace::update::update(
    &mut annotated,
    Message::AnnotationToolChanged(super::annotation::AnnotationTool::Text),
);
let _ = view(&annotated);
let _ = crate::timeline_workspace::update::update(
    &mut annotated,
    Message::AnnotationToolChanged(super::annotation::AnnotationTool::Redaction),
);
let _ = view(&annotated);
```

- [ ] **Step 2: Run test**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::view::tests::view_builds_for_selected_empty_and_discard_states`

Expected: PASS after implementation; it may fail before the message enum exists if Task 3 is incomplete.

- [ ] **Step 3: Add compact controls**

In `annotation_modal`, before the image container, add:

```rust
let tool = session.tool;
let tool_row = row![
    button(text("Number"))
        .on_press(Message::AnnotationToolChanged(super::annotation::AnnotationTool::Number))
        .style(if tool == super::annotation::AnnotationTool::Number {
            button::primary
        } else {
            button::secondary
        }),
    button(text("Text"))
        .on_press(Message::AnnotationToolChanged(super::annotation::AnnotationTool::Text))
        .style(if tool == super::annotation::AnnotationTool::Text {
            button::primary
        } else {
            button::secondary
        }),
    button(text("Redact"))
        .on_press(Message::AnnotationToolChanged(super::annotation::AnnotationTool::Redaction))
        .style(if tool == super::annotation::AnnotationTool::Redaction {
            button::primary
        } else {
            button::secondary
        }),
    Space::new().width(Length::Fill),
    button(text("Undo"))
        .on_press_maybe(doc.document.can_undo().then_some(Message::AnnotationUndo))
        .style(button::secondary),
    button(text("Redo"))
        .on_press_maybe(doc.document.can_redo().then_some(Message::AnnotationRedo))
        .style(button::secondary),
]
.spacing(6)
.align_y(Alignment::Center);

let text_controls: Element<Message> = if tool == super::annotation::AnnotationTool::Text {
    text_input("Text note", &session.text_note)
        .on_input(Message::AnnotationTextChanged)
        .into()
} else {
    Space::new()
        .width(Length::Fill)
        .height(Length::Fixed(0.0))
        .into()
};
```

Then include `tool_row` and `text_controls` in the modal column between the header row and the image container.

Also change the header's trailing text from the hard-coded `Number callout` to:

```rust
text(match session.tool {
    super::annotation::AnnotationTool::Number => "Number callout",
    super::annotation::AnnotationTool::Text => "Text note",
    super::annotation::AnnotationTool::Redaction => "Opaque redaction",
})
.size(12)
```

- [ ] **Step 4: Run view test**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::view::tests::view_builds_for_selected_empty_and_discard_states`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(action): add annotation modal tool controls"
```

---

### Task 5: Verify Storyboard and Issue Pack Use Flattened Text/Redaction Output

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Consumes: existing `render_timeline_storyboard` and `timeline_issue_pack_action`
- Produces: regression coverage proving all annotation primitives flow into Storyboard and Issue Pack.

- [ ] **Step 1: Write storyboard regression test**

Add this test to `update.rs`:

```rust
#[test]
fn storyboard_render_uses_flattened_text_and_redaction_annotations() {
    let mut state = ws(recording_from_frames());
    let source = state.selected_step().unwrap().source;
    let before = render_timeline_storyboard(
        &state,
        StoryboardOptions {
            max_width: 240,
            max_canvas_pixels: 1_000_000,
            outer_padding: 12,
            card_spacing: 10,
            card_padding: 8,
            show_titles: true,
        },
    )
    .expect("storyboard render before annotation");

    let step = state.selected_step().unwrap().clone();
    let doc = state
        .presentation
        .document_for_step(&step, &state.store)
        .expect("presentation doc");
    doc.document
        .add_text_note(rollshot_image_document::ImagePoint::new(2.0, 2.0), "Note".to_string())
        .unwrap();
    doc.document
        .add_redaction(rollshot_image_document::ImageRect {
            x: 10.0,
            y: 10.0,
            width: 8.0,
            height: 8.0,
        })
        .unwrap();
    assert!(state.presentation.has_annotations(source));
    assert!(
        state
            .presentation
            .doc(source)
            .unwrap()
            .document
            .flatten()
            .pixels()
            .any(|pixel| pixel.0 == [0, 0, 0, 255]),
        "redaction should flatten to opaque black before storyboard render"
    );

    let after = render_timeline_storyboard(
        &state,
        StoryboardOptions {
            max_width: 240,
            max_canvas_pixels: 1_000_000,
            outer_padding: 12,
            card_spacing: 10,
            card_padding: 8,
            show_titles: true,
        },
    )
    .expect("storyboard render");

    assert_ne!(
        before.image.as_raw(),
        after.image.as_raw(),
        "annotated storyboard render should differ from raw keyframe render"
    );
}
```

- [ ] **Step 2: Write Issue Pack regression test**

Add this test to `update.rs`:

```rust
#[test]
fn issue_pack_action_carries_annotated_storyboard_image() {
    let mut state = ws(recording_from_frames());
    state.issue_pack = Some(super::IssuePackDialog {
        review_confirmed: true,
        pending_kind: None,
        include_gif: false,
    });
    let before = {
        let action = timeline_issue_pack_action(&state);
        action.storyboard_image.expect("storyboard before annotation")
    };

    let step = state.selected_step().unwrap().clone();
    let doc = state
        .presentation
        .document_for_step(&step, &state.store)
        .expect("presentation doc");
    doc.document
        .add_redaction(rollshot_image_document::ImageRect {
            x: 4.0,
            y: 4.0,
            width: 12.0,
            height: 12.0,
        })
        .unwrap();

    let action = timeline_issue_pack_action(&state);

    assert!(action.storyboard_image.is_some());
    let image = action.storyboard_image.as_ref().unwrap();
    assert_ne!(
        before.as_raw(),
        image.as_raw(),
        "Issue Pack storyboard image should use flattened annotated keyframes"
    );
}
```

- [ ] **Step 3: Run regression tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide storyboard_render_uses_flattened_text_and_redaction_annotations
rtk cargo test -p rollshot-app --features action-guide issue_pack_action_carries_annotated_storyboard_image
```

Expected: PASS.

- [ ] **Step 4: Run full focused test suite**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "test(action): cover annotated storyboard exports"
```

---

### Task 6: Final Verification

**Files:**
- No code changes unless verification reveals a defect.

**Interfaces:**
- Consumes: all prior tasks
- Produces: verified branch ready for review

- [ ] **Step 1: Run package tests**

Run: `rtk cargo test -p rollshot-app --features action-guide`

Expected: PASS.

- [ ] **Step 2: Run relevant headless core tests**

Run: `rtk cargo test -p rollshot-image-document`

Expected: PASS.

- [ ] **Step 3: Run formatting check**

Run: `rtk cargo fmt --check`

Expected: PASS.

- [ ] **Step 4: Run clippy if time/risk budget allows**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS. If this is too broad because optional OCR or platform dependencies are unavailable, record the exact failure and run the narrower `rtk cargo clippy -p rollshot-app --features action-guide --all-targets -- -D warnings`.

- [ ] **Step 5: Manual UI smoke check**

Run: `rtk cargo run -p rollshot-app --features action-guide`

Expected:
- Timeline Workspace can open from an Action Guide capture path.
- `Annotate Step` opens the modal.
- Number drag creates a callout.
- Text mode requires text and places a note on click.
- Redact mode drag creates an opaque rectangle.
- Undo/redo update the live preview.
- `Preview Storyboard` and `Export Bug Report...` include the flattened annotated storyboard.

- [ ] **Step 6: Commit verification-only fixes if needed**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/annotation.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "fix(action): polish step annotation verification issues"
```

Only run this commit step if verification required follow-up code changes.

---

## Self-Review Notes

- Spec coverage: This plan completes the missing manual primitives from P4 without starting agent callout proposals. Existing number-callout MVP, flattened Storyboard preview/export, Issue Pack storyboard integration, caption preservation, and keyframe-replacement clearing are already in `HEAD`.
- Placeholder scan: No placeholder markers or unbounded generic implementation steps remain.
- Type consistency: New message names, `AnnotationTool`, `AnnotationDraft`, and helper functions are introduced before use in later tasks.
- Scope limit: The plan intentionally excludes moving/resizing annotations, selecting existing annotations, copy-to-clipboard, and agent annotation proposals. Those belong in a later phase after this manual primitive set is stable.

---

## Engineering Review Addendum

### Auto Decisions Applied

**Auto decision D1 - Fix invalid multi-filter cargo test commands**
Context: Several Run steps passed multiple test filters to one `cargo test` invocation.
ELI10: Cargo test filtering is one search string at a time. If a plan tells an executor to pass several bare filter strings, the command can fail before any test runs.
Stakes if we pick wrong: The executor wastes time debugging the plan instead of the feature.
Recommendation: 1A, split into separate commands or use one broad module/prefix filter, because it is explicit and matches Cargo's CLI.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Split/broaden filters (recommended) - human: ~5 min / AI: ~1 min
  + Concrete commands run in Cargo today.
  - Slightly more lines in the plan.
B) Leave commands as-is - human: 0 min / AI: 0 min
  + No edit.
  - Execution can fail for reasons unrelated to product code.
Net: More command lines are cheaper than ambiguous test execution.

**Auto decision D2 - Make Task 2 red-green instead of adding a passing smoke test first**
Context: The original Canvas task admitted its first test would pass before implementation.
ELI10: A test that already passes does not prove the next code change did anything. The plan needs at least one failing test before the helper/rendering change.
Stakes if we pick wrong: The Canvas task can silently skip redaction draft behavior and still look tested.
Recommendation: 2A, introduce and test a small `draft_annotation` helper first, because it creates a real red-green loop without inventing a custom renderer test harness.
Completeness: A=9/10, B=5/10
Pros / cons:
A) Add helper red test (recommended) - human: ~25 min / AI: ~5 min
  + Tests draft-to-annotation conversion used by the draw path.
  - Still cannot pixel-test iced Canvas headlessly.
B) Keep passing construction test first - human: 0 min / AI: 0 min
  + Minimal plan churn.
  - Violates the plan's own TDD contract.
Net: The helper is a small real seam that improves both testability and draw-path DRYness.

**Auto decision D3 - Avoid mutable-borrow conflicts in the update helper**
Context: The original `commit_annotation_release` snippet assigned `state.message` while a mutable borrow of the presentation document could still be live.
ELI10: Rust will reject code that edits two overlapping parts of the same state if it cannot prove the first borrow is done. The fix is to validate early, do the document mutation, collect an error string, then set the banner afterward.
Stakes if we pick wrong: The executor hits a compiler error in the central update task.
Recommendation: 3A, stage validation and error message assignment outside the document borrow, because explicit lifetimes beat clever NLL reliance.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Stage errors outside the borrow (recommended) - human: ~20 min / AI: ~5 min
  + More likely to compile and easier to read.
  - Slightly more local variables.
B) Leave the helper as-is - human: 0 min / AI: 0 min
  + Shorter snippet.
  - Borrow checker failure risk is high.
Net: The staged version is boring and robust.

**Auto decision D4 - Add negative tests for empty text and zero-area redaction**
Context: The plan added happy-path text/redaction tests but did not test the recoverable error paths.
ELI10: Users will click with empty text or click instead of drag for redaction. Those should show clear messages and avoid committing broken annotations.
Stakes if we pick wrong: The annotation modal can silently create nothing or show confusing behavior.
Recommendation: 4A, add both negative tests in Task 3, because AI execution makes this cheap and these are expected user mistakes.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Add negative tests (recommended) - human: ~30 min / AI: ~8 min
  + Covers common error paths and user-visible banners.
  - Adds two tests to an already busy task.
B) Happy path only - human: 0 min / AI: 0 min
  + Faster initial implementation.
  - Leaves common mistakes unprotected.
Net: This is the kind of completeness that prevents support churn.

**Auto decision D5 - Replace brittle black-pixel storyboard assertions**
Context: The original storyboard tests looked for any opaque black pixel in the final storyboard.
ELI10: Storyboards can contain black text or other black pixels even when redaction did not flow through. Comparing before and after renders proves annotations changed the output.
Stakes if we pick wrong: A regression test can pass while annotated Storyboard export is broken.
Recommendation: 5A, compare unannotated and annotated render bytes and separately assert the document flatten contains redaction black, because it tests the integration point without relying on coordinates.
Completeness: A=9/10, B=5/10
Pros / cons:
A) Before/after comparison (recommended) - human: ~20 min / AI: ~5 min
  + Fails if the annotated image is not used.
  - Does not pinpoint the exact redaction coordinate in the storyboard.
B) Any-black-pixel assertion - human: 0 min / AI: 0 min
  + Very simple.
  - Can pass for unrelated text/card pixels.
Net: Before/after render comparison is the right small-diff test here.

### Test Diagram

```text
User gesture in annotation modal
        |
        v
timeline_workspace::Message
        |
        v
StepAnnotationSession draft/tool/text
        |
        v
ActionGuidePresentation[CandidateId] -> ImageDocument
        |
        +--> live Canvas overlay uses annotation_shapes(...)
        |
        +--> ImageDocument::flatten()
                 |
                 v
        rollshot_action::render_storyboard_steps(...)
                 |
                 +--> Preview Storyboard / Export Storyboard
                 |
                 +--> Issue Pack action-guide/storyboard.png
```

### Test Coverage Table

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
| --- | --- | --- | --- | --- |
| Task 1 / annotation tool defaults | yes | no | no | no |
| Task 1 / redaction draft normalizes inverted drag | yes | no | no | no |
| Task 2 / redaction draft converts to opaque redaction annotation | yes | no | no | no |
| Task 2 / mixed annotation Canvas construction | yes | no | no | no |
| Task 3 / text note commits on click | yes | yes, update state | no | no |
| Task 3 / redaction commits on drag | yes | yes, update state | no | no |
| Task 3 / undo and redo mutate current document | yes | yes, update state | no | no |
| Task 3 / empty text shows message and commits nothing | yes | yes, update state | no | no |
| Task 3 / zero-area redaction shows message and commits nothing | yes | yes, update state | no | no |
| Task 4 / modal view builds for Number/Text/Redaction tools | no | no | yes, view smoke | no |
| Task 5 / Storyboard render uses flattened annotated keyframes | yes | yes | no | no |
| Task 5 / Issue Pack source carries annotated storyboard image | yes | yes | no | no |
| Task 6 / real UI workflow | no | no | no | yes |

### Failure Modes

| Codepath | Realistic failure | Covered by plan | Error handling/user signal |
| --- | --- | --- | --- |
| Open annotation modal | Selected keyframe missing from `FrameStore` | Existing `AnnotateStepRequested` tests cover normal open; missing-keyframe path is existing behavior | Existing banner: `Cannot annotate this step because its keyframe is unavailable.` |
| Text note commit | Empty or whitespace-only text | Task 3 negative test | Banner: `Enter text before placing a text note.` |
| Redaction commit | Click creates zero-area rectangle | Task 3 negative test | Banner: `Redaction failed: ...` |
| Step deleted during modal | Session source no longer exists | Existing update path remains in Task 3 helper | Banner: `Annotation session closed because the step no longer exists.` |
| Keyframe replaced after annotation | Stale annotation coordinates | Existing replace-keyframe behavior remains in scope | Banner: `Step annotations were cleared because the keyframe changed.` |
| Storyboard render with annotations | Missing keyframe while rendering | Existing `render_timeline_storyboard` returns `StoryboardError::KeyframeMissing` | Existing preview/export failure banner |
| Issue Pack storyboard generation | Annotated storyboard render fails | Existing `timeline_issue_pack_action` falls back to `None`; `issue_pack` writes warning if export fails | Manifest/banners include storyboard warning when file generation fails |

Critical gaps flagged: none after the D4 and D5 edits.

### What Already Exists

- `rollshot-image-document::ImageDocument` already owns source pixels, annotation graph, undo/redo, and `flatten()`. This plan reuses it.
- `rollshot-image-document::annotation_shapes` already defines framework-neutral render primitives for number, text, and redaction. This plan extends the iced Canvas adapter to use all variants instead of creating another shape model.
- `TimelineWorkspace::presentation` already stores per-step annotation documents keyed by `CandidateId`. This plan extends the existing state rather than adding persistence or a parallel map.
- `render_timeline_storyboard` already flattens annotated documents into `StoryboardStep` images. This plan adds regression coverage and keeps that pipeline.
- `issue_pack::ActionGuideExportSource::storyboard_image` already accepts a pre-rendered storyboard. This plan verifies annotated Storyboard output flows through it.

### NOT in Scope

- Moving, resizing, selecting, or deleting individual existing annotations: deferred because the goal is primitive completion, not a full editor.
- Agent-proposed callouts/redactions: deferred until manual annotation primitives are stable.
- Copy Storyboard to clipboard: deferred because export/preview/Issue Pack coverage is the current share path.
- Persisting annotation documents across app restarts: deferred because current Action Guide review state is session-owned.
- Redacting original Action Guide keyframes in Issue Pack: deferred to avoid implying the whole Issue Pack is safe-redacted.
- Reworking Timeline Workspace layout or replacing buttons with menus/icons: deferred to keep this phase surgical.

### Parallelization Strategy

Sequential execution, no parallelization opportunity. Tasks 1-5 all touch `crates/rollshot-app/src/timeline_workspace/` and depend on the previous task's message/state shape; parallel subagents would create avoidable merge conflicts.
