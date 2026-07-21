# Open Existing Image Design

**Date:** 2026-07-21  
**Status:** Approved design  
**Scope:** Open one existing PNG or JPEG from the CLI in Rollshot's Result Workspace for annotation and optional OCR

## 1. Summary

Rollshot will accept an existing image through:

```bash
rollshot-app open <IMAGE>
```

The command decodes one static PNG or JPEG and opens it directly in the existing Result Workspace. It does not start capture, auto-save the source, show the capture overlay, or show the macOS post-capture thumbnail. The imported image gains the same annotation, redaction, copy, export, viewport, and—when compiled with the `ocr` feature—selectable OCR capabilities as a captured result.

An imported file is always a read-only source. Rollshot never overwrites it. Annotation output is a new flattened PNG created through Save As, with `<source-stem>-annotated.png` as the initial filename.

## 2. Product Thesis

Users often already have the screenshot-like image they need to explain, redact, or inspect: a downloaded image, a test artifact, a file from a colleague, or a screenshot created on another device. Requiring them to reproduce and recapture that content is wasteful and sometimes impossible.

This feature extends Rollshot's existing strength—the focused post-capture annotation and OCR workspace—to existing images. It does not turn Rollshot into a general-purpose image editor or file manager.

The signature experience is immediate: run one explicit command, see the image fit in the Workspace, annotate it or select OCR text, then copy or export a safe result without risking the source file.

## 3. Goals

- Add the explicit CLI form `rollshot-app open <IMAGE>`.
- Accept exactly one local static PNG or JPEG.
- Open the image directly in the shared Result Workspace on Linux and macOS.
- Reuse the existing annotation, secure-redaction, viewport, copy, export, and OCR behavior.
- Keep imported sources read-only and prevent all equivalent-path overwrites.
- Make imported, modified, and exported state understandable in the Workspace.
- Keep `open` available in builds without the `ocr` feature; only the OCR tool is absent in those builds.
- Ensure the official product distribution that promises this workflow is built with OCR enabled.
- Fail before opening a Workspace when the input cannot be read or decoded.

## 4. Non-Goals

- GUI File → Open, drag-and-drop, recent files, or OS file associations.
- Opening multiple images, tabs, batches, or directories.
- GIF, animated images, WebP, SVG, PDF, HEIC, RAW, or project-file import.
- Crop, rotate, resize, color correction, filters, or other general image-editing features.
- Persisting the editable annotation graph across launches.
- Overwriting or modifying the imported source under any circumstances.
- A file-to-stdout OCR pipeline or OCR text-file export.
- Refactoring capture, import, and Action Guide into a generalized application/session framework.

## 5. Product Decisions

1. The CLI uses an `open` subcommand rather than a bare positional path.
2. The command accepts one required image path.
3. Imported images enter the Result Workspace immediately.
4. Imported sources are always read-only.
5. Save and keyboard save actions always mean Save As for imported images.
6. The default export filename is `<source-stem>-annotated.png` in the source directory. If no usable stem exists, use `Rollshot-annotated.png`.
7. Flattened exports are PNG regardless of source format.
8. `open` works without OCR; the official OCR-capable distribution includes the `ocr` feature.
9. OCR preparation and selection are transient and do not dirty the annotation document.
10. The first release supports only static PNG and JPEG content.

## 6. Current Behavior and Boundary

The current CLI resolves capture, region OCR, daemon, and optional Action Guide commands. It has no image-open mode. Captured images reach the Result Workspace only after the platform capture and presentation policy:

- Linux auto-saves and opens a standalone iced Result Workspace.
- macOS normally retains the in-memory capture, presents a thumbnail after a successful auto-save, and opens the Workspace through the product host.

The Result Workspace already owns the user-facing annotation, secure sharing, selectable OCR, copy, Save As, dirty-state, and close-confirmation behavior. `ImageDocument` already accepts an in-memory `RgbaImage`. The new design therefore adds a file-import boundary and thin platform launch adapters; it does not route imported files through capture completion.

## 7. CLI Contract

Add an `Open` launch command and mode with one required `PathBuf`:

```text
rollshot-app open <IMAGE>
```

Clap owns syntax failures:

- missing `<IMAGE>`;
- more than one positional image;
- unknown options; and
- invalid command combinations.

The open command waits for the Workspace to close, matching the existing foreground application behavior. A normal window close returns exit code 0. Import or launch failure returns a non-zero exit code.

`open` does not accept capture backend, cursor, workflow, scope, OCR-capture, or graphical-feedback options. Global diagnostics options such as `--log-file` remain available.

## 8. Architecture

### 8.1 Launch routing

`launch.rs` parses the new command into `LaunchMode::Open { path }`. The top-level application dispatches it to a dedicated open-image flow rather than `run_iced_capture` or post-capture handling.

```text
CLI open
  → shared image import loader
  → imported ResultDocument
  → Linux standalone Result Workspace
     or macOS product host in Workspace phase
```

### 8.2 Image-import boundary

A focused app-level image-import module owns all filesystem and decoding work for this feature. Its public responsibility is:

```text
load(path) → ImportedImage { pixels, source_path, source_identity }
```

It must:

- open the path read-only;
- require a regular readable file;
- determine PNG or JPEG from file contents rather than the extension alone;
- decode to `RgbaImage`;
- apply JPEG EXIF orientation before producing pixels;
- retain a user-facing source path for labels and Reveal; and
- derive a resolved identity used to block source overwrite.

It must not create files, write metadata, create sidecars, or mutate the source directory.

The loader is independent of iced and capture backends. This keeps decoding and path-identity behavior testable without launching a window and leaves a reusable boundary for a later GUI Open action without implementing that action now.

### 8.3 Document origin

Replace the current ambiguous optional source-path state with an explicit document origin equivalent to:

```text
UnsavedCapture
SavedCapture(path)
ImportedImage {
    source_path,
    source_identity,
}
```

`last_export_path` remains separate because an export is not the source. The document exposes behavior through methods rather than requiring Workspace code to match origin variants throughout the UI:

- display name and origin label;
- source and preferred Reveal paths;
- whether closing can lose an uncopied capture;
- whether source-overwrite protection applies;
- default export directory and filename; and
- imported/captured status text.

### 8.4 Platform hosts

Linux uses the existing standalone Result Workspace runner with the imported document.

macOS gains a narrow bootstrap path that starts the existing product host directly in its Workspace phase with the imported document. It does not synthesize a capture result and does not enter ScreenCaptureKit, the overlay, auto-save policy, thumbnail state, tray, or daemon capture purpose.

The platform adapters differ only in window hosting. Import, document, Workspace, annotation, OCR, and export semantics are shared.

## 9. Format and Decode Semantics

- Supported content formats are static PNG and JPEG.
- Format recognition uses decoded content/signatures, not filename extension.
- A valid PNG or JPEG with a misleading or missing extension is accepted.
- Unsupported or animated formats are rejected rather than partially interpreted.
- JPEG EXIF orientation is applied once during import so display, annotation coordinates, OCR, copy, and export all use the same upright pixel space.
- Pixels are normalized to RGBA before constructing `ImageDocument`.
- Decoder safety/resource limits remain enabled. The existing Workspace retains the full-resolution source and uses its display-downscale mechanism for GPU texture limits.
- The design does not promise byte-for-byte preservation. Even an unannotated export is a newly encoded PNG representing the decoded visual pixels.

## 10. Workspace Experience

On successful import:

- the Workspace opens at its existing Fit to Window default;
- the image filename is visible through the document display name;
- the status communicates `Imported` while clean;
- annotation controls are immediately usable; and
- the OCR Text control appears only when the build has OCR.

The document starts clean. Annotation creation, modification, deletion, undo, and redo use the existing annotation history and dirty-baseline behavior. OCR preparation, OCR failure, OCR selection, Copy OCR Selection, and Copy All OCR Text are transient and never mark the image document dirty.

After an annotation edit, the Workspace communicates `Unsaved edits`. A successful flattened export records the current annotation state as the exported baseline. Later edits become dirty again.

The feature does not add an import-specific editor or tool mode.

## 11. Copy, Export, Reveal, and Close

### 11.1 Copy

The normal Copy action copies the existing flattened Workspace payload. With secure redactions, the existing safe-copy labels and safeguards remain in effect.

Copy Original means the decoded, orientation-corrected source pixels kept in memory; it is not promised to reproduce the source file bytes. When secure redactions exist, copying the unredacted original keeps the existing confirmation behavior.

### 11.2 Save As

Imported documents never expose an in-place save path. Save and keyboard save actions invoke Save As.

The initial dialog location is the source directory. The initial filename is `<source-stem>-annotated.png`, falling back to `Rollshot-annotated.png` when no usable stem exists. The output format is PNG. A missing extension is completed with `.png`; an explicitly non-PNG destination is rejected with a clear message rather than writing PNG bytes under a misleading extension.

The exported payload is the current flattened document. The source file remains untouched even when no annotations exist.

An imported source may never be selected as the destination. Source equivalence must cover:

- the same literal path;
- relative and absolute representations of the same path;
- normalized `.` and `..` components;
- symlinks that resolve to the source; and
- an existing hard link or other filesystem identity that denotes the same file.

For an existing destination, compare resolved filesystem identity. For a destination that does not yet exist, resolve the existing parent directory and compare the resulting destination path to the resolved source path. If identity cannot be established safely, fail closed only when the candidate can plausibly denote the source; do not block unrelated export destinations merely because optional canonicalization failed.

Rejected source overwrite leaves the Workspace and all edits intact and shows:

> Imported source is read-only. Choose another export location.

A user may overwrite a previous export through the normal platform file-dialog confirmation because that export is not the imported source.

### 11.3 Reveal

Before the first successful export, Reveal targets the imported source. After export, Reveal prefers `last_export_path`. Existing secure-redaction confirmation remains required before revealing an unredacted source.

### 11.4 Close

Closing a clean imported document requires no confirmation because the source already exists and has not changed. Closing with annotation changes newer than the last successful export shows the existing discard-edits confirmation. The prompt must not claim that the imported source itself will be lost.

## 12. OCR Behavior

The open-image command is not feature-gated. In builds without `ocr`, imported images still open and all annotation behavior remains available; the OCR Text tool is absent exactly as it is elsewhere in that build.

In OCR-enabled builds, imported images use the existing selectable OCR workflow:

- entering OCR Text prepares OCR from the in-memory, orientation-corrected source;
- recognized text is selectable;
- selection and all-text copy use the existing clipboard behavior;
- opaque secure redactions continue to mask OCR results; and
- OCR failure displays an inline error, returns to a usable non-OCR tool state, and preserves all image and annotation state.

The official distribution advertised for this product requirement must enable OCR. A development or custom build without OCR remains a valid annotation-only build.

## 13. Error Handling and Diagnostics

Failures before Workspace launch do not open an empty or partially initialized window. They return a non-zero exit status and present a concise stderr message containing the affected path and an actionable category:

- path does not exist;
- path is not a regular file;
- permission/read failure;
- unsupported image format;
- corrupt or incomplete image;
- decoder resource-limit failure;
- orientation/decode failure; or
- platform Workspace launch failure.

Linux and macOS share error categories and user-facing wording. Error chains may retain technical detail for diagnostics, but the primary message must not dump decoder internals.

Runtime diagnostics use stable explicit `rollshot::*` targets and structured fields such as error category and format. Diagnostics may record the user-supplied path consistently with existing launch diagnostics, but must never record image bytes, decoded pixels, OCR text, or annotation contents.

Once the Workspace is open, import has completed. Later OCR, copy, Reveal, and export failures remain recoverable inline failures and do not close the Workspace or discard edits.

## 14. Testing Strategy

### 14.1 CLI and routing

- Parse a valid `open <IMAGE>` command.
- Reject missing paths, multiple paths, and unknown options.
- Resolve the parsed command to the open launch mode without capture options.
- Verify the open route never enters capture backend selection or post-capture presentation.

### 14.2 Loader

- Decode representative PNG and JPEG fixtures to expected RGBA pixels.
- Accept valid supported content with a misleading or missing extension.
- Reject GIF, WebP, SVG, PDF, corrupt data, directories, missing files, and unreadable files.
- Apply all relevant JPEG EXIF orientation variants and verify final dimensions/pixels.
- Preserve a usable source display path and resolved source identity.
- Verify import creates no files and does not modify source bytes or metadata.

### 14.3 Document lifecycle and source protection

- Imported documents start clean and are not treated as losable unsaved captures.
- Annotation changes become dirty; successful export establishes a clean baseline; later edits become dirty again.
- OCR state changes do not dirty the document.
- Default export naming produces `<stem>-annotated.png` and the documented fallback.
- Same-path, relative/absolute equivalent, symlink, and hard-link destinations are rejected.
- A distinct path and a prior export destination remain valid.
- Rejection preserves document and edit state.
- Reveal selects source before export and latest export afterward.

### 14.4 Workspace and platform behavior

- Shared iced behavior verifies the Imported/Unsaved edits states, Save As routing, close confirmation, and recoverable error messages.
- A non-OCR build opens and annotates imported images without exposing OCR controls.
- An OCR-enabled build prepares selectable OCR for an imported image and preserves redaction masking.
- Linux launches the standalone Workspace directly.
- macOS launches the product host directly in Workspace phase.
- Neither platform performs capture, auto-save, or thumbnail presentation for imported images.

Because this changes user-visible iced UI state, implementation must invoke the repo-local `testing-iced-ui` skill before editing. Raw scenario evidence and any golden-baseline decision follow the independent-review rules in `AGENTS.md`; the product-changing agent does not approve its own baseline.

### 14.5 Verification

At minimum, implementation verification includes:

- focused tests for launch, loader, document, secure-sharing, and platform routing;
- `rtk cargo test -p rollshot-app`;
- the OCR-enabled app test lane;
- `rtk cargo fmt --check`; and
- `rtk cargo clippy --workspace --all-targets -- -D warnings` when the completed change risk warrants the workspace-wide run.

Manual/runtime smoke checks cover Linux and macOS because the window hosts differ, even though import and Workspace behavior are shared.

## 15. Acceptance Criteria

The feature is complete when:

1. `rollshot-app open <valid-png-or-jpeg>` opens that image directly in the Result Workspace on Linux and macOS.
2. No capture, overlay, auto-save, or thumbnail step occurs.
3. Existing annotation tools, copy, secure redaction, viewport, and Save As work on the imported image.
4. OCR-enabled builds expose selectable OCR; non-OCR builds retain a fully working annotation flow.
5. The imported source remains byte-for-byte unchanged through annotation, OCR, copy, export, Reveal, and close flows.
6. Save As defaults to `<stem>-annotated.png` and never overwrites any filesystem-equivalent form of the source.
7. Dirty, exported, Reveal, and close-confirmation behavior matches the rules above.
8. Unsupported, corrupt, missing, unreadable, or unsafe-to-decode inputs fail before Workspace launch with a non-zero status and actionable error.
9. JPEG orientation is consistent across display, annotation coordinates, OCR, copy, and export.
10. Automated tests cover parser, loader, lifecycle, overwrite protection, OCR feature variants, and both platform routes.

## 16. Alternatives Considered

### 16.1 Treat import as a capture result

Rejected. It appears to reduce initial wiring but gives imported files the wrong ownership and presentation semantics: auto-save, capture messaging, and the macOS thumbnail. Those exceptions would accumulate around a false abstraction.

### 16.2 Generalize the application into a document-first session framework

Deferred. It may become useful if Rollshot later adds multi-document tabs, drag-and-drop, file associations, or persistent editable projects. None is needed for the single-file CLI workflow, so the refactor would expand risk without improving the first user outcome.

### 16.3 Bare positional path

Rejected. The no-subcommand invocation already means capture. An explicit `open` command preserves a clear CLI grammar and leaves room for later open-specific options without ambiguity.

## 17. Follow-On Opportunities

The loader and imported-document boundary may later support a GUI Open action, drag-and-drop, or OS file association. Those entry points should reuse this design rather than create separate import semantics, but they are not part of this spec or its implementation plan.
