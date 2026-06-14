# Secure Redaction Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Result Workspace Copy, Save As, Reveal, and original-access actions enforce and clearly communicate the approved safe-sharing contract whenever opaque redactions exist.

**Architecture:** Add a small pure `secure_sharing` policy module that derives all safe-sharing state, labels, routes, confirmation copy, filename behavior, and overwrite protection from `ResultDocument`. Keep `ImageDocument` and opaque rasterization unchanged; `update.rs` executes policy decisions, `view.rs` renders them, and `mod.rs` stores only pending confirmation state plus existing transient messages.

**Tech Stack:** Rust 2021, iced 0.14, image 0.25, existing `rollshot-image-document`, built-in Rust path/filesystem APIs.

---

## File Structure

- Create `crates/rollshot-app/src/result_workspace/secure_sharing.rs`
  - Independently tested safe-sharing policy. All label/route/disclosure
    derivation is pure; the single impurity is `safe_export_overwrites_source`,
    which calls `std::fs::canonicalize` to catch symlink/syntactic source
    aliases (see Task 1 Step 3 note). Tests of that function therefore touch a
    real temp filesystem.
  - Owns user-facing safe/original labels and confirmation copy so the UI never invents security language.
  - Owns reveal routing, `-redacted` filename generation, and source-path overwrite detection.
- Modify `crates/rollshot-app/src/result_workspace/mod.rs`
  - Register the policy module.
  - Store the pending unredacted action.
  - Expose derived policy helpers and apply safe-vs-general save completion messages.
- Modify `crates/rollshot-app/src/result_workspace/update.rs`
  - Route Copy, Save As, Copy Original, and Reveal through the policy.
  - Capture whether asynchronous Copy/Save completion was a safe output.
  - Reject a safe export before writing when it targets the unredacted source.
- Modify `crates/rollshot-app/src/result_workspace/view.rs`
  - Render contextual action labels, retained-original disclosure, and the unredacted-action confirmation modal.
- Modify `crates/rollshot-app/src/result_workspace/actions.rs`
  - Add an encoded-PNG boundary test proving flattened output is the only pixel payload and no ancillary metadata chunks are emitted.

No changes are planned for `rollshot-image-document`: its existing opaque-redaction rasterization and immutable source model already satisfy the rendering contract.

### Task 1: Add The Pure Safe-Sharing Policy

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/secure_sharing.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs:1-8`

- [ ] **Step 1: Register the empty module and write failing policy tests**

Add `mod secure_sharing;` beside the existing Result Workspace modules. Create `secure_sharing.rs` with a `#[cfg(test)]` module that builds saved and unsaved `ResultDocument` values and asserts:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_image_document::ImageRect;
    use std::path::{Path, PathBuf};

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]))
    }

    fn saved() -> ResultDocument {
        ResultDocument::saved(image(), PathBuf::from("/tmp/original.png"))
    }

    fn add_redaction(document: &mut ResultDocument) -> rollshot_image_document::AnnotationId {
        document
            .image
            .add_redaction(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            })
            .unwrap()
    }

    #[test]
    fn derived_redaction_state_drives_labels_and_disclosure() {
        let mut document = saved();
        assert!(!has_secure_redactions(&document));
        assert_eq!(copy_label(&document), "Copy");
        assert_eq!(save_label(&document), "Save As");
        assert_eq!(copy_original_label(&document), "Copy Original");
        assert_eq!(retained_original_disclosure(&document), None);

        let id = add_redaction(&mut document);
        assert!(has_secure_redactions(&document));
        assert_eq!(copy_label(&document), "Copy Safe Image");
        assert_eq!(save_label(&document), "Save Safe Image As");
        assert_eq!(
            copy_original_label(&document),
            "Copy Unredacted Original\u{2026}"
        );
        assert_eq!(
            retained_original_disclosure(&document),
            Some(RETAINED_ORIGINAL_DISCLOSURE)
        );

        document.image.undo();
        assert!(!has_secure_redactions(&document));
        document.image.redo();
        assert!(has_secure_redactions(&document));
        document.image.delete_annotation(id).unwrap();
        assert!(!has_secure_redactions(&document));
    }

    #[test]
    fn unsaved_redacted_document_has_no_retained_original_disclosure() {
        let mut document = ResultDocument::unsaved(image());
        add_redaction(&mut document);
        assert_eq!(retained_original_disclosure(&document), None);
    }

    #[test]
    fn reveal_policy_prefers_last_safe_export_then_warns_for_original() {
        let mut document = saved();
        add_redaction(&mut document);
        assert_eq!(
            reveal_action(&document),
            RevealAction::ConfirmUnredacted(Path::new("/tmp/original.png"))
        );

        document.last_export_path = Some(PathBuf::from("/tmp/safe.png"));
        assert_eq!(
            reveal_action(&document),
            RevealAction::Immediate {
                label: "Reveal Last Safe Export",
                path: Path::new("/tmp/safe.png"),
            }
        );
    }

    #[test]
    fn safe_filename_inserts_redacted_before_extension() {
        let mut document = saved();
        add_redaction(&mut document);
        assert_eq!(default_save_name(&document), "original-redacted.png");

        let document = ResultDocument::unsaved(image());
        assert!(!default_save_name(&document).contains("-redacted"));
    }

    #[test]
    fn source_path_is_rejected_only_for_safe_exports() {
        let mut document = saved();
        let source = Path::new("/tmp/original.png");
        assert!(!safe_export_overwrites_source(&document, source));
        add_redaction(&mut document);
        assert!(safe_export_overwrites_source(&document, source));
        assert!(!safe_export_overwrites_source(
            &document,
            Path::new("/tmp/other.png")
        ));
    }

    #[test]
    fn canonical_source_alias_is_rejected_for_safe_export() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.png");
        std::fs::write(&source, b"source").unwrap();
        let mut document = ResultDocument::saved(image(), source.clone());
        add_redaction(&mut document);

        let alias = dir.path().join(".").join("original.png");
        assert_ne!(source, alias);
        assert!(safe_export_overwrites_source(&document, &alias));
    }

    #[test]
    fn user_facing_policy_copy_never_says_secure() {
        let mut document = saved();
        add_redaction(&mut document);
        for text in [
            RETAINED_ORIGINAL_DISCLOSURE,
            SAFE_EXPORT_OVERWRITE_ERROR,
            COPY_SAFE_SUCCESS,
            SAVE_SAFE_SUCCESS,
            copy_label(&document),
            save_label(&document),
            copy_original_label(&document),
            reveal_action(&document).label(),
            UnredactedAction::CopyOriginal.prompt(),
            UnredactedAction::CopyOriginal.confirm_label(),
            UnredactedAction::RevealOriginal.prompt(),
            UnredactedAction::RevealOriginal.confirm_label(),
        ] {
            assert!(!text.to_ascii_lowercase().contains("secure"), "{text}");
        }
    }
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app secure_sharing::tests
```

Expected: compilation fails because the policy constants, enums, and functions do not exist.

- [ ] **Step 3: Implement the minimal pure policy**

Implement the following public-to-sibling-module surface in `secure_sharing.rs`:

```rust
use std::path::Path;

use rollshot_image_document::Annotation;

use super::document::ResultDocument;

pub(crate) const RETAINED_ORIGINAL_DISCLOSURE: &str =
    "Unredacted original remains saved. Safe exports are flattened.";
pub(crate) const SAFE_EXPORT_OVERWRITE_ERROR: &str =
    "Safe export cannot overwrite the unredacted original. Choose another location.";
pub(crate) const COPY_SAFE_SUCCESS: &str = "Copied safe flattened image";
pub(crate) const SAVE_SAFE_SUCCESS: &str = "Saved safe flattened image";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnredactedAction {
    CopyOriginal,
    RevealOriginal,
}

impl UnredactedAction {
    pub(crate) fn prompt(self) -> &'static str {
        match self {
            Self::CopyOriginal => {
                "Copy the unredacted original? This will expose content hidden by redactions."
            }
            Self::RevealOriginal => {
                "Reveal the unredacted original? This file contains content hidden by redactions."
            }
        }
    }

    pub(crate) fn confirm_label(self) -> &'static str {
        match self {
            Self::CopyOriginal => "Copy Unredacted Original",
            Self::RevealOriginal => "Reveal Unredacted Original",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealAction<'a> {
    Disabled,
    Immediate { label: &'static str, path: &'a Path },
    ConfirmUnredacted(&'a Path),
}

impl RevealAction<'_> {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Reveal",
            Self::Immediate { label, .. } => label,
            Self::ConfirmUnredacted(_) => "Reveal Unredacted Original\u{2026}",
        }
    }
}

pub(crate) fn has_secure_redactions(document: &ResultDocument) -> bool {
    document
        .image
        .annotations()
        .iter()
        .any(|annotation| matches!(annotation, Annotation::OpaqueRedaction { .. }))
}

pub(crate) fn copy_label(document: &ResultDocument) -> &'static str {
    if has_secure_redactions(document) {
        "Copy Safe Image"
    } else {
        "Copy"
    }
}

pub(crate) fn save_label(document: &ResultDocument) -> &'static str {
    if has_secure_redactions(document) {
        "Save Safe Image As"
    } else {
        "Save As"
    }
}

pub(crate) fn copy_original_label(document: &ResultDocument) -> &'static str {
    if has_secure_redactions(document) {
        "Copy Unredacted Original\u{2026}"
    } else {
        "Copy Original"
    }
}

pub(crate) fn retained_original_disclosure(document: &ResultDocument) -> Option<&'static str> {
    (has_secure_redactions(document) && document.source_path.is_some())
        .then_some(RETAINED_ORIGINAL_DISCLOSURE)
}

pub(crate) fn reveal_action(document: &ResultDocument) -> RevealAction<'_> {
    if has_secure_redactions(document) {
        if let Some(path) = document.last_export_path.as_deref() {
            return RevealAction::Immediate {
                label: "Reveal Last Safe Export",
                path,
            };
        }
        return document
            .source_path
            .as_deref()
            .map(RevealAction::ConfirmUnredacted)
            .unwrap_or(RevealAction::Disabled);
    }

    document
        .reveal_path()
        .map(|path| RevealAction::Immediate {
            label: "Reveal",
            path,
        })
        .unwrap_or(RevealAction::Disabled)
}

pub(crate) fn default_save_name(document: &ResultDocument) -> String {
    let base = super::document::default_save_name(document);
    if !has_secure_redactions(document) {
        return base;
    }

    let path = Path::new(&base);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_else(|| base.as_str().into());
    match path.extension().map(|extension| extension.to_string_lossy()) {
        Some(extension) => format!("{stem}-redacted.{extension}"),
        None => format!("{stem}-redacted"),
    }
}

pub(crate) fn safe_export_overwrites_source(
    document: &ResultDocument,
    destination: &Path,
) -> bool {
    if !has_secure_redactions(document) {
        return false;
    }
    let Some(source) = document.source_path.as_deref() else {
        return false;
    };
    if source == destination {
        return true;
    }
    match (
        std::fs::canonicalize(source),
        std::fs::canonicalize(destination),
    ) {
        (Ok(source), Ok(destination)) => source == destination,
        _ => false,
    }
}
```

This comparison intentionally combines direct path equality with
`canonicalize` when both paths exist. It catches the normal same-path case plus
syntactic and symlink aliases. It does not claim to detect every filesystem
alias such as a separate hard link, which is outside the approved safe-sharing
contract and must not be described as secure deletion.

- [ ] **Step 4: Run the focused tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-app secure_sharing::tests
```

Expected: all `secure_sharing::tests` pass.

- [ ] **Step 5: Commit the policy module**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/secure_sharing.rs
rtk git commit -m "feat(app): add secure sharing policy"
```

### Task 2: Route Safe Copy And Save Through The Policy

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs:65-202` (and its
  `#[cfg(test)]` module: the existing `apply_save_as` test callers gain a
  `false` argument)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:26-55,386-455`
  (and its `#[cfg(test)]` module: `save_completion_marks_the_written_state_not_newer_edits`
  passes the new `safe_output` field)

(Line ranges are approximate orientation, not exact; verify against the current
file before editing.)

- [ ] **Step 1: Write failing Copy/Save routing tests**

Add focused tests in `update.rs` and `mod.rs`:

```rust
use super::super::secure_sharing::{
    COPY_SAFE_SUCCESS, SAFE_EXPORT_OVERWRITE_ERROR, SAVE_SAFE_SUCCESS,
};

#[test]
fn safe_copy_completion_uses_safe_message() {
    let mut state = saved_workspace();
    state
        .document
        .image
        .add_redaction(ImageRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        })
        .unwrap();

    let _ = update(
        &mut state,
        Message::CopyFinished {
            result: Ok(()),
            safe_output: true,
        },
    );
    assert_eq!(state.message_text().as_deref(), Some(COPY_SAFE_SUCCESS));
}

#[test]
fn safe_save_rejects_source_before_write_and_preserves_state() {
    let mut state = saved_workspace();
    state
        .document
        .image
        .add_redaction(ImageRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        })
        .unwrap();
    let state_id = state.document.image.state_id();

    let _ = update(
        &mut state,
        Message::SavePathChosen(Some(PathBuf::from("/tmp/result.png"))),
    );

    assert_eq!(
        state.message_text().as_deref(),
        Some(SAFE_EXPORT_OVERWRITE_ERROR)
    );
    assert_eq!(state.document.image.state_id(), state_id);
    assert!(state.document.last_export_path.is_none());
    assert!(state.annotations_dirty());
}

#[test]
fn safe_save_completion_records_safe_message_and_path() {
    let mut state = saved_workspace();
    state
        .document
        .image
        .add_redaction(ImageRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        })
        .unwrap();
    let state_id = state.document.image.state_id();

    state.apply_save_as(
        Ok(Some(PathBuf::from("/tmp/safe.png"))),
        state_id,
        true,
    );

    assert_eq!(state.message_text().as_deref(), Some(SAVE_SAFE_SUCCESS));
    assert_eq!(
        state.document.last_export_path.as_deref(),
        Some(Path::new("/tmp/safe.png"))
    );
    // Spec §Copy And Save: a successful safe save advances the saved-state
    // marker, so the document is no longer dirty relative to the written state.
    assert!(!state.annotations_dirty());
}
```

- [ ] **Step 2: Run focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::update::tests::safe_
rtk cargo test -p rollshot-app result_workspace::tests::safe_
```

Expected: compilation fails because `CopyFinished` and `apply_save_as` do not carry `safe_output`, and source overwrite is not rejected.

- [ ] **Step 3: Capture safe-output identity in asynchronous messages**

Change the message variants:

```rust
CopyFinished {
    result: Result<(), String>,
    safe_output: bool,
},
SaveFinished {
    result: Result<PathBuf, String>,
    saved_state_id: u64,
    safe_output: bool,
},
```

In `Message::Copy`, keep the existing `commit_text_draft(state);` call first, then compute `let safe_output = state.has_secure_redactions();` (after the commit, so a just-committed annotation is reflected), copy the existing flattened payload, and map the result into the new variant. In `Message::CopyFinished`, use `COPY_SAFE_SUCCESS` for safe output and `"Copied image"` otherwise.

In `ResultWorkspace`, add:

```rust
pub(crate) fn has_secure_redactions(&self) -> bool {
    secure_sharing::has_secure_redactions(&self.document)
}
```

Change `apply_save_as` to accept `safe_output: bool`; on success use `SAVE_SAFE_SUCCESS` when true and the existing `Saved to {path}` text otherwise. Update all existing callers and tests to pass `false` where they represent general Save As.

- [ ] **Step 4: Route Save As naming, payload, overwrite rejection, and completion**

Use `secure_sharing::default_save_name(&state.document)` when opening the dialog.

In `Message::SavePathChosen(Some(path))`:

```rust
let safe_output = state.has_secure_redactions();
if secure_sharing::safe_export_overwrites_source(&state.document, &path) {
    state.message = Some(InlineMessage::Error(
        secure_sharing::SAFE_EXPORT_OVERWRITE_ERROR.to_string(),
    ));
    return Task::none();
}
let image = save_payload(state);
let saved_state_id = state.document.image.state_id();
Task::perform(
    async move { super::actions::write_save_as(&image, &path) },
    move |result| Message::SaveFinished {
        result,
        saved_state_id,
        safe_output,
    },
)
```

Pass `safe_output` into `apply_save_as` from `Message::SaveFinished`.

- [ ] **Step 5: Run focused and package tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::update::tests
rtk cargo test -p rollshot-app result_workspace::tests
rtk cargo test -p rollshot-app
```

Expected: all Result Workspace and `rollshot-app` tests pass.

- [ ] **Step 6: Commit safe Copy/Save routing**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(app): enforce safe redaction exports"
```

### Task 3: Require Confirmation For Unredacted Original Actions

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs:65-140`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:26-98,386-470,967-1012,1173-1182`

- [ ] **Step 1: Write failing confirmation and reveal-routing tests**

Add these tests in `update.rs`:

```rust
use super::super::secure_sharing::UnredactedAction;

#[test]
fn redacted_copy_original_requires_fresh_confirmation() {
    let mut state = saved_workspace();
    state
        .document
        .image
        .add_redaction(ImageRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        })
        .unwrap();

    let _ = update(&mut state, Message::CopyOriginal);
    assert_eq!(
        state.pending_unredacted_action,
        Some(UnredactedAction::CopyOriginal)
    );
    let _ = update(&mut state, Message::CancelUnredactedAction);
    assert_eq!(state.pending_unredacted_action, None);
    let _ = update(&mut state, Message::CopyOriginal);
    assert_eq!(
        state.pending_unredacted_action,
        Some(UnredactedAction::CopyOriginal)
    );
    let _ = update(&mut state, Message::ConfirmUnredactedAction);
    assert_eq!(state.pending_unredacted_action, None);
    let _ = update(&mut state, Message::CopyOriginal);
    assert_eq!(
        state.pending_unredacted_action,
        Some(UnredactedAction::CopyOriginal)
    );
}

#[test]
fn redacted_reveal_original_requires_confirmation() {
    let mut state = saved_workspace();
    state
        .document
        .image
        .add_redaction(ImageRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        })
        .unwrap();

    let _ = update(&mut state, Message::Reveal);
    assert_eq!(
        state.pending_unredacted_action,
        Some(UnredactedAction::RevealOriginal)
    );
}

#[test]
fn request_close_clears_unredacted_confirmation_before_close_routing() {
    let mut state = saved_workspace();
    state.pending_unredacted_action = Some(UnredactedAction::CopyOriginal);
    let _ = update(&mut state, Message::RequestClose);
    assert_eq!(state.pending_unredacted_action, None);
}

#[test]
fn escape_cancels_pending_unredacted_confirmation_without_closing() {
    let mut state = saved_workspace();
    state.pending_unredacted_action = Some(UnredactedAction::RevealOriginal);
    let _ = update(&mut state, Message::EscapePressed);
    assert_eq!(state.pending_unredacted_action, None);
    // Esc cancelled the blocking dialog; it must not have escalated to close.
    assert!(state.pending_discard.is_none());
}
```

- [ ] **Step 2: Run focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::update::tests::redacted_
rtk cargo test -p rollshot-app result_workspace::update::tests::request_close_clears_
rtk cargo test -p rollshot-app result_workspace::update::tests::escape_cancels_
```

Expected: compilation fails because pending unredacted confirmation state and messages do not exist.

- [ ] **Step 3: Add pending confirmation state and messages**

Add to `ResultWorkspace`:

```rust
pub pending_unredacted_action: Option<secure_sharing::UnredactedAction>,
```

Initialize it to `None`.

Add messages:

```rust
ConfirmUnredactedAction,
CancelUnredactedAction,
```

Update `RequestClose` to clear `pending_unredacted_action` before close routing. `CancelUnredactedAction` clears it and does nothing else.

Update `Message::EscapePressed` so that a pending unredacted-action
confirmation is the highest-priority Escape target: if
`pending_unredacted_action.is_some()`, clear it and return `Task::none()`
*before* the existing copy-menu / draft / drag / selection / close branches.
The confirmation is a blocking dialog (spec §Confirmation And Error Behavior),
so Esc must cancel it rather than fall through to close routing.

- [ ] **Step 4: Route Copy Original and Reveal through confirmation**

For `Message::CopyOriginal`, close the copy menu and commit text. If redactions exist, set `pending_unredacted_action` to `CopyOriginal` and return without touching the clipboard. Otherwise retain the existing direct original-copy behavior.

For `Message::Reveal`, keep the existing `commit_text_draft(state);` call first
(the current handler commits an open text draft before revealing; an existing
test, `clicking_non_canvas_controls_commits_the_open_draft`, depends on this and
would otherwise regress), then match
`secure_sharing::reveal_action(&state.document)`:

```rust
commit_text_draft(state);
match secure_sharing::reveal_action(&state.document) {
    secure_sharing::RevealAction::Disabled => Task::none(),
    secure_sharing::RevealAction::Immediate { path, .. } => {
        Task::done(Message::RevealFinished(super::actions::reveal(path)))
    }
    secure_sharing::RevealAction::ConfirmUnredacted(_) => {
        state.pending_unredacted_action = Some(UnredactedAction::RevealOriginal);
        Task::none()
    }
}
```

For `ConfirmUnredactedAction`, take and clear the pending action, then use this
complete routing:

```rust
match state.pending_unredacted_action.take() {
    Some(UnredactedAction::CopyOriginal) => {
        let result = super::actions::copy_image(&copy_original_payload(state));
        Task::done(Message::CopyFinished {
            result,
            safe_output: false,
        })
    }
    Some(UnredactedAction::RevealOriginal) => {
        let Some(path) = state.document.source_path.as_deref() else {
            return Task::none();
        };
        Task::done(Message::RevealFinished(super::actions::reveal(path)))
    }
    None => Task::none(),
}
```

Do not route confirmation through `Message::CopyOriginal` or `Message::Reveal`, because that would reopen the confirmation.

- [ ] **Step 5: Run focused and package tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::update::tests
rtk cargo test -p rollshot-app
```

Expected: all tests pass.

- [ ] **Step 6: Commit unredacted-action confirmation routing**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(app): confirm unredacted original actions"
```

### Task 4: Render Contextual Labels, Disclosure, And Confirmation Modal

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/view.rs:1-183,321-389`
- Modify: `crates/rollshot-app/src/result_workspace/secure_sharing.rs`

- [ ] **Step 1: Add failing view-policy tests**

Extend `secure_sharing.rs` tests to cover the exact visible states:

```rust
#[test]
fn reveal_labels_cover_disabled_general_original_and_last_safe_export() {
    let unsaved = ResultDocument::unsaved(image());
    assert_eq!(reveal_action(&unsaved).label(), "Reveal");

    let saved = saved();
    assert_eq!(reveal_action(&saved).label(), "Reveal");

    let mut redacted = saved();
    add_redaction(&mut redacted);
    assert_eq!(
        reveal_action(&redacted).label(),
        "Reveal Unredacted Original\u{2026}"
    );

    redacted.last_export_path = Some(PathBuf::from("/tmp/safe.png"));
    assert_eq!(
        reveal_action(&redacted).label(),
        "Reveal Last Safe Export"
    );
}
```

Add a style test in `view.rs` proving the unredacted confirmation reuses a solid dialog and translucent scrim:

```rust
#[test]
fn unredacted_confirmation_uses_blocking_modal_styles() {
    let dialog = confirmation_dialog_style(&iced::Theme::Dark);
    let scrim = confirmation_scrim_style(&iced::Theme::Dark);
    assert!(dialog.background.is_some());
    assert!(scrim.background.is_some());
}
```

- [ ] **Step 2: Run focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app secure_sharing::tests::reveal_labels_
rtk cargo test -p rollshot-app result_workspace::view::tests::unredacted_
```

Expected: the style helper functions do not exist.

- [ ] **Step 3: Render contextual toolbar and disclosure**

In `toolbar`, derive labels only from policy functions:

```rust
let copy_label = super::secure_sharing::copy_label(&state.document);
let save_label = super::secure_sharing::save_label(&state.document);
```

Use those labels for the primary buttons. Keep the tool tooltip named `Redact`.

Change `copy_menu` to accept `state` and render
`secure_sharing::copy_original_label(&state.document)`.

Change `reveal_button` to use `secure_sharing::reveal_action(&state.document)`,
its `label()`, and enable the button unless the action is `Disabled`.

Add a plain, low-key disclosure row immediately below the toolbar and above the
transient message row:

```rust
fn retained_original_disclosure(state: &ResultWorkspace) -> Element<'_, Message> {
    match super::secure_sharing::retained_original_disclosure(&state.document) {
        Some(disclosure) => container(text(disclosure).size(12))
            .width(Length::Fill)
            .padding([2, 4])
            .into(),
        None => Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .into(),
    }
}
```

Build the main column as:

```rust
column![
    toolbar,
    retained_original_disclosure(state),
    message_area,
    workspace_row,
    status
]
```

- [ ] **Step 4: Render the blocking unredacted-action confirmation**

Generalize the existing modal style functions to:

```rust
fn confirmation_dialog_style(theme: &iced::Theme) -> container::Style {
    container::rounded_box(theme)
}

fn confirmation_scrim_style(_theme: &iced::Theme) -> container::Style {
    container::Style::default().background(Color {
        a: 0.8,
        ..Color::BLACK
    })
}
```

Update the discard modal to use them. Because this renames
`discard_dialog_style` / `discard_scrim_style`, update their two existing
callers in the `view.rs` test module (`discard_dialog_has_solid_background` and
`discard_scrim_is_translucent_black`) to the new
`confirmation_dialog_style` / `confirmation_scrim_style` names, or the package
will no longer compile. Add:

```rust
fn unredacted_action_modal<'a>(
    base: Element<'a, Message>,
    action: super::secure_sharing::UnredactedAction,
) -> Element<'a, Message> {
    let dialog = container(
        column![
            text(action.prompt()),
            row![
                button(text("Cancel")).on_press(Message::CancelUnredactedAction),
                button(text(action.confirm_label())).on_press(Message::ConfirmUnredactedAction),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .padding(20)
    .style(confirmation_dialog_style);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog))
                .style(confirmation_scrim_style)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::ModalScrimPressed),
    );
    iced::widget::stack![base, scrim].into()
}
```

Apply modal precedence in `view`: discard confirmation first, otherwise
unredacted-action confirmation, otherwise the base layout. Apply the copy menu
only when neither blocking modal is open.

- [ ] **Step 5: Run focused and package tests**

Run:

```bash
rtk cargo test -p rollshot-app secure_sharing::tests
rtk cargo test -p rollshot-app result_workspace::view::tests
rtk cargo test -p rollshot-app
```

Expected: all tests pass.

- [ ] **Step 6: Commit contextual safe-sharing UI**

```bash
rtk git add crates/rollshot-app/src/result_workspace/view.rs crates/rollshot-app/src/result_workspace/secure_sharing.rs
rtk git commit -m "feat(app): show safe sharing state"
```

### Task 5: Verify The Flattened PNG Boundary And Complete Acceptance

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/actions.rs:70-105`
- Verify: `crates/rollshot-image-document/src/flatten.rs`

- [ ] **Step 1: Add an encoded-PNG boundary test**

Add a test helper and test under `actions.rs`:

```rust
fn png_chunk_types(bytes: &[u8]) -> Vec<[u8; 4]> {
    let mut offset = 8;
    let mut chunks = Vec::new();
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let chunk_type: [u8; 4] = bytes[offset + 4..offset + 8].try_into().unwrap();
        chunks.push(chunk_type);
        offset += 12 + length;
        if chunk_type == *b"IEND" {
            break;
        }
    }
    chunks
}

#[test]
fn save_as_png_contains_only_flattened_pixels_and_core_png_chunks() {
    use rollshot_image_document::{ImageDocument, ImageRect};

    let mut document = ImageDocument::new(RgbaImage::from_pixel(
        4,
        4,
        image::Rgba([10, 20, 30, 255]),
    ));
    document
        .add_redaction(ImageRect {
            x: 0.0,
            y: 0.0,
            width: 2.0,
            height: 2.0,
        })
        .unwrap();
    let flattened = document.flatten();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("safe.png");
    write_save_as(&flattened, &path).unwrap();

    let decoded = image::open(&path).unwrap().to_rgba8();
    assert_eq!(decoded.as_raw(), flattened.as_raw());
    assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, 255]);
    assert_ne!(
        decoded.get_pixel(0, 0).0,
        document.source().get_pixel(0, 0).0
    );

    let chunks = png_chunk_types(&std::fs::read(path).unwrap());
    assert!(chunks
        .iter()
        .all(|chunk| matches!(chunk.as_slice(), b"IHDR" | b"IDAT" | b"IEND")));
}
```

- [ ] **Step 2: Run the focused test and verify the encoder boundary**

Run:

```bash
rtk cargo test -p rollshot-app save_as_png_contains_only_flattened_pixels_and_core_png_chunks -- --nocapture
```

Expected: PASS if the current `image` PNG encoder emits only core chunks. If it
emits a standard non-sensitive ancillary chunk, inspect it, document why it is
safe, and narrow the assertion to explicitly allow only that chunk. Do not
allow text, EXIF, or unknown ancillary chunks.

- [ ] **Step 3: Run automated verification**

Run:

```bash
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-app
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected:

- `rollshot-image-document` tests pass, including exact opaque replacement and immutable source tests.
- `rollshot-app` tests pass, including policy, routing, confirmation, view, and PNG boundary tests.
- Formatting check passes.
- Clippy reports no warnings.

- [ ] **Step 4: Run the user-facing copy scan**

Run:

```bash
rtk rg -n '"[^"]*[Ss]ecure[^"]*"' crates/rollshot-app/src/result_workspace
```

Expected: no user-facing Result Workspace string contains `Secure`. Internal
identifiers such as `has_secure_redactions` are allowed and should not appear
inside quoted UI strings.

- [ ] **Step 5: Perform Linux and macOS manual acceptance**

On Linux:

1. Complete a capture and confirm the original still auto-saves.
2. Add a redaction and confirm the persistent retained-original disclosure appears.
3. Confirm Copy/Save labels become safe labels.
4. Confirm Copy Original and Reveal Original require confirmation every time.
5. Save a safe image, confirm the filename default and success message, and confirm Reveal becomes `Reveal Last Safe Export`.
6. Attempt to save over the source path and confirm no write occurs.

On macOS:

1. Complete a capture and confirm the existing auto-save thumbnail flow remains unchanged.
2. Open Result Workspace and repeat the safe-sharing checks above.

Record any platform that cannot be run locally as an explicit remaining runtime-verification risk.

- [ ] **Step 6: Commit the PNG boundary test and final verification adjustments**

```bash
rtk git add crates/rollshot-app/src/result_workspace/actions.rs
rtk git commit -m "test(app): verify safe PNG export boundary"
```

Do not create an empty commit if the PNG boundary test required no source adjustment beyond the committed test.

## Plan Completion Criteria

- Every approved spec requirement maps to one of Tasks 1-5.
- User-facing Result Workspace copy uses `Safe` and never `Secure`.
- Safe outputs are flattened, have contextual labels and exact success messages, and cannot target `source_path`.
- Original-copy and original-reveal actions require fresh confirmation whenever redactions exist.
- The retained-original disclosure is persistent derived state.
- Linux and macOS shared Result Workspace behavior is verified, with any unavailable platform called out explicitly.
