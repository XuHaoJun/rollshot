# Preset Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `rollshot-preset` crate — durable, file-based JSON persistence for Smart Redaction presets and their immutable automation revisions, with safe active-revision selection and revalidate-on-load — and unify the app's config-root resolver on the XDG (etcetera) strategy.

**Architecture:** A framework-neutral library crate stores each preset as a directory under an injected root: `preset.json` plus write-once `revisions/<id>.json` files, each wrapping a `rollshot_automation::ValidatedAutomation`. Writes are atomic (`tmp → fsync → rename`); `preset.json` mutations take an `fs4` advisory lock. Loading a revision calls `rollshot_automation::ensure_compatible` to revalidate it. The product edge resolves the root via the shared `rollshot_config_dir()`, upgraded from `dirs` to `etcetera`.

**Tech Stack:** Rust (edition 2021, MSRV 1.94), `serde`/`serde_json`, `thiserror`, `fs4` (file locks), `etcetera` (XDG dirs, product edge only), `rollshot-automation`. Tests use `tempfile`.

**Spec:** `docs/superpowers/specs/2026-06-23-preset-persistence-design.md`

## Global Constraints

Every task's requirements implicitly include these (verbatim from the spec):

- Crate `rollshot-preset`; `unsafe_code = "forbid"` (inherited via `[lints] workspace = true`).
- Dependency direction: `rollshot-preset → rollshot-automation` **only**. No dependency on `rollshot-agent`, no UI/windowing/capture/provider code.
- The store accepts an **injected `root: PathBuf`**; the crate never resolves a home/config path and never reads environment variables. All crate tests run against a temp dir.
- IDs and timestamps are **caller-supplied** opaque values (`PresetId(String)`, `RevisionId(String)`, RFC 3339 `created_at`/`now: String`). The crate treats them as opaque.
- `AutomationRevision` is **immutable / write-once**; there is no overwrite or mutate API. Only `preset.json` fields (`active_revision_id`, `name`, `updated_at`) change.
- `add_revision` does **not** auto-activate; activation is a separate `set_active_revision` call.
- Every file write is atomic: serialize to `<file>.tmp`, `fsync`, `rename` over the destination. Readers/listers ignore `.tmp`.
- `load_revision` / `load_active_revision` must call `rollshot_automation::ensure_compatible(&artifact)` before returning.
- `store_schema_version` (`const STORE_SCHEMA_VERSION: u16 = 1`) is embedded in every file, independent of the automation schema versions.
- Tracing (if any) uses stable `rollshot::*` targets with privacy-safe fields only; no `println!`/`eprintln!`/`dbg!`. (This plan adds no tracing.)
- Verify each task with `rtk cargo test -p rollshot-preset`, `rtk cargo fmt --check`, `rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings`.

---

### Task 1: Crate scaffold, domain types, and error model

**Files:**
- Create: `crates/rollshot-preset/Cargo.toml`
- Create: `crates/rollshot-preset/src/lib.rs`
- Create: `crates/rollshot-preset/src/domain.rs`
- Create: `crates/rollshot-preset/src/error.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**
- Consumes: `rollshot_automation::{ValidatedAutomation, CompatibilityError, validate_source, ValidationLimits}`.
- Produces: types `PresetId(String)`, `RevisionId(String)`, `RevisionOrigin`, `RevisionProvenance`, `Preset`, `AutomationRevision`, `PresetSummary`, `RevisionSummary`, `const STORE_SCHEMA_VERSION: u16`; `enum StoreError`, `enum EntityKind`, `type Result<T> = std::result::Result<T, StoreError>`.

- [ ] **Step 1: Add the crate to the workspace members**

In `Cargo.toml`, add the new crate to the `members` array (after `"crates/rollshot-agent",`):

```toml
    "crates/rollshot-agent",
    "crates/rollshot-preset",
]
```

- [ ] **Step 2: Create `crates/rollshot-preset/Cargo.toml`**

```toml
[package]
name = "rollshot-preset"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
rollshot-automation = { path = "../rollshot-automation" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
fs4 = { workspace = true }

[dev-dependencies]
tempfile = "3"

[lints]
workspace = true
```

- [ ] **Step 3: Create `crates/rollshot-preset/src/error.rs`**

```rust
use std::path::PathBuf;

/// What kind of entity a [`StoreError::NotFound`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Preset,
    Revision,
}

/// Errors returned by the preset store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("{kind:?} not found: {id}")]
    NotFound { kind: EntityKind, id: String },
    #[error("incompatible automation artifact: {0}")]
    Incompatible(#[from] rollshot_automation::CompatibilityError),
    #[error("integrity violation: {0}")]
    Integrity(String),
    #[error("revision already exists: {0}")]
    RevisionExists(String),
    #[error("corrupt store entry at {path}: {detail}")]
    Corrupt { path: PathBuf, detail: String },
}

/// Convenience alias for store operations.
pub type Result<T> = std::result::Result<T, StoreError>;
```

- [ ] **Step 4: Create `crates/rollshot-preset/src/domain.rs`**

```rust
use rollshot_automation::ValidatedAutomation;
use serde::{Deserialize, Serialize};

/// On-disk envelope version for SP5 records, independent of the automation
/// schema versions embedded in the artifact.
pub const STORE_SCHEMA_VERSION: u16 = 1;

/// Opaque, caller-supplied preset identifier (used as a directory name).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PresetId(pub String);

/// Opaque, caller-supplied revision identifier (used as a file name stem).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevisionId(pub String);

/// How a revision came to exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevisionOrigin {
    AgentRun,
    Import,
    Manual,
}

/// Provenance recorded alongside an immutable revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionProvenance {
    pub origin: RevisionOrigin,
    pub note: Option<String>,
    /// Reserved opaque hook for future SP6 session linkage (no migration later).
    pub source_run_ref: Option<String>,
}

/// A preset: durable, user-authored configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub store_schema_version: u16,
    pub id: PresetId,
    pub name: String,
    pub original_intent: String,
    /// `None` until the user accepts a first revision.
    pub active_revision_id: Option<RevisionId>,
    pub created_at: String,
    pub updated_at: String,
}

/// An immutable automation revision wrapping a validated artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationRevision {
    pub store_schema_version: u16,
    pub id: RevisionId,
    pub preset_id: PresetId,
    pub parent_id: Option<RevisionId>,
    pub created_at: String,
    pub provenance: RevisionProvenance,
    pub artifact: ValidatedAutomation,
}

/// Lightweight projection for listing presets without loading every artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetSummary {
    pub id: PresetId,
    pub name: String,
    pub active_revision_id: Option<RevisionId>,
    pub updated_at: String,
}

/// Lightweight projection for listing revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionSummary {
    pub id: RevisionId,
    pub parent_id: Option<RevisionId>,
    pub created_at: String,
}
```

- [ ] **Step 5: Create `crates/rollshot-preset/src/lib.rs` with a serde round-trip test**

```rust
//! Durable persistence for Smart Redaction presets and immutable automation
//! revisions (Sub-project 5). File-based JSON under an injected root. No UI,
//! agent, provider, or capture code.

mod domain;
mod error;

pub use domain::{
    AutomationRevision, Preset, PresetId, PresetSummary, RevisionId, RevisionOrigin,
    RevisionProvenance, RevisionSummary, STORE_SCHEMA_VERSION,
};
pub use error::{EntityKind, Result, StoreError};

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_automation::{validate_source, ValidatedAutomation, ValidationLimits};

    /// A source verified to validate by the rollshot-automation test suite
    /// (`crates/rollshot-automation/tests/fixtures/valid_main.js`).
    pub(crate) const SAMPLE_SOURCE: &str = r#"function expandBounds(rect, padding) {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}

function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 10 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: expandBounds(match.bounds, 8),
      confidence: match.confidence,
      label: "ocr-match",
    })),
  };
}
"#;

    pub(crate) fn sample_artifact() -> ValidatedAutomation {
        validate_source(SAMPLE_SOURCE, &ValidationLimits::default())
            .expect("sample automation should validate")
    }

    #[test]
    fn revision_serde_round_trip() {
        let revision = AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("preset-1".into()),
            parent_id: None,
            created_at: "2026-06-24T00:00:00Z".into(),
            provenance: RevisionProvenance {
                origin: RevisionOrigin::AgentRun,
                note: Some("first".into()),
                source_run_ref: None,
            },
            artifact: sample_artifact(),
        };

        let json = serde_json::to_vec(&revision).unwrap();
        let decoded: AutomationRevision = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded, revision);
    }
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `rtk cargo test -p rollshot-preset`
Expected: PASS (1 test). The crate compiles and `AutomationRevision` round-trips through JSON.

- [ ] **Step 7: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-preset
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
git add Cargo.toml crates/rollshot-preset
git commit -m "feat(preset): scaffold rollshot-preset crate with domain and error types"
```

---

### Task 2: Atomic write and read helpers (`io.rs`)

**Files:**
- Create: `crates/rollshot-preset/src/io.rs`
- Modify: `crates/rollshot-preset/src/lib.rs` (add `mod io;`)

**Interfaces:**
- Consumes: `StoreError` from Task 1.
- Produces: `pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()>`; `pub(crate) fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>>`.

- [ ] **Step 1: Add `mod io;` to `lib.rs`**

Add below `mod error;`:

```rust
mod error;
mod io;
```

- [ ] **Step 2: Write the failing test in `crates/rollshot-preset/src/io.rs`**

```rust
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::error::{Result, StoreError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("file.json");
        write_atomic(&path, b"hello").unwrap();
        assert_eq!(read_optional_bytes(&path).unwrap(), Some(b"hello".to_vec()));
    }

    #[test]
    fn read_optional_bytes_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.json");
        assert_eq!(read_optional_bytes(&path).unwrap(), None);
    }

    #[test]
    fn write_atomic_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        write_atomic(&path, b"data").unwrap();
        assert!(!dir.path().join("file.tmp").exists());
        assert!(path.exists());
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-preset io::`
Expected: FAIL — `write_atomic`/`read_optional_bytes` not found.

- [ ] **Step 4: Implement the helpers (above the `#[cfg(test)]` block)**

```rust
/// Write `bytes` to `path` atomically: serialize to a sibling `.tmp`, fsync,
/// then rename over the destination. A reader sees the old file or the new
/// one, never a partial write.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| StoreError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let tmp = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp).map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
        file.write_all(bytes).map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
    }

    std::fs::rename(&tmp, path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    // Best-effort: fsync the directory so the rename is durable.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Read all bytes at `path`, returning `None` if the file does not exist.
pub(crate) fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StoreError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}
```

Note: `OpenOptions` is imported for Task 3's lock helper, which lands in this same file; if clippy flags it as unused now, add the lock helper in Task 3 before linting, or temporarily drop the import. To avoid churn, remove `OpenOptions` from the `use` line in this task and re-add it in Task 3.

For this task, the imports line should read:

```rust
use std::fs::File;
use std::io::Write;
use std::path::Path;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-preset io::`
Expected: PASS (3 tests).

- [ ] **Step 6: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-preset
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
git add crates/rollshot-preset/src/io.rs crates/rollshot-preset/src/lib.rs
git commit -m "feat(preset): atomic write and read helpers"
```

---

### Task 3: Advisory directory lock (`io.rs`)

**Files:**
- Modify: `crates/rollshot-preset/src/io.rs`

**Interfaces:**
- Consumes: `StoreError` from Task 1.
- Produces: `pub(crate) struct DirLock` (RAII guard releasing on drop); `pub(crate) fn lock_dir(dir: &Path) -> Result<DirLock>`.

- [ ] **Step 1: Write the failing test (add to the `tests` module in `io.rs`)**

```rust
    #[test]
    fn lock_dir_serializes_two_handles() {
        use fs4::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let guard = lock_dir(dir.path()).unwrap();

        // A second exclusive try-lock on the same lock file must report contention
        // while the first guard is alive (mirrors daemon InstanceGuard semantics).
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.path().join(".lock"))
            .unwrap();
        assert!(matches!(
            FileExt::try_lock(&second),
            Err(fs4::TryLockError::WouldBlock)
        ));

        drop(guard);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-preset io::lock_dir`
Expected: FAIL — `lock_dir` / `DirLock` not found.

- [ ] **Step 3: Implement the lock helper**

Update the imports at the top of `io.rs` to re-add `OpenOptions` and import the `fs4` trait:

```rust
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use fs4::FileExt;

use crate::error::{Result, StoreError};
```

Add the implementation (above the `#[cfg(test)]` block):

```rust
/// RAII advisory lock over a preset directory. The OS releases the flock when
/// the held file is dropped/closed.
pub(crate) struct DirLock {
    _file: File,
}

/// Acquire a blocking exclusive advisory lock on `<dir>/.lock`, creating `dir`
/// if needed. Serializes concurrent `preset.json` mutations across processes.
pub(crate) fn lock_dir(dir: &Path) -> Result<DirLock> {
    std::fs::create_dir_all(dir).map_err(|source| StoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let path = dir.join(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
    FileExt::lock(&file).map_err(|source| StoreError::Io { path, source })?;
    Ok(DirLock { _file: file })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-preset io::`
Expected: PASS (4 tests).

- [ ] **Step 5: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-preset
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
git add crates/rollshot-preset/src/io.rs
git commit -m "feat(preset): advisory directory lock via fs4"
```

---

### Task 4: `PresetStore` — create, load, list presets, and id validation

**Files:**
- Create: `crates/rollshot-preset/src/store.rs`
- Modify: `crates/rollshot-preset/src/lib.rs` (add `mod store;` and `pub use store::PresetStore;`)

**Interfaces:**
- Consumes: `io::{write_atomic, read_optional_bytes}`; domain types; `StoreError`.
- Produces:
  - `pub struct PresetStore`
  - `PresetStore::open(root: PathBuf) -> Self`
  - `create_preset(&self, id: PresetId, name: String, original_intent: String, now: String) -> Result<Preset>`
  - `load_preset(&self, id: &PresetId) -> Result<Preset>`
  - `list_presets(&self) -> Result<Vec<PresetSummary>>`
  - internal path helpers and `validate_id`.

- [ ] **Step 1: Wire the module in `lib.rs`**

```rust
mod domain;
mod error;
mod io;
mod store;

pub use domain::{
    AutomationRevision, Preset, PresetId, PresetSummary, RevisionId, RevisionOrigin,
    RevisionProvenance, RevisionSummary, STORE_SCHEMA_VERSION,
};
pub use error::{EntityKind, Result, StoreError};
pub use store::PresetStore;
```

- [ ] **Step 2: Write the failing tests in `crates/rollshot-preset/src/store.rs`**

```rust
use std::path::PathBuf;

use crate::domain::{
    AutomationRevision, Preset, PresetId, PresetSummary, RevisionId, RevisionSummary,
    STORE_SCHEMA_VERSION,
};
use crate::error::{EntityKind, Result, StoreError};
use crate::io;

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, PresetStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = PresetStore::open(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn create_then_load_preset() {
        let (_dir, store) = store();
        let created = store
            .create_preset(
                PresetId("p1".into()),
                "Hide SSNs".into(),
                "hide social security numbers".into(),
                "2026-06-24T00:00:00Z".into(),
            )
            .unwrap();
        assert_eq!(created.active_revision_id, None);
        assert_eq!(created.store_schema_version, STORE_SCHEMA_VERSION);

        let loaded = store.load_preset(&PresetId("p1".into())).unwrap();
        assert_eq!(loaded, created);
    }

    #[test]
    fn load_missing_preset_is_not_found() {
        let (_dir, store) = store();
        let err = store.load_preset(&PresetId("nope".into())).unwrap_err();
        assert!(matches!(
            err,
            StoreError::NotFound {
                kind: EntityKind::Preset,
                ..
            }
        ));
    }

    #[test]
    fn list_presets_empty_then_sorted() {
        let (_dir, store) = store();
        assert!(store.list_presets().unwrap().is_empty());

        for id in ["b", "a"] {
            store
                .create_preset(
                    PresetId(id.into()),
                    id.into(),
                    String::new(),
                    "2026-06-24T00:00:00Z".into(),
                )
                .unwrap();
        }
        let ids: Vec<String> = store
            .list_presets()
            .unwrap()
            .into_iter()
            .map(|s| s.id.0)
            .collect();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn create_rejects_unsafe_id() {
        let (_dir, store) = store();
        let err = store
            .create_preset(
                PresetId("../evil".into()),
                "x".into(),
                String::new(),
                "2026-06-24T00:00:00Z".into(),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::Integrity(_)));
    }

    #[test]
    fn create_existing_preset_is_integrity_error() {
        let (_dir, store) = store();
        let mk = || {
            store.create_preset(
                PresetId("p1".into()),
                "x".into(),
                String::new(),
                "2026-06-24T00:00:00Z".into(),
            )
        };
        mk().unwrap();
        assert!(matches!(mk().unwrap_err(), StoreError::Integrity(_)));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-preset store::`
Expected: FAIL — `PresetStore` not found.

- [ ] **Step 4: Implement `PresetStore` (above the `#[cfg(test)]` block)**

```rust
/// File-based store for presets and immutable automation revisions, rooted at
/// an injected directory.
pub struct PresetStore {
    root: PathBuf,
}

fn validate_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(StoreError::Integrity(format!("invalid id: {id:?}")))
    }
}

impl PresetStore {
    /// Open a store rooted at `root`. Performs no filesystem access.
    pub fn open(root: PathBuf) -> Self {
        Self { root }
    }

    fn presets_dir(&self) -> PathBuf {
        self.root.join("presets")
    }

    fn preset_dir(&self, id: &PresetId) -> PathBuf {
        self.presets_dir().join(&id.0)
    }

    fn preset_json(&self, id: &PresetId) -> PathBuf {
        self.preset_dir(id).join("preset.json")
    }

    /// Create a new preset with no active revision. Errors if a preset with
    /// this id already exists or the id is unsafe as a path component.
    pub fn create_preset(
        &self,
        id: PresetId,
        name: String,
        original_intent: String,
        now: String,
    ) -> Result<Preset> {
        validate_id(&id.0)?;
        if self.preset_json(&id).exists() {
            return Err(StoreError::Integrity(format!(
                "preset already exists: {}",
                id.0
            )));
        }
        let preset = Preset {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: id.clone(),
            name,
            original_intent,
            active_revision_id: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let bytes = serde_json::to_vec_pretty(&preset)?;
        io::write_atomic(&self.preset_json(&id), &bytes)?;
        Ok(preset)
    }

    /// Load a preset's metadata.
    pub fn load_preset(&self, id: &PresetId) -> Result<Preset> {
        let path = self.preset_json(id);
        match io::read_optional_bytes(&path)? {
            None => Err(StoreError::NotFound {
                kind: EntityKind::Preset,
                id: id.0.clone(),
            }),
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt {
                path,
                detail: e.to_string(),
            }),
        }
    }

    /// List all presets as summaries, sorted by id. Directories without a
    /// readable `preset.json` are skipped.
    pub fn list_presets(&self) -> Result<Vec<PresetSummary>> {
        let dir = self.presets_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(StoreError::Io { path: dir, source }),
        };

        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| StoreError::Io {
                path: dir.clone(),
                source,
            })?;
            let is_dir = entry
                .file_type()
                .map_err(|source| StoreError::Io {
                    path: entry.path(),
                    source,
                })?
                .is_dir();
            if !is_dir {
                continue;
            }
            let id = PresetId(entry.file_name().to_string_lossy().into_owned());
            match self.load_preset(&id) {
                Ok(p) => out.push(PresetSummary {
                    id: p.id,
                    name: p.name,
                    active_revision_id: p.active_revision_id,
                    updated_at: p.updated_at,
                }),
                Err(StoreError::NotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        out.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(out)
    }
}
```

Note: `AutomationRevision`, `RevisionId`, `RevisionSummary`, and `io::{read_optional_bytes via NotFound}` are imported now but only fully exercised in Tasks 5–6. If clippy flags an unused import for `AutomationRevision`/`RevisionId`/`RevisionSummary` in this task, narrow the `use crate::domain::{...}` line to only `{Preset, PresetId, PresetSummary, STORE_SCHEMA_VERSION}` here and widen it in Task 5.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-preset store::`
Expected: PASS (5 tests).

- [ ] **Step 6: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-preset
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
git add crates/rollshot-preset/src/store.rs crates/rollshot-preset/src/lib.rs
git commit -m "feat(preset): PresetStore create/load/list with id validation"
```

---

### Task 5: Immutable revisions — `add_revision`, `load_revision`, `list_revisions`

**Files:**
- Modify: `crates/rollshot-preset/src/store.rs`

**Interfaces:**
- Consumes: `rollshot_automation::{ValidatedAutomation, ensure_compatible}`; `RevisionProvenance`; domain/io/error from prior tasks.
- Produces:
  - `add_revision(&self, preset_id: &PresetId, id: RevisionId, parent_id: Option<RevisionId>, artifact: ValidatedAutomation, provenance: RevisionProvenance, now: String) -> Result<AutomationRevision>`
  - `load_revision(&self, preset_id: &PresetId, rev_id: &RevisionId) -> Result<AutomationRevision>`
  - `list_revisions(&self, preset_id: &PresetId) -> Result<Vec<RevisionSummary>>`
  - path helpers `revisions_dir`, `revision_json`.

- [ ] **Step 1: Write the failing tests (add to the `tests` module in `store.rs`)**

```rust
    use rollshot_automation::{validate_source, ValidatedAutomation, ValidationLimits};

    const SAMPLE_SOURCE: &str = r#"function expandBounds(rect, padding) {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}

function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 10 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: expandBounds(match.bounds, 8),
      confidence: match.confidence,
      label: "ocr-match",
    })),
  };
}
"#;

    fn sample_artifact() -> ValidatedAutomation {
        validate_source(SAMPLE_SOURCE, &ValidationLimits::default()).unwrap()
    }

    fn provenance() -> crate::domain::RevisionProvenance {
        crate::domain::RevisionProvenance {
            origin: crate::domain::RevisionOrigin::AgentRun,
            note: None,
            source_run_ref: None,
        }
    }

    fn seeded() -> (tempfile::TempDir, PresetStore) {
        let (dir, store) = store();
        store
            .create_preset(
                PresetId("p1".into()),
                "p".into(),
                String::new(),
                "2026-06-24T00:00:00Z".into(),
            )
            .unwrap();
        (dir, store)
    }

    #[test]
    fn add_then_load_revision_does_not_activate() {
        let (_dir, store) = seeded();
        let rev = store
            .add_revision(
                &PresetId("p1".into()),
                RevisionId("r1".into()),
                None,
                sample_artifact(),
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
            .unwrap();

        let loaded = store
            .load_revision(&PresetId("p1".into()), &RevisionId("r1".into()))
            .unwrap();
        assert_eq!(loaded, rev);

        // add_revision must NOT change the active pointer.
        let preset = store.load_preset(&PresetId("p1".into())).unwrap();
        assert_eq!(preset.active_revision_id, None);
    }

    #[test]
    fn add_revision_to_missing_preset_is_not_found() {
        let (_dir, store) = store();
        let err = store
            .add_revision(
                &PresetId("ghost".into()),
                RevisionId("r1".into()),
                None,
                sample_artifact(),
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            StoreError::NotFound {
                kind: EntityKind::Preset,
                ..
            }
        ));
    }

    #[test]
    fn duplicate_revision_id_is_rejected() {
        let (_dir, store) = seeded();
        let add = || {
            store.add_revision(
                &PresetId("p1".into()),
                RevisionId("r1".into()),
                None,
                sample_artifact(),
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
        };
        add().unwrap();
        assert!(matches!(add().unwrap_err(), StoreError::RevisionExists(_)));
    }

    #[test]
    fn load_missing_revision_is_not_found() {
        let (_dir, store) = seeded();
        let err = store
            .load_revision(&PresetId("p1".into()), &RevisionId("absent".into()))
            .unwrap_err();
        assert!(matches!(
            err,
            StoreError::NotFound {
                kind: EntityKind::Revision,
                ..
            }
        ));
    }

    #[test]
    fn list_revisions_returns_summaries() {
        let (_dir, store) = seeded();
        for id in ["r2", "r1"] {
            store
                .add_revision(
                    &PresetId("p1".into()),
                    RevisionId(id.into()),
                    None,
                    sample_artifact(),
                    provenance(),
                    "2026-06-24T00:01:00Z".into(),
                )
                .unwrap();
        }
        let ids: Vec<String> = store
            .list_revisions(&PresetId("p1".into()))
            .unwrap()
            .into_iter()
            .map(|s| s.id.0)
            .collect();
        assert_eq!(ids, vec!["r1".to_string(), "r2".to_string()]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-preset store::`
Expected: FAIL — `add_revision` / `load_revision` / `list_revisions` not found.

- [ ] **Step 3: Implement the revision methods (inside `impl PresetStore`)**

Ensure the `use rollshot_automation::...` import is present at the top of `store.rs`:

```rust
use rollshot_automation::{ensure_compatible, ValidatedAutomation};
```

Add the path helpers and methods inside `impl PresetStore`:

```rust
    fn revisions_dir(&self, id: &PresetId) -> PathBuf {
        self.preset_dir(id).join("revisions")
    }

    fn revision_json(&self, preset_id: &PresetId, rev_id: &RevisionId) -> PathBuf {
        self.revisions_dir(preset_id)
            .join(format!("{}.json", rev_id.0))
    }

    /// Append a new immutable revision. Does NOT change the active pointer.
    /// Errors if the preset is missing, the revision id already exists, or the
    /// id is unsafe as a path component.
    pub fn add_revision(
        &self,
        preset_id: &PresetId,
        id: RevisionId,
        parent_id: Option<RevisionId>,
        artifact: ValidatedAutomation,
        provenance: crate::domain::RevisionProvenance,
        now: String,
    ) -> Result<AutomationRevision> {
        validate_id(&id.0)?;
        // Preset must exist.
        let _ = self.load_preset(preset_id)?;
        let path = self.revision_json(preset_id, &id);
        if path.exists() {
            return Err(StoreError::RevisionExists(id.0.clone()));
        }
        let revision = AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: id.clone(),
            preset_id: preset_id.clone(),
            parent_id,
            created_at: now,
            provenance,
            artifact,
        };
        let bytes = serde_json::to_vec_pretty(&revision)?;
        io::write_atomic(&path, &bytes)?;
        Ok(revision)
    }

    /// Load a revision and revalidate its artifact against the installed
    /// automation schemas before returning.
    pub fn load_revision(
        &self,
        preset_id: &PresetId,
        rev_id: &RevisionId,
    ) -> Result<AutomationRevision> {
        let path = self.revision_json(preset_id, rev_id);
        let bytes = io::read_optional_bytes(&path)?.ok_or_else(|| StoreError::NotFound {
            kind: EntityKind::Revision,
            id: rev_id.0.clone(),
        })?;
        let revision: AutomationRevision =
            serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt {
                path: path.clone(),
                detail: e.to_string(),
            })?;
        ensure_compatible(&revision.artifact)?;
        Ok(revision)
    }

    /// List a preset's revisions as summaries, sorted by id. Does not
    /// revalidate artifacts. Errors if the preset is missing.
    pub fn list_revisions(&self, preset_id: &PresetId) -> Result<Vec<RevisionSummary>> {
        let _ = self.load_preset(preset_id)?;
        let dir = self.revisions_dir(preset_id);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(StoreError::Io { path: dir, source }),
        };

        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| StoreError::Io {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue; // skip `.tmp` and anything else
            }
            let bytes = match io::read_optional_bytes(&path)? {
                Some(b) => b,
                None => continue,
            };
            let revision: AutomationRevision =
                serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt {
                    path: path.clone(),
                    detail: e.to_string(),
                })?;
            out.push(RevisionSummary {
                id: revision.id,
                parent_id: revision.parent_id,
                created_at: revision.created_at,
            });
        }
        out.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(out)
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-preset store::`
Expected: PASS (5 prior + 5 new = 10 store tests).

- [ ] **Step 5: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-preset
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
git add crates/rollshot-preset/src/store.rs
git commit -m "feat(preset): immutable revisions with revalidate-on-load"
```

---

### Task 6: Active selection and preset lifecycle — `set_active_revision`, `load_active_revision`, `rename_preset`, `delete_preset`

**Files:**
- Modify: `crates/rollshot-preset/src/store.rs`

**Interfaces:**
- Consumes: `io::lock_dir`; prior store methods.
- Produces:
  - `set_active_revision(&self, preset_id: &PresetId, rev_id: &RevisionId, now: String) -> Result<()>`
  - `load_active_revision(&self, preset_id: &PresetId) -> Result<AutomationRevision>`
  - `rename_preset(&self, preset_id: &PresetId, new_name: String, now: String) -> Result<()>`
  - `delete_preset(&self, id: &PresetId) -> Result<()>`

(Note: `set_active_revision` takes `now` so it can bump `updated_at`; this refines the spec §7 signature, which omitted it.)

- [ ] **Step 1: Write the failing tests (add to the `tests` module in `store.rs`)**

```rust
    #[test]
    fn activate_then_load_active() {
        let (_dir, store) = seeded();
        store
            .add_revision(
                &PresetId("p1".into()),
                RevisionId("r1".into()),
                None,
                sample_artifact(),
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
            .unwrap();
        store
            .set_active_revision(
                &PresetId("p1".into()),
                &RevisionId("r1".into()),
                "2026-06-24T00:02:00Z".into(),
            )
            .unwrap();

        let preset = store.load_preset(&PresetId("p1".into())).unwrap();
        assert_eq!(preset.active_revision_id, Some(RevisionId("r1".into())));
        assert_eq!(preset.updated_at, "2026-06-24T00:02:00Z");

        let active = store
            .load_active_revision(&PresetId("p1".into()))
            .unwrap();
        assert_eq!(active.id, RevisionId("r1".into()));
    }

    #[test]
    fn activate_missing_revision_is_integrity_error() {
        let (_dir, store) = seeded();
        let err = store
            .set_active_revision(
                &PresetId("p1".into()),
                &RevisionId("ghost".into()),
                "2026-06-24T00:02:00Z".into(),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::Integrity(_)));
    }

    #[test]
    fn load_active_without_selection_is_integrity_error() {
        let (_dir, store) = seeded();
        let err = store
            .load_active_revision(&PresetId("p1".into()))
            .unwrap_err();
        assert!(matches!(err, StoreError::Integrity(_)));
    }

    #[test]
    fn rename_updates_name_and_timestamp() {
        let (_dir, store) = seeded();
        store
            .rename_preset(
                &PresetId("p1".into()),
                "Renamed".into(),
                "2026-06-24T00:03:00Z".into(),
            )
            .unwrap();
        let preset = store.load_preset(&PresetId("p1".into())).unwrap();
        assert_eq!(preset.name, "Renamed");
        assert_eq!(preset.updated_at, "2026-06-24T00:03:00Z");
    }

    #[test]
    fn delete_removes_preset() {
        let (_dir, store) = seeded();
        store.delete_preset(&PresetId("p1".into())).unwrap();
        assert!(matches!(
            store.load_preset(&PresetId("p1".into())).unwrap_err(),
            StoreError::NotFound { .. }
        ));
        assert!(matches!(
            store.delete_preset(&PresetId("p1".into())).unwrap_err(),
            StoreError::NotFound { .. }
        ));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-preset store::`
Expected: FAIL — `set_active_revision` / `load_active_revision` / `rename_preset` / `delete_preset` not found.

- [ ] **Step 3: Implement the methods (inside `impl PresetStore`)**

```rust
    /// Set the active revision. Errors if the preset is missing or the target
    /// revision does not exist for this preset. Serialized via a directory lock.
    pub fn set_active_revision(
        &self,
        preset_id: &PresetId,
        rev_id: &RevisionId,
        now: String,
    ) -> Result<()> {
        if !self.preset_json(preset_id).exists() {
            return Err(StoreError::NotFound {
                kind: EntityKind::Preset,
                id: preset_id.0.clone(),
            });
        }
        let _lock = io::lock_dir(&self.preset_dir(preset_id))?;
        let mut preset = self.load_preset(preset_id)?;
        if !self.revision_json(preset_id, rev_id).exists() {
            return Err(StoreError::Integrity(format!(
                "revision {} not found for preset {}",
                rev_id.0, preset_id.0
            )));
        }
        preset.active_revision_id = Some(rev_id.clone());
        preset.updated_at = now;
        let bytes = serde_json::to_vec_pretty(&preset)?;
        io::write_atomic(&self.preset_json(preset_id), &bytes)?;
        Ok(())
    }

    /// Load and revalidate the active revision. Errors if no revision is active.
    pub fn load_active_revision(&self, preset_id: &PresetId) -> Result<AutomationRevision> {
        let preset = self.load_preset(preset_id)?;
        let rev_id = preset.active_revision_id.ok_or_else(|| {
            StoreError::Integrity(format!("preset {} has no active revision", preset_id.0))
        })?;
        self.load_revision(preset_id, &rev_id)
    }

    /// Rename a preset. Serialized via a directory lock.
    pub fn rename_preset(
        &self,
        preset_id: &PresetId,
        new_name: String,
        now: String,
    ) -> Result<()> {
        if !self.preset_json(preset_id).exists() {
            return Err(StoreError::NotFound {
                kind: EntityKind::Preset,
                id: preset_id.0.clone(),
            });
        }
        let _lock = io::lock_dir(&self.preset_dir(preset_id))?;
        let mut preset = self.load_preset(preset_id)?;
        preset.name = new_name;
        preset.updated_at = now;
        let bytes = serde_json::to_vec_pretty(&preset)?;
        io::write_atomic(&self.preset_json(preset_id), &bytes)?;
        Ok(())
    }

    /// Delete a preset and all its revisions.
    pub fn delete_preset(&self, id: &PresetId) -> Result<()> {
        if !self.preset_json(id).exists() {
            return Err(StoreError::NotFound {
                kind: EntityKind::Preset,
                id: id.0.clone(),
            });
        }
        let dir = self.preset_dir(id);
        std::fs::remove_dir_all(&dir).map_err(|source| StoreError::Io { path: dir, source })?;
        Ok(())
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-preset store::`
Expected: PASS (10 prior + 5 new = 15 store tests).

- [ ] **Step 5: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-preset
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
git add crates/rollshot-preset/src/store.rs
git commit -m "feat(preset): active-revision selection and preset lifecycle"
```

---

### Task 7: Revalidate-on-load failure cases (tamper, schema bump, corrupt JSON)

**Files:**
- Modify: `crates/rollshot-preset/src/store.rs` (tests only)

**Interfaces:**
- Consumes: existing `add_revision` / `load_revision`; `serde_json::Value` for on-disk mutation.
- Produces: no new public API; verifies `StoreError::Incompatible` and `StoreError::Corrupt` paths.

- [ ] **Step 1: Write the failing tests (add to the `tests` module in `store.rs`)**

```rust
    /// Path to the revision file (mirrors the store's internal layout).
    fn revision_path(dir: &std::path::Path, preset: &str, rev: &str) -> PathBuf {
        dir.join("presets")
            .join(preset)
            .join("revisions")
            .join(format!("{rev}.json"))
    }

    #[test]
    fn tampered_source_is_incompatible() {
        let (dir, store) = seeded();
        store
            .add_revision(
                &PresetId("p1".into()),
                RevisionId("r1".into()),
                None,
                sample_artifact(),
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
            .unwrap();

        // Corrupt the stored canonical source so a fresh validation no longer
        // matches the stored struct.
        let path = revision_path(dir.path(), "p1", "r1");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["artifact"]["source"] = serde_json::Value::String("@@@ not javascript @@@".into());
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let err = store
            .load_revision(&PresetId("p1".into()), &RevisionId("r1".into()))
            .unwrap_err();
        assert!(matches!(err, StoreError::Incompatible(_)));
    }

    #[test]
    fn stale_schema_version_is_incompatible() {
        let (dir, store) = seeded();
        store
            .add_revision(
                &PresetId("p1".into()),
                RevisionId("r1".into()),
                None,
                sample_artifact(),
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
            .unwrap();

        let path = revision_path(dir.path(), "p1", "r1");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        // LanguageSchemaVersion serializes as its inner u16.
        value["artifact"]["language_schema_version"] = serde_json::json!(999);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let err = store
            .load_revision(&PresetId("p1".into()), &RevisionId("r1".into()))
            .unwrap_err();
        assert!(matches!(err, StoreError::Incompatible(_)));
    }

    #[test]
    fn corrupt_revision_json_is_corrupt_error() {
        let (dir, store) = seeded();
        store
            .add_revision(
                &PresetId("p1".into()),
                RevisionId("r1".into()),
                None,
                sample_artifact(),
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
            .unwrap();

        let path = revision_path(dir.path(), "p1", "r1");
        std::fs::write(&path, b"{ not valid json").unwrap();

        let err = store
            .load_revision(&PresetId("p1".into()), &RevisionId("r1".into()))
            .unwrap_err();
        assert!(matches!(err, StoreError::Corrupt { .. }));
    }
```

- [ ] **Step 2: Run the tests**

Run: `rtk cargo test -p rollshot-preset store::`
Expected: PASS. These exercise existing code (no implementation change). If `tampered_source_is_incompatible` or `stale_schema_version_is_incompatible` fail, the bug is in how `load_revision` propagates `ensure_compatible`'s error — confirm `ensure_compatible(&revision.artifact)?` is present and `StoreError::Incompatible(#[from] CompatibilityError)` is wired.

- [ ] **Step 3: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-preset
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
git add crates/rollshot-preset/src/store.rs
git commit -m "test(preset): revalidate-on-load tamper, schema, and corruption cases"
```

---

### Task 8: Unify the config-root resolver on etcetera (shared with the daemon)

**Files:**
- Modify: `Cargo.toml` (add `etcetera` to `[workspace.dependencies]`)
- Modify: `crates/rollshot-app/Cargo.toml` (add `etcetera = { workspace = true }`)
- Modify: `crates/rollshot-app/src/daemon/config.rs:166-170` (`rollshot_config_dir`)

**Interfaces:**
- Consumes: `etcetera::base_strategy::{choose_base_strategy, BaseStrategy}`.
- Produces: unchanged signature `rollshot_config_dir() -> Result<PathBuf, String>`, now resolving via the XDG strategy. SP6 will build the preset root as `rollshot_config_dir()?.join("presets")`.

- [ ] **Step 1: Add `etcetera` to the workspace dependencies**

In `Cargo.toml` `[workspace.dependencies]`, add (alphabetically near the top is fine):

```toml
etcetera = "0.11"
```

- [ ] **Step 2: Add `etcetera` to `rollshot-app`**

In `crates/rollshot-app/Cargo.toml`, under `[dependencies]`, add:

```toml
etcetera = { workspace = true }
```

- [ ] **Step 3: Write the failing test in `crates/rollshot-app/src/daemon/config.rs`**

Add to the existing `#[cfg(test)] mod tests` block in that file:

```rust
    #[test]
    fn rollshot_config_dir_resolves_under_rollshot() {
        let dir = super::rollshot_config_dir().expect("config dir resolves");
        assert!(dir.ends_with("rollshot"), "got {dir:?}");
    }
```

(If the test module imports `use super::*;`, call `rollshot_config_dir()` directly instead of `super::rollshot_config_dir()`.)

- [ ] **Step 4: Run the test to verify current behavior, then change the implementation**

Run: `rtk cargo test -p rollshot-app config::tests::rollshot_config_dir_resolves_under_rollshot`
Expected: PASS already (the `dirs` implementation also ends with `rollshot`). This test is a guard that survives the swap.

Replace `rollshot_config_dir` (currently `crates/rollshot-app/src/daemon/config.rs:166-170`):

```rust
pub fn rollshot_config_dir() -> Result<PathBuf, String> {
    use etcetera::base_strategy::{choose_base_strategy, BaseStrategy};
    choose_base_strategy()
        .map(|strategy| strategy.config_dir().join("rollshot"))
        .map_err(|error| format!("platform configuration directory is unavailable: {error}"))
}
```

- [ ] **Step 5: Run the test and the daemon-config suite to verify they pass**

Run: `rtk cargo test -p rollshot-app config::`
Expected: PASS — the guard test passes and existing `load_from`-based config tests are unaffected (they pass explicit paths).

- [ ] **Step 6: Format, lint, commit**

```bash
rtk cargo fmt -p rollshot-app
rtk cargo clippy -p rollshot-app --all-targets -- -D warnings
git add Cargo.toml crates/rollshot-app/Cargo.toml crates/rollshot-app/src/daemon/config.rs
git commit -m "refactor(app): resolve rollshot config dir via etcetera XDG strategy"
```

---

### Task 9: Workspace verification gate

**Files:** none (verification only).

- [ ] **Step 1: Full crate gate**

Run:
```bash
rtk cargo test -p rollshot-preset
rtk cargo fmt --check
rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings
```
Expected: all PASS; `rollshot-preset` has ~19 tests (1 lib + 4 io + 15 store... adjust count to actual).

- [ ] **Step 2: Workspace-wide gate (catches the resolver change's blast radius)**

Run:
```bash
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
```
Expected: all PASS. If a `dirs`-import-now-unused warning appears in `crates/rollshot-app/src/daemon/config.rs`, confirm `dirs::config_dir()` is no longer referenced there; `dirs` remains used elsewhere in `rollshot-app` (e.g. `storage.rs`), so the crate dependency stays.

- [ ] **Step 3: Final commit (if any fmt fixups were needed)**

```bash
git add -A
git commit -m "chore(preset): workspace fmt/clippy pass" || echo "nothing to commit"
```

---

## Self-Review

**1. Spec coverage:**

| Spec section | Task(s) |
|---|---|
| §2.1 Preset record | Task 1 (domain), Task 4 (CRUD) |
| §2.1 immutable AutomationRevision | Task 1 (domain), Task 5 |
| §2.1 active-revision selection + integrity | Task 6 |
| §2.1 revalidate via `ensure_compatible` | Task 5 (impl), Task 7 (failure cases) |
| §2.1 atomic, crash-safe writes | Task 2 |
| §2.1 concurrency (advisory lock) | Task 3, used in Task 6 |
| §2.1 typed error model | Task 1 (`StoreError`) |
| §4 crate boundary (automation-only dep, injected root) | Task 1 (Cargo.toml), Global Constraints |
| §5 caller-supplied ids/timestamps, `store_schema_version` | Task 1, Task 4 |
| §6 on-disk layout | Task 4 (paths), Task 5 (revision paths) |
| §6 shared etcetera resolver | Task 8 |
| §7 store API | Tasks 4–6 |
| §8 revalidate-on-load | Tasks 5, 7 |
| §9 atomic write / ordering / lock | Tasks 2, 3, 6 |
| §10 error model | Task 1, exercised across 4–7 |
| §11 privacy (no extra persistence, no payload logging) | satisfied by design; no tracing added |
| §13 verification (round-trip, immutability, integrity, revalidate, atomic, listing) | Tasks 1–7 |
| §14 out of scope (sessions, migration, UI) | not implemented, by design |

No spec requirement is left without a task.

**2. Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N"/"write tests for the above" — every code and test step contains complete code. Path-traversal, immutability, integrity, and revalidation each have concrete test bodies.

**3. Type consistency:** `PresetId`/`RevisionId` are `(pub String)` newtypes used consistently; `set_active_revision`/`rename_preset`/`create_preset`/`add_revision` all take `now: String`; `load_revision`/`load_active_revision` both call `ensure_compatible`; `StoreError` variants referenced in tests (`NotFound{kind,..}`, `Integrity`, `RevisionExists`, `Incompatible`, `Corrupt`) all match `error.rs`. `RevisionProvenance`/`RevisionOrigin` field and variant names match between `domain.rs` and the test helpers. `write_atomic`'s `<file>.tmp` naming matches the `.json`-only extension filter in `list_revisions`.
