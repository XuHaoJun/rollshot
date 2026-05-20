use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use image::DynamicImage;

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata};

pub struct FixtureBackend {
    dir: PathBuf,
}

impl FixtureBackend {
    pub fn new<P: Into<PathBuf>>(dir: P) -> Self {
        Self { dir: dir.into() }
    }

    fn collect_frames(&self) -> Result<Vec<PathBuf>, CaptureError> {
        if !self.dir.is_dir() {
            return Err(CaptureError::InvalidConfig {
                message: format!("fixture directory not found: {}", self.dir.display()),
            });
        }

        let entries = fs::read_dir(&self.dir).map_err(|err| CaptureError::InvalidConfig {
            message: format!("failed to read {}: {err}", self.dir.display()),
        })?;

        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| CaptureError::InvalidConfig {
                message: format!("failed to read entry in {}: {err}", self.dir.display()),
            })?;
            let path = entry.path();
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            if !is_file {
                continue;
            }
            let ext = path
                .extension()
                .and_then(OsStr::to_str)
                .map(str::to_ascii_lowercase);
            if matches!(ext.as_deref(), Some("png" | "jpg" | "jpeg")) {
                paths.push(path);
            }
        }
        paths.sort();

        if paths.is_empty() {
            return Err(CaptureError::InvalidConfig {
                message: format!(
                    "no supported images in {} (expected .png/.jpg/.jpeg)",
                    self.dir.display()
                ),
            });
        }

        Ok(paths)
    }
}

impl CaptureBackend for FixtureBackend {
    fn name(&self) -> &'static str {
        "fixture"
    }

    fn probe(&self) -> CaptureProbe {
        CaptureProbe {
            backend: "fixture",
            available: true,
            message: "directory-based test backend".to_string(),
            details: vec![("dir".to_string(), self.dir.display().to_string())],
        }
    }

    fn start(&mut self, _options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        let paths = self.collect_frames()?;
        Ok(Box::new(FixtureFrameStream { paths, index: 0 }))
    }
}

pub struct FixtureFrameStream {
    paths: Vec<PathBuf>,
    index: usize,
}

impl FrameStream for FixtureFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let path = match self.paths.get(self.index) {
            Some(p) => p.clone(),
            None => return Err(CaptureError::EndOfStream),
        };
        self.index += 1;

        let decoded = image::open(&path).map_err(|err| CaptureError::InvalidConfig {
            message: format!("failed to decode {}: {err}", path.display()),
        })?;
        let image = into_rgba(decoded);

        Ok(CapturedFrame {
            image,
            timestamp: SystemTime::now(),
            metadata: FrameMetadata::fixture(),
        })
    }
}

fn into_rgba(image: DynamicImage) -> image::RgbaImage {
    match image {
        DynamicImage::ImageRgba8(rgba) => rgba,
        other => other.to_rgba8(),
    }
}

#[cfg(test)]
mod tests {
    use super::FixtureBackend;
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;
    use crate::types::CaptureOptions;
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "rollshot-fixture-{label}-{nanos}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn write_solid(dir: &std::path::Path, name: &str, color: [u8; 4]) {
        let img = RgbaImage::from_pixel(4, 4, Rgba(color));
        img.save(dir.join(name)).expect("save fixture frame");
    }

    #[test]
    fn missing_directory_returns_invalid_config() {
        let mut backend = FixtureBackend::new("/tmp/rollshot-fixture-does-not-exist");
        match backend.start(CaptureOptions::default()) {
            Err(CaptureError::InvalidConfig { message }) => {
                assert!(
                    message.contains("fixture directory not found"),
                    "msg = {message}"
                );
            }
            Err(other) => panic!("expected InvalidConfig, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn empty_directory_returns_invalid_config() {
        let dir = temp_dir("empty");
        let mut backend = FixtureBackend::new(&dir);
        match backend.start(CaptureOptions::default()) {
            Err(CaptureError::InvalidConfig { message }) => {
                assert!(message.contains("no supported images"), "msg = {message}");
            }
            Err(other) => panic!("expected InvalidConfig, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frames_returned_in_sorted_order_then_end_of_stream() {
        let dir = temp_dir("sorted");
        write_solid(&dir, "frame_002.png", [0, 0, 255, 255]);
        write_solid(&dir, "frame_000.png", [255, 0, 0, 255]);
        write_solid(&dir, "frame_001.png", [0, 255, 0, 255]);
        // Non-image file should be ignored
        std::fs::write(dir.join("notes.txt"), b"ignore me").expect("write note");

        let mut backend = FixtureBackend::new(&dir);
        let mut stream = backend
            .start(CaptureOptions::default())
            .expect("start fixture backend");

        let first = stream.next_frame().expect("first frame");
        assert_eq!(first.image.get_pixel(0, 0).0, [255, 0, 0, 255]);

        let second = stream.next_frame().expect("second frame");
        assert_eq!(second.image.get_pixel(0, 0).0, [0, 255, 0, 255]);

        let third = stream.next_frame().expect("third frame");
        assert_eq!(third.image.get_pixel(0, 0).0, [0, 0, 255, 255]);

        match stream.next_frame() {
            Err(CaptureError::EndOfStream) => {}
            other => panic!("expected EndOfStream, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_marks_available() {
        let dir = temp_dir("probe");
        let backend = FixtureBackend::new(&dir);
        let probe = backend.probe();
        assert_eq!(probe.backend, "fixture");
        assert!(probe.available);
        assert!(probe.details.iter().any(|(k, _)| k == "dir"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
