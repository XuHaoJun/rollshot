use rollshot_image_document::pixelate::{PixelateError, RasterRegion};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewKey {
    pub source_id: usize,
    pub region: RasterRegion,
    pub block_size: u32,
    pub display_scale_bits: u32,
}

impl PreviewKey {
    pub fn new(
        source_id: usize,
        region: RasterRegion,
        block_size: u32,
        display_scale: f32,
    ) -> Self {
        debug_assert!(
            display_scale.is_finite() && display_scale > 0.0,
            "display_scale must be finite and positive, got {display_scale}"
        );
        Self {
            source_id,
            region,
            block_size,
            display_scale_bits: display_scale.to_bits(),
        }
    }
}

impl Hash for PreviewKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.source_id.hash(state);
        self.region.x.hash(state);
        self.region.y.hash(state);
        self.region.width.hash(state);
        self.region.height.hash(state);
        self.block_size.hash(state);
        self.display_scale_bits.hash(state);
    }
}

#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub key: PreviewKey,
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct PreviewPixels {
    pub request: PreviewRequest,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewGenerationError {
    Kernel(PixelateError),
    WorkerFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    Accepted,
    Stale,
}

struct CacheEntry {
    generation: u64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    last_used: u64,
}

pub struct PixelatePreviewCache {
    byte_limit: usize,
    entries: HashMap<PreviewKey, CacheEntry>,
    in_flight: HashMap<PreviewKey, u64>,
    retained_bytes: usize,
    clock: u64,
    generation_counter: u64,
    failure_counts: HashMap<PreviewKey, u32>,
}

impl PixelatePreviewCache {
    pub fn new(byte_limit: usize) -> Self {
        Self {
            byte_limit,
            entries: HashMap::new(),
            in_flight: HashMap::new(),
            retained_bytes: 0,
            clock: 0,
            generation_counter: 0,
            failure_counts: HashMap::new(),
        }
    }

    pub fn lookup(&mut self, key: PreviewKey) -> Option<(u32, u32, &[u8])> {
        if let Some(entry) = self.entries.get_mut(&key) {
            self.clock += 1;
            entry.last_used = self.clock;
            Some((entry.width, entry.height, &entry.rgba))
        } else {
            None
        }
    }

    pub fn begin_request(&mut self, key: PreviewKey) -> Option<PreviewRequest> {
        if self.in_flight.contains_key(&key) {
            return None;
        }
        self.generation_counter += 1;
        let generation = self.generation_counter;
        self.in_flight.insert(key, generation);
        Some(PreviewRequest { key, generation })
    }

    pub fn complete(&mut self, pixels: PreviewPixels) -> Completion {
        let key = pixels.request.key;
        let generation = pixels.request.generation;
        let expected = match self.in_flight.get(&key) {
            Some(&g) => g,
            None => {
                tracing::trace!(target: "rollshot::cache", cache_outcome = "stale", "no in-flight entry");
                return Completion::Stale;
            }
        };
        if generation != expected {
            tracing::trace!(target: "rollshot::cache", cache_outcome = "stale", "generation mismatch");
            return Completion::Stale;
        }
        self.in_flight.remove(&key);
        let byte_len = match checked_byte_len(pixels.width, pixels.height) {
            Some(b) => b,
            None => return Completion::Stale,
        };
        self.evict_to_fit(byte_len, &key);
        self.clock += 1;
        self.retained_bytes += byte_len;
        self.entries.insert(
            key,
            CacheEntry {
                generation,
                width: pixels.width,
                height: pixels.height,
                rgba: pixels.rgba,
                last_used: self.clock,
            },
        );
        Completion::Accepted
    }

    pub fn fail(&mut self, request: PreviewRequest) -> bool {
        let key = request.key;
        let generation = request.generation;
        let expected = match self.in_flight.get(&key) {
            Some(&g) => g,
            None => return false,
        };
        if generation != expected {
            return false;
        }
        self.in_flight.remove(&key);
        let count = self.failure_counts.entry(key).or_insert(0);
        *count += 1;
        *count == 1
    }

    pub fn retain_requested(&mut self, requested: &[PreviewKey]) {
        let requested_set: std::collections::HashSet<PreviewKey> =
            requested.iter().copied().collect();
        let mut to_remove = Vec::new();
        for (&key, entry) in &self.entries {
            if !requested_set.contains(&key) {
                to_remove.push((key, entry.last_used));
            }
        }
        to_remove.sort_by_key(|&(_, last_used)| last_used);
        for (key, _) in to_remove {
            if let Some(entry) = self.entries.remove(&key) {
                let bytes = checked_byte_len(entry.width, entry.height).unwrap_or(0);
                self.retained_bytes = self.retained_bytes.saturating_sub(bytes);
            }
        }
        if self.retained_bytes > self.byte_limit {
            let mut remaining: Vec<(PreviewKey, u64)> = self
                .entries
                .iter()
                .filter(|(k, _)| !requested_set.contains(k))
                .map(|(&k, v)| (k, v.last_used))
                .collect();
            remaining.sort_by_key(|&(_, last_used)| last_used);
            for (key, _) in remaining {
                if self.retained_bytes <= self.byte_limit {
                    break;
                }
                if let Some(entry) = self.entries.remove(&key) {
                    let bytes = checked_byte_len(entry.width, entry.height).unwrap_or(0);
                    self.retained_bytes = self.retained_bytes.saturating_sub(bytes);
                }
            }
        }
    }

    pub fn invalidate_key(&mut self, key: PreviewKey) {
        self.in_flight.remove(&key);
        self.generation_counter += 1;
        self.failure_counts.remove(&key);
    }

    pub fn clear_for_source(&mut self, source_id: usize) {
        self.entries.retain(|k, _| k.source_id != source_id);
        self.in_flight.retain(|k, _| k.source_id != source_id);
        self.failure_counts.retain(|k, _| k.source_id != source_id);
        self.recalculate_retained_bytes();
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn is_in_flight(&self, key: PreviewKey) -> bool {
        self.in_flight.contains_key(&key)
    }

    fn evict_to_fit(&mut self, incoming_bytes: usize, requested_key: &PreviewKey) {
        while self.retained_bytes + incoming_bytes > self.byte_limit {
            let victim = self
                .entries
                .iter()
                .filter(|(&k, _)| k != *requested_key)
                .min_by_key(|(_, v)| v.last_used)
                .map(|(&k, _)| k);
            match victim {
                Some(key) => {
                    if let Some(entry) = self.entries.remove(&key) {
                        let bytes = checked_byte_len(entry.width, entry.height).unwrap_or(0);
                        self.retained_bytes = self.retained_bytes.saturating_sub(bytes);
                    }
                }
                None => break,
            }
        }
    }

    fn recalculate_retained_bytes(&mut self) {
        self.retained_bytes = self
            .entries
            .values()
            .map(|e| checked_byte_len(e.width, e.height).unwrap_or(0))
            .sum();
    }
}

fn checked_byte_len(width: u32, height: u32) -> Option<usize> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    pixels.checked_mul(4)
}

pub fn source_id_from_arc<T>(arc: &Arc<T>) -> usize {
    Arc::as_ptr(arc) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: u32, y: u32, w: u32, h: u32) -> RasterRegion {
        RasterRegion {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn key(
        source_id: usize,
        region: RasterRegion,
        block_size: u32,
        display_scale: f32,
    ) -> PreviewKey {
        PreviewKey::new(source_id, region, block_size, display_scale)
    }

    fn pixels_for(request: PreviewRequest, width: u32, height: u32) -> PreviewPixels {
        PreviewPixels {
            request,
            width,
            height,
            rgba: vec![0u8; (width as usize) * (height as usize) * 4],
        }
    }

    // -- Step 1: Key/in-flight/stale tests ------------------------------------

    #[test]
    fn lookup_returns_none_for_missing_key() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        assert!(cache.lookup(k).is_none());
    }

    #[test]
    fn completion_requires_exact_key_and_generation() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        let request = cache.begin_request(k).unwrap();
        cache.invalidate_key(k);
        assert_eq!(cache.complete(pixels_for(request, 8, 8)), Completion::Stale);
        assert!(cache.lookup(k).is_none());
    }

    #[test]
    fn one_key_has_at_most_one_in_flight_generation() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        assert!(cache.begin_request(k).is_some());
        assert!(cache.begin_request(k).is_none());
    }

    #[test]
    fn different_source_ids_are_different_keys() {
        let mut cache = PixelatePreviewCache::new(1024);
        let r = region(0, 0, 8, 8);
        let k1 = key(1, r, 16, 1.0);
        let k2 = key(2, r, 16, 1.0);
        let req1 = cache.begin_request(k1).unwrap();
        let req2 = cache.begin_request(k2).unwrap();
        assert_eq!(cache.complete(pixels_for(req1, 8, 8)), Completion::Accepted);
        assert_eq!(cache.complete(pixels_for(req2, 8, 8)), Completion::Accepted);
        assert!(cache.lookup(k1).is_some());
        assert!(cache.lookup(k2).is_some());
    }

    #[test]
    fn different_regions_are_different_keys() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k1 = key(1, region(0, 0, 8, 8), 16, 1.0);
        let k2 = key(1, region(0, 0, 16, 16), 16, 1.0);
        assert!(cache.begin_request(k1).is_some());
        assert!(cache.begin_request(k2).is_some());
    }

    #[test]
    fn different_block_sizes_are_different_keys() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k1 = key(1, region(0, 0, 8, 8), 16, 1.0);
        let k2 = key(1, region(0, 0, 8, 8), 32, 1.0);
        assert!(cache.begin_request(k1).is_some());
        assert!(cache.begin_request(k2).is_some());
    }

    #[test]
    fn different_display_scales_are_different_keys() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k1 = key(1, region(0, 0, 8, 8), 16, 1.0);
        let k2 = key(1, region(0, 0, 8, 8), 16, 2.0);
        assert!(cache.begin_request(k1).is_some());
        assert!(cache.begin_request(k2).is_some());
    }

    #[test]
    fn accepted_completion_populates_lookup() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 4, 4), 16, 1.0);
        let request = cache.begin_request(k).unwrap();
        assert_eq!(
            cache.complete(pixels_for(request, 4, 4)),
            Completion::Accepted
        );
        let (w, h, data) = cache.lookup(k).unwrap();
        assert_eq!(w, 4);
        assert_eq!(h, 4);
        assert_eq!(data.len(), 64);
    }

    #[test]
    fn stale_completion_after_invalidation_is_rejected() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        let request = cache.begin_request(k).unwrap();
        cache.clear_for_source(1);
        assert_eq!(cache.complete(pixels_for(request, 8, 8)), Completion::Stale);
        assert!(cache.lookup(k).is_none());
    }

    #[test]
    fn generation_increments_after_invalidation_and_new_request() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        let req1 = cache.begin_request(k).unwrap();
        cache.clear_for_source(1);
        let req2 = cache.begin_request(k).unwrap();
        assert!(req2.generation > req1.generation);
        assert_eq!(cache.complete(pixels_for(req1, 8, 8)), Completion::Stale);
        assert_eq!(cache.complete(pixels_for(req2, 8, 8)), Completion::Accepted);
    }

    #[test]
    fn clear_for_source_removes_all_entries() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k1 = key(1, region(0, 0, 8, 8), 16, 1.0);
        let k2 = key(1, region(8, 8, 4, 4), 16, 1.0);
        let req1 = cache.begin_request(k1).unwrap();
        let req2 = cache.begin_request(k2).unwrap();
        assert_eq!(cache.complete(pixels_for(req1, 8, 8)), Completion::Accepted);
        assert_eq!(cache.complete(pixels_for(req2, 4, 4)), Completion::Accepted);
        cache.clear_for_source(1);
        assert!(cache.lookup(k1).is_none());
        assert!(cache.lookup(k2).is_none());
    }

    #[test]
    fn is_in_flight_tracks_pending_requests() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        assert!(!cache.is_in_flight(k));
        let request = cache.begin_request(k).unwrap();
        assert!(cache.is_in_flight(k));
        assert_eq!(
            cache.complete(pixels_for(request, 8, 8)),
            Completion::Accepted
        );
        assert!(!cache.is_in_flight(k));
    }

    #[test]
    fn scroll_only_reuses_same_key() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        let request = cache.begin_request(k).unwrap();
        assert_eq!(
            cache.complete(pixels_for(request, 8, 8)),
            Completion::Accepted
        );
        assert!(cache.lookup(k).is_some());
        assert!(cache.lookup(k).is_some());
    }

    // -- Step 3: LRU/oversized/failure tests ----------------------------------

    #[test]
    fn lru_evicts_least_recently_used_on_pressure() {
        let mut cache = PixelatePreviewCache::new(80);
        let k1 = key(1, region(0, 0, 4, 4), 16, 1.0); // 64 bytes
        let k2 = key(1, region(4, 4, 4, 4), 16, 1.0); // 64 bytes
        let req1 = cache.begin_request(k1).unwrap();
        assert_eq!(cache.complete(pixels_for(req1, 4, 4)), Completion::Accepted);
        assert_eq!(cache.retained_bytes(), 64);
        cache.lookup(k1);
        let req2 = cache.begin_request(k2).unwrap();
        assert_eq!(cache.complete(pixels_for(req2, 4, 4)), Completion::Accepted);
        assert_eq!(cache.retained_bytes(), 64);
        assert!(cache.lookup(k1).is_none());
        assert!(cache.lookup(k2).is_some());
    }

    #[test]
    fn oversized_entry_allowed_when_requested() {
        let mut cache = PixelatePreviewCache::new(64);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0); // 256 bytes > 64 limit
        let request = cache.begin_request(k).unwrap();
        assert_eq!(
            cache.complete(pixels_for(request, 8, 8)),
            Completion::Accepted
        );
        assert_eq!(cache.retained_bytes(), 256);
        assert!(cache.lookup(k).is_some());
    }

    #[test]
    fn oversized_entry_evicted_after_retain_requested_with_empty() {
        let mut cache = PixelatePreviewCache::new(64);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0); // 256 bytes > 64 limit
        let request = cache.begin_request(k).unwrap();
        assert_eq!(
            cache.complete(pixels_for(request, 8, 8)),
            Completion::Accepted
        );
        assert_eq!(cache.retained_bytes(), 256);
        cache.retain_requested(&[]);
        assert!(cache.lookup(k).is_none());
        assert_eq!(cache.retained_bytes(), 0);
    }

    #[test]
    fn failure_removes_in_flight_state() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        let request = cache.begin_request(k).unwrap();
        assert!(cache.is_in_flight(k));
        cache.fail(request);
        assert!(!cache.is_in_flight(k));
    }

    #[test]
    fn repeated_failure_returns_warn_user_only_once() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 8, 8), 16, 1.0);
        let r1 = cache.begin_request(k).unwrap();
        assert!(cache.fail(r1));
        let r2 = cache.begin_request(k).unwrap();
        assert!(!cache.fail(r2));
    }

    #[test]
    fn lookup_refreshes_recency() {
        let mut cache = PixelatePreviewCache::new(128);
        let k1 = key(1, region(0, 0, 4, 4), 16, 1.0); // 64 bytes
        let k2 = key(1, region(4, 4, 4, 4), 16, 1.0); // 64 bytes
        let req1 = cache.begin_request(k1).unwrap();
        assert_eq!(cache.complete(pixels_for(req1, 4, 4)), Completion::Accepted);
        cache.lookup(k1);
        let req2 = cache.begin_request(k2).unwrap();
        assert_eq!(cache.complete(pixels_for(req2, 4, 4)), Completion::Accepted);
        assert!(cache.lookup(k1).is_some());
        assert!(cache.lookup(k2).is_some());
    }

    #[test]
    fn retain_requested_preserves_requested_entries() {
        let mut cache = PixelatePreviewCache::new(256);
        let k1 = key(1, region(0, 0, 4, 4), 16, 1.0);
        let k2 = key(1, region(4, 4, 4, 4), 16, 1.0);
        let req1 = cache.begin_request(k1).unwrap();
        let req2 = cache.begin_request(k2).unwrap();
        assert_eq!(cache.complete(pixels_for(req1, 4, 4)), Completion::Accepted);
        assert_eq!(cache.complete(pixels_for(req2, 4, 4)), Completion::Accepted);
        cache.retain_requested(&[k1]);
        assert!(cache.lookup(k1).is_some());
        assert!(cache.lookup(k2).is_none());
        assert_eq!(cache.retained_bytes(), 64);
    }

    #[test]
    fn clear_for_source_recalculates_retained_bytes() {
        let mut cache = PixelatePreviewCache::new(1024);
        let k = key(1, region(0, 0, 4, 4), 16, 1.0);
        let request = cache.begin_request(k).unwrap();
        assert_eq!(
            cache.complete(pixels_for(request, 4, 4)),
            Completion::Accepted
        );
        assert_eq!(cache.retained_bytes(), 64);
        cache.clear_for_source(1);
        assert_eq!(cache.retained_bytes(), 0);
    }
}
