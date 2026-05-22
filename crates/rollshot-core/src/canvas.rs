//! Single-axis stitched canvas that can grow in four directions.

use image::{GenericImage, GenericImageView, Rgba, RgbaImage};

use crate::static_region::StaticMask;
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
}

fn apply_static_mask(
    slice: &mut RgbaImage,
    frame_w: u32,
    frame_h: u32,
    slice_origin_in_frame: (u32, u32),
    mask: &StaticMask,
) {
    let (off_x, off_y) = slice_origin_in_frame;
    for sy in 0..slice.height() {
        for sx in 0..slice.width() {
            let fx = sx + off_x;
            let fy = sy + off_y;

            let fill = mask
                .top
                .filter(|b| fy < b.thickness)
                .map(|b| b.bg_color)
                .or_else(|| {
                    mask.bottom
                        .filter(|b| fy + b.thickness >= frame_h && b.thickness <= frame_h)
                        .map(|b| b.bg_color)
                })
                .or_else(|| mask.left.filter(|b| fx < b.thickness).map(|b| b.bg_color))
                .or_else(|| {
                    mask.right
                        .filter(|b| fx + b.thickness >= frame_w && b.thickness <= frame_w)
                        .map(|b| b.bg_color)
                });

            if let Some(color) = fill {
                slice.put_pixel(sx, sy, Rgba(color));
            }
        }
    }
}

impl LinearCanvas {
    pub fn new(first_frame: RgbaImage) -> Self {
        Self {
            image: first_frame,
            axis: None,
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
        mask: Option<&StaticMask>,
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
            }
            ScrollAxis::Horizontal => {
                if frame.height() != self.image.height() {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.image.height(),
                        frame: frame.height(),
                    });
                }
            }
        }

        if slice_px == 0 {
            return Err(CanvasAppendError::EmptyAppend);
        }

        let added = match direction {
            AppendDirection::Bottom => self.append_bottom(frame, slice_px, mask),
            AppendDirection::Top => self.prepend_top(frame, slice_px, mask),
            AppendDirection::Right => self.append_right(frame, slice_px, mask),
            AppendDirection::Left => self.prepend_left(frame, slice_px, mask),
        };

        self.axis = Some(target_axis);
        Ok(added)
    }

    fn append_bottom(
        &mut self,
        frame: &RgbaImage,
        slice_px: u32,
        mask: Option<&StaticMask>,
    ) -> u32 {
        let slice_px = slice_px.min(frame.height());
        let overlap = frame.height() - slice_px;
        let mut slice = frame.view(0, overlap, frame.width(), slice_px).to_image();
        if let Some(mask) = mask {
            apply_static_mask(
                &mut slice,
                frame.width(),
                frame.height(),
                (0, overlap),
                mask,
            );
        }
        let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined
            .copy_from(&slice, 0, self.image.height())
            .expect("copy slice");
        self.image = combined;
        slice_px
    }

    fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32, mask: Option<&StaticMask>) -> u32 {
        let slice_px = slice_px.min(frame.height());
        let mut slice = frame.view(0, 0, frame.width(), slice_px).to_image();
        if let Some(mask) = mask {
            apply_static_mask(&mut slice, frame.width(), frame.height(), (0, 0), mask);
        }
        let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
        combined.copy_from(&slice, 0, 0).expect("copy slice");
        combined
            .copy_from(&self.image, 0, slice_px)
            .expect("copy base");
        self.image = combined;
        slice_px
    }

    fn append_right(&mut self, frame: &RgbaImage, slice_px: u32, mask: Option<&StaticMask>) -> u32 {
        let slice_px = slice_px.min(frame.width());
        let overlap = frame.width() - slice_px;
        let mut slice = frame.view(overlap, 0, slice_px, frame.height()).to_image();
        if let Some(mask) = mask {
            apply_static_mask(
                &mut slice,
                frame.width(),
                frame.height(),
                (overlap, 0),
                mask,
            );
        }
        let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined
            .copy_from(&slice, self.image.width(), 0)
            .expect("copy slice");
        self.image = combined;
        slice_px
    }

    fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32, mask: Option<&StaticMask>) -> u32 {
        let slice_px = slice_px.min(frame.width());
        let mut slice = frame.view(0, 0, slice_px, frame.height()).to_image();
        if let Some(mask) = mask {
            apply_static_mask(&mut slice, frame.width(), frame.height(), (0, 0), mask);
        }
        let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
        combined.copy_from(&slice, 0, 0).expect("copy slice");
        combined
            .copy_from(&self.image, slice_px, 0)
            .expect("copy base");
        self.image = combined;
        slice_px
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::static_region::{StaticMask, StickyBand};
    use image::Rgba;

    fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    #[test]
    fn append_bottom_adds_slice_below() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Bottom, &frame, 2, None)
            .unwrap();
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
        let added = canvas
            .append(AppendDirection::Top, &frame, 3, None)
            .unwrap();
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
        let added = canvas
            .append(AppendDirection::Right, &frame, 2, None)
            .unwrap();
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
        let added = canvas
            .append(AppendDirection::Left, &frame, 3, None)
            .unwrap();
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
        canvas
            .append(AppendDirection::Bottom, &frame, 1, None)
            .unwrap();
        let err = canvas
            .append(AppendDirection::Right, &frame, 1, None)
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
            .append(AppendDirection::Bottom, &frame, 1, None)
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
            .append(AppendDirection::Right, &frame, 1, None)
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
    fn zero_slice_px_is_rejected() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 4, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Bottom, &frame, 0, None)
            .unwrap_err();
        assert_eq!(err, CanvasAppendError::EmptyAppend);
    }

    #[test]
    fn slice_larger_than_frame_is_clamped_to_frame_size() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Bottom, &frame, 99, None)
            .unwrap();
        assert_eq!(added, 4);
        assert_eq!(canvas.height(), 8);
    }

    fn band(thickness: u32, color: [u8; 4]) -> StickyBand {
        StickyBand {
            thickness,
            bg_color: color,
        }
    }

    fn left_only(thickness: u32, color: [u8; 4]) -> StaticMask {
        StaticMask {
            left: Some(band(thickness, color)),
            ..StaticMask::default()
        }
    }

    fn right_only(thickness: u32, color: [u8; 4]) -> StaticMask {
        StaticMask {
            right: Some(band(thickness, color)),
            ..StaticMask::default()
        }
    }

    fn top_only(thickness: u32, color: [u8; 4]) -> StaticMask {
        StaticMask {
            top: Some(band(thickness, color)),
            ..StaticMask::default()
        }
    }

    fn bottom_only(thickness: u32, color: [u8; 4]) -> StaticMask {
        StaticMask {
            bottom: Some(band(thickness, color)),
            ..StaticMask::default()
        }
    }

    #[test]
    fn append_bottom_with_left_mask_fills_left_columns() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let mask = left_only(2, [50, 60, 70, 255]);
        canvas
            .append(AppendDirection::Bottom, &frame, 2, Some(&mask))
            .unwrap();
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(1, 4), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(2, 4), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(7, 5), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn append_bottom_with_right_mask_fills_right_columns() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let mask = right_only(2, [50, 60, 70, 255]);
        canvas
            .append(AppendDirection::Bottom, &frame, 2, Some(&mask))
            .unwrap();
        assert_eq!(canvas.image().get_pixel(7, 4), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(6, 4), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(5, 4), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_with_bottom_mask_fills_bottom_rows_of_slice() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let mask = bottom_only(2, [50, 60, 70, 255]);
        canvas
            .append(AppendDirection::Bottom, &frame, 3, Some(&mask))
            .unwrap();
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([50, 60, 70, 255]));
    }

    #[test]
    fn prepend_top_with_top_mask_fills_top_rows_of_slice() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let mask = top_only(2, [50, 60, 70, 255]);
        canvas
            .append(AppendDirection::Top, &frame, 3, Some(&mask))
            .unwrap();
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(0, 1), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(0, 2), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_right_with_right_mask_fills_right_columns_of_slice() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let mask = right_only(2, [50, 60, 70, 255]);
        canvas
            .append(AppendDirection::Right, &frame, 3, Some(&mask))
            .unwrap();
        assert_eq!(canvas.image().get_pixel(4, 0), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(5, 0), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(6, 0), &Rgba([50, 60, 70, 255]));
    }

    #[test]
    fn prepend_left_with_left_mask_fills_left_columns_of_slice() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let mask = left_only(2, [50, 60, 70, 255]);
        canvas
            .append(AppendDirection::Left, &frame, 3, Some(&mask))
            .unwrap();
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(1, 0), &Rgba([50, 60, 70, 255]));
        assert_eq!(canvas.image().get_pixel(2, 0), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn top_band_overrides_left_band_at_corner() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let mask = StaticMask {
            top: Some(band(1, [1, 2, 3, 255])),
            left: Some(band(1, [9, 9, 9, 255])),
            ..StaticMask::default()
        };
        canvas
            .append(AppendDirection::Top, &frame, 2, Some(&mask))
            .unwrap();
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([1, 2, 3, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([1, 2, 3, 255]));
        assert_eq!(canvas.image().get_pixel(0, 1), &Rgba([9, 9, 9, 255]));
        assert_eq!(canvas.image().get_pixel(1, 1), &Rgba([200, 0, 0, 255]));
    }
}
