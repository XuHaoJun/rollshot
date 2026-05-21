//! Single-axis stitched canvas that can grow in four directions.

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
            AppendDirection::Bottom => self.append_bottom(frame, slice_px),
            AppendDirection::Top => self.prepend_top(frame, slice_px),
            AppendDirection::Right => self.append_right(frame, slice_px),
            AppendDirection::Left => self.prepend_left(frame, slice_px),
        };

        self.axis = Some(target_axis);
        Ok(added)
    }

    fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.height());
        let overlap = frame.height() - slice_px;
        let slice = frame
            .view(0, overlap, frame.width(), slice_px)
            .to_image();
        let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined
            .copy_from(&slice, 0, self.image.height())
            .expect("copy slice");
        self.image = combined;
        slice_px
    }

    fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.height());
        let slice = frame.view(0, 0, frame.width(), slice_px).to_image();
        let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
        combined.copy_from(&slice, 0, 0).expect("copy slice");
        combined
            .copy_from(&self.image, 0, slice_px)
            .expect("copy base");
        self.image = combined;
        slice_px
    }

    fn append_right(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.width());
        let overlap = frame.width() - slice_px;
        let slice = frame
            .view(overlap, 0, slice_px, frame.height())
            .to_image();
        let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined
            .copy_from(&slice, self.image.width(), 0)
            .expect("copy slice");
        self.image = combined;
        slice_px
    }

    fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.width());
        let slice = frame.view(0, 0, slice_px, frame.height()).to_image();
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
}
