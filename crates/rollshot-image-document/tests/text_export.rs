use image::{Rgba, RgbaImage};
use rollshot_image_document::{draw_text_block, ImagePoint, Rgba8};

#[test]
fn public_text_draw_api_paints_pixels() {
    let mut image = RgbaImage::from_pixel(160, 60, Rgba([255, 255, 255, 255]));

    draw_text_block(
        &mut image,
        ImagePoint::new(8.0, 8.0),
        "Step 1 - Click",
        20.0,
        true,
        Rgba8::new(20, 24, 31, 255),
    );

    let changed = image
        .pixels()
        .filter(|pixel| pixel.0 != [255, 255, 255, 255])
        .count();
    assert!(changed > 20, "expected glyph pixels, got {changed}");
}
