# OCR Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: SP6 (Preset Workbench) scope
- Started: 2026-06-24
- Last updated: 2026-06-24

## Decision

Which OCR backend should SP6 wire into `rollshot-vision::RealAutomationHost`
(via the existing prepare-outside-QuickJS / cached-lookup-inside pattern), and
does it satisfy the bounded-query, host-callback < 1 ms, cross-platform,
`forbid(unsafe_code)` isolation, and screenshot-text-accuracy constraints?

Primary candidate (chosen by the user, referencing `learn-projects/snow-shot`):
`paddle-ocr-rs` 0.6.1 (RapidOCR det/cls/rec ONNX pipeline) + `ort` 2.0.0-rc.10
(ONNX Runtime) + RapidOCR PP-OCRv4 ONNX models — the exact stack snow-shot ships
in a comparable screenshot product.

## Environment

- Repo: rollshot, spike crate `spikes/ocr-feasibility` (standalone, empty
  `[workspace]`, `unsafe_code = "allow"`, edition 2024, rust-version 1.94).
- Authoring/runtime OS: Linux (this environment). macOS: **UNTESTED** (no
  hardware here) — must be verified in product integration.
- Workspace MSRV: 1.94. Workspace `unsafe_code = "forbid"`; any `unsafe` backend
  needs an isolation crate with `unsafe_code = "allow"` (cf.
  `rollshot-macos-oneshot`, `rollshot-automation-rquickjs`).
- Versions (must be pinned exactly — see Risk 1): `paddle-ocr-rs = "=0.6.1"`,
  `ort = "=2.0.0-rc.10"`, `ndarray = "=0.16.1"`, `num_cpus = "1.17.0"`,
  `image = "0.25"`.
- Models (SHA256-verified against RapidOCR `default_models.yaml` @ v3.1.0):
  - det `ch_PP-OCRv4_det_infer.onnx` 4.5 MB — `d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9`
  - cls `ch_ppocr_mobile_v2.0_cls_infer.onnx` 571.8 KB — `e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c`
  - rec `ch_PP-OCRv4_rec_infer.onnx` 10.4 MB — `48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b`
  - Source: `https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.1.0/onnx/PP-OCRv4/{det,cls,rec}/...`
  - `init_models` reads the char dict from the rec ONNX metadata; no external
    `ppocr_keys_v1.txt` is needed for the simple `init_models` path.
- Fixtures: `paddle-ocr-rs` 0.6.1 shipped `docs/test_images/test_{1..4}.png`
  (not committed; gitignored).

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| 1 cross-platform + unsafe isolation (Linux build at MSRV 1.94, unsafe confined) | hard | compile + runtime | PASS | `cargo build -release` clean; session init ~89-96 ms; single `ndarray 0.16.1`; unsafe only in `ort`/`paddle_ocr_rs` inside the `allow` crate |
| 2 screenshot-text accuracy + latency | hard | runtime | PASS | 19 blocks across 4 fixtures; English test_3: punctuation/currency/email at 0.97+ conf; warm 45-399 ms/image |
| 3 host-callback < 1 ms (cached lookup + truncate) | hard | automated | PASS | 0.0036 ms/call (200-entry cache, Full+Rect, limit 100) — ~280x under budget |
| 4 footprint / distribution | soft | compile | MITIGATED | 15.5 MB ONNX models + ONNX Runtime shared lib; ship or first-run download |
| 5 license + maintenance | soft | compile | PASS | paddle-ocr-rs MIT; RapidOCR models Apache-2.0; ort MIT/Apache-2.0; active (snow-shot ships it) |
| 6 version fragility (ort rc float → two ndarray versions) | soft | compile | MITIGATED | must pin `ort = "=2.0.0-rc.10"` + `paddle-ocr-rs = "=0.6.1"` + `ndarray = "=0.16.1"` exactly |

## Observations

### Stage 1 — compile/link + isolation gate (PASS)

First build let `ort = "2.0.0-rc.10"` float to `2.0.0-rc.12`, pulling a newer
`ndarray` and producing two `ndarray` versions in the graph → `Tensor::from_array`
trait-bound errors in `paddle-ocr-rs` (`crnn_net.rs`, `db_net.rs`). Pinning
exactly to snow-shot's lockfile set (`ort = "=2.0.0-rc.10"`, `paddle-ocr-rs = "=0.6.1"`,
`ndarray = "=0.16.1"`) resolved a single `ndarray 0.16.1` and built clean.

```
cargo build --release → Finished in 24.16s (release)
cargo run --release  → stage1: session init 88.9-96.1 ms (threads=8)
```

Isolation: the spike crate is `unsafe_code = "allow"`; all `unsafe` is inside
`ort` (ONNX Runtime FFI) and `paddle_ocr-rs`. A production `forbid(unsafe_code)`
vision crate can depend on a safe wrapper exposed from an isolation crate shaped
like this one (the `rollshot-macos-oneshot` pattern already proves this in the
workspace).

### Stage 2 — screenshot-text accuracy + latency (PASS)

```
stage2: test_1.png -> blocks=2  cold=149.4ms warm=82.6ms  valid_shape=true
stage2: test_2.png -> blocks=1  cold=110.3ms warm=45.1ms  valid_shape=true
stage2: test_3.png -> blocks=12 cold=468.8ms warm=398.5ms valid_shape=true
stage2: test_4.png -> blocks=4  cold=299.5ms warm=230.1ms valid_shape=true
stage2: total_blocks=19 any_valid=true
```

`valid_shape=true` = every mapped `OcrMatch` has finite bounds, w>0, h>0,
confidence in [0,1]. English fixture test_3 sample (high confidence on
punctuation, currency, and an email-like string — directly representative of
Smart Redaction targets):

```
[59,32 463x46] conf=0.983 "The (quick) [brown] {fox} jumps!"
[57,61 505x49] conf=0.985 "Over the $43,456.78 <lazy> #90 dog"
[57,95 473x43] conf=0.985 "& duck/goose, as 12.5% of E-mail"
[57,126 518x47] conf=0.982 "from aspammer@website.com is spam."
```

Latency is for the **prepare** step (runs outside QuickJS); the agent waits for
the tool result, so 45-400 ms/image is acceptable. Tall/1080p captures will be
higher but remain in the seconds range — fine for an inspection tool, and the
bounded query supports a sub-`Region` to limit cost.

### Stage 3 — host callback < 1 ms (PASS)

```
stage3: cached callback 3610 ns/call (200-entry cache, Full + Rect, limit 100)
        -> 0.0036 ms/call
```

Mirrors `RealAutomationHost`'s prepare/cached-lookup pattern: OCR runs in
`prepare_ocr` (outside QuickJS), the QuickJS callback only does region-filter +
truncate on the cached `Vec<OcrMatch>`. ~0.004 ms/call vs the 1 ms budget —
~280x headroom. This matches the existing `region_features`/`template_match`
callback precedent.

## Final Recommendation

- **Go.** Wire `paddle-ocr-rs` + `ort` + RapidOCR PP-OCRv4 ONNX into
  `rollshot-vision::RealAutomationHost` as the OCR backend for SP6.
- Supporting evidence: all three hard gates PASS on Linux (build/isolation,
  accuracy/latency, callback < 1 ms). snow-shot ships the same stack in a
  comparable screenshot product.
- Rejected alternatives:
  - `ocrs` + `ort` (Tesseract LSTM ONNX port): viable but less proven on
    screenshot/UI text than PaddleOCR; snow-shot's PaddleOCR choice is stronger
    evidence for this use case.
  - System Tesseract (C bindings): heavier system dependency, larger footprint,
    no accuracy advantage over PaddleOCR-on-ONNX for screenshots.
  - Platform-native (macOS Vision + Linux OCR): two divergent backends, doubles
    work, breaks single-backend simplicity; defer unless macOS accuracy demands.
- Fallback triggers:
  - macOS build/runtime fails for `ort` 2.0.0-rc.10 or model I/O mismatch →
    spike macOS specifically; consider `ort/load-dynamic` or a newer ort release.
  - English UI-text accuracy insufficient on real Rollshot captures → tune
    `boxScoreThresh`/`unClipRatio`, enable angle rollback (snow-shot uses
    `detect_angle_rollback` with 0.9 threshold), or upscale 1.5x for small text
    (snow-shot's approach).
- Remaining risks (carry into product implementation):
  - **macOS UNTESTED** here — must verify `ort`/paddle-ocr-rs build + the model
    I/O shapes on macOS at MSRV 1.94.
  - **Version pinning is load-bearing.** `ort`, `paddle-ocr-rs`, and `ndarray`
    must be exact-pinned in the isolation crate; floating `ort` re-introduces
    the two-ndarray trait-bound breakage. Record this in the crate's Cargo.toml.
  - **Model distribution.** 15.5 MB of ONNX must ship with the app or download
    on first use (with SHA256 verification + a privacy disclosure if network
    access is involved). Decide: bundle vs first-run fetch.
  - **Isolation crate.** Add a new `rollshot-ocr` (or `rollshot-vision-ocr`)
    crate with `unsafe_code = "allow"`; `rollshot-vision` (`forbid(unsafe_code)`)
    depends on its safe API. Mirror `rollshot-macos-oneshot` structure.
  - **Bounded query mapping.** Map paddle `TextBlock.box_points` (4-point quad)
    to an axis-aligned `ImageRect` for `OcrMatch.bounds`; use `text_score` as
    `confidence`. Sub-`Region` queries should crop before OCR to bound cost.
  - **Angle handling** for rotated screenshots: use `detect_angle_rollback`
    (snow-shot default) rather than the spike's `detect(..., do_angle=false)`.
- Product handoff:
  1. Create `rollshot-ocr` (isolation crate, `unsafe_code = "allow"`), exact-pin
     `ort`/`paddle-ocr-rs`/`ndarray`, expose `prepare_ocr(&VisualIndex, &OcrQuery)
     -> Result<Vec<OcrMatch>, CapabilityError>` (safe API).
  2. In `rollshot-vision::RealAutomationHost`, add `prepare_ocr` + a cached
     `ocr` callback (mirror the existing `prepare_region_features`/`region_features`
     pair). Replace the `capability_unavailable` stub.
  3. Ship/verify the three PP-OCRv4 ONNX models (SHA256-verified; bundle vs
     first-run download decision).
  4. Validate on real Rollshot captures (English UI text, tall stitches) and on
     macOS before SP6 ships.
  5. `layout` (`inspectLayout`) remains stubbed — separate work, not blocked by
     this spike.
