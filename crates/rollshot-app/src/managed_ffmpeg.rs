#![allow(dead_code)] // scaffolding: consumed by later Action Guide MP4 tasks

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file =
        std::fs::File::open(path).map_err(|error| format!("failed to open archive: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|error| format!("failed to hash archive: {error}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn verify_archive_sha(path: &Path, expected: &str) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(format!(
            "managed FFmpeg sha256 mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

pub(crate) fn build_manifest(
    metadata: ManagedFfmpegMetadata,
    binary_path: PathBuf,
    ffmpeg_version_line: String,
) -> ManagedFfmpegManifest {
    ManagedFfmpegManifest {
        schema_version: 1,
        platform: metadata.platform.to_string(),
        version: metadata.version.to_string(),
        source_url: metadata.source_url.to_string(),
        license: metadata.license.to_string(),
        license_url: metadata.license_url.to_string(),
        archive_sha256: metadata.archive_sha256.to_string(),
        archive_size: metadata.archive_size,
        binary_path,
        ffmpeg_version_line,
        installed_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub(crate) fn write_manifest(root: &Path, manifest: &ManagedFfmpegManifest) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("failed to create managed FFmpeg directory: {error}"))?;
    let path = manifest_path(root);
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("failed to encode managed FFmpeg manifest: {error}"))?;
    std::fs::write(&path, text)
        .map_err(|error| format!("failed to write managed FFmpeg manifest: {error}"))
}

struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(root: &Path) -> Result<Self, String> {
        let tmp_root = root.join("tmp");
        std::fs::create_dir_all(&tmp_root)
            .map_err(|error| format!("failed to create FFmpeg tmp directory: {error}"))?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = tmp_root.join(format!("scratch-{nanos}-{}", std::process::id()));
        std::fs::create_dir(&path)
            .map_err(|error| format!("failed to create FFmpeg scratch directory: {error}"))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if self.path.exists() {
            if let Err(error) = std::fs::remove_dir_all(&self.path) {
                tracing::warn!(
                    target: "rollshot::action::ffmpeg",
                    scratch_dir = ?self.path,
                    error = %error,
                    "failed to remove FFmpeg scratch directory"
                );
            }
        }
    }
}

pub(crate) fn download_managed_ffmpeg() -> Result<PathBuf, String> {
    let metadata = pinned_metadata_for_current_platform()
        .ok_or_else(|| "managed FFmpeg is not available for this platform".to_string())?;
    let root = managed_root()?;
    let scratch = ScratchDir::new(&root)?;

    let archive = ffmpeg_sidecar::download::download_ffmpeg_package_with_progress(
        metadata.source_url,
        scratch.path(),
        |event| match event {
            ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::Starting => {
                tracing::info!(
                    target: "rollshot::action::ffmpeg",
                    source_url = metadata.source_url,
                    "managed FFmpeg download started"
                );
            }
            ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::Downloading {
                total_bytes,
                downloaded_bytes,
            } => {
                tracing::info!(
                    target: "rollshot::action::ffmpeg",
                    total_bytes,
                    downloaded_bytes,
                    "managed FFmpeg download progress"
                );
            }
            ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::UnpackingArchive => {
                tracing::info!(
                    target: "rollshot::action::ffmpeg",
                    "managed FFmpeg unpacking archive"
                );
            }
            ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::Done => {
                tracing::info!(
                    target: "rollshot::action::ffmpeg",
                    "managed FFmpeg download complete"
                );
            }
        },
    )
    .map_err(|error| format!("failed to download managed FFmpeg: {error}"))?;

    if let Err(error) = verify_archive_sha(&archive, metadata.archive_sha256) {
        if let Err(remove_error) = std::fs::remove_file(&archive) {
            tracing::warn!(
                target: "rollshot::action::ffmpeg",
                archive_path = ?archive,
                error = %remove_error,
                "failed to remove mismatched managed FFmpeg archive"
            );
        }
        return Err(error);
    }

    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|error| format!("failed to create FFmpeg bin directory: {error}"))?;
    let binary = managed_binary_path(&root);
    ffmpeg_sidecar::download::unpack_ffmpeg_without_extras(&archive, &bin_dir).map_err(
        |error| {
            let _ = std::fs::remove_file(&binary);
            format!("failed to unpack managed FFmpeg: {error}")
        },
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = match std::fs::metadata(&binary) {
            Ok(metadata) => metadata.permissions(),
            Err(error) => {
                let _ = std::fs::remove_file(&binary);
                return Err(format!("failed to inspect managed FFmpeg: {error}"));
            }
        };
        perms.set_mode(0o755);
        if let Err(error) = std::fs::set_permissions(&binary, perms) {
            let _ = std::fs::remove_file(&binary);
            return Err(format!("failed to set FFmpeg executable bit: {error}"));
        }
    }

    let version_line = match validate_ffmpeg(&binary) {
        Ok(line) => line,
        Err(error) => {
            let _ = std::fs::remove_file(&binary);
            return Err(error);
        }
    };

    let manifest = build_manifest(metadata, binary.clone(), version_line);
    if let Err(error) = write_manifest(&root, &manifest) {
        let _ = std::fs::remove_file(&binary);
        return Err(error);
    }

    Ok(binary)
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

    #[test]
    fn sha256_file_detects_content() {
        let dir = tempdir();
        let path = dir.path().join("archive.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_archive_sha_rejects_mismatch() {
        let dir = tempdir();
        let path = dir.path().join("archive.bin");
        std::fs::write(&path, b"abc").unwrap();
        let result = verify_archive_sha(&path, "0000");
        assert!(result.unwrap_err().contains("sha256 mismatch"));
    }

    #[test]
    fn write_manifest_persists_valid_json() {
        let dir = tempdir();
        let binary = dir.path().join("bin/ffmpeg");
        let manifest = build_manifest(
            LINUX_X86_64_METADATA,
            binary.clone(),
            "ffmpeg version 6.0.1-static".to_string(),
        );
        write_manifest(dir.path(), &manifest).unwrap();
        let restored = load_manifest(dir.path()).unwrap();
        assert_eq!(restored.binary_path, binary);
        assert_eq!(
            restored.archive_sha256,
            LINUX_X86_64_METADATA.archive_sha256
        );
    }

    #[test]
    fn scratch_dir_creates_under_root_tmp_and_cleans_up() {
        let root = tempdir();
        {
            let scratch = ScratchDir::new(root.path()).unwrap();
            assert!(scratch.path().starts_with(root.path().join("tmp")));
            assert!(scratch.path().exists());
        }
        let tmp = root.path().join("tmp");
        assert!(tmp.exists());
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0);
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
