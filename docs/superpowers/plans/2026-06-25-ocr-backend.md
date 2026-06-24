# OCR Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `ocr` `capability_unavailable` stub in `rollshot-vision::RealAutomationHost` with a real RapidOCR/ONNX backend, isolated in a new `rollshot-ocr` crate, behind an off-by-default `ocr` feature, verified by real-OCR Smart-Redaction integration tests.

**Architecture:** A new `unsafe_code = "allow"` isolation crate `rollshot-ocr` wraps `paddle-ocr-rs` + `ort` (ONNX Runtime FFI) behind a safe API (`OcrEngine::new` / `OcrEngine::detect`) returning primitives (`OcrDetection`). It owns the small-text upscale **and its coordinate inversion** internally. `rollshot-vision` depends on it **optionally** (`ocr` feature), wires a lazy `prepare_ocr` + cached `ocr` callback pair mirroring the existing `region_features` precedent, and adds only the crop offset to produce full-image-native `OcrMatch.bounds`. The three PP-OCRv4 ONNX models are **not committed to git**: a `build.rs` provisions them into `OUT_DIR` (local cache dir first, GitHub Release-asset download fallback, SHA256-verified) and `lib.rs` `include_bytes!`s them, so the runtime stays offline.

**Tech Stack:** Rust 2021, MSRV 1.94; `paddle-ocr-rs =0.6.1`, `ort =2.0.0-rc.10` (default-features off — lib provided), `ndarray =0.16.1`, `num_cpus`, `image 0.25`, `sha2`, `thiserror`, `tracing`; build-deps `etcetera`, `sha2`, `ureq`; dev-deps `ab_glyph` (test text rendering); CI helper shell script for static ONNX Runtime provisioning. Source spec: `docs/superpowers/specs/2026-06-24-ocr-backend-design.md` (eng-review decisions D1–D17).

## Global Constraints

- **MSRV 1.94**; workspace `edition = "2021"`. `rollshot-ocr` is the **only** crate that sets `unsafe_code = "allow"`; the rest of the workspace stays `unsafe_code = "forbid"` (do **not** add `[lints] workspace = true` to `rollshot-ocr`).
- **Exact version pins are load-bearing** (spec §3.2): `paddle-ocr-rs = "=0.6.1"`, `ort = "=2.0.0-rc.10"`, `ndarray = "=0.16.1"`. Floating `ort` pulls a second `ndarray` and breaks `Tensor::from_array`. Record this in a comment in `rollshot-ocr/Cargo.toml`.
- **No `rollshot-*` dependency in `rollshot-ocr`** — it returns primitives only.
- **Models are NOT committed to git** (spec §5/D16). `crates/rollshot-ocr/models/` is git-ignored; `build.rs` provisions into `OUT_DIR`.
- **No OCR text, query contents, or image pixels in `tracing`** (spec §6/D9). Events on target `rollshot::vision::ocr` carry only `duration_ms`, `result_count`, and (on error) a static `code`.
- **`ocr` feature is OFF by default**; `rollshot-ocr` is excluded from workspace `default-members` (spec §13/D17). With the feature off, `RealAutomationHost::ocr` returns `Failed { code: "capability_unavailable" }`.
- **Detection defaults** (spec §4.4), encoded in `OcrRegionQuery::default()`: `padding=50`, `max_side_len=0` (0 ⇒ paddle uses the image's own longest side — no downscale, the D1 fix), `box_score_thresh=0.5`, `box_thresh=0.3`, `unclip_ratio=1.6`, `do_angle=false`, `min_scale=1.5`.
- **ONNX Runtime lib provisioning is the known-uncertain part (spec §3.3/D4).** `ort` is pinned `default-features = false`, so the native lib must be provided (`ORT_LIB_LOCATION` to a vendored static build from `supertone-inc/onnxruntime-build`). Local `ort-sys 2.0.0-rc.10` declares ONNX Runtime `1.22.0`; Snow Shot validates the same `ort = 2.0.0-rc.10` family with supertone `1.22.1`, and supertone publishes `v1.22.1`/`v1.22.2` static releases. Use `1.22.1` as the provisional CI script default, verify it in Task 1 Step 3, and do **not** silently advance to `1.22.2` unless both Linux and macOS OCR CI pass.
- **OCR memory ceiling:** internal scaling must never turn an OCR region into an unbounded working image. `OcrEngine` clamps effective scale so `scaled_width * scaled_height <= MAX_UPSCALED_PIXELS` (private constant, default `16_000_000`) and unit-tests the clamp; host-side `MAX_OCR_AREA` still rejects oversized prepared regions before OCR.

## File Structure

- Create: `crates/rollshot-ocr/Cargo.toml` — isolation crate manifest (pins, build-deps, `unsafe_code = "allow"`).
- Create: `crates/rollshot-ocr/build.rs` — model provisioning (local dir → Release download → SHA256 verify → `OUT_DIR`).
- Create: `crates/rollshot-ocr/src/lib.rs` — `OcrEngine`, `OcrDetection`, `OcrRegionQuery`, `OcrError`, model hashes, unit tests.
- Create: `crates/rollshot-ocr/.gitignore` — ignore `models/`.
- Modify: `Cargo.toml` (workspace root) — add `rollshot-ocr` to `members`; add `default-members` excluding it.
- Modify: `crates/rollshot-vision/Cargo.toml` — `ocr` feature + optional `rollshot-ocr` dep + `ab_glyph` dev-dep.
- Modify: `crates/rollshot-vision/src/rect.rs` — add `pub const MAX_OCR_AREA`.
- Modify: `crates/rollshot-vision/src/host.rs` — feature-gated OCR fields, `prepare_ocr`, real `ocr` body, mapping, unit tests.
- Modify: `crates/rollshot-vision/src/lib.rs` — cfg-aware contract test; re-export nothing new.
- Create: `crates/rollshot-vision/tests/ocr_integration.rs` — gated real-OCR Smart-Redaction e2e + privacy test.
- Modify: `.github/workflows/ci.yml` — default lane excludes `rollshot-ocr`.
- Create: `.github/workflows/ci-ocr.yml` — path-filtered + `main`-push OCR lane (ubuntu + macos).
- Create: `scripts/ci/provision-onnxruntime.sh` — version-pinned static ONNX Runtime fetch for OCR CI.
- Modify: `AGENTS.md`, `README.md` — project-map / workspace updates.
- Create: `docs/superpowers/handoffs/2026-06-25-ocr-backend.md` — completion handoff.
- Modify: `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md` — §12 delivery status (parent spec).

---

## Task 1: `rollshot-ocr` isolation crate

**Files:**
- Create: `crates/rollshot-ocr/Cargo.toml`
- Create: `crates/rollshot-ocr/build.rs`
- Create: `crates/rollshot-ocr/src/lib.rs`
- Create: `crates/rollshot-ocr/.gitignore`
- Modify: `Cargo.toml` (workspace root) — `members` + `default-members`

**Interfaces:**
- Produces (consumed by Task 2):
  - `rollshot_ocr::OcrEngine` with `fn new() -> Result<OcrEngine, OcrError>` and `fn detect(&mut self, img: &image::RgbImage, query: &OcrRegionQuery) -> Result<Vec<OcrDetection>, OcrError>`.
  - `rollshot_ocr::OcrDetection { x: f32, y: f32, w: f32, h: f32, text: String, confidence: f32 }` (coords in the **input image's native space**).
  - `rollshot_ocr::OcrRegionQuery { padding: u32, max_side_len: u32, min_scale: f32, box_score_thresh: f32, box_thresh: f32, unclip_ratio: f32, do_angle: bool }` with `Default` per Global Constraints.
  - `rollshot_ocr::OcrError` (enum; `Debug + std::error::Error`).
  - `OcrEngine` is `Send` (required by `AutomationHost: Send`) and implements `Debug` opaquely.

- [ ] **Step 1: Add the crate to the workspace (root `Cargo.toml`)**

Add `"crates/rollshot-ocr"` to `members`, and add a `default-members` list containing every current member **except** `rollshot-ocr` (spec §13/D17 — keeps the heavy crate out of bare `cargo build`/`test` and the default CI lane).

```toml
[workspace]
members = [
    "crates/rollshot-core",
    "crates/rollshot-image-document",
    "crates/rollshot-action",
    "crates/rollshot-linux-input",
    "crates/rollshot-macos-input",
    "crates/rollshot-capture",
    "crates/rollshot-dev",
    "crates/rollshot-app",
    "crates/rollshot-iced-overlay",
    "crates/rollshot-overlay-core",
    "crates/rollshot-macos-oneshot",
    "crates/rollshot-linux-desktop",
    "crates/rollshot-edit-proposal",
    "crates/rollshot-automation",
    "crates/rollshot-automation-rquickjs",
    "crates/rollshot-vision",
    "crates/rollshot-agent",
    "crates/rollshot-preset",
    "crates/rollshot-ocr",
]
# rollshot-ocr is excluded from default-members (eng-review D17): the heavy
# ort + 15.5 MB-model toolchain is built only via `-p rollshot-ocr` / the
# `ocr` feature / the dedicated OCR CI lane, not bare `cargo build`/`test`.
default-members = [
    "crates/rollshot-core",
    "crates/rollshot-image-document",
    "crates/rollshot-action",
    "crates/rollshot-linux-input",
    "crates/rollshot-macos-input",
    "crates/rollshot-capture",
    "crates/rollshot-dev",
    "crates/rollshot-app",
    "crates/rollshot-iced-overlay",
    "crates/rollshot-overlay-core",
    "crates/rollshot-macos-oneshot",
    "crates/rollshot-linux-desktop",
    "crates/rollshot-edit-proposal",
    "crates/rollshot-automation",
    "crates/rollshot-automation-rquickjs",
    "crates/rollshot-vision",
    "crates/rollshot-agent",
    "crates/rollshot-preset",
]
resolver = "2"
```

- [ ] **Step 2: Create `crates/rollshot-ocr/.gitignore` and `crates/rollshot-ocr/Cargo.toml`**

`.gitignore`:

```gitignore
# Models are provisioned by build.rs (eng-review D16); never commit them.
models/
```

`Cargo.toml`:

```toml
[package]
name = "rollshot-ocr"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
publish = false

[lints.rust]
# Isolation crate for ort (ONNX Runtime FFI) + paddle-ocr-rs unsafe.
# The rest of the workspace stays unsafe_code = "forbid".
unsafe_code = "allow"

[dependencies]
# EXACT pins are load-bearing (spec §3.2): floating `ort` pulls a second
# `ndarray` and breaks `Tensor::from_array` in paddle-ocr-rs.
paddle-ocr-rs = "=0.6.1"
ort = { version = "=2.0.0-rc.10", default-features = false }
ndarray = "=0.16.1"
num_cpus = "1.17.0"
image = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
sha2 = "0.10"

[build-dependencies]
etcetera = { workspace = true }
sha2 = "0.10"
ureq = "2"
```

- [ ] **Step 3: Provision the ONNX Runtime lib and the models locally, then verify the crate links**

This is the environment-setup step (spec §3.3/D4 — the known-uncertain part). Do it once locally.

1. **Models** — place the three `.onnx` files (the maintainer's backup) in the cache dir build.rs reads, or point an env var at them:

```bash
rtk mkdir -p ~/.cache/rollshot/ocr-models
# copy ch_PP-OCRv4_det_infer.onnx, ch_ppocr_mobile_v2.0_cls_infer.onnx,
# ch_PP-OCRv4_rec_infer.onnx into ~/.cache/rollshot/ocr-models/
# (or: export ROLLSHOT_OCR_MODELS_DIR=/path/to/backup)
```

2. **ONNX Runtime static lib** — download the version-matched static build from `supertone-inc/onnxruntime-build` and point `ort` at it:

```bash
# Verify which ONNX Runtime version ort 2.0.0-rc.10 expects, pick the matching
# supertone tag, then:
export ORT_LIB_LOCATION=/path/to/onnxruntime/lib   # contains libonnxruntime*.a
```

Expected: `rtk cargo build -p rollshot-ocr` reaches the `include_bytes!` stage (it will fail later only on the not-yet-written `lib.rs`, which is fine — this step proves models provision and `ort` links).

- [ ] **Step 4: Write `build.rs` (model provisioning → `OUT_DIR`)**

```rust
//! Provisions the three PP-OCRv4 ONNX models into OUT_DIR (eng-review D16).
//! Resolution: $ROLLSHOT_OCR_MODELS_DIR, else the etcetera cache dir; missing
//! files are downloaded from the rollshot GitHub Release asset. Every model's
//! SHA256 is verified against the recorded hash (build fails on mismatch).
use std::{env, fs, path::PathBuf};

use sha2::{Digest, Sha256};

const MODELS: [(&str, &str); 3] = [
    (
        "ch_PP-OCRv4_det_infer.onnx",
        "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
    ),
    (
        "ch_ppocr_mobile_v2.0_cls_infer.onnx",
        "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
    ),
    (
        "ch_PP-OCRv4_rec_infer.onnx",
        "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
    ),
];

// Maintainer uploads the three files to this release tag once (eng-review D16).
const RELEASE_BASE: &str =
    "https://github.com/xuhaojun/rollshot/releases/download/ocr-models-v3.1.0";

fn cache_dir() -> PathBuf {
    if let Ok(dir) = env::var("ROLLSHOT_OCR_MODELS_DIR") {
        return PathBuf::from(dir);
    }
    use etcetera::base_strategy::{choose_base_strategy, BaseStrategy};
    choose_base_strategy()
        .expect("no platform cache dir")
        .cache_dir()
        .join("rollshot/ocr-models")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    println!("cargo:rerun-if-env-changed=ROLLSHOT_OCR_MODELS_DIR");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let src = cache_dir();
    fs::create_dir_all(&src).ok();

    for (name, want) in MODELS {
        let local = src.join(name);
        let bytes = if local.is_file() {
            fs::read(&local).unwrap_or_else(|e| panic!("read {}: {e}", local.display()))
        } else {
            let url = format!("{RELEASE_BASE}/{name}");
            let mut buf = Vec::new();
            ureq::get(&url)
                .call()
                .unwrap_or_else(|e| panic!("download {url}: {e}"))
                .into_reader()
                .read_to_end(&mut buf)
                .unwrap_or_else(|e| panic!("read body {url}: {e}"));
            fs::write(&local, &buf).ok(); // populate the cache for next time
            buf
        };
        let got = sha256_hex(&bytes);
        assert_eq!(
            got, want,
            "SHA256 mismatch for {name}: expected {want}, got {got}"
        );
        fs::write(out.join(name), &bytes).expect("write model to OUT_DIR");
    }
}
```

(`use std::io::Read;` is required for `read_to_end`; add it to the `use` block.)

- [ ] **Step 5: Write `src/lib.rs` — types, model hashes, `OcrEngine::new`, `detect`**

```rust
//! # rollshot-ocr
//!
//! Unsafe-isolation crate wrapping RapidOCR (`paddle-ocr-rs`) + ONNX Runtime
//! (`ort`). The public API is safe and returns primitives only (no rollshot
//! deps), so `rollshot-vision` stays `forbid(unsafe_code)`.
//!
//! Coordinate convention (eng-review D6/D15): `detect` upscales small input by
//! `min_scale`, runs OCR, then divides coordinates back, so `OcrDetection` is
//! always in the INPUT image's native pixel space.

use std::sync::OnceLock;

use image::RgbImage;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use paddle_ocr_rs::{ocr_lite::OcrLite, ocr_result::Point};
use sha2::{Digest, Sha256};
use thiserror::Error;

const DET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ch_PP-OCRv4_det_infer.onnx"));
const CLS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ch_ppocr_mobile_v2.0_cls_infer.onnx"));
const REC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ch_PP-OCRv4_rec_infer.onnx"));

const DET_SHA256: &str = "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9";
const CLS_SHA256: &str = "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c";
const REC_SHA256: &str = "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b";
const MAX_UPSCALED_PIXELS: u64 = 16_000_000;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("ocr session init failed")]
    SessionInit,
    #[error("ocr detection failed")]
    Detect,
    #[error("invalid image")]
    InvalidImage,
    #[error("bundled model hash mismatch")]
    ModelHashMismatch,
}

/// Detection in the INPUT image's native pixel space (upscale already inverted).
#[derive(Debug, Clone, PartialEq)]
pub struct OcrDetection {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: String,
    pub confidence: f32,
}

/// Detector knobs. `Default` encodes the snow-shot-validated screenshot params
/// (spec §4.4). `max_side_len == 0` ⇒ paddle uses the image's own longest side
/// (no downscale — the tall-capture fix, eng-review D1).
#[derive(Debug, Clone, Copy)]
pub struct OcrRegionQuery {
    pub padding: u32,
    pub max_side_len: u32,
    pub min_scale: f32,
    pub box_score_thresh: f32,
    pub box_thresh: f32,
    pub unclip_ratio: f32,
    pub do_angle: bool,
}

impl Default for OcrRegionQuery {
    fn default() -> Self {
        Self {
            padding: 50,
            max_side_len: 0,
            min_scale: 1.5,
            box_score_thresh: 0.5,
            box_thresh: 0.3,
            unclip_ratio: 1.6,
            do_angle: false,
        }
    }
}

pub struct OcrEngine {
    ocr: OcrLite,
}

impl std::fmt::Debug for OcrEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OcrEngine { .. }")
    }
}

fn verify_bundled_hashes_once() -> Result<(), OcrError> {
    static OK: OnceLock<bool> = OnceLock::new();
    // include_bytes! constants can't change at runtime, so hash once per process
    // (eng-review D12); guards against binary corruption.
    let ok = *OK.get_or_init(|| {
        let check = |bytes: &[u8], want: &str| {
            let got: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
            got == want
        };
        check(DET, DET_SHA256) && check(CLS, CLS_SHA256) && check(REC, REC_SHA256)
    });
    if ok {
        Ok(())
    } else {
        Err(OcrError::ModelHashMismatch)
    }
}

fn build_session(builder: SessionBuilder) -> Result<SessionBuilder, ort::Error> {
    let threads = num_cpus::get_physical().max(1);
    builder
        .with_inter_threads(threads)?
        .with_intra_threads(threads)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
}

fn effective_scale(width: u32, height: u32, min_scale: f32) -> f32 {
    let requested = min_scale.max(1.0);
    let pixels = (width as u64).saturating_mul(height as u64).max(1);
    let cap_scale = ((MAX_UPSCALED_PIXELS as f64) / (pixels as f64)).sqrt() as f32;
    requested.min(cap_scale).max(0.01)
}

impl OcrEngine {
    pub fn new() -> Result<Self, OcrError> {
        verify_bundled_hashes_once()?;
        let mut ocr = OcrLite::new();
        // Match Snow Shot's validated RapidOCR/ORT shape: explicit session
        // thread counts and Level3 graph optimizations instead of relying on
        // default builder behavior.
        ocr.init_models_from_memory_custom(DET, CLS, REC, build_session)
            .map_err(|_| OcrError::SessionInit)?;
        Ok(Self { ocr })
    }

    /// Detect text in `img`. Returns bounds in `img`'s native pixel space.
    pub fn detect(
        &mut self,
        img: &RgbImage,
        query: &OcrRegionQuery,
    ) -> Result<Vec<OcrDetection>, OcrError> {
        if img.width() == 0 || img.height() == 0 {
            return Err(OcrError::InvalidImage);
        }
        let scale = effective_scale(img.width(), img.height(), query.min_scale);
        let result = if (scale - 1.0).abs() > f32::EPSILON {
            let nw = ((img.width() as f32) * scale).round().max(1.0) as u32;
            let nh = ((img.height() as f32) * scale).round().max(1.0) as u32;
            let work = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Lanczos3);
            self.ocr.detect(
                &work,
                query.padding,
                query.max_side_len,
                query.box_score_thresh,
                query.box_thresh,
                query.unclip_ratio,
                query.do_angle,
                false,
            )
        } else {
            self.ocr.detect(
                img,
                query.padding,
                query.max_side_len,
                query.box_score_thresh,
                query.box_thresh,
                query.unclip_ratio,
                query.do_angle,
                false,
            )
        }
        .map_err(|_| OcrError::Detect)?;

        let mut out = Vec::with_capacity(result.text_blocks.len());
        for block in &result.text_blocks {
            if block.box_points.is_empty() {
                continue;
            }
            let (x, y, w, h) = aabb(&block.box_points);
            // Invert the upscale → input-native coordinates (eng-review D6/D15).
            let (x, y, w, h) = (x / scale, y / scale, w / scale, h / scale);
            if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) {
                continue;
            }
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            out.push(OcrDetection {
                x,
                y,
                w,
                h,
                text: block.text.clone(),
                confidence: block.text_score,
            });
        }
        Ok(out)
    }
}

/// Axis-aligned bounding box of a paddle 4-point quad.
fn aabb(points: &[Point]) -> (f32, f32, f32, f32) {
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in points {
        let (px, py) = (p.x as f32, p.y as f32);
        min_x = min_x.min(px);
        min_y = min_y.min(py);
        max_x = max_x.max(px);
        max_y = max_y.max(py);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}
```

- [ ] **Step 6: Write the unit tests (append to `src/lib.rs`)**

These run real OCR, so they require the models + ORT lib from Step 3. A shared helper renders black text on white using the vendored DejaVu font via `ab_glyph` — but `rollshot-ocr` has no rollshot deps, so the test embeds its own font bytes from the `image-document` asset path through a `dev-dependencies` font crate. To keep `rollshot-ocr` rollshot-free, the test draws text with `ab_glyph` + a font file read at test time from the workspace asset.

Add to `Cargo.toml`:

```toml
[dev-dependencies]
ab_glyph = "0.2"
imageproc = { workspace = true }
```

Tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::FontRef;
    use image::{Rgb, RgbImage};
    use imageproc::drawing::{draw_text_mut, text_size};

    // Vendored deterministic font (workspace asset), read at test time.
    const FONT: &[u8] =
        include_bytes!("../../rollshot-image-document/assets/fonts/DejaVuSans.ttf");

    /// White image with black `text` at (x,y); returns the rendered text box.
    fn text_image(w: u32, h: u32, x: i32, y: i32, px: f32, text: &str) -> (RgbImage, (u32, u32, u32, u32)) {
        let font = FontRef::try_from_slice(FONT).unwrap();
        let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
        draw_text_mut(&mut img, Rgb([0, 0, 0]), x, y, px, &font, text);
        let (tw, th) = text_size(px, &font, text);
        (img, (x as u32, y as u32, tw, th))
    }

    #[test]
    fn engine_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<OcrEngine>();
    }

    #[test]
    fn default_query_matches_screenshot_params() {
        let q = OcrRegionQuery::default();
        assert_eq!(q.padding, 50);
        assert_eq!(q.max_side_len, 0);
        assert_eq!(q.min_scale, 1.5);
        assert_eq!(q.box_score_thresh, 0.5);
        assert_eq!(q.box_thresh, 0.3);
        assert_eq!(q.unclip_ratio, 1.6);
        assert!(!q.do_angle);
    }

    #[test]
    fn bundled_model_hashes_match() {
        verify_bundled_hashes_once().unwrap();
    }

    #[test]
    fn new_succeeds() {
        OcrEngine::new().unwrap();
    }

    #[test]
    fn detect_reads_text_with_valid_shape() {
        let (img, _) = text_image(640, 160, 20, 50, 48.0, "Hello OCR");
        let mut engine = OcrEngine::new().unwrap();
        let dets = engine.detect(&img, &OcrRegionQuery::default()).unwrap();
        assert!(!dets.is_empty(), "expected ≥1 detection");
        for d in &dets {
            assert!(d.w.is_finite() && d.h.is_finite() && d.w > 0.0 && d.h > 0.0);
            assert!((0.0..=1.0).contains(&d.confidence));
        }
    }

    #[test]
    fn detect_on_blank_returns_zero() {
        let img = RgbImage::from_pixel(320, 120, Rgb([255, 255, 255]));
        let mut engine = OcrEngine::new().unwrap();
        assert_eq!(engine.detect(&img, &OcrRegionQuery::default()).unwrap().len(), 0);
    }

    #[test]
    fn detect_rejects_zero_dim() {
        let img = RgbImage::new(0, 0);
        let mut engine = OcrEngine::new().unwrap();
        assert!(matches!(
            engine.detect(&img, &OcrRegionQuery::default()),
            Err(OcrError::InvalidImage)
        ));
    }

    #[test]
    fn effective_scale_caps_large_images() {
        assert_eq!(effective_scale(300, 90, 1.5), 1.5);
        let scale = effective_scale(8000, 4000, 1.5);
        assert!(scale > 0.0 && scale < 1.0);
        let pixels = (8000.0 * scale) * (4000.0 * scale);
        assert!(pixels <= MAX_UPSCALED_PIXELS as f32 + 1024.0);
    }

    #[test]
    fn upscale_inversion_keeps_native_coords() {
        // A small image: default min_scale=1.5 upscales internally; bounds must
        // come back in the SMALL image's coordinate space and overlap the text.
        let (img, (tx, ty, tw, th)) = text_image(300, 90, 12, 28, 32.0, "INVOICE");
        let mut engine = OcrEngine::new().unwrap();
        let dets = engine.detect(&img, &OcrRegionQuery::default()).unwrap();
        assert!(!dets.is_empty());
        let d = &dets[0];
        // Detected box is within the small (300x90) image, not 1.5x larger.
        assert!(d.x + d.w <= 300.0 + 4.0 && d.y + d.h <= 90.0 + 4.0);
        // ...and overlaps the rendered text box.
        let overlap = (d.x < (tx + tw) as f32) && ((d.x + d.w) > tx as f32)
            && (d.y < (ty + th) as f32) && ((d.y + d.h) > ty as f32);
        assert!(overlap, "detection {d:?} should overlap text box {:?}", (tx, ty, tw, th));
    }
}
```

- [ ] **Step 7: Run the unit tests**

Run: `rtk cargo test -p rollshot-ocr`
Expected: PASS (9 tests). If `new` fails with a link error, revisit Step 3 (ORT lib). If `detect_reads_text_with_valid_shape` finds 0 blocks, the font render is too small — raise `px`.

- [ ] **Step 8: fmt + clippy + commit**

Run: `rtk cargo fmt -p rollshot-ocr` then `rtk cargo clippy -p rollshot-ocr --all-targets -- -D warnings`
Expected: clean.

```bash
git add Cargo.toml crates/rollshot-ocr
git commit -m "feat(ocr): rollshot-ocr isolation crate (RapidOCR/ONNX, bundled models)"
```

---

## Task 2: `rollshot-vision` host wiring + `ocr` feature gate

**Files:**
- Modify: `crates/rollshot-vision/Cargo.toml` — `ocr` feature, optional `rollshot-ocr`, `ab_glyph` dev-dep
- Modify: `crates/rollshot-vision/src/rect.rs` — `pub const MAX_OCR_AREA`
- Modify: `crates/rollshot-vision/src/host.rs` — gated fields, `prepare_ocr`, real `ocr`, mapping, tests
- Modify: `crates/rollshot-vision/src/lib.rs` — cfg-aware contract test

**Interfaces:**
- Consumes (from Task 1): `rollshot_ocr::{OcrEngine, OcrDetection, OcrRegionQuery, OcrError}`.
- Produces (used by Task 3): `RealAutomationHost::prepare_ocr(&mut self, &VisualIndex, &OcrQuery) -> Result<(), CapabilityError>` (only under `feature = "ocr"`); the `AutomationHost::ocr` callback returning `OcrMatch`es in full-image-native coords; `MAX_OCR_AREA: u64`.

- [ ] **Step 1: Add the feature + optional dep (`crates/rollshot-vision/Cargo.toml`)**

```toml
[features]
ocr = ["dep:rollshot-ocr"]

[dependencies]
# ...existing...
rollshot-ocr = { path = "../rollshot-ocr", optional = true }

[dev-dependencies]
# ...existing...
ab_glyph = "0.2"
```

- [ ] **Step 2: Add the OCR area cap (`crates/rollshot-vision/src/rect.rs`)**

```rust
/// Cap on prepared-OCR region area (pixels). Larger than MAX_SEARCH_AREA because
/// OCR legitimately covers full screenshots; tall captures beyond this must use
/// a bounded `Rect` query (eng-review D13). ~ up to a 4K full screen.
pub const MAX_OCR_AREA: u64 = 16_000_000;
```

- [ ] **Step 3: Write the failing host unit tests (`crates/rollshot-vision/src/host.rs`, in `mod tests`)**

Add these `#[cfg(feature = "ocr")]` tests. They render text with the vendored font (helper added to the test module):

```rust
#[cfg(feature = "ocr")]
mod ocr_tests {
    use super::*;
    use ab_glyph::FontRef;
    use image::Rgba;
    use rollshot_automation::OcrQuery;

    const FONT: &[u8] =
        include_bytes!("../../rollshot-image-document/assets/fonts/DejaVuSans.ttf");

    fn text_scene(w: u32, h: u32, x: i32, y: i32, px: f32, text: &str) -> image::RgbaImage {
        use imageproc::drawing::draw_text_mut;
        let font = FontRef::try_from_slice(FONT).unwrap();
        let mut img = image::RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
        draw_text_mut(&mut img, Rgba([0, 0, 0, 255]), x, y, px, &font, text);
        img
    }

    #[test]
    fn unprepared_ocr_query_fails_explicitly() {
        let mut host = RealAutomationHost::new();
        let err = host
            .ocr(OcrQuery { region: rollshot_automation::Region::Full, limit: 1 })
            .unwrap_err();
        assert_eq!(err, CapabilityError::Failed { code: "vision_index_unavailable" });
    }

    #[test]
    fn ocr_rejects_zero_limit() {
        let mut host = RealAutomationHost::new();
        let err = host
            .ocr(OcrQuery { region: rollshot_automation::Region::Full, limit: 0 })
            .unwrap_err();
        assert_eq!(err, CapabilityError::InvalidInput { code: "invalid_query" });
    }

    #[test]
    fn prepare_then_ocr_returns_full_image_bounds() {
        let scene = text_scene(640, 160, 30, 60, 48.0, "Hello");
        let index = VisualIndex::build(scene).unwrap();
        let mut host = RealAutomationHost::new();
        let q = OcrQuery { region: rollshot_automation::Region::Full, limit: 10 };
        host.prepare_ocr(&index, &q).unwrap();
        let out = host.ocr(q).unwrap();
        assert!(!out.is_empty());
        // bounds are in full-image space and overlap the rendered text near (30,60).
        let m = &out[0];
        assert!(m.bounds.x < 640.0 && m.bounds.y < 160.0);
        assert!((m.bounds.x - 30.0).abs() < 80.0 && (m.bounds.y - 60.0).abs() < 60.0);
    }

    #[test]
    fn ocr_limit_over_prepared_is_limit_exceeded() {
        let scene = text_scene(640, 160, 30, 60, 48.0, "Hello");
        let index = VisualIndex::build(scene).unwrap();
        let mut host = RealAutomationHost::new();
        host.prepare_ocr(&index, &OcrQuery { region: rollshot_automation::Region::Full, limit: 1 })
            .unwrap();
        let err = host
            .ocr(OcrQuery { region: rollshot_automation::Region::Full, limit: 2 })
            .unwrap_err();
        assert_eq!(err, CapabilityError::LimitExceeded);
    }

    #[test]
    fn prepare_ocr_rejects_non_finite_region() {
        let scene = text_scene(64, 64, 4, 20, 24.0, "x");
        let index = VisualIndex::build(scene).unwrap();
        let mut host = RealAutomationHost::new();
        let bad = rollshot_automation::Region::Rect {
            bounds: ImageRect { x: f32::NAN, y: 0.0, width: 8.0, height: 8.0 },
        };
        let err = host.prepare_ocr(&index, &OcrQuery { region: bad, limit: 1 }).unwrap_err();
        assert_eq!(err, CapabilityError::InvalidInput { code: "non_finite_region" });
    }
}
```

- [ ] **Step 4: Run the new tests to confirm they fail**

Run: `rtk cargo test -p rollshot-vision --features ocr ocr_tests`
Expected: FAIL to compile — `prepare_ocr` and the OCR fields do not exist yet.

- [ ] **Step 5: Implement the host wiring (`crates/rollshot-vision/src/host.rs`)**

Add imports and the gated key/prepared types near the top:

```rust
use crate::rect::{region_to_pixel_rect, PixelRect, MAX_OCR_AREA};
#[cfg(feature = "ocr")]
use rollshot_automation::OcrQuery;
#[cfg(feature = "ocr")]
use rollshot_ocr::{OcrEngine, OcrRegionQuery};

#[cfg(feature = "ocr")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct OcrKey {
    rect: PixelRect,
}

#[cfg(feature = "ocr")]
#[derive(Debug, Clone)]
struct PreparedOcr {
    key: OcrKey,
    max_limit: u32,
    results: Vec<OcrMatch>,
}
```

Add gated fields to the struct (`#[derive(Debug, Default)]` still works — `OcrEngine` is `Debug`, `Option<OcrEngine>` defaults to `None`):

```rust
#[derive(Debug, Default)]
pub struct RealAutomationHost {
    prepared_template_matches: Vec<PreparedTemplateMatch>,
    prepared_region_features: Vec<PreparedRegionFeatures>,
    #[cfg(feature = "ocr")]
    prepared_ocr: Vec<PreparedOcr>,
    #[cfg(feature = "ocr")]
    ocr_engine: Option<OcrEngine>,
    image_dimensions: Option<(u32, u32)>,
}
```

Add `prepare_ocr` to the `impl RealAutomationHost` block:

```rust
/// Expensive preparation. Call before entering `QuickJsExecutor`.
/// Crops to the region, runs OCR (engine owns the upscale + its inversion),
/// then maps to full-image-native `OcrMatch.bounds` by adding the crop offset.
#[cfg(feature = "ocr")]
pub fn prepare_ocr(
    &mut self,
    index: &VisualIndex,
    query: &OcrQuery,
) -> Result<(), CapabilityError> {
    let started = Instant::now();
    let rect = region_to_pixel_rect(&query.region, index.width(), index.height(), MAX_OCR_AREA)?;

    // Crop RGBA → RGB for paddle without materializing an intermediate RGBA
    // crop. A full-screen OCR region is already large; do one RGB allocation.
    let mut rgb = image::RgbImage::new(rect.width, rect.height);
    for (x, y, dst) in rgb.enumerate_pixels_mut() {
        let src = index.image().get_pixel(rect.x + x, rect.y + y);
        *dst = image::Rgb([src[0], src[1], src[2]]);
    }

    if self.ocr_engine.is_none() {
        self.ocr_engine = Some(
            OcrEngine::new().map_err(|_| CapabilityError::Failed { code: "ocr_session_init" })?,
        );
    }
    let engine = self.ocr_engine.as_mut().expect("engine just set");
    let detections = engine
        .detect(&rgb, &OcrRegionQuery::default())
        .map_err(|_| CapabilityError::Failed { code: "ocr_detect" })?;

    let (ox, oy) = (rect.x as f32, rect.y as f32);
    let results: Vec<OcrMatch> = detections
        .into_iter()
        .filter_map(|d| {
            let bounds = ImageRect { x: d.x + ox, y: d.y + oy, width: d.w, height: d.h };
            if !bounds.is_finite() || d.w <= 0.0 || d.h <= 0.0 {
                return None;
            }
            Some(OcrMatch { bounds, text: d.text, confidence: d.confidence })
        })
        .collect();

    let key = OcrKey { rect };
    self.image_dimensions = Some((index.width(), index.height()));
    self.prepared_ocr.retain(|p| p.key != key);
    let result_count = results.len() as u64;
    self.prepared_ocr.push(PreparedOcr { key, max_limit: query.limit, results });
    tracing::debug!(
        target: "rollshot::vision::ocr",
        duration_ms = started.elapsed().as_millis() as u64,
        result_count,
        "ocr prepared"
    );
    Ok(())
}
```

Replace the stub `ocr` trait method body with a cfg-split:

```rust
fn ocr(&mut self, query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
    #[cfg(not(feature = "ocr"))]
    {
        let _ = query;
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }
    #[cfg(feature = "ocr")]
    {
        if query.limit == 0 {
            return Err(CapabilityError::InvalidInput { code: "invalid_query" });
        }
        let (w, h) = self
            .image_dimensions
            .ok_or(CapabilityError::Failed { code: "vision_index_unavailable" })?;
        let rect = region_to_pixel_rect(&query.region, w, h, MAX_OCR_AREA)?;
        let key = OcrKey { rect };
        let prepared = self
            .prepared_ocr
            .iter()
            .find(|p| p.key == key)
            .ok_or(CapabilityError::Failed { code: "vision_index_unavailable" })?;
        if query.limit > prepared.max_limit {
            return Err(CapabilityError::LimitExceeded);
        }
        Ok(prepared.results.iter().take(query.limit as usize).cloned().collect())
    }
}
```

Note: the signature uses `OcrQuery`, which is already imported in the existing `use rollshot_automation::{... OcrQuery ...}` at the top of `host.rs` — keep that import unconditional (the trait method needs it in both configs).

- [ ] **Step 6: Run the host tests (feature on)**

Run: `rtk cargo test -p rollshot-vision --features ocr`
Expected: PASS (existing template/region tests + the 5 new `ocr_tests`).

- [ ] **Step 7: Make the contract test cfg-aware (`crates/rollshot-vision/src/lib.rs`)**

In `all_unimplemented_capabilities_report_unavailable`, replace the `ocr` assertion with a cfg-split (spec §8.2/D17):

```rust
#[cfg(not(feature = "ocr"))]
assert_eq!(
    host.ocr(OcrQuery { region: Region::Full, limit: 1 }).unwrap_err(),
    rollshot_automation::CapabilityError::Failed { code: "capability_unavailable" }
);
#[cfg(feature = "ocr")]
assert_eq!(
    host.ocr(OcrQuery { region: Region::Full, limit: 1 }).unwrap_err(),
    rollshot_automation::CapabilityError::Failed { code: "vision_index_unavailable" }
);
```

(`layout` still asserts `capability_unavailable` unconditionally.)

- [ ] **Step 8: Run both configs + clippy**

Run (default, OCR off):
`rtk cargo test -p rollshot-vision`
Expected: PASS, `ocr` reports `capability_unavailable`.

Run (OCR on):
`rtk cargo test -p rollshot-vision --features ocr`
Expected: PASS.

Run: `rtk cargo clippy -p rollshot-vision --all-targets -- -D warnings` and `rtk cargo clippy -p rollshot-vision --all-targets --features ocr -- -D warnings`
Expected: both clean.

- [ ] **Step 9: Commit**

```bash
git add crates/rollshot-vision
git commit -m "feat(ocr): wire RealAutomationHost prepare_ocr/ocr behind off-by-default ocr feature"
```

---

## Task 3: Real-OCR Smart-Redaction integration tests

**Files:**
- Create: `crates/rollshot-vision/tests/ocr_integration.rs`

**Interfaces:**
- Consumes: `RealAutomationHost::prepare_ocr`, `QuickJsExecutor`, `execute_to_proposal`, `ExecutionPolicy::smart_redaction_default` (same harness as `tests/integration.rs`).

- [ ] **Step 1: Write the gated integration test file**

```rust
#![cfg(feature = "ocr")]
//! Real-OCR Smart-Redaction e2e (spec §7). Gated behind `ocr`; fixtures render
//! deterministic text with the vendored DejaVu font (eng-review D8).

use std::time::Duration;

use ab_glyph::FontRef;
use image::Rgba;
use imageproc::drawing::{draw_text_mut, text_size};
use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    OcrQuery, ProposalContext, ProposedEdit, ProposedEditKind, Region, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{EditProposal, ProposalId, Provenance, ProvenanceSource};
use rollshot_image_document::ImageRect;
use rollshot_vision::{RealAutomationHost, VisualIndex};

const FONT: &[u8] = include_bytes!("../../rollshot-image-document/assets/fonts/DejaVuSans.ttf");

/// White scene with black `text` at (x,y); returns (scene, text box in image coords).
fn scene_with_text(w: u32, h: u32, x: i32, y: i32, px: f32, text: &str) -> (image::RgbaImage, ImageRect) {
    let font = FontRef::try_from_slice(FONT).unwrap();
    let mut img = image::RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
    draw_text_mut(&mut img, Rgba([0, 0, 0, 255]), x, y, px, &font, text);
    let (tw, th) = text_size(px, &font, text);
    (img, ImageRect { x: x as f32, y: y as f32, width: tw as f32, height: th as f32 })
}

fn overlaps(a: &ImageRect, b: &ImageRect) -> bool {
    a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

/// Run a redaction script over a prepared OCR scene. `region` controls prepare/query.
fn run_ocr(js: &str, scene: image::RgbaImage, region: Region) -> EditProposal {
    let (w, h) = scene.dimensions();
    let automation = validate_source(js, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: Some(region),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 1 } },
    };
    let mut policy =
        ExecutionPolicy::smart_redaction_default(Duration::from_secs(5), 32 * 1024 * 1024, 256 * 1024);
    policy.allowed_edit_kinds.insert(ProposedEditKind::AddRedaction);

    let index = VisualIndex::build(scene).unwrap();
    let mut host = RealAutomationHost::new();
    host.prepare_ocr(&index, &OcrQuery { region, limit: 50 }).unwrap();
    let (proposal, _m) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    proposal
}

// Redact every OCR match whose text matches a predicate, padded.
const REDACT_JS: &str = r#"
function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 50 });
  return {
    candidates: matches
      .filter((m) => PREDICATE(m.text))
      .map((m) => ({
        kind: "addRedaction",
        bounds: { x: m.bounds.x - 2, y: m.bounds.y - 2, width: m.bounds.width + 4, height: m.bounds.height + 4 },
        confidence: m.confidence,
        label: "ocr-candidate",
      })),
  };
}
"#;

fn js_with(predicate: &str) -> String {
    REDACT_JS.replace("PREDICATE(m.text)", predicate)
}

fn candidate_bounds(p: &EditProposal) -> Vec<ImageRect> {
    p.candidates
        .iter()
        .filter_map(|c| match &c.edit {
            ProposedEdit::AddRedaction { bounds } => Some(*bounds),
            _ => None,
        })
        .collect()
}

#[test]
fn email_masking() {
    let (scene, email_box) = scene_with_text(700, 200, 30, 60, 44.0, "contact@example.com");
    let p = run_ocr(&js_with("m.text.indexOf('@') >= 0"), scene, Region::Full);
    let bounds = candidate_bounds(&p);
    assert!(!bounds.is_empty(), "expected ≥1 email candidate");
    assert!(bounds.iter().any(|b| overlaps(b, &email_box)));
}

#[test]
fn ssn_like() {
    let (scene, ssn_box) = scene_with_text(700, 200, 30, 60, 44.0, "123-45-6789");
    let p = run_ocr(
        &js_with("m.text.indexOf('-') >= 0 && (m.text.match(/[0-9]/g) || []).length >= 4"),
        scene,
        Region::Full,
    );
    assert!(candidate_bounds(&p).iter().any(|b| overlaps(b, &ssn_box)));
}

#[test]
fn key_value() {
    let (scene, tok_box) = scene_with_text(800, 200, 30, 60, 40.0, "Token: AKIAEXAMPLEKEY");
    let p = run_ocr(&js_with("m.text.indexOf('Token') === 0"), scene, Region::Full);
    assert!(candidate_bounds(&p).iter().any(|b| overlaps(b, &tok_box)));
}

#[test]
fn no_match_no_error() {
    let scene = image::RgbaImage::from_pixel(400, 120, Rgba([255, 255, 255, 255]));
    let p = run_ocr(&js_with("m.text.indexOf('@') >= 0"), scene, Region::Full);
    assert_eq!(candidate_bounds(&p).len(), 0);
}

#[test]
fn bounded_region_query() {
    // Email inside the queried rect; "noise@out" far below it, outside the rect.
    let mut scene = image::RgbaImage::from_pixel(700, 400, Rgba([255, 255, 255, 255]));
    let font = FontRef::try_from_slice(FONT).unwrap();
    draw_text_mut(&mut scene, Rgba([0, 0, 0, 255]), 30, 40, 40.0, &font, "inside@example.com");
    draw_text_mut(&mut scene, Rgba([0, 0, 0, 255]), 30, 320, 40.0, &font, "outside@example.com");
    let region = Region::Rect { bounds: ImageRect { x: 0.0, y: 0.0, width: 700.0, height: 160.0 } };
    let p = run_ocr(&js_with("m.text.indexOf('@') >= 0"), scene, region);
    let bounds = candidate_bounds(&p);
    assert!(!bounds.is_empty());
    // every candidate is in the top region (y well under 320).
    assert!(bounds.iter().all(|b| b.y < 200.0));
}
```

- [ ] **Step 2: Run the integration tests**

Run: `rtk cargo test -p rollshot-vision --features ocr --test ocr_integration`
Expected: PASS (5 tests). If a substring filter misses (e.g. `@` misread), raise the font `px` (D10 — fixtures must render large/high-contrast).

- [ ] **Step 3: Add the tracing-privacy test (append to `ocr_integration.rs`)**

Add `tracing-subscriber` as a dev-dependency in `crates/rollshot-vision/Cargo.toml`:

```toml
[dev-dependencies]
tracing-subscriber = { workspace = true }
```

Test (captures all `tracing` events into a buffer and asserts the secret never appears):

```rust
#[test]
fn no_ocr_text_or_pixels_in_tracing() {
    use std::sync::{Arc, Mutex};
    use std::io::Write;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    impl<'a> MakeWriter<'a> for Buf {
        type Writer = Buf;
        fn make_writer(&'a self) -> Buf { self.clone() }
    }

    let buf = Buf(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();

    let secret = "topsecret@example.com";
    tracing::subscriber::with_default(subscriber, || {
        let (scene, _) = scene_with_text(700, 200, 30, 60, 44.0, secret);
        let _ = run_ocr(&js_with("m.text.indexOf('@') >= 0"), scene, Region::Full);
    });

    let captured = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(
        !captured.contains("secret") && !captured.contains('@'),
        "OCR text leaked into tracing: {captured}"
    );
}
```

- [ ] **Step 4: Run the privacy test + full feature suite**

Run: `rtk cargo test -p rollshot-vision --features ocr`
Expected: PASS (host unit tests + 6 integration tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-vision/tests/ocr_integration.rs crates/rollshot-vision/Cargo.toml
git commit -m "test(ocr): real-OCR Smart-Redaction e2e + tracing-privacy test"
```

---

## Task 4: CI lanes (default-exclude + path-filtered OCR lane)

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `.github/workflows/ci-ocr.yml`
- Create: `scripts/ci/provision-onnxruntime.sh`

**Interfaces:**
- Consumes: the `ocr` feature (Task 2), `default-members` exclusion (Task 1), build.rs model provisioning + ORT lib provisioning (Task 1 Step 3/4).

- [ ] **Step 1: Exclude `rollshot-ocr` from the default lane (`.github/workflows/ci.yml`)**

Change every workspace build/test step so the default lane never builds the OCR toolchain:

```yaml
      - name: Clippy
        run: cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings

      - name: Test
        run: cargo test --workspace --exclude rollshot-ocr

      - name: Clippy (action-guide feature)
        run: cargo clippy --workspace --exclude rollshot-ocr --all-targets --features rollshot-app/action-guide -- -D warnings

      - name: Test (action-guide feature)
        run: cargo test --workspace --exclude rollshot-ocr --features rollshot-app/action-guide
```

(`cargo fmt --all -- --check` stays as-is — formatting is build-free and should still cover `rollshot-ocr`.)

- [ ] **Step 2: Create the OCR lane (`.github/workflows/ci-ocr.yml`)**

```yaml
name: CI (OCR)

on:
  pull_request:
    paths:
      - "crates/rollshot-ocr/**"
      - "crates/rollshot-vision/**"
      - "crates/rollshot-automation/**"
      - "crates/rollshot-image-document/**"
      - "Cargo.toml"
      - ".github/workflows/ci-ocr.yml"
  push:
    branches: [main]
  workflow_dispatch:

jobs:
  ocr:
    name: OCR (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, macos-14]
    env:
      ROLLSHOT_OCR_MODELS_DIR: ${{ github.workspace }}/.ocr-models
      ORT_LIB_LOCATION: ${{ github.workspace }}/.ort/lib
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - name: Install Linux capture deps
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-dev libdbus-1-dev libxkbcommon-dev

      # Models: warm cache → offline; cold cache → build.rs Release download.
      - name: Cache OCR models
        uses: actions/cache@v4
        with:
          path: ${{ github.workspace }}/.ocr-models
          key: ocr-models-ppocrv4-${{ hashFiles('crates/rollshot-ocr/build.rs') }}

      # ONNX Runtime static lib (eng-review D4 / §3.3): vendor + ORT_LIB_LOCATION.
      - name: Cache ONNX Runtime lib
        id: ort-cache
        uses: actions/cache@v4
        with:
          path: ${{ github.workspace }}/.ort
          key: ort-static-${{ matrix.os }}-2.0.0-rc.10
      - name: Provision ONNX Runtime static lib
        if: steps.ort-cache.outputs.cache-hit != 'true'
        run: |
          mkdir -p "${{ github.workspace }}/.ort"
          # Download the version-matched static build from supertone-inc/onnxruntime-build
          # for this runner OS, extract so $ORT_LIB_LOCATION contains libonnxruntime*.a.
          # (Exact asset URL/version verified in Task 1 Step 3.)
          ./scripts/ci/provision-onnxruntime.sh "${{ runner.os }}" "${{ github.workspace }}/.ort"

      - uses: swatinem/rust-cache@v2

      - name: Check ONNX Runtime provision script
        run: bash -n scripts/ci/provision-onnxruntime.sh
      - name: Clippy (ocr)
        run: cargo clippy -p rollshot-ocr -p rollshot-vision --features rollshot-vision/ocr --all-targets -- -D warnings
      - name: Test rollshot-ocr
        run: cargo test -p rollshot-ocr
      - name: Test rollshot-vision (ocr)
        run: cargo test -p rollshot-vision --features ocr
```

- [ ] **Step 3: Add `scripts/ci/provision-onnxruntime.sh`**

A small script that downloads the matching `supertone-inc/onnxruntime-build` static asset per OS into the target dir's `lib/`. Verify the exact version/URL against `ort 2.0.0-rc.10` during Task 1 Step 3, then encode it here:

```bash
#!/usr/bin/env bash
set -euo pipefail
os="$1"; dest="$2"
ver="1.22.1"   # Snow Shot-validated supertone static lib for ort 2.0.0-rc.10; verify in Task 1.
base="https://github.com/supertone-inc/onnxruntime-build/releases/download/v${ver}"
case "$os" in
  Linux)  asset="onnxruntime-linux-x64-static_lib-${ver}.tgz" ;;
  macOS)  asset="onnxruntime-osx-universal2-static_lib-${ver}.tgz" ;;
  *) echo "unsupported os: $os" >&2; exit 1 ;;
esac
tmp="$(mktemp -d)"
curl -fL "${base}/${asset}" -o "${tmp}/ort.tgz"
tar -xzf "${tmp}/ort.tgz" -C "${tmp}"
libdir="$(find "${tmp}" -type d -name lib | head -n1)"
mkdir -p "${dest}"
cp -r "${libdir}" "${dest}/lib"
```

Then run `rtk chmod +x scripts/ci/provision-onnxruntime.sh`.

- [ ] **Step 4: Verify the default lane is clean locally (proxy for CI)**

Run:
```bash
rtk bash -n scripts/ci/provision-onnxruntime.sh
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk cargo test --workspace --exclude rollshot-ocr
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets --features rollshot-app/action-guide -- -D warnings
rtk cargo test --workspace --exclude rollshot-ocr --features rollshot-app/action-guide
```
Expected: PASS, no `ort`/model build (the OCR crate is skipped; `rollshot-vision` builds with `ocr` off).

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/ci-ocr.yml scripts/ci/provision-onnxruntime.sh
git commit -m "ci(ocr): default lane excludes rollshot-ocr; add path-filtered OCR lane"
```

---

## Task 5: Docs, handoff, parent-spec status

**Files:**
- Modify: `AGENTS.md` (§9 Project Map, §10 learn-projects)
- Modify: `README.md` (Workspace list)
- Create: `docs/superpowers/handoffs/2026-06-25-ocr-backend.md`
- Modify: `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md` (§12 status)

- [ ] **Step 1: Update `AGENTS.md` §9 + §10**

Add a `rollshot-ocr` project-map entry (unsafe-isolation crate for RapidOCR/ONNX Runtime OCR; safe API; bundled PP-OCRv4 models provisioned by build.rs; excluded from `default-members`; used by `rollshot-vision` behind the `ocr` feature). Update the `rollshot-vision` entry: `ocr` is real behind the off-by-default `ocr` feature via `rollshot-ocr`; `layout` remains stubbed. Strengthen the `snow-shot` row in §10: validated OCR-stack reference (paddle-ocr-rs + ort + RapidOCR; runtime plugin-download model strategy, supertone static ORT lib in CI).

- [ ] **Step 2: Update `README.md` Workspace list**

Add `rollshot-ocr` and `rollshot-vision` entries (the latter is currently absent).

- [ ] **Step 3: Write the completion handoff**

Create `docs/superpowers/handoffs/2026-06-25-ocr-backend.md` recording (spec §10): delivered crate + exact pins; public API + usage example; bundled model hashes + Release source; `prepare_ocr`/`ocr` wiring; integration-test evidence (Linux + macOS); measured OCR-lane wall-clock (D14); known limitations (angle handling, `layout` stubbed, `ch`-set only, ORT-lib provisioning recipe); how SP6 consumes `ocr`; migration notes for `ort`/`paddle-ocr-rs`/`ndarray` upgrades and the SP6 switch to a self-contained static-ORT product build.

- [ ] **Step 4: Update parent spec §12 delivery status**

In `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md` §12, record that the OCR backend is now a delivered subproject (do not rewrite historical decisions — append a delivery note).

- [ ] **Step 5: Final full verification**

Run:
```bash
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk cargo test --workspace --exclude rollshot-ocr
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets --features rollshot-app/action-guide -- -D warnings
rtk cargo test --workspace --exclude rollshot-ocr --features rollshot-app/action-guide
rtk bash -n scripts/ci/provision-onnxruntime.sh
rtk cargo clippy -p rollshot-ocr -p rollshot-vision --features rollshot-vision/ocr --all-targets -- -D warnings
rtk cargo test -p rollshot-ocr
rtk cargo test -p rollshot-vision --features ocr
```
Expected: all clean/PASS.

- [ ] **Step 6: Commit**

```bash
git add AGENTS.md README.md docs/superpowers/handoffs/2026-06-25-ocr-backend.md docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md
git commit -m "docs(ocr): project map, README, completion handoff, parent-spec status"
```

---

## Engineering Review Addendum (2026-06-25, auto mode)

### Step 0: Scope Challenge

- **Goal vs steps alignment:** All five tasks contribute to the goal: crate isolation, host wiring, real Smart-Redaction OCR tests, CI distribution, and docs/handoff. No task is pure scope creep.
- **Existing code reused:** `rollshot-vision::RealAutomationHost` already has the prepare-then-cached-callback pattern in `prepare_region_features` / `region_features`; this plan reuses that shape. `crates/rollshot-vision/src/rect.rs` already owns finite/empty/oversized region validation; OCR adds only `MAX_OCR_AREA`. `rollshot-automation` already defines `OcrQuery`, `OcrMatch`, `AutomationHost::ocr`, and `CapabilityError`; the plan does not rebuild those contracts. Snow Shot (`learn-projects/snow-shot`) already validates the RapidOCR + `paddle-ocr-rs` + `ort` approach, cached `OcrLite`, memory-loaded models, explicit ORT session tuning, and supertone static ONNX Runtime provisioning.
- **Minimum viable plan:** Tasks 1-4 are required to achieve the goal. Task 5 is not runtime-critical, but it is required for handoff and future maintainability because this introduces an unsafe-isolation crate plus native model/lib provisioning.
- **Complexity check:** 5 tasks, 17 declared file entries after review, 1 new crate, 1 new CI script. Net-new files are 8, below the >12 smell threshold. Scope accepted as-is.
- **Search/reference check:** `paddle-ocr-rs` is explicitly RapidOCR/ONNX Runtime based; `ort 2.0.0-rc.12` has newer multiversion guidance, but this plan pins `rc.10` for local dependency compatibility; supertone publishes static ONNX Runtime releases including `v1.22.1`/`v1.22.2`; Snow Shot uses `ort = 2.0.0-rc.10` with supertone `1.22.1`. No built-in Rust OCR facility replaces this.
- **Completeness check:** AI-assisted execution makes complete negative tests and CI gating cheap; the plan now includes the missing `Send`/defaults/upscale tests, default/action-guide CI exclusions, and script syntax verification.
- **Distribution check:** The plan introduces a library crate and native build artifacts, not a user-installed binary. CI provisioning and docs/handoff are in scope; product packaging of a self-contained ORT build is explicitly deferred below.

### Auto Decisions Applied

**Auto decision D1 — Keep the five-task scope**
Context: The file count is above 12 only because CI/docs/handoff are included with the runtime work.
ELI10: The plan is not trying to build a second OCR product; it is making one backend usable, tested, and buildable. Removing CI or docs would make the code easier to write but harder to ship safely. The expensive part is the native dependency, so keeping CI in scope is justified.
Stakes if we pick wrong: Cutting CI/docs would let a native-link failure land silently and block the next engineer.
Recommendation: 1A because the current scope is the smallest complete shippable slice.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Keep Tasks 1-5 (recommended) — effort human: ~4-6 days / AI: ~1-2 hours; risk medium; maintenance burden low after CI exists.
  ✅ Produces code, tests, native dependency proof, and handoff together.
  ❌ Bigger review surface than a runtime-only patch.
B) Defer Task 5 docs/handoff — effort human: ~3-5 days / AI: ~1 hour; risk medium-high; maintenance burden higher.
  ✅ Slightly less editing now.
  ❌ Leaves native dependency/version knowledge in chat or commit history.
Net: Keep the complete slice because native dependency knowledge must be captured where maintainers will look.

**Auto decision D2 — Use Snow Shot-style ORT session tuning**
Context: The original `OcrEngine::new` used `init_models_from_memory` with only a thread count.
ELI10: OCR works by running neural-network sessions. Snow Shot already proved this stack in a screenshot app and explicitly configures ORT threads and graph optimization. Rollshot should copy that boring, known shape instead of hoping defaults are equivalent.
Stakes if we pick wrong: OCR may be slower, less predictable, or different across Linux/macOS.
Recommendation: 2A because explicit session settings match the reference project and reduce runtime variance.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) Use `init_models_from_memory_custom` + `build_session` (recommended) — effort human: ~30 min / AI: ~5 min; risk low; maintenance burden low.
  ✅ Mirrors Snow Shot's validated service behavior.
  ❌ Couples the wrapper to `ort::SessionBuilder` API details inside the isolation crate.
B) Keep default `init_models_from_memory` — effort human: 0 / AI: 0; risk medium; maintenance burden medium.
  ✅ Less code.
  ❌ Leaves performance/configuration implicit.
Net: This spends a few lines to make the native runtime behavior explicit.

**Auto decision D3 — Pin provisional static ORT to supertone `1.22.1`, not `1.22.2`**
Context: Local `ort-sys 2.0.0-rc.10` declares ONNX Runtime `1.22.0`, while Snow Shot documents supertone `1.22.1` for the same ORT line.
ELI10: A patch-level native library mismatch can be fine, but it should be deliberate. Snow Shot is the closest real reference we have. The plan should start from its known-good `1.22.1`, then let the OCR CI prove Linux and macOS.
Stakes if we pick wrong: CI or product builds may fail at link time or load a subtly incompatible runtime.
Recommendation: 3A because it is the most evidence-backed static-lib default.
Completeness: A=9/10, B=6/10, C=5/10
Pros / cons:
A) Default to `1.22.1` and hard-gate in CI (recommended) — effort human: ~30 min / AI: ~5 min; risk low-medium; maintenance burden low.
  ✅ Matches the Snow Shot reference and available supertone release.
  ❌ Still requires the Task 1 smoke check to confirm Rollshot's exact build.
B) Use `1.22.2` — effort human: ~30 min / AI: ~5 min; risk medium; maintenance burden medium.
  ✅ Newer patch release.
  ❌ Not the version validated by the reference project.
C) Leave the script placeholder — effort human: 0 / AI: 0; risk high; maintenance burden high.
  ✅ Avoids deciding now.
  ❌ CI cannot be trusted until someone fills it in later.
Net: Start from the closest validated version, then let the dedicated OCR lane prove it.

**Auto decision D4 — Exclude `rollshot-ocr` from action-guide workspace CI too**
Context: The original CI edit excluded OCR from the normal clippy/test steps but left action-guide `--workspace` steps unchanged.
ELI10: `--workspace` means "all workspace members," so the heavy OCR crate can still sneak into CI through a feature lane that has nothing to do with OCR. That defeats the whole default-members/exclude strategy.
Stakes if we pick wrong: Default CI may download/link ORT unexpectedly and fail unrelated PRs.
Recommendation: 4A because every default workspace lane must have the same exclusion rule.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Add `--exclude rollshot-ocr` to action-guide clippy/test (recommended) — effort human: ~15 min / AI: ~2 min; risk low; maintenance burden low.
  ✅ Keeps non-OCR CI independent from native OCR assets.
  ❌ One more repeated CLI flag in CI.
B) Exclude only normal clippy/test — effort human: 0 / AI: 0; risk high; maintenance burden medium.
  ✅ Smaller YAML diff.
  ❌ The heavy crate can still build in default CI.
Net: CI behavior must be explicit over clever; repeat the flag.

**Auto decision D5 — Declare and syntax-check the ONNX Runtime provision script**
Context: Task 4 created `scripts/ci/provision-onnxruntime.sh` but the file was missing from top-level declarations and verification.
ELI10: A CI workflow that calls a script is only as reliable as the script existing and parsing. Shell syntax checks are cheap and catch broken quoting before CI runs.
Stakes if we pick wrong: The OCR lane may fail immediately on a missing or malformed helper script.
Recommendation: 5A because file lists and Run/Expected checks must match actual task edits.
Completeness: A=10/10, B=5/10
Pros / cons:
A) Add the script to File Structure/Task 4 and run `bash -n` (recommended) — effort human: ~15 min / AI: ~2 min; risk low; maintenance burden low.
  ✅ Makes the plan internally consistent and verifiable.
  ❌ Adds one extra local verification command.
B) Leave it implicit in the workflow — effort human: 0 / AI: 0; risk medium; maintenance burden medium.
  ✅ Fewer doc lines.
  ❌ Execution agents can miss staging or validating the script.
Net: File declarations should be boringly accurate.

**Auto decision D6 — Add low-cost API contract tests**
Context: The original `rollshot-ocr` tests were mostly real-OCR behavior tests.
ELI10: Some promises do not need OCR models to test: `OcrEngine` being `Send`, default query values, and scale clamping. These are public contracts and should fail fast when someone changes them.
Stakes if we pick wrong: A later edit can break host compatibility or detection defaults without a focused failure.
Recommendation: 6A because public API guarantees deserve small direct tests.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Add `Send`, default-query, and scale-cap tests (recommended) — effort human: ~45 min / AI: ~5 min; risk low; maintenance burden low.
  ✅ Catches contract drift without depending on OCR recognition quality.
  ❌ Adds three more tests to maintain.
B) Rely on integration tests only — effort human: 0 / AI: 0; risk medium; maintenance burden medium.
  ✅ Less test code.
  ❌ Failures will be slower and less diagnostic.
Net: Complete coverage is cheap here and improves debugging.

**Auto decision D7 — Keep real-OCR tests gated but deterministic**
Context: Real OCR tests depend on native models and recognition quality, but they run in the dedicated OCR lane.
ELI10: OCR is inherently probabilistic enough that tiny text or weak fixtures can be flaky. The plan already uses large black text on white with a deterministic font; the review keeps that path and adds API-level tests for invariants that should not rely on OCR output.
Stakes if we pick wrong: CI may fail intermittently or hide real OCR regressions behind overly mocked tests.
Recommendation: 7A because real OCR must be exercised, but deterministic fixtures and smaller unit tests reduce flake blast radius.
Completeness: A=9/10, B=5/10
Pros / cons:
A) Keep gated real OCR plus stronger unit contracts (recommended) — effort human: ~1 day / AI: ~20 min; risk medium-low; maintenance burden medium.
  ✅ Proves the actual Smart-Redaction path without burdening default CI.
  ❌ Requires ORT/model assets in the OCR lane.
B) Replace real OCR with mocks only — effort human: ~0.5 day / AI: ~10 min; risk high; maintenance burden low.
  ✅ Faster and simpler CI.
  ❌ Does not prove the capability this plan exists to deliver.
Net: Real OCR is the product behavior; keep it, but isolate it.

**Auto decision D8 — Cap OCR upscale memory and avoid RGBA crop duplication**
Context: Default `min_scale = 1.5` can turn a large full-screen region into a much larger working image, and the host snippet originally allocated RGBA crop plus RGB copy.
ELI10: Screenshots are big. If we enlarge every image and duplicate the crop first, memory usage grows fast and users with tall captures can hit slowdowns or OOM. The fix keeps the same API but bounds the working image and does one RGB allocation.
Stakes if we pick wrong: OCR can become the new memory hotspot, especially on long screenshots.
Recommendation: 8A because it preserves the feature while putting a clear ceiling on resource use.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Add private `MAX_UPSCALED_PIXELS` and direct RGBA→RGB crop conversion (recommended) — effort human: ~1-2 hours / AI: ~15 min; risk low; maintenance burden low.
  ✅ Prevents unbounded upscale memory and removes one full-image allocation.
  ❌ Very large direct engine inputs may get less upscale, or a downscale if they bypass the host area cap.
B) Keep unconditional upscale and double allocation — effort human: 0 / AI: 0; risk high; maintenance burden medium.
  ✅ Simplest code.
  ❌ Memory use scales badly with capture size.
Net: The bounded version is still simple and much safer in the hot path.

### Test Coverage Table

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / workspace membership and default-members exclusion | - | - | smoke via default CI | no |
| Task 1 / model provisioning, SHA256 verification, Release fallback | yes | - | OCR CI build | no |
| Task 1 / `OcrEngine::new` session init with static ORT | yes | - | OCR CI linux+macOS | no |
| Task 1 / `OcrEngine::detect` text, blank image, zero-dim error | yes | - | - | no |
| Task 1 / defaults, `Send`, upscale coordinate inversion, scale cap | yes | - | - | no |
| Task 2 / OCR feature gate off returns `capability_unavailable` | yes | - | default CI | no |
| Task 2 / `prepare_ocr` crop mapping to full-image bounds | yes | - | - | no |
| Task 2 / zero limit, unprepared, over-limit, non-finite region | yes | - | - | no |
| Task 3 / Smart-Redaction email, SSN-like, key-value OCR | - | yes | yes | no |
| Task 3 / blank image no-match and bounded-region query | - | yes | yes | no |
| Task 3 / no OCR text/pixels in tracing | - | yes | yes | no |
| Task 4 / default CI excludes OCR native dependency | - | - | yes | no |
| Task 4 / OCR CI provisions models + static ORT on Linux/macOS | - | - | yes | no |
| Task 5 / docs and handoff completeness | - | - | - | review only |

### Failure Modes

| New codepath | Production failure | Test covers it | Error handling in plan | User-visible result |
|---|---|---|---|---|
| `build.rs` model provisioning | Release download missing or hash mismatch | Task 1 Step 7 / OCR CI build | build panic with model name/hash | build failure, not silent |
| Static ONNX Runtime provisioning | wrong static lib version or missing archive | Task 1 Step 3; Task 4 OCR CI Linux/macOS | CI/link failure; handoff records version | build failure, not silent |
| `OcrEngine::new` | ORT session init fails | Task 1 `new_succeeds`; Task 2 maps init error | `OcrError::SessionInit` → `Failed { code: "ocr_session_init" }` | clear capability failure code |
| `OcrEngine::detect` | empty/invalid image | Task 1 `detect_rejects_zero_dim` | `OcrError::InvalidImage` | clear capability failure code when surfaced |
| Scale path | huge image would allocate too much | Task 1 `effective_scale_caps_large_images` | private scale clamp | degraded upscale/downscale, not crash |
| Host OCR prepare | non-finite or too-large region | Task 2 `prepare_ocr_rejects_non_finite_region`; existing rect tests cover oversize | `CapabilityError::InvalidInput` | clear invalid input code |
| Host OCR callback | called before prepare or with higher limit | Task 2 unprepared / limit tests | `vision_index_unavailable` / `LimitExceeded` | clear capability failure |
| Smart-Redaction e2e | OCR returns no text on sensitive fixture | Task 3 email/SSN/key tests fail | assertion failure in OCR CI | CI failure, not silent |
| Tracing | OCR text or image bytes leak | Task 3 privacy test | no text/pixels in tracing fields | CI failure, not silent |

**Critical gaps:** none after review edits. The original gaps were action-guide CI still building OCR, no script syntax check, no `Send`/defaults/scale-cap tests, and unbounded upscale memory; all are now covered by Tasks 1, 2, and 4.

### NOT in scope

- Product UI toggle for OCR: backend plumbing only; UI exposure belongs to the Smart-Redaction product plan.
- `layout` capability implementation: remains `capability_unavailable`; this plan only replaces `ocr`.
- Windows OCR CI/product support: reference Snow Shot is Windows-heavy, but this Rollshot plan gates Linux/macOS first to match current CI.
- Shipping a fully self-contained static-ORT product artifact: deferred to SP6/product packaging; this plan proves CI and dev builds.
- OCR model upgrades beyond PP-OCRv4 Chinese set: version/hash changes need a separate migration plan.
- Angle detection as default behavior: `do_angle=false` remains default to reduce false corrections for screenshots.

### What already exists

- `RealAutomationHost` prepare/cache/callback pattern: reused for OCR via `prepare_ocr` + cached `ocr`.
- `rect.rs` finite/empty/clamped/oversized region validation: reused with new `MAX_OCR_AREA`.
- `rollshot-automation` OCR types and host trait: reused; no duplicate public contract.
- `VisualIndex` image ownership: reused as source image for OCR crops.
- Smart-Redaction automation harness (`execute_to_proposal`, `QuickJsExecutor`, `ExecutionPolicy::smart_redaction_default`): reused for real-OCR integration tests.
- Snow Shot OCR service: used as reference for `OcrLite`, memory-loaded models, explicit ORT session tuning, and supertone static ONNX Runtime provisioning.

### Parallelization Strategy

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1: `rollshot-ocr` isolation crate | workspace root, `crates/rollshot-ocr/` | - |
| Task 2: host wiring | `crates/rollshot-vision/`, `crates/rollshot-automation/` contracts consumed | Task 1 |
| Task 3: real-OCR integration tests | `crates/rollshot-vision/tests/`, `crates/rollshot-vision/Cargo.toml` | Task 2 |
| Task 4: CI lanes | `.github/workflows/`, `scripts/ci/` | Task 1, Task 2 |
| Task 5: docs/handoff | `AGENTS.md`, `README.md`, `docs/superpowers/` | Tasks 1-4 |

Sequential execution is recommended. Task 1 modifies the workspace root and creates the crate every later task depends on; Task 2 creates the host API used by Task 3; Task 4 needs the final feature/build shape; Task 5 records the completed result. No useful parallel lane outweighs merge/coordination risk.

### Completion Summary

Plan reviewed:           `docs/superpowers/plans/2026-06-25-ocr-backend.md`
Tasks in plan:           5
Files Create/Modify:     8 create / 9 modify

- Step 0: Scope Challenge   — accepted as-is
- Architecture Review:        3 issues resolved (session tuning, ORT version, CI distribution)
- Plan Structure + Code Q:    2 issues resolved (file declarations, script verification)
- Test Review:                table produced, 2 gaps resolved
- Performance Review:         1 issue resolved
- NOT in scope:               written
- What already exists:        written
- Failure modes:              0 critical gaps after edits
- Parallelization:            1 lane, sequential execution recommended
- Unresolved decisions:       0

Plan is locked in — run `superpowers:executing-plans` for a single sequential implementation path. `superpowers:subagent-driven-development` is not recommended unless Task 5 docs are intentionally split after Task 4 passes.

### Plan Edits From Review

- Added Snow Shot-style ORT session builder (`init_models_from_memory_custom`, physical-thread settings, Level3 optimization).
- Added OCR upscale memory cap and tests for scaling, `Send`, and default query values.
- Changed host OCR crop conversion to avoid an intermediate full RGBA crop allocation.
- Added `scripts/ci/provision-onnxruntime.sh` to all file declarations and verification.
- Changed provisional static ORT script version from `1.22.2` to Snow Shot-validated `1.22.1`, with explicit hard-gate verification.
- Added `--exclude rollshot-ocr` to action-guide CI clippy/test commands.
- Added `bash -n` verification for the provision script and expanded final verification.
- Added required NOT-in-scope, existing-code, failure-mode, coverage, and parallelization sections.

## Self-Review

**Spec coverage** (spec §2 in-scope → task):
- New `rollshot-ocr` crate, pins, bundled models, SHA256, no-rollshot-deps → Task 1. ✓
- `prepare_ocr` + cached `ocr` callback replacing the stub; `OcrDetection`→`OcrMatch` + quad→AABB → Task 2. ✓
- Lazy `Option<OcrEngine>` (D2); area cap (D13); coordinate inversion split (D6/D15) → Task 1 (`detect` inversion) + Task 2 (crop offset, cap). ✓
- Feature gate + default-members (D17) → Task 1 (root Cargo) + Task 2 (vision Cargo + cfg) + Task 4 (CI). ✓
- Models out of git + build.rs + cache + Release fallback (D16); etcetera cache dir → Task 1. ✓
- Real-OCR JS e2e (5 scenarios) + vendored font (D8) + flakiness handling (D10) + privacy test (D9) → Task 3. ✓
- macOS hard gate → Task 4 (`macos-14` OCR lane). ✓
- ORT static-vendor recipe (D4) → Task 4 Step 2–3. ✓
- Docs/handoff/parent status → Task 5. ✓
- Negative tests: zero-limit, unprepared, limit-exceeded, non-finite region (D11), zero-dim image → Tasks 1–2. ✓
- D14 timing → recorded in handoff (Task 5 Step 3). ✓

**Placeholder scan:** No `TODO`/"add error handling"/"similar to". The one externally-sensitive value is the static ONNX Runtime asset version (`1.22.1` in `provision-onnxruntime.sh`) and the Release tag URL. Both are concrete defaults, Snow Shot-informed, and hard-gated by Task 1 Step 3 plus the Linux/macOS OCR CI lane; they are not silent placeholders.

**Type consistency:** `OcrDetection { x,y,w,h,text,confidence }`, `OcrRegionQuery::default()`, `OcrEngine::{new,detect}`, `OcrError`, `MAX_OCR_AREA`, `OcrKey`/`PreparedOcr`, error codes (`ocr_session_init`, `ocr_detect`, `vision_index_unavailable`, `invalid_query`, `capability_unavailable`) are used identically across Tasks 1–3. `detect` returns input-native coords (Task 1), vision adds crop offset only (Task 2) — the split matches §4.3. ✓
