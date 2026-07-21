# Open Existing Image Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `rollshot-app open <IMAGE>` so one existing PNG or JPEG opens directly in the shared annotation and optional OCR Result Workspace without risking the source file.

**Architecture:** Add a focused app-level image loader that returns orientation-corrected RGBA pixels plus a durable imported-source identity. Represent capture and imported origins explicitly in `ResultDocument`, reuse the existing Workspace, and add thin Linux/macOS launch adapters that bypass capture, auto-save, and thumbnail presentation.

**Tech Stack:** Rust 2021, clap 4.6, image 0.25.10, iced 0.14, iced_test 0.14, rfd 0.15, tracing, tempfile.

## Global Constraints

- The public CLI is exactly `rollshot-app open <IMAGE>` and accepts one required local path.
- Supported input content is static PNG or JPEG detected from bytes, not filename extension.
- JPEG EXIF orientations 1 through 8 are applied once before constructing `ImageDocument`.
- Imported source contents are read-only and must never be overwritten or metadata-written by Rollshot.
- Save As defaults to `<source-stem>-annotated.png` in the source directory and always encodes PNG.
- A missing export extension becomes `.png`; an explicitly non-PNG extension is rejected.
- `open` is available without the `ocr` Cargo feature; only the OCR Text tool is absent in that build.
- The official product distribution that advertises this workflow is built with OCR enabled.
- Linux opens the standalone Result Workspace; macOS boots the existing product daemon directly in `Phase::Workspace`.
- Neither platform may enter capture, overlay, auto-save, post-capture presentation, or thumbnail code for `open`.
- Every runtime diagnostic uses a stable explicit `rollshot::*` target and structured fields; never log image pixels, OCR text, or annotation contents.
- Before the first user-visible iced edit, invoke the repo-local `iced-rs` and `testing-iced-ui` skills. Use testing-iced-ui auto mode and follow the independent golden-baseline review rules in `AGENTS.md`.
- Inspect both Linux and macOS product paths before changing shared Workspace behavior.
- Do not create a worktree; this branch is already `feat/open-image` and repo instructions forbid worktrees unless explicitly requested.
- Prefix every implementation shell command with `rtk`.

---

## File Structure

- Create `crates/rollshot-app/src/image_import.rs`: content-based PNG/JPEG loading, EXIF orientation, typed import failures, and imported-source filesystem identity.
- Create `crates/rollshot-app/tests/open_cli.rs`: process-level CLI failure and stderr contract tests.
- Modify `crates/rollshot-app/src/launch.rs`: `open` clap command and `LaunchMode::Open`.
- Modify `crates/rollshot-app/src/main.rs`: module registration, open-image dispatch, import diagnostics, and platform routing.
- Modify `crates/rollshot-app/src/result_workspace/document.rs`: explicit `DocumentOrigin`, imported constructor, source/status/default-export methods, and lifecycle tests.
- Modify `crates/rollshot-app/src/result_workspace/secure_sharing.rs`: imported-source overwrite validation while retaining saved-capture redaction protection.
- Modify `crates/rollshot-app/src/result_workspace/actions.rs`: PNG destination normalization before write.
- Modify `crates/rollshot-app/src/result_workspace/update.rs`: imported default directory, destination validation, normalized PNG path, and source-path method use.
- Modify `crates/rollshot-app/src/result_workspace/mod.rs`: imported initialization, persistent imported status, and iced behavior tests.
- Modify `crates/rollshot-app/src/result_workspace/view.rs`: render imported/dirty status in the existing status bar.
- Modify `crates/rollshot-app/src/macos_product.rs`: direct imported-document bootstrap and daemon runner reuse.
- Modify `crates/rollshot-app/src/diagnostics.rs`: import target/category classification and privacy tests.
- Modify `README.md`: document the CLI, formats, source protection, output naming, and OCR feature behavior.
- Modify `.github/workflows/internal-release.yml`: provision pinned OCR build inputs and enable OCR in the macOS artifact.
- Modify `packaging/arch/PKGBUILD`: provision pinned OCR build inputs and enable OCR in the Arch artifact.
- Modify `scripts/release/test_packaging_files.py`: lock the official OCR-enabled packaging contract.

---

### Task 1: Add the explicit CLI launch contract

**Files:**
- Modify: `crates/rollshot-app/src/launch.rs:1-210`
- Test: `crates/rollshot-app/src/launch.rs:214-end`

**Interfaces:**
- Consumes: existing `LaunchCli`, `LaunchCommand`, and `resolve_launch_mode`.
- Produces: `LaunchCommand::Open(OpenArgs)` and `LaunchMode::Open { path: PathBuf }`.

- [ ] **Step 1: Write failing parser and resolution tests**

Add these tests beside the existing launch tests:

```rust
#[test]
fn open_requires_exactly_one_image_path() {
    let mode = parse(&["rollshot-app", "open", "fixtures/sample.png"])
        .expect("open path parses");
    assert_eq!(
        mode,
        LaunchMode::Open {
            path: PathBuf::from("fixtures/sample.png"),
        }
    );

    assert!(LaunchCli::try_parse_from(["rollshot-app", "open"]).is_err());
    assert!(
        LaunchCli::try_parse_from(["rollshot-app", "open", "a.png", "b.png"]).is_err()
    );
}

#[test]
fn open_rejects_capture_only_flags() {
    assert!(
        LaunchCli::try_parse_from(["rollshot-app", "open", "a.png", "--backend", "auto"])
            .is_err()
    );
    assert!(
        LaunchCli::try_parse_from(["rollshot-app", "open", "a.png", "--show-cursor"])
            .is_err()
    );
}
```

Add `use std::path::PathBuf;` in the test module.

- [ ] **Step 2: Run the focused tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-app launch::tests::open_ -- --nocapture
```

Expected: compilation fails because `LaunchMode::Open` and the `open` subcommand do not exist.

- [ ] **Step 3: Add the minimal clap types and launch lowering**

Add the new mode, command, and args:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
    Open { path: PathBuf },
    Ocr {
        options: InteractiveLaunchOptions,
        graphical_feedback: bool,
    },
    Daemon,
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
    #[cfg(feature = "action-guide")]
    ActionGuide(ActionGuideLaunch),
}

#[derive(Debug, clap::Args)]
pub struct OpenArgs {
    /// Existing static PNG or JPEG to open in the Result Workspace.
    #[arg(value_name = "IMAGE")]
    pub path: PathBuf,
}
```

Add this `LaunchCommand` variant:

```rust
/// Open an existing PNG or JPEG for annotation and optional OCR.
Open(OpenArgs),
```

Add this `resolve_launch_mode` arm before the daemon arm:

```rust
Some(LaunchCommand::Open(args)) => Ok(LaunchMode::Open { path: args.path }),
```

- [ ] **Step 4: Run launch tests**

Run:

```bash
rtk cargo test -p rollshot-app launch::tests -- --nocapture
```

Expected: all launch tests pass, including no-subcommand capture defaults and new open parsing.

- [ ] **Step 5: Commit the launch contract**

```bash
rtk git add crates/rollshot-app/src/launch.rs
rtk git commit -m "feat(app): add open image CLI command"
```

---

### Task 2: Build the content-based image import boundary

**Files:**
- Create: `crates/rollshot-app/src/image_import.rs`
- Modify: `crates/rollshot-app/src/main.rs:1-38`
- Test: `crates/rollshot-app/src/image_import.rs`

**Interfaces:**
- Consumes: `image::ImageReader`, `image::ImageDecoder`, `image::DynamicImage`, and local filesystem metadata.
- Produces: `pub(crate) fn load(path: &Path) -> Result<ImportedImage, ImageImportError>`.
- Produces: `ImportedImage { pub pixels: RgbaImage, pub source: ImportedSource }`.
- Produces: `ImportedSource::display_path()`, `default_export_dir()`, and `destination_matches()`.

- [ ] **Step 1: Register the module and write failing format/source tests**

Register the module in `main.rs`:

```rust
mod image_import;
```

Create `image_import.rs` with a test module that first specifies content detection and source preservation:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    fn write_png(path: &Path) -> Vec<u8> {
        let image = RgbaImage::from_pixel(2, 3, Rgba([10, 20, 30, 255]));
        image.save_with_format(path, ImageFormat::Png).unwrap();
        std::fs::read(path).unwrap()
    }

    #[test]
    fn loads_png_from_content_and_preserves_source_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("misleading.bin");
        let before = write_png(&path);
        let before_metadata = std::fs::metadata(&path).unwrap();

        let imported = load(&path).unwrap();

        assert_eq!(imported.pixels.dimensions(), (2, 3));
        assert_eq!(imported.source.display_path(), path.as_path());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let after_metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(after_metadata.len(), before_metadata.len());
        assert_eq!(
            after_metadata.modified().unwrap(),
            before_metadata.modified().unwrap()
        );
        assert_eq!(
            after_metadata.permissions().readonly(),
            before_metadata.permissions().readonly()
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn rejects_unsupported_content_before_decode() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("animation.gif", b"GIF89a".as_slice()),
            ("image.webp", b"RIFF\x04\0\0\0WEBP".as_slice()),
            ("vector.svg", b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice()),
            ("document.pdf", b"%PDF-1.7".as_slice()),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            let error = load(&path).unwrap_err();
            assert_eq!(error.kind(), ImageImportErrorKind::UnsupportedFormat);
            assert!(error.to_string().contains(name));
        }
    }

    #[test]
    fn rejects_missing_directory_and_corrupt_inputs_by_category() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.png");
        assert_eq!(load(&missing).unwrap_err().kind(), ImageImportErrorKind::NotFound);
        assert_eq!(load(dir.path()).unwrap_err().kind(), ImageImportErrorKind::NotFile);

        let corrupt = dir.path().join("corrupt.png");
        std::fs::write(&corrupt, b"\x89PNG\r\n\x1a\ninvalid").unwrap();
        assert_eq!(load(&corrupt).unwrap_err().kind(), ImageImportErrorKind::Decode);
    }
}
```

- [ ] **Step 2: Add a failing EXIF orientation fixture test**

Add a local helper that inserts a valid big-endian EXIF orientation entry into an encoded JPEG:

```rust
fn jpeg_with_orientation(orientation: u8) -> Vec<u8> {
    let source = RgbaImage::from_fn(2, 1, |x, _| {
        if x == 0 {
            Rgba([240, 20, 20, 255])
        } else {
            Rgba([20, 20, 240, 255])
        }
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
        .encode_image(&source)
        .unwrap();

    let mut exif = b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01"
        .to_vec();
    exif.extend_from_slice(&[0, orientation, 0, 0]);
    exif.extend_from_slice(&[0, 0, 0, 0]);
    let segment_len = u16::try_from(exif.len() + 2).unwrap().to_be_bytes();

    let mut oriented = Vec::with_capacity(jpeg.len() + exif.len() + 4);
    oriented.extend_from_slice(&jpeg[..2]);
    oriented.extend_from_slice(&[0xff, 0xe1]);
    oriented.extend_from_slice(&segment_len);
    oriented.extend_from_slice(&exif);
    oriented.extend_from_slice(&jpeg[2..]);
    oriented
}

#[test]
fn applies_jpeg_exif_orientation_before_returning_rgba() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phone.jpg");
    std::fs::write(&path, jpeg_with_orientation(6)).unwrap();

    let imported = load(&path).unwrap();

    assert_eq!(imported.pixels.dimensions(), (1, 2));
}
```

Replace the single orientation assertion with an exact pixel comparison for every EXIF value. Decode the same JPEG bytes without applying metadata, apply the expected `image::metadata::Orientation` explicitly, and compare the complete RGBA buffer:

```rust
#[test]
fn applies_all_jpeg_exif_orientations_before_returning_rgba() {
    for orientation in 1..=8 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("orientation-{orientation}.jpg"));
        let encoded = jpeg_with_orientation(orientation);
        std::fs::write(&path, &encoded).unwrap();

        let mut expected = image::load_from_memory_with_format(&encoded, ImageFormat::Jpeg)
            .unwrap();
        expected.apply_orientation(
            image::metadata::Orientation::from_exif(orientation).unwrap(),
        );

        assert_eq!(load(&path).unwrap().pixels, expected.to_rgba8());
    }
}
```

Add an animated-PNG preflight test so static-only support is explicit:

```rust
#[test]
fn rejects_apng_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("animated.png");
    let base = dir.path().join("base.png");
    let png = write_png(&base);
    let ihdr_end = 8 + 12 + 13;
    let mut apng = png[..ihdr_end].to_vec();
    apng.extend_from_slice(&[0, 0, 0, 8]);
    apng.extend_from_slice(b"acTL");
    apng.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]);
    apng.extend_from_slice(&[0, 0, 0, 0]);
    apng.extend_from_slice(&png[ihdr_end..]);
    std::fs::write(&path, apng).unwrap();

    assert_eq!(
        load(&path).unwrap_err().kind(),
        ImageImportErrorKind::UnsupportedFormat
    );
}
```

- [ ] **Step 3: Run loader tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-app image_import::tests -- --nocapture
```

Expected: compilation fails because the loader types and function are not defined.

- [ ] **Step 4: Implement typed failures and imported-source identity**

Define the focused data types:

```rust
use image::{DynamicImage, ImageDecoder as _, ImageError, ImageFormat, ImageReader, RgbaImage};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, ErrorKind, Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageImportErrorKind {
    NotFound,
    NotFile,
    Read,
    UnsupportedFormat,
    ResourceLimit,
    Orientation,
    Decode,
}

#[derive(Debug)]
pub(crate) struct ImageImportError {
    kind: ImageImportErrorKind,
    path: PathBuf,
    detail: String,
}

impl ImageImportError {
    pub(crate) fn kind(&self) -> ImageImportErrorKind {
        self.kind
    }
}

impl fmt::Display for ImageImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not open image {}: {}",
            self.path.display(),
            self.detail
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedSource {
    display_path: PathBuf,
    canonical_path: PathBuf,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl ImportedSource {
    pub(crate) fn display_path(&self) -> &Path {
        &self.display_path
    }

    pub(crate) fn default_export_dir(&self) -> PathBuf {
        self.display_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }

    pub(crate) fn destination_matches(&self, destination: &Path) -> Result<bool, String> {
        match fs::metadata(destination) {
            Ok(metadata) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt as _;
                    return Ok(metadata.dev() == self.device && metadata.ino() == self.inode);
                }
                #[cfg(not(unix))]
                {
                    return fs::canonicalize(destination)
                        .map(|path| path == self.canonical_path)
                        .map_err(|error| format!("could not verify export destination: {error}"));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let parent = destination
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let name = destination
                    .file_name()
                    .ok_or_else(|| "export destination has no filename".to_string())?;
                let parent = fs::canonicalize(parent)
                    .map_err(|error| format!("could not verify export destination: {error}"))?;
                Ok(parent.join(name) == self.canonical_path)
            }
            Err(error) => Err(format!("could not verify export destination: {error}")),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ImportedImage {
    pub(crate) pixels: RgbaImage,
    pub(crate) source: ImportedSource,
}
```

- [ ] **Step 5: Implement content detection, decoder limits, and orientation**

Implement `load` using `ImageReader::new`, not `ImageReader::open`, so the extension never seeds the format:

```rust
pub(crate) fn load(path: &Path) -> Result<ImportedImage, ImageImportError> {
    let file = File::open(path).map_err(|error| ImageImportError {
        kind: open_error_kind(&error),
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let metadata = file.metadata().map_err(|error| ImageImportError {
        kind: ImageImportErrorKind::Read,
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(ImageImportError {
            kind: ImageImportErrorKind::NotFile,
            path: path.to_path_buf(),
            detail: "path is not a regular file".to_string(),
        });
    }

    let canonical_path = fs::canonicalize(path).map_err(|error| ImageImportError {
        kind: ImageImportErrorKind::Read,
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let source = ImportedSource {
        display_path: path.to_path_buf(),
        canonical_path,
        #[cfg(unix)]
        device: {
            use std::os::unix::fs::MetadataExt as _;
            metadata.dev()
        },
        #[cfg(unix)]
        inode: {
            use std::os::unix::fs::MetadataExt as _;
            metadata.ino()
        },
    };

    let reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| ImageImportError {
            kind: ImageImportErrorKind::Read,
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    let format = reader.format().ok_or_else(|| ImageImportError {
        kind: ImageImportErrorKind::UnsupportedFormat,
        path: path.to_path_buf(),
        detail: "supported formats are static PNG and JPEG".to_string(),
    })?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(ImageImportError {
            kind: ImageImportErrorKind::UnsupportedFormat,
            path: path.to_path_buf(),
            detail: "supported formats are static PNG and JPEG".to_string(),
        });
    }

    reject_animated_png(path, format)?;
    let mut decoder = reader.into_decoder().map_err(|error| decode_error(path, error))?;
    let orientation = decoder.orientation().map_err(|_error| ImageImportError {
        kind: ImageImportErrorKind::Orientation,
        path: path.to_path_buf(),
        detail: "image orientation metadata could not be read".to_string(),
    })?;
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|error| decode_error(path, error))?;
    image.apply_orientation(orientation);

    Ok(ImportedImage {
        pixels: image.to_rgba8(),
        source,
    })
}

fn open_error_kind(error: &std::io::Error) -> ImageImportErrorKind {
    if error.kind() == ErrorKind::NotFound {
        ImageImportErrorKind::NotFound
    } else {
        ImageImportErrorKind::Read
    }
}

fn reject_animated_png(path: &Path, format: ImageFormat) -> Result<(), ImageImportError> {
    if format != ImageFormat::Png {
        return Ok(());
    }
    let mut file = File::open(path).map_err(|error| ImageImportError {
        kind: ImageImportErrorKind::Read,
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    file.seek(SeekFrom::Start(8)).map_err(|error| ImageImportError {
        kind: ImageImportErrorKind::Read,
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    loop {
        let mut header = [0_u8; 8];
        file.read_exact(&mut header).map_err(|error| ImageImportError {
            kind: ImageImportErrorKind::Decode,
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap());
        let chunk = &header[4..];
        if chunk == b"acTL" {
            return Err(ImageImportError {
                kind: ImageImportErrorKind::UnsupportedFormat,
                path: path.to_path_buf(),
                detail: "animated PNG is not supported".to_string(),
            });
        }
        if chunk == b"IDAT" || chunk == b"IEND" {
            return Ok(());
        }
        file.seek(SeekFrom::Current(i64::from(length) + 4))
            .map_err(|error| ImageImportError {
                kind: ImageImportErrorKind::Decode,
                path: path.to_path_buf(),
                detail: error.to_string(),
            })?;
    }
}

fn decode_error(path: &Path, error: ImageError) -> ImageImportError {
    let kind = if matches!(error, ImageError::Limits(_)) {
        ImageImportErrorKind::ResourceLimit
    } else {
        ImageImportErrorKind::Decode
    };
    ImageImportError {
        kind,
        path: path.to_path_buf(),
        detail: match kind {
            ImageImportErrorKind::ResourceLimit =>
                "image exceeds decoder resource limits".to_string(),
            _ => "image data is corrupt or incomplete".to_string(),
        },
    }
}
```

Use the same concise wording for `decoder.orientation()` failures (`"image orientation metadata could not be read"`). Keep raw `ImageError` text out of `Display`; the primary stderr contract must not dump decoder internals. Add this focused mapping test:

```rust
#[test]
fn decoder_limit_errors_have_a_concise_actionable_category() {
    let error = ImageError::Limits(image::error::LimitError::from_kind(
        image::error::LimitErrorKind::InsufficientMemory,
    ));

    let error = decode_error(Path::new("large.png"), error);

    assert_eq!(error.kind(), ImageImportErrorKind::ResourceLimit);
    assert_eq!(
        error.to_string(),
        "could not open image large.png: image exceeds decoder resource limits"
    );
}
```

The reader's default 512 MiB allocation limit remains enabled; do not call `no_limits()`.

Add a platform-independent unreadable-input category test without relying on the test runner's filesystem privileges:

```rust
#[test]
fn permission_denied_is_a_read_failure() {
    let error = std::io::Error::from(ErrorKind::PermissionDenied);
    assert_eq!(open_error_kind(&error), ImageImportErrorKind::Read);
}
```

- [ ] **Step 6: Add identity tests for symlinks, hard links, and non-existing destinations**

Add Unix-only tests:

```rust
#[cfg(unix)]
#[test]
fn source_identity_matches_symlink_hard_link_and_resolved_missing_path() {
    use std::os::unix::fs::symlink;

    let current_dir = std::env::current_dir().unwrap();
    let dir = tempfile::tempdir_in(&current_dir).unwrap();
    let source_path = dir.path().join("source.png");
    write_png(&source_path);
    let imported = load(&source_path).unwrap();

    let relative_path = source_path.strip_prefix(&current_dir).unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let normalized_path = nested.join("..").join("source.png");

    let symlink_path = dir.path().join("alias.png");
    symlink(&source_path, &symlink_path).unwrap();
    let hard_link_path = dir.path().join("hard.png");
    std::fs::hard_link(&source_path, &hard_link_path).unwrap();

    assert!(imported.source.destination_matches(&source_path).unwrap());
    assert!(imported.source.destination_matches(relative_path).unwrap());
    assert!(imported.source.destination_matches(&normalized_path).unwrap());
    assert!(imported.source.destination_matches(&symlink_path).unwrap());
    assert!(imported.source.destination_matches(&hard_link_path).unwrap());
    assert!(!imported
        .source
        .destination_matches(&dir.path().join("new-export.png"))
        .unwrap());

    std::fs::remove_file(&source_path).unwrap();
    assert!(imported.source.destination_matches(&source_path).unwrap());
}
```

Also exercise empty relative parents without mutating the process current directory:

```rust
#[test]
fn relative_display_and_destination_paths_use_current_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    write_png(&path);
    let metadata = std::fs::metadata(&path).unwrap();
    let source = ImportedSource {
        display_path: PathBuf::from("source.png"),
        canonical_path: std::fs::canonicalize(&path).unwrap(),
        #[cfg(unix)]
        device: {
            use std::os::unix::fs::MetadataExt as _;
            metadata.dev()
        },
        #[cfg(unix)]
        inode: {
            use std::os::unix::fs::MetadataExt as _;
            metadata.ino()
        },
    };
    let missing = Path::new("rollshot-import-test-nonexistent.png");
    assert!(!missing.exists());
    assert_eq!(source.default_export_dir(), Path::new("."));
    assert!(!source.destination_matches(missing).unwrap());
}
```

- [ ] **Step 7: Run loader tests and app formatting**

Run:

```bash
rtk cargo test -p rollshot-app image_import::tests -- --nocapture
rtk cargo fmt --check
```

Expected: all import tests pass; formatting check exits 0.

- [ ] **Step 8: Commit the import boundary**

```bash
rtk git add crates/rollshot-app/src/image_import.rs crates/rollshot-app/src/main.rs
rtk git commit -m "feat(app): load existing PNG and JPEG images"
```

---

### Task 3: Model imported documents explicitly

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/document.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/secure_sharing.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-app/src/result_workspace/document.rs`

**Interfaces:**
- Consumes: `crate::image_import::ImportedSource` from Task 2.
- Produces: `DocumentOrigin::{UnsavedCapture, SavedCapture, Imported}`.
- Produces: `ResultDocument::imported`, `source_path`, `imported_source`, `is_imported`, `default_save_dir`, `default_save_name`, and `origin_status`.

- [ ] **Step 1: Write failing imported lifecycle and naming tests**

Add these tests in `result_workspace/document.rs`:

```rust
fn imported_document() -> ResultDocument {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("screen.jpg");
    image().save_with_format(&path, image::ImageFormat::Png).unwrap();
    let imported = crate::image_import::load(&path).unwrap();
    ResultDocument::imported(imported.pixels, imported.source)
}

#[test]
fn imported_origin_is_clean_durable_and_uses_annotated_png_name() {
    let document = imported_document();

    assert!(document.is_imported());
    assert_eq!(document.display_name(), "screen.jpg");
    assert_eq!(document.default_save_name(), "screen-annotated.png");
    assert_eq!(document.origin_status(false), Some("Imported"));
    assert_eq!(
        document.origin_status(true),
        Some("Imported • Unsaved edits")
    );
    assert_eq!(close_decision(&document, false), CloseDecision::Close);
    assert_eq!(
        close_decision(&document, true),
        CloseDecision::Confirm(DiscardPrompt {
            lose_capture: false,
            lose_edits: true,
        })
    );
}

#[test]
fn imported_reveal_prefers_latest_export() {
    let mut document = imported_document();
    let source = document.source_path().unwrap().to_path_buf();
    assert_eq!(document.reveal_path(), Some(source.as_path()));

    document.last_export_path = Some(PathBuf::from("/tmp/export.png"));
    assert_eq!(document.reveal_path(), Some(Path::new("/tmp/export.png")));
}
```

Keep the `TempDir` alive in the helper by returning it with the document:

```rust
fn imported_document() -> (tempfile::TempDir, ResultDocument) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("screen.jpg");
    image().save_with_format(&path, image::ImageFormat::Png).unwrap();
    let imported = crate::image_import::load(&path).unwrap();
    let document = ResultDocument::imported(imported.pixels, imported.source);
    (dir, document)
}
```

Use `let (_dir, document) = imported_document();` in each test.

- [ ] **Step 2: Run document tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::document::tests -- --nocapture
```

Expected: compilation fails because imported origin APIs do not exist.

- [ ] **Step 3: Replace optional source state with `DocumentOrigin`**

Change the document model to:

```rust
pub enum DocumentOrigin {
    UnsavedCapture,
    SavedCapture(PathBuf),
    Imported(crate::image_import::ImportedSource),
}

pub struct ResultDocument {
    pub image: ImageDocument,
    pub(crate) origin: DocumentOrigin,
    pub last_export_path: Option<PathBuf>,
    pub last_export_is_safe: bool,
}
```

Preserve existing constructor names and add imported construction:

```rust
impl ResultDocument {
    pub fn saved(image: RgbaImage, path: PathBuf) -> Self {
        Self::with_origin(image, DocumentOrigin::SavedCapture(path))
    }

    pub fn unsaved(image: RgbaImage) -> Self {
        Self::with_origin(image, DocumentOrigin::UnsavedCapture)
    }

    pub(crate) fn imported(
        image: RgbaImage,
        source: crate::image_import::ImportedSource,
    ) -> Self {
        Self::with_origin(image, DocumentOrigin::Imported(source))
    }

    fn with_origin(image: RgbaImage, origin: DocumentOrigin) -> Self {
        Self {
            image: ImageDocument::new(image),
            origin,
            last_export_path: None,
            last_export_is_safe: false,
        }
    }
}
```

- [ ] **Step 4: Add origin behavior methods and make close decisions origin-aware**

Add methods with these exact signatures and behavior:

```rust
pub fn source_path(&self) -> Option<&Path> {
    match &self.origin {
        DocumentOrigin::SavedCapture(path) => Some(path),
        DocumentOrigin::Imported(source) => Some(source.display_path()),
        DocumentOrigin::UnsavedCapture => None,
    }
}

pub(crate) fn imported_source(&self) -> Option<&crate::image_import::ImportedSource> {
    match &self.origin {
        DocumentOrigin::Imported(source) => Some(source),
        DocumentOrigin::UnsavedCapture | DocumentOrigin::SavedCapture(_) => None,
    }
}

pub(crate) fn is_imported(&self) -> bool {
    matches!(&self.origin, DocumentOrigin::Imported(_))
}

pub(crate) fn default_save_dir(&self) -> Option<PathBuf> {
    self.imported_source().map(|source| source.default_export_dir())
}

pub(crate) fn default_save_name(&self) -> String {
    match &self.origin {
        DocumentOrigin::Imported(source) => {
            let stem = source
                .display_path()
                .file_stem()
                .map(|stem| stem.to_string_lossy())
                .filter(|stem| !stem.is_empty())
                .unwrap_or_else(|| "Rollshot".into());
            format!("{stem}-annotated.png")
        }
        DocumentOrigin::SavedCapture(path) => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(capture_default_save_name),
        DocumentOrigin::UnsavedCapture => capture_default_save_name(),
    }
}

pub(crate) fn origin_status(&self, dirty: bool) -> Option<&'static str> {
    self.is_imported().then_some(if dirty {
        "Imported • Unsaved edits"
    } else {
        "Imported"
    })
}
```

Move the timestamp fallback into `capture_default_save_name()`. Update `display_name`, `reveal_path`, and `close_decision`; `lose_capture` is true only for `UnsavedCapture` with no export.

- [ ] **Step 5: Replace direct `source_path` field reads with methods**

Make these exact call-site substitutions:

```rust
document.source_path.as_deref()
```

becomes:

```rust
document.source_path()
```

Apply it in:

- `result_workspace/mod.rs` initial messages;
- `result_workspace/secure_sharing.rs` retained-original and Reveal logic;
- `result_workspace/update.rs` confirmed unredacted Reveal;
- macOS tests that assert a source exists.

Do not route imported documents through `post_capture.rs`; captured constructors remain unchanged there.

- [ ] **Step 6: Keep secure naming exact for imported exports**

Update `secure_sharing::default_save_name` so imported documents keep the approved `<stem>-annotated.png` name even when they contain redactions; captured documents retain the current `-redacted` suffix behavior:

```rust
pub(crate) fn default_save_name(document: &ResultDocument) -> String {
    let base = document.default_save_name();
    if !has_secure_redactions(document) || document.is_imported() {
        return base;
    }
    add_redacted_suffix(&base)
}
```

Extract the existing suffix code unchanged into `add_redacted_suffix`.

- [ ] **Step 7: Run document, secure-sharing, and workspace state tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::document::tests -- --nocapture
rtk cargo test -p rollshot-app result_workspace::secure_sharing::tests -- --nocapture
rtk cargo test -p rollshot-app result_workspace::tests -- --nocapture
```

Expected: all three focused suites pass; existing saved and unsaved capture behavior remains unchanged.

- [ ] **Step 8: Commit the document-origin model**

```bash
rtk git add crates/rollshot-app/src/result_workspace/document.rs crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/secure_sharing.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/macos_product.rs
rtk git commit -m "refactor(app): model imported image documents"
```

---

### Task 4: Enforce read-only source and PNG-only Save As

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/actions.rs`
- Modify: `crates/rollshot-app/src/result_workspace/secure_sharing.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Test: `crates/rollshot-app/src/result_workspace/actions.rs`
- Test: `crates/rollshot-app/src/result_workspace/secure_sharing.rs`
- Test: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**
- Consumes: `ResultDocument::imported_source`, `default_save_dir`, and `default_save_name` from Task 3.
- Produces: `normalize_png_destination(PathBuf) -> Result<PathBuf, String>`.
- Produces: `validate_export_destination(&ResultDocument, &Path) -> Result<(), ExportDestinationError>`.

- [ ] **Step 1: Write failing PNG destination normalization tests**

Add in `actions.rs`:

```rust
#[test]
fn png_destination_normalizes_missing_extension_and_rejects_other_extensions() {
    assert_eq!(
        normalize_png_destination(PathBuf::from("/tmp/result")).unwrap(),
        PathBuf::from("/tmp/result.png")
    );
    assert_eq!(
        normalize_png_destination(PathBuf::from("/tmp/result.PNG")).unwrap(),
        PathBuf::from("/tmp/result.PNG")
    );
    assert_eq!(
        normalize_png_destination(PathBuf::from("/tmp/result.jpg")).unwrap_err(),
        "Rollshot exports PNG files. Choose a .png filename."
    );
}
```

- [ ] **Step 2: Write failing imported-source protection tests**

Add a helper that loads a real temporary source, constructs `ResultDocument::imported`, and test:

```rust
#[cfg(unix)]
#[test]
fn imported_source_is_rejected_with_or_without_redactions() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("source.png");
    image().save_with_format(&source_path, image::ImageFormat::Png).unwrap();
    let imported = crate::image_import::load(&source_path).unwrap();
    let mut document = ResultDocument::imported(imported.pixels, imported.source);

    assert_eq!(
        validate_export_destination(&document, &source_path).unwrap_err(),
        ExportDestinationError::ImportedSourceReadOnly
    );

    let alias = dir.path().join("alias.png");
    symlink(&source_path, &alias).unwrap();
    assert_eq!(
        validate_export_destination(&document, &alias).unwrap_err(),
        ExportDestinationError::ImportedSourceReadOnly
    );

    add_redaction(&mut document);
    assert_eq!(
        validate_export_destination(&document, &source_path).unwrap_err(),
        ExportDestinationError::ImportedSourceReadOnly
    );
    assert!(validate_export_destination(&document, &dir.path().join("safe.png")).is_ok());
}
```

Keep the existing saved-capture test that rejects source overwrite only when secure redactions exist.

- [ ] **Step 3: Run focused tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::actions::tests::png_destination -- --nocapture
rtk cargo test -p rollshot-app result_workspace::secure_sharing::tests::imported_source -- --nocapture
```

Expected: compilation fails because normalization and validation APIs do not exist.

- [ ] **Step 4: Implement PNG destination normalization**

Add:

```rust
pub(crate) fn normalize_png_destination(mut path: PathBuf) -> Result<PathBuf, String> {
    match path.extension() {
        None => {
            path.set_extension("png");
            Ok(path)
        }
        Some(extension)
            if extension
                .to_str()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png")) =>
        {
            Ok(path)
        }
        Some(_) => Err("Rollshot exports PNG files. Choose a .png filename.".to_string()),
    }
}
```

An extension that is not valid UTF-8 also returns the same PNG-only error instead of being treated as absent.

- [ ] **Step 5: Replace the boolean overwrite helper with typed validation**

Define:

```rust
pub(crate) const IMPORTED_SOURCE_READ_ONLY_ERROR: &str =
    "Imported source is read-only. Choose another export location.";
pub(crate) const DESTINATION_VERIFICATION_ERROR: &str =
    "Rollshot could not verify the export destination. Choose another location.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportDestinationError {
    ImportedSourceReadOnly,
    UnsafeRedactionSource,
    VerificationFailed,
}

impl ExportDestinationError {
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::ImportedSourceReadOnly => IMPORTED_SOURCE_READ_ONLY_ERROR,
            Self::UnsafeRedactionSource => SAFE_EXPORT_OVERWRITE_ERROR,
            Self::VerificationFailed => DESTINATION_VERIFICATION_ERROR,
        }
    }
}

pub(crate) fn validate_export_destination(
    document: &ResultDocument,
    destination: &Path,
) -> Result<(), ExportDestinationError> {
    if let Some(source) = document.imported_source() {
        return match source.destination_matches(destination) {
            Ok(true) => Err(ExportDestinationError::ImportedSourceReadOnly),
            Ok(false) => Ok(()),
            Err(_) => Err(ExportDestinationError::VerificationFailed),
        };
    }

    if has_secure_redactions(document)
        && document
            .source_path()
            .is_some_and(|source| paths_resolve_equal(source, destination))
    {
        return Err(ExportDestinationError::UnsafeRedactionSource);
    }
    Ok(())
}
```

Keep the current literal/canonical path comparison in a private `paths_resolve_equal` helper for saved captures.

- [ ] **Step 6: Route Save As through document defaults, normalization, and validation**

Change the `Message::SaveAs` default directory:

```rust
let default_dir = state.document.default_save_dir().unwrap_or_else(|| {
    crate::storage::Platform::current()
        .and_then(crate::storage::default_output_dir)
        .unwrap_or_else(|_| PathBuf::from("."))
});
let default_name = super::secure_sharing::default_save_name(&state.document);
```

Change `Message::SavePathChosen(Some(path))` to normalize before validation and preserve state on either failure:

```rust
Message::SavePathChosen(Some(path)) => {
    let path = match super::actions::normalize_png_destination(path) {
        Ok(path) => path,
        Err(error) => {
            state.message = Some(InlineMessage::Error(error));
            return Task::none();
        }
    };
    if let Err(error) =
        super::secure_sharing::validate_export_destination(&state.document, &path)
    {
        state.message = Some(InlineMessage::Error(error.message().to_string()));
        return Task::none();
    }
    let safe_output = state.has_secure_redactions();
    let image = save_payload(state);
    let saved_state_id = state.document.image.state_id();
    Task::perform(
        async move { super::actions::write_save_as(&image, &path) },
        move |result| Message::SaveFinished {
            result,
            saved_state_id,
            safe_output,
        },
    )
}
```

- [ ] **Step 7: Add update-level state-preservation tests**

Add tests that send `SavePathChosen` for the source and a `.jpg` path:

```rust
fn imported_workspace_for_save() -> (tempfile::TempDir, ResultWorkspace, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("source.png");
    image::RgbaImage::new(4, 4)
        .save_with_format(&source_path, image::ImageFormat::Png)
        .unwrap();
    let imported = crate::image_import::load(&source_path).unwrap();
    let state = ResultWorkspace::with_config_path(
        super::super::document::ResultDocument::imported(imported.pixels, imported.source),
        None,
        None,
    );
    (dir, state, source_path)
}

#[test]
fn rejected_imported_destinations_preserve_document_and_export_state() {
    let (dir, mut state, source_path) = imported_workspace_for_save();
    for rejected_path in [source_path, dir.path().join("wrong-extension.jpg")] {
        let before_state_id = state.document.image.state_id();
        let before_export = state.document.last_export_path.clone();
        let task = update(&mut state, Message::SavePathChosen(Some(rejected_path)));
        drop(task);
        assert_eq!(state.document.image.state_id(), before_state_id);
        assert_eq!(state.document.last_export_path, before_export);
        assert!(state.message.as_ref().unwrap().is_error());
    }
}
```

Also keep the successful Save As test proving `apply_save_as` sets `editor.saved_state_id`, clears pending discard state, and records the normalized `.png` path.

- [ ] **Step 8: Run focused save and secure-sharing suites**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::actions -- --nocapture
rtk cargo test -p rollshot-app result_workspace::secure_sharing -- --nocapture
rtk cargo test -p rollshot-app result_workspace::update::tests::save -- --nocapture
```

Expected: all focused tests pass, including pre-existing secure-redaction behavior.

- [ ] **Step 9: Commit source protection and export normalization**

```bash
rtk git add crates/rollshot-app/src/result_workspace/actions.rs crates/rollshot-app/src/result_workspace/secure_sharing.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(app): protect imported image sources"
```

---

### Task 5: Show imported and dirty state in the Workspace

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Test: `crates/rollshot-app/src/result_workspace/mod.rs`

**Interfaces:**
- Consumes: `ResultDocument::origin_status` from Task 3 and `ResultWorkspace::annotations_dirty`.
- Produces: `ResultWorkspace::document_status_text() -> Option<&'static str>` rendered in the status bar.

- [ ] **Step 1: Invoke the required iced skills before editing UI**

Invoke `iced-rs` for iced 0.14 API guidance, then invoke `testing-iced-ui` in auto mode. Record the selected scenario, capability probe result, raw evidence path, and allowed baseline paths. Do not create or approve a golden baseline from the product-changing context.

- [ ] **Step 2: Write failing state and rendered-visibility tests**

Add an imported-workspace helper and tests in `result_workspace/mod.rs`:

```rust
fn imported_workspace() -> (tempfile::TempDir, ResultWorkspace) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    image().save_with_format(&path, image::ImageFormat::Png).unwrap();
    let imported = crate::image_import::load(&path).unwrap();
    let state = ResultWorkspace::with_config_path(
        ResultDocument::imported(imported.pixels, imported.source),
        None,
        None,
    );
    (dir, state)
}

#[test]
fn imported_workspace_status_is_visible_and_tracks_dirty_state() {
    let (_dir, mut state) = imported_workspace();
    assert_eq!(state.document_status_text(), Some("Imported"));

    let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));
    assert!(ui.find("Imported").is_ok());

    state
        .document
        .image
        .add_text_note(
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
            "note".to_string(),
        )
        .unwrap();
    assert_eq!(
        state.document_status_text(),
        Some("Imported • Unsaved edits")
    );
    let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));
    assert!(ui.find("Imported • Unsaved edits").is_ok());
}
```

- [ ] **Step 3: Run the focused test and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::tests::imported_workspace_status -- --nocapture
```

Expected: compilation fails because `document_status_text` and rendered status are absent.

- [ ] **Step 4: Implement persistent imported status without changing capture chrome**

Add:

```rust
pub(crate) fn document_status_text(&self) -> Option<&'static str> {
    self.document.origin_status(self.annotations_dirty())
}
```

In `view.rs::status_bar`, start with an empty row, conditionally push imported status, then push the existing dimensions, zoom label, and controls:

```rust
let mut status = row![].spacing(8).align_y(Alignment::Center);
if let Some(document_status) = state.document_status_text() {
    status = status.push(text(document_status));
}
let status = status
    .push(text(dims))
    .push(text(zoom_label).width(Length::Fill))
    .push(button(text("Fit Width")).on_press(Message::SetZoom(ZoomMode::FitWidth)))
    .push(button(text("Fit Window")).on_press(Message::SetZoom(ZoomMode::FitWindow)))
    .push(button(text("Fit Height")).on_press(Message::SetZoom(ZoomMode::FitHeight)))
    .push(button(text("100%")).on_press(Message::SetZoom(ZoomMode::ActualSize)))
    .push(button(text("-")).on_press(Message::ZoomStep(ZoomDirection::Out)))
    .push(button(text("+")).on_press(Message::ZoomStep(ZoomDirection::In)));
```

Keep the OCR-only `Copy all OCR text` append after this block.

In `ResultWorkspace::with_max_texture_dim`, show the existing `Saved to` initial success only for `SavedCapture`; imported status belongs in the persistent status bar and does not create an expiring success message.

- [ ] **Step 5: Run shared iced behavior tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::tests::imported_workspace_status -- --nocapture
rtk cargo test -p rollshot-app result_workspace::tests::result_workspace_chrome_is_visible_at_supported_window_sizes -- --nocapture
```

Expected: imported status is visible at 1100×760 and existing toolbar/status controls remain unclipped at 1100×760 and 640×420.

- [ ] **Step 6: Capture and independently review raw visual evidence**

Run the `testing-iced-ui` scenario selected in Step 1 for clean imported and dirty imported states at wide and minimum supported sizes. Send raw evidence to a clean-context reviewer using `fork_turns="none"`. Only that reviewer may update explicitly allowed baseline paths after the skill's semantic image-capability probe succeeds. If semantic inspection is unavailable, obtain a capable clean-context review or switch to explicit human mode.

- [ ] **Step 7: Commit Workspace status behavior and any independently approved baseline**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/view.rs
rtk git commit -m "feat(app): show imported image status"
```

If an independent reviewer approved a baseline file, add only that explicitly authorized path to the same commit.

---

### Task 6: Route open-image startup on Linux and macOS

**Files:**
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/diagnostics.rs`
- Create: `crates/rollshot-app/tests/open_cli.rs`
- Test: `crates/rollshot-app/src/main.rs`
- Test: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-app/src/diagnostics.rs`

**Interfaces:**
- Consumes: `LaunchMode::Open`, `image_import::load`, and `ResultDocument::imported`.
- Produces: `prepare_open_document(&Path) -> Result<ResultDocument, String>`.
- Produces: Linux `run_open_image(ResultDocument)` and macOS `macos_product::run_imported(ResultDocument)`.

- [ ] **Step 1: Write failing preparation and diagnostics tests**

Add in `main.rs` tests:

```rust
#[test]
fn open_preparation_builds_imported_document_without_ocr_requirement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.png");
    image::RgbaImage::new(3, 4)
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap();

    let document = super::prepare_open_document(&path).unwrap();

    assert!(document.is_imported());
    assert_eq!(document.image.source().dimensions(), (3, 4));
}
```

Add in `diagnostics.rs` tests:

```rust
#[test]
fn image_import_failures_have_their_own_category() {
    assert_eq!(
        classify_app_error("could not open image /tmp/a.png: unsupported image format"),
        "image_import"
    );
}
```

- [ ] **Step 2: Write a failing process-level stderr test**

Create `tests/open_cli.rs`:

```rust
use std::process::Command;

#[test]
fn missing_open_image_exits_nonzero_with_actionable_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot-app"))
        .arg("open")
        .arg(&missing)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("could not open image"), "stderr = {stderr}");
    assert!(stderr.contains("missing.png"), "stderr = {stderr}");
}
```

- [ ] **Step 3: Run focused tests and confirm the red state**

Run:

```bash
rtk cargo test -p rollshot-app open_preparation -- --nocapture
rtk cargo test -p rollshot-app image_import_failures_have_their_own_category -- --nocapture
rtk cargo test -p rollshot-app --test open_cli -- --nocapture
```

Expected: preparation and process-level tests fail because open dispatch and user-visible error logging are not implemented.

- [ ] **Step 4: Implement shared preparation and Linux direct launch**

Add:

```rust
fn prepare_open_document(path: &std::path::Path) -> Result<result_workspace::ResultDocument, String> {
    let imported = image_import::load(path).map_err(|error| error.to_string())?;
    tracing::info!(
        target: diagnostics::TARGET_IMAGE_IMPORT,
        width = imported.pixels.width(),
        height = imported.pixels.height(),
        "image import complete"
    );
    Ok(result_workspace::ResultDocument::imported(
        imported.pixels,
        imported.source,
    ))
}

#[cfg(target_os = "linux")]
fn run_open_image(document: result_workspace::ResultDocument) -> Result<(), String> {
    result_workspace::run(document, None)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn run_open_image(_document: result_workspace::ResultDocument) -> Result<(), String> {
    Err("image workspace is unsupported on this platform".to_string())
}
```

Add the top-level match arm:

```rust
LaunchMode::Open { path } => {
    tracing::info!(
        target: diagnostics::TARGET_IMAGE_IMPORT,
        "image import started"
    );
    let document = prepare_open_document(&path)?;
    run_open_image(document).map_err(|error| {
        format!(
            "could not open image {}: workspace launch failed: {error}",
            path.display()
        )
    })
}
```

Do not call `run_iced_capture`, `storage::auto_save`, or `post_capture` from this arm.

- [ ] **Step 5: Add stable import diagnostics and expose the actual error on stderr**

Add:

```rust
pub(crate) const TARGET_IMAGE_IMPORT: &str = "rollshot::app::image_import";
```

Make import classification precede generic save/image classification:

```rust
if lower.contains("could not open image") || lower.contains("image import") {
    "image_import"
} else if lower.contains("launch") || lower.contains("argument") || lower.contains("payload") {
    "launch"
}
```

Include the actual user-facing error in the initialized tracing subscriber path:

```rust
tracing::error!(
    target: diagnostics::TARGET_APP,
    error_category = diagnostics::classify_app_error(&error),
    error = %error,
    "application failed"
);
```

Keep paths out of the import success event. The failure string may include the user-supplied path as approved by the spec.

- [ ] **Step 6: Add direct macOS Workspace construction and daemon reuse**

In `macos_product.rs`, add a constructor that never enters capture state:

```rust
fn from_imported_document(document: ResultDocument) -> (MacosProduct, Task<Message>) {
    let workspace = ResultWorkspace::new(document, None)
        .with_initial_viewport(INITIAL_WORKSPACE_VIEWPORT);
    let mut product = MacosProduct {
        phase: Phase::Workspace(workspace),
        purpose: CapturePurpose::Present,
        document: None,
        thumbnail_window: None,
        workspace_window: None,
        thumbnail_cursor: Point::ORIGIN,
        #[cfg(feature = "action-guide")]
        recording_tray: None,
        #[cfg(feature = "action-guide")]
        lock_conflict_path: None,
    };
    let open_task = open_presentation_window(&mut product);
    (product, open_task)
}
```

Extract the common daemon body from `run`:

```rust
fn run_product(product: MacosProduct, boot_task: Task<Message>) -> Result<(), String> {
    use std::sync::Mutex;

    let slot = Mutex::new(Some((product, boot_task)));
    iced::daemon(
        move || {
            slot.lock()
                .unwrap()
                .take()
                .expect("product already taken by daemon boot")
        },
        update,
        view,
    )
    .subscription(subscription)
    .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
    .font(rollshot_image_document::style::FONT_BOLD_BYTES)
    .theme(theme)
    .style(style)
    .run()
    .map_err(|error| error.to_string())
}

pub fn run_imported(document: ResultDocument) -> Result<(), String> {
    let (product, boot_task) = from_imported_document(document);
    run_product(product, boot_task)
}
```

Change existing capture `run` to call `run_product(product, boot_task)` after `MacosProduct::new` succeeds.

Add the macOS main adapter:

```rust
#[cfg(target_os = "macos")]
fn run_open_image(document: result_workspace::ResultDocument) -> Result<(), String> {
    macos_product::run_imported(document)
}
```

- [ ] **Step 7: Add macOS phase-routing tests**

In `macos_product.rs` tests, construct an imported document from a real temporary PNG and assert:

```rust
#[test]
fn imported_document_boots_workspace_without_capture_or_thumbnail_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    image().save_with_format(&path, image::ImageFormat::Png).unwrap();
    let imported = crate::image_import::load(&path).unwrap();
    let document = ResultDocument::imported(imported.pixels, imported.source);

    let (product, _open_task) = from_imported_document(document);

    assert!(matches!(product.phase, Phase::Workspace(_)));
    assert!(product.document.is_none());
    assert!(product.thumbnail_window.is_none());
    assert!(product.workspace_window.is_some());
}
```

This test is compiled and run in the macOS CI lane because `macos_product` is target-gated.

- [ ] **Step 8: Run default and OCR-enabled routing tests**

Run:

```bash
rtk cargo test -p rollshot-app open_preparation -- --nocapture
rtk cargo test -p rollshot-app image_import_failures_have_their_own_category -- --nocapture
rtk cargo test -p rollshot-app --test open_cli -- --nocapture
rtk cargo test -p rollshot-app --features ocr open_preparation -- --nocapture
```

Expected: all commands pass. The default-feature preparation test proves opening an image does not depend on OCR.

- [ ] **Step 9: Commit platform routing and diagnostics**

```bash
rtk git add crates/rollshot-app/src/main.rs crates/rollshot-app/src/macos_product.rs crates/rollshot-app/src/diagnostics.rs crates/rollshot-app/tests/open_cli.rs
rtk git commit -m "feat(app): open imported images in workspace"
```

---

### Task 7: Verify OCR behavior, document the feature, and run release checks

**Files:**
- Modify: `README.md:440-475`
- Modify: `.github/workflows/internal-release.yml`
- Modify: `packaging/arch/PKGBUILD`
- Modify: `scripts/release/test_packaging_files.py`
- Test: `crates/rollshot-app/src/result_workspace/update.rs`
- Test: `crates/rollshot-app/src/result_workspace/toolbar.rs`
- Verify: all files changed by Tasks 1–6

**Interfaces:**
- Consumes: complete open-image flow and existing OCR Text workflow.
- Produces: documented user contract and final verification evidence.
- Produces: OCR-enabled Arch and macOS official release artifacts.

- [ ] **Step 1: Add an OCR feature-matrix regression test for imported documents**

In the existing OCR-gated Workspace update tests, add:

```rust
#[cfg(feature = "ocr")]
#[test]
fn imported_document_enters_existing_selectable_ocr_flow() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    image::RgbaImage::new(20, 10)
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap();
    let imported = crate::image_import::load(&path).unwrap();
    let mut state = super::super::ResultWorkspace::with_config_path(
        super::super::document::ResultDocument::imported(imported.pixels, imported.source),
        None,
        None,
    );

    let task = update(&mut state, Message::SelectTool(Tool::OcrText));
    drop(task);

    assert!(state.ocr_text.is_preparing_or_ready());
    assert!(!state.annotations_dirty());
}
```

In `toolbar.rs`, add this feature-matrix test; it proves imported documents use the normal annotation toolbar and OCR remains compile-time controlled:

```rust
#[test]
fn imported_document_uses_standard_annotation_toolbar() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    image().save_with_format(&path, image::ImageFormat::Png).unwrap();
    let imported = crate::image_import::load(&path).unwrap();
    let state = ResultWorkspace::with_config_path(
        ResultDocument::imported(imported.pixels, imported.source),
        None,
        None,
    );
    let model = toolbar_model(&state, 1100.0);

    for tool in [Tool::Select, Tool::Text, Tool::Arrow, Tool::Pen, Tool::Redact] {
        assert!(model.visible_tools.contains(&tool), "missing {tool:?}");
    }
    #[cfg(feature = "ocr")]
    assert!(model.more.iter().any(|item| item.label == "OCR Text"));
    #[cfg(not(feature = "ocr"))]
    assert!(!model.more.iter().any(|item| item.label == "OCR Text"));
}
```

- [ ] **Step 2: Run the OCR and non-OCR focused suites**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::toolbar::tests -- --nocapture
rtk cargo test -p rollshot-app --features ocr imported_document_enters_existing_selectable_ocr_flow -- --nocapture
rtk cargo test -p rollshot-app --features ocr result_workspace::ocr_text::tests -- --nocapture
```

Expected: annotation-only default tests and OCR-enabled imported tests all pass.

- [ ] **Step 3: Write failing release-packaging contract tests**

Extend `scripts/release/test_packaging_files.py` with exact assertions that both official artifacts enable OCR and provision the pinned ONNX Runtime library:

```python
def test_official_packages_build_rollshot_app_with_ocr():
    pkgbuild = (ROOT / "packaging/arch/PKGBUILD").read_text()
    workflow = (ROOT / ".github/workflows/internal-release.yml").read_text()

    assert "cargo build --release -p rollshot-app --features ocr" in pkgbuild
    assert "cargo build --release -p rollshot-app --features ocr" in workflow
    assert "provision-onnxruntime.sh Linux" in pkgbuild
    assert 'provision-onnxruntime.sh "${{ runner.os }}"' in workflow
    assert "ORT_LIB_LOCATION" in pkgbuild
    assert "ORT_LIB_LOCATION" in workflow
```

Run:

```bash
rtk pytest -q scripts/release/test_packaging_files.py
```

Expected: `test_official_packages_build_rollshot_app_with_ocr` fails because the release builds still use default features and do not provision ONNX Runtime.

- [ ] **Step 4: Enable OCR in both official release artifacts**

In `packaging/arch/PKGBUILD`:

- Add `curl` to `makedepends` because the pinned provisioner downloads the static ONNX Runtime archive.
- Provision into a package-build-local directory and pass its `lib` directory only to the build:

```bash
build() {
  cd "$startdir/../.."
  local ort_root="$srcdir/rollshot-ort"
  scripts/ci/provision-onnxruntime.sh Linux "$ort_root"
  ORT_LIB_LOCATION="$ort_root/lib" \
    cargo build --release -p rollshot-app --features ocr
}
```

The OCR models remain compile-time embedded by `crates/rollshot-ocr/build.rs`; do not add runtime model files to the package.

In `.github/workflows/internal-release.yml`:

- Add `curl` to the Arch container's explicit `pacman -S` build dependency list so it matches the new `makedepends` contract.
- Give the macOS job these build-only paths:

```yaml
    env:
      ROLLSHOT_OCR_MODELS_DIR: ${{ github.workspace }}/.ocr-models
      ORT_LIB_LOCATION: ${{ github.workspace }}/.ort/lib
```

- Before the macOS build, cache `.ocr-models` using the same `build.rs` hash key as `.github/workflows/ci-ocr.yml`, cache `.ort` with the pinned macOS key, and on a cache miss run:

```yaml
      - name: Provision ONNX Runtime static lib
        if: steps.ort-cache.outputs.cache-hit != 'true'
        run: |
          mkdir -p "${{ github.workspace }}/.ort"
          ./scripts/ci/provision-onnxruntime.sh "${{ runner.os }}" "${{ github.workspace }}/.ort"
```

- Change the macOS build command to `cargo build --release -p rollshot-app --features ocr`.

Do not ship the ONNX Runtime archive or model cache as separate artifacts: the application statically links the runtime and embeds the verified models.

- [ ] **Step 5: Run packaging contract and shell-syntax tests**

Run:

```bash
rtk pytest -q scripts/release/test_packaging_files.py
rtk bash -n scripts/ci/provision-onnxruntime.sh
```

Expected: all packaging tests pass and the provisioner remains valid shell.

- [ ] **Step 6: Document the exact CLI and trust contract**

Add a concise README section near Region Text Capture:

~~~~markdown
### Open an Existing Image

Open one static PNG or JPEG directly in the Result Workspace:

```bash
cargo run -p rollshot-app -- open ./screenshot.png
```

The source image is read-only. Annotation exports use **Save As**, default to
`<source-stem>-annotated.png` beside the source, and are flattened PNG files.
Rollshot refuses destinations that resolve to the imported source.

Opening and annotation work in the default build. Selectable **OCR Text** is
available when `rollshot-app` is built with the `ocr` feature:

```bash
cargo run -p rollshot-app --features ocr -- open ./screenshot.png
```

The first release accepts one local static PNG or JPEG. It does not add a GUI
file picker, drag-and-drop, multi-image tabs, or other image formats.
~~~~

- [ ] **Step 7: Run the complete app test matrix**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-app --features ocr
```

Expected: both commands exit 0 with no failed tests. OCR model/runtime-dependent tests use the repository's dedicated OCR test-lane wrapper when required by `scripts/ci/run-ocr-test.sh`.

- [ ] **Step 8: Run formatting and clippy**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both commands exit 0 with no formatting diff and no warnings.

- [ ] **Step 9: Run the dedicated OCR CI-equivalent command when local prerequisites exist**

Run:

```bash
rtk bash scripts/ci/run-ocr-test.sh cargo test -p rollshot-app --features ocr
```

Expected: OCR-enabled app tests exit 0. If the repository script reports a missing platform/runtime prerequisite, retain that exact output in the handoff and rely on the dedicated OCR CI lane rather than claiming the lane passed locally.

- [ ] **Step 10: Perform platform runtime smoke checks**

On Linux, run:

```bash
rtk cargo run -p rollshot-app -- open crates/rollshot-app/tests/eval/fixtures/selftest_region/image.png
```

Verify direct Workspace launch, annotation, imported/dirty status, Save As default, source-overwrite rejection, Reveal, and close confirmation.

On macOS, run the same command with `crates/rollshot-app/tests/eval/fixtures/selftest_region/image.png` in a macOS checkout and verify direct `Phase::Workspace` launch with no ScreenCaptureKit permission prompt and no thumbnail. Record macOS as unchecked if no macOS runtime is available; do not infer runtime success from Linux tests.

- [ ] **Step 11: Run final graph-aware impact and coverage checks**

Use code-review-graph after all edits:

1. `detect_changes` against `main` with source included.
2. `get_affected_flows` for the changed files.
3. `query_graph` with `pattern="tests_for"` for `launch.rs`, `image_import.rs`, `document.rs`, `secure_sharing.rs`, `main.rs`, and `macos_product.rs`.

Resolve any uncovered changed behavior with a focused test before completion.

- [ ] **Step 12: Commit documentation, release packaging, and final test adjustments**

```bash
rtk git add README.md .github/workflows/internal-release.yml packaging/arch/PKGBUILD scripts/release/test_packaging_files.py crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/toolbar.rs
rtk git commit -m "feat(release): ship open image OCR support"
```

- [ ] **Step 13: Verify the branch is clean and summarize evidence**

Run:

```bash
rtk git status --short --branch
rtk git log --oneline main..HEAD
```

Expected: the branch is `feat/open-image`, the working tree has no unstaged or staged files, and the task commits appear after the approved spec commits.

Summarize default tests, OCR tests, formatting, clippy, visual evidence review, Linux runtime, macOS runtime, and any environment-limited checks separately. Do not collapse unchecked platform/runtime evidence into a generic passing claim.
