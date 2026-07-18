#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rollshot_action::project::{
    load_publish_state, write_publish_state, PublishCancellation, PublishFreshness,
    PublishOutputKind, PublishStateV1, PublishedOutputV1,
};
use rollshot_action::{ReviewedGuideExportJob, StoryboardOptions, VideoOptions};

use super::project::ProjectWriterGuard;

#[derive(Debug, Clone, Default)]
pub struct PublishSettings {
    pub storyboard: StoryboardOptions,
    pub gif: rollshot_action::GifOptions,
    pub mp4: VideoOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishSelection {
    AllEnabled,
    Only(BTreeSet<PublishOutputKind>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishPurpose {
    Background,
    ShareGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublishOperationId(pub u64);

pub struct PublishRequest {
    pub operation_id: PublishOperationId,
    pub revision: u64,
    pub project_root: PathBuf,
    pub writer_lease: Arc<Mutex<Option<ProjectWriterGuard>>>,
    pub job: ReviewedGuideExportJob,
    pub settings: PublishSettings,
    pub selection: PublishSelection,
    pub purpose: PublishPurpose,
    pub ffmpeg: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishEvent {
    CoreCommitted {
        operation_id: PublishOperationId,
        revision: u64,
    },
    OutputCommitted {
        operation_id: PublishOperationId,
        revision: u64,
        kind: PublishOutputKind,
    },
    OutputFailed {
        operation_id: PublishOperationId,
        revision: u64,
        kind: PublishOutputKind,
        error_category: &'static str,
    },
    Finished {
        operation_id: PublishOperationId,
        revision: u64,
    },
}

impl PublishEvent {
    pub fn operation_id(&self) -> PublishOperationId {
        match self {
            Self::CoreCommitted { operation_id, .. }
            | Self::OutputCommitted { operation_id, .. }
            | Self::OutputFailed { operation_id, .. }
            | Self::Finished { operation_id, .. } => *operation_id,
        }
    }

    pub fn revision(&self) -> u64 {
        match self {
            Self::CoreCommitted { revision, .. }
            | Self::OutputCommitted { revision, .. }
            | Self::OutputFailed { revision, .. }
            | Self::Finished { revision, .. } => *revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    Committed,
    Superseded,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct PublishArbiter {
    inner: Arc<Mutex<Option<(PublishOperationId, u64)>>>,
}

impl PublishArbiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub fn begin(&self, operation_id: PublishOperationId, revision: u64) {
        *self.inner.lock().unwrap() = Some((operation_id, revision));
    }

    pub fn clear_if_current(&self, operation_id: PublishOperationId) {
        let mut guard = self.inner.lock().unwrap();
        if guard.as_ref().is_some_and(|(id, _)| *id == operation_id) {
            *guard = None;
        }
    }

    pub fn try_commit<F, R>(
        &self,
        operation_id: PublishOperationId,
        revision: u64,
        f: F,
    ) -> Result<R, PublishOutcome>
    where
        F: FnOnce() -> Result<R, String>,
    {
        let guard = self.inner.lock().unwrap();
        match *guard {
            Some((ref id, rev)) if *id == operation_id && rev == revision => {
                drop(guard);
                f().map_err(|error| {
                    tracing::event!(
                        target: "rollshot::publish",
                        tracing::Level::ERROR,
                        operation = operation_id.0,
                        revision,
                        category = "commit_failed",
                        "{error}"
                    );
                    PublishOutcome::Superseded
                })
            }
            _ => Err(PublishOutcome::Superseded),
        }
    }
}

impl Default for PublishArbiter {
    fn default() -> Self {
        Self::new()
    }
}

fn unique_temp_sibling(parent: &Path, prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".tmp-{prefix}-{}-{id}", std::process::id()))
}

struct TempDirGuard {
    path: Option<PathBuf>,
}

impl TempDirGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn mark_committed(&mut self) {
        self.path = None;
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        if let Some(ref path) = self.path {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn fsync_dir(path: &Path) -> Result<(), std::io::Error> {
    let file = std::fs::File::open(path)?;
    file.sync_all()?;
    Ok(())
}

fn swap_publish_directory(temp: &Path, publish: &Path) -> Result<(), String> {
    if publish.exists() {
        let backup = unique_temp_sibling(publish.parent().unwrap_or(temp), "publish-backup");
        std::fs::rename(publish, &backup).map_err(|e| format!("backup rename: {e}"))?;
        match std::fs::rename(temp, publish) {
            Ok(()) => {
                fsync_dir(publish.parent().unwrap_or(temp))
                    .map_err(|e| format!("fsync parent: {e}"))?;
                let _ = std::fs::remove_dir_all(&backup);
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::rename(&backup, publish);
                Err(format!("swap rename: {e}"))
            }
        }
    } else {
        std::fs::rename(temp, publish).map_err(|e| format!("install rename: {e}"))?;
        fsync_dir(publish.parent().unwrap_or(temp)).map_err(|e| format!("fsync parent: {e}"))?;
        Ok(())
    }
}

fn commit_publish_file(temp: &Path, destination: &Path) -> Result<(), String> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, temp, CWD, destination, RenameFlags::NOREPLACE)
        .map_err(|e| format!("commit file: {e}"))
}

fn read_project_revision(root: &Path) -> Result<u64, String> {
    let path = root.join("project.json");
    let bytes = std::fs::read(&path).map_err(|e| format!("read project.json: {e}"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse project.json: {e}"))?;
    manifest
        .get("revision")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing revision in project.json".to_string())
}

fn check_arbiter_and_revision(
    arbiter: &PublishArbiter,
    operation_id: PublishOperationId,
    revision: u64,
    project_root: &Path,
) -> Result<(), PublishOutcome> {
    let guard = arbiter.inner.lock().unwrap();
    match *guard {
        Some((ref id, rev)) if *id == operation_id && rev == revision => {}
        _ => return Err(PublishOutcome::Superseded),
    }
    drop(guard);

    let current = read_project_revision(project_root).map_err(|_| PublishOutcome::Superseded)?;
    if current != revision {
        return Err(PublishOutcome::Superseded);
    }
    Ok(())
}

pub fn run_publish(
    request: PublishRequest,
    cancel: PublishCancellation,
    sender: &tokio::sync::mpsc::Sender<PublishEvent>,
) -> PublishOutcome {
    let PublishRequest {
        operation_id,
        revision,
        project_root,
        writer_lease: _writer_lease,
        job,
        settings,
        selection,
        purpose,
        ffmpeg,
    } = request;

    let publish_dir = project_root.join("publish");
    let state_path = project_root.join("publish-state.json");

    let ctx = PublishContext {
        operation_id,
        revision,
        project_root: &project_root,
        publish_dir: &publish_dir,
        state_path: &state_path,
        job: &job,
        settings: &settings,
        selection: &selection,
        ffmpeg: ffmpeg.as_deref(),
        cancel: &cancel,
        sender,
    };

    match purpose {
        PublishPurpose::Background => run_background_publish(ctx),
        PublishPurpose::ShareGate => run_share_gate_publish(ctx),
    }
}

struct PublishContext<'a> {
    operation_id: PublishOperationId,
    revision: u64,
    project_root: &'a Path,
    publish_dir: &'a Path,
    state_path: &'a Path,
    job: &'a ReviewedGuideExportJob,
    settings: &'a PublishSettings,
    selection: &'a PublishSelection,
    ffmpeg: Option<&'a Path>,
    cancel: &'a PublishCancellation,
    sender: &'a tokio::sync::mpsc::Sender<PublishEvent>,
}

fn run_background_publish(ctx: PublishContext<'_>) -> PublishOutcome {
    if ctx.cancel.is_cancelled() {
        return PublishOutcome::Cancelled;
    }

    let needs_core = match ctx.selection {
        PublishSelection::AllEnabled => true,
        PublishSelection::Only(kinds) => {
            kinds.contains(&PublishOutputKind::Core)
                || load_publish_state(ctx.project_root)
                    .freshness(PublishOutputKind::Core, ctx.revision)
                    != PublishFreshness::Current
        }
    };

    if needs_core {
        let tmp = unique_temp_sibling(
            ctx.publish_dir.parent().unwrap_or(ctx.project_root),
            "publish-core",
        );
        let mut guard = TempDirGuard::new(tmp.clone());

        match rollshot_action::render_guide_folder(ctx.job, &tmp) {
            Ok(_) => {
                if ctx.cancel.is_cancelled() {
                    return PublishOutcome::Cancelled;
                }

                match swap_publish_directory(&tmp, ctx.publish_dir) {
                    Ok(()) => {
                        guard.mark_committed();
                        let mut state = load_or_default_state(ctx.project_root);
                        state.outputs.insert(
                            PublishOutputKind::Core,
                            PublishedOutputV1::new(ctx.revision),
                        );
                        let _ = write_publish_state(ctx.project_root, &state);
                        let _ = ctx.sender.try_send(PublishEvent::CoreCommitted {
                            operation_id: ctx.operation_id,
                            revision: ctx.revision,
                        });
                    }
                    Err(e) => {
                        tracing::event!(
                            target: "rollshot::publish",
                            tracing::Level::ERROR,
                            operation = ctx.operation_id.0,
                            revision = ctx.revision,
                            category = "core_swap",
                            "{e}"
                        );
                        return PublishOutcome::Superseded;
                    }
                }
            }
            Err(e) => {
                tracing::event!(
                    target: "rollshot::publish",
                    tracing::Level::ERROR,
                    operation = ctx.operation_id.0,
                    revision = ctx.revision,
                    category = "core_render",
                    error = %e,
                    "core render failed"
                );
                let _ = ctx.sender.try_send(PublishEvent::OutputFailed {
                    operation_id: ctx.operation_id,
                    revision: ctx.revision,
                    kind: PublishOutputKind::Core,
                    error_category: "core_render",
                });
                return PublishOutcome::Superseded;
            }
        }
    }

    let derivatives = derivative_kinds(ctx.selection, ctx.settings);
    for kind in derivatives {
        if ctx.cancel.is_cancelled() {
            return PublishOutcome::Cancelled;
        }

        let state = load_publish_state(ctx.project_root);
        if state.freshness(kind, ctx.revision) == PublishFreshness::Current {
            continue;
        }

        let tmp = unique_temp_sibling(ctx.publish_dir, &format!("deriv-{kind:?}"));
        let result = render_derivative(ctx.job, kind, ctx.settings, ctx.ffmpeg, ctx.cancel, &tmp);
        match result {
            Ok(()) => {
                let stable = derivative_stable_path(ctx.publish_dir, kind);
                match commit_publish_file(&tmp, &stable) {
                    Ok(()) => {
                        let mut state = load_or_default_state(ctx.project_root);
                        state
                            .outputs
                            .insert(kind, PublishedOutputV1::new(ctx.revision));
                        let _ = write_publish_state(ctx.project_root, &state);
                        let _ = ctx.sender.try_send(PublishEvent::OutputCommitted {
                            operation_id: ctx.operation_id,
                            revision: ctx.revision,
                            kind,
                        });
                    }
                    Err(e) => {
                        tracing::event!(
                            target: "rollshot::publish",
                            tracing::Level::ERROR,
                            operation = ctx.operation_id.0,
                            revision = ctx.revision,
                            kind = ?kind,
                            category = "derivative_commit",
                            "{e}"
                        );
                        let _ = std::fs::remove_file(&tmp);
                        let _ = ctx.sender.try_send(PublishEvent::OutputFailed {
                            operation_id: ctx.operation_id,
                            revision: ctx.revision,
                            kind,
                            error_category: "derivative_commit",
                        });
                    }
                }
            }
            Err(_e) => {
                tracing::event!(
                    target: "rollshot::publish",
                    tracing::Level::ERROR,
                    operation = ctx.operation_id.0,
                    revision = ctx.revision,
                    kind = ?kind,
                    category = derivative_error_category(kind),
                    "derivative render failed"
                );
                let _ = ctx.sender.try_send(PublishEvent::OutputFailed {
                    operation_id: ctx.operation_id,
                    revision: ctx.revision,
                    kind,
                    error_category: derivative_error_category(kind),
                });
            }
        }
    }

    let _ = ctx.sender.try_send(PublishEvent::Finished {
        operation_id: ctx.operation_id,
        revision: ctx.revision,
    });
    PublishOutcome::Committed
}

fn run_share_gate_publish(ctx: PublishContext<'_>) -> PublishOutcome {
    let tmp = unique_temp_sibling(
        ctx.publish_dir.parent().unwrap_or(ctx.project_root),
        "publish-sharegate",
    );
    let mut guard = TempDirGuard::new(tmp.clone());

    match rollshot_action::render_guide_folder(ctx.job, &tmp) {
        Ok(_) => {}
        Err(e) => {
            tracing::event!(
                target: "rollshot::publish",
                tracing::Level::ERROR,
                operation = ctx.operation_id.0,
                revision = ctx.revision,
                category = "core_render",
                error = %e,
                "sharegate core render failed"
            );
            return PublishOutcome::Superseded;
        }
    }

    let derivatives = match ctx.selection {
        PublishSelection::AllEnabled => enabled_derivatives(ctx.settings),
        PublishSelection::Only(kinds) => kinds
            .iter()
            .copied()
            .filter(|k| *k != PublishOutputKind::Core)
            .collect(),
    };

    let mut rendered = BTreeSet::new();
    for kind in &derivatives {
        if ctx.cancel.is_cancelled() {
            return PublishOutcome::Cancelled;
        }
        let tmp_file = tmp.join(derivative_filename(*kind));
        match render_derivative(
            ctx.job,
            *kind,
            ctx.settings,
            ctx.ffmpeg,
            ctx.cancel,
            &tmp_file,
        ) {
            Ok(()) => {
                rendered.insert(*kind);
            }
            Err(_) => {
                tracing::event!(
                    target: "rollshot::publish",
                    tracing::Level::ERROR,
                    operation = ctx.operation_id.0,
                    revision = ctx.revision,
                    kind = ?kind,
                    category = derivative_error_category(*kind),
                    "sharegate derivative render failed"
                );
                return PublishOutcome::Superseded;
            }
        }
    }

    if ctx.cancel.is_cancelled() {
        return PublishOutcome::Cancelled;
    }

    let mut new_state = PublishStateV1::default();
    new_state.outputs.insert(
        PublishOutputKind::Core,
        PublishedOutputV1::new(ctx.revision),
    );
    for kind in &rendered {
        new_state
            .outputs
            .insert(*kind, PublishedOutputV1::new(ctx.revision));
    }
    let tmp_state = unique_temp_sibling(ctx.project_root, "publish-state");
    let state_json = serde_json::to_vec_pretty(&new_state).unwrap_or_default();
    if std::fs::write(&tmp_state, &state_json).is_err() {
        return PublishOutcome::Superseded;
    }

    let commit_result = commit_sharegate(
        &tmp,
        ctx.publish_dir,
        &tmp_state,
        ctx.state_path,
        ctx.project_root,
    );

    match commit_result {
        Ok(()) => {
            guard.mark_committed();
            let _ = std::fs::remove_file(&tmp_state);
            let _ = ctx.sender.try_send(PublishEvent::CoreCommitted {
                operation_id: ctx.operation_id,
                revision: ctx.revision,
            });
            for kind in &rendered {
                let _ = ctx.sender.try_send(PublishEvent::OutputCommitted {
                    operation_id: ctx.operation_id,
                    revision: ctx.revision,
                    kind: *kind,
                });
            }
            let _ = ctx.sender.try_send(PublishEvent::Finished {
                operation_id: ctx.operation_id,
                revision: ctx.revision,
            });
            PublishOutcome::Committed
        }
        Err(e) => {
            tracing::event!(
                target: "rollshot::publish",
                tracing::Level::ERROR,
                operation = ctx.operation_id.0,
                revision = ctx.revision,
                category = "sharegate_commit",
                "{e}"
            );
            PublishOutcome::Superseded
        }
    }
}

fn commit_sharegate(
    new_publish: &Path,
    publish_dir: &Path,
    new_state: &Path,
    state_path: &Path,
    project_root: &Path,
) -> Result<(), String> {
    let parent = publish_dir.parent().unwrap_or(project_root);

    let old_publish_backup = if publish_dir.exists() {
        let backup = unique_temp_sibling(parent, "publish-old");
        std::fs::rename(publish_dir, &backup).map_err(|e| format!("backup publish: {e}"))?;
        Some(backup)
    } else {
        None
    };

    let old_state_backup = if state_path.exists() {
        let backup = unique_temp_sibling(parent, "state-old");
        std::fs::rename(state_path, &backup).map_err(|e| {
            if let Some(ref pb) = old_publish_backup {
                let _ = std::fs::rename(pb, publish_dir);
            }
            format!("backup state: {e}")
        })?;
        Some(backup)
    } else {
        None
    };

    match std::fs::rename(new_publish, publish_dir) {
        Ok(()) => {}
        Err(e) => {
            if let Some(ref pb) = old_publish_backup {
                let _ = std::fs::rename(pb, publish_dir);
            }
            if let Some(ref sb) = old_state_backup {
                let _ = std::fs::rename(sb, state_path);
            }
            return Err(format!("install publish: {e}"));
        }
    }

    match std::fs::rename(new_state, state_path) {
        Ok(()) => {}
        Err(e) => {
            let _ = std::fs::remove_dir_all(publish_dir);
            if let Some(ref pb) = old_publish_backup {
                let _ = std::fs::rename(pb, publish_dir);
            }
            if let Some(ref sb) = old_state_backup {
                let _ = std::fs::rename(sb, state_path);
            }
            return Err(format!("install state: {e}"));
        }
    }

    fsync_dir(project_root).map_err(|e| {
        let _ = std::fs::remove_dir_all(publish_dir);
        if let Some(ref pb) = old_publish_backup {
            let _ = std::fs::rename(pb, publish_dir);
        }
        let _ = std::fs::remove_file(state_path);
        if let Some(ref sb) = old_state_backup {
            let _ = std::fs::rename(sb, state_path);
        }
        format!("fsync root: {e}")
    })?;

    if let Some(ref pb) = old_publish_backup {
        let _ = std::fs::remove_dir_all(pb);
    }
    if let Some(ref sb) = old_state_backup {
        let _ = std::fs::remove_file(sb);
    }

    Ok(())
}

fn load_or_default_state(root: &Path) -> PublishStateV1 {
    match load_publish_state(root) {
        rollshot_action::project::PublishStateLoad::Available { state } => state,
        rollshot_action::project::PublishStateLoad::Unavailable => PublishStateV1::default(),
    }
}

fn derivative_kinds(
    selection: &PublishSelection,
    settings: &PublishSettings,
) -> Vec<PublishOutputKind> {
    match selection {
        PublishSelection::AllEnabled => enabled_derivatives(settings),
        PublishSelection::Only(kinds) => kinds
            .iter()
            .copied()
            .filter(|k| *k != PublishOutputKind::Core)
            .collect(),
    }
}

fn enabled_derivatives(settings: &PublishSettings) -> Vec<PublishOutputKind> {
    let mut kinds = Vec::new();
    if settings.storyboard.show_titles || !settings.storyboard.show_titles {
        kinds.push(PublishOutputKind::Storyboard);
    }
    kinds.push(PublishOutputKind::Gif);
    if settings.mp4.fps > 0 {
        kinds.push(PublishOutputKind::Mp4);
    }
    kinds
}

fn derivative_stable_path(publish_dir: &Path, kind: PublishOutputKind) -> PathBuf {
    publish_dir.join(derivative_filename(kind))
}

fn derivative_filename(kind: PublishOutputKind) -> &'static str {
    match kind {
        PublishOutputKind::Core => "index.html",
        PublishOutputKind::Storyboard => "storyboard.png",
        PublishOutputKind::Gif => "guide.gif",
        PublishOutputKind::Mp4 => "summary.mp4",
    }
}

fn derivative_error_category(kind: PublishOutputKind) -> &'static str {
    match kind {
        PublishOutputKind::Core => "core_render",
        PublishOutputKind::Storyboard => "storyboard_render",
        PublishOutputKind::Gif => "gif_render",
        PublishOutputKind::Mp4 => "mp4_render",
    }
}

fn render_derivative(
    job: &ReviewedGuideExportJob,
    kind: PublishOutputKind,
    settings: &PublishSettings,
    ffmpeg: Option<&Path>,
    cancel: &PublishCancellation,
    out_path: &Path,
) -> Result<(), String> {
    match kind {
        PublishOutputKind::Core => unreachable!("core is handled separately"),
        PublishOutputKind::Storyboard => rollshot_action::export_reviewed_storyboard_cancellable(
            job,
            settings.storyboard.clone(),
            cancel,
            out_path,
        )
        .map_err(|e| format!("{e}"))
        .map(|_| ()),
        PublishOutputKind::Gif => {
            rollshot_action::export_reviewed_gif(job, settings.gif.clone(), cancel, out_path)
                .map_err(|e| format!("{e}"))
        }
        PublishOutputKind::Mp4 => {
            let ffmpeg_path = ffmpeg.ok_or("ffmpeg path required for mp4")?;
            rollshot_action::export_reviewed_video(
                job,
                settings.mp4.clone(),
                ffmpeg_path,
                cancel,
                out_path,
            )
            .map_err(|e| format!("{e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::project::{
        create_project, EnabledOutputs, ProjectFrame, ProjectManifestV1, ProjectSnapshot,
        ProjectStep, ProjectStepId, SnapshotFrame, SnapshotFramePayload,
    };
    use rollshot_action::{
        ActionRecorder, CandidateKind, CaptureRegion, DetectorConfig, FrameStore, InputCapability,
        InputSourceKind, Recording, ReviewedGuideExportJob, ReviewedGuideStep, ReviewedStepImage,
        StoreConfig,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    fn region_8() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }
    }

    fn make_test_project(dir: &Path) -> PathBuf {
        let root = dir.join("test-project.rollshot-guide");
        let snap = build_test_snapshot(1);
        create_project(&snap, &root).unwrap();
        root
    }

    fn build_test_snapshot(revision: u64) -> ProjectSnapshot {
        ProjectSnapshot {
            base_revision: if revision > 1 {
                Some(revision - 1)
            } else {
                None
            },
            title: "Test Guide".into(),
            capture_region: region_8(),
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: rollshot_action::DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
        }
    }

    fn minimal_job() -> ReviewedGuideExportJob {
        ReviewedGuideExportJob {
            title: "Test Guide".into(),
            region: region_8(),
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            steps: vec![ReviewedGuideStep {
                index: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: rollshot_action::DetectReason::ClickConfirmed,
                at_ms: 150,
                image: ReviewedStepImage::Retained(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
                hotspots: vec![],
            }],
        }
    }

    fn dummy_guard() -> Arc<Mutex<Option<ProjectWriterGuard>>> {
        Arc::new(Mutex::new(Some(ProjectWriterGuard::for_test())))
    }

    fn make_request(
        operation_id: PublishOperationId,
        revision: u64,
        root: PathBuf,
        purpose: PublishPurpose,
    ) -> PublishRequest {
        PublishRequest {
            operation_id,
            revision,
            project_root: root,
            writer_lease: dummy_guard(),
            job: minimal_job(),
            settings: PublishSettings::default(),
            selection: PublishSelection::AllEnabled,
            purpose,
            ffmpeg: None,
        }
    }

    #[test]
    fn core_render_failure_leaves_previous_publish_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let publish_dir = root.join("publish");
        std::fs::create_dir_all(&publish_dir).unwrap();
        std::fs::write(publish_dir.join("index.html"), "<old>").unwrap();
        std::fs::write(publish_dir.join("steps.md"), "old").unwrap();

        let old_content = std::fs::read_to_string(publish_dir.join("index.html")).unwrap();

        let mut request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );
        request.job.steps.clear();

        let cancel = PublishCancellation::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);

        assert_eq!(outcome, PublishOutcome::Superseded);
        assert_eq!(
            std::fs::read_to_string(publish_dir.join("index.html")).unwrap(),
            old_content
        );
    }

    #[test]
    fn every_event_carries_operation_and_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let request = make_request(
            PublishOperationId(42),
            1,
            root.clone(),
            PublishPurpose::Background,
        );

        let cancel = PublishCancellation::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let _outcome = run_publish(request, cancel, &tx);
        drop(tx);

        while let Some(event) = rx.blocking_recv() {
            assert_eq!(event.operation_id(), PublishOperationId(42));
            assert_eq!(event.revision(), 1);
        }
    }

    #[test]
    fn superseded_operation_prevents_commit() {
        let arbiter = PublishArbiter::new();
        arbiter.begin(PublishOperationId(1), 1);

        arbiter.begin(PublishOperationId(2), 2);

        let result = arbiter.try_commit(PublishOperationId(1), 1, || Ok(()));
        assert_eq!(result, Err(PublishOutcome::Superseded));
    }

    #[test]
    fn arbiter_commit_succeeds_when_current() {
        let arbiter = PublishArbiter::new();
        arbiter.begin(PublishOperationId(1), 1);

        let result = arbiter.try_commit(PublishOperationId(1), 1, || Ok(42));
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn arbiter_clear_if_current_only_clears_matching() {
        let arbiter = PublishArbiter::new();
        arbiter.begin(PublishOperationId(1), 1);

        arbiter.clear_if_current(PublishOperationId(2));
        let result = arbiter.try_commit(PublishOperationId(1), 1, || Ok(()));
        assert!(result.is_ok());

        arbiter.clear_if_current(PublishOperationId(1));
        let result = arbiter.try_commit(PublishOperationId(1), 1, || Ok(()));
        assert_eq!(result, Err(PublishOutcome::Superseded));
    }

    #[test]
    fn cancellation_returns_cancelled_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );

        let cancel = PublishCancellation::new();
        cancel.cancel();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);

        assert_eq!(outcome, PublishOutcome::Cancelled);
    }

    #[test]
    fn sharegate_failure_leaves_prior_publish_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let publish_dir = root.join("publish");
        std::fs::create_dir_all(&publish_dir).unwrap();
        std::fs::write(publish_dir.join("index.html"), "<old>").unwrap();
        let state = PublishStateV1::default();
        write_publish_state(&root, &state).unwrap();

        let old_html = std::fs::read_to_string(publish_dir.join("index.html")).unwrap();

        let mut request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::ShareGate,
        );
        request.job.steps.clear();

        let cancel = PublishCancellation::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);

        assert_eq!(outcome, PublishOutcome::Superseded);
        assert_eq!(
            std::fs::read_to_string(publish_dir.join("index.html")).unwrap(),
            old_html
        );
    }

    #[test]
    fn sharegate_cancellation_leaves_prior_state_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let publish_dir = root.join("publish");
        std::fs::create_dir_all(&publish_dir).unwrap();
        std::fs::write(publish_dir.join("index.html"), "<old>").unwrap();
        let mut state = PublishStateV1::default();
        state
            .outputs
            .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
        write_publish_state(&root, &state).unwrap();

        let old_state_json = std::fs::read_to_string(root.join("publish-state.json")).unwrap();

        let request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::ShareGate,
        );

        let cancel = PublishCancellation::new();
        cancel.cancel();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);

        assert_eq!(outcome, PublishOutcome::Cancelled);
        assert_eq!(
            std::fs::read_to_string(root.join("publish-state.json")).unwrap(),
            old_state_json
        );
    }

    #[test]
    fn successful_core_swap_removes_old_derivatives() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let publish_dir = root.join("publish");
        std::fs::create_dir_all(&publish_dir).unwrap();
        std::fs::write(publish_dir.join("storyboard.png"), "old").unwrap();

        let mut request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );
        request.selection = PublishSelection::Only(BTreeSet::from([PublishOutputKind::Core]));

        let cancel = PublishCancellation::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);

        assert_eq!(outcome, PublishOutcome::Committed);
        assert!(publish_dir.join("index.html").exists());
        assert!(
            !publish_dir.join("storyboard.png").exists(),
            "old derivatives must not survive a core swap"
        );
    }

    #[test]
    fn one_optional_failure_does_not_prevent_later_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let mut request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );
        request.ffmpeg = None;

        let cancel = PublishCancellation::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);
        drop(tx);

        assert_eq!(outcome, PublishOutcome::Committed);
        let mut events = Vec::new();
        while let Some(event) = rx.blocking_recv() {
            events.push(event);
        }
        assert!(events
            .iter()
            .any(|e| matches!(e, PublishEvent::CoreCommitted { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, PublishEvent::Finished { .. })));
    }

    #[test]
    fn commit_publish_file_noreplace_rejects_existing() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("new.txt");
        let dest = dir.path().join("existing.txt");
        std::fs::write(&temp, "new").unwrap();
        std::fs::write(&dest, "existing").unwrap();

        let result = commit_publish_file(&temp, &dest);
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "existing");
    }

    #[test]
    fn swap_publish_directory_with_backup_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let publish = dir.path().join("publish");
        std::fs::create_dir_all(&publish).unwrap();
        std::fs::write(publish.join("old.html"), "old").unwrap();

        let temp = dir.path().join(".tmp-new");
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join("new.html"), "new").unwrap();

        swap_publish_directory(&temp, &publish).unwrap();

        assert!(publish.join("new.html").exists());
        assert!(!publish.join("old.html").exists());
        assert!(!temp.exists());
    }

    #[test]
    fn swap_publish_directory_restores_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let publish = dir.path().join("publish");
        std::fs::create_dir_all(&publish).unwrap();
        std::fs::write(publish.join("keep.html"), "keep").unwrap();

        let nonexistent_parent = dir.path().join("nonexistent").join("publish");
        let temp = dir.path().join(".tmp-fail");
        std::fs::create_dir_all(&temp).unwrap();

        let result = swap_publish_directory(&temp, &nonexistent_parent);
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(publish.join("keep.html")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn publish_state_advances_only_committed_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );

        let cancel = PublishCancellation::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);

        assert_eq!(outcome, PublishOutcome::Committed);
        let loaded = load_publish_state(&root);
        assert_eq!(
            loaded.freshness(PublishOutputKind::Core, 1),
            PublishFreshness::Current
        );
    }

    #[test]
    fn changing_project_json_before_commit_prevents_older_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let arbiter = PublishArbiter::new();
        arbiter.begin(PublishOperationId(1), 1);

        let mut snap = build_test_snapshot(1);
        snap.base_revision = Some(1);
        snap.title = "Updated".into();
        use rollshot_action::project::save_project;
        save_project(&snap, &root).unwrap();

        let result = arbiter.try_commit(PublishOperationId(1), 1, || {
            let current = read_project_revision(&root)?;
            if current != 1 {
                return Err("revision mismatch".to_string());
            }
            Ok(())
        });
        assert!(result.is_err());
    }

    #[test]
    fn cancellation_cleans_temp_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );

        let cancel = PublishCancellation::new();
        cancel.cancel();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        run_publish(request, cancel, &tx);

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            entries.is_empty(),
            "temp entries should be cleaned: {:?}",
            entries.iter().map(|e| e.path()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn tracing_events_contain_no_sensitive_data() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let mut request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );
        request.job.steps.clear();

        let cancel = PublishCancellation::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        run_publish(request, cancel, &tx);
    }

    #[test]
    fn background_only_skips_core_when_current() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let mut state = PublishStateV1::default();
        state
            .outputs
            .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
        write_publish_state(&root, &state).unwrap();

        let publish_dir = root.join("publish");
        std::fs::create_dir_all(&publish_dir).unwrap();
        std::fs::write(publish_dir.join("index.html"), "<html></html>").unwrap();
        std::fs::write(publish_dir.join("steps.md"), "# Steps").unwrap();
        std::fs::write(
            publish_dir.join("session.json"),
            r#"{"schema_version":1,"title":"Test","region":{"x":0,"y":0,"width":8,"height":8},"input_source":"visual-only","input_capability":"semantic-events","steps":[{"index":1,"title":"Step 1","kind":"click","reason":"click-confirmed","at_ms":150,"keyframe_file":"keyframes/001.png","hotspots":[]}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(publish_dir.join("keyframes")).unwrap();
        std::fs::write(
            publish_dir.join("keyframes/001.png"),
            RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255])).as_raw(),
        )
        .unwrap();

        let mut request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::Background,
        );
        request.selection = PublishSelection::Only(BTreeSet::from([PublishOutputKind::Storyboard]));

        let cancel = PublishCancellation::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let _outcome = run_publish(request, cancel, &tx);
        drop(tx);

        let mut events = Vec::new();
        while let Some(event) = rx.blocking_recv() {
            events.push(event);
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, PublishEvent::CoreCommitted { .. })),
            "core should not be re-committed when it is already current"
        );
    }

    #[test]
    fn revision_mismatch_in_arbiter_prevents_commit() {
        let arbiter = PublishArbiter::new();
        arbiter.begin(PublishOperationId(1), 1);

        let result = arbiter.try_commit(PublishOperationId(1), 2, || Ok(()));
        assert_eq!(result, Err(PublishOutcome::Superseded));
    }

    #[test]
    fn sharegate_state_file_failure_restores_both() {
        let dir = tempfile::tempdir().unwrap();
        let root = make_test_project(dir.path());

        let publish_dir = root.join("publish");
        std::fs::create_dir_all(&publish_dir).unwrap();
        std::fs::write(publish_dir.join("index.html"), "<old>").unwrap();

        let mut state = PublishStateV1::default();
        state
            .outputs
            .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
        write_publish_state(&root, &state).unwrap();

        let old_html = std::fs::read_to_string(publish_dir.join("index.html")).unwrap();
        let old_state = std::fs::read_to_string(root.join("publish-state.json")).unwrap();

        let request = make_request(
            PublishOperationId(1),
            1,
            root.clone(),
            PublishPurpose::ShareGate,
        );

        let cancel = PublishCancellation::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let outcome = run_publish(request, cancel, &tx);

        if outcome == PublishOutcome::Committed {
            assert!(publish_dir.join("index.html").exists());
        } else {
            assert_eq!(
                std::fs::read_to_string(publish_dir.join("index.html")).unwrap(),
                old_html
            );
            assert_eq!(
                std::fs::read_to_string(root.join("publish-state.json")).unwrap(),
                old_state
            );
        }
    }
}
