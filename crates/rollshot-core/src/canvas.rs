//! Single-axis stitched canvas that can grow in four directions.
//!
//! # v0.3 overlap-and-overwrite topology
//!
//! Each new slice widens to `max(H/2, slice_px)` and pastes back into the
//! existing canvas by `overlap_size = max(0, H/2 - slice_px)` pixels, so the
//! new slice's overlap portion overwrites the previous slice's trailing
//! portion. Only the most recently appended slice's trailing pixels survive
//! in the canvas — naturally hiding sticky horizontal bands and 1 px
//! decorative borders without explicit detection.
//!
//! For `slice_px >= H/2`, `overlap_size` collapses to 0 and the helper
//! degenerates to v0.2's minimal-slice append.
//!
//! `append_bottom` geometry (vertical scroll-down; other directions
//! symmetric):
//!
//! ```text
//!   frame (H tall):           canvas grows downward:
//!   ┌───────────┐ row 0       canvas[0 .. paste_y)         preserved
//!   │  unused   │             canvas[paste_y .. canvas_h)  overlap, overwritten
//!   │  upper    │             canvas[canvas_h .. canvas_h + slice_px)  new
//!   │  portion  │
//!   ├───────────┤ row H - total_slice            paste_y = canvas_h - overlap_size
//!   │  overlap  │\
//!   │  portion  │ \--- overwrites canvas tail
//!   ├───────────┤ row H - slice_px
//!   │  new      │
//!   │  content  │ ---- appended at canvas tail
//!   └───────────┘ row H - 1
//! ```
//!
//! Full derivation and edge cases:
//! `docs/superpowers/specs/2026-05-22-rollshot-overlap-stitch-topology-design.md`.

use std::collections::VecDeque;

use image::{imageops, RgbaImage};

use crate::types::{AppendDirection, ScrollAxis};

/// Compact strips into a single base strip once their combined byte size
/// exceeds this multiple of the logical canvas. `2` bounds resident memory at
/// roughly the same level as the old eager `LinearCanvas` while preserving the
/// `O(frame_h)` amortized append cost.
const COMPACT_FACTOR: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasAppendError {
    /// The append direction's axis does not match the canvas's locked axis.
    AxisMismatch {
        locked: ScrollAxis,
        attempted: ScrollAxis,
    },
    /// The frame's perpendicular dimension does not match the canvas.
    DimensionMismatch { canvas: u32, frame: u32 },
    /// `slice_px` is zero -- nothing to append.
    EmptyAppend,
}

pub struct StripCanvas {
    axis: Option<ScrollAxis>,
    logical_width: u32,
    logical_height: u32,
    strips: VecDeque<CanvasStrip>,
    composed_cache: Option<RgbaImage>,
    last_append_copied_bytes: u64,
}

#[derive(Debug, Clone)]
struct CanvasStrip {
    image: RgbaImage,
    x: i64,
    y: i64,
}

impl StripCanvas {
    pub fn new(first_frame: RgbaImage) -> Self {
        let logical_width = first_frame.width();
        let logical_height = first_frame.height();
        let mut strips = VecDeque::new();
        strips.push_back(CanvasStrip {
            image: first_frame,
            x: 0,
            y: 0,
        });
        Self {
            axis: None,
            logical_width,
            logical_height,
            strips,
            composed_cache: None,
            last_append_copied_bytes: 0,
        }
    }

    pub fn image(&mut self) -> &RgbaImage {
        self.compose_if_needed();
        self.composed_cache.as_ref().expect("composed image")
    }

    pub fn into_image(mut self) -> RgbaImage {
        self.compose_if_needed();
        self.composed_cache.take().expect("composed image")
    }

    pub fn axis(&self) -> Option<ScrollAxis> {
        self.axis
    }

    pub fn width(&self) -> u32 {
        self.logical_width
    }

    pub fn height(&self) -> u32 {
        self.logical_height
    }

    pub fn allocated_bytes(&self) -> u64 {
        let strip_bytes: u64 = self
            .strips
            .iter()
            .map(|strip| strip.image.as_raw().len() as u64)
            .sum();
        let cache_bytes = self
            .composed_cache
            .as_ref()
            .map(|img| img.as_raw().len() as u64)
            .unwrap_or(0);
        strip_bytes + cache_bytes
    }

    pub fn logical_pixels(&self) -> u64 {
        self.logical_width as u64 * self.logical_height as u64
    }

    pub fn last_append_copied_bytes(&self) -> u64 {
        self.last_append_copied_bytes
    }

    pub fn append(
        &mut self,
        direction: AppendDirection,
        frame: &RgbaImage,
        slice_px: u32,
    ) -> Result<u32, CanvasAppendError> {
        let target_axis = direction.axis();
        if let Some(locked) = self.axis {
            if locked != target_axis {
                return Err(CanvasAppendError::AxisMismatch {
                    locked,
                    attempted: target_axis,
                });
            }
        }

        match target_axis {
            ScrollAxis::Vertical => {
                if frame.width() != self.logical_width {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_width,
                        frame: frame.width(),
                    });
                }
                if frame.height() > self.logical_height {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_height,
                        frame: frame.height(),
                    });
                }
            }
            ScrollAxis::Horizontal => {
                if frame.height() != self.logical_height {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_height,
                        frame: frame.height(),
                    });
                }
                if frame.width() > self.logical_width {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_width,
                        frame: frame.width(),
                    });
                }
            }
        }

        if slice_px == 0 {
            return Err(CanvasAppendError::EmptyAppend);
        }

        let added = match direction {
            AppendDirection::Bottom => self.append_bottom(frame, slice_px),
            AppendDirection::Top => self.prepend_top(frame, slice_px),
            AppendDirection::Right => self.append_right(frame, slice_px),
            AppendDirection::Left => self.prepend_left(frame, slice_px),
        };

        self.axis = Some(target_axis);
        self.composed_cache = None;
        self.compact_if_needed();
        Ok(added)
    }

    fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_h = frame.height();
        let slice_px = slice_px.min(frame_h);
        let overlap_px = (frame_h / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_h);
        let crop = imageops::crop_imm(frame, 0, frame_h - total_slice, frame.width(), total_slice)
            .to_image();
        let paste_y = self.logical_height as i64 - overlap_px as i64;
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: 0,
            y: paste_y,
        });
        self.logical_height += slice_px;
        slice_px
    }

    fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_h = frame.height();
        let slice_px = slice_px.min(frame_h);
        let overlap_px = (frame_h / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_h);
        for strip in &mut self.strips {
            strip.y += slice_px as i64;
        }
        let crop = imageops::crop_imm(frame, 0, 0, frame.width(), total_slice).to_image();
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: 0,
            y: 0,
        });
        self.logical_height += slice_px;
        slice_px
    }

    fn append_right(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_w = frame.width();
        let slice_px = slice_px.min(frame_w);
        let overlap_px = (frame_w / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_w);
        let crop = imageops::crop_imm(frame, frame_w - total_slice, 0, total_slice, frame.height())
            .to_image();
        let paste_x = self.logical_width as i64 - overlap_px as i64;
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: paste_x,
            y: 0,
        });
        self.logical_width += slice_px;
        slice_px
    }

    fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_w = frame.width();
        let slice_px = slice_px.min(frame_w);
        let overlap_px = (frame_w / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_w);
        for strip in &mut self.strips {
            strip.x += slice_px as i64;
        }
        let crop = imageops::crop_imm(frame, 0, 0, total_slice, frame.height()).to_image();
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: 0,
            y: 0,
        });
        self.logical_width += slice_px;
        slice_px
    }

    fn compose_if_needed(&mut self) {
        if self.composed_cache.is_some() {
            return;
        }
        let mut out = RgbaImage::new(self.logical_width, self.logical_height);
        for strip in &self.strips {
            overlay_copy(&mut out, &strip.image, strip.x, strip.y);
        }
        self.composed_cache = Some(out);
    }

    /// Collapse strips into a single base strip once their redundant overlap
    /// retention pushes total strip bytes past `COMPACT_FACTOR * logical`. This
    /// bounds resident memory while keeping append `O(frame_h)` amortized.
    fn compact_if_needed(&mut self) {
        let logical_bytes = self.logical_pixels() * 4;
        let strip_bytes: u64 = self
            .strips
            .iter()
            .map(|strip| strip.image.as_raw().len() as u64)
            .sum();
        if strip_bytes <= logical_bytes.saturating_mul(COMPACT_FACTOR) {
            return;
        }
        self.compose_if_needed();
        let base = self.composed_cache.take().expect("composed image");
        self.strips.clear();
        self.strips.push_back(CanvasStrip {
            image: base,
            x: 0,
            y: 0,
        });
    }
}

fn overlay_copy(dst: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64) {
    let dst_w = dst.width() as i64;
    let dst_h = dst.height() as i64;
    let src_w = src.width() as i64;
    let src_h = src.height() as i64;

    let copy_x0 = x.max(0);
    let copy_x1 = (x + src_w).min(dst_w);
    if copy_x1 <= copy_x0 {
        return;
    }
    let sx0 = (copy_x0 - x) as usize;
    let len_px = (copy_x1 - copy_x0) as usize;
    let len = len_px * 4;

    for sy in 0..src_h {
        let dy = y + sy;
        if dy < 0 || dy >= dst_h {
            continue;
        }
        let src_start = ((sy as usize * src.width() as usize) + sx0) * 4;
        let dst_start = ((dy as usize * dst.width() as usize) + copy_x0 as usize) * 4;
        dst.as_mut()[dst_start..dst_start + len]
            .copy_from_slice(&src.as_raw()[src_start..src_start + len]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImage, GenericImageView, Rgba};

    // ------------------------------------------------------------------
    // LegacyLinearCanvas: preserved copy of the old eager LinearCanvas for
    // equivalence testing against StripCanvas.
    // ------------------------------------------------------------------

    struct LegacyLinearCanvas {
        image: RgbaImage,
        axis: Option<ScrollAxis>,
        last_append_copied_bytes: u64,
    }

    impl LegacyLinearCanvas {
        fn new(first_frame: RgbaImage) -> Self {
            Self {
                image: first_frame,
                axis: None,
                last_append_copied_bytes: 0,
            }
        }

        fn image(&self) -> &RgbaImage {
            &self.image
        }

        fn axis(&self) -> Option<ScrollAxis> {
            self.axis
        }

        fn width(&self) -> u32 {
            self.image.width()
        }

        fn height(&self) -> u32 {
            self.image.height()
        }

        fn append(
            &mut self,
            direction: AppendDirection,
            frame: &RgbaImage,
            slice_px: u32,
        ) -> Result<u32, CanvasAppendError> {
            let target_axis = direction.axis();
            if let Some(locked) = self.axis {
                if locked != target_axis {
                    return Err(CanvasAppendError::AxisMismatch {
                        locked,
                        attempted: target_axis,
                    });
                }
            }

            match target_axis {
                ScrollAxis::Vertical => {
                    if frame.width() != self.image.width() {
                        return Err(CanvasAppendError::DimensionMismatch {
                            canvas: self.image.width(),
                            frame: frame.width(),
                        });
                    }
                    if frame.height() > self.image.height() {
                        return Err(CanvasAppendError::DimensionMismatch {
                            canvas: self.image.height(),
                            frame: frame.height(),
                        });
                    }
                }
                ScrollAxis::Horizontal => {
                    if frame.height() != self.image.height() {
                        return Err(CanvasAppendError::DimensionMismatch {
                            canvas: self.image.height(),
                            frame: frame.height(),
                        });
                    }
                    if frame.width() > self.image.width() {
                        return Err(CanvasAppendError::DimensionMismatch {
                            canvas: self.image.width(),
                            frame: frame.width(),
                        });
                    }
                }
            }

            if slice_px == 0 {
                return Err(CanvasAppendError::EmptyAppend);
            }

            let added = match direction {
                AppendDirection::Bottom => self.append_bottom(frame, slice_px),
                AppendDirection::Top => self.prepend_top(frame, slice_px),
                AppendDirection::Right => self.append_right(frame, slice_px),
                AppendDirection::Left => self.prepend_left(frame, slice_px),
            };

            self.axis = Some(target_axis);
            Ok(added)
        }

        fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
            let frame_h = frame.height();
            let slice_px = slice_px.min(frame_h);
            let overlap_size = (frame_h / 2).saturating_sub(slice_px);
            let total_slice = (slice_px + overlap_size).min(frame_h);
            let slice = frame.view(0, frame_h - total_slice, frame.width(), total_slice);
            let new_height = self.image.height() + slice_px;
            let paste_y = self.image.height() - overlap_size;
            let mut combined = RgbaImage::new(self.image.width(), new_height);
            combined.copy_from(&self.image, 0, 0).expect("copy base");
            combined.copy_from(&*slice, 0, paste_y).expect("copy slice");
            self.last_append_copied_bytes = combined.as_raw().len() as u64;
            self.image = combined;
            slice_px
        }

        fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
            let frame_h = frame.height();
            let slice_px = slice_px.min(frame_h);
            let overlap_size = (frame_h / 2).saturating_sub(slice_px);
            let total_slice = (slice_px + overlap_size).min(frame_h);
            let slice = frame.view(0, 0, frame.width(), total_slice);
            let new_height = self.image.height() + slice_px;
            let mut combined = RgbaImage::new(self.image.width(), new_height);
            combined.copy_from(&*slice, 0, 0).expect("copy slice");
            let kept_old = self.image.view(
                0,
                overlap_size,
                self.image.width(),
                self.image.height() - overlap_size,
            );
            combined
                .copy_from(&*kept_old, 0, total_slice)
                .expect("copy base");
            self.last_append_copied_bytes = combined.as_raw().len() as u64;
            self.image = combined;
            slice_px
        }

        fn append_right(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
            let frame_w = frame.width();
            let slice_px = slice_px.min(frame_w);
            let overlap_size = (frame_w / 2).saturating_sub(slice_px);
            let total_slice = (slice_px + overlap_size).min(frame_w);
            let slice = frame.view(frame_w - total_slice, 0, total_slice, frame.height());
            let new_width = self.image.width() + slice_px;
            let paste_x = self.image.width() - overlap_size;
            let mut combined = RgbaImage::new(new_width, self.image.height());
            combined.copy_from(&self.image, 0, 0).expect("copy base");
            combined.copy_from(&*slice, paste_x, 0).expect("copy slice");
            self.last_append_copied_bytes = combined.as_raw().len() as u64;
            self.image = combined;
            slice_px
        }

        fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
            let frame_w = frame.width();
            let slice_px = slice_px.min(frame_w);
            let overlap_size = (frame_w / 2).saturating_sub(slice_px);
            let total_slice = (slice_px + overlap_size).min(frame_w);
            let slice = frame.view(0, 0, total_slice, frame.height());
            let new_width = self.image.width() + slice_px;
            let mut combined = RgbaImage::new(new_width, self.image.height());
            combined.copy_from(&*slice, 0, 0).expect("copy slice");
            let kept_old = self.image.view(
                overlap_size,
                0,
                self.image.width() - overlap_size,
                self.image.height(),
            );
            combined
                .copy_from(&*kept_old, total_slice, 0)
                .expect("copy base");
            self.last_append_copied_bytes = combined.as_raw().len() as u64;
            self.image = combined;
            slice_px
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    fn patterned(width: u32, height: u32, seed: u8) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                img.put_pixel(
                    x,
                    y,
                    Rgba([
                        seed.wrapping_add((x * 3) as u8),
                        seed.wrapping_add((y * 5) as u8),
                        seed.wrapping_add(((x + y) * 7) as u8),
                        255,
                    ]),
                );
            }
        }
        img
    }

    fn assert_images_eq(left: &RgbaImage, right: &RgbaImage) {
        assert_eq!(left.dimensions(), right.dimensions());
        assert_eq!(left.as_raw(), right.as_raw());
    }

    // ------------------------------------------------------------------
    // LegacyLinearCanvas unit tests
    // ------------------------------------------------------------------

    #[test]
    fn append_bottom_adds_slice_below() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 6);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Vertical));
    }

    #[test]
    fn prepend_top_adds_slice_above() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Top, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.height(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Vertical));
    }

    #[test]
    fn append_right_adds_slice_to_the_right() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 0, 200, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Right, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), 6);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(5, 0), &Rgba([0, 0, 200, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Horizontal));
    }

    #[test]
    fn prepend_left_adds_slice_to_the_left() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Left, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.width(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Horizontal));
    }

    #[test]
    fn axis_lock_rejects_perpendicular_direction() {
        let mut canvas = LegacyLinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 4, [1, 1, 1, 255]);
        canvas.append(AppendDirection::Bottom, &frame, 1).unwrap();
        let err = canvas
            .append(AppendDirection::Right, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::AxisMismatch {
                locked: ScrollAxis::Vertical,
                attempted: ScrollAxis::Horizontal,
            }
        );
    }

    #[test]
    fn dimension_mismatch_is_reported() {
        let mut canvas = LegacyLinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(6, 4, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Bottom, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::DimensionMismatch {
                canvas: 4,
                frame: 6,
            }
        );
    }

    #[test]
    fn dimension_mismatch_in_horizontal_mode_is_reported() {
        let mut canvas = LegacyLinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 6, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Right, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::DimensionMismatch {
                canvas: 4,
                frame: 6,
            }
        );
    }

    #[test]
    fn parallel_dim_larger_than_canvas_is_rejected_vertical() {
        let mut canvas = LegacyLinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 8, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Bottom, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::DimensionMismatch {
                canvas: 4,
                frame: 8,
            }
        );
    }

    #[test]
    fn parallel_dim_larger_than_canvas_is_rejected_horizontal() {
        let mut canvas = LegacyLinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(8, 4, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Right, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::DimensionMismatch {
                canvas: 4,
                frame: 8,
            }
        );
    }

    #[test]
    fn zero_slice_px_is_rejected() {
        let mut canvas = LegacyLinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 4, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Bottom, &frame, 0)
            .unwrap_err();
        assert_eq!(err, CanvasAppendError::EmptyAppend);
    }

    #[test]
    fn slice_larger_than_frame_is_clamped_to_frame_size() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 99).unwrap();
        assert_eq!(added, 4);
        assert_eq!(canvas.height(), 8);
    }

    #[test]
    fn append_bottom_pastes_at_canvas_height_minus_overlap() {
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [200, 0, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 10);
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 9), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_large_motion_uses_zero_overlap() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.height(), 7);
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_tiny_motion_overlap_is_h_over_2_minus_one() {
        let base = solid(2, 10, [10, 10, 10, 255]);
        let frame = solid(2, 10, [200, 0, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 1).unwrap();
        assert_eq!(added, 1);
        assert_eq!(canvas.height(), 11);
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 10), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_net_growth_equals_slice_px() {
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [200, 0, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let h0 = canvas.height();
        let added = canvas.append(AppendDirection::Bottom, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(
            canvas.height(),
            h0 + 2,
            "canvas must grow by exactly slice_px"
        );
    }

    #[test]
    fn prepend_top_drops_overlap_rows_of_existing_canvas() {
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [0, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Top, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 10);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 9), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_top_large_motion_uses_zero_overlap() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Top, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.height(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 2), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_top_net_growth_equals_slice_px() {
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [0, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let h0 = canvas.height();
        let added = canvas.append(AppendDirection::Top, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), h0 + 2);
    }

    #[test]
    fn append_right_pastes_at_canvas_width_minus_overlap() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [0, 0, 200, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Right, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), 10);
        assert_eq!(canvas.image().get_pixel(5, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(6, 0), &Rgba([0, 0, 200, 255]));
        assert_eq!(canvas.image().get_pixel(9, 0), &Rgba([0, 0, 200, 255]));
    }

    #[test]
    fn append_right_large_motion_uses_zero_overlap() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 0, 200, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Right, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.width(), 7);
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(4, 0), &Rgba([0, 0, 200, 255]));
    }

    #[test]
    fn append_right_net_growth_equals_slice_px() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [0, 0, 200, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let w0 = canvas.width();
        let added = canvas.append(AppendDirection::Right, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), w0 + 2);
    }

    #[test]
    fn prepend_left_drops_overlap_cols_of_existing_canvas() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [200, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Left, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), 10);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(4, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(9, 0), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_left_large_motion_uses_zero_overlap() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Left, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.width(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(2, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_left_net_growth_equals_slice_px() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [200, 200, 0, 255]);
        let mut canvas = LegacyLinearCanvas::new(base);
        let w0 = canvas.width();
        let added = canvas.append(AppendDirection::Left, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), w0 + 2);
    }

    #[test]
    fn allocated_bytes_matches_image_buffer_length() {
        let canvas = LegacyLinearCanvas::new(solid(8, 4, [0, 0, 0, 255]));
        assert_eq!(canvas.last_append_copied_bytes, 0);
    }

    #[test]
    fn last_append_copied_bytes_starts_at_zero() {
        let canvas = LegacyLinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        assert_eq!(canvas.last_append_copied_bytes, 0);
    }

    // ------------------------------------------------------------------
    // StripCanvas equivalence tests
    // ------------------------------------------------------------------

    fn assert_strip_matches_legacy(
        direction: AppendDirection,
        frames: &[RgbaImage],
        slices: &[u32],
    ) {
        let mut legacy = LegacyLinearCanvas::new(frames[0].clone());
        let mut strip = StripCanvas::new(frames[0].clone());

        assert_images_eq(legacy.image(), strip.image());
        for (idx, slice_px) in slices.iter().copied().enumerate() {
            let frame = &frames[idx + 1];
            assert_eq!(
                legacy.append(direction, frame, slice_px),
                strip.append(direction, frame, slice_px)
            );
            assert_images_eq(legacy.image(), strip.image());
            assert_eq!(legacy.axis(), strip.axis());
            assert_eq!(legacy.width(), strip.width());
            assert_eq!(legacy.height(), strip.height());
        }
    }

    #[test]
    fn strip_canvas_matches_legacy_bottom_appends() {
        let frames = vec![patterned(9, 8, 1), patterned(9, 8, 11), patterned(9, 8, 31)];
        assert_strip_matches_legacy(AppendDirection::Bottom, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_top_prepends() {
        let frames = vec![patterned(9, 8, 2), patterned(9, 8, 12), patterned(9, 8, 32)];
        assert_strip_matches_legacy(AppendDirection::Top, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_right_appends() {
        let frames = vec![patterned(8, 9, 3), patterned(8, 9, 13), patterned(8, 9, 33)];
        assert_strip_matches_legacy(AppendDirection::Right, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_left_prepends() {
        let frames = vec![patterned(8, 9, 4), patterned(8, 9, 14), patterned(8, 9, 34)];
        assert_strip_matches_legacy(AppendDirection::Left, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_full_image_cache_is_stable_and_invalidated() {
        let mut canvas = StripCanvas::new(patterned(6, 6, 5));
        let first = canvas.image().clone();
        assert_images_eq(&first, canvas.image());

        canvas
            .append(AppendDirection::Bottom, &patterned(6, 6, 25), 2)
            .unwrap();
        let after = canvas.image().clone();
        assert_ne!(first.as_raw(), after.as_raw());
        assert_images_eq(&after, canvas.image());
    }

    #[test]
    fn strip_canvas_append_copied_bytes_tracks_only_new_strip() {
        let mut canvas = StripCanvas::new(patterned(4, 8, 6));
        canvas
            .append(AppendDirection::Bottom, &patterned(4, 8, 26), 2)
            .unwrap();

        assert_eq!(canvas.width(), 4);
        assert_eq!(canvas.height(), 10);
        assert_eq!(canvas.last_append_copied_bytes(), 4 * 4 * 4);
        assert!(canvas.last_append_copied_bytes() < canvas.logical_pixels() * 4);
    }

    #[test]
    fn strip_canvas_compacts_to_keep_memory_bounded() {
        let mut canvas = StripCanvas::new(patterned(8, 32, 7));
        for i in 0..40u8 {
            canvas
                .append(AppendDirection::Bottom, &patterned(8, 32, 50 + i), 4)
                .unwrap();
        }
        let logical_bytes = canvas.logical_pixels() * 4;
        assert!(
            canvas.allocated_bytes() <= logical_bytes * 3,
            "allocated {} should stay bounded vs logical {} (compaction not firing?)",
            canvas.allocated_bytes(),
            logical_bytes,
        );
        assert_eq!(canvas.height(), 32 + 40 * 4);
    }

    #[test]
    fn strip_canvas_matches_legacy_repeated_top_prepends() {
        let frames = vec![
            patterned(9, 8, 2),
            patterned(9, 8, 12),
            patterned(9, 8, 22),
            patterned(9, 8, 32),
            patterned(9, 8, 42),
        ];
        assert_strip_matches_legacy(AppendDirection::Top, &frames, &[2, 3, 2, 3]);
    }

    #[test]
    fn strip_canvas_matches_legacy_mixed_directions_and_compaction() {
        let base = patterned(8, 32, 9);
        let mut legacy = LegacyLinearCanvas::new(base.clone());
        let mut strip = StripCanvas::new(base);
        let mut ops: Vec<(AppendDirection, u8, u32)> = (0..30u8)
            .map(|i| (AppendDirection::Bottom, 40 + i, 4))
            .collect();
        ops.push((AppendDirection::Top, 200, 3));
        ops.push((AppendDirection::Bottom, 210, 5));
        for (dir, seed, slice) in ops {
            let f = patterned(8, 32, seed);
            assert_eq!(legacy.append(dir, &f, slice), strip.append(dir, &f, slice));
            assert_images_eq(legacy.image(), strip.image());
            assert_eq!(legacy.width(), strip.width());
            assert_eq!(legacy.height(), strip.height());
        }
    }
}
