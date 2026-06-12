// `write_png` and `unique_capture_path` have only test callers on Linux; the
// rest of this module is wired into the capture flows. Allow dead_code so those
// platform-internal helpers don't trip `-D warnings`.
#![allow(dead_code)]

use image::RgbaImage;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::diagnostics::TARGET_SAVE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

impl Platform {
    pub fn current() -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        return Ok(Platform::Linux);
        #[cfg(target_os = "macos")]
        return Ok(Platform::Macos);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        return Err("unsupported platform".to_string());
    }
}

pub fn default_output_dir(platform: Platform) -> Result<PathBuf, String> {
    match platform {
        Platform::Linux => {
            let config_home = std::env::var("XDG_CONFIG_HOME")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("/"))
                        .join(".config")
                });
            let user_dirs_path = config_home.join("user-dirs.dirs");
            let user_dirs_content = std::fs::read_to_string(&user_dirs_path).unwrap_or_default();
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
            Ok(linux_desktop_from(&user_dirs_content, &home, |p| {
                p.is_dir()
            }))
        }
        Platform::Macos => {
            dirs::desktop_dir().ok_or_else(|| "cannot resolve ~/Desktop".to_string())
        }
    }
}

/// Extract XDG_DESKTOP_DIR from a user-dirs.dirs file content and resolve the path.
/// Falls back to `home/Pictures` if absent, malformed, relative, or `!is_dir`.
pub fn linux_desktop_from(user_dirs: &str, home: &Path, is_dir: impl Fn(&Path) -> bool) -> PathBuf {
    let desktop = parse_xdg_desktop_dir(user_dirs, home);
    if let Some(path) = desktop {
        if path.is_absolute() && is_dir(&path) {
            return path;
        }
    }
    home.join("Pictures")
}

fn parse_xdg_desktop_dir(user_dirs: &str, home: &Path) -> Option<PathBuf> {
    for line in user_dirs.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("XDG_DESKTOP_DIR=") {
            // Per the freedesktop user-dirs spec, the value is a double-quoted path,
            // e.g. XDG_DESKTOP_DIR="$HOME/Desktop". A trailing inline `# comment` or
            // other malformed value is not specially handled here — it will simply fail
            // the `is_dir` check in the caller and fall back to ~/Pictures (safe).
            let value = rest.trim_matches('"');
            let expanded = expand_home(value, home);
            return Some(PathBuf::from(expanded));
        }
    }
    None
}

fn expand_home(s: &str, home: &Path) -> String {
    let home_str = home.to_string_lossy();
    // Replace ${HOME} first, then $HOME
    s.replace("${HOME}", home_str.as_ref())
        .replace("$HOME", home_str.as_ref())
}

/// Find the first non-existing path for a capture. Starts with
/// `dir/Rollshot {timestamp}.png`, then tries `-2`, `-3`, ...
pub fn unique_capture_path(dir: &Path, timestamp: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let base = dir.join(format!("Rollshot {timestamp}.png"));
    if !exists(&base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let candidate = dir.join(format!("Rollshot {timestamp}-{n}.png"));
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Private helper: encode `image` as PNG and write to `path` with exclusive-create semantics.
/// Returns `Ok(true)` if written, `Ok(false)` if the path already existed, `Err` on other errors.
fn try_write_png_exclusive(image: &RgbaImage, path: &Path) -> std::io::Result<bool> {
    let mut buf = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| std::io::Error::other(format!("PNG encode error: {e}")))?;

    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(&buf)
                .map_err(|e| std::io::Error::new(e.kind(), format!("failed to write PNG: {e}")))?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

/// Encode `image` as PNG and write to `path` using exclusive-create semantics.
/// Returns `Err` on `AlreadyExists` (caller retries) or on other IO/encode errors.
pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    match try_write_png_exclusive(image, path) {
        Ok(true) => Ok(()),
        Ok(false) => Err("already exists".to_string()),
        Err(e) => Err(format!("failed to create file: {e}")),
    }
}

/// Write `image` as PNG into `dir` with the given `timestamp`.
/// Fails if `dir` does not exist (never creates it).
/// Retries with suffix increments on exclusive-create collision.
/// Stops after 10_000 suffix attempts and returns an error.
pub fn auto_save_to(image: &RgbaImage, dir: &Path, timestamp: &str) -> Result<PathBuf, String> {
    if !dir.exists() {
        tracing::error!(
            target: TARGET_SAVE,
            category = "path_missing",
            destination = destination_category(dir),
            "save failure"
        );
        return Err(format!(
            "output directory does not exist: {}",
            dir.display()
        ));
    }

    let width = image.width();
    let height = image.height();
    tracing::info!(target: TARGET_SAVE, width, height, "save start");

    const MAX_ATTEMPTS: u32 = 10_000;
    for _ in 0..MAX_ATTEMPTS {
        let path = unique_capture_path(dir, timestamp, |p| p.exists());
        match try_write_png_exclusive(image, &path) {
            Ok(true) => {
                let encoded_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let category = destination_category(dir);
                tracing::info!(
                    target: TARGET_SAVE,
                    width,
                    height,
                    encoded_bytes,
                    platform = %platform_label(),
                    category,
                    "save success"
                );
                return Ok(path);
            }
            Ok(false) => continue,
            Err(e) => {
                let category = classify_save_error(&e.to_string());
                tracing::error!(target: TARGET_SAVE, category, "save failure");
                return Err(e.to_string());
            }
        }
    }
    tracing::error!(
        target: TARGET_SAVE,
        category = "filename_exhausted",
        "save failure"
    );
    Err(format!(
        "auto-save could not find a free filename after {MAX_ATTEMPTS} attempts"
    ))
}

/// Save `image` to the platform desktop directory using the current wall-clock timestamp.
pub fn auto_save(image: &RgbaImage, platform: Platform) -> Result<PathBuf, String> {
    let timestamp = chrono::Local::now()
        .format("%Y-%m-%d at %H.%M.%S")
        .to_string();
    let dir = default_output_dir(platform)?;
    auto_save_to(image, &dir, &timestamp)
}

fn platform_label() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    }
}

fn destination_category(dir: &Path) -> &'static str {
    let dir_str = dir.to_string_lossy().to_lowercase();
    if dir_str.contains("desktop") {
        "desktop"
    } else if dir_str.contains("pictures") {
        "pictures"
    } else {
        "unknown"
    }
}

pub(crate) fn classify_save_error(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.contains("permission") || lower.contains("access") {
        "permission"
    } else if lower.contains("space") || lower.contains("disk") || lower.contains("no space") {
        "disk_space"
    } else if lower.contains("not found") || lower.contains("missing") {
        "path_missing"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- linux_desktop_from ---

    #[test]
    fn linux_desktop_expands_home_from_user_dirs() {
        let home = Path::new("/home/noah");
        let configured = r#"XDG_DESKTOP_DIR="$HOME/Desktop""#;
        assert_eq!(
            linux_desktop_from(configured, home, |path| path
                == Path::new("/home/noah/Desktop")),
            PathBuf::from("/home/noah/Desktop")
        );
    }

    #[test]
    fn linux_desktop_falls_back_to_pictures_when_configured_directory_is_missing() {
        let home = Path::new("/home/noah");
        let configured = r#"XDG_DESKTOP_DIR="$HOME/Desktop""#;
        assert_eq!(
            linux_desktop_from(configured, home, |path| path
                == Path::new("/home/noah/Pictures")),
            PathBuf::from("/home/noah/Pictures")
        );
    }

    #[test]
    fn linux_desktop_falls_back_to_pictures_when_no_desktop_configured() {
        let home = Path::new("/home/noah");
        // No XDG_DESKTOP_DIR entry at all
        assert_eq!(
            linux_desktop_from("", home, |_| false),
            PathBuf::from("/home/noah/Pictures")
        );
    }

    #[test]
    fn linux_desktop_falls_back_to_pictures_when_both_missing() {
        let home = Path::new("/home/noah");
        let configured = r#"XDG_DESKTOP_DIR="$HOME/Desktop""#;
        // is_dir returns false for every path (neither Desktop nor Pictures exist)
        assert_eq!(
            linux_desktop_from(configured, home, |_| false),
            PathBuf::from("/home/noah/Pictures")
        );
    }

    #[test]
    fn linux_desktop_expands_braced_home() {
        let home = Path::new("/home/noah");
        let configured = r#"XDG_DESKTOP_DIR="${HOME}/Desktop""#;
        assert_eq!(
            linux_desktop_from(configured, home, |path| path
                == Path::new("/home/noah/Desktop")),
            PathBuf::from("/home/noah/Desktop")
        );
    }

    #[test]
    fn linux_desktop_rejects_relative_value() {
        let home = Path::new("/home/noah");
        // If the value doesn't start with '/', it's relative — must fall back.
        let configured = r#"XDG_DESKTOP_DIR="relative/path""#;
        assert_eq!(
            linux_desktop_from(configured, home, |_| true),
            PathBuf::from("/home/noah/Pictures")
        );
    }

    // --- unique_capture_path ---

    #[test]
    fn unique_capture_path_returns_base_when_free() {
        let dir = Path::new("/tmp");
        let path = unique_capture_path(dir, "2026-06-09 at 12.34.56", |_| false);
        assert_eq!(
            path,
            PathBuf::from("/tmp/Rollshot 2026-06-09 at 12.34.56.png")
        );
    }

    #[test]
    fn unique_capture_path_returns_suffix_2_when_base_exists() {
        let dir = Path::new("/tmp");
        let path = unique_capture_path(dir, "2026-06-09 at 12.34.56", |candidate| {
            candidate.ends_with("Rollshot 2026-06-09 at 12.34.56.png")
        });
        assert_eq!(
            path,
            PathBuf::from("/tmp/Rollshot 2026-06-09 at 12.34.56-2.png")
        );
    }

    #[test]
    fn unique_capture_path_returns_suffix_3_when_base_and_2_exist() {
        let dir = Path::new("/tmp");
        let path = unique_capture_path(dir, "2026-06-09 at 12.34.56", |candidate| {
            candidate.ends_with("Rollshot 2026-06-09 at 12.34.56.png")
                || candidate.ends_with("Rollshot 2026-06-09 at 12.34.56-2.png")
        });
        assert_eq!(
            path,
            PathBuf::from("/tmp/Rollshot 2026-06-09 at 12.34.56-3.png")
        );
    }

    // --- auto_save_to ---

    #[test]
    fn auto_save_does_not_create_missing_directory() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("Desktop");
        let err = auto_save_to(
            &image::RgbaImage::new(2, 2),
            &missing,
            "2026-06-09 at 12.34.56",
        )
        .expect_err("missing directory must fail");
        assert!(err.contains("does not exist"));
        assert!(!missing.exists());
    }

    #[test]
    fn missing_auto_save_directory_emits_private_failure_category() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("Secret");
        let log = crate::diagnostics::capture_test_logs(|| {
            let _ = auto_save_to(
                &image::RgbaImage::new(2, 2),
                &missing,
                "2026-06-09 at 12.34.56",
            );
        });

        assert!(log.contains("save failure"), "log = {log}");
        assert!(log.contains("category=\"path_missing\""), "log = {log}");
        assert!(
            !log.contains(missing.to_string_lossy().as_ref()),
            "log = {log}"
        );
    }

    #[test]
    fn auto_save_to_writes_png_and_returns_path() {
        let dir = tempfile::tempdir().unwrap();
        let image = image::RgbaImage::new(4, 4);
        let result = auto_save_to(&image, dir.path(), "2026-06-09 at 12.34.56");
        let path = result.expect("auto_save_to should succeed");
        assert!(path.exists(), "written file must exist");
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Rollshot 2026-06-09 at 12.34.56"));
    }

    #[test]
    fn auto_save_to_second_call_produces_distinct_file() {
        let dir = tempfile::tempdir().unwrap();
        let image = image::RgbaImage::new(4, 4);
        let p1 = auto_save_to(&image, dir.path(), "2026-06-09 at 12.34.56").unwrap();
        let p2 = auto_save_to(&image, dir.path(), "2026-06-09 at 12.34.56").unwrap();
        assert_ne!(p1, p2, "second save must produce a distinct file");
        assert!(p1.exists());
        assert!(p2.exists());
        // Second file should have -2 suffix
        assert!(p2
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-2.png"));
    }
}
