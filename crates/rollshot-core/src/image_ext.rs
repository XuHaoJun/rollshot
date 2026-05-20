use image::{GenericImage, GenericImageView, RgbaImage};

/// Returns a new image with the bottom `dy` rows of `frame` stacked under `base`.
///
/// `base` and `frame` must share the same width. When `dy` is 0 the function
/// returns a clone of `base`. When `dy` is larger than `frame.height()` the full
/// frame is appended.
pub fn append_below(base: &RgbaImage, frame: &RgbaImage, dy: u32) -> RgbaImage {
    assert_eq!(
        base.width(),
        frame.width(),
        "append_below requires equal widths"
    );

    if dy == 0 {
        return base.clone();
    }

    let dy = dy.min(frame.height());
    let mut combined = RgbaImage::new(base.width(), base.height() + dy);
    combined
        .copy_from(base, 0, 0)
        .expect("copy base into combined");

    let overlap = frame.height() - dy;
    let slice = frame.view(0, overlap, frame.width(), dy).to_image();
    combined
        .copy_from(&slice, 0, base.height())
        .expect("copy slice into combined");

    combined
}

#[cfg(test)]
mod tests {
    use super::append_below;
    use image::{Rgba, RgbaImage};

    fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    #[test]
    fn dy_zero_returns_clone_of_base() {
        let base = solid(4, 3, [10, 20, 30, 255]);
        let frame = solid(4, 3, [40, 50, 60, 255]);

        let combined = append_below(&base, &frame, 0);

        assert_eq!(combined.dimensions(), (4, 3));
        assert_eq!(combined.get_pixel(0, 0), &Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn appends_bottom_rows_of_frame() {
        let base = solid(2, 2, [10, 10, 10, 255]);
        let mut frame = solid(2, 4, [0, 0, 0, 255]);
        frame.put_pixel(0, 2, Rgba([1, 1, 1, 255]));
        frame.put_pixel(1, 2, Rgba([2, 2, 2, 255]));
        frame.put_pixel(0, 3, Rgba([3, 3, 3, 255]));
        frame.put_pixel(1, 3, Rgba([4, 4, 4, 255]));

        let combined = append_below(&base, &frame, 2);

        assert_eq!(combined.dimensions(), (2, 4));
        assert_eq!(combined.get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(combined.get_pixel(0, 2), &Rgba([1, 1, 1, 255]));
        assert_eq!(combined.get_pixel(1, 3), &Rgba([4, 4, 4, 255]));
    }

    #[test]
    fn dy_larger_than_frame_height_appends_full_frame() {
        let base = solid(2, 1, [10, 10, 10, 255]);
        let frame = solid(2, 2, [7, 7, 7, 255]);

        let combined = append_below(&base, &frame, 999);

        assert_eq!(combined.dimensions(), (2, 3));
        assert_eq!(combined.get_pixel(0, 1), &Rgba([7, 7, 7, 255]));
        assert_eq!(combined.get_pixel(1, 2), &Rgba([7, 7, 7, 255]));
    }
}
