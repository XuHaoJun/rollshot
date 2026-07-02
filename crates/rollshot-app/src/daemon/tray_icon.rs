const NORMAL_TRAY_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tray/generated/runtime/rollshot-tray-normal-32.png"
));

pub(crate) fn normal_tray_rgba() -> Result<(u32, u32, Vec<u8>), String> {
    let image = image::load_from_memory_with_format(NORMAL_TRAY_PNG, image::ImageFormat::Png)
        .map_err(|error| format!("failed to decode embedded Rollshot tray icon: {error}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

#[cfg(target_os = "macos")]
pub(crate) fn normal_tray_icon() -> Result<tray_icon::Icon, String> {
    let (width, height, rgba) = normal_tray_rgba()?;
    tray_icon::Icon::from_rgba(rgba, width, height)
        .map_err(|error| format!("failed to create Rollshot tray icon: {error}"))
}

#[cfg(target_os = "linux")]
pub(crate) fn normal_ksni_icon() -> Result<ksni::Icon, String> {
    let (width, height, mut data) = normal_tray_rgba()?;
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }
    Ok(ksni::Icon {
        width: width as i32,
        height: height as i32,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_tray_png_decodes_with_expected_size() {
        let (width, height, rgba) = normal_tray_rgba().unwrap();
        assert_eq!((width, height), (32, 32));
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn normal_ksni_icon_converts_rgba_to_argb() {
        let (_width, _height, rgba) = normal_tray_rgba().unwrap();
        let icon = normal_ksni_icon().unwrap();
        let rgba_pixel = rgba.chunks_exact(4).find(|pixel| pixel[3] > 0).unwrap();
        let argb_pixel = icon
            .data
            .chunks_exact(4)
            .find(|pixel| pixel[0] > 0)
            .unwrap();
        assert_eq!(argb_pixel, [rgba_pixel[3], rgba_pixel[0], rgba_pixel[1], rgba_pixel[2]]);
    }
}
