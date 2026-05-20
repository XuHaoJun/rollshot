use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

pub fn run<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        None | Some("--help" | "-h") => Ok(help()),
        Some("--version" | "-V") => Ok(format!("rollshot {}\n", env!("CARGO_PKG_VERSION"))),
        Some("probe") => Ok(probe()),
        Some("stitch-folder") => stitch_folder(&args[2..]),
        Some(command) => Err(format!("unknown command: {command}\n\n{}", help())),
    }
}

fn help() -> String {
    String::from(
        "rollshot\n\
         \n\
         Usage:\n\
           rollshot probe\n\
           rollshot stitch-folder <frames-dir> --output <png>\n\
           rollshot --version\n",
    )
}

fn probe() -> String {
    format!(
        "rollshot {}\n\
         os: {}\n\
         real capture: unavailable in bootstrap phase\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
    )
}

fn stitch_folder(args: &[String]) -> Result<String, String> {
    let mut frames_dir: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--output" | "-o" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("--output requires a path argument"))?;
                output = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => {
                return Err(format!("unexpected argument: {other}"));
            }
            other if frames_dir.is_none() => {
                frames_dir = Some(PathBuf::from(other));
            }
            other => {
                return Err(format!("unexpected argument: {other}"));
            }
        }
    }

    let frames_dir = frames_dir
        .ok_or_else(|| String::from("usage: rollshot stitch-folder <frames-dir> --output <png>"))?;
    let output = output.ok_or_else(|| {
        String::from(
            "--output is required\n\nusage: rollshot stitch-folder <frames-dir> --output <png>",
        )
    })?;

    if !frames_dir.is_dir() {
        return Err(format!(
            "frames directory not found: {}",
            frames_dir.display()
        ));
    }

    let frame_paths = collect_frame_paths(&frames_dir)?;
    if frame_paths.is_empty() {
        return Err(format!(
            "no supported images in {} (expected .png/.jpg/.jpeg)",
            frames_dir.display()
        ));
    }

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut appended = 0u32;
    let mut duplicates = 0u32;
    let mut no_match = 0u32;
    let mut no_progress = 0u32;

    for path in &frame_paths {
        let img = image::open(path)
            .map_err(|err| format!("failed to decode {}: {err}", path.display()))?;
        let frame = into_rgba(img);

        match stitcher.push_frame(frame) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            StitchOutcome::Duplicate => duplicates += 1,
            StitchOutcome::NoMatch { .. } => no_match += 1,
            StitchOutcome::NoProgress => no_progress += 1,
        }
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| String::from("no stitched output available"))?;
    stitched
        .save_with_format(&output, ImageFormat::Png)
        .map_err(|err| format!("failed to save {}: {err}", output.display()))?;

    Ok(format!(
        "stitch-folder: {dir}\n\
         input frames: {input}\n\
         appended: {appended}\n\
         duplicates: {duplicates}\n\
         no-progress: {no_progress}\n\
         no-match: {no_match}\n\
         output: {out} ({w}x{h})\n",
        dir = frames_dir.display(),
        input = frame_paths.len(),
        appended = appended,
        duplicates = duplicates,
        no_progress = no_progress,
        no_match = no_match,
        out = output.display(),
        w = stitched.width(),
        h = stitched.height(),
    ))
}

fn collect_frame_paths(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        fs::read_dir(dir).map_err(|err| format!("failed to read {}: {err}", dir.display()))?;

    let mut paths = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("failed to read entry in {}: {err}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("failed to inspect {}: {err}", path.display()))?;
        if !file_type.is_file() {
            continue;
        }
        if matches!(
            path.extension()
                .and_then(OsStr::to_str)
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("png" | "jpg" | "jpeg")
        ) {
            paths.push(path);
        }
    }

    paths.sort();
    Ok(paths)
}

fn into_rgba(image: DynamicImage) -> image::RgbaImage {
    match image {
        DynamicImage::ImageRgba8(rgba) => rgba,
        other => other.to_rgba8(),
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use image::{imageops, Rgba, RgbaImage};
    use std::path::PathBuf;

    #[test]
    fn probe_reports_bootstrap_status() {
        let output = run(["rollshot", "probe"]).expect("probe should succeed");

        assert!(output.contains("rollshot"));
        assert!(output.contains("real capture: unavailable"));
    }

    #[test]
    fn stitch_folder_requires_arguments() {
        let err = run(["rollshot", "stitch-folder"]).expect_err("missing args should fail");

        assert!(err.contains("usage"), "err = {err}");
    }

    #[test]
    fn stitch_folder_rejects_missing_directory() {
        let err = run([
            "rollshot",
            "stitch-folder",
            "/tmp/this/path/does/not/exist-rollshot",
            "--output",
            "/tmp/should-never-write.png",
        ])
        .expect_err("missing dir should fail");

        assert!(err.contains("not found"), "err = {err}");
    }

    #[test]
    fn stitch_folder_writes_png_from_supported_images() {
        let tempdir = tempdir_for_test("rollshot-cli-lib-stitch");
        let frames_dir = tempdir.join("frames");
        std::fs::create_dir_all(&frames_dir).expect("create frames dir");

        let canvas = make_scroll_canvas(160, 600);
        for (idx, y) in [0u32, 40, 80].iter().enumerate() {
            let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
            let path = frames_dir.join(format!("frame_{idx:03}.png"));
            frame.save(&path).expect("save frame");
        }

        let output_png = tempdir.join("stitched.notpng");
        let output = run([
            "rollshot",
            "stitch-folder",
            frames_dir.to_str().expect("frames dir utf8"),
            "--output",
            output_png.to_str().expect("output path utf8"),
        ])
        .expect("stitch-folder should succeed");

        assert!(output.contains("input frames: 3"), "output = {output}");
        assert!(output.contains("appended:"), "output = {output}");
        assert!(output.contains(output_png.to_string_lossy().as_ref()));

        let bytes = std::fs::read(&output_png).expect("read stitched png");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let stitched = image::load_from_memory(&bytes)
            .expect("decode stitched png")
            .to_rgba8();
        assert_eq!(stitched.width(), 160);
        assert!(stitched.height() > 160, "height = {}", stitched.height());

        let _ = std::fs::remove_dir_all(&tempdir);
    }

    fn tempdir_for_test(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("{label}-{nanos}-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));

        for y in (0..height).step_by(36) {
            let accent = ((y / 3) % 180) as u8;
            for x in 24..width.saturating_sub(24) {
                let stripe = if (x / 7 + y / 11) % 2 == 0 { 220 } else { 180 };
                img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
                if y + 1 < height {
                    img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }

        for col in [21u32, 47, 73, 99, 125] {
            if col >= width {
                continue;
            }
            for y in 12..height.saturating_sub(12) {
                if (y / 13) % 3 != 0 {
                    img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
                }
            }
        }

        img
    }
}
