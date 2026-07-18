use std::path::{Path, PathBuf};

use rollshot_action::project::PublishCancellation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareKind {
    SafeCopy,
    EditableProject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareProgress {
    WaitingForPublish,
    Copying,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub(crate) struct ShareRequest {
    pub source_root: PathBuf,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShareOutcome {
    Complete(PathBuf),
    Cancelled,
    Failed(String),
}

fn is_safe_publish_entry(name: &str) -> bool {
    matches!(
        name,
        "index.html" | "steps.md" | "session.json" | "storyboard.png" | "guide.gif" | "summary.mp4"
    ) || name == "keyframes"
}

fn is_editable_project_entry(name: &str) -> bool {
    name == "project.json" || name == "assets" || name == "publish-state.json" || name == "publish"
}

fn is_blocked_name(name: &str) -> bool {
    if name == ".lock" {
        return true;
    }
    if name.ends_with(".tmp") {
        return true;
    }
    if name.starts_with(".tmp-") {
        return true;
    }
    if name.ends_with(".bak") || name.ends_with(".backup") {
        return true;
    }
    false
}

fn is_safe_publish_subdir_entry(dir_name: &str, file_name: &str) -> bool {
    if dir_name == "keyframes" {
        return file_name.ends_with(".png");
    }
    true
}

fn copy_file_no_follow(
    source: &Path,
    destination: &Path,
    cancel: &PublishCancellation,
) -> Result<(), String> {
    use rustix::fs::{openat, unlinkat, AtFlags, Mode, OFlags, CWD};

    let src_file = openat(
        CWD,
        source,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NOCTTY,
        Mode::empty(),
    )
    .map_err(|e| format!("open source: {e}"))?;

    let src_meta = rustix::fs::fstat(&src_file).map_err(|e| format!("stat source: {e}"))?;
    if rustix::fs::FileType::from_raw_mode(src_meta.st_mode) != rustix::fs::FileType::RegularFile {
        return Err("source is not a regular file".to_string());
    }

    let tmp_path = destination.with_extension(format!("tmp.{}", std::process::id()));

    let dst_file = openat(
        CWD,
        &tmp_path,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|e| format!("create temp: {e}"))?;

    let mut src = std::fs::File::from(src_file);
    let mut dst = std::fs::File::from(dst_file);

    let mut buf = [0u8; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            drop(dst);
            drop(src);
            let _ = unlinkat(CWD, &tmp_path, AtFlags::empty());
            return Err("cancelled".to_string());
        }
        use std::io::{Read, Write};
        let n = src.read(&mut buf).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("read: {e}")
        })?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("write: {e}")
        })?;
    }

    use std::io::Write;
    dst.flush().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("flush: {e}")
    })?;
    drop(dst);
    drop(src);

    commit_file_noreplace(&tmp_path, destination).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp_path);
    })
}

fn commit_file_noreplace(source: &Path, destination: &Path) -> Result<(), String> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE)
        .map_err(|e| format!("commit: {e}"))
}

fn fsync_dir(path: &Path) -> Result<(), std::io::Error> {
    let file = std::fs::File::open(path)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn copy_safe_publish(
    request: &ShareRequest,
    cancel: &PublishCancellation,
) -> ShareOutcome {
    let ShareRequest {
        source_root,
        destination,
    } = request;

    let publish_dir = source_root.join("publish");
    if !publish_dir.is_dir() {
        return ShareOutcome::Failed("publish directory not found".to_string());
    }

    match copy_tree_filtered(
        &publish_dir,
        destination,
        cancel,
        |name, _| is_safe_publish_entry(name),
        is_safe_publish_subdir_entry,
    ) {
        Ok(()) => {
            let _ = fsync_dir(destination);
            ShareOutcome::Complete(destination.clone())
        }
        Err(e) if e == "cancelled" => ShareOutcome::Cancelled,
        Err(e) => ShareOutcome::Failed(e),
    }
}

pub(crate) fn copy_editable_project(
    request: &ShareRequest,
    cancel: &PublishCancellation,
) -> ShareOutcome {
    let ShareRequest {
        source_root,
        destination,
    } = request;

    let manifest_path = source_root.join("project.json");
    let manifest_bytes = match std::fs::read(&manifest_path) {
        Ok(b) => b,
        Err(e) => return ShareOutcome::Failed(format!("read project.json: {e}")),
    };
    let manifest: serde_json::Value = match serde_json::from_slice(&manifest_bytes) {
        Ok(v) => v,
        Err(e) => return ShareOutcome::Failed(format!("parse project.json: {e}")),
    };

    let referenced_sha256: std::collections::BTreeSet<String> = manifest
        .get("frames")
        .and_then(|f| f.as_array())
        .map(|frames| {
            frames
                .iter()
                .filter_map(|f| f.get("sha256").and_then(|s| s.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match copy_tree_filtered(
        source_root,
        destination,
        cancel,
        |name, _path| {
            if is_blocked_name(name) {
                return false;
            }
            if is_editable_project_entry(name) {
                return true;
            }
            if name.starts_with('.') {
                return false;
            }
            false
        },
        |dir_name, file_name| {
            if dir_name == "assets" {
                if file_name == "frames" {
                    return true;
                }
                return false;
            }
            if dir_name == "frames" {
                if file_name.ends_with(".png") {
                    let sha256 = file_name.trim_end_matches(".png");
                    return referenced_sha256.contains(sha256);
                }
                return false;
            }
            if dir_name == "publish" {
                return is_safe_publish_entry(file_name);
            }
            if dir_name == "keyframes" {
                return file_name.ends_with(".png");
            }
            true
        },
    ) {
        Ok(()) => {
            let _ = fsync_dir(destination);
            ShareOutcome::Complete(destination.clone())
        }
        Err(e) if e == "cancelled" => ShareOutcome::Cancelled,
        Err(e) => ShareOutcome::Failed(e),
    }
}

fn copy_tree_filtered(
    source: &Path,
    destination: &Path,
    cancel: &PublishCancellation,
    allow_entry: impl Fn(&str, &Path) -> bool + Copy,
    allow_subdir_entry: impl Fn(&str, &str) -> bool + Copy,
) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|e| format!("create dest: {e}"))?;

    let entries = std::fs::read_dir(source)
        .map_err(|e| format!("read source dir: {e}"))?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();

    for entry in &entries {
        if cancel.is_cancelled() {
            return Err("cancelled".to_string());
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let source_path = entry.path();

        if is_blocked_name(&name_str) {
            continue;
        }

        let meta = match std::fs::symlink_metadata(&source_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.file_type().is_symlink() {
            tracing::event!(
                target: "rollshot::share",
                tracing::Level::WARN,
                category = "symlink_skipped",
                "symlink skipped in share copy"
            );
            continue;
        }

        if !allow_entry(&name_str, &source_path) {
            continue;
        }

        let dest_path = destination.join(&name);

        if meta.file_type().is_dir() {
            std::fs::create_dir_all(&dest_path).map_err(|e| format!("create subdir: {e}"))?;
            copy_subdir_filtered(
                &source_path,
                &dest_path,
                cancel,
                &name_str,
                allow_subdir_entry,
            )?;
            let _ = fsync_dir(&dest_path);
        } else if meta.file_type().is_file() {
            copy_file_no_follow(&source_path, &dest_path, cancel)?;
        }
    }

    Ok(())
}

fn copy_subdir_filtered(
    source: &Path,
    destination: &Path,
    cancel: &PublishCancellation,
    parent_name: &str,
    allow_entry: impl Fn(&str, &str) -> bool + Copy,
) -> Result<(), String> {
    let entries = std::fs::read_dir(source)
        .map_err(|e| format!("read subdir: {e}"))?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();

    for entry in &entries {
        if cancel.is_cancelled() {
            return Err("cancelled".to_string());
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let source_path = entry.path();

        if is_blocked_name(&name_str) {
            continue;
        }

        let meta = match std::fs::symlink_metadata(&source_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.file_type().is_symlink() {
            continue;
        }

        if !allow_entry(parent_name, &name_str) {
            continue;
        }

        let dest_path = destination.join(&name);

        if meta.file_type().is_dir() {
            std::fs::create_dir_all(&dest_path)
                .map_err(|e| format!("create nested subdir: {e}"))?;
            copy_subdir_filtered(&source_path, &dest_path, cancel, &name_str, allow_entry)?;
            let _ = fsync_dir(&dest_path);
        } else if meta.file_type().is_file() {
            copy_file_no_follow(&source_path, &dest_path, cancel)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::project::{
        write_publish_state, PublishOutputKind, PublishStateV1, PublishedOutputV1,
    };

    fn cancel() -> PublishCancellation {
        PublishCancellation::new()
    }

    fn setup_project(dir: &Path) -> PathBuf {
        let root = dir.join("test-project.rollshot-guide");
        std::fs::create_dir_all(root.join("assets/frames")).unwrap();
        std::fs::create_dir_all(root.join("publish/keyframes")).unwrap();

        std::fs::write(root.join("project.json"), r#"{"schema_version":1,"revision":1,"title":"T","capture_region":{"x":0,"y":0,"width":8,"height":8},"input_source":"visual-only","input_capability":"semantic-events","enabled_outputs":{},"frames":[{"id":1,"at_ms":100,"sha256":"abc123","width":8,"height":8}],"steps":[{"id":1,"order":1,"title":"S","caption":null,"kind":"click","reason":"click-confirmed","at_ms":100,"keyframe":1,"nearby":[1],"annotations":null}]}"#).unwrap();

        let png = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        let png_bytes = {
            let mut buf = std::io::Cursor::new(Vec::new());
            png.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        };
        std::fs::write(root.join("assets/frames/abc123.png"), &png_bytes).unwrap();

        std::fs::write(root.join("publish/index.html"), "<html></html>").unwrap();
        std::fs::write(root.join("publish/steps.md"), "# Steps").unwrap();
        std::fs::write(
            root.join("publish/session.json"),
            r#"{"schema_version":1,"title":"T","region":{"x":0,"y":0,"width":8,"height":8},"input_source":"visual-only","input_capability":"semantic-events","steps":[{"index":1,"title":"S","kind":"click","reason":"click-confirmed","at_ms":100,"keyframe_file":"keyframes/001.png","hotspots":[]}]}"#,
        )
        .unwrap();
        std::fs::write(root.join("publish/keyframes/001.png"), &png_bytes).unwrap();

        let mut state = PublishStateV1::default();
        state
            .outputs
            .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
        write_publish_state(&root, &state).unwrap();

        root
    }

    fn setup_project_with_noise(dir: &Path) -> PathBuf {
        let root = setup_project(dir);

        std::fs::write(root.join(".lock"), "locked").unwrap();
        std::fs::write(root.join("publish/.tmp-foo-123-0"), "temp").unwrap();
        std::fs::write(root.join("data.tmp"), "tempfile").unwrap();
        std::fs::write(root.join("old.bak"), "backup").unwrap();
        std::os::unix::fs::symlink("/nonexistent", root.join("publish/evil-link")).unwrap();
        std::os::unix::fs::symlink("/nonexistent", root.join("assets/frames/evil-link.png"))
            .unwrap();

        root
    }

    fn setup_published_with_derivatives(dir: &Path) -> PathBuf {
        let root = setup_project(dir);
        std::fs::write(root.join("publish/storyboard.png"), "storyboard-data").unwrap();
        std::fs::write(root.join("publish/guide.gif"), "gif-data").unwrap();

        let mut state = PublishStateV1::default();
        state
            .outputs
            .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
        state
            .outputs
            .insert(PublishOutputKind::Storyboard, PublishedOutputV1::new(1));
        state
            .outputs
            .insert(PublishOutputKind::Gif, PublishedOutputV1::new(1));
        write_publish_state(&root, &state).unwrap();

        root
    }

    #[test]
    fn safe_copy_contains_only_publish_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project_with_noise(dir.path());
        let dest = dir.path().join("safe-output");

        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let result = copy_safe_publish(&request, &cancel());
        assert_eq!(result, ShareOutcome::Complete(dest.clone()));

        assert!(dest.join("index.html").exists());
        assert!(dest.join("steps.md").exists());
        assert!(dest.join("session.json").exists());
        assert!(dest.join("keyframes/001.png").exists());

        assert!(!dest.join("project.json").exists());
        assert!(!dest.join("assets").exists());
        assert!(!dest.join("publish-state.json").exists());
    }

    #[test]
    fn editable_project_contains_manifest_assets_publish_state_and_publish() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project_with_noise(dir.path());
        let dest = dir.path().join("editable-output");

        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let result = copy_editable_project(&request, &cancel());
        assert_eq!(result, ShareOutcome::Complete(dest.clone()));

        assert!(dest.join("project.json").exists());
        assert!(dest.join("assets/frames/abc123.png").exists());
        assert!(dest.join("publish-state.json").exists());
        assert!(dest.join("publish/index.html").exists());
        assert!(dest.join("publish/steps.md").exists());
        assert!(dest.join("publish/session.json").exists());
        assert!(dest.join("publish/keyframes/001.png").exists());
    }

    #[test]
    fn neither_mode_copies_blocked_targets() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project_with_noise(dir.path());
        let safe_dest = dir.path().join("safe-blocked");
        let editable_dest = dir.path().join("editable-blocked");

        let safe_req = ShareRequest {
            source_root: root.clone(),
            destination: safe_dest.clone(),
        };
        let _ = copy_safe_publish(&safe_req, &cancel());

        assert!(!safe_dest.join(".lock").exists());
        assert!(!safe_dest.join(".tmp-foo-123-0").exists());
        assert!(!safe_dest.join("evil-link").exists());

        let editable_req = ShareRequest {
            source_root: root.clone(),
            destination: editable_dest.clone(),
        };
        let _ = copy_editable_project(&editable_req, &cancel());

        assert!(!editable_dest.join(".lock").exists());
        assert!(!editable_dest.join("data.tmp").exists());
        assert!(!editable_dest.join("old.bak").exists());
        assert!(!editable_dest.join("evil-link").exists());
        assert!(!editable_dest.join("assets/frames/evil-link.png").exists());
    }

    #[test]
    fn symlink_substituted_for_allowlisted_path_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());

        let dest = dir.path().join("symlink-test");
        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let result = copy_safe_publish(&request, &cancel());
        assert_eq!(result, ShareOutcome::Complete(dest.clone()));
        assert!(dest.join("index.html").exists());

        let edit_dest = dir.path().join("symlink-edit-test");
        std::fs::remove_file(root.join("assets/frames/abc123.png")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("assets/frames/abc123.png")).unwrap();

        let edit_req = ShareRequest {
            source_root: root.clone(),
            destination: edit_dest.clone(),
        };
        let result = copy_editable_project(&edit_req, &cancel());
        assert_eq!(result, ShareOutcome::Complete(edit_dest.clone()));
        assert!(
            !edit_dest.join("assets/frames/abc123.png").exists(),
            "symlinked asset must not be copied"
        );
    }

    #[test]
    fn pre_existing_destination_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        let dest = dir.path().join("existing-dest");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("index.html"), "ORIGINAL").unwrap();

        let request = ShareRequest {
            source_root: root,
            destination: dest.clone(),
        };

        let result = copy_safe_publish(&request, &cancel());
        assert!(
            matches!(result, ShareOutcome::Failed(_)),
            "must not overwrite existing destination"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("index.html")).unwrap(),
            "ORIGINAL"
        );
    }

    #[test]
    fn cancellation_during_copy_removes_partial_output() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        let dest = dir.path().join("cancel-test");

        let cancel = PublishCancellation::new();
        cancel.cancel();

        let request = ShareRequest {
            source_root: root,
            destination: dest.clone(),
        };

        let result = copy_safe_publish(&request, &cancel);
        assert_eq!(result, ShareOutcome::Cancelled);
    }

    #[test]
    fn diagnostics_contain_no_private_text() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        let dest = dir.path().join("diag-test");

        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let logs = crate::diagnostics::capture_test_logs(|| {
            let _ = copy_safe_publish(&request, &cancel());
        });

        let root_str = root.to_string_lossy();
        if root_str.len() > 4 {
            assert!(
                !logs.contains(root_str.as_ref()),
                "logs must not contain project root: {logs}"
            );
        }
    }

    #[test]
    fn editable_project_only_copies_referenced_assets() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());

        let unreferenced = image::RgbaImage::from_pixel(8, 8, image::Rgba([99, 99, 99, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        unreferenced
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(
            root.join("assets/frames/unreferenced.png"),
            buf.into_inner(),
        )
        .unwrap();

        let dest = dir.path().join("ref-test");
        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let result = copy_editable_project(&request, &cancel());
        assert_eq!(result, ShareOutcome::Complete(dest.clone()));
        assert!(dest.join("assets/frames/abc123.png").exists());
        assert!(
            !dest.join("assets/frames/unreferenced.png").exists(),
            "unreferenced asset must not be copied"
        );
    }

    #[test]
    fn safe_copy_with_derivatives_includes_all_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_published_with_derivatives(dir.path());
        let dest = dir.path().join("full-safe");

        let request = ShareRequest {
            source_root: root,
            destination: dest.clone(),
        };

        let result = copy_safe_publish(&request, &cancel());
        assert_eq!(result, ShareOutcome::Complete(dest.clone()));
        assert!(dest.join("index.html").exists());
        assert!(dest.join("storyboard.png").exists());
        assert!(dest.join("guide.gif").exists());
        assert!(!dest.join("project.json").exists());
    }
}
