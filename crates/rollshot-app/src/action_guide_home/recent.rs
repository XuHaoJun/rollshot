use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 10;
const FILE_NAME: &str = "recent-action-guides.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecentFile {
    schema_version: u32,
    entries: Vec<RecentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub display_name: String,
    pub last_opened_ms: u64,
    #[serde(default = "default_available")]
    pub available: bool,
}

fn default_available() -> bool {
    true
}

pub struct RecentProjects {
    dir: PathBuf,
    entries: Vec<RecentEntry>,
}

impl RecentProjects {
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join(FILE_NAME);
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) => parse_recent_json(&text),
            Err(_) => {
                tracing::warn!(target: "rollshot::action_guide_home", "failed to read recent projects file, starting empty");
                Vec::new()
            }
        };
        Self {
            dir: config_dir.to_path_buf(),
            entries,
        }
    }

    pub fn entries(&self) -> &[RecentEntry] {
        &self.entries
    }

    pub fn record_open_at(&mut self, path: PathBuf, display_name: String, now_ms: u64) {
        self.entries.retain(|e| e.path != path);
        self.entries.insert(
            0,
            RecentEntry {
                path,
                display_name,
                last_opened_ms: now_ms,
                available: true,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }

    pub fn refresh_availability(&mut self) {
        for entry in &mut self.entries {
            entry.available = entry.path.exists();
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let file = RecentFile {
            schema_version: 1,
            entries: self.entries.clone(),
        };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| {
                tracing::error!(target: "rollshot::action_guide_home", error = %e, "failed to serialize recent projects");
                format!("failed to serialize recent projects: {e}")
            })?;

        std::fs::create_dir_all(&self.dir).map_err(|e| {
            tracing::error!(target: "rollshot::action_guide_home", error = %e, "failed to create config directory");
            format!("failed to create config directory: {e}")
        })?;

        let target = self.dir.join(FILE_NAME);
        let temp = self
            .dir
            .join(format!("{FILE_NAME}.tmp.{}", std::process::id()));

        std::fs::write(&temp, json.as_bytes()).map_err(|e| {
            tracing::error!(target: "rollshot::action_guide_home", error = %e, "failed to write temp recent file");
            format!("failed to write temp recent file: {e}")
        })?;

        let file = std::fs::File::open(&temp).map_err(|e| {
            tracing::error!(target: "rollshot::action_guide_home", error = %e, "failed to open temp recent file for sync");
            format!("failed to open temp recent file for sync: {e}")
        })?;
        file.sync_all().map_err(|e| {
            tracing::error!(target: "rollshot::action_guide_home", error = %e, "failed to sync temp recent file");
            format!("failed to sync temp recent file: {e}")
        })?;

        std::fs::rename(&temp, &target).map_err(|e| {
            tracing::error!(target: "rollshot::action_guide_home", error = %e, "failed to rename temp recent file");
            format!("failed to rename temp recent file: {e}")
        })?;

        if let Ok(dir_file) = std::fs::File::open(&self.dir) {
            let _ = dir_file.sync_all();
        }

        Ok(())
    }
}

fn parse_recent_json(text: &str) -> Vec<RecentEntry> {
    let file: RecentFile = match serde_json::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(target: "rollshot::action_guide_home", error = %e, "failed to parse recent projects file, starting empty");
            return Vec::new();
        }
    };
    if file.schema_version != 1 {
        tracing::warn!(target: "rollshot::action_guide_home", version = file.schema_version, "unsupported schema version, starting empty");
        return Vec::new();
    }
    file.entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        (dir, config_dir)
    }

    #[test]
    fn malformed_json_loads_empty() {
        let (_dir, config_dir) = setup();
        std::fs::write(config_dir.join(FILE_NAME), "{not valid json").unwrap();

        let recent = RecentProjects::load(&config_dir);
        assert!(recent.entries().is_empty());
    }

    #[test]
    fn unsupported_schema_version_loads_empty() {
        let (_dir, config_dir) = setup();
        let json = r#"{"schema_version": 99, "entries": [{"path": "/a", "display_name": "A", "last_opened_ms": 1, "available": true}]}"#;
        std::fs::write(config_dir.join(FILE_NAME), json).unwrap();

        let recent = RecentProjects::load(&config_dir);
        assert!(recent.entries().is_empty());
    }

    #[test]
    fn duplicate_path_moves_to_front() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(PathBuf::from("/a"), "A".into(), 1);
        recent.record_open_at(PathBuf::from("/b"), "B".into(), 2);
        recent.record_open_at(PathBuf::from("/a"), "A".into(), 3);

        assert_eq!(recent.entries().len(), 2);
        assert_eq!(recent.entries()[0].path, PathBuf::from("/a"));
        assert_eq!(recent.entries()[0].last_opened_ms, 3);
        assert_eq!(recent.entries()[1].path, PathBuf::from("/b"));
    }

    #[test]
    fn list_truncates_to_ten() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        for i in 0..12 {
            recent.record_open_at(
                PathBuf::from(format!("/project-{i}")),
                format!("Project {i}"),
                i as u64,
            );
        }

        assert_eq!(recent.entries().len(), MAX_ENTRIES);
        assert_eq!(recent.entries()[0].path, PathBuf::from("/project-11"));
    }

    #[test]
    fn missing_project_has_available_false_after_refresh() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(PathBuf::from("/nonexistent/path"), "Missing".into(), 1);
        recent.refresh_availability();

        assert!(!recent.entries()[0].available);
    }

    #[test]
    fn existing_project_stays_available_after_refresh() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(config_dir.clone(), "Config".into(), 1);
        recent.refresh_availability();

        assert!(recent.entries()[0].available);
    }

    #[test]
    fn display_name_is_only_title_content_stored() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(PathBuf::from("/a"), "My Project".into(), 1);

        assert_eq!(recent.entries()[0].display_name, "My Project");
    }

    #[test]
    fn save_creates_file_and_load_round_trips() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(PathBuf::from("/a"), "A".into(), 100);
        recent.save().unwrap();

        let loaded = RecentProjects::load(&config_dir);
        assert_eq!(loaded.entries().len(), 1);
        assert_eq!(loaded.entries()[0].path, PathBuf::from("/a"));
        assert_eq!(loaded.entries()[0].display_name, "A");
        assert_eq!(loaded.entries()[0].last_opened_ms, 100);
    }

    #[test]
    fn save_replaces_prior_content() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(PathBuf::from("/a"), "A".into(), 1);
        recent.save().unwrap();

        let original_content = std::fs::read_to_string(config_dir.join(FILE_NAME)).unwrap();

        let mut recent2 = RecentProjects::load(&config_dir);
        recent2.record_open_at(PathBuf::from("/b"), "B".into(), 2);
        recent2.save().unwrap();

        let new_content = std::fs::read_to_string(config_dir.join(FILE_NAME)).unwrap();
        assert_ne!(original_content, new_content);
        assert!(new_content.contains("/b"));
    }

    #[test]
    fn remove_entry() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(PathBuf::from("/a"), "A".into(), 1);
        recent.record_open_at(PathBuf::from("/b"), "B".into(), 2);
        recent.remove(&PathBuf::from("/a"));

        assert_eq!(recent.entries().len(), 1);
        assert_eq!(recent.entries()[0].path, PathBuf::from("/b"));
    }

    #[test]
    fn save_file_has_schema_version_one() {
        let (_dir, config_dir) = setup();
        let mut recent = RecentProjects::load(&config_dir);

        recent.record_open_at(PathBuf::from("/a"), "A".into(), 1);
        recent.save().unwrap();

        let text = std::fs::read_to_string(config_dir.join(FILE_NAME)).unwrap();
        let file: RecentFile = serde_json::from_str(&text).unwrap();
        assert_eq!(file.schema_version, 1);
    }
}
