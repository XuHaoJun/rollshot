use rollshot_image_document::pixelate::{PixelateError, RasterRegion};
use rollshot_image_document::{Annotation, ImageDocument, ImageRect};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Instant;

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

    /// Non-mutating read for use during view rendering (no LRU update).
    pub fn get(&self, key: PreviewKey) -> Option<(u32, u32, &[u8])> {
        self.entries
            .get(&key)
            .map(|entry| (entry.width, entry.height, entry.rgba.as_slice()))
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

/// Pure collector: which preview keys are needed for the current view.
///
/// Walks committed annotations in graph order, filters by visible bounds,
/// appends the current transient replacement/draft once, and deduplicates
/// exact keys.
pub(crate) fn requested_pixelate_keys(
    document: &ImageDocument,
    transient_annotations: &[Annotation],
    visible_image_bounds: ImageRect,
    display_scale: f32,
) -> Vec<PreviewKey> {
    let source = document.shared_source();
    let source_id = source_id_from_arc(&source);
    let mut seen = std::collections::HashSet::new();
    let mut keys = Vec::new();

    // Walk committed annotations in graph order.
    for annotation in document.annotations() {
        if !matches!(annotation, Annotation::Pixelate { .. }) {
            continue;
        }
        let bounds = rollshot_image_document::annotation_bounds(annotation);
        if !bounds.intersects(&visible_image_bounds) {
            continue;
        }
        if let Annotation::Pixelate {
            bounds, block_size, ..
        } = annotation
        {
            let region = match rollshot_image_document::raster_region(
                *bounds,
                source.width(),
                source.height(),
            ) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let key = PreviewKey::new(source_id, region, *block_size, display_scale);
            if seen.insert(key) {
                keys.push(key);
            }
        }
    }

    // Append transient draft/property/direct-manipulation Pixelate once.
    for annotation in transient_annotations {
        if let Annotation::Pixelate {
            bounds, block_size, ..
        } = annotation
        {
            let region = match rollshot_image_document::raster_region(
                *bounds,
                source.width(),
                source.height(),
            ) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let key = PreviewKey::new(source_id, region, *block_size, display_scale);
            if seen.insert(key) {
                keys.push(key);
            }
        }
    }

    keys
}

/// Synchronous worker: generate a pixelate preview.
///
/// Calls the pixelate kernel at full source resolution, then downsizes the
/// generated region when `display_scale < 1.0` using nearest-neighbor.
pub(crate) fn generate_preview(
    source: Arc<image::RgbaImage>,
    request: PreviewRequest,
) -> Result<PreviewPixels, PreviewGenerationError> {
    let t0 = Instant::now();
    let bounds = ImageRect {
        x: request.key.region.x as f32,
        y: request.key.region.y as f32,
        width: request.key.region.width as f32,
        height: request.key.region.height as f32,
    };
    let block_size = request.key.block_size;
    let display_scale = f32::from_bits(request.key.display_scale_bits);

    let pixelated = rollshot_image_document::pixelate_region(&source, bounds, block_size)
        .map_err(PreviewGenerationError::Kernel)?;

    let region = &pixelated.region;
    let generated_bytes = region.byte_len();

    let (out_w, out_h, rgba) = if display_scale < 1.0 {
        let w = ((region.width as f32 * display_scale).round() as u32).max(1);
        let h = ((region.height as f32 * display_scale).round() as u32).max(1);
        let resized = image::imageops::resize(
            &pixelated.pixels,
            w,
            h,
            image::imageops::FilterType::Nearest,
        );
        (w, h, resized.into_raw())
    } else {
        (region.width, region.height, pixelated.pixels.into_raw())
    };

    tracing::trace!(
        target: "rollshot::annotation",
        source_id = request.key.source_id,
        region_w = request.key.region.width,
        region_h = request.key.region.height,
        block_size,
        cache_outcome = "generated",
        generated_bytes,
        elapsed_us = t0.elapsed().as_micros() as u64,
        "preview generated"
    );
    tracing::debug!(
        target: "rollshot::annotation",
        "pixelate preview generated for {}x{} region, block_size={}",
        request.key.region.width,
        request.key.region.height,
        block_size
    );

    Ok(PreviewPixels {
        request,
        width: out_w,
        height: out_h,
        rgba,
    })
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

    // -- Step 1: Scheduler tests -----------------------------------------------

    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_image_document::ImageDocument;

    fn make_doc(w: u32, h: u32) -> ImageDocument {
        ImageDocument::new(RgbaImage::from_pixel(w, h, Rgba([100, 150, 200, 255])))
    }

    fn visible_all(w: u32, h: u32) -> ImageRect {
        ImageRect {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        }
    }

    #[test]
    fn collector_returns_keys_for_visible_committed_pixelate() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let keys = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].block_size, 16);
    }

    #[test]
    fn collector_includes_transient_draft_pixelate() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let transient = Annotation::pixelate(
            rollshot_image_document::AnnotationId(u64::MAX),
            ImageRect {
                x: 60.0,
                y: 60.0,
                width: 20.0,
                height: 20.0,
            },
            8,
        );
        let keys = requested_pixelate_keys(&doc, &[transient], visible_all(100, 100), 1.0);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn collector_skips_annotations_outside_visible_bounds() {
        let mut doc = make_doc(100, 10000);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 9000.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let visible = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 200.0,
        };
        let keys = requested_pixelate_keys(&doc, &[], visible, 1.0);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn collector_deduplicates_exact_keys() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        // Same bounds, same block_size, same display_scale → same key.
        let transient = Annotation::pixelate(
            rollshot_image_document::AnnotationId(u64::MAX),
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let keys = requested_pixelate_keys(&doc, &[transient], visible_all(100, 100), 1.0);
        assert_eq!(keys.len(), 1, "exact duplicates are deduped");
    }

    #[test]
    fn no_new_key_for_scroll_only_repositioning() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let visible1 = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let visible2 = ImageRect {
            x: 5.0,
            y: 5.0,
            width: 100.0,
            height: 100.0,
        };
        let k1 = requested_pixelate_keys(&doc, &[], visible1, 1.0);
        let k2 = requested_pixelate_keys(&doc, &[], visible2, 1.0);
        assert_eq!(k1, k2, "scrolling doesn't change keys for same annotation");
    }

    #[test]
    fn geometry_change_produces_different_key() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let keys1 = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        // Move the annotation.
        let annotations = doc.annotations();
        let id = annotations[0].id();
        doc.set_pixelate_bounds(
            id,
            ImageRect {
                x: 20.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
        )
        .unwrap();
        let keys2 = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        assert_ne!(keys1, keys2, "geometry change → different key");
    }

    #[test]
    fn block_size_change_produces_different_key() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let keys1 = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        let id = doc.annotations()[0].id();
        doc.set_pixelate_block_size(id, 32).unwrap();
        let keys2 = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        assert_ne!(keys1, keys2, "block_size change → different key");
    }

    #[test]
    fn undo_removes_key_from_requested() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let keys = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        assert_eq!(keys.len(), 1);
        doc.undo();
        let keys = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        assert_eq!(keys.len(), 0, "undo removes the key");
    }

    #[test]
    fn redo_restores_key() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        doc.undo();
        doc.redo();
        let keys = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        assert_eq!(keys.len(), 1, "redo restores the key");
    }

    #[test]
    fn display_scale_change_produces_different_key() {
        let mut doc = make_doc(100, 100);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
            16,
        );
        let keys1 = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        let keys2 = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 0.5);
        assert_ne!(keys1, keys2, "display_scale change → different key");
    }

    #[test]
    fn collector_skips_non_pixelate_annotations() {
        let mut doc = make_doc(100, 100);
        doc.add_number_callout(
            rollshot_image_document::ImagePoint::new(10.0, 10.0),
            rollshot_image_document::ImagePoint::new(10.0, 10.0),
        );
        doc.add_redaction(ImageRect {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        })
        .unwrap();
        let keys = requested_pixelate_keys(&doc, &[], visible_all(100, 100), 1.0);
        assert_eq!(keys.len(), 0, "non-pixelate annotations are skipped");
    }

    #[test]
    fn collector_preserves_graph_order() {
        let mut doc = make_doc(200, 200);
        doc.add_pixelate(
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 30.0,
                height: 30.0,
            },
            16,
        );
        doc.add_pixelate(
            ImageRect {
                x: 80.0,
                y: 80.0,
                width: 30.0,
                height: 30.0,
            },
            8,
        );
        let keys = requested_pixelate_keys(&doc, &[], visible_all(200, 200), 1.0);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].block_size, 16);
        assert_eq!(keys[1].block_size, 8);
    }

    // -- generate_preview tests ------------------------------------------------

    #[test]
    fn generate_preview_produces_rgba_at_full_resolution() {
        let source = Arc::new(RgbaImage::from_pixel(64, 64, Rgba([100, 150, 200, 255])));
        let region = RasterRegion {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        };
        let request = PreviewRequest {
            key: PreviewKey::new(source_id_from_arc(&source), region, 16, 1.0),
            generation: 1,
        };
        let result = generate_preview(source, request).unwrap();
        assert_eq!(result.width, 32);
        assert_eq!(result.height, 32);
        assert_eq!(result.rgba.len(), 32 * 32 * 4);
    }

    #[test]
    fn generate_preview_downsizes_when_display_scale_below_one() {
        let source = Arc::new(RgbaImage::from_pixel(64, 64, Rgba([100, 150, 200, 255])));
        let region = RasterRegion {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        };
        let request = PreviewRequest {
            key: PreviewKey::new(source_id_from_arc(&source), region, 16, 0.5),
            generation: 1,
        };
        let result = generate_preview(source, request).unwrap();
        assert_eq!(result.width, 16);
        assert_eq!(result.height, 16);
        assert_eq!(result.rgba.len(), 16 * 16 * 4);
    }

    #[test]
    fn generate_preview_returns_error_for_invalid_bounds() {
        let source = Arc::new(RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255])));
        let region = RasterRegion {
            x: 100,
            y: 100,
            width: 10,
            height: 10,
        };
        let request = PreviewRequest {
            key: PreviewKey::new(source_id_from_arc(&source), region, 16, 1.0),
            generation: 1,
        };
        let result = generate_preview(source, request);
        assert!(result.is_err());
    }
}
