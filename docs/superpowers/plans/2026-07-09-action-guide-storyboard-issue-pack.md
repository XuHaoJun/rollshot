# Action Guide Storyboard Issue Pack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Include the existing Action Guide Storyboard PNG in Local Issue Pack exports and reference it only when generation succeeds.

**Architecture:** Keep the change inside `crates/rollshot-app/src/issue_pack.rs`. The Issue Pack builder calls the existing `rollshot_action::export_storyboard(...)` API after `export_guide(...)`, then derives `include_storyboard` from the written file before rendering Markdown and manifest JSON.

**Tech Stack:** Rust, `rollshot-app`, feature-gated `rollshot-action`, `image`, `serde_json`, `tempfile`, Cargo tests through `rtk`.

## Global Constraints

- Generate `action-guide/storyboard.png` by default for Issue Packs that include a reviewed Action Guide.
- Show the Storyboard as an Overview image near the top of `issue.md` when the PNG exists.
- Add an `action_storyboard` asset entry to `manifest.json` when the PNG exists.
- Treat Storyboard generation as optional: if it fails after the guide export succeeds, keep the Issue Pack valid and record a warning.
- Preserve existing screenshot-only, Action Guide-only, combined, folder, ZIP, GIF, `steps.md`, `session.json`, and keyframe behavior.
- No user-facing checkbox or setting for Storyboard inclusion.
- No changes to the Timeline Workspace header or export dialog.
- No changes to `rollshot-action` renderer APIs.
- No caption or annotation model changes.
- No claim that Action Guide keyframes are redacted or sensitive-free.

---

## File Structure

- Modify `crates/rollshot-app/src/issue_pack.rs`
  - Owns Issue Pack input models, Markdown rendering, manifest rendering, folder/ZIP export, and tests.
  - Add `include_storyboard: bool` parameters beside existing `include_gif` handling.
  - Add Storyboard export attempt in the feature-gated Action Guide branch of `build_folder(...)`.
  - Add tests in existing `tests` and `action_guide_tests` modules.

No new files or crates are needed.

---

## Review Lock-In

### Step 0 Scope Challenge

- Goal vs steps alignment: both tasks directly support the goal. Task 1 makes Markdown and manifest references conditional; Task 2 produces the Storyboard asset and warning behavior.
- Minimum viable plan: Task 1 plus Task 2 is the smallest shippable unit. Task 1 alone would reference a derived asset no export path produces; Task 2 depends on Task 1 to avoid missing-file links.
- Complexity check: 0 new files, 0 new crates/modules, 2 tasks. No scope reduction needed.
- Search check: no new architecture pattern, runtime, concurrency model, distribution artifact, or infrastructure component is introduced.
- Distribution check: no new binary, library, container, or package is introduced; the behavior ships through the existing `rollshot-app` Issue Pack export flow.

### What Already Exists

- `rollshot_action::export_storyboard(...)` already renders `Guide + FrameStore` to a PNG and enforces canvas limits. This plan reuses it instead of adding a second renderer.
- `rollshot_action::export_guide(...)` already writes `action-guide/steps.md`, `session.json`, and keyframes. This plan keeps it as the required Action Guide export primitive.
- `rollshot_action::export_gif(...)` already uses an optional-asset warning pattern in `issue_pack.rs`. This plan mirrors that pattern for Storyboard.
- `render_issue_markdown(...)`, `manifest_assets(...)`, and `render_manifest_json(...)` already centralize Issue Pack references. This plan threads `include_storyboard` through those helpers instead of duplicating Markdown or JSON generation.
- `zip_directory(...)` already packages every file under the generated folder. This plan adds a feature-gated zip assertion to make sure the new asset reaches ZIP exports through that existing path.

### NOT In Scope

- Storyboard preview: deferred because this P1 slice has no Timeline Workspace UI changes.
- Storyboard layout controls: deferred because the existing default renderer is sufficient for Issue Pack overview use.
- Step captions: deferred because it requires Action Guide model and export contract changes.
- Per-step annotations or redactions: deferred because it changes evidence semantics and needs a separate privacy review.
- Agent suggestions: deferred until manual caption/annotation primitives exist.
- User checkbox for Storyboard inclusion: deferred because default inclusion is the approved P1 behavior.
- Renderer refactor or in-memory render API: deferred because `export_storyboard(...)` already satisfies this export path.

### Test Coverage Table

```text
Task / behavior                                           Unit  Integ  E2E / smoke  Manual only
────────────────────────────────────────────────────────  ────  ─────  ───────────  ───────────
Task 1 / Markdown Overview when Storyboard exists          ✓     —      —            no
Task 1 / Markdown omits Overview when Storyboard missing   ✓     —      —            no
Task 1 / Manifest includes action_storyboard conditionally ✓     —      —            no
Task 2 / Folder export writes storyboard.png               —     ✓      —            no
Task 2 / issue.md and manifest reference written asset     —     ✓      —            no
Task 2 / Storyboard canvas-limit failure warns             —     ✓      —            no
Task 2 / ZIP export contains storyboard.png                —     ✓      —            no
Task 2 / Screenshot-only no-feature Issue Pack path        —     ✓      —            no
Task 2 / Formatting and clippy verification                —     ✓      —            no
```

### Failure Modes

- `export_guide(...)` fails because a keyframe is missing. Covered by existing `action_guide_only_missing_keyframe_rolls_back_temp_output`; the plan keeps this as fatal through `IssuePackError::Io("export failed: ...")`, and the user sees an export failure.
- `export_storyboard(...)` fails because the Storyboard exceeds the canvas pixel limit. Covered by Task 2 / Step 1 `storyboard_export_failure_warns_without_blocking_issue_pack`; the plan records `storyboard_export_failed`, omits Storyboard references, and the user sees an Issue Pack warning.
- `export_storyboard(...)` fails because a keyframe is missing after `export_guide(...)` succeeded. This should not happen for the same `Guide + FrameStore`, but the same `Err(error)` arm records `storyboard_export_failed`, omits Storyboard references, and returns a valid pack.
- `guide.gif` generation fails. Existing warning behavior remains unchanged through `gif_export_failed`; the new plan does not alter that path.
- `zip_directory(...)` misses the Storyboard file. Covered by Task 2 / Step 1 `export_zip_with_action_guide_includes_storyboard`; if it failed, the test would catch the missing ZIP entry before release.
- `render_issue_markdown(...)` or `manifest_assets(...)` references a missing Storyboard. Covered by Task 1 conditional helper tests and Task 2 warning-path integration test.

No critical gaps remain: every new silent-failure risk has either direct test coverage or an existing fatal export path.

### Parallelization

Sequential execution, no parallelization opportunity. Both tasks modify `crates/rollshot-app/src/issue_pack.rs`, and Task 2 depends on Task 1 helper signatures.

---

### Task 1: Markdown And Manifest Storyboard References

**Files:**
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

**Interfaces:**
- Consumes: existing `IssuePackInput`, `ActionGuideIssueAssets`, `IssuePackStep`, and `AssetEntry`.
- Produces:
  - `pub(crate) fn render_issue_markdown(input: &IssuePackInput, include_storyboard: bool) -> String`
  - `pub(crate) fn manifest_assets(input: &IssuePackInput, include_gif: bool, include_storyboard: bool) -> Vec<AssetEntry>`
  - `fn render_manifest_json(input: &IssuePackInput, warnings: &[IssuePackWarning], include_gif: bool, include_storyboard: bool) -> Result<String, IssuePackError>`

- [ ] **Step 1: Write failing tests for Markdown and manifest behavior**

Add these tests inside the existing `#[cfg(test)] mod tests` in `crates/rollshot-app/src/issue_pack.rs`:

```rust
    fn action_guide_input_with_one_step(include_gif: bool) -> IssuePackInput {
        let mut input = base_input();
        input.final_image = None;
        input.action_guide = Some(ActionGuideIssueAssets {
            include_gif,
            steps: vec![IssuePackStep {
                index: 1,
                title: "Open Settings".to_string(),
                keyframe_path: "action-guide/keyframes/001.png".to_string(),
            }],
        });
        input.evidence_review.result_workspace_images_reviewed = false;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input.redaction.result_workspace_images_are_flattened = false;
        input
    }

    #[test]
    fn renders_storyboard_overview_when_action_storyboard_exists() {
        let input = action_guide_input_with_one_step(false);
        let md = render_issue_markdown(&input, true);

        assert!(md.contains("Overview:\n\n![](action-guide/storyboard.png)"), "md = {md}");
        assert!(md.contains("1. Open Settings"), "md = {md}");
        assert!(md.contains("![](action-guide/keyframes/001.png)"), "md = {md}");
    }

    #[test]
    fn omits_storyboard_overview_when_action_storyboard_is_absent() {
        let input = action_guide_input_with_one_step(false);
        let md = render_issue_markdown(&input, false);

        assert!(!md.contains("action-guide/storyboard.png"), "md = {md}");
        assert!(md.contains("1. Open Settings"), "md = {md}");
    }

    #[test]
    fn manifest_assets_include_storyboard_only_when_present() {
        let input = action_guide_input_with_one_step(true);

        let without_storyboard = manifest_assets(&input, true, false);
        assert!(
            !without_storyboard
                .iter()
                .any(|asset| asset.kind == "action_storyboard"),
            "assets = {without_storyboard:?}"
        );

        let with_storyboard = manifest_assets(&input, true, true);
        let paths: Vec<_> = with_storyboard.iter().map(|asset| asset.path.as_str()).collect();
        assert!(
            paths.contains(&"action-guide/storyboard.png"),
            "paths = {paths:?}"
        );
        assert!(
            with_storyboard
                .iter()
                .any(|asset| asset.kind == "action_storyboard"
                    && asset.path == "action-guide/storyboard.png"),
            "assets = {with_storyboard:?}"
        );
    }
```

- [ ] **Step 2: Run tests to verify the new calls fail before implementation**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack
```

Expected: FAIL to compile with errors equivalent to:

```text
this function takes 1 argument but 2 arguments were supplied
this function takes 2 arguments but 3 arguments were supplied
```

- [ ] **Step 3: Implement Markdown and manifest helper signatures**

Change `render_issue_markdown(...)` to accept `include_storyboard` and insert the Overview block before step rows:

```rust
pub(crate) fn render_issue_markdown(input: &IssuePackInput, include_storyboard: bool) -> String {
    let mut md = String::from("# Bug Report\n\n");
    md.push_str("## Summary\n\n[Write a short summary]\n\n");
    md.push_str("## Steps to reproduce\n\n");
    if let Some(action) = &input.action_guide {
        if include_storyboard {
            md.push_str("Overview:\n\n");
            md.push_str("![](action-guide/storyboard.png)\n\n");
        }
        for step in &action.steps {
            md.push_str(&format!(
                "{}. {}\n\n   ![]({})\n\n",
                step.index, step.title, step.keyframe_path
            ));
        }
    } else {
        md.push_str("[Write the steps to reproduce]\n\n");
    }
    md.push_str("## Actual result\n\n");
    if let Some(image) = &input.final_image {
        md.push_str("The UI reached this state:\n\n");
        md.push_str(&format!("![](images/{})\n\n", image.file_name));
    } else {
        md.push_str("[Describe what happened]\n\n");
    }
    md.push_str("## Expected result\n\n[Write what should have happened]\n\n");
    if !input.ocr_snippets.is_empty() {
        md.push_str("## OCR snippets\n\n");
        for snippet in &input.ocr_snippets {
            md.push_str(&format!("- {}\n", snippet.text));
        }
        md.push('\n');
    }
    md.push_str("## Environment\n\n");
    md.push_str(&format!("- OS: {}\n", input.platform.os));
    md.push_str(&format!("- Architecture: {}\n", input.platform.arch));
    md.push_str(&format!(
        "- Rollshot version: {}\n\n",
        input.rollshot_version
    ));
    md.push_str("## Attachments\n\n");
    if input.action_guide.is_some() {
        md.push_str("- `action-guide/steps.md`\n");
        md.push_str("- `action-guide/session.json`\n");
    }
    md.push_str("- `manifest.json`\n");
    md
}
```

Change `manifest_assets(...)` to accept `include_storyboard` and add the asset before keyframes:

```rust
pub(crate) fn manifest_assets(
    input: &IssuePackInput,
    include_gif: bool,
    include_storyboard: bool,
) -> Vec<AssetEntry> {
    let mut assets = vec![
        AssetEntry {
            kind: "issue_markdown".to_string(),
            path: "issue.md".to_string(),
        },
        AssetEntry {
            kind: "manifest".to_string(),
            path: "manifest.json".to_string(),
        },
    ];
    if let Some(image) = &input.final_image {
        assets.push(AssetEntry {
            kind: "final_redacted_image".to_string(),
            path: format!("images/{}", image.file_name),
        });
    }
    if let Some(action) = &input.action_guide {
        assets.push(AssetEntry {
            kind: "action_steps".to_string(),
            path: "action-guide/steps.md".to_string(),
        });
        assets.push(AssetEntry {
            kind: "action_session".to_string(),
            path: "action-guide/session.json".to_string(),
        });
        if include_storyboard {
            assets.push(AssetEntry {
                kind: "action_storyboard".to_string(),
                path: "action-guide/storyboard.png".to_string(),
            });
        }
        for step in &action.steps {
            assets.push(AssetEntry {
                kind: "action_keyframe".to_string(),
                path: step.keyframe_path.clone(),
            });
        }
        if include_gif {
            assets.push(AssetEntry {
                kind: "action_gif".to_string(),
                path: "action-guide/guide.gif".to_string(),
            });
        }
    }
    assets
}
```

Change `render_manifest_json(...)` to pass the new flag:

```rust
fn render_manifest_json(
    input: &IssuePackInput,
    warnings: &[IssuePackWarning],
    include_gif: bool,
    include_storyboard: bool,
) -> Result<String, IssuePackError> {
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        created_at: input.created_at.to_rfc3339(),
        rollshot_version: &input.rollshot_version,
        export_mode: EXPORT_MODE,
        evidence_review: EvidenceReviewManifest {
            required: input.evidence_review.required,
            completed: input.evidence_review.completed,
            result_workspace_images_reviewed: input
                .evidence_review
                .result_workspace_images_reviewed,
            action_guide_keyframes_reviewed: input.evidence_review.action_guide_keyframes_reviewed,
        },
        platform: PlatformManifest {
            os: &input.platform.os,
            arch: &input.platform.arch,
        },
        redaction: RedactionManifest {
            review_required: input.redaction.review_required,
            review_completed: input.redaction.review_completed,
            result_workspace_images_are_flattened: input
                .redaction
                .result_workspace_images_are_flattened,
            original_pixels_included: input.redaction.original_pixels_included,
            redaction_count: input.redaction.redaction_count,
        },
        assets: manifest_assets(input, include_gif, include_storyboard),
        ocr: OcrManifest {
            included: !input.ocr_snippets.is_empty(),
            snippet_count: input.ocr_snippets.len(),
        },
        warnings,
    };
    serde_json::to_string_pretty(&manifest).map_err(|e| IssuePackError::Json(e.to_string()))
}
```

In `build_folder(...)`, compute both local flags before rendering Markdown and manifest:

```rust
    let include_gif = tmp_dir.join("action-guide/guide.gif").exists();
    let include_storyboard = tmp_dir.join("action-guide/storyboard.png").exists();
    std::fs::write(
        tmp_dir.join("issue.md"),
        render_issue_markdown(input, include_storyboard),
    )
    .map_err(|e| IssuePackError::Io(e.to_string()))?;
    let manifest = render_manifest_json(input, warnings, include_gif, include_storyboard)?;
```

Update existing tests and helper call sites so they pass `false` unless the test specifically covers Storyboard:

```rust
let md = render_issue_markdown(&base_input(), false);
let md = render_issue_markdown(&input, false);
let assets = manifest_assets(&input, true, false);
let json = render_manifest_json(&input, &warnings, false, false).unwrap();
```

- [ ] **Step 4: Run tests for the pure Issue Pack helpers**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack
```

Expected: PASS for non-feature-gated Issue Pack tests.

- [ ] **Step 5: Commit Task 1**

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(issue-pack): render storyboard references"
```

---

### Task 2: Action Guide Storyboard Export Integration

**Files:**
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

**Interfaces:**
- Consumes:
  - `rollshot_action::export_storyboard(guide, store, StoryboardOptions::default(), &Path)`
  - Task 1 helper signatures with `include_storyboard`.
- Produces:
  - `action-guide/storyboard.png` in successful Action Guide Issue Packs.
  - `IssuePackWarning { code: "storyboard_export_failed", message }` when Storyboard rendering fails after `export_guide(...)` succeeds.

- [ ] **Step 1: Write failing feature-gated export tests**

Add this helper inside `#[cfg(all(test, feature = "action-guide"))] mod action_guide_tests`:

```rust
    fn many_step_action_input(
        count: usize,
    ) -> (
        IssuePackInput,
        Guide,
        FrameStore,
        CaptureRegion,
        InputCapability,
        InputSourceKind,
    ) {
        let mut store = FrameStore::new(StoreConfig {
            ring_capacity: count + 16,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 0,
            window_after: 0,
            nearby_max: 1,
        });
        let mut candidates = Vec::with_capacity(count);
        for i in 0..count {
            let id = store.ingest(quadrant(), i as u64 * 100);
            store.retain_window(id);
            candidates.push(CandidateStep {
                id,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: i as u64 * 100,
                keyframe: id,
                nearby: vec![id],
            });
        }
        let guide = Guide::from_candidates(candidates);
        let mut input = super::tests::base_input();
        input.final_image = None;
        input.evidence_review.result_workspace_images_reviewed = false;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input.redaction.result_workspace_images_are_flattened = false;
        input.action_guide = Some(ActionGuideIssueAssets::from_guide(&guide, false));
        (
            input,
            guide,
            store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
    }
```

Add this assertion block to the existing `export_folder_includes_action_guide_folder` test after the current keyframe assertion:

```rust
        assert!(result.directory.join("action-guide/storyboard.png").exists());
        assert!(result.warnings.is_empty(), "warnings = {:?}", result.warnings);
```

Extend the same test's Markdown and manifest assertions:

```rust
        assert!(
            md.contains("Overview:\n\n![](action-guide/storyboard.png)"),
            "md = {md}"
        );
        assert!(
            manifest.contains("\"action_storyboard\""),
            "manifest = {manifest}"
        );
        assert!(
            manifest.contains("\"action-guide/storyboard.png\""),
            "manifest = {manifest}"
        );
```

Add this new test in the same module:

```rust
    #[test]
    fn storyboard_export_failure_warns_without_blocking_issue_pack() {
        let (input, guide, store, region, capability, source_kind) = many_step_action_input(260);
        let tmp = tempfile::tempdir().unwrap();
        let action = ActionGuideExportSource {
            guide: &guide,
            store: &store,
            region,
            capability,
            source_kind,
            include_gif: false,
        };

        let result = export_folder_with_action_guide(&input, Some(action), tmp.path()).unwrap();

        assert!(result.directory.join("action-guide/steps.md").exists());
        assert!(result
            .directory
            .join("action-guide/keyframes/001.png")
            .exists());
        assert!(!result.directory.join("action-guide/storyboard.png").exists());
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, "storyboard_export_failed");

        let md = std::fs::read_to_string(result.directory.join("issue.md")).unwrap();
        assert!(!md.contains("action-guide/storyboard.png"), "md = {md}");

        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(
            manifest.contains("\"storyboard_export_failed\""),
            "manifest = {manifest}"
        );
        assert!(
            !manifest.contains("\"action_storyboard\""),
            "manifest = {manifest}"
        );
    }
```

Add this zip coverage test in the same module:

```rust
    #[test]
    fn export_zip_with_action_guide_includes_storyboard() {
        let (input, guide, store, region, capability, source_kind) = action_input();
        let tmp = tempfile::tempdir().unwrap();
        let action = ActionGuideExportSource {
            guide: &guide,
            store: &store,
            region,
            capability,
            source_kind,
            include_gif: false,
        };

        let result = export_zip_with_action_guide(&input, Some(action), tmp.path()).unwrap();
        let zip_path = result.zip_path.clone().expect("zip path");
        let file = std::fs::File::open(zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        names.sort();

        assert!(
            names.contains(&"action-guide/storyboard.png".to_string()),
            "names = {names:?}"
        );
    }
```

- [ ] **Step 2: Run feature-gated tests to verify failure before implementation**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack
```

Expected: FAIL because `action-guide/storyboard.png` is not written and the warning path is not implemented.

- [ ] **Step 3: Implement Storyboard export attempt**

Inside the existing feature-gated `if let Some(action) = action` block in `build_folder(...)`, call Storyboard export after `export_guide(...)` and before GIF export:

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

Keep the existing GIF export block after this new Storyboard block:

```rust
        if action.include_gif {
            let gif_path = tmp_dir.join("action-guide/guide.gif");
            if let Err(error) = rollshot_action::export_gif(
                action.guide,
                action.store,
                rollshot_action::GifOptions::default(),
                &gif_path,
            ) {
                warnings.push(IssuePackWarning {
                    code: "gif_export_failed".to_string(),
                    message: format!("GIF export failed: {error}"),
                });
            }
        }
```

- [ ] **Step 4: Run feature-gated Issue Pack tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack
```

Expected: PASS, including the new successful Storyboard export test and the canvas-limit warning test.

- [ ] **Step 5: Run focused non-feature tests**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack
```

Expected: PASS. This confirms screenshot-only and non-Action-Guide Issue Pack paths still compile with the new helper signatures.

- [ ] **Step 6: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS. If it fails, run `rtk cargo fmt`, inspect the diff, then rerun `rtk cargo fmt --check`.

- [ ] **Step 7: Run clippy for the no-feature Issue Pack path**

Run:

```bash
rtk cargo clippy -p rollshot-app --all-targets -- -D warnings
```

Expected: PASS. This covers the default build where `ActionGuideExportSource` is the non-feature placeholder type.

- [ ] **Step 8: Run clippy for the feature-gated Action Guide Issue Pack path**

Run:

```bash
rtk cargo clippy -p rollshot-app --all-targets --features action-guide -- -D warnings
```

Expected: PASS. This covers the real `rollshot_action::export_storyboard(...)` integration path.

- [ ] **Step 9: Commit Task 2**

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(issue-pack): include action guide storyboard"
```
