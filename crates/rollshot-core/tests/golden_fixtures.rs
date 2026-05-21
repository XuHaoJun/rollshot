use std::fs;
use std::path::{Path, PathBuf};

use image::{imageops, Rgba, RgbaImage};
use rollshot_core::{
    AppendDirection, MatchMethod, StitchConfig, StitchOutcome, Stitcher,
};
use serde::Deserialize;

const FIXTURE_ROOT: &str = "tests/fixtures/linearscroll_v2";

const MAX_PIXEL_CHANNEL_DIFF: u8 = 4;
const MAX_MISMATCHED_PIXEL_RATIO: f32 = 0.005;

#[derive(Debug, Deserialize)]
struct ExpectedMotion {
    frame: usize,
    dx: i32,
    dy: i32,
    direction: String,
}

#[derive(Debug)]
struct ObservedMotion {
    frame: usize,
    dx: i32,
    dy: i32,
    direction: AppendDirection,
    method: MatchMethod,
}

#[test]
fn golden_fixtures_match_expected_outputs() {
    for family in [
        "linear_vertical_down",
        "linear_vertical_up",
        "linear_horizontal_right",
        "linear_horizontal_left",
        "repeated_rows",
        "repeated_grid",
        "low_feature_text",
        "image_cards",
        "bad_frame",
        "duplicate_frames",
    ] {
        run_fixture(family, StitchConfig::default());
    }

    let mut sticky_cfg = StitchConfig::default();
    sticky_cfg.verifier.downsample_max_mad = 40.0 / 255.0;
    sticky_cfg.verifier.full_res_max_mad = 30.0 / 255.0;
    run_fixture("sticky_header", sticky_cfg);
}

#[cfg(feature = "akaze")]
#[test]
fn akaze_golden_fixture_uses_akaze_fallback() {
    let mut cfg = StitchConfig::default();
    cfg.second_best_margin = 0.95;
    cfg.akaze.enabled = true;
    cfg.akaze.detector_threshold = 0.0005;
    cfg.akaze.min_raw_matches = 8;
    cfg.akaze.min_inliers = 6;
    cfg.akaze.min_inlier_ratio = 0.25;
    let observed = run_fixture("akaze_fallback", cfg);
    assert!(
        observed
            .iter()
            .any(|motion| motion.method == MatchMethod::Akaze),
        "akaze_fallback should contain at least one AKAZE motion, got {observed:?}"
    );
}

fn run_fixture(family: &str, config: StitchConfig) -> Vec<ObservedMotion> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(family);
    let frames = load_frames(&root.join("frames"));
    let expected_output = image::open(root.join("expected/output.png"))
        .expect("decode expected output")
        .to_rgba8();
    let expected_motions = load_expected_motions(&root.join("expected/motions.json"));

    let mut stitcher = Stitcher::new(config);
    let mut observed = Vec::new();

    for (idx, frame) in frames.into_iter().enumerate() {
        match stitcher.push_frame(frame) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended {
                direction,
                estimate,
                ..
            } => {
                observed.push(ObservedMotion {
                    frame: idx,
                    dx: estimate.dx,
                    dy: estimate.dy,
                    direction,
                    method: estimate.method,
                });
            }
            StitchOutcome::Duplicate | StitchOutcome::NoProgress { .. } => {}
            StitchOutcome::NoMatch { .. } | StitchOutcome::AxisChanged { .. } => {}
        }
    }

    let actual = stitcher.full_image().expect("stitched output");
    let image_ok = images_within_tolerance(actual, &expected_output);
    let motions_ok = motions_match(&observed, &expected_motions);
    if !image_ok || !motions_ok {
        write_failure_artifacts(family, actual, &expected_output, &observed, &expected_motions);
    }

    assert!(
        image_ok,
        "{family} output mismatch beyond tolerance"
    );
    assert!(
        motions_ok,
        "{family} motions mismatch: observed={observed:?}, expected={expected_motions:?}"
    );

    observed
}

fn images_within_tolerance(actual: &RgbaImage, expected: &RgbaImage) -> bool {
    if actual.dimensions() != expected.dimensions() {
        return false;
    }
    let total = (actual.width() as u64) * (actual.height() as u64);
    let mut mismatched = 0u64;
    for (a, e) in actual.pixels().zip(expected.pixels()) {
        let dr = a[0].abs_diff(e[0]);
        let dg = a[1].abs_diff(e[1]);
        let db = a[2].abs_diff(e[2]);
        let da = a[3].abs_diff(e[3]);
        let max_chan = dr.max(dg).max(db).max(da);
        if max_chan > MAX_PIXEL_CHANNEL_DIFF {
            return false;
        }
        if max_chan > 0 {
            mismatched += 1;
        }
    }
    let ratio = mismatched as f32 / total.max(1) as f32;
    ratio <= MAX_MISMATCHED_PIXEL_RATIO
}

fn load_frames(dir: &Path) -> Vec<RgbaImage> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read frames dir")
        .map(|entry| entry.expect("read frame entry").path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("png"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| image::open(path).expect("decode frame").to_rgba8())
        .collect()
}

fn load_expected_motions(path: &Path) -> Vec<ExpectedMotion> {
    let text = fs::read_to_string(path).expect("read motions json");
    serde_json::from_str(&text).expect("parse motions json")
}

fn motions_match(observed: &[ObservedMotion], expected: &[ExpectedMotion]) -> bool {
    observed.len() == expected.len()
        && observed
            .iter()
            .zip(expected)
            .all(|(observed, expected)| {
                observed.frame == expected.frame
                    && observed.dx == expected.dx
                    && observed.dy == expected.dy
                    && format!("{:?}", observed.direction) == expected.direction
            })
}

fn write_failure_artifacts(
    family: &str,
    actual: &RgbaImage,
    expected: &RgbaImage,
    observed: &[ObservedMotion],
    expected_motions: &[ExpectedMotion],
) {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-artifacts")
        .join(family);
    fs::create_dir_all(&out).expect("create artifact dir");

    actual
        .save(out.join("actual.png"))
        .expect("save actual");
    expected
        .save(out.join("expected.png"))
        .expect("save expected");
    diff_image(expected, actual)
        .save(out.join("diff.png"))
        .expect("save diff");
    side_by_side(expected, actual)
        .save(out.join("matches.png"))
        .expect("save side-by-side");

    let observed_json = observed
        .iter()
        .map(|motion| {
            format!(
                "    {{ \"frame\": {}, \"dx\": {}, \"dy\": {}, \"direction\": \"{:?}\", \"method\": \"{:?}\" }}",
                motion.frame, motion.dx, motion.dy, motion.direction, motion.method
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let expected_json = expected_motions
        .iter()
        .map(|motion| {
            format!(
                "    {{ \"frame\": {}, \"dx\": {}, \"dy\": {}, \"direction\": \"{}\" }}",
                motion.frame, motion.dx, motion.dy, motion.direction
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let report = format!(
        "{{\n  \"family\": \"{}\",\n  \"observed\": [\n{}\n  ],\n  \"expected\": [\n{}\n  ]\n}}\n",
        family, observed_json, expected_json
    );
    fs::write(out.join("report.json"), report).expect("write report");
}

fn diff_image(expected: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    let mut out = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    for y in 0..height {
        for x in 0..width {
            let e = if x < expected.width() && y < expected.height() {
                *expected.get_pixel(x, y)
            } else {
                Rgba([0, 0, 0, 255])
            };
            let a = if x < actual.width() && y < actual.height() {
                *actual.get_pixel(x, y)
            } else {
                Rgba([0, 0, 0, 255])
            };
            out.put_pixel(
                x,
                y,
                Rgba([
                    e[0].abs_diff(a[0]),
                    e[1].abs_diff(a[1]),
                    e[2].abs_diff(a[2]),
                    255,
                ]),
            );
        }
    }
    out
}

fn side_by_side(left: &RgbaImage, right: &RgbaImage) -> RgbaImage {
    let width = left.width() + right.width();
    let height = left.height().max(right.height());
    let mut out = RgbaImage::from_pixel(width, height, Rgba([20, 20, 20, 255]));
    imageops::replace(&mut out, left, 0, 0);
    imageops::replace(&mut out, right, left.width() as i64, 0);
    out
}

// ---------------------------------------------------------------------------
// Fixture Generator (run with --ignored)
// ---------------------------------------------------------------------------

#[ignore]
#[test]
fn refresh_linearscroll_v2_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ROOT);
    fs::create_dir_all(&root).expect("create fixture root");

    write_vertical_fixture(
        &root,
        "linear_vertical_down",
        FixtureCanvas::Generic,
        [0, 180, 356],
        0,
        320,
        false,
    );
    write_vertical_fixture(
        &root,
        "linear_vertical_up",
        FixtureCanvas::Generic,
        [356, 176, 0],
        0,
        320,
        false,
    );
    write_horizontal_fixture(&root, "linear_horizontal_right", [0, 180, 356], 0, 320);
    write_horizontal_fixture(&root, "linear_horizontal_left", [356, 176, 0], 0, 320);
    write_vertical_fixture(
        &root,
        "sticky_header",
        FixtureCanvas::ScrollCanvas,
        [0, 70, 140],
        0,
        320,
        true,
    );

    write_vertical_fixture(
        &root,
        "low_feature_text",
        FixtureCanvas::LowFeatureText,
        [0, 150, 300],
        0,
        320,
        false,
    );
    write_vertical_fixture(
        &root,
        "image_cards",
        FixtureCanvas::ImageCards,
        [0, 170, 340],
        0,
        320,
        false,
    );

    write_rejected_fixture(&root, "repeated_rows", FixtureCanvas::RepeatedRows);
    write_rejected_fixture(&root, "repeated_grid", FixtureCanvas::RepeatedGrid);

    write_bad_frame_fixture(&root);
    write_duplicate_fixture(&root);
    write_akaze_fixture(&root);
}

#[derive(Debug, Clone, Copy)]
enum FixtureCanvas {
    Generic,
    LowFeatureText,
    ImageCards,
    RepeatedRows,
    RepeatedGrid,
    ScrollCanvas,
}

fn build_canvas(kind: FixtureCanvas, width: u32, height: u32) -> RgbaImage {
    match kind {
        FixtureCanvas::Generic => make_fixture_canvas(width, height),
        FixtureCanvas::LowFeatureText => make_low_feature_text_canvas(width, height),
        FixtureCanvas::ImageCards => make_image_card_canvas(width, height),
        FixtureCanvas::RepeatedRows => make_repeated_rows_canvas(width, height),
        FixtureCanvas::RepeatedGrid => make_repeated_grid_canvas(width, height),
        FixtureCanvas::ScrollCanvas => make_sticky_scroll_canvas(width, height),
    }
}

fn write_vertical_fixture(
    root: &Path,
    name: &str,
    canvas_kind: FixtureCanvas,
    offsets: [u32; 3],
    x: u32,
    viewport: u32,
    sticky: bool,
) {
    let canvas = build_canvas(canvas_kind, 480, 1100);
    let dir = root.join(name);
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, y) in offsets.iter().enumerate() {
        let mut frame = imageops::crop_imm(&canvas, x, *y, viewport, viewport).to_image();
        if sticky {
            paint_fixture_header(&mut frame, 42);
        }
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let min_y = *offsets.iter().min().expect("min y");
    let max_y = offsets.iter().max().expect("max y") + viewport;
    let mut expected =
        imageops::crop_imm(&canvas, x, min_y, viewport, max_y - min_y).to_image();
    if sticky {
        paint_fixture_header(&mut expected, 42);
    }
    expected
        .save(expected_dir.join("output.png"))
        .expect("save expected output");

    write_motions(
        &expected_dir.join("motions.json"),
        &offsets
            .windows(2)
            .enumerate()
            .map(|(idx, pair)| {
                let dy = pair[1] as i32 - pair[0] as i32;
                (idx + 1, 0, dy, if dy >= 0 { "Bottom" } else { "Top" })
            })
            .collect::<Vec<_>>(),
    );
}

fn write_horizontal_fixture(root: &Path, name: &str, offsets: [u32; 3], y: u32, viewport: u32) {
    let canvas = make_fixture_canvas(1100, 480);
    let dir = root.join(name);
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, x) in offsets.iter().enumerate() {
        imageops::crop_imm(&canvas, *x, y, viewport, viewport)
            .to_image()
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let min_x = *offsets.iter().min().expect("min x");
    let max_x = offsets.iter().max().expect("max x") + viewport;
    imageops::crop_imm(&canvas, min_x, y, max_x - min_x, viewport)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected output");

    write_motions(
        &expected_dir.join("motions.json"),
        &offsets
            .windows(2)
            .enumerate()
            .map(|(idx, pair)| {
                let dx = pair[1] as i32 - pair[0] as i32;
                (
                    idx + 1,
                    dx,
                    0,
                    if dx >= 0 { "Right" } else { "Left" },
                )
            })
            .collect::<Vec<_>>(),
    );
}

fn write_akaze_fixture(root: &Path) {
    let canvas = make_akaze_fixture_canvas(320, 900);
    let dir = root.join("akaze_fallback");
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, y) in [0u32, 96, 192].iter().enumerate() {
        imageops::crop_imm(&canvas, 0, *y, 320, 320)
            .to_image()
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    imageops::crop_imm(&canvas, 0, 0, 320, 512)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(
        &expected_dir.join("motions.json"),
        &[(1, 0, 96, "Bottom"), (2, 0, 96, "Bottom")],
    );
}

fn write_rejected_fixture(root: &Path, name: &str, canvas_kind: FixtureCanvas) {
    let canvas = build_canvas(canvas_kind, 320, 700);
    let dir = root.join(name);
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, y) in [0u32, 32, 64].iter().enumerate() {
        imageops::crop_imm(&canvas, 0, *y, 320, 320)
            .to_image()
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }
    imageops::crop_imm(&canvas, 0, 0, 320, 320)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(&expected_dir.join("motions.json"), &[]);
}

fn write_bad_frame_fixture(root: &Path) {
    let canvas = make_fixture_canvas(320, 760);
    let dir = root.join("bad_frame");
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    imageops::crop_imm(&canvas, 0, 0, 320, 320)
        .to_image()
        .save(frames_dir.join("frame_000.png"))
        .expect("save frame");
    RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]))
        .save(frames_dir.join("frame_001.png"))
        .expect("save bad frame");
    imageops::crop_imm(&canvas, 0, 120, 320, 320)
        .to_image()
        .save(frames_dir.join("frame_002.png"))
        .expect("save recovery frame");
    imageops::crop_imm(&canvas, 0, 0, 320, 440)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(
        &expected_dir.join("motions.json"),
        &[(2, 0, 120, "Bottom")],
    );
}

fn write_duplicate_fixture(root: &Path) {
    let canvas = make_fixture_canvas(320, 760);
    let dir = root.join("duplicate_frames");
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    let first = imageops::crop_imm(&canvas, 0, 0, 320, 320).to_image();
    first
        .save(frames_dir.join("frame_000.png"))
        .expect("save first");
    first
        .save(frames_dir.join("frame_001.png"))
        .expect("save duplicate");
    imageops::crop_imm(&canvas, 0, 100, 320, 320)
        .to_image()
        .save(frames_dir.join("frame_002.png"))
        .expect("save scrolled");
    imageops::crop_imm(&canvas, 0, 0, 320, 420)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(
        &expected_dir.join("motions.json"),
        &[(2, 0, 100, "Bottom")],
    );
}

fn recreate_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
    fs::create_dir_all(path).expect("create dir");
}

fn write_motions(path: &Path, motions: &[(usize, i32, i32, &str)]) {
    let mut out = String::from("[\n");
    for (idx, (frame, dx, dy, direction)) in motions.iter().enumerate() {
        let comma = if idx + 1 == motions.len() { "" } else { "," };
        out.push_str(&format!(
            "  {{ \"frame\": {frame}, \"dx\": {dx}, \"dy\": {dy}, \"direction\": \"{direction}\" }}{comma}\n"
        ));
    }
    out.push_str("]\n");
    fs::write(path, out).expect("write motions");
}

fn make_fixture_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([242, 242, 242, 255]));
    for y in (0..height).step_by(31) {
        for x in 16..width.saturating_sub(16) {
            let v = ((x / 5 + y / 7) % 180) as u8;
            img.put_pixel(x, y, Rgba([40 + v / 2, 80 + v / 3, 130 + v / 4, 255]));
        }
    }
    for i in 0..80u32 {
        let x = 18 + ((i * 47) % width.saturating_sub(36).max(1));
        let y = 18 + ((i * 59) % height.saturating_sub(36).max(1));
        let color = Rgba([
            (30 + (i * 13) % 190) as u8,
            (50 + (i * 17) % 170) as u8,
            (70 + (i * 23) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 24).min(height) {
            for xx in x..(x + 42).min(width) {
                if xx % 7 == 0 || yy % 11 == 0 {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }

    for col in [42u32, 96, 154, 211, 268, 325, 382, 440] {
        if col >= width {
            continue;
        }
        for y in 20..height.saturating_sub(20) {
            if (y / 13) % 3 != 0 {
                img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
            }
        }
    }

    img
}

fn make_low_feature_text_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([252, 252, 252, 255]));
    for line in 0..(height / 28) {
        let y = 16 + line * 28;
        if y + 12 >= height {
            break;
        }
        let mut x = 20u32;
        let mut word_idx = 0u32;
        while x + 24 < width.saturating_sub(20) {
            let word_w = 24 + ((line * 7 + word_idx * 11) % 38);
            let gray = (28 + (word_idx * 23) % 60) as u8;
            for yy in y..(y + 10).min(height) {
                for xx in x..(x + word_w).min(width) {
                    img.put_pixel(xx, yy, Rgba([gray, gray, gray, 255]));
                }
            }
            x += word_w + 12 + ((word_idx * 5) % 14);
            word_idx += 1;
        }
    }
    img
}

fn make_image_card_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([248, 248, 248, 255]));
    let card_h = 140u32;
    let gap = 24u32;
    let mut idx = 0u32;
    let mut y = 16u32;
    while y + card_h < height {
        let interior = Rgba([
            (40 + (idx * 47) % 180) as u8,
            (60 + (idx * 53) % 170) as u8,
            (80 + (idx * 59) % 160) as u8,
            255,
        ]);
        let border = Rgba([30, 30, 30, 255]);
        for yy in y..(y + card_h).min(height) {
            for xx in 16..width.saturating_sub(16) {
                let on_border = yy == y
                    || yy + 1 == y + card_h
                    || xx == 16
                    || xx + 1 == width.saturating_sub(16);
                img.put_pixel(xx, yy, if on_border { border } else { interior });
            }
        }
        y += card_h + gap;
        idx += 1;
    }
    img
}

fn make_repeated_rows_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([250, 250, 250, 255]));
    for y in 0..height {
        let band = (y / 16) % 2;
        let color = if band == 0 {
            Rgba([40, 40, 40, 255])
        } else {
            Rgba([210, 210, 210, 255])
        };
        for x in 0..width {
            img.put_pixel(x, y, color);
        }
    }
    img
}

fn make_repeated_grid_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 16 + y / 16) % 2 == 0 { 48 } else { 208 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    img
}

fn make_akaze_fixture_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([246, 246, 246, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 18 + y / 18) % 2 == 0 { 232 } else { 214 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }

    for i in 0..80u32 {
        let x = 20 + ((i * 43) % width.saturating_sub(40).max(1));
        let y = 20 + ((i * 61) % height.saturating_sub(40).max(1));
        let color = Rgba([
            (20 + (i * 19) % 180) as u8,
            (30 + (i * 23) % 160) as u8,
            (40 + (i * 29) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 9).min(height) {
            for xx in x..(x + 9).min(width) {
                if xx == x || yy == y || xx + 1 == x + 9 || yy + 1 == y + 9 || xx == x + yy - y {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }

    img
}

fn paint_fixture_header(frame: &mut RgbaImage, header_h: u32) {
    for y in 0..header_h.min(frame.height()) {
        for x in 0..frame.width() {
            let color = if (x / 6 + y / 4) % 2 == 0 {
                Rgba([180, 40, 40, 255])
            } else {
                Rgba([35, 35, 90, 255])
            };
            frame.put_pixel(x, y, color);
        }
    }
}

fn make_sticky_scroll_canvas(width: u32, height: u32) -> RgbaImage {
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

    for block in 0..10u32 {
        let y0 = 30 + block * 80;
        let block_h = 34 + (block % 3) * 8;
        let color = [
            ((40u16 + block as u16 * 17) % 200) as u8,
            ((90u16 + block as u16 * 11) % 200) as u8,
            ((140u16 + block as u16 * 19) % 200) as u8,
            255,
        ];
        for y in y0..(y0 + block_h).min(height) {
            for x in 30..width.saturating_sub(30) {
                if x % (9 + block % 5) == 0 || y % (7 + block % 4) == 0 {
                    img.put_pixel(x, y, Rgba(color));
                }
            }
        }
    }

    for col in [42u32, 96, 154, 211, 268, 325, 382, 440] {
        if col >= width {
            continue;
        }
        for y in 20..height.saturating_sub(20) {
            if (y / 13) % 3 != 0 {
                img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
            }
        }
    }

    img
}
