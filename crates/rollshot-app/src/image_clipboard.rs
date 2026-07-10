use std::borrow::Cow;

pub(crate) fn image_data(image: &image::RgbaImage) -> arboard::ImageData<'_> {
    arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Borrowed(image.as_raw().as_slice()),
    }
}

pub(crate) fn copy_rgba_image(image: &image::RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("clipboard error: {error}"))?;
    clipboard
        .set_image(image_data(image))
        .map_err(|error| format!("clipboard write error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_data_preserves_dimensions_and_rgba_order() {
        let image = image::RgbaImage::from_raw(
            2,
            1,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
        ).unwrap();

        let data = image_data(&image);

        assert_eq!(data.width, 2);
        assert_eq!(data.height, 1);
        assert_eq!(data.bytes.as_ref(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn image_data_supports_empty_image_without_touching_clipboard() {
        let image = image::RgbaImage::new(0, 0);
        let data = image_data(&image);
        assert_eq!((data.width, data.height), (0, 0));
        assert!(data.bytes.is_empty());
    }
}
