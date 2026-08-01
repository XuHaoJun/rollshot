# Launch Teaser MVP Design

**Date:** 2026-08-01  
**Status:** Approved design  
**Area:** Agent skills, Action Guide, video export  
**Branch:** `feat/launch-teaser-mvp`

## 1. Decision

Build a product-facing narrow launch-teaser workflow in the Action Guide Timeline Workspace.

A user with a persistent native motion recording and at least three reviewed Action Guide steps can create a deterministic teaser draft without a model provider, edit and review it, generate an external preview, and explicitly render a silent 15–25 second landscape MP4. If a provider is available, the user may ask the bundled launch-teaser skill to improve the draft. Repository enrichment is optional and requires an explicit bounded allowlist with a visible read ledger.

The implementation uses a typed `LaunchTeaserPlanV1` and a fixed Rollshot-owned FFmpeg renderer. It does not execute agent-generated code, accept arbitrary FFmpeg expressions, or introduce a general video platform.

## 2. Current Readiness and Accepted Risk

The original idea required a reviewed skills direction and one smaller skill demonstrated end to end with bounded tools, cancellation, explicit permissions, and reviewable output. Current code now provides:

- a bounded static skill catalog with digest-pinned invocation and skill-use receipts;
- immutable run authority snapshots with disclosure ceilings, prepared capabilities, granted operations, and receipts;
- registered typed tools, run budgets, cancellation, terminal states, durable tasks, artifacts, continuity, and audit observability;
- product paths for Smart Redaction, Action Guide caption proposals, and Action Guide visual-annotation proposals;
- reviewable accept/reject/stale proposal semantics and provenance;
- persistent Action Guide native motion at `assets/motion/recording.mp4`, bound by digest and media metadata.

The latest Phase 0 aggregate report still has a formal `STOP` verdict because one Rollshot case substituted placeholder motion and recorded two story-critical operation gaps. Those gaps were classified as case-specific rather than systemic. The product decision for this design is to proceed directly to a narrow MVP instead of first repeating that case with a real motion asset.

This decision does not erase the evidence gap. Implementation completion proves the product contract, not teaser quality or demand. Before launch-ready status, at least one real-motion Rollshot case must show that the fixed operation set produces a coherent story without placeholder assets.

## 3. Goals

1. Produce a coherent 1920×1080, 30 fps, silent H.264 teaser lasting 15–25 seconds.
2. Use reviewed Action Guide steps and the project-owned native motion as the primary evidence.
3. Work end to end without a model provider through a deterministic seed plan.
4. Let an agent optionally improve shot selection and copy through a bundled provider-neutral skill.
5. Let users inspect and accept every agent-proposed change before mutation or rendering.
6. Support optional repository enrichment through explicit, bounded, auditable read authority.
7. Execute only a typed, fixed operation set through a deterministic Rollshot renderer.
8. Preserve cancellation, resource bounds, privacy, provenance, and stale-data protection.

## 4. Non-goals

- No audio stream, bundled music, imported audio, voiceover, or audio mixing.
- No vertical or square output.
- No template system, marketplace, or multiple visual treatments.
- No nonlinear timeline editor or general motion-graphics Studio.
- No arbitrary HTML, CSS, JavaScript, shaders, FFmpeg filters, or generated composition code.
- No Hyperframes runtime or adapter in the MVP.
- No cloud rendering or publishing.
- No arbitrary filesystem, shell, network, process, or write authority for skills.
- No requirement to configure a provider or grant repository access.
- No promise of byte-identical MP4 files across different FFmpeg builds.
- No inline Iced video player in the first release.

## 5. Ownership and Boundaries

### 5.1 `rollshot-action`

Owns the framework-neutral teaser domain and renderer:

- `LaunchTeaserPlanV1` and related validated types;
- deterministic seed generation from reviewed steps and motion metadata;
- project revision, projection, and motion binding;
- plan validation and exact duration calculation;
- caption-overlay rasterization inputs;
- compilation to the fixed FFmpeg operation graph;
- preview and final rendering;
- ffprobe result validation;
- cancellation and temporary-output cleanup.

The teaser renderer is a sibling of the existing reviewed-keyframe summary video exporter. The existing summary MP4 contract remains unchanged.

`rollshot-action` reuses its current dependencies and patterns:

- `VideoToolchain` for FFmpeg and ffprobe;
- `ValidatedMotionAsset` and the persisted project motion contract;
- publish cancellation and process termination behavior;
- temporary sibling output followed by atomic rename;
- `rollshot-image-document` deterministic text rendering and vendored fonts.

### 5.2 `rollshot-agent`

Owns the agent-specific enhancement path:

- bundled `action-guide-launch-teaser` skill package;
- provider-neutral prompt composition;
- strict decoding into typed plan suggestions or patches;
- authority checks for Action Guide disclosure and optional repository reading;
- repository read tool registration and receipts;
- skill, authority, run, and proposal provenance;
- durable task, cancellation, terminal, continuity, and audit integration.

The skill may propose changes only. It cannot render, launch processes, write the project, accept its own proposal, or bypass product validation.

### 5.3 `rollshot-app`

Owns the product interaction in Timeline Workspace:

- entry-point eligibility and disabled reasons;
- teaser workspace state machine;
- deterministic draft creation;
- storyboard and selected-shot editing;
- optional repository scope confirmation;
- agent progress, cancellation, and field-level diff review;
- preview generation and native opening;
- explicit captured-content confirmation;
- final save destination and render job;
- completed-output actions and accepted-plan persistence.

The Timeline Workspace path is shared by the Linux and macOS products. Platform-specific helpers remain responsible for native Open and Show in Folder behavior.

## 6. Plan Contract

`LaunchTeaserPlanV1` is a versioned review artifact, not an executable program.

```text
LaunchTeaserPlanV1
├── schema_version
├── source
│   ├── project_revision
│   ├── projection_digest
│   ├── motion_sha256
│   ├── motion_duration_ms
│   ├── motion_width
│   └── motion_height
├── hook
├── shots[3..=5]
│   ├── reviewed_step_id
│   ├── source_start_ms
│   ├── source_end_ms
│   ├── focus_path
│   │   ├── start normalized point
│   │   ├── end normalized point
│   │   └── zoom_permille
│   ├── speed_permille
│   ├── caption
│   └── transition
├── outro_text
└── provenance
    ├── deterministic_seed_version
    ├── optional_agent_run
    ├── optional_skill_receipt
    ├── repository_read_receipts[]
    └── accepted_user_edits[]
```

### 6.1 Stable numeric representation

Coordinates and rates use bounded integers rather than serialized floats:

- normalized coordinates use a documented integer range;
- zoom uses integer permille;
- speed uses an enumerated set of integer permille values;
- times use integer milliseconds.

This keeps validation, hashing, duration math, and rendering stable.

### 6.2 Fixed output

- width: 1920 pixels;
- height: 1080 pixels;
- frame rate: 30 fps;
- codec: H.264;
- pixel format: yuv420p;
- duration: 15,000–25,000 ms, allowing at most one output-frame tolerance during post-render verification;
- audio streams: zero.

### 6.3 Shot constraints

- exactly three through five shots;
- each shot references an existing reviewed Action Guide step;
- source ranges are absolute timestamps within the bound motion asset;
- source ranges are ordered and non-overlapping;
- post-speed and post-transition duration is calculated exactly before rendering;
- focus path coordinates remain in bounds;
- zoom and speed come from bounded allowed values;
- transition is either `Cut` or a bounded `Crossfade`;
- hook, caption, and outro lengths are bounded by bytes and characters;
- text fields contain no markup or executable expressions.

### 6.4 Source and stale binding

Every plan binds to:

- Action Guide project revision;
- projection digest;
- native motion SHA-256;
- native motion duration and dimensions.

Preview and final render reopen and validate the source. A revision, projection, or motion mismatch makes the plan stale. A stale plan cannot render. The user must regenerate it or use an explicit product-owned rebase flow that revalidates every referenced step and source range.

No silent rebase is permitted.

## 7. Deterministic Seed

The no-provider path is a complete product path, not a placeholder.

Seed generation:

1. requires at least three reviewed steps and available persistent motion;
2. preserves the first and last reviewed steps;
3. when more than five reviewed steps exist, evenly samples interior steps in guide order;
4. chooses at most five total steps;
5. allocates bounded source windows around each step timestamp;
6. uses fixed default focus, speed, caption, and transition values;
7. computes the final duration including speed and transition overlap;
8. returns either a valid plan or typed validation issues.

If the available source windows cannot form at least 15 seconds of valid output, seed generation does not invent footage or expand outside the motion asset. The Review screen displays the issues and keeps Render disabled until the user selects valid ranges or removes unusable choices while preserving the three-shot minimum.

## 8. Optional Repository Authority

Repository enrichment is opt-in and separate from Action Guide evidence disclosure.

### 8.1 User grant

The user chooses:

- one workspace root;
- explicit relative file or directory entries beneath that root.

Rollshot shows the proposed scope before starting the agent run. There is no implicit repository discovery and no reuse of a previous grant without a new visible confirmation for the run.

### 8.2 Path and content policy

The repository reader:

- canonicalizes the selected root once;
- rejects traversal, symlinks, special files, and paths outside the root;
- applies a denylist for credentials, keys, `.env` material, VCS internals, and equivalent sensitive paths even beneath an allowed directory;
- permits only bounded text-file types;
- enforces maximum files, bytes per file, total bytes, and returned text;
- reports truncation explicitly;
- checks cancellation between reads;
- never writes, executes, follows links, accesses the network, or invokes a shell.

Directory entries may grant recursive reads, but every actual file still passes the denylist, type, size, and canonical-root checks.

### 8.3 Typed tool and receipts

The model can obtain repository content only through a registered `read_authorized_project_text` tool.

The run authority receipt records:

- workspace-root identity without exposing an absolute private path in provider-visible data;
- canonical grant digest;
- operation grant;
- disclosure and resource ceilings.

Each successful or truncated read receipt records:

- relative path;
- content SHA-256;
- bytes read and bytes returned;
- truncation status.

The Review screen exposes the exact read ledger. Absolute local paths do not enter prompts, product artifacts intended for sharing, or provider-visible provenance.

### 8.4 Allowed influence

Repository evidence may improve official terminology, hook wording, and captions. It cannot:

- add footage that is absent from the reviewed Action Guide;
- select a non-reviewed step;
- make an unsupported product claim without a visible user-authored edit marker;
- bypass plan validation or content review.

## 9. Product Flow

### 9.1 Entry eligibility

Timeline Workspace exposes **Create teaser** when:

- the project is writable;
- native motion is available;
- at least three steps are reviewed.

The disabled control identifies the exact unmet condition: read-only project, no motion, unavailable motion, or too few reviewed steps.

### 9.2 Create

Selecting **Create teaser** produces the deterministic seed immediately. It does not mutate the Action Guide project. The draft remains an in-memory review artifact until a successful final render.

### 9.3 Review layout

The Review screen contains:

- an ordered shot-card list with step thumbnail, range, caption, speed, and transition;
- a selected-shot keyframe with a 16:9 crop/focus overlay and start/end focus handles;
- hook and outro editing;
- validation issues;
- provenance and repository-context controls.

Users may reorder cards and edit only bounded fields. Source range, speed, zoom, and transition controls cannot express invalid or unsupported operations.

### 9.4 Improve with Agent

The optional agent action behaves as follows:

1. the user chooses whether to add repository context;
2. repository context requires a separate visible scope confirmation;
3. the product creates a bounded, cancellable registered job;
4. the skill receives only authorized Action Guide evidence and authorized repository reads;
5. strict output decoding produces a second proposal;
6. the Review screen shows field-level differences;
7. the user may accept all, accept individual fields, or reject the proposal.

Agent completion never overwrites current user edits. Every accepted suggestion is recorded in provenance.

### 9.5 Preview

The first release does not add an inline Iced video decoder/player.

**Generate preview**:

- renders a temporary 960×540 MP4 through the same plan validation and renderer;
- opens it through the existing platform `open_path` mechanism;
- marks the preview stale after any plan edit;
- deletes temporary preview data when the teaser workspace closes.

Storyboard thumbnails and focus geometry remain visible in the Iced review surface.

### 9.6 Final render

Render is enabled only for a current, valid plan.

Before starting it, the user:

1. chooses a destination;
2. confirms that captured content and repository-derived copy were reviewed;
3. explicitly starts rendering.

The final render runs as a cancellable registered job. The destination is not created until the temporary output passes ffprobe validation and is atomically renamed.

### 9.7 Completion and persistence

On success, the UI shows:

- duration and dimensions;
- destination path;
- **Open**;
- **Show in Folder**.

Only after successful final render does Rollshot atomically persist the accepted plan and provenance as the derived sidecar `publish/launch-teaser-plan-v1.json`. The sidecar records the guide revision it was rendered from but does not change that revision; otherwise persisting it would make the plan immediately stale. A later guide revision leaves the sidecar available as historical provenance but marks it stale. The project does not duplicate the exported MP4.

## 10. State Machine

```text
SeededDraft → Reviewing ↔ AgentRunning
     │             │
     │             ├→ PreviewRendering → Reviewing
     │             └→ FinalRendering → Completed
     └──────── validation/stale errors remain in Reviewing

Any running state → Cancel → Reviewing
Project/motion revision change → Stale → regeneration required
```

Only one agent, preview, or final-render job may own the teaser workspace at a time.

Late messages from cancelled or superseded jobs are ignored by operation identity, following existing job-coordination patterns.

## 11. Renderer Pipeline

### 11.1 Input preparation

1. Reopen and hash the project motion asset.
2. Validate project and motion binding.
3. Validate the complete plan and exact output duration.
4. Rasterize hook, captions, and outro to bounded transparent PNGs using `rollshot-image-document`'s vendored-font path.
5. Create all intermediate assets inside one owned scratch directory.

### 11.2 Fixed FFmpeg graph

Rollshot compiles validated data into its own fixed graph:

- trim and timestamp reset;
- bounded crop/focus interpolation;
- scale and pad to the target resolution;
- speed adjustment through timestamp transformation;
- Rollshot-created image overlay;
- concat for cuts or bounded crossfade for crossfades;
- H.264/yuv420p encoding at 30 fps with no audio mapping.

User and model strings are never interpreted as:

- file paths;
- command arguments;
- filter names;
- filter expressions;
- codec or container options;
- graph fragments.

Text is rasterized before FFmpeg invocation, preventing text from entering FFmpeg expression syntax.

### 11.3 Output verification

ffprobe must confirm:

- one H.264 video stream;
- zero audio streams;
- 1920×1080 dimensions;
- 30 fps;
- duration within one frame of the validated plan and within 15–25 seconds.

Only then is the temporary file atomically renamed to the destination.

### 11.4 Provenance

The accepted artifact records:

- schema and renderer versions;
- FFmpeg and ffprobe versions;
- plan digest;
- source project revision and projection digest;
- source motion SHA-256;
- output SHA-256;
- deterministic seed version;
- optional skill and agent run receipts;
- optional authority and repository read receipts;
- accepted user edits.

## 12. Cancellation and Error Handling

### 12.1 Cancellation

Cancellation:

- signals the registered job;
- terminates the FFmpeg process tree;
- waits for process exit;
- removes scratch assets and temporary output;
- returns the UI to Review;
- preserves the editable accepted plan;
- never leaves a destination file that appears successful.

### 12.2 Error categories

Errors are typed and surfaced without leaking private paths or prompt content:

- ineligible project;
- stale project or motion;
- invalid plan;
- repository authority denied;
- repository path or content rejected;
- repository budget exceeded;
- provider unavailable;
- invalid agent proposal;
- FFmpeg unavailable or missing required capability;
- render process failed;
- output verification failed;
- destination I/O failed;
- cancelled.

Retryable render failures preserve the plan. Authority or digest failures terminate the affected operation and require a fresh grant or regeneration.

## 13. Resource Bounds

- 3–5 source clips;
- maximum 25 seconds of output;
- fixed maximum 1920×1080 final output;
- bounded preview resolution of 960×540;
- bounded text and overlay dimensions;
- one FFmpeg child per render;
- one active teaser job per workspace;
- bounded repository files, file bytes, total bytes, and returned text;
- bounded agent turns, tool calls, model tokens, attachments, and wall time through existing run budgets.

## 14. Verification Strategy

### 14.1 Plan and seed tests

Test observable contracts for:

- deterministic first/last and interior step selection;
- three- and five-shot boundaries;
- duration math across speed and transition combinations;
- source range ordering and bounds;
- normalized focus, zoom, and speed bounds;
- stale revision, projection, and motion rejection;
- unknown schema fields and unsupported versions;
- text limits and invalid transition rejection;
- insufficient source motion.

### 14.2 Repository authority tests

Test:

- no read without an explicit grant;
- canonical-root containment;
- symlink, traversal, and special-file rejection;
- denylist enforcement beneath an otherwise allowed directory;
- text extension and binary rejection;
- file-count, per-file, total-byte, and returned-text ceilings;
- truncation receipts;
- exact relative-path read ledger and content digests;
- cancellation and late-result suppression;
- absence of absolute paths in provider-visible data and shareable receipts.

### 14.3 Agent contract tests

Test:

- bundled package discovery and digest receipt;
- provider-neutral invocation;
- unavailable tools remain unavailable;
- strict proposal decoding and unknown-field rejection;
- review artifact creation without mutation;
- field-level accept/reject behavior;
- stale proposal handling;
- cancellation, terminal states, continuity, and audit provenance.

### 14.4 Renderer integration tests

Using synthetic motion and the resolved FFmpeg toolchain, exercise:

- trim;
- crop and focus movement;
- speed adjustment;
- text overlays;
- cuts and crossfades;
- preview and final output profiles;
- ffprobe verification of codec, resolution, frame rate, duration, and no audio;
- cancellation with no destination file;
- invalid or stale source rejection before process launch.

The tests verify visible/output contracts rather than FFmpeg command source text.

### 14.5 Product UI tests

Test:

- entry eligibility and each disabled reason;
- provider-free deterministic flow;
- review edits and validation gating;
- repository confirmation and read-ledger visibility;
- agent field-level diff review;
- preview staleness after edits;
- explicit content confirmation before final render;
- cancellation and superseded-job handling;
- successful persistence only after verified render;
- Open and Show in Folder actions.

### 14.6 Iced and platform evidence

Because this is a user-visible Iced change:

- invoke the repo-local `iced-rs` and `testing-iced-ui` skills before implementation;
- exercise shared Timeline Workspace scenarios at normal and narrow widths;
- send raw visual evidence to an independent clean-context reviewer for any golden baseline decision;
- inspect both Linux and macOS product entry paths;
- verify platform-specific Open and Show in Folder helpers;
- record any macOS runtime path that cannot be exercised on the Linux workstation as an explicit remaining runtime risk.

## 15. Acceptance Criteria

The MVP is complete when all of the following are demonstrated:

1. A writable Action Guide project with persistent motion and at least three reviewed steps enables **Create teaser**.
2. With no model provider, Rollshot creates a deterministic draft that can be edited, previewed, explicitly approved, and rendered.
3. The final output is a verified silent H.264/yuv420p MP4 at 1920×1080, 30 fps, and 15–25 seconds.
4. Every shot comes from a reviewed step and an in-bounds range of the bound native motion asset.
5. Project or motion changes make the plan stale and prevent rendering.
6. Preview and final rendering are cancellable and leave no false-success destination file.
7. With a provider, the bundled skill returns a reviewable proposal and cannot mutate or render directly.
8. Repository content is read only after explicit bounded authorization, and the user can inspect every file read.
9. Agent suggestions require explicit acceptance and preserve skill, authority, source, tool, and user-decision provenance.
10. No product path executes arbitrary agent-generated code or arbitrary FFmpeg operations.
11. Shared Timeline Workspace behavior is verified, both platform entry paths are inspected, and any unexercised macOS runtime risk is recorded.
12. A real-motion Rollshot quality case is completed before the feature is described as launch-ready.
