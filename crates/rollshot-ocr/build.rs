//! Provisions the three PP-OCRv4 ONNX models into OUT_DIR (eng-review D16).
//!
//! Resolution order:
//!
//!   1. $ROLLSHOT_OCR_MODELS_DIR override (offline / CI-warm path)
//!   2. etcetera cache dir (warm from a previous build)
//!   3. RapidOCR official ModelScope URL (primary, always available)
//!   4. rollshot GitHub Release mirror (optional fallback, may not exist yet)
//!
//! Every model's SHA256 is verified against the recorded hash (build fails on
//! mismatch). The upstream RapidOCR filenames use the `_mobile.onnx` suffix
//! (v3.9.0 tag); lib.rs expects the legacy `_infer.onnx` names, so build.rs
//! downloads under the official name but writes to OUT_DIR under the stable
//! name used by include_bytes!.
use std::{env, fs, io::Read, path::PathBuf};

use sha2::{Digest, Sha256};

struct Model {
    /// Filename written to OUT_DIR and used by lib.rs include_bytes!.
    out_name: &'static str,
    /// Filename used by RapidOCR's official ModelScope distribution.
    cache_name: &'static str,
    sha256: &'static str,
    primary_url: &'static str,
}

const MODELS: &[Model] = &[
    Model {
        out_name: "ch_PP-OCRv4_det_infer.onnx",
        cache_name: "ch_PP-OCRv4_det_mobile.onnx",
        sha256: "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
        primary_url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.0/onnx/PP-OCRv4/det/ch_PP-OCRv4_det_mobile.onnx",
    },
    Model {
        out_name: "ch_ppocr_mobile_v2.0_cls_infer.onnx",
        cache_name: "ch_ppocr_mobile_v2.0_cls_mobile.onnx",
        sha256: "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
        primary_url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.0/onnx/PP-OCRv4/cls/ch_ppocr_mobile_v2.0_cls_mobile.onnx",
    },
    Model {
        out_name: "ch_PP-OCRv4_rec_infer.onnx",
        cache_name: "ch_PP-OCRv4_rec_mobile.onnx",
        sha256: "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
        primary_url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.0/onnx/PP-OCRv4/rec/ch_PP-OCRv4_rec_mobile.onnx",
    },
];

// Optional mirror fallback — maintainer may upload the three files here later.
// Not a prerequisite; the primary ModelScope URLs are always tried first.
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

fn download(url: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    ureq::get(url)
        .call()
        .unwrap_or_else(|e| panic!("download {url}: {e}"))
        .into_reader()
        .read_to_end(&mut buf)
        .unwrap_or_else(|e| panic!("read body {url}: {e}"));
    buf
}

fn main() {
    println!("cargo:rerun-if-env-changed=ROLLSHOT_OCR_MODELS_DIR");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let src = cache_dir();
    fs::create_dir_all(&src).ok();

    for model in MODELS {
        // 1. Try the local cache (keyed by the official RapidOCR filename).
        let local = src.join(model.cache_name);
        let bytes = if local.is_file() {
            fs::read(&local).unwrap_or_else(|e| panic!("read {}: {e}", local.display()))
        } else {
            // 2. Try the official ModelScope URL (primary, always available).
            // 3. Fall back to the optional GitHub Release mirror.
            let bytes = match ureq::get(model.primary_url).call() {
                Ok(resp) => {
                    let mut buf = Vec::new();
                    resp.into_reader()
                        .read_to_end(&mut buf)
                        .unwrap_or_else(|e| panic!("read body {}: {e}", model.primary_url));
                    buf
                }
                Err(_) => {
                    let mirror = format!("{RELEASE_BASE}/{}", model.cache_name);
                    download(&mirror)
                }
            };
            fs::write(&local, &bytes).ok(); // populate the cache for next time
            bytes
        };
        let got = sha256_hex(&bytes);
        assert_eq!(
            got, model.sha256,
            "SHA256 mismatch for {}: expected {}, got {got}",
            model.cache_name, model.sha256
        );
        // Write to OUT_DIR under the stable name used by lib.rs include_bytes!.
        fs::write(out.join(model.out_name), &bytes).expect("write model to OUT_DIR");
    }
}
