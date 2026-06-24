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
    replacing the `capability_unavailable` stub — lookup-by-canonical-key +
    truncate, < 1 ms (mirrors `region_features`; eng-review D3);
  - lazy `Option<OcrEngine>` ownership keeping `RealAutomationHost::new()`
    cheap/infallible (eng-review D2);
  - a prepared-OCR **area cap** (mirroring `MAX_REGION_FEATURES_AREA`) so a
    `Region::Full` tall capture cannot exhaust memory/time (eng-review D13);
  - `OcrDetection` (primitives) → `OcrMatch { bounds: ImageRect, text,
    confidence }` mapping with quad-to-AABB conversion **and** crop-offset /
    upscale coordinate inversion to full-image native coords (eng-review D6).
- Real-OCR JavaScript integration tests in `rollshot-vision` covering Smart
  Redaction scenarios on programmatically generated text fixture images.
- **Feature gating (eng-review D17):** `rollshot-ocr` is an **optional**
  dependency behind a new **off-by-default `ocr` feature** on `rollshot-vision`;
  the workspace `default-members` **excludes** `rollshot-ocr` so the default
  build/test/CI lane skips the heavy `ort` + 15.5 MB toolchain entirely. A
  dedicated OCR CI lane builds `-p rollshot-ocr` and
  `rollshot-vision --features ocr` (the §8.5 macOS gate runs in that lane).
  Mirrors the existing `action-guide` precedent, taken one step further
  (default-members exclusion) because OCR is far heavier.
- macOS **hard gate**: `cargo build/test -p rollshot-ocr` + one fixture run
  must pass on macOS (in the OCR CI lane).
- Completion handoff + parent spec status update + AGENTS.md/README.md project
  map updates.

### 2.2 Out of scope

- The `layout` / `inspectLayout` capability — remains stubbed; its own later
  subproject.
- `regionFeatures` and `templateMatch` — already implemented, unchanged.
- Agent sessions, provider adapters, preset/workbench UI, persistence, save
  handoff (SP4–SP8).
- **Runtime** model download / first-run fetch — the shipped binary embeds the
  models (offline). Note: models are *not* in git; a `build.rs` provisions them
  at **build time** from a local dir or a checksummed download (§5, eng-review
  D16) — that is build-time only and does not affect the runtime guarantee.
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
  │     // owns the small-text upscale end-to-end: upscales internally, runs OCR,
  │     // then DIVIDES coordinates back so OcrDetection is in the INPUT image's
  │     // native space (eng-review D15). The caller never sees upscaled coords.
  ├─ OcrDetection { x, y, w, h, text, confidence }   // primitives, no rollshot deps
  ├─ OcrRegionQuery { max_side_len, min_scale, score_thresh, unclip_ratio, do_angle }
  │     // min_scale: upscale factor (default 1.5) applied when input is small;
  │     // applied + inverted inside detect.
  └─ 3 PP-OCRv4 ONNX via include_bytes! from OUT_DIR  (det/cls/rec, ~15.5 MB;
        provisioned by build.rs, NOT committed to git — see §5, eng-review D16)

rollshot-vision  (forbid(unsafe_code), stance unchanged)
  └─ RealAutomationHost
       ├─ prepare_ocr(&VisualIndex, &OcrQuery)   ← expensive, outside QuickJS
       └─ ocr(&OcrQuery) -> Vec<OcrMatch>        ← cached callback, < 1 ms (replaces stub)
```

`rollshot-ocr` dependencies: `paddle-ocr-rs = "=0.6.1"`,
`ort = "=2.0.0-rc.10"`, `ndarray = "=0.16.1"`, `num_cpus = "1.17.0"`,
`image` (workspace), `thiserror` (workspace), `tracing` (workspace),
`sha2` (for runtime model-hash verification). No `rollshot-*` dependency.

`[build-dependencies]` for `build.rs` (model provisioning, §5): `etcetera`
(workspace — default cache-dir resolution, same crate `rollshot-app` already
uses), `sha2` (build-time hash verification), and a lightweight HTTP client
(`ureq`) for the model download (ModelScope primary, optional Release mirror).
These are build-only and do not enter the
crate's runtime graph. (If the maintainer later chooses local-dir-only with no
fallback, the HTTP client drops out.)

`rollshot-vision` adds `rollshot-ocr` as an **optional** path dependency behind
an off-by-default `ocr` feature (eng-review D17):

```toml
# rollshot-vision/Cargo.toml
[features]
ocr = ["dep:rollshot-ocr"]                       # off by default
[dependencies]
rollshot-ocr = { path = "../rollshot-ocr", optional = true }
```

```toml
# workspace Cargo.toml — keep rollshot-ocr a member, exclude it from the default set
[workspace]
members      = [ …, "crates/rollshot-ocr" ]
default-members = [ …everything except "crates/rollshot-ocr"… ]
```

With the feature **off** (the default), `rollshot-vision` does not pull
`rollshot-ocr`, and `default-members` keeps the heavy crate out of bare
`cargo build`/`test` and the default CI lane. It already depends on
`rollshot-automation` (for the `AutomationHost` trait and query/result types)
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

### 3.3 ONNX Runtime native library (distribution, eng-review D4)

The "no network, no first-run fetch" guarantee in §5 covers the **models**, not
the ONNX Runtime engine itself. `ort 2.0.0-rc.10` needs a native ONNX Runtime
binary. Two strategies exist: ort's default `download-binaries` (fetches a
prebuilt lib from the pyke CDN at build time), or **self-provided static lib**
(point ort at a vendored static build via `ORT_LIB_LOCATION` and static-link).

**Adopt snow-shot's proven recipe (self-provided static lib).** `snow-shot`'s
release CI does not rely on the pyke CDN; it downloads a **static** ONNX Runtime
build from the pinned `supertone-inc/onnxruntime-build` GitHub Release and
static-links it. We mirror that, adapted to our `ubuntu-24.04` + `macos-14`
matrix:

- CI step downloads the per-OS static asset from
  `supertone-inc/onnxruntime-build` (e.g. `onnxruntime-osx-universal2-static_lib-*.tgz`
  for macOS, the matching `onnxruntime-linux-x64-static_lib-*.tgz` for Linux),
  extracts `lib/`, and points ort at it (`ORT_LIB_LOCATION`).
- macOS forces static linking as snow-shot does
  (`PKG_CONFIG_ALL_STATIC=1`, `MACOSX_DEPLOYMENT_TARGET` set).
- `actions/cache` keys the ORT lib on its version (same warm/cold pattern as the
  models in §5); `swatinem/rust-cache` covers the Rust build.

Static linking means **no `.so`/`.dylib` to ship at runtime** — the engine is in
the binary, matching the bundled-models offline stance.

**Version pin.** Pin supertone **`1.22.2`** — the version snow-shot ships with the
same `ort = 2.0.0-rc.10`, and supertone publishes `1.22.2` static libs for
linux-x64, osx-universal2, and win-x64 (all confirmed to exist). Point `ort` at it
via `ORT_LIB_LOCATION` (the env var `ort-sys` rc.10 reads; rc.12 renamed it to
`ORT_LIB_PATH` and changed feature behavior — do **not** adopt rc.12's env var or
features while pinned to rc.10). `ort-sys` rc.10 *declares* ONNX Runtime `1.22.0`;
the only residual unknown is that patch-level match, which the Linux+macOS OCR CI
lane gates (snow-shot runs this exact combination in production). If a future ort
bump breaks the static build, fall back to ort `download-binaries`, or
`load-dynamic` as the last resort (spike fallback trigger). The macOS hard gate
(§8.5) validates the strategy links on macOS; the completion handoff records it
for SP6 packaging.

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

**Feature gating (eng-review D17).** The real `prepare_ocr` and the real `ocr`
body are `#[cfg(feature = "ocr")]`. With the feature **off**,
`AutomationHost::ocr` keeps returning today's `capability_unavailable` stub
(`host.rs:124`) — the trait method itself stays unconditional and always
compiles. With the feature **on**, the stub is replaced by the
prepare/cached-callback impl below. Only the OCR fields/impl and the
`rollshot-ocr` dependency are gated, not the trait surface.

**`OcrEngine` lifecycle (eng-review D2).** `RealAutomationHost` holds
`ocr_engine: Option<OcrEngine>` (itself `#[cfg(feature = "ocr")]`), **lazily**
constructed on the first `prepare_ocr` call. `RealAutomationHost::new()` stays cheap and infallible (no
behavior change for existing template/region callers and tests); the
`OcrEngine::new` cost (~90 ms model load + hash verify) and any init error are
paid inside `prepare_ocr` (the "expensive, outside QuickJS" step), surfaced as
`CapabilityError::Failed { code: "ocr_session_init" }`.

**Cache model (eng-review D3).** This mirrors `region_features` *exactly*: the
prepared entry is keyed on the **canonical region pixel-rect** (via the existing
`region_to_pixel_rect`), and the callback does **lookup-by-key + truncate
only** — there is no per-call region filtering. (Earlier drafts mixed the
`region_features` exact-key model with the spike's prepare-Full-then-filter
model; this resolves it to the precedent the section claims to match.)

- `prepare_ocr(&VisualIndex, &OcrQuery)`:
  - resolves `OcrQuery.region` to a canonical pixel-rect; rejects non-finite /
    empty regions via `region_to_pixel_rect` (parity with `region_features`,
    eng-review D11), and enforces the prepared-OCR area cap (eng-review D13);
  - crops to that rect (bounded cost), recording the **crop origin** only.
    The small-text upscale and its inversion now live **inside**
    `OcrEngine::detect`, which returns coordinates in the cropped image's
    native space (eng-review D15) — `rollshot-vision` never deals with the
    upscale factor;
  - calls `OcrEngine::detect` with the snow-shot-validated defaults;
  - maps `OcrDetection` → `OcrMatch` by adding only the **crop origin**
    (`bounds = detection_aabb + crop_origin`; eng-review D6/D15) and caches
    under the canonical region-rect key + prepared `max_limit`;
  - records `image_dimensions` so the cached callback can validate.
- `ocr(&OcrQuery)` (QuickJS callback):
  - `limit == 0` → `CapabilityError::InvalidInput { code: "invalid_query" }`;
  - no prepared entry for the canonical region key → `CapabilityError::Failed {
    code: "vision_index_unavailable" }`;
  - `limit > prepared.max_limit` → `CapabilityError::LimitExceeded`;
  - otherwise truncate cached `OcrMatch`s to `limit` and return them.

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

**Coordinate responsibility split (eng-review D6 + D15).** The upscale and its
inversion are owned by the isolation crate; region placement is owned by vision:

```text
rollshot-ocr::detect:   upscale input ×min_scale → OCR → divide coords ÷min_scale
                        ⇒ OcrDetection in the INPUT (cropped) image's native space
rollshot-vision:        OcrMatch.bounds = OcrDetection_aabb + crop_origin
                        ⇒ then clamp + validate (finite, non-zero-area)
```

The spike (`spikes/ocr-feasibility`) OCR'd whole fixtures at native resolution
with no upscale, so the upscale-inversion is new surface not covered by the
spike's `valid_shape=true` evidence — and it is now tested **where it lives**:

- `rollshot-ocr` unit test: a glyph at a known position in a small image,
  after internal ×1.5 upscale, comes back with bounds (in input-native coords)
  overlapping that position (§8.1).
- `rollshot-vision` test: a detection in a cropped region maps to full-image
  bounds by crop-offset addition only (§8.2).

A wrong inversion silently misplaces redactions over the very PII they must
cover, so both halves are asserted independently.

### 4.4 Detection defaults

From snow-shot's validated screenshot parameters
(`learn-projects/snow-shot/.../ocr_service.rs`, `ocr_detect_core`):

| Parameter | Value | Source |
|---|---|---|
| `padding` | 50 | snow-shot default |
| `max_side_len` | region's longest side (no fixed cap) | snow-shot `ocr_detect_core` (`max_size = max(width,height)`) — see eng-review D1 |
| `boxScoreThresh` | 0.5 | snow-shot default |
| `boxThresh` | 0.3 | snow-shot default |
| `unClipRatio` | 1.6 | snow-shot default |
| `do_angle` | false | this subproject default (carry-forward: enable `detect_angle_rollback` @ 0.9 if rotated captures misread) |
| small-text upscale (`min_scale`) | 1.5× when input is small | snow-shot `ocr_detect_core`; applied **and inverted inside `OcrEngine::detect`** (eng-review D15), the spike did **not** apply this — new surface |

**`max_side_len` correction (eng-review D1).** Earlier drafts set
`max_side_len = 1024` and labelled it a snow-shot default. It is **not**:
snow-shot's `ocr_detect_core` passes `image.height().max(image.width())` (the
image's own longest side, i.e. *no* downscaling); `1024` was the spike's value,
and the spike's fixtures are small so it never downscaled. Rollshot's signature
output is **tall scrolling captures** (e.g. 1080×20000); a fixed 1024 long-side
cap would squash such an image to ~1024 px tall and destroy all text. Therefore
`rollshot-ocr` passes the **prepared region's** longest side (snow-shot
behavior), bounded by the `Rect` query and the prepared-OCR **area cap**
(eng-review D13) so a `Region::Full` tall capture cannot blow up memory/time.

These are fixed defaults in `rollshot-ocr` for this subproject. Tuning them is a
later product-level decision, not part of this scope.

## 5. Model Bundling

Three ONNX files are compiled into the `rollshot-ocr` binary via `include_bytes!`
so the **runtime** stays fully offline: no network access, no first-run fetch, no
privacy disclosure for model retrieval. The model bytes are **not committed to
git** (eng-review D16); a `build.rs` provisions them into `OUT_DIR` at build time
and `lib.rs` does `include_bytes!(concat!(env!("OUT_DIR"), "/…onnx"))`. This
keeps ~15.5 MB of binary blobs out of the repo history while preserving the
embedded-at-runtime guarantee.

| Embedded name (`include_bytes!`) | Size | SHA256 |
|---|---|---|
| `ch_PP-OCRv4_det_infer.onnx` | 4.5 MB | `d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9` |
| `ch_ppocr_mobile_v2.0_cls_infer.onnx` | 571.8 KB | `e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c` |
| `ch_PP-OCRv4_rec_infer.onnx` | 10.4 MB | `48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b` |

Source: RapidOCR's official ModelScope distribution at tag **v3.9.0**, which
publishes these PP-OCRv4 `ch` models under a `_mobile.onnx` suffix:
`https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.0/onnx/PP-OCRv4/{det,rec}/ch_PP-OCRv4_{det,rec}_mobile.onnx`
(cls: `.../cls/ch_ppocr_mobile_v2.0_cls_mobile.onnx`). The v3.9.0 `_mobile.onnx`
files are **byte-identical** to the SHA256s above; `build.rs` downloads under the
official `_mobile.onnx` name (its `cache_name`) and writes `OUT_DIR` under the
stable `_infer.onnx` name (`out_name`) that `lib.rs` `include_bytes!`s, so the
upstream rename never reaches the embed paths. We pin these exact PP-OCRv4 `ch`
hashes deliberately — `build.rs` must **not** auto-track RapidOCR's evolving
default model set (now extended to PP-OCRv5/v6); the SHA256s define the models.
License: RapidOCR models are Apache-2.0; `paddle-ocr-rs` is Apache-2.0; `ort` is
MIT/Apache-2.0.

`OcrEngine::new()` verifies each bundled model's SHA256 and returns
`OcrError::ModelHashMismatch` on corruption (defense against binary corruption,
recorded in the spike). Because the bytes are compile-time `include_bytes!`
constants, the hash check runs **once per process** (guarded by a `OnceLock`),
not on every `OcrEngine::new()` — re-hashing 15.5 MB of immutable constants on
each construction adds latency for no added safety (eng-review D12). A unit test
asserts the bundled bytes match the recorded hashes.

**Init entry point (eng-review D7).** The spike loaded models by *file path*
(`OcrLite::init_models`); the bundled-bytes design instead uses the in-memory
path — `OcrLite::init_models_from_memory_custom(det, cls, rec, build_session)`,
the same entry point snow-shot ships, with the session builder configured as
snow-shot does (`inter_threads = intra_threads = num_cpus::get_physical()`,
`GraphOptimizationLevel::Level3`). This path is validated by snow-shot but **not**
by the Linux spike, so `OcrEngine::new` (memory-init + hash match) is covered by
a unit test (§8.1).

**Model provisioning (eng-review D16 — models are NOT in git).** There is **no
model file on disk at runtime** — the bytes are `include_bytes!`-embedded, so the
running app never reads a model from any config/data/cache dir. A model file only
exists at **build time**, in the cache dir below and then in `OUT_DIR`.
`crates/rollshot-ocr/models/` is git-ignored (defensive — nothing is ever
committed there). `build.rs` resolves the three models, **verifies each SHA256
against the table above (fail the build on mismatch)**, and copies them into
`OUT_DIR`:

1. **Local cache dir first** — `$ROLLSHOT_OCR_MODELS_DIR` if set, else the
   default **`etcetera` cache dir**, resolved exactly like the existing
   `rollshot_config_dir()` (`rollshot-app/src/daemon/config.rs:170`) but using
   `cache_dir()` (the models are regenerable downloads, not user config):
   `choose_base_strategy().cache_dir().join("rollshot/ocr-models")`
   → `~/.cache/rollshot/ocr-models/` (Linux), `~/Library/Caches/rollshot/ocr-models/`
   (macOS). The cache is keyed by the official `_mobile.onnx` filename
   (`cache_name`); this dir is shared across worktrees/branches, so a model is
   fetched at most once per machine. Drop a personal backup here or point
   `$ROLLSHOT_OCR_MODELS_DIR` at it for a fully offline build.
2. **Checksummed download** — if a model is absent locally, fetch it and verify
   its SHA256 before use, so fresh checkouts and CI build without manual setup:
   - **RapidOCR ModelScope official URL (primary).** The hardcoded v3.9.0
     ModelScope URLs in the table above are the primary source — currently live
     and requiring no maintainer action. This closes the original provisioning
     hole: the earlier design assumed a pre-existing `xuhaojun/rollshot` GitHub
     Release (`ocr-models-v3.1.0`) as the download source, but that release does
     not exist, so a cold build would hard-fail.
   - **GitHub Release mirror (optional fallback).** If the ModelScope fetch
     fails (CN host can be unreliable from GitHub-hosted runners), `build.rs`
     falls back to a GitHub Release asset (`ocr-models-v3.1.0` tag, `_mobile.onnx`
     names). This is an **optional mirror, not a prerequisite** — the maintainer
     may upload the backup there later, but a missing release no longer blocks
     the build. A release asset is not in the git tree, so it still honors
     "models not in git".
   Both sources are SHA256-verified against the table; a mismatch fails the build.

Build-time provisioning is independent of the **runtime** offline guarantee
(the shipped binary embeds the verified bytes and never phones home). The
build-time SHA256 check plus the once-per-process runtime check (eng-review D12)
together cover provisioning errors and binary corruption.

**CI provisioning (GitHub Actions — eng-review D16).** The workspace CI is the
stock GitHub-hosted matrix (`ubuntu-24.04` + `macos-14`, ephemeral; the
`macos-14` job also satisfies the §8.5 macOS gate). The `etcetera` `cache_dir()`
default resolves to a **different absolute path per OS**, which is awkward to
cache, so CI **sets `$ROLLSHOT_OCR_MODELS_DIR` to a fixed workspace path** (e.g.
`$GITHUB_WORKSPACE/.ocr-models`) and caches that — deterministic across the
matrix. Add an `actions/cache` step on that directory keyed by the three pinned
SHA256s (a static key — it changes only when models are upgraded, and an active
repo keeps the cache warm indefinitely since the eviction timer resets on each
hit):

- **warm cache** → models restored, build is fully offline (no network);
- **cold cache** (new key/branch, fork PR, eviction, outage) → the step 2
  download (ModelScope primary, optional Release mirror) fetches once,
  SHA256-verifies, and re-populates the cache.

A cache alone cannot guarantee presence (it is best-effort), so the step 2
download is what makes a cold cache recoverable; the two are complementary, not
alternatives. (Pure local-dir-only with *no* fallback would hard-fail CI on any
cold miss and is only robust on a self-hosted runner / pre-baked image, which
this project does not use.)

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
fixture images**.

**Deterministic text rendering (eng-review D8).** The `image` crate cannot
render text — it draws pixels only. Determinism across Linux and macOS requires
a *committed* font; a system font would draw different glyphs per platform. The
repo already vendors one: `rollshot_image_document::style::FONT_REGULAR_BYTES`
(DejaVuSans, `include_bytes!`). Fixtures render with these vendored bytes (via
`imageproc::drawing::draw_text_mut` + an `ab_glyph` dev-dependency, or the
existing `rollshot-image-document` text path) at a large size / high contrast so
recognition is robust. No committed binary fixture images.

**Real-OCR flakiness (eng-review D10).** These scenarios depend on the
recognizer producing exact substrings (`@`, `-`, `Token:`), which real OCR can
occasionally miss. To keep them meaningful without flaking CI: they live in the
integration suite (separate from `cargo test -p rollshot-ocr`, so a recognition
miss never blocks the core unit suite); fixtures render large/high-contrast with
the vendored font; and assertions prefer detection + bounds-overlap over exact
text where the scenario allows. The real-OCR flakiness risk is documented in the
completion handoff.

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
- **Upscale inversion (eng-review D6/D15):** a glyph at a known position in a
  small image, after `detect`'s internal ×1.5 upscale, returns bounds in the
  **input image's native coordinates** (not upscaled) that overlap that
  position. This pins the upscale + inversion inside `rollshot-ocr`.

### 8.2 Unit tests (`rollshot-vision`)

- `RealAutomationHost::ocr` unprepared → `vision_index_unavailable`.
- `prepare_ocr` then `ocr` returns the cached matches, truncated to `limit`.
- `limit == 0` → `invalid_query`; `limit > prepared` → `LimitExceeded`.
- `prepare_ocr` rejects a non-finite region and an empty (zero-area) region,
  mirroring `region_features` (`non_finite_region` / `empty_region`) — parity
  gap close (eng-review D11).
- **Crop-offset mapping (eng-review D6/D15):** a detection in a cropped
  `Rect` region maps to `OcrMatch.bounds` in full-image native coords by adding
  the crop origin only (vision no longer touches the upscale — that is covered
  by the `rollshot-ocr` test in §8.1).
- **Privacy (eng-review D9, success criterion #6):** with a `tracing-subscriber`
  capture layer installed, run `prepare_ocr` + `ocr` over a fixture containing a
  known secret string and assert **no captured event field contains that string**
  (nor raw pixels) — only duration / result-count / error-code fields appear.
- `ocr` callback latency < 1 ms on a 200-entry cache (automated bench, mirrors
  spike Stage 3).
- Existing `all_unimplemented_capabilities_report_unavailable` test in
  `crates/rollshot-vision/src/lib.rs:34` is made **cfg-aware** (eng-review D17):
  with the `ocr` feature **off**, `ocr` still reports `capability_unavailable`
  (the stub); with the feature **on**, unprepared `ocr` reports
  `vision_index_unavailable` (matching `template_match`). `layout` reports
  `capability_unavailable` in both cases.

### 8.3 Integration tests

- `crates/rollshot-vision/tests/ocr_integration.rs` — the 5 Smart Redaction
  scenarios in §7. The whole file is gated (`#![cfg(feature = "ocr")]`), so it
  only compiles/runs under `--features ocr` (eng-review D17).
- Existing `crates/rollshot-vision/tests/integration.rs` (template-match)
  continues to pass unchanged, with or without the `ocr` feature.

### 8.4 Required commands

Two lanes (eng-review D17). The **default lane** skips the OCR toolchain
(`rollshot-ocr` is excluded from `default-members` and the `ocr` feature is off);
the **OCR lane** builds it explicitly:

```bash
# Default lane (every PR — no ort, no models, fast)
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk cargo test  --workspace --exclude rollshot-ocr

# OCR lane (dedicated — builds ort + provisions models; also the §8.5 macOS gate)
rtk cargo clippy -p rollshot-ocr -p rollshot-vision --features rollshot-vision/ocr --all-targets -- -D warnings
rtk cargo test  -p rollshot-ocr
rtk cargo test  -p rollshot-vision --features ocr
```

(`cargo fmt --all` formats every member including `rollshot-ocr` — formatting is
build-free, so it stays in the default lane.)

The OCR lane runs **automatically** (no manual trigger) but is **path-filtered**:
a separate `ci-ocr.yml` fires on PRs that touch OCR-relevant paths and always on
`main` push (§9, eng-review D17). PRs that don't touch OCR skip it entirely.

**Test-suite timing (eng-review D14).** `OcrEngine` is not `Sync`, so it cannot
be shared across the default parallel test threads — each OCR `#[test]`
reconstructs an engine (~90 ms init + detect). With the feature gate (D17) this
cost lands only in the **OCR lane**, not the default `--workspace --exclude
rollshot-ocr` run. The completion handoff records the measured OCR-lane
wall-clock; if it grows excessive, group scenarios into fewer `#[test]`s that
share one engine, or move the heaviest to a `#[ignore]`d slow lane.

### 8.5 macOS hard gate

The macOS leg of the **OCR CI lane** (§8.4) is the hard gate: `cargo build
-p rollshot-ocr`, `cargo test -p rollshot-ocr` (including one fixture-driven
`detect`), and `cargo test -p rollshot-vision --features ocr` must pass on
`macos-14` at MSRV 1.94. (The default lane excludes `rollshot-ocr`, so the gate
lives in the OCR lane, not the default `--workspace` run.) The spike was
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
  - update the `rollshot-vision` entry to note `ocr` is real behind the
    off-by-default `ocr` feature (via `rollshot-ocr`) and `layout` remains
    stubbed;
  - note `rollshot-ocr` is excluded from `default-members` and built only in
    the dedicated OCR feature lane;
  - strengthen the `snow-shot` row in §10 to note it is the validated
    OCR-stack reference.
- `README.md` Workspace:
  - add `rollshot-ocr` and `rollshot-vision` entries (the latter is currently
    absent from the README workspace list).
- `.github/workflows/ci.yml` (eng-review D17): keep the default lane on
  `--workspace --exclude rollshot-ocr` (runs on every PR, no `ort`/models).
- **New `.github/workflows/ci-ocr.yml`** (eng-review D17 — path-filtered, **not**
  manual): the OCR lane on both `ubuntu-24.04` and `macos-14` (`-p rollshot-ocr`
  + `rollshot-vision --features ocr`, with the model `actions/cache` + ORT
  static-lib provisioning from §3.3 / §5). The `macos-14` leg is the §8.5 gate.
  Triggers:

  ```yaml
  on:
    pull_request:
      paths:                              # OCR + everything it depends on
        - "crates/rollshot-ocr/**"
        - "crates/rollshot-vision/**"
        - "crates/rollshot-automation/**"
        - "crates/rollshot-image-document/**"
        - "Cargo.toml"                    # workspace deps / members
        - ".github/workflows/ci-ocr.yml"
    push:
      branches: [main]                    # safety net: always run on merge
  ```

  Rationale: unrelated PRs (stitcher, overlay, …) skip the heavy OCR build
  automatically; the `main` push run catches any cross-crate breakage that a
  path filter alone would miss.

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
11. `OcrMatch.bounds` are returned in **full-image native coordinates**:
    `rollshot-ocr` inverts the upscale internally (verified by the §8.1 test)
    and `rollshot-vision` adds only the crop offset (verified by the §8.2 test)
    (eng-review D6/D15).
12. A `Region::Full` OCR on a tall capture is bounded by the prepared-OCR area
    cap (no OOM/unbounded stall); `max_side_len` follows snow-shot (region's
    longest side), not a fixed 1024 (eng-review D1/D13).
13. The `.onnx` models are **not** committed to git; `build.rs` provisions them
    into `OUT_DIR` with build-time SHA256 verification, and the runtime stays
    offline (embedded via `include_bytes!`) (eng-review D16).
14. OCR is behind the off-by-default `ocr` feature with `rollshot-ocr` excluded
    from `default-members`: the default lane
    (`cargo clippy/test --workspace --exclude rollshot-ocr`) builds **without**
    `ort`/models and `ocr` reports `capability_unavailable`; the dedicated OCR
    lane (`--features ocr`) builds the engine and passes all OCR tests on both
    `ubuntu-24.04` and `macos-14` (eng-review D17).

## 13. Engineering Review Revisions (2026-06-24)

A `plan-eng-review` pass (auto mode) against this spec + `learn-projects/snow-shot`
+ `spikes/ocr-feasibility` recorded the following decisions, applied inline
above. They tighten correctness within the already-approved architecture (crate
isolation, backend choice, bundling, prepare/cached-callback wiring are
**unchanged**); none reverse an approved decision.

| # | Decision | Where applied |
|---|---|---|
| D1 | `max_side_len` is **not** 1024 (that was the spike's value); snow-shot uses the region's longest side. Fixed 1024 destroys text on tall captures — pass the region's longest side, bounded by D13. | §2.1, §4.4, §12.12 |
| D2 | `RealAutomationHost` owns a **lazy** `Option<OcrEngine>`; `new()` stays cheap/infallible. | §2.1, §4.2 |
| D3 | Resolve the §4.2 cache-model contradiction to the **`region_features` exact-key** precedent: lookup-by-canonical-key + truncate only (no per-call region filter). **Revisit when SP6 lands** — if redaction scripts typically `ocr(Full)` then filter many sub-regions in JS, the spike's prepare-Full-then-filter model may fit better. | §2.1, §4.2 |
| D4 | Specify the **ONNX Runtime native-lib** strategy. **Adopts snow-shot's recipe**: vendor a **static** ORT lib from `supertone-inc/onnxruntime-build` (per-OS), `ORT_LIB_LOCATION` + static-link, `actions/cache` by version; no runtime `.dylib`. Version-match caveat (snow-shot ORT 1.22.2 vs `ort 2.0.0-rc.10`); `download-binaries`/`load-dynamic` as fallbacks. | §3.3 |
| D5 | This spec needs a `superpowers:writing-plans` plan before execution; decomposition: T1 crate (incl. `build.rs`/provisioning) → T2 host wiring + `ocr` feature gate → T3 integration tests → T4 CI lanes (`ci.yml` default + OCR lane, eng-review D17) → T5 docs/handoff (T4/T5 parallel-ish), tests-first per §8. | this section, §8.4, §9 |
| D6 | Specify and **test** coordinate mapping → full-image native coords. (Split by D15: upscale inversion lives in `rollshot-ocr`, crop-offset addition in `rollshot-vision`.) | §2.1, §4.3, §8.1, §8.2, §12.11 |
| D7 | Init via `init_models_from_memory_custom` (snow-shot path, not the spike's file-path `init_models`); cover bundled-bytes init in a unit test. | §5 |
| D8 | Render text fixtures with the **vendored DejaVu font** (`rollshot_image_document::style::FONT_REGULAR_BYTES`), not "the image crate" — required for cross-platform determinism. | §7 |
| D9 | Add a **tracing privacy test** asserting no OCR text/pixels appear in events (success criterion #6 was untested). | §8.2 |
| D10 | Treat real-OCR substring assertions as integration-only + large/high-contrast fixtures + bounds-overlap where possible; document flakiness. | §7 |
| D11 | Add **non-finite / empty region** rejection tests for `prepare_ocr` (parity with `region_features`). | §4.2, §8.2 |
| D12 | Verify bundled-model SHA256 **once per process** (`OnceLock`), not per `OcrEngine::new()`. | §5 |
| D13 | Enforce a prepared-OCR **area cap** (mirror `MAX_REGION_FEATURES_AREA`) so tall `Region::Full` OCR is bounded. | §2.1, §4.4, §12.12 |
| D14 | Measure the OCR test-suite wall-clock (engine is `!Sync` → per-test init); group/`#[ignore]` only if it pushes `--workspace` past ~30 s. | §8.4 |

### Follow-up architecture decisions (2026-06-24, post-review discussion)

| # | Decision | Where applied |
|---|---|---|
| D15 | **Move the small-text upscale + its coordinate inversion into `rollshot-ocr::detect`** (returns input-native coords). `rollshot-vision` only adds the crop offset. Cleaner boundary: the isolation crate owns the scale trick, vision owns region placement. | §3, §4.2, §4.3, §4.4, §8.1, §8.2, §12.11 |
| D16 | **Models are NOT committed to git.** `crates/rollshot-ocr/models/` is git-ignored; `build.rs` provisions the three `.onnx` into `OUT_DIR` (local `$ROLLSHOT_OCR_MODELS_DIR`/cache first, **RapidOCR ModelScope official URL** primary download, optional GitHub Release mirror fallback) with build-time SHA256 verification; `lib.rs` `include_bytes!` from `OUT_DIR`. Runtime stays offline. CI (GitHub-hosted `ubuntu-24.04` + `macos-14`) layers `actions/cache` keyed on the model SHA256s; cold cache recovers via the download. | §2.2, §3, §5, §12.13 |
| D17 | **Feature-gate OCR (Level 2).** `rollshot-vision` gets an off-by-default `ocr = ["dep:rollshot-ocr"]` feature; `rollshot-ocr` is excluded from workspace `default-members`. Real `prepare_ocr`/`ocr` are `#[cfg(feature="ocr")]` (stub when off); trait stays unconditional. CI splits into a default lane (`--workspace --exclude rollshot-ocr`, no ort/models, every PR) and a **path-filtered, auto** OCR lane (`ci-ocr.yml`: `--features ocr`, ubuntu+macos, hosts the §8.5 gate) that fires on OCR-relevant PR paths + always on `main` push — not manual. Mirrors `action-guide`, plus default-members exclusion + path filter because OCR is far heavier. | §2.1, §3, §4.2, §8.2, §8.3, §8.4, §8.5, §9, §12.14 |

Notes: **macOS risk downgraded** — maintainer confirms snow-shot ships OCR on
macOS, so the §8.5 hard-gate-at-completion stance stands (no earlier spike
needed). **D3 cache model** kept exact-key for now (maintainer chose option A),
flagged to revisit with SP6. **D16 source resolved (revised 2026-06-25)** —
primary download is RapidOCR's official ModelScope URL (v3.9.0, `_mobile.onnx`
names, byte-identical to the table's SHA256s), with an optional GitHub Release
mirror fallback, layered under `actions/cache` keyed on the model SHA256s
(warm = offline, cold = one fetch). This **replaces** the earlier "GitHub
Release asset is the download fallback" resolution, which assumed a pre-existing
`xuhaojun/rollshot` `ocr-models-v3.1.0` release that does not exist — a hardcoded
always-live primary was needed so cold builds don't hard-fail. Pure
local-dir-only remains rejected as not robust on stock GitHub-hosted runners.

**snow-shot cross-check (2026-06-24).** Reviewed how `snow-shot` handles OCR in
CI. Findings: (a) snow-shot does **not** bundle models — it downloads a
`rapid_ocr` plugin zip at **runtime** from its own server
(`snowshot.top/plugins/…`), and its CI never runs OCR, so CI never touches
models; (b) snow-shot's CI **does** vendor a static ONNX Runtime lib from
`supertone-inc/onnxruntime-build` and static-links it. Decisions: **model
strategy stays bundle** (D16) — our CI *runs* real OCR integration tests, so
runtime-download would not simplify our CI (it would still need models at test
time) and would break the §1 offline / no-disclosure stance and the
agent-always-ready need; **ORT-lib strategy adopts snow-shot's static-vendor
recipe** (D4, §3.3).

**Model location clarified (2026-06-24).** Because models are `include_bytes!`
into the binary, **no model file exists on disk at runtime** — `etcetera` is
**not** used at runtime. The build-time model cache uses `etcetera` `cache_dir()`
(`choose_base_strategy().cache_dir().join("rollshot/ocr-models")`), mirroring the
existing `rollshot_config_dir()` convention but via `cache_dir()` since the
downloads are regenerable; overridable by `$ROLLSHOT_OCR_MODELS_DIR`. CI pins
that env var to a fixed workspace path so `actions/cache` is deterministic across
the OS matrix. `etcetera` is added to `rollshot-ocr` `[build-dependencies]` (§3).

**Critical gaps closed:** D6 (silent misplaced redactions), D9 (silent privacy
leak in logs), D13 (tall-capture OOM/stall) had no test and no error handling
before this pass.
