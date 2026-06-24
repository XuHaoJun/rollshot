use std::path::PathBuf;

use crate::domain::{Preset, PresetId, PresetSummary, STORE_SCHEMA_VERSION};
use crate::error::{EntityKind, Result, StoreError};
use crate::io;

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

fn ensure_store_schema(path: PathBuf, found: u16) -> Result<()> {
    if found == STORE_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(StoreError::UnsupportedStoreSchema {
            path,
            expected: STORE_SCHEMA_VERSION,
            found,
        })
    }
}

impl PresetStore {
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

    pub fn create_preset(
        &self,
        id: PresetId,
        name: String,
        original_intent: String,
        now: String,
    ) -> Result<Preset> {
        validate_id(&id.0)?;
        let _lock = io::lock_dir(&self.preset_dir(&id))?;
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

    pub fn load_preset(&self, id: &PresetId) -> Result<Preset> {
        let path = self.preset_json(id);
        match io::read_optional_bytes(&path)? {
            None => Err(StoreError::NotFound {
                kind: EntityKind::Preset,
                id: id.0.clone(),
            }),
            Some(bytes) => {
                let preset: Preset =
                    serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt {
                        path: path.clone(),
                        detail: e.to_string(),
                    })?;
                ensure_store_schema(path, preset.store_schema_version)?;
                Ok(preset)
            }
        }
    }

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

    #[test]
    fn unsupported_preset_store_schema_is_rejected() {
        let (dir, store) = store();
        store
            .create_preset(
                PresetId("p1".into()),
                "x".into(),
                String::new(),
                "2026-06-24T00:00:00Z".into(),
            )
            .unwrap();
        let path = dir.path().join("presets").join("p1").join("preset.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["store_schema_version"] = serde_json::json!(999);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        assert!(matches!(
            store.load_preset(&PresetId("p1".into())).unwrap_err(),
            StoreError::UnsupportedStoreSchema { .. }
        ));
    }
}
