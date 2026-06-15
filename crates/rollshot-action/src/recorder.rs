//! Orchestrates a recording: pushes cropped frames into the bounded store,
//! drives the detector, and resolves detector markers into `CandidateStep`s
//! once enough after-frames exist to retain a stable window. The producer never
//! blocks — the store absorbs and bounds bursts (see `FrameStore`). `finish`
//! returns the candidates plus the store so export can read keyframe pixels.

use image::RgbaImage;

use crate::detector::{CandidateMarker, Detector, DetectorConfig};
use crate::diagnostics::TARGET_ACTION;
use crate::frame_store::{FrameStore, StoreConfig};
use crate::models::{CandidateId, CandidateStep, CaptureRegion, Millis, TimedSemanticAction};

/// Output of a finished recording: detected candidates and the frame store that
/// still holds their retained keyframe/nearby pixels.
pub struct Recording {
    pub candidates: Vec<CandidateStep>,
    pub store: FrameStore,
}

struct Pending {
    marker: CandidateMarker,
    resolve_at: u64,
}

pub struct ActionRecorder {
    #[allow(dead_code)] // surfaced in session.json by the app (Plan 2)
    region: CaptureRegion,
    store: FrameStore,
    detector: Detector,
    window_after: u64,
    frame_count: u64,
    pending: Vec<Pending>,
    candidates: Vec<CandidateStep>,
    next_candidate_id: CandidateId,
}

impl ActionRecorder {
    pub fn new(region: CaptureRegion, store: StoreConfig, det: DetectorConfig) -> Self {
        let window_after = store.window_after as u64;
        Self {
            region,
            store: FrameStore::new(store),
            detector: Detector::new(det),
            window_after,
            frame_count: 0,
            pending: Vec::new(),
            candidates: Vec::new(),
            next_candidate_id: 0,
        }
    }

    /// Push one cropped full-resolution frame. Always returns immediately.
    pub fn ingest_frame(&mut self, image: RgbaImage, at_ms: Millis) {
        self.store.ingest(image, at_ms);
        self.frame_count += 1;
        while let Some(frame) = self.store.take_analysis() {
            if let Some(marker) = self.detector.observe_frame(&frame) {
                self.pending.push(Pending {
                    marker,
                    resolve_at: self.frame_count + self.window_after,
                });
            }
        }
        self.resolve_ready();
    }

    /// Feed a privacy-filtered semantic event to the detector. (P0a never calls
    /// this — `VisualOnlySource` produces none — but P0b wires real events here.)
    pub fn ingest_event(&mut self, ev: TimedSemanticAction) {
        self.detector.observe_event(ev);
    }

    pub fn dropped_analysis(&self) -> u64 {
        self.store.dropped_analysis()
    }

    pub fn finish(mut self) -> Recording {
        while let Some(frame) = self.store.take_analysis() {
            if let Some(marker) = self.detector.observe_frame(&frame) {
                self.pending.push(Pending {
                    marker,
                    resolve_at: self.frame_count,
                });
            }
        }
        if let Some(marker) = self.detector.finish() {
            self.pending.push(Pending {
                marker,
                resolve_at: self.frame_count,
            });
        }
        for p in std::mem::take(&mut self.pending) {
            self.finalize(p.marker);
        }
        Recording {
            candidates: self.candidates,
            store: self.store,
        }
    }

    fn resolve_ready(&mut self) {
        let now = self.frame_count;
        let mut still = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            if p.resolve_at <= now {
                self.finalize(p.marker);
            } else {
                still.push(p);
            }
        }
        self.pending = still;
    }

    fn finalize(&mut self, marker: CandidateMarker) {
        let window = self.store.retain_window(marker.center_id);
        if window.is_empty() {
            tracing::debug!(
                target: TARGET_ACTION,
                center = marker.center_id,
                "candidate window unavailable; dropping (bounded loss)"
            );
            return;
        }
        let keyframe = if window.contains(&marker.center_id) {
            marker.center_id
        } else {
            *window.last().expect("non-empty window")
        };
        let nearby = self.store.nearby(&window, keyframe);
        let id = self.next_candidate_id;
        self.next_candidate_id += 1;
        self.candidates.push(CandidateStep {
            id,
            kind: marker.kind,
            reason: marker.reason,
            at_ms: marker.at_ms,
            keyframe,
            nearby,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::StoreConfig;
    use crate::models::{CandidateKind, CaptureRegion};
    use image::{Rgba, RgbaImage};

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }
    }
    fn black() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))
    }
    fn quadrant() -> RgbaImage {
        let mut img = black();
        for y in 0..4 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }
    fn cfg() -> DetectorConfig {
        DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            per_sample_threshold: 12.0,
            cooldown_ms: 0,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        }
    }
    fn store_cfg() -> StoreConfig {
        StoreConfig {
            ring_capacity: 30,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 2,
            window_after: 2,
            nearby_max: 3,
        }
    }

    #[test]
    fn visual_only_recording_produces_one_deterministic_step_with_retained_keyframe() {
        let mut rec = ActionRecorder::new(region(), store_cfg(), cfg());
        let frames = [
            black(),
            quadrant(),
            quadrant(),
            quadrant(),
            quadrant(),
            quadrant(),
            quadrant(),
        ];
        for (i, f) in frames.into_iter().enumerate() {
            rec.ingest_frame(f, i as u64 * 100);
        }
        let recording = rec.finish();
        assert_eq!(recording.candidates.len(), 1);
        let step = &recording.candidates[0];
        assert_eq!(step.kind, CandidateKind::UiChanged);
        assert!(!step.nearby.is_empty() && step.nearby.len() <= 3);
        assert!(
            step.nearby.windows(2).all(|w| w[0] < w[1]),
            "nearby is time-ordered"
        );
        assert!(step.nearby.contains(&step.keyframe));
        assert!(recording.store.retained(step.keyframe).is_some());
    }

    #[test]
    fn ingest_never_blocks_and_every_keyframe_survives_a_burst() {
        let mut rec = ActionRecorder::new(region(), store_cfg(), cfg());
        rec.ingest_frame(black(), 0);
        for i in 1..40u64 {
            rec.ingest_frame(quadrant(), i * 100);
        }
        let recording = rec.finish();
        assert!(!recording.candidates.is_empty());
        for step in &recording.candidates {
            assert!(
                recording.store.retained(step.keyframe).is_some(),
                "every step keyframe must be retained for export"
            );
        }
    }
}
