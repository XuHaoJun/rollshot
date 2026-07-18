use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::error::ProjectError;
use super::store::write_json_atomic;

const PUBLISH_STATE_SCHEMA_VERSION: u32 = 1;
const PUBLISH_STATE_FILE: &str = "publish-state.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishOutputKind {
    Core,
    Storyboard,
    Gif,
    Mp4,
}

impl PublishOutputKind {
    pub const ALL: &'static [PublishOutputKind] = &[
        PublishOutputKind::Core,
        PublishOutputKind::Storyboard,
        PublishOutputKind::Gif,
        PublishOutputKind::Mp4,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedOutputV1 {
    pub last_successful_revision: u64,
}

impl PublishedOutputV1 {
    pub fn new(revision: u64) -> Self {
        Self {
            last_successful_revision: revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishStateV1 {
    pub schema_version: u32,
    pub outputs: BTreeMap<PublishOutputKind, PublishedOutputV1>,
}

impl Default for PublishStateV1 {
    fn default() -> Self {
        Self {
            schema_version: PUBLISH_STATE_SCHEMA_VERSION,
            outputs: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishFreshness {
    Current,
    Stale,
}

pub enum PublishStateLoad {
    Unavailable,
    Available { state: PublishStateV1 },
}

impl PublishStateLoad {
    pub fn freshness(&self, kind: PublishOutputKind, current_revision: u64) -> PublishFreshness {
        match self {
            PublishStateLoad::Unavailable => PublishFreshness::Stale,
            PublishStateLoad::Available { state } => {
                let Some(output) = state.outputs.get(&kind) else {
                    return PublishFreshness::Stale;
                };
                if output.last_successful_revision != current_revision {
                    return PublishFreshness::Stale;
                }
                PublishFreshness::Current
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

pub fn load_publish_state(root: &Path) -> PublishStateLoad {
    let path = root.join(PUBLISH_STATE_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return PublishStateLoad::Unavailable,
    };

    let state: PublishStateV1 = match serde_json::from_slice(&bytes) {
        Ok(s) => s,
        Err(_) => return PublishStateLoad::Unavailable,
    };

    if state.schema_version != PUBLISH_STATE_SCHEMA_VERSION {
        return PublishStateLoad::Unavailable;
    }

    let reconciled = reconcile_availability(root, &state);
    PublishStateLoad::Available { state: reconciled }
}

fn reconcile_availability(root: &Path, state: &PublishStateV1) -> PublishStateV1 {
    let mut reconciled = PublishStateV1 {
        schema_version: state.schema_version,
        outputs: BTreeMap::new(),
    };

    for (&kind, output) in &state.outputs {
        let artifact_present = match kind {
            PublishOutputKind::Core => is_core_tree_present(root, output.last_successful_revision),
            PublishOutputKind::Storyboard => is_regular_file(root.join("publish/storyboard.png")),
            PublishOutputKind::Gif => is_regular_file(root.join("publish/guide.gif")),
            PublishOutputKind::Mp4 => is_regular_file(root.join("publish/summary.mp4")),
        };
        if artifact_present {
            reconciled.outputs.insert(kind, output.clone());
        }
    }

    reconciled
}

fn is_regular_file(path: impl AsRef<Path>) -> bool {
    match std::fs::symlink_metadata(path.as_ref()) {
        Ok(m) => m.file_type().is_file(),
        Err(_) => false,
    }
}

fn is_core_tree_present(root: &Path, _revision: u64) -> bool {
    let publish = root.join("publish");

    if !is_regular_file(publish.join("index.html")) {
        return false;
    }
    if !is_regular_file(publish.join("steps.md")) {
        return false;
    }
    if !is_regular_file(publish.join("session.json")) {
        return false;
    }

    let session_bytes = match std::fs::read(publish.join("session.json")) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let manifest: crate::SessionManifest = match serde_json::from_slice(&session_bytes) {
        Ok(m) => m,
        Err(_) => return false,
    };

    validate_core_keyframes(root, &manifest.steps)
}

fn validate_core_keyframes(root: &Path, steps: &[crate::ManifestStep]) -> bool {
    let mut seen = BTreeSet::new();

    for step in steps {
        let kf = &step.keyframe_file;

        if kf.starts_with('/') || kf.starts_with("..") || kf.contains("/..") {
            return false;
        }
        if !kf.starts_with("keyframes/") {
            return false;
        }
        if !kf.ends_with(".png") {
            return false;
        }
        if !seen.insert(kf) {
            return false;
        }

        let abs = root.join("publish").join(kf);
        if !is_regular_file(&abs) {
            return false;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

pub fn write_publish_state(root: &Path, state: &PublishStateV1) -> Result<(), ProjectError> {
    write_json_atomic(root, PUBLISH_STATE_FILE, state)
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishCancelled;

impl std::fmt::Display for PublishCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("publish cancelled")
    }
}

impl std::error::Error for PublishCancelled {}

#[derive(Debug, Clone)]
pub struct PublishCancellation {
    flag: Arc<AtomicBool>,
}

impl PublishCancellation {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), PublishCancelled> {
        if self.is_cancelled() {
            Err(PublishCancelled)
        } else {
            Ok(())
        }
    }

    pub fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}

impl Default for PublishCancellation {
    fn default() -> Self {
        Self::new()
    }
}
