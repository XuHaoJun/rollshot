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

use image::{GenericImage, GenericImageView, RgbaImage};

use crate::types::{AppendDirection, ScrollAxis};

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

pub struct LinearCanvas {
    image: RgbaImage,
    axis: Option<ScrollAxis>,
    last_append_copied_bytes: u64,
}

impl LinearCanvas {
    pub fn new(first_frame: RgbaImage) -> Self {
        Self {
            image: first_frame,
            axis: None,
            last_append_copied_bytes: 0,
        }
    }

    pub fn image(&self) -> &RgbaImage {
        &self.image
    }

    pub fn into_image(self) -> RgbaImage {
        self.image
    }

    pub fn axis(&self) -> Option<ScrollAxis> {
        self.axis
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn allocated_bytes(&self) -> u64 {
        self.image.as_raw().len() as u64
    }

    pub fn logical_pixels(&self) -> u64 {
        self.image.width() as u64 * self.image.height() as u64
    }

    pub fn last_append_copied_bytes(&self) -> u64 {
        self.last_append_copied_bytes
    }

    /// Appends `slice_px` new pixels from `frame` in the given `direction`.
    ///
    /// `slice_px` is the number of new rows (Bottom/Top) or columns
    /// (Right/Left) the canvas should gain. The caller is expected to derive
    /// it from a verified `MotionEstimate` (`dy` for vertical, `dx` for
    /// horizontal). The non-stitching dimension of `frame` must equal the
    /// canvas's current matching dimension.
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

        // The perpendicular dimension must match exactly. The parallel
        // dimension must not exceed the canvas's current parallel dimension,
        // because `overlap_size = frame_parallel/2 - slice_px` is subtracted
        // from `canvas.parallel` to compute the paste position; a frame taller
        // (or wider) than the canvas could otherwise underflow that subtraction.
        // For the normal stitcher flow, every frame's parallel dim equals the
        // first frame's parallel dim, and the canvas only grows, so this
        // condition is naturally satisfied.
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    #[test]
    fn append_bottom_adds_slice_below() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
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
        let mut canvas = LinearCanvas::new(base);
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
        let mut canvas = LinearCanvas::new(base);
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
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Left, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.width(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Horizontal));
    }

    #[test]
    fn axis_lock_rejects_perpendicular_direction() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
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
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
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
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
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
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
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
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
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
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
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
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 99).unwrap();
        assert_eq!(added, 4);
        assert_eq!(canvas.height(), 8);
    }

    // ------------------------------------------------------------------
    // v0.3 overlap-and-overwrite tests: append_bottom
    // ------------------------------------------------------------------

    #[test]
    fn append_bottom_pastes_at_canvas_height_minus_overlap() {
        // H=8, slice_px=2 → overlap = 4-2 = 2, total_slice = 4.
        // Slice = frame rows [4..8). Paste at canvas y = 8 - 2 = 6.
        // Slice overwrites canvas y=6..7 (= frame 1's rows 6..7) and adds
        // new canvas y=8..9 (= frame 2's rows 6..7).
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 10);
        // y=0..5 stays frame 1 (gray 10/10/10).
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([10, 10, 10, 255]));
        // y=6..9 is now frame 2's slice (red 200/0/0).
        assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 9), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_large_motion_uses_zero_overlap() {
        // H=4, slice_px=3 → overlap = max(0, 2 - 3) = 0, total_slice = 3.
        // Behaves identically to v0.2 minimal-slice append.
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.height(), 7);
        // y=0..3 stays frame 1.
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([10, 10, 10, 255]));
        // y=4..6 is frame 2's bottom 3 rows.
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_tiny_motion_overlap_is_h_over_2_minus_one() {
        // H=10, slice_px=1 → overlap = 5-1 = 4, total_slice = 5.
        // Slice = frame rows [5..10). Paste at canvas y = 10 - 4 = 6.
        // Overwrites canvas y=6..9 (frame 1) with frame 2's rows 5..8;
        // adds canvas y=10 (frame 2's row 9).
        let base = solid(2, 10, [10, 10, 10, 255]);
        let frame = solid(2, 10, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 1).unwrap();
        assert_eq!(added, 1);
        assert_eq!(canvas.height(), 11);
        // y=0..5 stays frame 1.
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([10, 10, 10, 255]));
        // y=6..10 is frame 2.
        assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 10), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_net_growth_equals_slice_px() {
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let h0 = canvas.height();
        let added = canvas.append(AppendDirection::Bottom, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(
            canvas.height(),
            h0 + 2,
            "canvas must grow by exactly slice_px"
        );
    }

    // ------------------------------------------------------------------
    // v0.3 overlap-and-overwrite tests: prepend_top
    // ------------------------------------------------------------------

    #[test]
    fn prepend_top_drops_overlap_rows_of_existing_canvas() {
        // H=8, slice_px=2 → overlap = 4-2 = 2, total_slice = 4.
        // Slice = frame rows [0..4). Combined y=0..3 = slice. Combined y=4..9
        // = old canvas rows [2..8). Old canvas rows 0..1 are dropped.
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Top, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 10);
        // y=0..3 is now frame 2's top (green 0/200/0).
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([0, 200, 0, 255]));
        // y=4..9 is what remains of frame 1.
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 9), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_top_large_motion_uses_zero_overlap() {
        // H=4, slice_px=3 → overlap = 0, total_slice = 3. Behaves like v0.2.
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
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
        let mut canvas = LinearCanvas::new(base);
        let h0 = canvas.height();
        let added = canvas.append(AppendDirection::Top, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), h0 + 2);
    }

    // ------------------------------------------------------------------
    // v0.3 overlap-and-overwrite tests: append_right
    // ------------------------------------------------------------------

    #[test]
    fn append_right_pastes_at_canvas_width_minus_overlap() {
        // W=8, slice_px=2 → overlap = 4-2 = 2, total_slice = 4.
        // Slice = frame cols [4..8). Paste at canvas x = 8 - 2 = 6.
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [0, 0, 200, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Right, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), 10);
        // x=0..5 stays frame 1.
        assert_eq!(canvas.image().get_pixel(5, 0), &Rgba([10, 10, 10, 255]));
        // x=6..9 is now frame 2's slice (blue).
        assert_eq!(canvas.image().get_pixel(6, 0), &Rgba([0, 0, 200, 255]));
        assert_eq!(canvas.image().get_pixel(9, 0), &Rgba([0, 0, 200, 255]));
    }

    #[test]
    fn append_right_large_motion_uses_zero_overlap() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 0, 200, 255]);
        let mut canvas = LinearCanvas::new(base);
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
        let mut canvas = LinearCanvas::new(base);
        let w0 = canvas.width();
        let added = canvas.append(AppendDirection::Right, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), w0 + 2);
    }

    // ------------------------------------------------------------------
    // v0.3 overlap-and-overwrite tests: prepend_left
    // ------------------------------------------------------------------

    #[test]
    fn prepend_left_drops_overlap_cols_of_existing_canvas() {
        // W=8, slice_px=2 → overlap = 2, total_slice = 4.
        // Slice = frame cols [0..4). Combined x=0..3 = slice (yellow).
        // Combined x=4..9 = old canvas cols [2..8).
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [200, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
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
        let mut canvas = LinearCanvas::new(base);
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
        let mut canvas = LinearCanvas::new(base);
        let w0 = canvas.width();
        let added = canvas.append(AppendDirection::Left, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), w0 + 2);
    }

    #[test]
    fn allocated_bytes_matches_image_buffer_length() {
        let canvas = LinearCanvas::new(solid(8, 4, [0, 0, 0, 255]));
        // 8 × 4 × 4 channels = 128 bytes
        assert_eq!(canvas.allocated_bytes(), 128);
    }

    #[test]
    fn logical_pixels_matches_width_times_height() {
        let canvas = LinearCanvas::new(solid(8, 4, [0, 0, 0, 255]));
        assert_eq!(canvas.logical_pixels(), 32);
    }

    #[test]
    fn last_append_copied_bytes_starts_at_zero() {
        let canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        assert_eq!(canvas.last_append_copied_bytes(), 0);
    }

    #[test]
    fn last_append_copied_bytes_reflects_combined_buffer_after_append() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        canvas
            .append(AppendDirection::Bottom, &solid(4, 4, [200, 0, 0, 255]), 2)
            .unwrap();
        // After append: canvas is 4 × 6 = 24 px × 4 channels = 96 bytes copied.
        assert_eq!(canvas.last_append_copied_bytes(), 96);
        // allocated_bytes should match.
        assert_eq!(canvas.allocated_bytes(), 96);
        assert_eq!(canvas.logical_pixels(), 24);
    }

    // ------------------------------------------------------------------
    // StripCanvas equivalence tests (RED — StripCanvas not yet declared)
    // ------------------------------------------------------------------

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

    fn assert_strip_matches_legacy(direction: AppendDirection, frames: &[RgbaImage], slices: &[u32]) {
        let mut legacy = LinearCanvas::new(frames[0].clone());
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
        let frames = vec![
            patterned(9, 8, 1),
            patterned(9, 8, 11),
            patterned(9, 8, 31),
        ];
        assert_strip_matches_legacy(AppendDirection::Bottom, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_top_prepends() {
        let frames = vec![
            patterned(9, 8, 2),
            patterned(9, 8, 12),
            patterned(9, 8, 32),
        ];
        assert_strip_matches_legacy(AppendDirection::Top, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_right_appends() {
        let frames = vec![
            patterned(8, 9, 3),
            patterned(8, 9, 13),
            patterned(8, 9, 33),
        ];
        assert_strip_matches_legacy(AppendDirection::Right, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_left_prepends() {
        let frames = vec![
            patterned(8, 9, 4),
            patterned(8, 9, 14),
            patterned(8, 9, 34),
        ];
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
        // Slow scroll: slice_px (4) << frame_h/2 (16), so each strip stores
        // ~16 rows but nets only 4. Without compaction, strip bytes grow to
        // several times the logical canvas (~3.5x here). Compaction must keep
        // resident strip+cache bytes within a small multiple of logical.
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
        // Output must still be correct after compaction.
        assert_eq!(canvas.height(), 32 + 40 * 4);
    }

    #[test]
    fn strip_canvas_matches_legacy_repeated_top_prepends() {
        // Multiple prepends: each shifts all prior strips and overwrites the
        // overlap. Byte-equivalence must hold after every prepend, not just one.
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
        // Slow bottom appends force at least one compaction (triggers at the
        // 5th append for these sizes), then a top prepend and a final bottom
        // append exercise direction changes *after* compaction. Output must
        // stay byte-identical to legacy through compaction and shifting.
        let base = patterned(8, 32, 9);
        let mut legacy = LinearCanvas::new(base.clone());
        let mut strip = StripCanvas::new(base);
        let mut ops: Vec<(AppendDirection, u8, u32)> =
            (0..30u8).map(|i| (AppendDirection::Bottom, 40 + i, 4)).collect();
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
