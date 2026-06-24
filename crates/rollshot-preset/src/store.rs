use std::path::PathBuf;

use rollshot_automation::{ensure_compatible, ValidatedAutomation};

use crate::domain::{
    AutomationRevision, Preset, PresetId, PresetSummary, RevisionId, RevisionSummary,
    STORE_SCHEMA_VERSION,
};
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

    fn revisions_dir(&self, id: &PresetId) -> PathBuf {
        self.preset_dir(id).join("revisions")
    }

    fn revision_json(&self, preset_id: &PresetId, rev_id: &RevisionId) -> PathBuf {
        self.revisions_dir(preset_id)
            .join(format!("{}.json", rev_id.0))
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
        ensure_compatible(&artifact)?;
        let _lock = io::lock_dir(&self.preset_dir(preset_id))?;
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
        ensure_store_schema(path, revision.store_schema_version)?;
        ensure_compatible(&revision.artifact)?;
        Ok(revision)
    }

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
        let _ = self.load_revision(preset_id, rev_id)?;
        preset.active_revision_id = Some(rev_id.clone());
        preset.updated_at = now;
        let bytes = serde_json::to_vec_pretty(&preset)?;
        io::write_atomic(&self.preset_json(preset_id), &bytes)?;
        Ok(())
    }

    pub fn load_active_revision(&self, preset_id: &PresetId) -> Result<AutomationRevision> {
        let preset = self.load_preset(preset_id)?;
        let rev_id = preset.active_revision_id.ok_or_else(|| {
            StoreError::Integrity(format!("preset {} has no active revision", preset_id.0))
        })?;
        self.load_revision(preset_id, &rev_id)
    }

    pub fn rename_preset(&self, preset_id: &PresetId, new_name: String, now: String) -> Result<()> {
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

    pub fn delete_preset(&self, id: &PresetId) -> Result<()> {
        if !self.preset_json(id).exists() {
            return Err(StoreError::NotFound {
                kind: EntityKind::Preset,
                id: id.0.clone(),
            });
        }
        let _lock = io::lock_dir(&self.preset_dir(id))?;
        let dir = self.preset_dir(id);
        std::fs::remove_dir_all(&dir).map_err(|source| StoreError::Io { path: dir, source })?;
        Ok(())
    }

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
                continue;
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
            ensure_store_schema(path, revision.store_schema_version)?;
            out.push(RevisionSummary {
                id: revision.id,
                parent_id: revision.parent_id,
                created_at: revision.created_at,
            });
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

    fn revision_path(dir: &std::path::Path, preset: &str, rev: &str) -> PathBuf {
        dir.join("presets")
            .join(preset)
            .join("revisions")
            .join(format!("{rev}.json"))
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

    #[test]
    fn add_revision_rejects_incompatible_artifact() {
        let (_dir, store) = seeded();
        let mut artifact = sample_artifact();
        artifact.source = "@@@ not javascript @@@".into();

        let err = store
            .add_revision(
                &PresetId("p1".into()),
                RevisionId("r1".into()),
                None,
                artifact,
                provenance(),
                "2026-06-24T00:01:00Z".into(),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::Incompatible(_)));
    }

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

        let active = store.load_active_revision(&PresetId("p1".into())).unwrap();
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
    fn activate_incompatible_revision_is_rejected() {
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
        value["artifact"]["source"] = serde_json::Value::String("@@@ not javascript @@@".into());
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let err = store
            .set_active_revision(
                &PresetId("p1".into()),
                &RevisionId("r1".into()),
                "2026-06-24T00:02:00Z".into(),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::Incompatible(_)));

        let preset = store.load_preset(&PresetId("p1".into())).unwrap();
        assert_eq!(preset.active_revision_id, None);
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
        value["artifact"]["language_schema_version"] = serde_json::json!(999);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let err = store
            .load_revision(&PresetId("p1".into()), &RevisionId("r1".into()))
            .unwrap_err();
        assert!(matches!(err, StoreError::Incompatible(_)));
    }

    #[test]
    fn unsupported_revision_store_schema_is_rejected() {
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
        value["store_schema_version"] = serde_json::json!(999);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let err = store
            .load_revision(&PresetId("p1".into()), &RevisionId("r1".into()))
            .unwrap_err();
        assert!(matches!(err, StoreError::UnsupportedStoreSchema { .. }));
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
}
