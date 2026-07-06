# Action Guide MP4 Summary Export Design

## Summary

Rollshot will add **Export MP4 Summary** for Action Guide sessions. The export is a reviewed workflow summary video built from the guide's current keyframes, not a raw screen recording. Each reviewed step keyframe is repeated for a fixed dwell, encoded to an H.264 MP4, and saved without closing the timeline workspace.

This extends the existing Action Guide export ladder:

- `steps.md` plus `keyframes/*.png` remains the formal artifact.
- `summary.gif` remains the quick looping visual preview.
- `summary.mp4` becomes the smaller, higher-quality, broadly shareable summary video.

## Goals

- Export an MP4 slideshow from reviewed Action Guide keyframes.
- Keep MP4 export behavior aligned with current GIF export: no app exit, inline success/failure message, no mutation of the editable guide.
- Use FFmpeg as an external process through `ffmpeg-sidecar` command/download helpers.
- Prefer an existing system FFmpeg, and offer a managed FFmpeg download only when needed and explicitly requested by the user.
- Make managed FFmpeg acquisition auditable: source, version, license, size, sha256, install path, and manifest.
- Keep CI stable when FFmpeg is not installed.

## Non-Goals

- Full-fps screen recording.
- Audio capture.
- Pause/resume recording.
- Hardware encoder selection.
- Burned-in captions, step numbers, cursor animation, click rings, zoom/pan, or text rendering.
- Issue Pack `summary.mp4` attachment support in this first slice.
- A general plugin manager or background FFmpeg auto-updater.
- Silent network access or startup-time FFmpeg download.

## Existing Context

`rollshot-action` already owns platform-neutral guide data, retained keyframes, Markdown/keyframe export, and GIF summary export. `gif.rs` validates all retained keyframes before writing, downscales frames, encodes one frame per step, and writes atomically.

`rollshot-app` owns iced timeline UI, file pickers, and user-visible export state. GIF export is triggered from the timeline header and keeps the workspace open. Guide export exits after writing the guide folder. MP4 summary should follow GIF behavior, not guide export behavior.

Snow Shot is a useful reference for FFmpeg process handling and sidecar-style binary management. Reusable lessons are: explicit FFmpeg path initialization, macOS executable permission repair, even dimensions for H.264 compatibility, compatible pixel formats, `-crf 23`, encoder preset, `+faststart`, and visible download/install state. Its full recording service, audio, pause/resume, capture-device, and segment merge behavior are intentionally out of scope.

## Architecture

### `rollshot-action`

Add `video.rs` with a platform-neutral export API:

```rust
pub struct VideoOptions {
    pub frame_dwell_ms: u32,
    pub fps: u32,
    pub max_width: u32,
}

pub fn export_video(
    guide: &Guide,
    store: &FrameStore,
    opts: VideoOptions,
    ffmpeg_path: &Path,
    out_path: &Path,
) -> Result<(), VideoError>
```

`rollshot-action` does not locate, download, or manage FFmpeg. It receives an explicit binary path and only handles guide/keyframe validation, frame preparation, FFmpeg invocation, and atomic output.

Processing flow:

1. Reject empty guide with `VideoError::Empty`.
2. Resolve each step's retained keyframe before writing anything.
3. Downscale frames to `max_width`, preserving aspect ratio and never upscaling.
4. Normalize all frames to a single output size.
5. Force even output dimensions for H.264 / `yuv420p` compatibility.
6. Repeat each keyframe for `ceil(frame_dwell_ms * fps / 1000)` frames.
7. Pipe raw RGBA frames to FFmpeg stdin.
8. Encode to a sibling temp MP4, then rename to `out_path` on success.
9. Remove temp output on failure.

FFmpeg command shape:

```text
ffmpeg
  -y
  -f rawvideo
  -pixel_format rgba
  -video_size WIDTHxHEIGHT
  -framerate FPS
  -i pipe:0
  -vf format=yuv420p
  -c:v libx264
  -preset veryfast
  -crf 23
  -movflags +faststart
  -f mp4
  output.tmp.mp4
```

The explicit `-f mp4` avoids relying on a temp extension for format detection.

### `rollshot-app`

Add timeline UI support beside current GIF export:

- Header button: `Export MP4`.
- Save picker default file name: `summary.mp4`.
- Success message: `MP4 saved to ...`.
- Failure message: `MP4 export failed: ...`.
- Cancelled picker is a no-op.
- Successful MP4 export keeps the timeline workspace open.

The app layer owns FFmpeg availability and managed download state because it owns user interaction, data directories, network consent, and UI messaging.

## FFmpeg Availability

Resolution order:

1. `ROLLSHOT_FFMPEG` environment variable, for development and explicit user override.
2. System FFmpeg on `PATH`.
3. Rollshot managed FFmpeg recorded in the managed FFmpeg manifest.
4. If none are valid, show the FFmpeg-required dialog.

Validation means the path exists, is executable where relevant, and `ffmpeg -version` succeeds.

## Managed FFmpeg Download

When FFmpeg is unavailable and the user clicks `Export MP4`, show a dialog:

```text
FFmpeg is required to export MP4

[Use system FFmpeg / install manually]
[Download managed FFmpeg]
```

`Download managed FFmpeg` opens a confirmation view before network access. The view must show:

- source/provider URL
- FFmpeg version
- license label and license/notice URL
- archive size
- expected sha256
- install location

The user must explicitly confirm before download starts.

Implementation should use `ffmpeg-sidecar` lower-level helpers, not `auto_download`, so Rollshot controls destination and verification:

- `ffmpeg_sidecar::download::download_ffmpeg_package_with_progress`
- `ffmpeg_sidecar::download::unpack_ffmpeg_without_extras`
- `ffmpeg_sidecar::command::FfmpegCommand` or equivalent command builder for encoding

Rollshot owns pinned per-platform metadata. Prod must not trust an unpinned "latest" download for integrity. A metadata entry contains:

- platform triple
- version
- source URL
- license label
- license/notice URL
- archive size
- archive sha256

Download flow:

1. Download to a managed temp/download directory.
2. Verify archive sha256 before unpacking.
3. Unpack only `ffmpeg`.
4. Ensure executable permissions on Unix/macOS.
5. Run `ffmpeg -version`.
6. Write `managed-ffmpeg.json` only after validation succeeds.
7. Delete incomplete temp files on failure.

Manifest fields:

```json
{
  "schema_version": 1,
  "platform": "linux-x86_64",
  "version": "...",
  "source_url": "...",
  "license": "...",
  "license_url": "...",
  "archive_sha256": "...",
  "archive_size": 0,
  "binary_path": "...",
  "ffmpeg_version_line": "...",
  "installed_at": "..."
}
```

Development can be looser: `ROLLSHOT_FFMPEG` and system FFmpeg are enough for day-to-day work. Managed-download tests can use fixtures or a local test archive rather than real internet access.

## Error Handling

`VideoError` should distinguish:

- empty guide
- missing retained keyframe
- invalid FFmpeg path
- FFmpeg spawn failure
- FFmpeg stdin write failure
- FFmpeg non-zero exit
- temp/output I/O failure

All runtime diagnostics in product paths use `tracing` with stable `rollshot::*` targets and structured fields.

User-facing app messages stay short and actionable. Detailed FFmpeg stderr is logged, not dumped into the main UI.

## Testing

Unit tests in `rollshot-action`:

- empty guide returns `VideoError::Empty`
- missing keyframe returns `VideoError::KeyframeMissing`
- frame repeat count is deterministic
- resize preserves aspect ratio and never upscales
- even dimensions are enforced
- command args include raw RGBA input, `yuv420p`, `libx264`, `crf 23`, `+faststart`, and explicit MP4 output
- failures leave no target output

App tests:

- `ExportMp4PathChosen(None)` is a no-op
- successful export sets a success message and keeps the timeline open
- export failure sets an inline failure message
- missing FFmpeg opens availability/download UI instead of trying export
- managed FFmpeg manifest validation rejects mismatched platform or missing binary

Download tests:

- metadata rendering shows source, version, license, size, sha256, and install location
- sha256 mismatch fails and does not write manifest
- successful validation writes manifest

Integration tests requiring real FFmpeg are opt-in:

```text
ROLLSHOT_TEST_FFMPEG=1
```

Those tests export a one- or two-step MP4 and check that the file exists and has non-zero length. If available, `ffprobe` may be used to check duration, but CI must not require it by default.

## Rollout

P0.6 ships standalone timeline `Export MP4`.

Later work can add:

1. Issue Pack `summary.mp4` attachment.
2. Optional burned-in step number/title.
3. Full recording evidence video, only if users need raw recordings.

## Initial Platform Scope

The first implementation supports MP4 export with `ROLLSHOT_FFMPEG` or system FFmpeg on every platform where Action Guide is enabled.

Managed FFmpeg download is initially enabled only for platforms with pinned metadata in the Rollshot source tree. The first pinned target is Linux x86_64. On platforms without pinned metadata, the FFmpeg-required dialog still appears, but `Download managed FFmpeg` is disabled with an explanation that managed FFmpeg is not yet available for that platform; users can still install system FFmpeg or set `ROLLSHOT_FFMPEG`.

macOS managed download should be added by extending the pinned metadata table and validation tests, not by changing the export pipeline.
