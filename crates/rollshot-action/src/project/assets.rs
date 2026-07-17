use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use image::{ImageEncoder, ImageReader, RgbaImage};
use sha2::{Digest, Sha256};

use super::error::{ProjectError, ProjectErrorCategory};
use super::model::{ProjectFrame, SnapshotFramePayload};

#[allow(dead_code)]
pub(crate) struct EncodedAsset {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}

#[allow(dead_code)]
pub(crate) struct InspectedAsset {
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}

#[allow(dead_code)]
pub(crate) fn encode_png_asset(image: &RgbaImage) -> Result<EncodedAsset, ProjectError> {
    let (width, height) = image.dimensions();
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| ProjectError::Encode {
            message: e.to_string(),
        })?;
    let sha256 = hex_sha256(&buf);
    Ok(EncodedAsset {
        bytes: buf,
        sha256,
        width,
        height,
    })
}

#[allow(dead_code)]
fn hex_sha256(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    format!("{hash:x}")
}

#[allow(dead_code)]
pub(crate) fn asset_relative_path(sha256: &str) -> PathBuf {
    PathBuf::from(format!("assets/frames/{sha256}.png"))
}

#[allow(dead_code)]
fn open_project_asset(root: &Path, sha256: &str) -> Result<std::fs::File, ProjectError> {
    use rustix::fs::{fstat, openat, Mode, OFlags, CWD};

    let assets_dir = openat(
        CWD,
        root.join("assets"),
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| ProjectError::Io {
        path: root.join("assets"),
        source: e.into(),
    })?;

    let frames_dir = openat(
        &assets_dir,
        "frames",
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| ProjectError::Io {
        path: root.join("assets/frames"),
        source: e.into(),
    })?;

    let filename = format!("{sha256}.png");
    let handle = openat(
        &frames_dir,
        &filename,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| ProjectError::Io {
        path: root.join("assets/frames").join(&filename),
        source: e.into(),
    })?;

    let stat = fstat(&handle).map_err(|e| ProjectError::Io {
        path: root.join("assets/frames").join(&filename),
        source: e.into(),
    })?;

    // Verify regular file (S_IFREG = 0o100000)
    if (stat.st_mode & 0o170000) != 0o100000 {
        return Err(ProjectError::InvalidAsset {
            category: ProjectErrorCategory::InvalidAsset,
            frame_id: 0,
        });
    }

    Ok(handle.into())
}

#[allow(dead_code)]
fn read_asset_bytes(root: &Path, sha256: &str) -> Result<Vec<u8>, ProjectError> {
    let mut file = open_project_asset(root, sha256)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| ProjectError::Io {
        path: asset_relative_path(sha256),
        source: e,
    })?;
    Ok(buf)
}

#[allow(dead_code)]
pub(crate) fn inspect_png_asset(
    root: &Path,
    sha256: &str,
    expected_width: u32,
    expected_height: u32,
) -> Result<InspectedAsset, ProjectError> {
    let mut file = open_project_asset(root, sha256)?;

    // Stream through SHA-256
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| ProjectError::Io {
            path: asset_relative_path(sha256),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let computed = format!("{:x}", hasher.finalize());

    // Seek back and read dimensions
    file.seek(SeekFrom::Start(0))
        .map_err(|e| ProjectError::Io {
            path: asset_relative_path(sha256),
            source: e,
        })?;

    let mut header_buf = vec![0u8; 64];
    let n = file.read(&mut header_buf).map_err(|e| ProjectError::Io {
        path: asset_relative_path(sha256),
        source: e,
    })?;
    header_buf.truncate(n);

    let reader = ImageReader::new(Cursor::new(header_buf))
        .with_guessed_format()
        .map_err(|e| ProjectError::Io {
            path: asset_relative_path(sha256),
            source: e,
        })?;
    let (w, h) = reader.into_dimensions().map_err(|e| ProjectError::Io {
        path: asset_relative_path(sha256),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
    })?;

    if computed != sha256 {
        return Err(ProjectError::InvalidAsset {
            category: ProjectErrorCategory::InvalidAsset,
            frame_id: 0,
        });
    }

    if w != expected_width || h != expected_height {
        return Err(ProjectError::InvalidAsset {
            category: ProjectErrorCategory::InvalidAsset,
            frame_id: 0,
        });
    }

    Ok(InspectedAsset {
        sha256: computed,
        width: w,
        height: h,
    })
}

#[allow(dead_code)]
pub(crate) fn decode_png_asset(
    root: &Path,
    sha256: &str,
    expected_width: u32,
    expected_height: u32,
) -> Result<RgbaImage, ProjectError> {
    let mut file = open_project_asset(root, sha256)?;

    // Stream through SHA-256
    let mut hasher = Sha256::new();
    let mut buf = Vec::new();
    let mut read_buf = [0u8; 8192];
    loop {
        let n = file.read(&mut read_buf).map_err(|e| ProjectError::Io {
            path: asset_relative_path(sha256),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&read_buf[..n]);
        buf.extend_from_slice(&read_buf[..n]);
    }
    let computed = format!("{:x}", hasher.finalize());

    if computed != sha256 {
        return Err(ProjectError::InvalidAsset {
            category: ProjectErrorCategory::InvalidAsset,
            frame_id: 0,
        });
    }

    // Decode from the buffered bytes
    let img = image::load_from_memory_with_format(&buf, image::ImageFormat::Png).map_err(|_e| {
        ProjectError::InvalidAsset {
            category: ProjectErrorCategory::InvalidAsset,
            frame_id: 0,
        }
    })?;
    let rgba = img.to_rgba8();

    if rgba.width() != expected_width || rgba.height() != expected_height {
        return Err(ProjectError::InvalidAsset {
            category: ProjectErrorCategory::InvalidAsset,
            frame_id: 0,
        });
    }

    Ok(rgba)
}

fn fsync_dir(path: &Path) -> Result<(), ProjectError> {
    let file = std::fs::File::open(path).map_err(|e| ProjectError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    file.sync_all().map_err(|e| ProjectError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn materialize_asset(
    root: &Path,
    payload: SnapshotFramePayload,
    frame_id: u64,
    at_ms: u64,
) -> Result<ProjectFrame, ProjectError> {
    let frames_dir = root.join("assets").join("frames");
    std::fs::create_dir_all(&frames_dir).map_err(|e| ProjectError::Io {
        path: frames_dir.clone(),
        source: e,
    })?;

    let (encoded_bytes, sha256, width, height) = match payload {
        SnapshotFramePayload::Pixels(image) => {
            let encoded = encode_png_asset(&image)?;
            // Full decode verification for newly encoded assets
            let decoded =
                image::load_from_memory_with_format(&encoded.bytes, image::ImageFormat::Png)
                    .map_err(|_| ProjectError::InvalidAsset {
                        category: ProjectErrorCategory::InvalidAsset,
                        frame_id,
                    })?;
            let rgba = decoded.to_rgba8();
            if rgba.width() != encoded.width || rgba.height() != encoded.height {
                return Err(ProjectError::InvalidAsset {
                    category: ProjectErrorCategory::InvalidAsset,
                    frame_id,
                });
            }
            (encoded.bytes, encoded.sha256, encoded.width, encoded.height)
        }
        SnapshotFramePayload::ExistingAsset {
            project_root,
            sha256,
            width,
            height,
        } => {
            // Stream-verify digest and header, then read bytes from
            // the same safe openat handle (no path reopening).
            let inspected = inspect_png_asset(&project_root, &sha256, width, height)?;
            if inspected.sha256 != sha256 {
                return Err(ProjectError::InvalidAsset {
                    category: ProjectErrorCategory::InvalidAsset,
                    frame_id,
                });
            }
            let bytes = read_asset_bytes(&project_root, &sha256)?;
            (bytes, sha256, width, height)
        }
    };

    let final_path = frames_dir.join(format!("{sha256}.png"));

    // If final path exists, verify and return (symlink_metadata to avoid following symlinks)
    if std::fs::symlink_metadata(&final_path).is_ok() {
        let _ = inspect_png_asset(root, &sha256, width, height)?;
        return Ok(ProjectFrame {
            id: frame_id,
            at_ms,
            sha256,
            width,
            height,
        });
    }

    // Write to unique temp sibling, fsync, rename
    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp_path = frames_dir.join(format!(".tmp-{}-{}", std::process::id(), counter));

    let cleanup = |path: &Path| {
        let _ = std::fs::remove_file(path);
    };

    std::fs::write(&temp_path, &encoded_bytes).map_err(|e| {
        cleanup(&temp_path);
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    // Fsync the temp file
    let temp_file = std::fs::File::open(&temp_path).map_err(|e| {
        cleanup(&temp_path);
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;
    temp_file.sync_all().map_err(|e| {
        cleanup(&temp_path);
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    // Rename atomically; if final exists, verify and discard temp
    match std::fs::rename(&temp_path, &final_path) {
        Ok(()) => {
            // Fsync the frames directory to persist the rename on non-journaled filesystems
            fsync_dir(&frames_dir)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Final path was created between our check and rename; verify and discard temp
            cleanup(&temp_path);
            let _ = inspect_png_asset(root, &sha256, width, height)?;
        }
        Err(e) => {
            cleanup(&temp_path);
            return Err(ProjectError::Io {
                path: final_path,
                source: e,
            });
        }
    }

    Ok(ProjectFrame {
        id: frame_id,
        at_ms,
        sha256,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use image::Rgba;

    fn setup_project(root: &Path) {
        std::fs::create_dir_all(root.join("assets/frames")).unwrap();
    }

    #[test]
    fn encoded_asset_digest_drives_derived_path() {
        let image = RgbaImage::from_pixel(4, 3, Rgba([1, 2, 3, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        assert_eq!(encoded.sha256.len(), 64);
        assert!(encoded
            .sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_eq!(
            asset_relative_path(&encoded.sha256),
            PathBuf::from(format!("assets/frames/{}.png", encoded.sha256))
        );
        image::load_from_memory_with_format(&encoded.bytes, image::ImageFormat::Png).unwrap();
    }

    #[test]
    fn encoded_asset_deterministic_bytes() {
        let image = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        let a = encode_png_asset(&image).unwrap();
        let b = encode_png_asset(&image).unwrap();
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn inspect_png_asset_validates_header_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let image = RgbaImage::from_pixel(16, 12, Rgba([1, 2, 3, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        let dest = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).unwrap();

        let inspected = inspect_png_asset(root, &encoded.sha256, 16, 12).unwrap();
        assert_eq!(inspected.sha256, encoded.sha256);
        assert_eq!(inspected.width, 16);
        assert_eq!(inspected.height, 12);
    }

    #[test]
    fn inspect_png_asset_rejects_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let image = RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        let dest = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).unwrap();

        // Corrupt one byte in the file
        let mut corrupted = encoded.bytes.clone();
        corrupted[20] ^= 0xFF;
        std::fs::write(&dest, &corrupted).unwrap();

        let result = inspect_png_asset(root, &encoded.sha256, 4, 4);
        assert!(result.is_err());
    }

    #[test]
    fn inspect_png_asset_rejects_corrupt_header() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let sha256 = "a".repeat(64);
        let dest = root.join("assets/frames").join(format!("{sha256}.png"));
        // Write garbage that is not a PNG
        std::fs::write(&dest, b"not a png file at all").unwrap();

        let result = inspect_png_asset(root, &sha256, 4, 4);
        assert!(result.is_err());
    }

    #[test]
    fn symlinked_assets_dir_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        // Remove assets and create symlink to a different directory
        let other = dir.path().join("other_assets");
        std::fs::create_dir_all(other.join("frames")).unwrap();
        std::fs::remove_dir_all(root.join("assets")).unwrap();
        std::os::unix::fs::symlink(&other, root.join("assets")).unwrap();

        let result = open_project_asset(root, &"a".repeat(64));
        assert!(result.is_err());
    }

    #[test]
    fn symlinked_frames_dir_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        // Remove frames and create symlink
        let other = dir.path().join("other_frames");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::remove_dir_all(root.join("assets/frames")).unwrap();
        std::os::unix::fs::symlink(&other, root.join("assets/frames")).unwrap();

        let result = open_project_asset(root, &"a".repeat(64));
        assert!(result.is_err());
    }

    #[test]
    fn symlinked_png_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let target = root.join("assets/frames/real.png");
        std::fs::write(&target, b"fake").unwrap();
        let sha256 = "b".repeat(64);
        std::os::unix::fs::symlink(
            &target,
            root.join("assets/frames").join(format!("{sha256}.png")),
        )
        .unwrap();

        let result = open_project_asset(root, &sha256);
        assert!(result.is_err());
    }

    #[test]
    fn materialize_pixels_encodes_and_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let image = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        let payload = SnapshotFramePayload::Pixels(Arc::new(image.clone()));

        let frame = materialize_asset(root, payload, 1, 100).unwrap();
        assert_eq!(frame.id, 1);
        assert_eq!(frame.at_ms, 100);
        assert_eq!(frame.width, 8);
        assert_eq!(frame.height, 8);
        assert_eq!(frame.sha256.len(), 64);

        // Verify file exists
        let path = root
            .join("assets/frames")
            .join(format!("{}.png", frame.sha256));
        assert!(path.exists());

        // Verify decode
        let decoded = image::load_from_memory_with_format(
            &std::fs::read(&path).unwrap(),
            image::ImageFormat::Png,
        )
        .unwrap();
        assert_eq!(decoded.width(), 8);
        assert_eq!(decoded.height(), 8);
    }

    #[test]
    fn materialize_existing_asset_copies_without_decode() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        setup_project(&source);
        setup_project(&dest);

        let image = RgbaImage::from_pixel(16, 16, Rgba([50, 60, 70, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        let src_path = source
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&src_path, &encoded.bytes).unwrap();

        let payload = SnapshotFramePayload::ExistingAsset {
            project_root: source,
            sha256: encoded.sha256.clone(),
            width: 16,
            height: 16,
        };

        let frame = materialize_asset(&dest, payload, 2, 200).unwrap();
        assert_eq!(frame.sha256, encoded.sha256);
        assert_eq!(frame.width, 16);
        assert_eq!(frame.height, 16);

        let dest_file = dest
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        assert!(dest_file.exists());
        assert_eq!(std::fs::read(&dest_file).unwrap(), encoded.bytes);
    }

    #[test]
    fn materialize_deduplicates_existing_final_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let image = RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        let dest = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).unwrap();

        // Second materialize with same content should succeed
        let payload = SnapshotFramePayload::Pixels(Arc::new(image));
        let frame = materialize_asset(root, payload, 1, 100).unwrap();
        assert_eq!(frame.sha256, encoded.sha256);
    }

    #[test]
    fn digest_valid_invalid_png_pixel_data() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let image = RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255]));
        let encoded = encode_png_asset(&image).unwrap();

        // Write corrupt data with valid-looking filename
        let sha256 = encoded.sha256.clone();
        let dest = root.join("assets/frames").join(format!("{sha256}.png"));
        // Write valid PNG header but corrupt pixel data
        let mut corrupt = encoded.bytes.clone();
        // Corrupt IDAT payload (after first 30 bytes which cover signature + IHDR)
        if corrupt.len() > 40 {
            corrupt[35] ^= 0xFF;
        }
        std::fs::write(&dest, &corrupt).unwrap();

        // inspect should detect hash mismatch
        let result = inspect_png_asset(root, &sha256, 4, 4);
        assert!(result.is_err());
    }

    #[test]
    fn decode_png_asset_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_project(root);

        let image = RgbaImage::from_pixel(8, 6, Rgba([100, 150, 200, 255]));
        let encoded = encode_png_asset(&image).unwrap();
        let dest = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).unwrap();

        let decoded = decode_png_asset(root, &encoded.sha256, 8, 6).unwrap();
        assert_eq!(decoded.width(), 8);
        assert_eq!(decoded.height(), 6);
        assert_eq!(decoded.get_pixel(0, 0), &Rgba([100, 150, 200, 255]));
    }

    #[test]
    fn asset_relative_path_lowercase() {
        let sha256 = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let path = asset_relative_path(sha256);
        assert_eq!(
            path,
            PathBuf::from("assets/frames/abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890.png")
        );
    }
}
