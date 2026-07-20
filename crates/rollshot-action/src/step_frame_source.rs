use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::frame_store::FrameStore;
use crate::models::{FrameId, Millis};
use crate::project::{
    decode_png_asset, LoadedProject, ProjectError, ProjectFrame, SnapshotFrame,
    SnapshotFramePayload,
};

pub const DEFAULT_PROJECT_FRAME_CACHE_BYTES: usize = 256 * 1024 * 1024;

pub struct LoadedStepFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub image: Arc<RgbaImage>,
}

impl Clone for LoadedStepFrame {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            at_ms: self.at_ms,
            image: Arc::clone(&self.image),
        }
    }
}

impl std::fmt::Debug for LoadedStepFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedStepFrame")
            .field("id", &self.id)
            .field("at_ms", &self.at_ms)
            .field(
                "image",
                &format!("{}x{}", self.image.width(), self.image.height()),
            )
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct StepFrameLoadRequest {
    pub project_root: PathBuf,
    pub frame: ProjectFrame,
}

pub enum StepFrameSource {
    InMemory(FrameStore),
    Project(ProjectFrameSource),
}

pub struct ProjectFrameSource {
    root: PathBuf,
    catalog: BTreeMap<FrameId, ProjectFrame>,
    cache: BTreeMap<FrameId, Arc<RgbaImage>>,
    lru: VecDeque<FrameId>,
    decoded_bytes: usize,
    byte_limit: usize,
}

impl ProjectFrameSource {
    pub fn from_loaded(project: &LoadedProject, byte_limit: usize) -> Self {
        let mut catalog = BTreeMap::new();
        for frame in &project.manifest.frames {
            catalog.insert(frame.id, frame.clone());
        }
        Self {
            root: project.root.clone(),
            catalog,
            cache: BTreeMap::new(),
            lru: VecDeque::new(),
            decoded_bytes: 0,
            byte_limit,
        }
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn frame(&self, id: FrameId) -> Option<&ProjectFrame> {
        self.catalog.get(&id)
    }

    pub fn cached(&mut self, id: FrameId) -> Option<Arc<RgbaImage>> {
        if let Some(img) = self.cache.get(&id) {
            if let Some(pos) = self.lru.iter().position(|&x| x == id) {
                self.lru.remove(pos);
            }
            self.lru.push_back(id);
            Some(Arc::clone(img))
        } else {
            None
        }
    }

    pub fn load_request(&self, id: FrameId) -> Option<StepFrameLoadRequest> {
        let frame = self.catalog.get(&id)?;
        Some(StepFrameLoadRequest {
            project_root: self.root.clone(),
            frame: frame.clone(),
        })
    }

    pub fn insert_loaded(&mut self, loaded: LoadedStepFrame) {
        let rgba_byte_size = (loaded.image.width() as usize)
            .checked_mul(loaded.image.height() as usize)
            .and_then(|v| v.checked_mul(4))
            .expect("checked byte math overflow");

        if rgba_byte_size > self.byte_limit {
            tracing::trace!(
                target: "rollshot::step_frame_source",
                id = loaded.id,
                bytes = rgba_byte_size,
                limit = self.byte_limit,
                "single oversized image returned but not cached"
            );
            return;
        }

        if let Some(old) = self.cache.remove(&loaded.id) {
            let old_size = (old.width() as usize)
                .checked_mul(old.height() as usize)
                .and_then(|v| v.checked_mul(4))
                .expect("checked byte math overflow");
            self.decoded_bytes = self.decoded_bytes.saturating_sub(old_size);
            self.lru.retain(|&x| x != loaded.id);
        }

        while self.decoded_bytes + rgba_byte_size > self.byte_limit {
            if let Some(evict_id) = self.lru.pop_front() {
                if let Some(evicted) = self.cache.remove(&evict_id) {
                    let evicted_size = (evicted.width() as usize)
                        .checked_mul(evicted.height() as usize)
                        .and_then(|v| v.checked_mul(4))
                        .expect("checked byte math overflow");
                    self.decoded_bytes = self.decoded_bytes.saturating_sub(evicted_size);
                }
            }
        }

        self.decoded_bytes += rgba_byte_size;
        self.cache.insert(loaded.id, loaded.image);
        self.lru.push_back(loaded.id);
    }

    #[cfg(test)]
    pub fn cached_count_for_test(&self) -> usize {
        self.cache.len()
    }
}

impl StepFrameSource {
    pub fn cached(&mut self, id: FrameId) -> Option<Arc<RgbaImage>> {
        match self {
            Self::InMemory(store) => store.retained(id).map(|f| Arc::clone(&f.image)),
            Self::Project(src) => src.cached(id),
        }
    }

    pub fn load_request(&self, id: FrameId) -> Option<StepFrameLoadRequest> {
        match self {
            Self::InMemory(_) => None,
            Self::Project(src) => src.load_request(id),
        }
    }

    pub fn insert_loaded(&mut self, loaded: LoadedStepFrame) {
        match self {
            Self::InMemory(_) => {}
            Self::Project(src) => src.insert_loaded(loaded),
        }
    }

    pub fn snapshot_frame(&mut self, id: FrameId) -> Option<SnapshotFrame> {
        match self {
            Self::InMemory(store) => {
                let rf = store.retained(id)?;
                Some(SnapshotFrame {
                    id: rf.id,
                    at_ms: rf.at_ms,
                    payload: SnapshotFramePayload::Pixels(Arc::clone(&rf.image)),
                })
            }
            Self::Project(src) => {
                let frame = src.catalog.get(&id)?.clone();
                if let Some(img) = src.cached(id) {
                    Some(SnapshotFrame {
                        id: frame.id,
                        at_ms: frame.at_ms,
                        payload: SnapshotFramePayload::Pixels(img),
                    })
                } else {
                    Some(SnapshotFrame {
                        id: frame.id,
                        at_ms: frame.at_ms,
                        payload: SnapshotFramePayload::ExistingAsset {
                            project_root: src.root.clone(),
                            sha256: frame.sha256,
                            width: frame.width,
                            height: frame.height,
                        },
                    })
                }
            }
        }
    }

    pub fn in_memory(&self) -> Option<&FrameStore> {
        match self {
            Self::InMemory(store) => Some(store),
            Self::Project(_) => None,
        }
    }
}

pub fn load_step_frame(request: StepFrameLoadRequest) -> Result<LoadedStepFrame, ProjectError> {
    let img = decode_png_asset(
        &request.project_root,
        &request.frame.sha256,
        request.frame.width,
        request.frame.height,
    )?;
    Ok(LoadedStepFrame {
        id: request.frame.id,
        at_ms: request.frame.at_ms,
        image: Arc::new(img),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::encode_png_asset;
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    fn setup_project(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("assets/frames")).unwrap();
    }

    fn write_asset(root: &std::path::Path, image: &RgbaImage) -> ProjectFrame {
        let encoded = encode_png_asset(image).unwrap();
        let dest = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).unwrap();
        ProjectFrame {
            id: 0,
            at_ms: 0,
            sha256: encoded.sha256,
            width: image.width(),
            height: image.height(),
        }
    }

    fn project_with_three_4x4_assets() -> LoadedProject {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        setup_project(&root);

        let mut frames = Vec::new();
        for i in 0..3u64 {
            let image = RgbaImage::from_pixel(4, 4, Rgba([i as u8, 0, 0, 255]));
            let mut frame = write_asset(&root, &image);
            frame.id = i + 1;
            frame.at_ms = i * 100;
            frames.push(frame);
        }

        // Leak the tempdir so assets persist for the test lifetime.
        // This is acceptable in tests that need on-disk assets.
        let _ = dir.keep();

        LoadedProject {
            root,
            manifest: crate::project::ProjectManifestV2 {
                schema_version: 2,
                revision: 1,
                title: "Test".into(),
                capture_region: crate::models::CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 4,
                },
                input_source: crate::models::InputSourceKind::VisualOnly,
                input_capability: crate::models::InputCapability::VisualOnly {
                    reason: crate::models::DegradedReason::SourceStartFailed,
                },
                enabled_outputs: Default::default(),
                frames,
                steps: Vec::new(),
                import_warnings: Vec::new(),
            },
        }
    }

    #[test]
    fn project_source_is_lazy_and_byte_bounded() {
        let loaded = project_with_three_4x4_assets();
        let mut source = ProjectFrameSource::from_loaded(&loaded, 4 * 4 * 4 * 2);
        assert_eq!(source.cached_count_for_test(), 0);

        let first = load_step_frame(source.load_request(1).unwrap()).unwrap();
        source.insert_loaded(first);
        let first_arc = source.cached(1).unwrap();
        assert!(Arc::ptr_eq(&first_arc, &source.cached(1).unwrap()));

        let second = load_step_frame(source.load_request(2).unwrap()).unwrap();
        source.insert_loaded(second);
        let first_arc_again = source.cached(1).unwrap();
        assert!(Arc::ptr_eq(&first_arc, &first_arc_again));
        let third = load_step_frame(source.load_request(3).unwrap()).unwrap();
        source.insert_loaded(third);
        assert!(source.cached(1).is_some());
        assert!(source.cached(2).is_none());
        assert!(source.cached(3).is_some());
    }

    #[test]
    fn construction_clones_only_root_and_catalog() {
        let loaded = project_with_three_4x4_assets();
        let source = ProjectFrameSource::from_loaded(&loaded, DEFAULT_PROJECT_FRAME_CACHE_BYTES);
        assert_eq!(source.catalog.len(), 3);
        assert!(source.cache.is_empty());
        assert!(source.lru.is_empty());
        assert_eq!(source.decoded_bytes, 0);
    }

    #[test]
    fn cache_hit_refreshes_lru_recency() {
        let loaded = project_with_three_4x4_assets();
        // 128 bytes per frame (4*4*4=64), limit 192 -> can hold 3 frames
        let mut source = ProjectFrameSource::from_loaded(&loaded, 4 * 4 * 4 * 3);

        for id in [1, 2, 3] {
            let req = source.load_request(id).unwrap();
            source.insert_loaded(load_step_frame(req).unwrap());
        }

        // Access frame 1 to refresh its recency
        source.cached(1);

        // Insert a 4th to evict the LRU front (frame 2)
        let mut frame4 = loaded.manifest.frames[0].clone();
        frame4.id = 4;
        frame4.at_ms = 300;
        let image = RgbaImage::from_pixel(4, 4, Rgba([3, 0, 0, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        let dest = loaded
            .root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).unwrap();
        frame4.sha256 = encoded.sha256;

        let mut src2 = ProjectFrameSource::from_loaded(&loaded, 4 * 4 * 4 * 3);
        for id in [1, 2, 3] {
            let req = src2.load_request(id).unwrap();
            src2.insert_loaded(load_step_frame(req).unwrap());
        }
        src2.cached(1); // refresh frame 1
        src2.insert_loaded(LoadedStepFrame {
            id: 4,
            at_ms: 300,
            image: Arc::new(image),
        });
        // Frame 2 should be evicted (was LRU front), frame 1 refreshed
        assert!(src2.cached(1).is_some());
        assert!(src2.cached(2).is_none());
        assert!(src2.cached(3).is_some());
        assert!(src2.cached(4).is_some());
    }

    #[test]
    fn replacing_cached_id_does_not_double_count_bytes() {
        let loaded = project_with_three_4x4_assets();
        let mut source = ProjectFrameSource::from_loaded(&loaded, 4 * 4 * 4 * 2);

        let req = source.load_request(1).unwrap();
        source.insert_loaded(load_step_frame(req).unwrap());
        let bytes_after_first = source.decoded_bytes;

        // Replace with same-sized image
        let image = RgbaImage::from_pixel(4, 4, Rgba([99, 0, 0, 255]));
        source.insert_loaded(LoadedStepFrame {
            id: 1,
            at_ms: 0,
            image: Arc::new(image),
        });
        assert_eq!(source.decoded_bytes, bytes_after_first);
    }

    #[test]
    fn single_oversized_image_is_returned_but_not_cached() {
        let loaded = project_with_three_4x4_assets();
        // Limit is 1 byte — no 4x4 RGBA image can fit
        let mut source = ProjectFrameSource::from_loaded(&loaded, 1);

        let req = source.load_request(1).unwrap();
        let loaded = load_step_frame(req).unwrap();
        // The image was decoded successfully
        assert_eq!(loaded.image.width(), 4);
        source.insert_loaded(loaded);
        // But it was not cached
        assert_eq!(source.cached_count_for_test(), 0);
    }

    #[test]
    fn checked_byte_math_cannot_wrap() {
        let loaded = project_with_three_4x4_assets();
        let mut source = ProjectFrameSource::from_loaded(&loaded, usize::MAX);

        // u32::MAX dimensions would overflow usize on 32-bit, but our test images are small.
        // Verify that normal images don't panic.
        let req = source.load_request(1).unwrap();
        source.insert_loaded(load_step_frame(req).unwrap());
        assert!(source.decoded_bytes > 0);
    }

    #[test]
    fn digest_valid_decode_failure_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        setup_project(&root);

        let image = RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        let sha256 = encoded.sha256.clone();
        let dest = root.join("assets/frames").join(format!("{sha256}.png"));
        // Write corrupt data
        let mut corrupt = encoded.bytes.clone();
        if corrupt.len() > 40 {
            corrupt[35] ^= 0xFF;
        }
        std::fs::write(&dest, &corrupt).unwrap();

        let request = StepFrameLoadRequest {
            project_root: root,
            frame: ProjectFrame {
                id: 1,
                at_ms: 100,
                sha256,
                width: 4,
                height: 4,
            },
        };
        let result = load_step_frame(request);
        assert!(result.is_err());
    }

    #[test]
    fn snapshot_frame_preserves_timestamp_and_payload_identity() {
        let loaded = project_with_three_4x4_assets();
        let mut source = StepFrameSource::Project(ProjectFrameSource::from_loaded(
            &loaded,
            DEFAULT_PROJECT_FRAME_CACHE_BYTES,
        ));

        // Uncached -> ExistingAsset
        let snap = source.snapshot_frame(1).unwrap();
        assert_eq!(snap.id, 1);
        assert_eq!(snap.at_ms, 0);
        match &snap.payload {
            SnapshotFramePayload::ExistingAsset { sha256, .. } => {
                assert_eq!(sha256, &loaded.manifest.frames[0].sha256);
            }
            _ => panic!("expected ExistingAsset for uncached frame"),
        }

        // Cached -> Pixels
        let req = source.load_request(1).unwrap();
        source.insert_loaded(load_step_frame(req).unwrap());
        let snap = source.snapshot_frame(1).unwrap();
        match &snap.payload {
            SnapshotFramePayload::Pixels(img) => {
                assert_eq!(img.width(), 4);
                assert_eq!(img.height(), 4);
            }
            _ => panic!("expected Pixels for cached frame"),
        }
    }

    #[test]
    fn in_memory_source_returns_frame_store() {
        let store = FrameStore::new(Default::default());
        let source = StepFrameSource::InMemory(store);
        assert!(source.in_memory().is_some());
    }

    #[test]
    fn project_source_returns_none_for_in_memory() {
        let loaded = project_with_three_4x4_assets();
        let source = StepFrameSource::Project(ProjectFrameSource::from_loaded(
            &loaded,
            DEFAULT_PROJECT_FRAME_CACHE_BYTES,
        ));
        assert!(source.in_memory().is_none());
    }

    #[test]
    fn frame_store_retained_shared_returns_timestamp_and_arc() {
        let mut store = FrameStore::new(Default::default());
        let image = RgbaImage::new(4, 4);
        let id = store.ingest(image, 42);
        store.retain_window(id);

        let (at_ms, arc) = store.retained_shared(id).unwrap();
        assert_eq!(at_ms, 42);
        let retained = store.retained(id).unwrap();
        assert!(Arc::ptr_eq(&arc, &retained.image));
    }
}
