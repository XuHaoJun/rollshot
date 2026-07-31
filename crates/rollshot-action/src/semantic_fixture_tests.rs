use std::sync::Arc;

use image::{Rgba, RgbaImage};

use crate::{
    downsample_luma, ActionRecorder, AnalysisFrame, CandidateKind, CaptureRegion, Detector,
    DetectorConfig, MouseButton, SemanticAction, SharedActionFrame, StoreConfig,
    TimedSemanticAction,
};

const W: u32 = 32;
const H: u32 = 24;

fn base() -> SharedActionFrame {
    Arc::new(RgbaImage::from_pixel(W, H, Rgba([24, 24, 24, 255])))
}

fn paint_rect(
    image: SharedActionFrame,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    v: u8,
) -> SharedActionFrame {
    let mut image = image;
    for py in y..(y + h).min(H) {
        for px in x..(x + w).min(W) {
            Arc::make_mut(&mut image).put_pixel(px, py, Rgba([v, v, v, 255]));
        }
    }
    image
}

fn checkbox_checked() -> SharedActionFrame {
    paint_rect(base(), 2, 2, 2, 2, 240)
}

fn popover() -> SharedActionFrame {
    paint_rect(base(), 8, 5, 16, 10, 220)
}

fn typed_text() -> SharedActionFrame {
    paint_rect(paint_rect(base(), 3, 18, 6, 1, 230), 10, 18, 5, 1, 230)
}

fn scrolled(offset: u32) -> SharedActionFrame {
    let mut image = base();
    for row in 0..4 {
        let y = (2 + row * 5 + offset) % H;
        image = paint_rect(image, 2, y, 28, 2, 80 + row as u8 * 35);
    }
    image
}

fn cursor_at(x: u32) -> SharedActionFrame {
    paint_rect(base(), x, 2, 1, 2, 255)
}

fn analysis(id: u64, at_ms: u64, image: &RgbaImage) -> AnalysisFrame {
    AnalysisFrame {
        id,
        at_ms,
        luma: downsample_luma(image, W),
    }
}

fn recorder(detector: DetectorConfig) -> ActionRecorder {
    ActionRecorder::new(
        CaptureRegion {
            x: 0,
            y: 0,
            width: W,
            height: H,
        },
        StoreConfig {
            ring_capacity: 30,
            analysis_capacity: 8,
            analysis_width: W,
            window_before: 2,
            window_after: 2,
            nearby_max: 5,
        },
        detector,
    )
}

fn click(at_ms: u64) -> TimedSemanticAction {
    TimedSemanticAction {
        action: SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        },
        at_ms,
    }
}

// ---- Positive fixtures ----

#[test]
fn fixture_small_checkbox_is_a_click_step_with_checked_keyframe() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(checkbox_checked(), 200);
    rec.ingest_frame(checkbox_checked(), 800);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(2, 2).0[0], 240);
}

#[test]
fn fixture_transient_popover_is_retained_after_it_disappears() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(popover(), 200);
    rec.ingest_frame(base(), 400);
    rec.ingest_frame(base(), 800);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(10, 7).0[0], 220);
}

#[test]
fn fixture_animated_click_prefers_stable_final_state() {
    let mut rec = recorder(DetectorConfig::default());
    let transition = paint_rect(base(), 4, 4, 8, 8, 100);
    let final_state = paint_rect(base(), 4, 4, 8, 8, 240);
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(transition, 200);
    rec.ingest_frame(Arc::clone(&final_state), 300);
    rec.ingest_frame(Arc::clone(&final_state), 400);
    rec.ingest_frame(final_state, 500);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(5, 5).0[0], 240);
    assert_ne!(image.get_pixel(5, 5).0[0], 100);
}

#[test]
fn fixture_scroll_settle_uses_shifted_rows() {
    let before = scrolled(0);
    let after = scrolled(2);
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(Arc::clone(&before), 0);
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::ScrollActivity,
        at_ms: 100,
    });
    rec.ingest_frame(Arc::clone(&after), 200);
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::ScrollActivity,
        at_ms: 250,
    });
    rec.ingest_frame(Arc::clone(&after), 400);
    rec.ingest_frame(after, 900);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Scroll);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(3, 4).0[0], 80);
    assert_ne!(image.get_pixel(3, 2), before.get_pixel(3, 2));
}

#[test]
fn fixture_typing_subtle_text_uses_completed_text() {
    let completed = typed_text();
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::TypingActivity,
        at_ms: 100,
    });
    rec.ingest_frame(completed.clone(), 200);
    rec.ingest_frame(completed, 900);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Typing);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(4, 18).0[0], 230);
    assert_eq!(image.get_pixel(11, 18).0[0], 230);
}

#[test]
fn fixture_stable_visual_navigation_remains_ui_changed() {
    let navigated = paint_rect(base(), 0, 0, W, H, 180);
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_frame(Arc::clone(&navigated), 200);
    rec.ingest_frame(Arc::clone(&navigated), 400);
    rec.ingest_frame(navigated, 600);
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::UiChanged);
    assert!(step.nearby.contains(&step.keyframe));
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    assert_eq!(image.get_pixel(0, 0).0[0], 180);
}

// ---- Negative fixtures ----

#[test]
fn fixture_no_op_click_emits_nothing() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(base(), 800);
    assert!(rec.finish().candidates.is_empty());
}

#[test]
fn fixture_click_noise_below_intensity_floor_emits_nothing() {
    let low_delta = paint_rect(base(), 2, 2, 2, 2, 40);
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(low_delta.clone(), 200);
    rec.ingest_frame(low_delta, 800);
    assert!(rec.finish().candidates.is_empty());
}

#[test]
fn fixture_cursor_only_visual_change_emits_nothing() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_frame(cursor_at(2), 200);
    rec.ingest_frame(cursor_at(3), 400);
    rec.ingest_frame(base(), 600);
    assert!(rec.finish().candidates.is_empty());
}

#[test]
fn fixture_spinner_returning_to_baseline_emits_nothing() {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_frame(popover(), 200);
    rec.ingest_frame(base(), 400);
    rec.ingest_frame(popover(), 600);
    rec.ingest_frame(base(), 800);
    assert!(rec.finish().candidates.is_empty());
}

// ---- Determinism fixtures ----

fn run_checkbox_fixture() -> Vec<crate::CandidateStep> {
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(checkbox_checked(), 200);
    rec.ingest_frame(checkbox_checked(), 800);
    rec.finish().candidates
}

#[test]
fn fixture_replay_is_deterministic() {
    assert_eq!(run_checkbox_fixture(), run_checkbox_fixture());
}

#[test]
fn fixture_observed_peak_survives_analysis_id_gap() {
    let config = DetectorConfig {
        area_threshold: 0.10,
        ..DetectorConfig::default()
    };
    let mut detector = Detector::new(config);
    let base_image = base();
    let popover_image = popover();
    detector.observe_frame(&analysis(0, 0, &base_image));
    detector.observe_event(click(100));
    assert!(detector
        .observe_frame(&analysis(1, 200, &popover_image))
        .is_none());
    let marker = detector
        .observe_frame(&analysis(9, 800, &base_image))
        .expect("observed peak must survive skipped analysis ids");
    assert_eq!(marker.center_id, 1);
}

#[test]
fn fixture_unseen_peak_is_not_invented() {
    let mut detector = Detector::new(DetectorConfig::default());
    let base_image = base();
    detector.observe_frame(&analysis(0, 0, &base_image));
    detector.observe_event(click(100));
    assert!(detector
        .observe_frame(&analysis(9, 800, &base_image))
        .is_none());
    assert!(detector.finish().is_none());
}

// ---- Product-scale fixture ----

#[test]
fn fixture_product_scale_checkbox_survives_1920_to_384_downsampling() {
    use image::{Rgba, RgbaImage};

    let base_img = RgbaImage::from_pixel(1920, 1080, Rgba([24, 24, 24, 255]));
    let mut checkbox = base_img.clone();
    // 24x24 checkbox at (400, 300) — high contrast
    for y in 300..324 {
        for x in 400..424 {
            checkbox.put_pixel(x, y, Rgba([240, 240, 240, 255]));
        }
    }

    let mut rec = ActionRecorder::new(
        CaptureRegion {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        StoreConfig {
            ring_capacity: 30,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 2,
            window_after: 2,
            nearby_max: 5,
        },
        DetectorConfig::default(),
    );

    rec.ingest_frame(Arc::new(base_img), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(Arc::new(checkbox), 200);
    rec.ingest_frame(
        Arc::new(RgbaImage::from_pixel(1920, 1080, Rgba([24, 24, 24, 255]))),
        400,
    );
    rec.ingest_frame(
        Arc::new(RgbaImage::from_pixel(1920, 1080, Rgba([24, 24, 24, 255]))),
        800,
    );
    let recording = rec.finish();

    assert_eq!(recording.candidates.len(), 1);
    let step = &recording.candidates[0];
    assert_eq!(step.kind, CandidateKind::Click);
    let image = &recording.store.retained(step.keyframe).unwrap().image;
    // The keyframe must be the checkbox frame — verify the checkbox pixel is bright
    assert_eq!(image.get_pixel(412, 312).0[0], 240);
}

// ---- Single-candidate ownership fixtures ----

#[test]
fn fixture_animated_click_emits_exactly_one_candidate() {
    let mut rec = recorder(DetectorConfig::default());
    let transition = paint_rect(base(), 4, 4, 8, 8, 100);
    let final_state = paint_rect(base(), 4, 4, 8, 8, 240);
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_frame(Arc::clone(&transition), 200);
    rec.ingest_frame(Arc::clone(&final_state), 300);
    rec.ingest_frame(Arc::clone(&final_state), 400);
    rec.ingest_frame(final_state, 500);
    assert_eq!(rec.finish().candidates.len(), 1);
}

#[test]
fn fixture_click_then_typing_emits_exactly_one_candidate() {
    let completed = typed_text();
    let mut rec = recorder(DetectorConfig::default());
    rec.ingest_frame(base(), 0);
    rec.ingest_event(click(100));
    rec.ingest_event(TimedSemanticAction {
        action: SemanticAction::TypingActivity,
        at_ms: 150,
    });
    rec.ingest_frame(Arc::clone(&completed), 200);
    rec.ingest_frame(completed, 900);
    assert_eq!(rec.finish().candidates.len(), 1);
}
