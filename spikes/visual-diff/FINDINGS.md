# Visual Diff Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: TBD
- Started: 2026-06-20
- Last updated: 2026-06-20

## Decision

How are proposed redaction candidates rendered and diffed in the Result Workspace,
and does the overlay approach scale to many candidates on tall stitched images?
Specific sub-questions:

1. **CPU-side geometry cost**: frustum culling + hit-testing + before/after diff at
   100/500/1000 candidates on ordinary (1920x1080) and tall (4000x12000) images.
2. **Iced 0.14 compile feasibility**: proposed vs accepted overlays + before/after
   toggle + `similar`-based source-diff pane + Workflow IR semantic-summary pane in
   a single scrollable + Canvas app.
3. **Headless GUI run**: smoke-test on the headless dev host.
4. **Data-model question (design recommendation, NOT spike-tested)**: transient
   review wrapper vs first-class Annotation variant.

## Environment

| Property | Value |
|---|---|
| OS | Ubuntu 22.04 / Linux 6.8.0-124-generic x86_64 |
| Host | Headless remote server (no display, no GPU) |
| CPU | Intel Core Ultra 7 265K, 8 threads |
| RAM | 32 GB |
| rustc | 1.96.0 (ac68faa20 2026-05-25) |
| iced | 0.14.0 |
| similar | 2.7.0 |
| criterion | 0.5.1 |
| xvfb-run | NOT available |
| Commit at spike start | 323b2b96 |
| Spike branch | feat/smart-redaction-agent-workbench |

## Risk Results

| Risk | Gate | Evidence | Result | Notes |
|---|---|---|---|---|
| CPU frustum cull at 1000 candidates (ordinary 1920x1080) | soft < 2ms | automated | PASS | 667 ns — 3000x margin |
| CPU frustum cull at 1000 candidates (tall 4000x12000) | soft < 2ms | automated | PASS | 689 ns — 2900x margin |
| CPU hit-test at 1000 candidates (ordinary) | soft < 2ms | automated | PASS | 427 ns |
| CPU hit-test at 1000 candidates (tall) | soft < 2ms | automated | PASS | 393 ns |
| CPU before/after diff at 1000 candidates (ordinary) | soft < 2ms | automated | PASS | 1.05 us |
| CPU before/after diff at 1000 candidates (tall) | soft < 2ms | automated | PASS | 1.28 us |
| iced 0.14 prototype compiles (overlay + toggle + source-diff + IR pane) | hard | compile | PASS | cargo build succeeded in ~30 s |
| Headless GUI run (interactive/GPU latency) | soft | runtime | UNTESTED | no xvfb; winit panicked: no DISPLAY set |
| macOS compile parity | soft | compile | UNTESTED | pending controller CI (Step 5 not performed) |

## Observations

### Step 2: CPU-side geometry benchmarks

All runs: `cargo bench` (release, criterion 0.5.1, 100 samples each).

#### (a) Frustum culling (overlay_cull/frustum) — median

| Candidates | ordinary 1920x1080 | tall 4000x12000 |
|---|---|---|
| 100 | 68 ns | 70 ns |
| 500 | 329 ns | 349 ns |
| 1000 | 667 ns | 689 ns |

O(n) linear scan. Both sizes well under 2 ms at 1000 candidates. The cull pass
uses 0.04% of a 16.6 ms frame at 1000 candidates.

#### (b) Point hit-testing (overlay_cull/hit_test) — median

| Candidates | ordinary 1920x1080 | tall 4000x12000 |
|---|---|---|
| 100 | 43 ns | 43 ns |
| 500 | 208 ns | 202 ns |
| 1000 | 427 ns | 393 ns |

O(n) scan with early-exit on first hit. Passes soft gate with >4000x margin.

#### (c) Before/after diff (overlay_cull/diff) — median

| Candidates | ordinary 1920x1080 | tall 4000x12000 |
|---|---|---|
| 100 | 247 ns | 237 ns |
| 500 | 609 ns | 620 ns |
| 1000 | 1.05 us | 1.28 us |

O(n+m) sorted-index merge. Most expensive operation but still ~1.3 us at 1000
candidates — 12,000x below the 16.6 ms frame budget.

Soft gate: all three operations PASS at 1000 candidates on both image sizes.

### Step 3: iced 0.14 prototype compile

`cargo build` succeeded (dev profile, ~30 s including dep compilation).

Prototype includes:
- AnnotationCanvas implementing canvas::Program<Message>: renders accepted
  annotations (opaque red fill) and proposed candidates (blue semi-transparent
  fill + outline; low-confidence <0.5 uses dimmer fill + thinner outline, per
  spec 8.2).
- Before/after toggle: OverlayMode enum toggled by a button; Cache::clear() on
  transition.
- similar-based source-diff pane: TextDiff::from_lines on old/new JS strings,
  rendered as colored line-by-line text.
- Workflow IR semantic-summary pane: hand-authored WorkflowIr for
  valid_detector.js (capabilities: ocr, layout-analysis; thresholds:
  confidence > 0.8, min_area_px > 400; candidate-count delta +4).

Compile evidence only. No display available; rendering correctness unverified.

### Step 4: Headless GUI run

xvfb-run not installed. Direct run without display:

  WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 cargo run

Result: winit panicked at startup:
  "neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set."

Outcome: UNTESTED. GPU/interaction latency remains an open risk. To close: run
spikes/visual-diff on a machine with a display. The CPU numbers suggest no
headroom issue, but GPU/composition overhead on tall images needs verification.

## Final Recommendation

### Spike-measured (evidence: automated + compile)

Go on the proposed rendering approach.

CPU-side geometry cost is negligible at all tested candidate counts. The full
iced 0.14 prototype (proposed/accepted overlay, before/after toggle, similar
source-diff pane, Workflow IR summary pane) compiles without error. The surface
selection (scrollable + Canvas) is confirmed viable.

Rendering approach: reuse the existing culling pattern from
result_workspace/canvas.rs (annotation_bounds(...).intersects(&visible)) unchanged.
No new data-structure work needed for the cull path.

Pending / not spike-measured:
- GPU/interaction latency on a real display: NOT obtainable on this headless host.
  Recommend a manual run before shipping. Risk is LOW given CPU numbers.
- macOS compile parity: UNTESTED — pending controller CI.

### Design recommendation (NOT spike-tested — design reasoning only)

The bench and compile steps did NOT exercise the data-model choice.

Recommended: proposed annotations as a transient review wrapper, not a
first-class Annotation variant.

Rationale:
- rollshot-image-document owns committed annotations; agent proposals are
  ephemeral until accepted. Mixing them would add agent concerns to an
  intentionally headless/framework-neutral crate.
- A transient ProposedCandidate { bounds, confidence, label } held in session
  state requires zero changes to rollshot-image-document. On accept, it converts
  to Annotation::OpaqueRedaction via the existing commit path.
- Before/after toggle is a pure session-state flag. No document mutation, no
  undo/redo participation.

Rejected alternatives:
- First-class Annotation variant: rejected — pollutes rollshot-image-document
  with agent concerns; requires undo/redo/serialization changes.
- Side-by-side before/after layout: toggle is simpler; side-by-side requires
  split scrollable + synchronized scroll state.

Fallback triggers:
- If GPU latency >8 ms at 1000 candidates while scrolling, use per-layer Cache
  (already present in prototype).
- If accept/reject UX is confusing, add explicit gesture — no data-model change.

Remaining risks:
- GPU/interaction latency: needs real-display run. Risk: LOW.
- macOS compile parity: UNTESTED.
- Tall-image texture upload: existing display_downscale_scale mitigates >8192 px
  limit; candidates don't change this risk.

Product handoff:
1. Implement ProposedCandidate wrapper in rollshot-app session state.
2. Add OverlayMode toggle to EditorState.
3. Extend AnnotationCanvas::draw to render proposed candidates after accepted.
4. Wire similar::TextDiff into the source-diff side panel.
5. Wire Workflow IR struct into the semantic-summary side panel.
6. Run prototype on a real display, record GPU/interaction latency at 500-1000
   candidates (closes UNTESTED risk).
7. Push spike branch and confirm macOS compile via CI.
