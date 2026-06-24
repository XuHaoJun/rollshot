# OCR Backend Design (Independent Subproject)

**Date:** 2026-06-24
**Status:** Approved design
**Spike:** `spikes/ocr-feasibility/FINDINGS.md` (GO, 2026-06-24)
**Related specs:**
- `docs/superpowers/specs/2026-06-21-automation-frontend-runtime-design.md` —
  Capability API v1 `rollshot.ocr(query)` and the `AutomationHost::ocr` trait
  this subproject makes real (the frontend spec deferred real adapters).
- `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md` —
  parent Smart Redaction design; this subproject is independent but unblocks
  its first use case (finding redaction candidates).

## 1. Summary

This subproject wires a real OCR backend into
`rollshot-vision::RealAutomationHost`, replacing the current
`capability_unavailable` stub for the `ocr` capability. It is **independent of
Smart Redaction / Preset Workbench (SP6)**: those subprojects will consume the
resulting capability later, but this subproject ships and verifies on its own.

The chosen backend mirrors `learn-projects/snow-shot`'s proven screenshot-OCR
stack, validated by `spikes/ocr-feasibility`:

- `paddle-ocr-rs =0.6.1` — RapidOCR det/cls/rec ONNX pipeline.
- `ort =2.0.0-rc.10` — ONNX Runtime (FFI).
- `ndarray =0.16.1` — pinned to keep a single ndarray in the graph.
- RapidOCR PP-OCRv4 ONNX models, **`ch` set** (Chinese + English, ~15.5 MB),
  **bundled in-app** via `include_bytes!` (offline, no model-fetch disclosure).

A new `rollshot-ocr` crate isolates the `unsafe` FFI behind a safe public API,
mirroring the `rollshot-macos-oneshot` isolation pattern, so the rest of the
workspace keeps `unsafe_code = "forbid"`. `rollshot-vision` depends on it and
wires a `prepare_ocr` / cached-`ocr` callback pair into `RealAutomationHost`,
exactly matching the existing `prepare_region_features` / `region_features`
precedent.

The subproject adds **real-OCR JavaScript end-to-end tests** for Smart Redaction
scenarios (email masking, SSN-like, key-value) — closing the gap between the
frontend spec's fake-host OCR→redaction test and the real backend.

## 2. Scope

### 2.1 In scope

- New `rollshot-ocr` workspace-member crate (`unsafe_code = "allow"`):
  - safe public API: `OcrEngine::new`, `OcrEngine::detect`;
  - bundles 3 PP-OCRv4 ONNX models via `include_bytes!` with SHA256 runtime
    verification;
  - exact pins: `paddle-ocr-rs = "=0.6.1"`, `ort = "=2.0.0-rc.10"`,
    `ndarray = "=0.16.1"`;
  - no rollshot dependencies (clean isolation).
- `rollshot-vision::RealAutomationHost`:
  - `prepare_ocr(&VisualIndex, &OcrQuery) -> Result<(), CapabilityError>` —
    expensive, runs outside QuickJS;
  - cached `ocr(&OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError>` callback
    replacing the `capability_unavailable` stub — region-filter + truncate,
    < 1 ms (mirrors `region_features`);
  - `OcrDetection` (primitives) → `OcrMatch { bounds: ImageRect, text,
    confidence }` mapping with quad-to-AABB conversion.
- Real-OCR JavaScript integration tests in `rollshot-vision` covering Smart
  Redaction scenarios on programmatically generated text fixture images.
- macOS **hard gate**: `cargo build/test -p rollshot-ocr` + one fixture run
  must pass on macOS.
- Completion handoff + parent spec status update + AGENTS.md/README.md project
  map updates.

### 2.2 Out of scope

- The `layout` / `inspectLayout` capability — remains stubbed; its own later
  subproject.
- `regionFeatures` and `templateMatch` — already implemented, unchanged.
- Agent sessions, provider adapters, preset/workbench UI, persistence, save
  handoff (SP4–SP8).
- Model download / first-run fetch — models are bundled.
- Non-`ch` recognition models (en-only, multilingual) — deferred.
- Rotated-screenshot angle correction as a default — `do_angle=false` by
  default; `detect_angle_rollback` noted as a carry-forward risk for real
  captures, not enabled by default in this subproject.

## 3. Crate and Dependency Boundaries

```text
rollshot-ocr  (NEW, unsafe_code = "allow", workspace member)
  ├─ OcrEngine { ocr: OcrLite }              // not Sync; caller-owned
  ├─ OcrEngine::new() -> Result<Self, OcrError>
  ├─ OcrEngine::detect(&RgbImage, &OcrRegionQuery) -> Result<Vec<OcrDetection>, OcrError>
  ├─ OcrDetection { x, y, w, h, text, confidence }   // primitives, no rollshot deps
  ├─ OcrRegionQuery { region, max_side_len, score_thresh, unclip_ratio, do_angle }
  └─ 3 PP-OCRv4 ONNX via include_bytes!  (det/cls/rec, ~15.5 MB)

rollshot-vision  (forbid(unsafe_code), stance unchanged)
  └─ RealAutomationHost
       ├─ prepare_ocr(&VisualIndex, &OcrQuery)   ← expensive, outside QuickJS
       └─ ocr(&OcrQuery) -> Vec<OcrMatch>        ← cached callback, < 1 ms (replaces stub)
```

`rollshot-ocr` dependencies: `paddle-ocr-rs = "=0.6.1"`,
`ort = "=2.0.0-rc.10"`, `ndarray = "=0.16.1"`, `num_cpus = "1.17.0"`,
`image` (workspace), `thiserror` (workspace), `tracing` (workspace),
`sha2` (for runtime model-hash verification). No `rollshot-*` dependency.

`rollshot-vision` adds `rollshot-ocr` as a path dependency. It already depends
on `rollshot-automation` (for the `AutomationHost` trait and query/result types)
and `rollshot-image-document` (for `ImageRect`).

### 3.1 Why an isolation crate

The workspace sets `unsafe_code = "forbid"` and carves out named isolation
crates for unavoidable `unsafe`: `rollshot-macos-oneshot` (objc2),
`rollshot-automation-rquickjs` (rquickjs), `rollshot-macos-input` (CGEventTap).
`ort` (ONNX Runtime FFI) and `paddle-ocr-rs` carry `unsafe`, so they require the
same treatment. Putting them directly in `rollshot-vision` and flipping it to
`allow` would break the workspace's unsafe-isolation discipline and transitively
expose unsafe to every vision consumer. The isolation crate exposes a safe API;
`rollshot-vision` stays `forbid(unsafe_code)`.

### 3.2 Version pinning is load-bearing

`spikes/ocr-feasibility` Stage 1 found that floating `ort = "2.0.0-rc.10"`
resolves to `2.0.0-rc.12`, which pulls a newer `ndarray` and produces **two
`ndarray` versions** in the graph, breaking `Tensor::from_array` trait bounds in
`paddle-ocr-rs`. The three pins (`ort`, `paddle-ocr-rs`, `ndarray`) must be
**exact** (`"=..."`) in `rollshot-ocr/Cargo.toml`, with a comment recording why.
Upgrading any of the three is an explicit engineering change that must rerun
the OCR unit and integration suites.

## 4. Query and Result Mapping

### 4.1 JavaScript surface (unchanged)

The frontend spec §6.1 defines the JavaScript contract; this subproject does
not change it:

```javascript
rollshot.ocr({ region, limit })
// -> [{ bounds: {x,y,width,height}, text, confidence }, ...]
```

`region` is `{ kind: "full" }` or `{ kind: "rect", bounds: {x,y,width,height} }`.

### 4.2 Rust host interface (unchanged trait, real impl)

`AutomationHost::ocr` stays as defined in the frontend spec §6.2. This
subproject replaces the stub body in `RealAutomationHost` with the
prepare/cached-callback pattern already used for `region_features`:

- `prepare_ocr(&VisualIndex, &OcrQuery)`:
  - crops to `OcrQuery.region` when `Rect` (bounded cost), upscales 1.5× for
    small text (snow-shot's approach for screenshot UI text);
  - calls `OcrEngine::detect` with the snow-shot-validated defaults;
  - maps `OcrDetection` → `OcrMatch` and caches under a canonical key
    (`region` + prepared `max_limit`);
  - records `image_dimensions` so the cached callback can validate.
- `ocr(&OcrQuery)` (QuickJS callback):
  - `limit == 0` → `CapabilityError::InvalidInput { code: "invalid_query" }`;
  - no prepared entry for the region → `CapabilityError::Failed { code:
    "vision_index_unavailable" }`;
  - `limit > prepared.max_limit` → `CapabilityError::LimitExceeded`;
  - otherwise region-filter + truncate to `limit`, return cached `OcrMatch`s.

This matches `RealAutomationHost::region_features` exactly (see
`crates/rollshot-vision/src/host.rs:136`), preserving the host contract:
enforce `limit` independently, < 1 ms callback, no detector work in the
callback.

### 4.3 Geometry mapping

Paddle `TextBlock.box_points` is a 4-point quad. `rollshot-ocr` converts to an
axis-aligned bounding box: `min/max` of x/y across the 4 points →
`OcrDetection { x, y, w, h }`. `rollshot-vision` maps that to
`rollshot_image_document::ImageRect { x, y, width, height }` for `OcrMatch.bounds`.

The host contract (frontend spec §6.2) requires finite coordinates and scores;
`rollshot-ocr` validates finiteness and rejects zero-area boxes before
returning. `confidence` = Paddle `text_score` (already in `[0,1]`).

### 4.4 Detection defaults

From snow-shot's validated screenshot parameters
(`learn-projects/snow-shot/.../ocr_service.rs`, `ocr_detect_core`):

| Parameter | Value | Source |
|---|---|---|
| `padding` | 50 | snow-shot default |
| `max_side_len` | 1024 | snow-shot default |
| `boxScoreThresh` | 0.5 | snow-shot default |
| `boxThresh` | 0.3 | snow-shot default |
| `unClipRatio` | 1.6 | snow-shot default |
| `do_angle` | false | this subproject default (carry-forward: enable `detect_angle_rollback` @ 0.9 if rotated captures misread) |
| small-text upscale | 1.5× when effective scale < 1.5 | snow-shot `ocr_detect_core` |

These are fixed defaults in `rollshot-ocr` for this subproject. Tuning them is a
later product-level decision, not part of this scope.

## 5. Model Bundling

Three files bundled via `include_bytes!` into `rollshot-ocr`, compiled into the
binary. No network access, no first-run fetch, no privacy disclosure for model
retrieval.

| File | Size | SHA256 (verified against RapidOCR `default_models.yaml` @ v3.1.0) |
|---|---|---|
| `ch_PP-OCRv4_det_infer.onnx` | 4.5 MB | `d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9` |
| `ch_ppocr_mobile_v2.0_cls_infer.onnx` | 571.8 KB | `e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c` |
| `ch_PP-OCRv4_rec_infer.onnx` | 10.4 MB | `48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b` |

Source: `https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.1.0/onnx/PP-OCRv4/{det,cls,rec}/...`
License: RapidOCR models are Apache-2.0; `paddle-ocr-rs` is MIT; `ort` is
MIT/Apache-2.0.

`OcrEngine::new()` verifies each bundled model's SHA256 once at session init and
returns `OcrError::ModelHashMismatch` on corruption (defense against binary
corruption, recorded in the spike). A unit test asserts the bundled bytes match
the recorded hashes.

The model files live under `crates/rollshot-ocr/models/` and are committed as
source assets (~15.5 MB, one-time). The repo root `.gitignore` only covers
`target/` and `spikes/*/target/`, so `crates/rollshot-ocr/models/*.onnx` is
tracked. (The spike's `spikes/ocr-feasibility/.gitignore` ignores `models/`
because spike models are disposable — that does not apply to the production
crate.)

## 6. Errors and Privacy

```rust
pub enum OcrError {
    SessionInit { code: &'static str },
    Detect { code: &'static str },
    InvalidImage,
    InvalidQuery { code: &'static str },
    ModelHashMismatch,
}
```

Mapped to `rollshot_automation::CapabilityError` in `rollshot-vision`:
`Failed { code }` for session/detect/invalid-image/hash-mismatch,
`InvalidInput { code }` for invalid query. The unprepared-callback case returns
`Failed { code: "vision_index_unavailable" }` (matches `region_features`).

Tracing (frontend spec §12): stable `rollshot::vision::ocr` target, structured
fields for duration, result count, and error code only. **No OCR text, query
contents, or image pixels in diagnostics.** This is critical for Smart
Redaction: the detected text may be the sensitive data the user wants hidden.

## 7. Real-OCR JavaScript End-to-End Tests

New file `crates/rollshot-vision/tests/ocr_integration.rs`, parallel to the
existing `integration.rs` (template-match e2e). It drives `RealAutomationHost` +
`QuickJsExecutor` against **real OCR** on **programmatically generated text
fixture images** (drawn with the `image` crate at test time — no committed
binaries, deterministic on Linux and macOS).

Scenarios (each is a `#[test]`):

1. **Email masking** — fixture image renders `contact@example.com` and some
   neutral text. JS: `rollshot.ocr` → `filter` (text contains `@`) → `map` →
   `addRedaction` with padded bounds. Assert: ≥1 candidate, and the candidate
   bounds overlap the rendered email's bounding box.
2. **SSN-like** — fixture renders `123-45-6789`. JS: `filter` (text contains
   `-` and has ≥4 digits). Assert: candidate lands on the SSN line.
3. **Key-value** — fixture renders `Token: AKIAEXAMPLEKEY`. JS: `filter` (text
   starts with `Token:`). Assert: candidate on the token line.
4. **No-match** — fixture with no text. JS produces zero candidates; no
   capability error.
5. **Bounded region query** — fixture with text both inside and outside a
   `Rect` region. JS uses `region: { kind: "rect", ... }`. Assert: only the
   in-region text yields candidates.

A shared helper renders text to an `RgbaImage` at a known position and returns
that position so assertions can check overlap. These tests close the gap
between the frontend spec's fake-host "OCR → filter → map → redaction" test
(`crates/rollshot-automation-rquickjs/tests/end_to_end.rs:86`, which already
uses the literal string `"secret@example.com"` as fake OCR text) and the real
backend.

## 8. Verification

### 8.1 Unit tests (`rollshot-ocr`)

- `OcrEngine::new` succeeds; bundled-model SHA256 matches recorded hashes.
- `detect` on a generated text image returns ≥1 `OcrDetection` with finite,
  non-zero-area bounds and `confidence` in `[0,1]`.
- `detect` on a blank image returns 0 detections (no error).
- `detect` rejects a zero-dimension image (`InvalidImage`).

### 8.2 Unit tests (`rollshot-vision`)

- `RealAutomationHost::ocr` unprepared → `vision_index_unavailable`.
- `prepare_ocr` then `ocr` returns the cached matches, truncated to `limit`.
- `limit == 0` → `invalid_query`; `limit > prepared` → `LimitExceeded`.
- `ocr` callback latency < 1 ms on a 200-entry cache (automated bench, mirrors
  spike Stage 3).
- Existing `all_unimplemented_capabilities_report_unavailable` test in
  `crates/rollshot-vision/src/lib.rs:34` is **updated**: `ocr` no longer
  reports `capability_unavailable` unprepared (it reports
  `vision_index_unavailable`, matching `template_match`); `layout` still
  reports `capability_unavailable`.

### 8.3 Integration tests

- `crates/rollshot-vision/tests/ocr_integration.rs` — the 5 Smart Redaction
  scenarios in §7.
- Existing `crates/rollshot-vision/tests/integration.rs` (template-match)
  continues to pass unchanged.

### 8.4 Required commands

```bash
rtk cargo test -p rollshot-ocr
rtk cargo test -p rollshot-vision
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test --workspace
```

### 8.5 macOS hard gate

`cargo build -p rollshot-ocr` and `cargo test -p rollshot-ocr` (including one
fixture-driven `detect`) must pass on macOS at MSRV 1.94. The spike was
Linux-only (no macOS hardware here); snow-shot ships this stack on macOS, so it
is expected to work, but **macOS verification is a success criterion**, not a
carry-forward. If macOS hits an `ort`/`paddle-ocr-rs` issue that cannot be
resolved within this subproject, the subproject is not complete and the spike's
fallback triggers apply (try `ort` load-dynamic or a newer ort release; worst
case, defer macOS and re-scope).

No real-display UI verification is required — this subproject contains no UI.

## 9. Documentation Updates

Committed with the final implementation (not the spec):

- `AGENTS.md` §9 Project Map:
  - add a `rollshot-ocr` entry (unsafe-isolation crate for RapidOCR/ONNX
    Runtime OCR, safe API, bundled PP-OCRv4 models, used by `rollshot-vision`);
  - update the `rollshot-vision` entry to note `ocr` is now real via
    `rollshot-ocr` and `layout` remains stubbed;
  - strengthen the `snow-shot` row in §10 to note it is the validated
    OCR-stack reference.
- `README.md` Workspace:
  - add `rollshot-ocr` and `rollshot-vision` entries (the latter is currently
    absent from the README workspace list).

## 10. Completion Handoff

Implementation is not complete until it adds:

`docs/superpowers/handoffs/YYYY-MM-DD-ocr-backend.md`

recording:

- delivered `rollshot-ocr` crate and exact dependency pins;
- public API and usage example;
- bundled model hashes and source URL;
- `RealAutomationHost::prepare_ocr`/`ocr` wiring;
- real-OCR integration test evidence (Linux + macOS);
- known limitations (angle handling, `layout` still stubbed, `ch`-set only);
- how SP6 / Smart Redaction consumes the `ocr` capability;
- migration considerations for `ort`/`paddle-ocr-rs`/`ndarray` upgrades.

The same change updates the parent Smart Redaction design's §12 Delivery
Decomposition status: the OCR backend (previously folded into "Technical
spikes" / "Automation frontend and runtime" as a deferred real adapter) is now
its own delivered subproject. This does not rewrite the parent's historical
decisions; it records delivery.

## 11. Decisions

1. Use `paddle-ocr-rs =0.6.1` + `ort =2.0.0-rc.10` + `ndarray =0.16.1`,
   exact-pinned (spike outcome; snow-shot precedent).
2. New `rollshot-ocr` isolation crate with `unsafe_code = "allow"`; safe public
   API; no rollshot dependencies.
3. Bundle the `ch` PP-OCRv4 ONNX set (~15.5 MB) in-app via `include_bytes!`;
   verify SHA256 at session init. No model download.
4. Wire `RealAutomationHost` with `prepare_ocr` + cached `ocr` callback
   mirroring the existing `region_features` pattern; < 1 ms callback.
5. Map Paddle quad → axis-aligned `ImageRect`; `text_score` → `confidence`.
6. Default `do_angle=false`; `detect_angle_rollback` is a carry-forward risk,
   not a default.
7. `layout` stays stubbed — separate subproject.
8. Real-OCR JavaScript e2e tests for Smart Redaction scenarios (email, SSN-like,
   key-value, no-match, bounded region) on generated text fixtures.
9. macOS is a hard gate, not a carry-forward.
10. Update AGENTS.md §9 and README.md Workspace with the new crate and the
    `rollshot-vision` behavior change at completion.

## 12. Success Criteria

This subproject is complete when:

1. `rollshot-ocr` builds with `unsafe` confined to the isolation crate; the
   rest of the workspace keeps `unsafe_code = "forbid"`.
2. `OcrEngine::new` verifies bundled model hashes; `detect` returns finite,
   non-zero-area `OcrDetection`s with `[0,1]` confidence on text images.
3. `RealAutomationHost::ocr` replaces the `capability_unavailable` stub with a
   `prepare_ocr`/cached-callback pair matching the `region_features` precedent.
4. The `ocr` QuickJS callback stays under 1 ms on a bounded cache (automated).
5. Real-OCR JavaScript integration tests pass for the 5 Smart Redaction
   scenarios on generated text fixtures.
6. No OCR text, query contents, or image pixels appear in `tracing` events.
7. `cargo test -p rollshot-ocr -p rollshot-vision`, workspace tests, fmt, and
   `clippy -- -D warnings` pass.
8. macOS build + test (including one fixture `detect`) passes — the hard gate.
9. AGENTS.md §9 and README.md Workspace are updated.
10. The completion handoff and parent spec §12 status update are present.
