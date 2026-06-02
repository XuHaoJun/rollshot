//! Synthetic stress scenarios for the bench harness.
//!
//! Each scenario is built lazily from a `SyntheticSpec`: a deterministic
//! `make_scroll_canvas` is sliced into N frames via `imageops::crop_imm`. The
//! frames are produced on demand so that 200-frame sequences don't sit in
//! RAM all at once.
//!
//! Patterns covered:
//!
//! - `long_vertical_text`: smooth scroll, dense text-like stripes (P1/P2/P3
//!   targets — append-time growth, prepare cache, NCC cost).
//! - `long_sticky_header`: same plus a sticky top band (P1 + sticky behavior
//!   under long runs).
//! - `long_vertical_jitter`: step ±2 px deterministic jitter (baseline for
//!   P7 subpixel work later).

use image::{imageops, Rgba, RgbaImage};

#[derive(Debug, Clone)]
pub struct SyntheticSpec {
    pub name: String,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub step_px: u32,
    /// Absolute jitter range. 0 = no jitter; 2 = each frame's offset varies by
    /// up to ±2 px from `idx * step_px`.
    pub step_jitter_px: u32,
    pub frame_count: usize,
    pub sticky_top_band_height: Option<u32>,
    /// When set to `(y0, y1)`, frames whose index is in `lazy_load_frames`
    /// paint a flat placeholder over rows [y0, y1) of the cropped frame,
    /// simulating a not-yet-loaded image. Other frames show textured content.
    pub lazy_block: Option<(u32, u32)>,
    pub lazy_load_frames: &'static [usize],
}

impl SyntheticSpec {
    pub fn validate(&self) {
        let jitter_abs = self.step_jitter_px;
        let last_offset = (self.frame_count as u32).saturating_sub(1) * self.step_px + jitter_abs;
        let required_canvas_height = self.frame_height + last_offset;
        assert!(
            self.canvas_height >= required_canvas_height,
            "SyntheticSpec[{}]: canvas_height={} too small for frame_count={} step_px={} jitter={} \
             (need >= {})",
            self.name,
            self.canvas_height,
            self.frame_count,
            self.step_px,
            jitter_abs,
            required_canvas_height,
        );
    }

    /// Lazy iterator over the spec's frames. Each frame is materialized on
    /// demand to keep peak memory bounded.
    pub fn frames<'a>(
        &'a self,
        base_canvas: &'a RgbaImage,
    ) -> impl Iterator<Item = RgbaImage> + 'a {
        let seed: u64 = 0xC0FFEE;
        let spec = self.clone();
        (0..spec.frame_count).map(move |idx| {
            let jitter = if spec.step_jitter_px == 0 {
                0i32
            } else {
                deterministic_jitter(seed, idx, spec.step_jitter_px)
            };
            let target_y = (idx as i64 * spec.step_px as i64 + jitter as i64)
                .max(0)
                .min((base_canvas.height() - spec.frame_height) as i64)
                as u32;

            let mut frame = imageops::crop_imm(
                base_canvas,
                0,
                target_y,
                spec.frame_width,
                spec.frame_height,
            )
            .to_image();

            if let Some(band_h) = spec.sticky_top_band_height {
                paint_sticky_band(&mut frame, band_h);
            }
            if let Some((y0, y1)) = spec.lazy_block {
                if spec.lazy_load_frames.contains(&idx) {
                    let h = frame.height();
                    let (y0, y1) = (y0.min(h), y1.min(h));
                    for y in y0..y1 {
                        for x in 0..frame.width() {
                            frame.put_pixel(x, y, Rgba([225, 225, 225, 255]));
                        }
                    }
                }
            }
            frame
        })
    }
}

fn deterministic_jitter(seed: u64, idx: usize, max_abs: u32) -> i32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = h.wrapping_add(idx as u64);
    h ^= h >> 32;
    let span = 2 * max_abs as u64 + 1;
    (h % span) as i32 - max_abs as i32
}

fn paint_sticky_band(frame: &mut RgbaImage, band_h: u32) {
    let w = frame.width();
    let h = band_h.min(frame.height());
    for y in 0..h {
        for x in 0..w {
            let v = if (x / 9) % 2 == 0 { 110 } else { 150 };
            frame.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
}

/// Builds a tall scroll-friendly canvas with stripes, color blocks and column
/// patterns. Mirrors `tests/common/mod.rs::make_scroll_canvas`.
pub fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
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

    for block in 0..40u32 {
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

    for col in [
        42u32, 96, 154, 211, 268, 340, 410, 480, 540, 620, 690, 760, 830,
    ] {
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

pub fn default_specs() -> Vec<SyntheticSpec> {
    vec![
        SyntheticSpec {
            name: "long_vertical_text".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 0,
            frame_count: 200,
            sticky_top_band_height: None,
            lazy_block: None,
            lazy_load_frames: &[],
        },
        SyntheticSpec {
            name: "long_sticky_header".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 0,
            frame_count: 200,
            sticky_top_band_height: Some(80),
            lazy_block: None,
            lazy_load_frames: &[],
        },
        SyntheticSpec {
            name: "long_vertical_jitter".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 2,
            frame_count: 200,
            sticky_top_band_height: None,
            lazy_block: None,
            lazy_load_frames: &[],
        },
        SyntheticSpec {
            name: "long_lazy_load".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 0,
            frame_count: 200,
            sticky_top_band_height: None,
            lazy_block: Some((560, 700)),
            lazy_load_frames: &[5, 20, 60, 120],
        },
    ]
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn deterministic_jitter_is_reproducible() {
        let a: Vec<_> = (0..50)
            .map(|i| deterministic_jitter(0xC0FFEE, i, 2))
            .collect();
        let b: Vec<_> = (0..50)
            .map(|i| deterministic_jitter(0xC0FFEE, i, 2))
            .collect();
        assert_eq!(a, b);
    }

    #[test]
    fn deterministic_jitter_respects_max_abs() {
        for i in 0..200 {
            let j = deterministic_jitter(0xC0FFEE, i, 3);
            assert!(j.abs() <= 3, "jitter {j} out of bounds at idx {i}");
        }
    }

    #[test]
    fn synthetic_spec_validate_accepts_well_formed_spec() {
        for spec in default_specs() {
            spec.validate();
        }
    }

    #[test]
    #[should_panic(expected = "canvas_height")]
    fn synthetic_spec_validate_rejects_oversized_traversal() {
        let mut bad = default_specs()[0].clone();
        bad.canvas_height = 100; // far too small for 200 frames × 40 px step
        bad.validate();
    }

    #[test]
    fn synthetic_spec_frames_yields_expected_count() {
        let spec = &default_specs()[0];
        let base = make_scroll_canvas(spec.canvas_width, spec.canvas_height);
        let count = spec.frames(&base).count();
        assert_eq!(count, spec.frame_count);
    }
}
