pub mod probe;
pub mod process;
pub mod scratch;
pub mod selection;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use probe::{parse_probe_json, probe_args, ProbeMetadata, VideoToolchain};
pub use process::run_cancellable_child;
pub use scratch::{cleanup_stale_import_scratch, ImportedScratch};
pub use selection::{evidence_sample_indices, CandidateSelector, SelectionResult};

use crate::models::{
    CandidateKind, CaptureRegion, DegradedReason, DetectReason, ImportWarning, InputCapability,
    InputSourceKind,
};
use crate::project::ProjectFrame;
use crate::Guide;

pub const ANALYSIS_FPS: u64 = 2;
pub const ANALYSIS_WIDTH: u32 = 384;
pub const MAX_GENERATED_STEPS: usize = 200;
pub const REDUCTION_BUCKETS: usize = 198;
pub const EVIDENCE_MAX_LONG_EDGE: u32 = 1920;
pub const MAX_EVIDENCE_FRAMES: usize = 600;
pub const MAX_ANALYSIS_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SCRATCH_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoImportPass {
    Preflight,
    Analyze,
    Extract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoImportProgress {
    pub pass: VideoImportPass,
    pub processed_ms: u64,
    pub total_ms: u64,
    pub retained_candidates: usize,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum VideoImportError {
    #[error("Video metadata could not be read.")]
    ProbeFailed,
    #[error("The selected file has no readable video stream.")]
    MissingVideoStream,
    #[error("The selected video has invalid dimensions or duration.")]
    InvalidVideoMetadata,
    #[error("The video decoder is unavailable.")]
    DecoderUnavailable,
    #[error("The video could not be decoded.")]
    DecodeFailed,
    #[error("Required evidence could not be extracted.")]
    EvidenceMissing,
    #[error("Temporary evidence storage failed.")]
    ScratchIo,
    #[error("The recording exceeds an internal resource bound.")]
    ResourceLimit,
    #[error("Import was cancelled.")]
    Cancelled,
}

impl VideoImportError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::ProbeFailed => "probe_failed",
            Self::MissingVideoStream => "missing_video_stream",
            Self::InvalidVideoMetadata => "invalid_video_metadata",
            Self::DecoderUnavailable => "decoder_unavailable",
            Self::DecodeFailed => "decode_failed",
            Self::EvidenceMissing => "evidence_missing",
            Self::ScratchIo => "scratch_io",
            Self::ResourceLimit => "resource_limit",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Default)]
pub struct VideoImportCancellation(Arc<AtomicBool>);

impl VideoImportCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub struct VideoImportRequest {
    pub input: PathBuf,
    pub toolchain: VideoToolchain,
    pub scratch_parent: PathBuf,
}

pub struct ImportedWorkspaceSeed {
    pub guide: Guide,
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub frames: Vec<ProjectFrame>,
    pub import_warnings: Vec<ImportWarning>,
    pub scratch: ImportedScratch,
}

impl std::fmt::Debug for ImportedWorkspaceSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportedWorkspaceSeed")
            .field("guide", &self.guide)
            .field("capture_region", &self.capture_region)
            .field("input_source", &self.input_source)
            .field("frames_count", &self.frames.len())
            .field("import_warnings", &self.import_warnings)
            .finish()
    }
}

pub fn import_video(
    request: VideoImportRequest,
    cancel: VideoImportCancellation,
    progress: impl Fn(VideoImportProgress) + Send + Sync,
) -> Result<ImportedWorkspaceSeed, VideoImportError> {
    use crate::detector::{Detector, DetectorConfig};
    use crate::frame_store::AnalysisFrame;
    use crate::models::FrameId;

    progress(VideoImportPass::Preflight.progress(0, 0, 0));

    let meta = process::probe_video(&request.input, &request.toolchain, &cancel)?;

    if cancel.is_cancelled() {
        return Err(VideoImportError::Cancelled);
    }

    progress(VideoImportPass::Analyze.progress(0, meta.duration_ms, 0));

    let frame_size = checked_frame_size(ANALYSIS_WIDTH, meta)?;

    let mut selector = CandidateSelector::new(meta.duration_ms);
    let mut detector = Detector::new(DetectorConfig::default());
    let mut last_sample_index: u64 = 0;
    let mut last_progress_ms: u64 = 0;
    let mut analysis_at_ms: Vec<u64> = Vec::new();

    let cancel_ref = &cancel;
    let progress_ref = &progress;

    process::run_analysis_pass(
        &request.input,
        &request.toolchain,
        meta,
        frame_size,
        cancel.clone(),
        |sample_index, luma| {
            last_sample_index = sample_index;
            let at_ms = sample_index * 1000 / ANALYSIS_FPS;
            analysis_at_ms.push(at_ms);

            let frame = AnalysisFrame {
                id: sample_index as FrameId,
                at_ms,
                luma,
            };
            if let Some(marker) = detector.observe_frame(&frame) {
                selector.push(marker);
            }

            let progress_ms = at_ms;
            if progress_ms.saturating_sub(last_progress_ms) >= 500 {
                last_progress_ms = progress_ms;
                progress_ref(VideoImportPass::Analyze.progress(
                    progress_ms,
                    meta.duration_ms,
                    selector.count(),
                ));
            }
        },
    )?;

    if let Some(marker) = detector.finish() {
        selector.push(marker);
    }

    let selection = selector.finish();

    if cancel_ref.is_cancelled() {
        return Err(VideoImportError::Cancelled);
    }

    progress(VideoImportPass::Extract.progress(0, meta.duration_ms, selection.candidates.len()));

    let center_indices: Vec<usize> = selection
        .candidates
        .iter()
        .map(|c| c.center_id as usize)
        .collect();

    let evidence_indices = if center_indices.is_empty() {
        // Zero candidates: extract the final sample as fallback evidence.
        vec![last_sample_index as usize]
    } else {
        evidence_sample_indices(&center_indices, last_sample_index as usize + 1)
    };

    assert!(
        evidence_indices.len() <= MAX_EVIDENCE_FRAMES,
        "evidence indices {} exceeds MAX_EVIDENCE_FRAMES",
        evidence_indices.len()
    );

    let mut scratch = ImportedScratch::create(&request.scratch_parent)
        .map_err(|_| VideoImportError::ScratchIo)?;

    let staging = scratch.root().join("staging");
    std::fs::create_dir_all(&staging).map_err(|_| VideoImportError::ScratchIo)?;

    let extracted = process::run_evidence_pass(
        &request.input,
        &request.toolchain,
        meta,
        &evidence_indices,
        &staging,
        cancel_ref,
        &progress_ref,
        meta.duration_ms,
    )?;

    if cancel_ref.is_cancelled() {
        return Err(VideoImportError::Cancelled);
    }

    let mut frames = Vec::new();
    let assets_dir = scratch.root().join("assets/frames");
    std::fs::create_dir_all(&assets_dir).map_err(|_| VideoImportError::ScratchIo)?;

    let mut evidence_w = 0u32;
    let mut evidence_h = 0u32;

    for (i, &requested_idx) in evidence_indices.iter().enumerate() {
        let staged_path = extracted.get(&requested_idx);
        let Some(path) = staged_path else {
            if center_indices.contains(&requested_idx) {
                return Err(VideoImportError::EvidenceMissing);
            }
            continue;
        };

        let img = image::open(path)
            .map_err(|_| VideoImportError::EvidenceMissing)?
            .to_rgba8();
        let (w, h) = img.dimensions();

        if w > EVIDENCE_MAX_LONG_EDGE || h > EVIDENCE_MAX_LONG_EDGE {
            return Err(VideoImportError::ResourceLimit);
        }

        if evidence_w == 0 {
            evidence_w = w;
            evidence_h = h;
        }

        let encoded =
            crate::project::encode_png_asset(&img).map_err(|_| VideoImportError::ScratchIo)?;
        let dest = assets_dir.join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).map_err(|_| VideoImportError::ScratchIo)?;

        let at_ms = analysis_at_ms
            .get(requested_idx)
            .copied()
            .unwrap_or(requested_idx as u64 * 1000 / ANALYSIS_FPS);
        frames.push(ProjectFrame {
            id: i as FrameId,
            at_ms,
            sha256: encoded.sha256,
            width: w,
            height: h,
        });

        scratch.add_bytes(encoded.bytes.len() as u64);
        if scratch.bytes_used() > MAX_SCRATCH_BYTES {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(VideoImportError::ResourceLimit);
        }
    }

    let _ = std::fs::remove_dir_all(&staging);

    let id_to_frame_idx: std::collections::HashMap<usize, usize> = frames
        .iter()
        .enumerate()
        .map(|(fi, f)| {
            let sample_idx = (f.at_ms * ANALYSIS_FPS / 1000) as usize;
            (sample_idx, fi)
        })
        .collect();

    let mut candidate_steps = Vec::new();
    for (step_idx, marker) in selection.candidates.iter().enumerate() {
        let center_sample = marker.center_id as usize;
        let Some(&frame_idx) = id_to_frame_idx.get(&center_sample) else {
            continue;
        };

        let mut nearby = Vec::new();
        for offset in [-1i32, 0, 1] {
            let neighbor = center_sample as i32 + offset;
            if neighbor < 0 {
                continue;
            }
            if let Some(&ni) = id_to_frame_idx.get(&(neighbor as usize)) {
                nearby.push(frames[ni].id);
            }
        }
        nearby.sort();
        nearby.dedup();

        candidate_steps.push(crate::models::CandidateStep {
            id: step_idx as crate::models::CandidateId,
            kind: marker.kind,
            reason: marker.reason,
            at_ms: marker.at_ms,
            keyframe: frames[frame_idx].id,
            nearby,
        });
    }

    let mut import_warnings = Vec::new();

    if candidate_steps.is_empty() {
        let final_sample = last_sample_index as usize;
        let final_frame_idx = id_to_frame_idx
            .get(&final_sample)
            .copied()
            .or_else(|| frames.last().map(|_| frames.len() - 1));

        if let Some(fi) = final_frame_idx {
            candidate_steps.push(crate::models::CandidateStep {
                id: 0,
                kind: CandidateKind::UiChanged,
                reason: DetectReason::VisualChange,
                at_ms: frames[fi].at_ms,
                keyframe: frames[fi].id,
                nearby: vec![frames[fi].id],
            });
        }
        import_warnings.push(ImportWarning::NoVisualChangesDetected);
    }

    if selection.reduced {
        import_warnings.push(ImportWarning::IntermediateChangesReduced);
    }

    let mut guide = Guide::from_candidates(candidate_steps);

    // For zero candidates (fallback), override the title to "Imported recording".
    if selection.candidates.is_empty() && !guide.is_empty() {
        guide.rename(1, "Imported recording".to_string());
    }

    let capture_region = CaptureRegion {
        x: 0,
        y: 0,
        width: evidence_w,
        height: evidence_h,
    };

    progress(VideoImportPass::Extract.progress(meta.duration_ms, meta.duration_ms, frames.len()));

    Ok(ImportedWorkspaceSeed {
        guide,
        capture_region,
        input_source: InputSourceKind::ImportedVideo,
        input_capability: InputCapability::VisualOnly {
            reason: DegradedReason::ImportedRecording,
        },
        frames,
        import_warnings,
        scratch,
    })
}

fn checked_frame_size(width: u32, meta: ProbeMetadata) -> Result<usize, VideoImportError> {
    let height = (width as u64) * (meta.display_height as u64) / (meta.display_width as u64);
    // Round up to even (required for yuv420p) using integer arithmetic.
    let height = (height.div_ceil(2) * 2) as u32;
    let frame_size = (width as usize)
        .checked_mul(height as usize)
        .ok_or(VideoImportError::ResourceLimit)?;

    if frame_size > MAX_ANALYSIS_FRAME_BYTES {
        return Err(VideoImportError::ResourceLimit);
    }
    Ok(frame_size)
}

impl VideoImportPass {
    fn progress(self, processed_ms: u64, total_ms: u64, retained: usize) -> VideoImportProgress {
        VideoImportProgress {
            pass: self,
            processed_ms,
            total_ms,
            retained_candidates: retained,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn ffmpeg_path() -> String {
        std::env::var("ROLLSHOT_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string())
    }

    fn ffprobe_path() -> String {
        std::env::var("ROLLSHOT_FFPROBE").unwrap_or_else(|_| "ffprobe".to_string())
    }

    fn toolchain() -> VideoToolchain {
        VideoToolchain {
            ffmpeg: PathBuf::from(ffmpeg_path()),
            ffprobe: PathBuf::from(ffprobe_path()),
        }
    }

    fn ffmpeg_available() -> bool {
        Command::new(ffmpeg_path())
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn fixtures_enabled() -> bool {
        std::env::var("ROLLSHOT_TEST_FFMPEG").ok().as_deref() == Some("1") && ffmpeg_available()
    }

    fn fixture_video(frame_colors: &[u8], with_audio: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("test.mp4");

        let mut cmd = Command::new(ffmpeg_path());
        cmd.args([
            "-y", "-nostdin", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", "320x240", "-r", "2",
            "-i", "pipe:0",
        ]);

        if with_audio {
            cmd.args(["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono", "-shortest"]);
        }

        cmd.args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            output.to_str().unwrap(),
        ]);

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            for &color in frame_colors {
                stdin.write_all(&vec![color; 320 * 240 * 3]).unwrap();
            }
        }
        child.wait().unwrap();

        dir
    }

    fn settle_sequence_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("settle.mp4");

        let mut child = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                "320x240",
                "-r",
                "2",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                output.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            // Phase 1: baseline (4 frames = 2s)
            for _ in 0..4 {
                stdin.write_all(&vec![100u8; 320 * 240 * 3]).unwrap();
            }
            // Phase 2: change (2 frames = 1s)
            for _ in 0..2 {
                stdin.write_all(&vec![200u8; 320 * 240 * 3]).unwrap();
            }
            // Phase 3: settle (4 frames = 2s)
            for _ in 0..4 {
                stdin.write_all(&vec![200u8; 320 * 240 * 3]).unwrap();
            }
            // Phase 4: another change (2 frames)
            for _ in 0..2 {
                stdin.write_all(&vec![50u8; 320 * 240 * 3]).unwrap();
            }
            // Phase 5: settle (4 frames)
            for _ in 0..4 {
                stdin.write_all(&vec![50u8; 320 * 240 * 3]).unwrap();
            }
        }
        child.wait().unwrap();
        dir
    }

    fn audio_bearing_4k_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("4k.mp4");

        let mut child = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                "320x240",
                "-r",
                "2",
                "-i",
                "pipe:0",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-shortest",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                output.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            for i in 0..10u8 {
                stdin.write_all(&vec![i * 25; 320 * 240 * 3]).unwrap();
            }
        }
        child.wait().unwrap();
        dir
    }

    fn alternating_fixture(num_frames: u32, with_audio: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("test.mp4");

        let mut cmd = Command::new(ffmpeg_path());
        cmd.args([
            "-y", "-nostdin", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", "320x240", "-r", "2",
            "-i", "pipe:0",
        ]);

        if with_audio {
            cmd.args(["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono", "-shortest"]);
        }

        cmd.args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            output.to_str().unwrap(),
        ]);

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            for i in 0..num_frames {
                let color = if i % 2 == 0 { 10u8 } else { 240u8 };
                stdin.write_all(&vec![color; 320 * 240 * 3]).unwrap();
            }
        }
        child.wait().unwrap();
        dir
    }

    fn run_import(
        video_path: &Path,
    ) -> Result<(ImportedWorkspaceSeed, tempfile::TempDir), VideoImportError> {
        let scratch_parent = tempfile::tempdir().unwrap();
        let request = VideoImportRequest {
            input: video_path.to_path_buf(),
            toolchain: toolchain(),
            scratch_parent: scratch_parent.path().to_path_buf(),
        };
        let seed = import_video(request, VideoImportCancellation::default(), |_p| {})?;
        Ok((seed, scratch_parent))
    }

    fn scratch_files(seed: &ImportedWorkspaceSeed) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(seed.scratch.root().join("assets/frames"))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
            .collect();
        files.sort();
        files
    }

    fn png_asset_files(seed: &ImportedWorkspaceSeed) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = seed
            .frames
            .iter()
            .map(|f| {
                seed.scratch
                    .root()
                    .join("assets/frames")
                    .join(format!("{}.png", f.sha256))
            })
            .collect();
        files.sort();
        files.dedup();
        files
    }

    #[test]
    fn static_video_returns_final_frame_fallback() {
        if !fixtures_enabled() {
            return;
        }
        let fixture = fixture_video(&[20u8; 8], true);
        let output = fixture.path().join("test.mp4");
        let (seed, _parent) = run_import(&output).unwrap();
        assert_eq!(seed.guide.steps().len(), 1);
        assert_eq!(seed.guide.steps()[0].title, "Imported recording");
        assert_eq!(
            seed.import_warnings,
            vec![ImportWarning::NoVisualChangesDetected]
        );
        assert_eq!(seed.guide.steps()[0].kind, CandidateKind::UiChanged);
    }

    #[test]
    fn visual_settles_produce_only_ui_changed_steps() {
        if !fixtures_enabled() {
            return;
        }
        let fixture = settle_sequence_fixture();
        let output = fixture.path().join("settle.mp4");
        if !output.exists() {
            return;
        }
        let (seed, _parent) = run_import(&output).unwrap();
        assert!(seed
            .guide
            .steps()
            .iter()
            .all(|step| step.title == "UI changed"));
        assert!(seed
            .guide
            .steps()
            .iter()
            .all(|step| step.kind == CandidateKind::UiChanged
                && step.reason == DetectReason::VisualChange));
    }

    #[test]
    fn evidence_is_scaled_bounded_and_audio_is_ignored() {
        if !fixtures_enabled() {
            return;
        }
        let fixture = audio_bearing_4k_fixture();
        let output = fixture.path().join("4k.mp4");
        if !output.exists() {
            return;
        }
        let (seed, _parent) = run_import(&output).unwrap();
        assert!(seed.frames.len() <= MAX_EVIDENCE_FRAMES);
        assert!(seed
            .frames
            .iter()
            .all(|frame| frame.width.max(frame.height) <= EVIDENCE_MAX_LONG_EDGE));
        assert_eq!(scratch_files(&seed), png_asset_files(&seed));
    }

    #[test]
    fn cancelled_import_returns_cancelled_error() {
        if !fixtures_enabled() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("cancel.mp4");

        let mut child = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                "320x240",
                "-r",
                "2",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                output.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            for _ in 0..20 {
                stdin.write_all(&vec![100u8; 320 * 240 * 3]).unwrap();
            }
        }
        child.wait().unwrap();

        let cancel = VideoImportCancellation::default();
        cancel.cancel();

        let scratch_parent = tempfile::tempdir().unwrap();
        let request = VideoImportRequest {
            input: output,
            toolchain: toolchain(),
            scratch_parent: scratch_parent.path().to_path_buf(),
        };
        let err = import_video(request, cancel, |_p| {}).unwrap_err();
        assert_eq!(err.category(), "cancelled");
    }

    #[test]
    fn cancelled_during_extraction_removes_scratch() {
        if !fixtures_enabled() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("cancel_extract.mp4");

        let mut child = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                "320x240",
                "-r",
                "2",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                output.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            // Enough frames to have some candidates
            for i in 0..40u8 {
                let color = if i < 10 { 50 } else { 200 };
                stdin.write_all(&vec![color; 320 * 240 * 3]).unwrap();
            }
        }
        child.wait().unwrap();

        let scratch_parent = tempfile::tempdir().unwrap();
        let cancel = VideoImportCancellation::default();
        cancel.cancel();

        let request = VideoImportRequest {
            input: output,
            toolchain: toolchain(),
            scratch_parent: scratch_parent.path().to_path_buf(),
        };
        let _ = import_video(request, cancel, |_p| {});
        // No scratch directory should remain
        let remaining: Vec<_> = std::fs::read_dir(scratch_parent.path())
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("import-"))
            .collect();
        assert!(
            remaining.is_empty(),
            "scratch directories should be cleaned up on cancellation"
        );
    }

    #[test]
    fn probe_failure_returns_probe_error() {
        let scratch_parent = tempfile::tempdir().unwrap();
        let request = VideoImportRequest {
            input: PathBuf::from("/nonexistent/video.mp4"),
            toolchain: toolchain(),
            scratch_parent: scratch_parent.path().to_path_buf(),
        };
        let err = import_video(request, VideoImportCancellation::default(), |_p| {}).unwrap_err();
        assert_eq!(err.category(), "probe_failed");
    }

    #[test]
    fn long_synthetic_video_catalog_is_bounded() {
        if !fixtures_enabled() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("long.mp4");

        // 601 different frames at 2fps = ~300.5s — more than MAX_EVIDENCE_FRAMES
        let mut child = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                "320x240",
                "-r",
                "2",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                output.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            for i in 0..601u16 {
                stdin
                    .write_all(&vec![(i % 256) as u8; 320 * 240 * 3])
                    .unwrap();
            }
        }
        child.wait().unwrap();

        let (seed, _parent) = run_import(&output).unwrap();
        assert!(
            seed.frames.len() <= MAX_EVIDENCE_FRAMES,
            "catalog must be bounded: got {}",
            seed.frames.len()
        );
    }

    #[test]
    fn rotation_metadata_is_respected() {
        if !fixtures_enabled() {
            return;
        }

        let meta = parse_probe_json(
            br#"{"streams":[{"width":1920,"height":1080,"duration":"1.0","side_data_list":[{"rotation":90}]}],"format":{"duration":"1.0"}}"#,
        )
        .unwrap();
        let frame_size = checked_frame_size(ANALYSIS_WIDTH, meta);
        assert!(frame_size.is_ok());
    }

    #[test]
    fn more_than_200_candidates_produces_reduction_warning() {
        if !fixtures_enabled() {
            return;
        }
        let fixture = alternating_fixture(404, true);
        let output = fixture.path().join("test.mp4");
        let (seed, _parent) = run_import(&output).unwrap();
        assert!(
            seed.import_warnings
                .contains(&ImportWarning::IntermediateChangesReduced),
            "expected IntermediateChangesReduced warning when >200 candidates detected"
        );
    }

    #[test]
    fn missing_center_evidence_returns_evidence_missing() {
        if !fixtures_enabled() {
            return;
        }
        let fixture = fixture_video(&[100u8, 200u8], false);
        let output = fixture.path().join("test.mp4");

        let center_indices = vec![1usize];
        let evidence_indices = evidence_sample_indices(&center_indices, 2);
        assert_eq!(evidence_indices, vec![0, 1]);

        let scratch_parent = tempfile::tempdir().unwrap();
        let staging = scratch_parent.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();

        let meta = process::probe_video(&output, &toolchain(), &VideoImportCancellation::default())
            .unwrap();

        let empty: &[usize] = &[];
        let extracted = process::run_evidence_pass(
            &output,
            &toolchain(),
            meta,
            empty,
            &staging,
            &VideoImportCancellation::default(),
            &|_| {},
            meta.duration_ms,
        )
        .unwrap();

        let mut frames = Vec::new();
        for (i, &requested_idx) in evidence_indices.iter().enumerate() {
            let staged_path = extracted.get(&requested_idx);
            let Some(_path) = staged_path else {
                if center_indices.contains(&requested_idx) {
                    panic!(
                        "center index {} missing from extraction should cause EvidenceMissing",
                        requested_idx
                    );
                }
                continue;
            };
            frames.push((i, requested_idx));
        }
        assert!(
            frames.is_empty(),
            "empty extraction set should leave no frames for center={:?}",
            center_indices
        );
    }

    #[test]
    fn edge_candidate_neighbors_are_gracefully_absent() {
        let indices = evidence_sample_indices(&[0], 10);
        assert_eq!(indices, vec![0, 1]);

        let indices = evidence_sample_indices(&[9], 10);
        assert_eq!(indices, vec![8, 9]);

        let indices = evidence_sample_indices(&[4], 10);
        assert_eq!(indices, vec![3, 4, 5]);
    }

    fn import_save_and_export_fixture(
        sentinel: &str,
    ) -> (ImportedWorkspaceSeed, tempfile::TempDir) {
        let fixture = fixture_video(&[100u8, 200u8], true);
        let input = fixture.path().join(sentinel);
        std::fs::copy(fixture.path().join("test.mp4"), &input).unwrap();
        let scratch_parent = tempfile::tempdir().unwrap();
        let request = VideoImportRequest {
            input,
            toolchain: toolchain(),
            scratch_parent: scratch_parent.path().to_path_buf(),
        };
        let seed = import_video(request, VideoImportCancellation::default(), |_p| {}).unwrap();
        (seed, scratch_parent)
    }

    #[test]
    fn persisted_and_exported_artifacts_never_contain_source_identity() {
        if !fixtures_enabled() {
            return;
        }
        let sentinel = "SECRET-customer-recording-8f7d.mp4";
        let (seed, _parent) = import_save_and_export_fixture(sentinel);

        let asset_dir = seed.scratch.root().join("assets/frames");
        let artifacts: Vec<Vec<u8>> = std::fs::read_dir(&asset_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "png"))
            .filter_map(|e| std::fs::read(e.path()).ok())
            .collect();

        assert!(
            !artifacts.is_empty(),
            "expected at least one persisted artifact"
        );
        for bytes in &artifacts {
            assert!(
                !String::from_utf8_lossy(bytes).contains(sentinel),
                "artifact must not contain source filename"
            );
        }
    }

    #[test]
    fn scratch_during_processing_contains_only_expected_assets() {
        if !fixtures_enabled() {
            return;
        }
        let (seed, _parent) = import_save_and_export_fixture("check-scratch.mp4");

        let root = seed.scratch.root();
        let assets = root.join("assets/frames");
        assert!(assets.exists(), "assets/frames must exist");

        let pngs: Vec<_> = std::fs::read_dir(&assets)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "png"))
            .collect();
        assert!(!pngs.is_empty(), "must have at least one PNG asset");
        for png in &pngs {
            let name = png.file_name();
            let name_str = name.to_string_lossy();
            assert!(
                name_str.ends_with(".png"),
                "asset filename should be sha256.png: {name_str}"
            );
            assert!(
                !name_str.contains("import-"),
                "asset filename must not contain import- prefix: {name_str}"
            );
        }

        let staging = root.join("staging");
        assert!(
            !staging.exists(),
            "staging should be removed after successful import"
        );
    }

    enum FaultOutcome {
        ProbeFailure,
        Pass1Failure,
        Pass2Failure,
        Cancelled,
    }

    struct FaultInjectionResult {
        scratch_paths: Vec<PathBuf>,
        live_child_count: usize,
    }

    impl FaultInjectionResult {
        fn scratch_paths(&self) -> &[PathBuf] {
            &self.scratch_paths
        }

        fn live_child_count(&self) -> usize {
            self.live_child_count
        }
    }

    fn run_fault_injected_import(outcome: FaultOutcome) -> FaultInjectionResult {
        let scratch_parent = tempfile::tempdir().unwrap();
        let scratch_parent_path = scratch_parent.path().to_path_buf();

        match outcome {
            FaultOutcome::ProbeFailure => {
                let request = VideoImportRequest {
                    input: PathBuf::from("/nonexistent/sentinel-probe-fail.mp4"),
                    toolchain: toolchain(),
                    scratch_parent: scratch_parent_path.clone(),
                };
                let err =
                    import_video(request, VideoImportCancellation::default(), |_p| {}).unwrap_err();
                assert_eq!(err.category(), "probe_failed");
            }
            FaultOutcome::Pass1Failure => {
                let dir = tempfile::tempdir().unwrap();
                let truncated = dir.path().join("truncated.mp4");
                create_truncated_mp4(&truncated);
                let request = VideoImportRequest {
                    input: truncated,
                    toolchain: toolchain(),
                    scratch_parent: scratch_parent_path.clone(),
                };
                let err =
                    import_video(request, VideoImportCancellation::default(), |_p| {}).unwrap_err();
                assert!(
                    matches!(
                        err,
                        VideoImportError::DecodeFailed
                            | VideoImportError::ProbeFailed
                            | VideoImportError::InvalidVideoMetadata
                    ),
                    "expected analysis-phase error, got: {err:?}"
                );
            }
            FaultOutcome::Pass2Failure => {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("corrupt-frames.mp4");
                create_corrupt_frame_mp4(&path);
                let request = VideoImportRequest {
                    input: path,
                    toolchain: toolchain(),
                    scratch_parent: scratch_parent_path.clone(),
                };
                let result = import_video(request, VideoImportCancellation::default(), |_p| {});
                match result {
                    Ok(_) => {
                        // If extraction succeeded, the test still verifies cleanup below.
                    }
                    Err(err) => {
                        assert!(
                            matches!(
                                err,
                                VideoImportError::EvidenceMissing
                                    | VideoImportError::DecodeFailed
                                    | VideoImportError::ScratchIo
                                    | VideoImportError::Cancelled
                            ),
                            "expected extraction-phase error, got: {err:?}"
                        );
                    }
                }
            }
            FaultOutcome::Cancelled => {
                let fixture = settle_sequence_fixture();
                let output = fixture.path().join("settle.mp4");
                let cancel = VideoImportCancellation::default();
                cancel.cancel();
                let request = VideoImportRequest {
                    input: output,
                    toolchain: toolchain(),
                    scratch_parent: scratch_parent_path.clone(),
                };
                let err = import_video(request, cancel, |_p| {}).unwrap_err();
                assert_eq!(err.category(), "cancelled");
            }
        }

        let remaining: Vec<PathBuf> = std::fs::read_dir(&scratch_parent_path)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("import-"))
            .map(|e| e.path())
            .collect();

        let children = count_children();

        FaultInjectionResult {
            scratch_paths: remaining,
            live_child_count: children,
        }
    }

    fn count_children() -> usize {
        let our_pid = std::process::id();
        let Ok(output) = Command::new("pgrep")
            .args(["-P", &our_pid.to_string()])
            .output()
        else {
            return 0;
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
    }

    fn create_truncated_mp4(path: &Path) {
        let mut child = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                "320x240",
                "-r",
                "2",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                path.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(&vec![100u8; 320 * 240 * 3]).unwrap();
        }
        child.stdin.take();
        child.wait().unwrap();

        let meta = std::fs::metadata(path).unwrap();
        if meta.len() > 512 {
            let content = std::fs::read(path).unwrap();
            let truncate_at = content.len() / 3;
            std::fs::write(path, &content[..truncate_at]).unwrap();
        }
    }

    fn create_corrupt_frame_mp4(path: &Path) {
        let mut child = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                "320x240",
                "-r",
                "2",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                path.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            for i in 0..20u8 {
                stdin.write_all(&vec![i * 12; 320 * 240 * 3]).unwrap();
            }
        }
        child.stdin.take();
        child.wait().unwrap();

        let content = std::fs::read(path).unwrap();
        if content.len() > 4096 {
            let corrupt_start = content.len() / 2;
            let mut corrupted = content;
            for b in &mut corrupted[corrupt_start..corrupt_start + 1024] {
                *b = 0xFF;
            }
            std::fs::write(path, &corrupted).unwrap();
        }
    }

    #[test]
    fn every_terminal_outcome_reaps_children_and_removes_scratch() {
        if !fixtures_enabled() {
            return;
        }
        for outcome in [
            FaultOutcome::ProbeFailure,
            FaultOutcome::Pass1Failure,
            FaultOutcome::Pass2Failure,
            FaultOutcome::Cancelled,
        ] {
            let result = run_fault_injected_import(outcome);
            assert!(
                result.scratch_paths().iter().all(|path| !path.exists()),
                "scratch directories should be cleaned up"
            );
            assert_eq!(
                result.live_child_count(),
                0,
                "no child processes should remain"
            );
        }
    }

    #[test]
    fn tracing_output_never_leaks_source_identity() {
        if !fixtures_enabled() {
            return;
        }
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        let sentinel = "SECRET-trace-leak-test-9c2a.mp4";
        let log_buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let log_buffer_check = log_buffer.clone();

        struct WriteAdaptor {
            buf: Arc<Mutex<Vec<u8>>>,
        }
        impl Write for WriteAdaptor {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                self.buf.lock().unwrap().extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || WriteAdaptor {
                buf: log_buffer.clone(),
            })
            .with_ansi(false)
            .with_target(true)
            .with_max_level(tracing::Level::TRACE)
            .finish();

        let _guard = tracing::subscriber::set_default(subscriber);

        let fixture = fixture_video(&[50u8, 150u8], true);
        let input = fixture.path().join(sentinel);
        std::fs::copy(fixture.path().join("test.mp4"), &input).unwrap();

        let scratch_parent = tempfile::tempdir().unwrap();
        let request = VideoImportRequest {
            input: input.clone(),
            toolchain: toolchain(),
            scratch_parent: scratch_parent.path().to_path_buf(),
        };
        let _seed = import_video(request, VideoImportCancellation::default(), |_p| {}).unwrap();

        let logs = String::from_utf8(log_buffer_check.lock().unwrap().clone()).unwrap();

        assert!(
            !logs.contains(sentinel),
            "tracing must not contain sentinel filename"
        );
        assert!(
            !logs.contains(input.to_string_lossy().as_ref()),
            "tracing must not contain full input path"
        );
        assert!(
            !logs.contains("Input #0"),
            "tracing must not contain FFmpeg input header (echoes file path)"
        );
        assert!(
            !logs.contains("libx264"),
            "tracing must not contain FFmpeg codec output"
        );
        assert!(
            !logs.contains("frame= "),
            "tracing must not contain FFmpeg progress output (may echo input path)"
        );
        assert!(
            !logs.contains("decoded pixels"),
            "tracing must not mention decoded pixel data"
        );
    }
}
