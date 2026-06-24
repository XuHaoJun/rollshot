#![cfg(feature = "ocr")]
//! Real-OCR Smart-Redaction e2e (spec §7). Gated behind `ocr`; fixtures render
//! deterministic text with the vendored DejaVu font (eng-review D8).
//!
//! NOTE: The automation language validator only allows numeric comparisons
//! (no string methods). Tests filter by confidence and verify bounding-box
//! overlap with the expected text region.

use std::time::Duration;

use ab_glyph::FontRef;
use image::Rgba;
use imageproc::drawing::{draw_text_mut, text_size};
use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    OcrQuery, ProposalContext, ProposedEditKind, Region, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{
    EditProposal, ProposalId, ProposedEdit, Provenance, ProvenanceSource,
};
use rollshot_image_document::ImageRect;
use rollshot_vision::{RealAutomationHost, VisualIndex};

const FONT: &[u8] = include_bytes!("../../rollshot-image-document/assets/fonts/DejaVuSans.ttf");

/// White scene with black `text` at (x,y); returns (scene, text box in image coords).
fn scene_with_text(
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    px: f32,
    text: &str,
) -> (image::RgbaImage, ImageRect) {
    let font = FontRef::try_from_slice(FONT).unwrap();
    let mut img = image::RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
    draw_text_mut(&mut img, Rgba([0, 0, 0, 255]), x, y, px, &font, text);
    let (tw, th) = text_size(px, &font, text);
    (
        img,
        ImageRect {
            x: x as f32,
            y: y as f32,
            width: tw as f32,
            height: th as f32,
        },
    )
}

fn overlaps(a: &ImageRect, b: &ImageRect) -> bool {
    a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

/// Redact every OCR match above a confidence threshold, padded.
const REDACT_ALL_JS: &str = r#"
function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 50 });
  return {
    candidates: matches
      .filter((m) => m.confidence > 0)
      .map((m) => ({
        kind: "addRedaction",
        bounds: { x: m.bounds.x - 2, y: m.bounds.y - 2, width: m.bounds.width + 4, height: m.bounds.height + 4 },
        confidence: m.confidence,
        label: "ocr-candidate",
      })),
  };
}
"#;

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
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 1 },
        },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(5),
        32 * 1024 * 1024,
        256 * 1024,
    );
    policy
        .allowed_edit_kinds
        .insert(ProposedEditKind::AddRedaction);

    let index = VisualIndex::build(scene).unwrap();
    let mut host = RealAutomationHost::new();
    host.prepare_ocr(&index, &OcrQuery { region, limit: 50 })
        .unwrap();
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
fn email_detected_and_redacted() {
    let (scene, email_box) = scene_with_text(700, 200, 30, 60, 44.0, "contact@example.com");
    let p = run_ocr(REDACT_ALL_JS, scene, Region::Full);
    let bounds = candidate_bounds(&p);
    assert!(
        !bounds.is_empty(),
        "expected >=1 OCR candidate for email text"
    );
    assert!(
        bounds.iter().any(|b| overlaps(b, &email_box)),
        "no candidate overlaps email text region {email_box:?}; got {bounds:?}"
    );
}

#[test]
fn ssn_like_detected() {
    let (scene, ssn_box) = scene_with_text(700, 200, 30, 60, 44.0, "123-45-6789");
    let p = run_ocr(REDACT_ALL_JS, scene, Region::Full);
    let bounds = candidate_bounds(&p);
    assert!(
        !bounds.is_empty(),
        "expected >=1 OCR candidate for SSN text"
    );
    assert!(
        bounds.iter().any(|b| overlaps(b, &ssn_box)),
        "no candidate overlaps SSN text region {ssn_box:?}; got {bounds:?}"
    );
}

#[test]
fn key_value_detected() {
    let (scene, tok_box) = scene_with_text(800, 200, 30, 60, 40.0, "Token: AKIAEXAMPLEKEY");
    let p = run_ocr(REDACT_ALL_JS, scene, Region::Full);
    let bounds = candidate_bounds(&p);
    assert!(
        !bounds.is_empty(),
        "expected >=1 OCR candidate for key-value text"
    );
    assert!(
        bounds.iter().any(|b| overlaps(b, &tok_box)),
        "no candidate overlaps key-value text region {tok_box:?}; got {bounds:?}"
    );
}

#[test]
fn blank_scene_produces_no_candidates() {
    let scene = image::RgbaImage::from_pixel(400, 120, Rgba([255, 255, 255, 255]));
    let p = run_ocr(REDACT_ALL_JS, scene, Region::Full);
    assert_eq!(
        candidate_bounds(&p).len(),
        0,
        "blank scene should have no OCR candidates"
    );
}

#[test]
fn bounded_region_query_excludes_out_of_region_text() {
    let mut scene = image::RgbaImage::from_pixel(700, 400, Rgba([255, 255, 255, 255]));
    let font = FontRef::try_from_slice(FONT).unwrap();
    draw_text_mut(
        &mut scene,
        Rgba([0, 0, 0, 255]),
        30,
        40,
        40.0,
        &font,
        "inside@example.com",
    );
    draw_text_mut(
        &mut scene,
        Rgba([0, 0, 0, 255]),
        30,
        320,
        40.0,
        &font,
        "outside@example.com",
    );
    let region = Region::Rect {
        bounds: ImageRect {
            x: 0.0,
            y: 0.0,
            width: 700.0,
            height: 160.0,
        },
    };
    let p = run_ocr(REDACT_ALL_JS, scene, region);
    let bounds = candidate_bounds(&p);
    assert!(
        !bounds.is_empty(),
        "expected >=1 candidate for text in top region"
    );
    assert!(
        bounds.iter().all(|b| b.y < 200.0),
        "all candidates should be in top region (y < 200); got {bounds:?}"
    );
}

#[test]
fn no_ocr_text_or_pixels_in_tracing() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Buf {
        type Writer = Buf;
        fn make_writer(&'a self) -> Buf {
            self.clone()
        }
    }

    let buf = Buf(Arc::new(Mutex::new(Vec::new())));
    // Capture only rollshot's own tracing. Every rollshot event uses an explicit
    // `rollshot::*` target (AGENTS.md §7), and rollshot's diagnostics are the only
    // privacy surface this test owns. ONNX Runtime (`ort`) emits verbose INFO logs
    // whose graph-node names contain `@` (e.g. `Reshape@8`, `Constant@92`), which
    // would false-trip the `@` assertion below; that third-party output is filtered
    // out so the test asserts on rollshot's logging, not ort's.
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::level_filters::LevelFilter::OFF.into())
                .parse_lossy("rollshot=trace"),
        )
        .finish();

    let secret = "topsecret@example.com";
    tracing::subscriber::with_default(subscriber, || {
        let (scene, _) = scene_with_text(700, 200, 30, 60, 44.0, secret);
        let _ = run_ocr(REDACT_ALL_JS, scene, Region::Full);
    });

    let captured = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(
        !captured.contains("topsecret") && !captured.contains('@'),
        "OCR text leaked into tracing: {captured}"
    );
}
