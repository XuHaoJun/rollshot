# Next Feature Discovery: Post-Capture Annotation

**Date:** 2026-06-11  
**Status:** Exploration complete; no implementation started

## Product Verdict

The next feature Rollshot should pursue is **post-capture annotation optimized
for long screenshots**.

Rollshot already handles the difficult capture workflow: interactive region
selection, scrolling capture, live stitching, auto-save, and a result workspace
with zoom, pan, copy, Save As, and reveal controls. Annotation is the clearest
next step because it turns a completed capture into something users can
immediately explain, review, redact, and share without leaving Rollshot.

The goal is not to build a general-purpose image editor. The valuable product
shape is a focused annotation mode inside the existing Result Workspace.

## Research Scope

This discovery compared:

- Rollshot's current implemented product flow.
- `learn-projects/flameshot`, especially its capture editing and pin tools.
- `learn-projects/snow-shot`, including drawing, OCR, pinning, history, and
  recording features.
- CleanShot X's published feature set and changelog.

This was a product exploration only. No feature implementation or code changes
were made.

## Current Rollshot Baseline

Rollshot already provides:

- Interactive screenshot and scrolling-capture modes.
- Drag-selected capture regions and a live stitched preview.
- Linux Wayland capture through XDG Desktop Portal and PipeWire.
- macOS capture through ScreenCaptureKit.
- Automatic PNG saving after capture.
- A Result Workspace with full-resolution source images, zoom, pan, copy,
  Save As, reveal-in-file-manager, and unsaved-result protection.
- A draggable post-capture thumbnail on macOS.

The Result Workspace is the natural integration point for the next feature.
It already owns the finalized image and the actions users take before sharing
or closing it.

## Evaluation Criteria

Candidates were evaluated by:

1. How frequently the capability appears across established screenshot tools.
2. Whether it strengthens Rollshot's scrolling-capture identity.
3. How directly it improves the capture-to-share workflow.
4. Whether it fits the existing product architecture and surfaces.
5. Cross-platform consistency and implementation risk.
6. Whether the feature creates a focused product or starts a separate product.

## Recommended Feature

### Post-Capture Annotation

Add an annotation mode to the existing Result Workspace, designed to remain
usable on very tall scrolling captures.

### Why It Wins

- Flameshot presents in-app screenshot editing as a core feature. Its users
  describe the quick post-selection editing tools as one of the product's most
  valuable capabilities.
- Snow Shot includes a broad annotation workflow with arrows, shapes, text,
  highlighting, blur, mosaic, and undo/redo.
- CleanShot X treats annotation as a core part of its capture and sharing
  workflow, alongside scrolling capture.
- Annotation closes Rollshot's largest workflow gap: users can capture and
  inspect a result, but cannot yet explain or redact it before sharing.
- It compounds Rollshot's distinctive strength. Annotating a long bug report,
  tutorial, document, or review capture is more valuable than annotating an
  ordinary screenshot because switching to a general image editor is especially
  awkward for extremely tall images.

### Recommended First Release

Keep the first release narrow:

- Enter annotation mode from the Result Workspace.
- Support arrow, rectangle, text, and secure redaction tools.
- Support selecting, moving, and deleting annotations.
- Support undo and redo.
- Keep annotations aligned while zooming and panning long screenshots.
- Make Copy and Save As export the composited result.
- Preserve the original auto-saved capture so annotation mistakes cannot
  destroy the source image.

Use opaque redaction for the first release. Blur and pixelation can create a
false sense of privacy and should not ship until their security behavior is
explicitly designed and tested.

### Not in the First Release

- A complete image editor.
- Editable project files.
- Freehand drawing, counters, spotlights, backgrounds, or image composition.
- Opening arbitrary external images.
- Blur or pixelation redaction.
- Cloud sharing.

These additions may be useful later, but they do not need to be present for a
coherent first annotation workflow.

## Candidate Comparison

| Rank | Candidate | Decision | Reason |
| --- | --- | --- | --- |
| 1 | Post-capture annotation | Recommend | Common across screenshot products and directly completes Rollshot's capture-to-share workflow. |
| 2 | Window or element selection | Next candidate | Highly useful and close to the capture core, but reliable Linux Wayland support is constrained by portal and desktop-environment behavior. |
| 3 | Pin to screen | Defer | Useful in Flameshot, Snow Shot, and CleanShot, but very tall captures are poorly suited to direct pinning and macOS already has a floating thumbnail. |
| 4 | Capture history | Defer | Improves recovery and organization, but Rollshot's auto-save and Reveal actions already cover part of the job. |
| 5 | OCR and text recognition | Defer | Valuable in Snow Shot and CleanShot, but introduces model, language, packaging, and platform complexity while doing less to strengthen scrolling capture. |
| 6 | Screen recording and GIF | Do not pursue now | Popular in broader screenshot suites, but it is effectively a separate product and would dilute Rollshot's current focus. |
| 7 | Cloud sharing | Do not pursue now | Requires ongoing service operations, accounts, privacy policy, abuse handling, and reliability commitments. |

## Important Candidate Notes

### Window or Element Selection

This is the strongest alternative. Snow Shot includes automatic window
selection on Windows and macOS, but its child-element selection is currently a
Windows-only feature implemented through Windows UI Automation; the equivalent
Linux and macOS child-element paths are not implemented. CleanShot supports
window capture as a first-class mode. It would improve ordinary screenshot
speed and precision.

However, Rollshot's Linux portal path currently accepts portal monitor
selection but explicitly rejects a selected window stream. Shipping a polished
window-capture experience would therefore create meaningful platform
differences or require deeper capture-backend work before the product benefit
is consistent.

### Pin to Screen

Pinning is mature in Flameshot and Snow Shot, and CleanShot exposes it as a
primary post-capture action. It is useful for keeping a visual reference above
other windows.

It ranks below annotation because it is less suitable for Rollshot's signature
long captures. A useful long-image pin would require its own scrolling,
resizing, placement, and lifecycle design rather than simply opening an
always-on-top image window.

### Capture History

Capture history is valuable for recovering recent work and appears in Snow Shot
and CleanShot. It should eventually become a durable Rollshot feature, but it
does not improve the quality or communicative value of an individual capture.
The existing auto-save workflow also reduces the urgency.

### OCR

OCR is prominent in Snow Shot and CleanShot. It solves a real extraction job,
but its strongest use case is obtaining text rather than creating and sharing
better screenshots. It also brings substantial language-model and packaging
decisions that are separate from Rollshot's current product core.

## Product Risks

- Annotation controls could overwhelm the Result Workspace. The primary actions
  must remain obvious, with annotation presented as a mode rather than a
  permanently expanded toolbar.
- Long screenshots make coordinate transforms, viewport behavior, and export
  correctness central to user trust.
- Users must always understand whether they are copying or saving the original
  capture or the annotated result.
- Redaction must be secure. A visually blurred secret that can be recovered is
  a critical product defect.
- Annotation must not overwrite the only original capture.

## Suggested Success Criteria

The feature direction is successful when a user can:

1. Finish a scrolling capture.
2. Open annotation mode from the Result Workspace.
3. Add an arrow, rectangle, text label, and secure redaction while navigating a
   long image.
4. Undo or modify those annotations.
5. Copy or save the annotated output.
6. Recover the untouched original capture afterward.

## References

### Local Reference Projects

- `learn-projects/flameshot/README.md`
- `learn-projects/flameshot/src/tools/`
- `learn-projects/flameshot/src/tools/pin/`
- `learn-projects/snow-shot/src/messages/en.ts`
- `learn-projects/snow-shot/src/pages/draw/`
- `learn-projects/snow-shot/src/pages/fixedContent/`
- `learn-projects/snow-shot/src/utils/captureHistory.ts`

### Rollshot Code

- `crates/rollshot-app/src/result_workspace/mod.rs`
- `crates/rollshot-app/src/result_workspace/actions.rs`
- `crates/rollshot-app/src/post_capture.rs`
- `crates/rollshot-capture/src/linux/portal.rs`

### External Sources

- [Flameshot features](https://github.com/flameshot-org/flameshot#features)
- [Flameshot discussion: post-selection editing tools](https://github.com/flameshot-org/flameshot/issues/1100)
- [CleanShot X features](https://cleanshot.com/features)
- [CleanShot X changelog](https://cleanshot.com/changelog)
- [CleanShot X URL scheme API](https://cleanshot.com/docs-api)
