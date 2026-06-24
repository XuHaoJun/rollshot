# OCR Backend — Completion Handoff

**Date:** 2026-06-25
**Spec:** `docs/superpowers/specs/2026-06-24-ocr-backend-design.md`
**Parent:** `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`

## Delivered Crate

`crates/rollshot-ocr` — unsafe-isolation crate wrapping RapidOCR (`paddle-ocr-rs`) + ONNX Runtime (`ort`). Public API is safe; returns primitives only (no rollshot deps). Excluded from `default-members`; built via `-p rollshot-ocr` or the dedicated OCR CI lane.

### Exact Pins (load-bearing)

| Dependency | Version | Reason |
|---|---|---|
| `paddle-ocr-rs` | `=0.6.1` | RapidOCR/ONNX Runtime wrapper |
| `ort` | `=2.0.0-rc.10` | Matches snow-shot's proven `ort`+ORT pairing; `rc.12` uses `ORT_LIB_PATH` instead of `ORT_LIB_LOCATION` |
| `ndarray` | `=0.16.1` | Floating `ndarray` breaks `Tensor::from_array` in `paddle-ocr-rs` |

## Public API

```rust
// Core types
pub struct OcrDetection { pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub text: String, pub confidence: f32 }
pub struct OcrRegionQuery { pub padding: u32, pub max_side_len: u32, pub min_scale: f32, pub box_score_thresh: f32, pub box_thresh: f32, pub unclip_ratio: f32, pub do_angle: bool }
pub struct OcrEngine { .. }

// Errors
pub enum OcrError { SessionInit, Detect, InvalidImage, ModelHashMismatch }

// Usage
let engine = OcrEngine::new()?;                                         // init once, caches session
let detections = engine.detect(&rgb_image, &OcrRegionQuery::default())?; // returns OcrDetection in input-native coords

// Coordinate convention: detect upscales small input by min_scale, runs OCR,
// then divides coordinates back. OcrDetection is always in the INPUT image's
// native pixel space.
```

## Bundled Models

Three PP-OCRv4 Chinese ONNX files compiled into the binary via `include_bytes!`:

| Model | SHA256 | Source |
|---|---|---|
| `ch_PP-OCRv4_det_infer.onnx` | `d2a7720d...` | RapidOCR ModelScope (primary) |
| `ch_ppocr_mobile_v2.0_cls_infer.onnx` | `e47acedf...` | RapidOCR ModelScope (primary) |
| `ch_PP-OCRv4_rec_infer.onnx` | `48fc40f2...` | RapidOCR ModelScope (primary) |

`build.rs` downloads from ModelScope as primary source. An optional GitHub Release mirror is configured as fallback. SHA256 verification runs at build time (compile failure on mismatch) and at runtime once per process (`verify_bundled_hashes_once`).

## Host Wiring

`rollshot-vision::RealAutomationHost` gained `prepare_ocr` + cached `ocr` callback, reusing the existing prepare-then-cached-callback pattern. The `ocr` capability is behind the off-by-default `ocr` Cargo feature on `rollshot-vision`.

- `prepare_ocr` maps the region query to full-image bounds, converts RGBA→RGB in one allocation, and caches the prepared result.
- The `ocr` callback performs the cached lookup/truncation.
- Errors map to `CapabilityError` codes: `ocr_session_init`, `ocr_detect`, `vision_index_unavailable`, `invalid_query`, `capability_unavailable`.

## Integration Tests

`crates/rollshot-vision/tests/ocr_integration.rs` — 5 Smart-Redaction e2e scenarios running through `QuickJsExecutor` + prepared `RealAutomationHost`:

1. Email detection on fixture with large black text on white
2. SSN-like pattern detection
3. Key-value OCR detection
4. Blank image returns no matches
5. Bounded-region query correctness
6. No OCR text/pixels in tracing output (privacy)

## CI

- **Default CI** (`ci.yml`): `--exclude rollshot-ocr` on all clippy/test commands (including action-guide lanes). OCR never builds in default CI.
- **OCR CI** (`ci-ocr.yml`): provisions static ONNX Runtime via `scripts/ci/provision-onnxruntime.sh`, runs on `ubuntu-24.04` and `macos-14`, caches model downloads, runs `-p rollshot-ocr` and `-p rollshot-vision --features ocr`.
- **ORT version:** supertone `1.22.2` static libs (the version snow-shot ships with `ort 2.0.0-rc.10`).

## OCR-Lane Wall-Clock (D14)

Measured on CI runners. The OCR lane adds ~2-3 min to CI (ORT provisioning + model download + build + test). The actual OCR test execution is <10s.

## Known Limitations

1. **Angle handling:** `do_angle=false` is the default. Angle detection is not exercised in tests.
2. **`layout` capability:** remains `capability_unavailable`. This plan only replaced `ocr`.
3. **`ch`-set only:** bundled models are PP-OCRv4 Chinese. Other language sets need a separate model migration.
4. **ORT-lib provisioning:** requires supertone static libs downloaded by CI script. Local dev needs `ORT_LIB_LOCATION` set to the extracted lib path.
5. **Windows:** not supported. CI runs Linux + macOS only.

## How SP6 Consumes OCR

`rollshot-vision` exposes OCR through `AutomationHost::ocr`. The `ocr` feature is off by default — enable it with `--features rollshot-vision/ocr`. SP6 (Smart Redaction agent) calls `host.ocr(query)` which delegates to the cached `OcrEngine::detect`. No direct `rollshot-ocr` dependency needed outside `rollshot-vision`.

## Migration Notes

- **`ort` upgrade:** changing `ort` version may change `ORT_LIB_LOCATION` env var name (rc.10) vs `ORT_LIB_PATH` (rc.12+). Update the CI provision script accordingly.
- **`paddle-ocr-rs` upgrade:** check `ndarray` compatibility; floating `ndarray` breaks `Tensor::from_array`.
- **Static ORT product build:** SP6/product packaging should switch to a self-contained static-ORT build. The current CI downloads supertone static libs; a product artifact should bundle them.
- **Model upgrade:** changing PP-OCRv4 model versions requires updating SHA256 constants, `build.rs` URLs, and `include_bytes!` paths.
