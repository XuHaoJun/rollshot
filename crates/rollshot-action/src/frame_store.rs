//! Bounded temporary frame storage: a continuously-overwritten full-resolution
//! ring buffer, a latest-useful downsampled analysis queue that drops
//! intermediate work under load (so capture never blocks), and long-lived
//! retained candidate windows copied out of the ring. All bounds are fixed and
//! independent of session length (spec §Fixed Bounds And Capture Rate).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use image::RgbaImage;

/// A reference-counted full-resolution frame shared between the ring buffer
/// and retained candidate windows, avoiding redundant pixel copies.
pub type SharedActionFrame = Arc<RgbaImage>;

use crate::metrics::{downsample_luma, LumaPlane};
use crate::models::{FrameId, Millis};

#[derive(Debug, Clone)]
pub struct StoreConfig {
    /// Full-res rolling window depth (continuously overwritten).
    pub ring_capacity: usize,
    /// Downsampled analysis queue cap; oldest dropped under load.
    pub analysis_capacity: usize,
    /// Target downsample width for analysis luma planes.
    pub analysis_width: u32,
    /// Frames retained before a candidate center.
    pub window_before: usize,
    /// Frames retained after a candidate center.
    pub window_after: usize,
    /// Max frames in a nearby-replacement strip.
    pub nearby_max: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            ring_capacity: 60,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 4,
            window_after: 8,
            nearby_max: 7,
        }
    }
}

#[derive(Debug, Clone)]
struct RingFrame {
    id: FrameId,
    at_ms: Millis,
    image: Arc<RgbaImage>,
}

/// A downsampled luma frame queued for the detector.
#[derive(Debug, Clone)]
pub struct AnalysisFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub luma: LumaPlane,
}

/// A full-resolution frame retained around a candidate window.
#[derive(Debug, Clone)]
pub struct RetainedFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub image: Arc<RgbaImage>,
}

pub struct FrameStore {
    config: StoreConfig,
    ring: VecDeque<RingFrame>,
    analysis: VecDeque<AnalysisFrame>,
    retained: BTreeMap<FrameId, RetainedFrame>,
    dropped: u64,
    next_id: FrameId,
}

impl FrameStore {
    pub fn new(config: StoreConfig) -> Self {
        // The ring must hold a full candidate window long enough for the
        // recorder to retain it `window_after` frames after detection; a smaller
        // ring silently drops steps (see `ActionRecorder::finalize`).
        debug_assert!(
            config.ring_capacity > config.window_before + config.window_after,
            "ring_capacity ({}) must exceed window_before + window_after ({})",
            config.ring_capacity,
            config.window_before + config.window_after,
        );
        Self {
            config,
            ring: VecDeque::new(),
            analysis: VecDeque::new(),
            retained: BTreeMap::new(),
            dropped: 0,
            next_id: 0,
        }
    }

    /// Push a cropped full-res frame. Stores it in the ring and enqueues a
    /// downsampled analysis frame. Never blocks: if the analysis queue is full,
    /// the oldest queued frame is dropped (latest-useful). Returns the frame id.
    pub fn ingest(&mut self, image: SharedActionFrame, at_ms: Millis) -> FrameId {
        let id = self.next_id;
        self.next_id += 1;
        let luma = downsample_luma(image.as_ref(), self.config.analysis_width);

        self.ring.push_back(RingFrame { id, at_ms, image });
        if self.ring.len() > self.config.ring_capacity {
            self.ring.pop_front();
        }

        self.analysis.push_back(AnalysisFrame { id, at_ms, luma });
        if self.analysis.len() > self.config.analysis_capacity {
            self.analysis.pop_front();
            self.dropped += 1;
        }
        id
    }

    /// Pop the oldest queued analysis frame for the detector, if any.
    pub fn take_analysis(&mut self) -> Option<AnalysisFrame> {
        self.analysis.pop_front()
    }

    /// Count of analysis frames dropped under load (for diagnostics).
    pub fn dropped_analysis(&self) -> u64 {
        self.dropped
    }

    /// Copy `[center - window_before, center + window_after]` (clamped to what
    /// is currently in the ring) into long-lived retained storage. Returns the
    /// retained ids in time order. Empty if the center has already rolled out
    /// of the ring.
    pub fn retain_window(&mut self, center_id: FrameId) -> Vec<FrameId> {
        let Some(idx) = self.ring.iter().position(|f| f.id == center_id) else {
            return Vec::new();
        };
        let lo = idx.saturating_sub(self.config.window_before);
        let hi = (idx + self.config.window_after).min(self.ring.len() - 1);
        let mut ids = Vec::new();
        for f in self.ring.iter().take(hi + 1).skip(lo) {
            self.retained.entry(f.id).or_insert_with(|| RetainedFrame {
                id: f.id,
                at_ms: f.at_ms,
                image: Arc::clone(&f.image),
            });
            ids.push(f.id);
        }
        ids
    }

    /// Look up a retained frame by id.
    pub fn retained(&self, id: FrameId) -> Option<&RetainedFrame> {
        self.retained.get(&id)
    }

    pub fn retained_shared(&self, id: FrameId) -> Option<(Millis, Arc<RgbaImage>)> {
        let rf = self.retained.get(&id)?;
        Some((rf.at_ms, Arc::clone(&rf.image)))
    }

    pub(crate) fn ring_bounds(&self) -> Option<(FrameId, FrameId)> {
        Some((self.ring.front()?.id, self.ring.back()?.id))
    }

    #[cfg(test)]
    pub fn retained_ids_for_test(&self) -> Vec<crate::models::FrameId> {
        self.retained.keys().copied().collect()
    }

    /// A bounded, time-ordered subset of `window` (size <= `nearby_max`)
    /// centered on `keyframe`, for the replacement strip.
    pub fn nearby(&self, window: &[FrameId], keyframe: FrameId) -> Vec<FrameId> {
        if window.is_empty() {
            return Vec::new();
        }
        let max = self.config.nearby_max.max(1);
        if window.len() <= max {
            return window.to_vec();
        }
        let idx = window.iter().position(|&f| f == keyframe).unwrap_or(0);
        let half = max / 2;
        let mut lo = idx.saturating_sub(half);
        if lo + max > window.len() {
            lo = window.len() - max;
        }
        window[lo..lo + max].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    fn frame(v: u8) -> SharedActionFrame {
        Arc::new(RgbaImage::from_pixel(8, 8, Rgba([v, v, v, 255])))
    }

    fn small_store() -> FrameStore {
        FrameStore::new(StoreConfig {
            ring_capacity: 10,
            analysis_capacity: 4,
            analysis_width: 384,
            window_before: 2,
            window_after: 3,
            nearby_max: 3,
        })
    }

    #[test]
    fn ingest_preserves_shared_frame_allocation() {
        let mut store = small_store();
        let frame = Arc::new(RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])));
        let weak = Arc::downgrade(&frame);
        store.ingest(Arc::clone(&frame), 0);
        drop(frame);
        let kept = weak.upgrade().expect("ring must keep the Arc alive");
        assert!(Arc::ptr_eq(&kept, &store.ring.back().unwrap().image));
    }

    #[test]
    fn ring_buffer_is_bounded_and_overwrites_oldest() {
        let mut store = small_store();
        for i in 0..15u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        assert_eq!(store.ring.len(), 10);
        // Oldest retained ring frame is id 5 (15 ingested, capacity 10).
        assert_eq!(store.ring.front().unwrap().id, 5);
        assert_eq!(store.ring.back().unwrap().id, 14);
    }

    #[test]
    fn analysis_queue_drops_intermediate_under_load_without_blocking_capture() {
        let mut store = small_store();
        // Ingest far more than analysis_capacity WITHOUT draining (slow detector).
        for i in 0..20u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        // Queue stays bounded; intermediate analysis work was dropped.
        assert_eq!(store.analysis.len(), 4);
        assert_eq!(store.dropped_analysis(), 16);
        // Latest-useful: the newest frame is still queued for the detector.
        assert_eq!(store.analysis.back().unwrap().id, 19);
    }

    #[test]
    fn take_analysis_returns_oldest_queued_then_none() {
        let mut store = small_store();
        store.ingest(frame(1), 100);
        store.ingest(frame(2), 200);
        assert_eq!(store.take_analysis().unwrap().id, 0);
        assert_eq!(store.take_analysis().unwrap().id, 1);
        assert!(store.take_analysis().is_none());
    }

    #[test]
    fn retain_window_copies_before_and_after_and_survives_ring_eviction() {
        let mut store = small_store();
        for i in 0..8u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        // center=4: window_before 2 -> {2,3}, center 4, window_after 3 -> {5,6,7}.
        let ids = store.retain_window(4);
        assert_eq!(ids, vec![2, 3, 4, 5, 6, 7]);
        assert!(store.retained(2).is_some());
        assert!(store.retained(0).is_none());
        // Evict the ring well past those ids; retained frames persist.
        for i in 8..30u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        assert!(store.ring.iter().all(|f| f.id != 2));
        assert!(
            store.retained(2).is_some(),
            "retained window must outlive the ring"
        );
        assert_eq!(store.retained(4).unwrap().at_ms, 400);
    }

    #[test]
    fn nearby_is_bounded_and_ordered_and_centered_on_keyframe() {
        let store = small_store();
        let window = vec![2u64, 3, 4, 5, 6, 7];
        // nearby_max = 3, keyframe 4 -> centered window [3,4,5].
        assert_eq!(store.nearby(&window, 4), vec![3, 4, 5]);
        // Small windows are returned whole.
        assert_eq!(store.nearby(&[9, 10], 9), vec![9, 10]);
    }

    #[test]
    fn ring_bounds_report_oldest_and_newest_without_exposing_pixels() {
        let mut store = small_store();
        assert_eq!(store.ring_bounds(), None);
        for i in 0..4u64 {
            store.ingest(frame(i as u8), i * 100);
        }
        assert_eq!(store.ring_bounds(), Some((0, 3)));
    }

    #[test]
    fn default_ring_covers_product_click_window_and_replacement_context_at_five_fps() {
        let store = StoreConfig::default();
        let click_frames = 600u64.div_ceil(200) as usize;
        let required = store.window_before + click_frames + store.window_after + 1;
        assert!(store.ring_capacity >= required);
    }

    #[test]
    fn retained_window_shares_ring_pixels() {
        let mut store = small_store();
        let id = store.ingest(Arc::new(RgbaImage::new(4, 4)), 0);
        store.retain_window(id);

        let ring = &store.ring.back().unwrap().image;
        let retained = &store.retained(id).unwrap().image;
        assert!(Arc::ptr_eq(ring, retained));
    }
}
