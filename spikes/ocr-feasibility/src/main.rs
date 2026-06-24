//! OCR feasibility spike — Stage 2 (accuracy + latency) and Stage 3 (host
//! callback < 1 ms), mirroring the production prepare-outside-QuickJS /
//! cached-lookup-inside pattern in `rollshot-vision::RealAutomationHost`.
//!
//! Stage 1 (compile/link/isolation) is implied: if the session builds and
//! models load, `ort` + `paddle_ocr_rs` init succeeds at MSRV 1.94 with unsafe
//! confined to this `unsafe_code = "allow"` crate.
//!
//! Models: ./models/{ch_PP-OCRv4_det_infer.onnx,
//! ch_ppocr_mobile_v2.0_cls_infer.onnx, ch_PP-OCRv4_rec_infer.onnx}
//! Fixtures: ./fixtures/*.png (paddle-ocr-rs shipped test images).

use std::path::PathBuf;
use std::time::Instant;

use paddle_ocr_rs::ocr_lite::OcrLite;
use paddle_ocr_rs::ocr_result::TextBlock;

/// Local mirror of `rollshot_automation::OcrMatch` (bounds + text + confidence)
/// to prove the paddle output maps cleanly into the product capability shape.
#[derive(Debug, Clone)]
struct OcrMatch {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: String,
    confidence: f32,
}

/// Local mirror of the bounded query (`OcrQuery { region, limit }`).
enum Region {
    Full,
    Rect { x: f32, y: f32, w: f32, h: f32 },
}

/// Axis-aligned bounds from a paddle `TextBlock`'s 4-point quad.
fn aabb(b: &TextBlock) -> (f32, f32, f32, f32) {
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in &b.box_points {
        min_x = min_x.min(p.x as f32);
        min_y = min_y.min(p.y as f32);
        max_x = max_x.max(p.x as f32);
        max_y = max_y.max(p.y as f32);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Mirrors `RealAutomationHost::prepare_*` (expensive, outside QuickJS):
/// run OCR once and cache the mapped candidates.
fn prepare_ocr(ocr: &mut OcrLite, img: &image::RgbImage) -> Vec<OcrMatch> {
    let res = ocr
        .detect(img, 50, 1024, 0.5, 0.3, 1.6, false, false)
        .expect("detect");
    res.text_blocks
        .iter()
        .map(|b| {
            let (x, y, w, h) = aabb(b);
            OcrMatch {
                x,
                y,
                w,
                h,
                text: b.text.clone(),
                confidence: b.text_score,
            }
        })
        .collect()
}

/// Mirrors the QuickJS callback (cheap, inside QuickJS): cached lookup by
/// region + truncate to `limit`. Must stay well under 1 ms.
fn cached_ocr_callback(cached: &[OcrMatch], region: &Region, limit: u32) -> Vec<OcrMatch> {
    let mut out: Vec<OcrMatch> = match region {
        Region::Full => cached.iter().cloned().collect(),
        Region::Rect { x, y, w, h } => cached
            .iter()
            .filter(|m| {
                // axis-aligned intersection test
                m.x < *x + *w && m.x + m.w > *x && m.y < *y + *h && m.y + m.h > *y
            })
            .cloned()
            .collect(),
    };
    out.truncate(limit as usize);
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let models = PathBuf::from("models");
    let det = models.join("ch_PP-OCRv4_det_infer.onnx");
    let cls = models.join("ch_ppocr_mobile_v2.0_cls_infer.onnx");
    let rec = models.join("ch_PP-OCRv4_rec_infer.onnx");
    for p in [&det, &cls, &rec] {
        assert!(p.exists(), "missing model: {}", p.display());
    }

    let mut ocr = OcrLite::new();
    let t0 = Instant::now();
    ocr.init_models(
        det.to_str().unwrap(),
        cls.to_str().unwrap(),
        rec.to_str().unwrap(),
        num_cpus::get_physical(),
    )?;
    let init_ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("stage1: session init {} ms (threads={})", init_ms, num_cpus::get_physical());

    // ---- Stage 2: accuracy + latency on each fixture ----
    let fixtures: Vec<PathBuf> = std::fs::read_dir("fixtures")?
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension().and_then(|e| e.to_str()) == Some("png")).then_some(p)
        })
        .collect();

    let mut all_blocks = 0usize;
    let mut any_valid = false;
    for fix in &fixtures {
        let img = image::open(fix)?.to_rgb8();
        // cold run on a fresh session (proves repeatable init + first-inference cost)
        let t = Instant::now();
        let mut ocr2 = OcrLite::new();
        ocr2.init_models(det.to_str().unwrap(), cls.to_str().unwrap(), rec.to_str().unwrap(), num_cpus::get_physical())?;
        let _ = ocr2.detect(&img, 50, 1024, 0.5, 0.3, 1.6, false, false)?;
        let cold_ms = t.elapsed().as_secs_f64() * 1000.0;
        // warm run on the persistent session (the production pattern)
        let t = Instant::now();
        let cached = prepare_ocr(&mut ocr, &img);
        let warm_ms = t.elapsed().as_secs_f64() * 1000.0;

        let blocks = cached.len();
        all_blocks += blocks;
        let valid = cached.iter().all(|m| {
            m.w.is_finite() && m.h.is_finite()
                && m.w > 0.0 && m.h > 0.0
                && m.confidence >= 0.0 && m.confidence <= 1.0
        });
        any_valid |= valid && blocks > 0;
        println!(
            "stage2: {} -> blocks={} cold={:.1}ms warm={:.1}ms valid_shape={}",
            fix.file_name().unwrap().to_string_lossy(),
            blocks,
            cold_ms,
            warm_ms,
            valid
        );
        for m in cached.iter().take(6) {
            println!(
                "    [{:.0},{:.0} {:.0}x{:.0}] conf={:.3} \"{}\"",
                m.x, m.y, m.w, m.h, m.confidence, m.text.trim()
            );
        }
    }
    println!("stage2: total_blocks={} any_valid={}", all_blocks, any_valid);
    assert!(any_valid, "no fixture produced valid OcrMatch-shaped output");

    // ---- Stage 3: host callback < 1 ms (cached lookup + truncate) ----
    // Simulate a large cached result set (up to 200 entries) and measure the
    // callback path that would run inside QuickJS.
    let big: Vec<OcrMatch> = (0..200)
        .map(|i| OcrMatch {
            x: (i % 20) as f32 * 50.0,
            y: (i / 20) as f32 * 30.0,
            w: 40.0,
            h: 20.0,
            text: format!("row{i}"),
            confidence: 0.9,
        })
        .collect();
    const ITERS: usize = 10_000;
    let t = Instant::now();
    for _ in 0..ITERS {
        let _ = cached_ocr_callback(&big, &Region::Full, 100);
        let _ = cached_ocr_callback(
            &big,
            &Region::Rect { x: 100.0, y: 100.0, w: 500.0, h: 500.0 },
            100,
        );
    }
    let per_call_ns = t.elapsed().as_nanos() as f64 / (ITERS as f64 * 2.0);
    println!(
        "stage3: cached callback {:.0} ns/call (200-entry cache, Full + Rect, limit 100) -> {:.4} ms/call",
        per_call_ns,
        per_call_ns / 1_000_000.0
    );
    assert!(per_call_ns < 1_000_000.0, "callback exceeded 1 ms");

    Ok(())
}
