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
#[allow(dead_code)]
pub(crate) struct ImageImportError {
    kind: ImageImportErrorKind,
    path: PathBuf,
    detail: String,
}

#[allow(dead_code)]
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
                    Ok(metadata.dev() == self.device && metadata.ino() == self.inode)
                }
                #[cfg(not(unix))]
                {
                    fs::canonicalize(destination)
                        .map(|path| path == self.canonical_path)
                        .map_err(|error| format!("could not verify export destination: {error}"))
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

pub(crate) fn load(path: &Path) -> Result<ImportedImage, ImageImportError> {
    // Stat before open: `File::open` on a FIFO (named pipe) blocks until a
    // writer appears, so reject non-regular files first.
    let metadata = fs::metadata(path).map_err(|error| ImageImportError {
        kind: open_error_kind(&error),
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

    let file = File::open(path).map_err(|error| ImageImportError {
        kind: open_error_kind(&error),
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;

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
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| decode_error(path, error))?;
    let orientation = decoder.orientation().map_err(|_error| ImageImportError {
        kind: ImageImportErrorKind::Orientation,
        path: path.to_path_buf(),
        detail: "image orientation metadata could not be read".to_string(),
    })?;
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|error| decode_error(path, error))?;
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
    file.seek(SeekFrom::Start(8))
        .map_err(|error| ImageImportError {
            kind: ImageImportErrorKind::Read,
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    loop {
        let mut header = [0_u8; 8];
        file.read_exact(&mut header)
            .map_err(|error| ImageImportError {
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
            ImageImportErrorKind::ResourceLimit => {
                "image exceeds decoder resource limits".to_string()
            }
            _ => "image data is corrupt or incomplete".to_string(),
        },
    }
}

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
            (
                "vector.svg",
                b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice(),
            ),
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
        assert_eq!(
            load(&missing).unwrap_err().kind(),
            ImageImportErrorKind::NotFound
        );
        assert_eq!(
            load(dir.path()).unwrap_err().kind(),
            ImageImportErrorKind::NotFile
        );

        let corrupt = dir.path().join("corrupt.png");
        std::fs::write(&corrupt, b"\x89PNG\r\n\x1a\ninvalid").unwrap();
        assert_eq!(
            load(&corrupt).unwrap_err().kind(),
            ImageImportErrorKind::Decode
        );
    }

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

        let mut exif = b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
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

    #[test]
    fn applies_all_jpeg_exif_orientations_before_returning_rgba() {
        for orientation in 1..=8 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("orientation-{orientation}.jpg"));
            let encoded = jpeg_with_orientation(orientation);
            std::fs::write(&path, &encoded).unwrap();

            let mut expected =
                image::load_from_memory_with_format(&encoded, ImageFormat::Jpeg).unwrap();
            expected
                .apply_orientation(image::metadata::Orientation::from_exif(orientation).unwrap());

            assert_eq!(load(&path).unwrap().pixels, expected.to_rgba8());
        }
    }

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

    #[test]
    fn permission_denied_is_a_read_failure() {
        let error = std::io::Error::from(ErrorKind::PermissionDenied);
        assert_eq!(open_error_kind(&error), ImageImportErrorKind::Read);
    }

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
        assert!(imported
            .source
            .destination_matches(&normalized_path)
            .unwrap());
        assert!(imported.source.destination_matches(&symlink_path).unwrap());
        assert!(imported
            .source
            .destination_matches(&hard_link_path)
            .unwrap());
        assert!(!imported
            .source
            .destination_matches(&dir.path().join("new-export.png"))
            .unwrap());

        std::fs::remove_file(&source_path).unwrap();
        assert!(imported.source.destination_matches(&source_path).unwrap());
    }

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
}
