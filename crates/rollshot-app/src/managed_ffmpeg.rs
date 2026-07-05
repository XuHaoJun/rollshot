#![allow(dead_code)] // scaffolding: consumed by later Action Guide MP4 tasks

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedFfmpegMetadata {
    pub platform: &'static str,
    pub version: &'static str,
    pub source_url: &'static str,
    pub license: &'static str,
    pub license_url: &'static str,
    pub archive_size: u64,
    pub archive_sha256: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManagedFfmpegManifest {
    pub schema_version: u32,
    pub platform: String,
    pub version: String,
    pub source_url: String,
    pub license: String,
    pub license_url: String,
    pub archive_sha256: String,
    pub archive_size: u64,
    pub binary_path: PathBuf,
    pub ffmpeg_version_line: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FfmpegResolution {
    Available(PathBuf),
    NeedsSetup(FfmpegSetupInfo),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfmpegSetupInfo {
    pub managed_download: Option<ManagedFfmpegMetadata>,
    pub install_location: PathBuf,
}

pub(crate) const LINUX_X86_64_METADATA: ManagedFfmpegMetadata = ManagedFfmpegMetadata {
    platform: "linux-x86_64",
    version: "6.0.1",
    source_url: "https://johnvansickle.com/ffmpeg/old-releases/ffmpeg-6.0.1-amd64-static.tar.xz",
    license: "GPLv3",
    license_url: "https://www.gnu.org/licenses/gpl-3.0.html",
    archive_size: 41_164_188,
    archive_sha256: "28268bf402f1083833ea269331587f60a242848880073be8016501d864bd07a5",
};

pub(crate) fn pinned_metadata_for_current_platform() -> Option<ManagedFfmpegMetadata> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(LINUX_X86_64_METADATA)
    } else {
        None
    }
}

pub(crate) fn resolve_ffmpeg() -> FfmpegResolution {
    if let Some(path) = std::env::var_os("ROLLSHOT_FFMPEG").map(PathBuf::from) {
        if validate_ffmpeg(&path).is_ok() {
            return FfmpegResolution::Available(path);
        }
    }

    let binary = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if let Some(path) = find_on_path(binary) {
        return FfmpegResolution::Available(path);
    }

    let root = managed_root().unwrap_or_else(|_| std::env::temp_dir().join("rollshot/ffmpeg"));
    if let Ok(manifest) = load_manifest(&root) {
        if manifest.schema_version == 1
            && pinned_metadata_for_current_platform()
                .is_some_and(|meta| manifest.platform == meta.platform)
            && validate_ffmpeg(&manifest.binary_path).is_ok()
        {
            return FfmpegResolution::Available(manifest.binary_path);
        }
    }

    FfmpegResolution::NeedsSetup(FfmpegSetupInfo {
        managed_download: pinned_metadata_for_current_platform(),
        install_location: root,
    })
}

fn managed_root() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ROLLSHOT_FFMPEG_ROOT").map(PathBuf::from) {
        return Ok(path);
    }
    crate::daemon::config::rollshot_config_dir().map(|dir| dir.join("ffmpeg"))
}

pub(crate) fn manifest_path(root: &Path) -> PathBuf {
    root.join("managed-ffmpeg.json")
}

pub(crate) fn managed_binary_path(root: &Path) -> PathBuf {
    let mut path = root.join("bin").join("ffmpeg");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

pub(crate) fn validate_ffmpeg(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("FFmpeg does not exist at {}", path.display()));
    }
    let output = Command::new(path)
        .arg("-version")
        .output()
        .map_err(|error| format!("failed to run FFmpeg at {}: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "FFmpeg at {} exited with {}",
            path.display(),
            output.status
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().next().unwrap_or("ffmpeg").to_string())
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(binary);
        if validate_ffmpeg(&candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn load_manifest(root: &Path) -> Result<ManagedFfmpegManifest, String> {
    let path = manifest_path(root);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read managed FFmpeg manifest: {error}"))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse managed FFmpeg manifest: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_metadata_is_pinned_and_auditable() {
        let meta = LINUX_X86_64_METADATA;
        assert_eq!(meta.platform, "linux-x86_64");
        assert_eq!(meta.version, "6.0.1");
        assert!(meta.source_url.starts_with("https://johnvansickle.com/"));
        assert_eq!(meta.license, "GPLv3");
        assert_eq!(meta.archive_size, 41_164_188);
        assert_eq!(meta.archive_sha256.len(), 64);
    }

    #[test]
    fn manifest_round_trips_json() {
        let manifest = ManagedFfmpegManifest {
            schema_version: 1,
            platform: "linux-x86_64".to_string(),
            version: "6.0.1".to_string(),
            source_url: LINUX_X86_64_METADATA.source_url.to_string(),
            license: "GPLv3".to_string(),
            license_url: LINUX_X86_64_METADATA.license_url.to_string(),
            archive_sha256: LINUX_X86_64_METADATA.archive_sha256.to_string(),
            archive_size: LINUX_X86_64_METADATA.archive_size,
            binary_path: PathBuf::from("/tmp/ffmpeg"),
            ffmpeg_version_line: "ffmpeg version 6.0.1-static".to_string(),
            installed_at: "2026-07-05T00:00:00+00:00".to_string(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let restored: ManagedFfmpegManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, manifest);
    }

    #[test]
    fn managed_paths_are_stable_under_root() {
        let root = PathBuf::from("/tmp/rollshot-ffmpeg");
        assert_eq!(manifest_path(&root), root.join("managed-ffmpeg.json"));
        let binary = managed_binary_path(&root);
        assert!(binary.ends_with(if cfg!(windows) {
            Path::new("bin/ffmpeg.exe")
        } else {
            Path::new("bin/ffmpeg")
        }));
    }

    #[test]
    fn managed_root_can_be_overridden_for_tests() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_root = std::env::var_os("ROLLSHOT_FFMPEG_ROOT");
        let root = PathBuf::from("/tmp/rollshot-managed-test-root");
        std::env::set_var("ROLLSHOT_FFMPEG_ROOT", &root);
        assert_eq!(managed_root().unwrap(), root);
        match old_root {
            Some(value) => std::env::set_var("ROLLSHOT_FFMPEG_ROOT", value),
            None => std::env::remove_var("ROLLSHOT_FFMPEG_ROOT"),
        }
    }

    #[test]
    fn validate_ffmpeg_rejects_missing_path() {
        let result = validate_ffmpeg(Path::new("/definitely/missing/ffmpeg"));
        assert!(result.is_err());
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
