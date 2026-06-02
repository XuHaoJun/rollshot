//! Regression: a stale/bad FIRST frame must not permanently block a capture.
//!
//! Repro of the reported bug: when the crop region's first frame still shows a
//! lazy-loaded placeholder where a product image will be (the image hadn't
//! painted yet when the first frame was grabbed), every later frame has the
//! image loaded. The first frame's overlap content therefore disagrees with
//! every subsequent frame, the pixel verifier vetoes the (correct) motion, and
//! because the stitcher only advances its anchor on a successful append, the
//! capture stays frozen on the stale first frame forever ("scrolling too fast"
//! on every frame). The stitcher must re-anchor off the bad first frame and
//! stitch the loaded content forward.

mod common;

use common::crop_frame;
use image::{Rgba, RgbaImage};
use rollshot_core::{AppendDirection, StitchConfig, StitchOutcome, Stitcher};

const W: u32 = 720;
const CANVAS_H: u32 = 2600;
const FRAME_H: u32 = 600;
const IMG_Y0: u32 = 480;
const IMG_Y1: u32 = 1180;
const STEP: u32 = 160;

/// A tall page: white background, richly textured text-like rows everywhere,
/// plus one large product-image block. `image_loaded` toggles whether that
/// block is the real (textured) photo or a flat lazy-load placeholder.
fn product_page(image_loaded: bool) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(W, CANVAS_H, Rgba([250, 250, 250, 255]));

    for y in 0..CANVAS_H {
        if (IMG_Y0..IMG_Y1).contains(&y) {
            continue; // image region painted below
        }
        let line = (y / 22) % 4;
        if line == 0 {
            for x in 30..W - 30 {
                if (x / 6 + y / 3) % 2 == 0 {
                    img.put_pixel(x, y, Rgba([40, 40, 40, 255]));
                }
            }
        } else if line == 1 && y % 22 < 3 {
            for x in 40..W - 120 {
                img.put_pixel(x, y, Rgba([70, 90, 160, 255]));
            }
        }
    }

    for y in IMG_Y0..IMG_Y1 {
        for x in 24..W - 24 {
            let px = if image_loaded {
                let r = (60 + ((x * 2 + y) % 160)) as u8;
                let g = (40 + ((x + y * 3) % 180)) as u8;
                let b = (90 + ((x * 3 + y * 2) % 150)) as u8;
                Rgba([r, g, b, 255])
            } else {
                Rgba([225, 225, 225, 255])
            };
            img.put_pixel(x, y, px);
        }
    }

    img
}

#[test]
fn stale_first_frame_recovers_instead_of_sticking() {
    let loaded = product_page(true);
    let placeholder = product_page(false);

    let mut stitcher = Stitcher::new(StitchConfig::default());

    // First frame: product image still a placeholder, partial at the bottom.
    assert_eq!(
        stitcher.push_frame(crop_frame(&placeholder, 0, FRAME_H)),
        StitchOutcome::FirstFrame
    );

    // Subsequent frames have the image loaded.
    let mut good_bottom_appends = 0;
    for i in 1..8 {
        let frame = crop_frame(&loaded, i * STEP, FRAME_H);
        if let StitchOutcome::Appended {
            direction: AppendDirection::Bottom,
            estimate,
            ..
        } = stitcher.push_frame(frame)
        {
            if (120..=200).contains(&estimate.dy) {
                good_bottom_appends += 1;
            }
        }
    }

    assert!(
        good_bottom_appends >= 2,
        "capture stayed stuck on the stale first frame: expected it to re-anchor \
         and stitch the loaded content forward, got {good_bottom_appends} correct \
         Bottom appends"
    );
}
