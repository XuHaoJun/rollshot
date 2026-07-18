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

pub(crate) fn destination_in(parent: &Path, source_root: &Path, kind: ShareKind) -> PathBuf {
    let project_name = source_root
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("action-guide.rollshot-guide"));
    match kind {
        ShareKind::SafeCopy => {
            let name = project_name.to_string_lossy();
            let stem = name.strip_suffix(".rollshot-guide").unwrap_or(&name);
            parent.join(format!("{stem}-safe-viewer"))
        }
        ShareKind::EditableProject => parent.join(project_name),
    }
}

fn open_source_root(path: &Path) -> Result<std::os::fd::OwnedFd, String> {
    use rustix::fs::{openat, Mode, OFlags, CWD};
    openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("open source directory: {error}"))
}

fn relative_components(relative: &Path) -> Result<Vec<&std::ffi::OsStr>, String> {
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(name) => components.push(name),
            _ => return Err("source path is not a safe relative path".to_string()),
        }
    }
    if components.is_empty() {
        return Err("source path is empty".to_string());
    }
    Ok(components)
}

fn open_relative_file(
    root: &std::os::fd::OwnedFd,
    relative: &Path,
    required: bool,
) -> Result<Option<std::os::fd::OwnedFd>, String> {
    use rustix::fs::{openat, Mode, OFlags};

    let components = relative_components(relative)?;
    let mut directory = rustix::io::dup(root).map_err(|error| format!("open source: {error}"))?;
    for component in &components[..components.len() - 1] {
        directory = match openat(
            &directory,
            *component,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(directory) => directory,
            Err(error) if !required && error == rustix::io::Errno::NOENT => return Ok(None),
            Err(error) => return Err(format!("open source directory: {error}")),
        };
    }

    let file = match openat(
        &directory,
        components[components.len() - 1],
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(file) => file,
        Err(error) if !required && error == rustix::io::Errno::NOENT => return Ok(None),
        Err(error) => return Err(format!("open source file: {error}")),
    };
    let metadata = rustix::fs::fstat(&file).map_err(|error| format!("stat source: {error}"))?;
    if rustix::fs::FileType::from_raw_mode(metadata.st_mode) != rustix::fs::FileType::RegularFile {
        return Err("source is not a regular file".to_string());
    }
    Ok(Some(file))
}

fn read_relative_file(
    root: &std::os::fd::OwnedFd,
    relative: &Path,
    required: bool,
) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let Some(file) = open_relative_file(root, relative, required)? else {
        return Ok(None);
    };
    let mut file = std::fs::File::from(file);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("read source file: {error}"))?;
    Ok(Some(bytes))
}

fn copy_relative_file(
    root: &std::os::fd::OwnedFd,
    relative: &Path,
    destination: &Path,
    cancel: &PublishCancellation,
) -> Result<bool, String> {
    let Some(src_file) = open_relative_file(root, relative, true)? else {
        return Ok(false);
    };
    copy_open_file(src_file, destination, cancel)?;
    Ok(true)
}

fn copy_optional_relative_file(
    root: &std::os::fd::OwnedFd,
    relative: &Path,
    destination: &Path,
    cancel: &PublishCancellation,
) -> Result<bool, String> {
    let Some(src_file) = open_relative_file(root, relative, false)? else {
        return Ok(false);
    };
    copy_open_file(src_file, destination, cancel)?;
    Ok(true)
}

fn copy_open_file(
    src_file: std::os::fd::OwnedFd,
    destination: &Path,
    cancel: &PublishCancellation,
) -> Result<(), String> {
    use rustix::fs::{openat, unlinkat, AtFlags, Mode, OFlags, CWD};
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("create subdir: {error}"))?;
    }
    let tmp_path = destination.with_extension(format!("tmp.{}", std::process::id()));
    let dst_file = openat(
        CWD,
        &tmp_path,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| format!("create temp: {error}"))?;
    let mut src = std::fs::File::from(src_file);
    let mut dst = std::fs::File::from(dst_file);
    let mut buf = [0_u8; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            drop(dst);
            drop(src);
            let _ = unlinkat(CWD, &tmp_path, AtFlags::empty());
            return Err("cancelled".to_string());
        }
        use std::io::{Read, Write};
        let count = src.read(&mut buf).map_err(|error| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("read: {error}")
        })?;
        if count == 0 {
            break;
        }
        dst.write_all(&buf[..count]).map_err(|error| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("write: {error}")
        })?;
    }
    use std::io::Write;
    dst.flush().map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("flush: {error}")
    })?;
    dst.sync_all().map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("sync: {error}")
    })?;
    drop(dst);
    drop(src);
    commit_file_noreplace(&tmp_path, destination).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp_path);
    })
}

fn copy_bytes_file(
    bytes: &[u8],
    destination: &Path,
    cancel: &PublishCancellation,
) -> Result<(), String> {
    use rustix::fs::{openat, Mode, OFlags, CWD};
    use std::io::Write;

    if cancel.is_cancelled() {
        return Err("cancelled".to_string());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("create subdir: {error}"))?;
    }
    let tmp_path = destination.with_extension(format!("tmp.{}", std::process::id()));
    let dst_file = openat(
        CWD,
        &tmp_path,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| format!("create temp: {error}"))?;
    let mut dst = std::fs::File::from(dst_file);
    dst.write_all(bytes).map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("write: {error}")
    })?;
    dst.flush().map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("flush: {error}")
    })?;
    dst.sync_all().map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("sync: {error}")
    })?;
    drop(dst);
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
    copy_safe_publish_impl(request, cancel, || {})
}

fn copy_safe_publish_impl(
    request: &ShareRequest,
    cancel: &PublishCancellation,
    before_copy: impl FnOnce(),
) -> ShareOutcome {
    let source = match open_source_root(&request.source_root.join("publish")) {
        Ok(source) => source,
        Err(error) => return ShareOutcome::Failed(error),
    };
    match copy_transaction(
        &source,
        &request.destination,
        cancel,
        before_copy,
        |source, staging, cancel| copy_safe_publish_files(source, staging, cancel, true),
    ) {
        Ok(()) => {
            let _ = fsync_dir(&request.destination);
            ShareOutcome::Complete(request.destination.clone())
        }
        Err(error) if error == "cancelled" => ShareOutcome::Cancelled,
        Err(error) => ShareOutcome::Failed(error),
    }
}

#[cfg(test)]
fn copy_safe_publish_with_hook(
    request: &ShareRequest,
    cancel: &PublishCancellation,
    before_copy: impl FnOnce(),
) -> ShareOutcome {
    copy_safe_publish_impl(request, cancel, before_copy)
}

pub(crate) fn copy_editable_project(
    request: &ShareRequest,
    cancel: &PublishCancellation,
) -> ShareOutcome {
    copy_editable_project_impl(request, cancel, || {}, || {})
}

fn copy_editable_project_impl(
    request: &ShareRequest,
    cancel: &PublishCancellation,
    before_copy: impl FnOnce(),
    after_manifest_read: impl FnOnce(),
) -> ShareOutcome {
    let source = match open_source_root(&request.source_root) {
        Ok(source) => source,
        Err(error) => return ShareOutcome::Failed(error),
    };
    match copy_transaction(
        &source,
        &request.destination,
        cancel,
        before_copy,
        |source, staging, cancel| {
            let manifest_bytes = read_relative_file(source, Path::new("project.json"), true)?
                .ok_or_else(|| "project.json is missing".to_string())?;
            let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)
                .map_err(|error| format!("parse project.json: {error}"))?;
            let referenced_sha256: std::collections::BTreeSet<String> = manifest
                .get("frames")
                .and_then(|frames| frames.as_array())
                .map(|frames| {
                    frames
                        .iter()
                        .filter_map(|frame| frame.get("sha256").and_then(|sha| sha.as_str()))
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            if referenced_sha256
                .iter()
                .any(|sha| sha.is_empty() || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
            {
                return Err("project.json contains an invalid frame digest".to_string());
            }
            after_manifest_read();

            copy_bytes_file(&manifest_bytes, &staging.join("project.json"), cancel)?;
            copy_optional_relative_file(
                source,
                Path::new("publish-state.json"),
                &staging.join("publish-state.json"),
                cancel,
            )?;
            for sha in referenced_sha256 {
                let relative = PathBuf::from(format!("assets/frames/{sha}.png"));
                copy_relative_file(source, &relative, &staging.join(&relative), cancel)?;
            }

            if read_relative_file(source, Path::new("publish/session.json"), false)?.is_some() {
                copy_safe_publish_files_at(
                    source,
                    Path::new("publish"),
                    &staging.join("publish"),
                    cancel,
                    true,
                )?;
            }
            Ok(())
        },
    ) {
        Ok(()) => {
            let _ = fsync_dir(&request.destination);
            ShareOutcome::Complete(request.destination.clone())
        }
        Err(error) if error == "cancelled" => ShareOutcome::Cancelled,
        Err(error) => ShareOutcome::Failed(error),
    }
}

#[cfg(test)]
fn copy_editable_project_with_hook(
    request: &ShareRequest,
    cancel: &PublishCancellation,
    before_copy: impl FnOnce(),
) -> ShareOutcome {
    copy_editable_project_impl(request, cancel, before_copy, || {})
}

#[cfg(test)]
fn copy_editable_project_with_manifest_hook(
    request: &ShareRequest,
    cancel: &PublishCancellation,
    after_manifest_read: impl FnOnce(),
) -> ShareOutcome {
    copy_editable_project_impl(request, cancel, || {}, after_manifest_read)
}

fn copy_transaction(
    source: &std::os::fd::OwnedFd,
    destination: &Path,
    cancel: &PublishCancellation,
    before_copy: impl FnOnce(),
    copy: impl FnOnce(&std::os::fd::OwnedFd, &Path, &PublishCancellation) -> Result<(), String>,
) -> Result<(), String> {
    match std::fs::symlink_metadata(destination) {
        Ok(_) => return Err("destination already exists".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect destination: {error}")),
    }

    let parent = destination
        .parent()
        .ok_or_else(|| "destination has no parent".to_string())?;
    let staging = unique_share_temp(parent);
    std::fs::create_dir(&staging).map_err(|e| format!("create staging directory: {e}"))?;
    let mut guard = StagingGuard::new(staging.clone());
    before_copy();
    copy(source, &staging, cancel)?;
    fsync_dir(&staging).map_err(|error| format!("sync staging directory: {error}"))?;
    commit_directory_noreplace(&staging, destination)?;
    guard.disarm();
    fsync_dir(parent).map_err(|error| {
        let _ = std::fs::remove_dir_all(destination);
        let _ = fsync_dir(parent);
        format!("sync destination parent: {error}")
    })
}

struct StagingGuard {
    path: PathBuf,
    armed: bool,
}

impl StagingGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn copy_safe_publish_files(
    source: &std::os::fd::OwnedFd,
    destination: &Path,
    cancel: &PublishCancellation,
    require_core: bool,
) -> Result<(), String> {
    copy_safe_publish_files_at(source, Path::new(""), destination, cancel, require_core)
}

fn copy_safe_publish_files_at(
    source: &std::os::fd::OwnedFd,
    source_prefix: &Path,
    destination: &Path,
    cancel: &PublishCancellation,
    require_core: bool,
) -> Result<(), String> {
    let session_relative = source_prefix.join("session.json");
    let session_bytes = read_relative_file(source, &session_relative, require_core)?;
    let Some(session_bytes) = session_bytes else {
        return Ok(());
    };
    let session: serde_json::Value = serde_json::from_slice(&session_bytes)
        .map_err(|error| format!("parse session.json: {error}"))?;
    let mut keyframes = std::collections::BTreeSet::new();
    for step in session
        .get("steps")
        .and_then(|steps| steps.as_array())
        .into_iter()
        .flatten()
    {
        let keyframe = step
            .get("keyframe_file")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "session.json contains an invalid keyframe path".to_string())?;
        let relative = Path::new(keyframe);
        let components = relative_components(relative)?;
        if components.len() != 2
            || components[0] != std::ffi::OsStr::new("keyframes")
            || !components[1].to_string_lossy().ends_with(".png")
        {
            return Err("session.json contains an invalid keyframe path".to_string());
        }
        keyframes.insert(relative.to_path_buf());
    }

    for name in ["index.html", "steps.md"] {
        let relative = source_prefix.join(name);
        copy_relative_file(source, &relative, &destination.join(name), cancel)?;
    }
    copy_bytes_file(&session_bytes, &destination.join("session.json"), cancel)?;
    for keyframe in keyframes {
        let relative = source_prefix.join(&keyframe);
        copy_relative_file(source, &relative, &destination.join(keyframe), cancel)?;
    }
    for name in ["storyboard.png", "guide.gif", "summary.mp4"] {
        let relative = source_prefix.join(name);
        copy_optional_relative_file(source, &relative, &destination.join(name), cancel)?;
    }
    Ok(())
}

fn unique_share_temp(parent: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    parent.join(format!(
        ".tmp-share-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn commit_directory_noreplace(source: &Path, destination: &Path) -> Result<(), String> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE)
        .map_err(|e| format!("commit directory: {e}"))
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
        assert!(matches!(result, ShareOutcome::Failed(_)));
        assert!(
            !edit_dest.exists(),
            "a rejected editable copy must leave no destination"
        );
    }

    #[test]
    fn directory_symlink_substitution_cannot_escape_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        let external = dir.path().join("external");
        std::fs::create_dir(&external).unwrap();
        std::fs::write(external.join("abc123.png"), b"external-private-data").unwrap();
        let dest = dir.path().join("directory-symlink-edit-test");
        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let result = copy_editable_project_with_hook(&request, &cancel(), || {
            std::fs::rename(
                root.join("assets/frames"),
                root.join("assets/original-frames"),
            )
            .unwrap();
            std::os::unix::fs::symlink(&external, root.join("assets/frames")).unwrap();
        });

        assert!(matches!(result, ShareOutcome::Failed(_)));
        assert!(!dest.exists(), "a rejected copy must remain atomic");
    }

    #[test]
    fn editable_copy_uses_the_exact_manifest_that_selected_assets() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        let original_manifest = std::fs::read(root.join("project.json")).unwrap();
        let dest = dir.path().join("manifest-snapshot-edit-test");
        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let result = copy_editable_project_with_manifest_hook(&request, &cancel(), || {
            let replacement = br#"{"schema_version":1,"revision":2,"title":"Changed","frames":[{"sha256":"def456"}]}"#;
            std::fs::write(root.join("project.next"), replacement).unwrap();
            std::fs::rename(root.join("project.next"), root.join("project.json")).unwrap();
            std::fs::write(root.join("assets/frames/def456.png"), b"replacement").unwrap();
        });

        assert_eq!(result, ShareOutcome::Complete(dest.clone()));
        assert_eq!(
            std::fs::read(dest.join("project.json")).unwrap(),
            original_manifest
        );
        assert!(dest.join("assets/frames/abc123.png").exists());
        assert!(!dest.join("assets/frames/def456.png").exists());
    }

    #[test]
    fn early_source_failure_cleans_staging_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        let dest = dir.path().join("early-failure-safe-test");
        let request = ShareRequest {
            source_root: root.clone(),
            destination: dest.clone(),
        };

        let result = copy_safe_publish_with_hook(&request, &cancel(), || {
            std::fs::remove_file(root.join("publish/session.json")).unwrap();
        });

        assert!(matches!(result, ShareOutcome::Failed(_)));
        assert!(!dest.exists());
        assert!(
            std::fs::read_dir(dir.path()).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-share-")),
            "an early source failure must remove staging"
        );
    }

    #[test]
    fn existing_empty_destination_is_rejected_without_modification() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        let dest = dir.path().join("existing-empty");
        std::fs::create_dir(&dest).unwrap();

        let result = copy_safe_publish(
            &ShareRequest {
                source_root: root,
                destination: dest.clone(),
            },
            &cancel(),
        );

        assert!(matches!(result, ShareOutcome::Failed(_)));
        assert_eq!(std::fs::read_dir(dest).unwrap().count(), 0);
    }

    #[test]
    fn selected_folder_is_treated_as_share_parent() {
        let parent = Path::new("/tmp/exports");
        let root = Path::new("/tmp/Checkout.rollshot-guide");

        assert_eq!(
            destination_in(parent, root, ShareKind::SafeCopy),
            parent.join("Checkout-safe-viewer")
        );
        assert_eq!(
            destination_in(parent, root, ShareKind::EditableProject),
            parent.join("Checkout.rollshot-guide")
        );
    }

    #[test]
    fn missing_referenced_asset_fails_editable_copy_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let root = setup_project(dir.path());
        std::fs::remove_file(root.join("assets/frames/abc123.png")).unwrap();
        let dest = dir.path().join("missing-asset");

        let result = copy_editable_project(
            &ShareRequest {
                source_root: root,
                destination: dest.clone(),
            },
            &cancel(),
        );

        assert!(matches!(result, ShareOutcome::Failed(_)));
        assert!(!dest.exists());
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

    #[test]
    fn issue_pack_export_respects_cancellation() {
        use crate::issue_pack::{
            ActionGuideExportSource, ActionGuideIssueAssets, EvidenceReviewSummary, IssuePackError,
            IssuePackInput, PlatformInfo, RedactionSummary,
        };
        use rollshot_action::project::PublishCancellation;

        let parent = tempfile::tempdir().unwrap();
        let job = rollshot_action::ReviewedGuideExportJob {
            title: "Test".into(),
            region: rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: rollshot_action::InputSourceKind::LinuxEvdev,
            input_capability: rollshot_action::InputCapability::SemanticEvents,
            steps: vec![rollshot_action::ReviewedGuideStep {
                index: 1,
                title: "Step".into(),
                caption: None,
                kind: rollshot_action::CandidateKind::Click,
                reason: rollshot_action::DetectReason::ClickConfirmed,
                at_ms: 100,
                image: rollshot_action::ReviewedStepImage::Retained(std::sync::Arc::new(
                    image::RgbaImage::new(8, 8),
                )),
                hotspots: Vec::new(),
            }],
        };
        let input = IssuePackInput {
            title: None,
            created_at: chrono::Local::now(),
            rollshot_version: "0.1.0".into(),
            platform: PlatformInfo::current(),
            final_image: None,
            action_guide: Some(ActionGuideIssueAssets::from_job(&job, false)),
            ocr_snippets: Vec::new(),
            evidence_review: EvidenceReviewSummary {
                required: true,
                completed: true,
                result_workspace_images_reviewed: false,
                action_guide_keyframes_reviewed: true,
            },
            redaction: RedactionSummary {
                review_required: false,
                review_completed: true,
                result_workspace_images_are_flattened: false,
                original_pixels_included: false,
                redaction_count: 0,
            },
        };
        let cancel = PublishCancellation::new();
        cancel.cancel();
        let err = crate::issue_pack::export_folder_with_action_guide_cancellable(
            &input,
            Some(ActionGuideExportSource {
                job,
                include_gif: false,
                publish_source: None,
            }),
            parent.path(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err, IssuePackError::Cancelled);
    }
}
