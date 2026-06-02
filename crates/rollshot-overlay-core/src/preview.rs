use image::{imageops, Rgba, RgbaImage};

use crate::capture_miss::CapturedEdge;

/// Fixed preview width (matches wayscrollshot's PREVIEW_MAX_WIDTH). Keeps the
/// per-frame preview texture small enough to upload stably on the
/// iced_layershell/wgpu path.
pub const PREVIEW_WIDTH: u32 = 280;
/// Cap on the preview height: the preview grows up to this, then follows the
/// bottom of the stitch.
pub const PREVIEW_MAX_HEIGHT: u32 = 480;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportPreviewRequest {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub edge: CapturedEdge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportPreview {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub total_width: u32,
    pub total_height: u32,
    pub viewport_x: u32,
    pub viewport_y: u32,
    pub viewport_width_in_canvas: u32,
    pub viewport_height_in_canvas: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrowingPreviewRequest {
    pub fixed_width: u32,
    pub max_height: u32,
    pub edge: CapturedEdge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowingPreview {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub scaled_full_height: u32,
    pub total_width: u32,
    pub total_height: u32,
}

/// Build a wayscrollshot-style live preview by scaling the full stitched image
/// to a fixed width, then cropping vertically to the requested height cap.
pub fn growing_preview(
    stitcher: &mut rollshot_core::Stitcher,
    request: GrowingPreviewRequest,
) -> Option<GrowingPreview> {
    let full = stitcher.full_image()?;
    let total_width = full.width().max(1);
    let total_height = full.height().max(1);
    let target_width = request.fixed_width.max(1);
    let max_height = request.max_height.max(1);
    let scale = target_width as f32 / total_width as f32;
    let scaled_height = ((total_height as f32 * scale).round() as u32).max(1);

    let resized = imageops::resize(
        full,
        target_width,
        scaled_height,
        imageops::FilterType::Triangle,
    );
    let out_height = scaled_height.min(max_height);
    let y = if scaled_height <= out_height || matches!(request.edge, CapturedEdge::Top) {
        0
    } else {
        scaled_height - out_height
    };

    let cropped = imageops::crop_imm(&resized, 0, y, target_width, out_height).to_image();

    Some(GrowingPreview {
        width: cropped.width(),
        height: cropped.height(),
        pixels: cropped.into_raw(),
        scaled_full_height: scaled_height,
        total_width,
        total_height,
    })
}

/// Build the viewport preview from the current `Stitcher` state.
///
/// The current-frame window is `frame_width` x `frame_height` in canvas
/// pixels, anchored at `edge` (`Top`/`Bottom`/`Unknown` => vertical scroll,
/// `Left`/`Right` => horizontal). The resulting `ViewportPreview` is the
/// windowed canvas aspect-fit into `viewport_width` x `viewport_height`,
/// letterboxed on a white background, with a small scrollbar-style position
/// indicator drawn along the far edge.
pub fn viewport_preview(
    stitcher: &mut rollshot_core::Stitcher,
    request: ViewportPreviewRequest,
) -> Option<ViewportPreview> {
    let stats = stitcher.stats();
    let total_width = stats.total_width.max(1);
    let total_height = stats.total_height.max(1);
    let vertical = !matches!(request.edge, CapturedEdge::Left | CapturedEdge::Right);

    let crop_width = if vertical {
        total_width
    } else {
        request.frame_width.min(total_width).max(1)
    };
    let crop_height = if vertical {
        request.frame_height.min(total_height).max(1)
    } else {
        total_height
    };

    let x = match request.edge {
        CapturedEdge::Right => total_width.saturating_sub(crop_width),
        CapturedEdge::Left => 0,
        _ => 0,
    };
    let y = match request.edge {
        CapturedEdge::Top => 0,
        CapturedEdge::Bottom | CapturedEdge::Unknown => total_height.saturating_sub(crop_height),
        CapturedEdge::Left | CapturedEdge::Right => 0,
    };

    let canvas = stitcher.canvas_viewport(x, y, crop_width, crop_height)?;
    let target_width = request.viewport_width.max(1);
    let target_height = request.viewport_height.max(1);
    let scale = (target_width as f32 / canvas.image.width().max(1) as f32)
        .min(target_height as f32 / canvas.image.height().max(1) as f32);
    let out_w = ((canvas.image.width() as f32 * scale).round() as u32).clamp(1, target_width);
    let out_h = ((canvas.image.height() as f32 * scale).round() as u32).clamp(1, target_height);
    let resized = image::imageops::resize(
        &canvas.image,
        out_w,
        out_h,
        image::imageops::FilterType::Triangle,
    );

    let mut boxed = RgbaImage::from_pixel(target_width, target_height, Rgba([255, 255, 255, 255]));
    let offset_x = (target_width - out_w) / 2;
    let offset_y = (target_height - out_h) / 2;
    for py in 0..out_h {
        for px in 0..out_w {
            boxed.put_pixel(offset_x + px, offset_y + py, *resized.get_pixel(px, py));
        }
    }

    draw_position_indicator(
        &mut boxed,
        vertical,
        canvas.x,
        canvas.y,
        canvas.image.width(),
        canvas.image.height(),
        canvas.total_width,
        canvas.total_height,
    );

    Some(ViewportPreview {
        width: boxed.width(),
        height: boxed.height(),
        pixels: boxed.into_raw(),
        total_width: canvas.total_width,
        total_height: canvas.total_height,
        viewport_x: canvas.x,
        viewport_y: canvas.y,
        viewport_width_in_canvas: canvas.image.width(),
        viewport_height_in_canvas: canvas.image.height(),
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_position_indicator(
    image: &mut RgbaImage,
    vertical: bool,
    viewport_x: u32,
    viewport_y: u32,
    viewport_w: u32,
    viewport_h: u32,
    total_w: u32,
    total_h: u32,
) {
    const TRACK: Rgba<u8> = Rgba([15, 23, 42, 128]);
    const THUMB: Rgba<u8> = Rgba([56, 189, 248, 230]);
    const MIN_THUMB: u32 = 8;
    let w = image.width();
    let h = image.height();

    if vertical {
        let x0 = w.saturating_sub(4);
        for y in 0..h {
            for x in x0..w {
                image.put_pixel(x, y, TRACK);
            }
        }
        let ratio = viewport_h as f32 / total_h.max(1) as f32;
        let thumb_len = ((h as f32 * ratio).round() as u32).clamp(MIN_THUMB.min(h), h);
        let max_start = h.saturating_sub(thumb_len);
        let start_ratio = viewport_y as f32 / total_h.saturating_sub(viewport_h).max(1) as f32;
        let start = ((max_start as f32 * start_ratio).round() as u32).min(max_start);
        for y in start..start + thumb_len {
            for x in x0..w {
                image.put_pixel(x, y, THUMB);
            }
        }
    } else {
        let y0 = h.saturating_sub(4);
        for y in y0..h {
            for x in 0..w {
                image.put_pixel(x, y, TRACK);
            }
        }
        let ratio = viewport_w as f32 / total_w.max(1) as f32;
        let thumb_len = ((w as f32 * ratio).round() as u32).clamp(MIN_THUMB.min(w), w);
        let max_start = w.saturating_sub(thumb_len);
        let start_ratio = viewport_x as f32 / total_w.saturating_sub(viewport_w).max(1) as f32;
        let start = ((max_start as f32 * start_ratio).round() as u32).min(max_start);
        for y in y0..h {
            for x in start..start + thumb_len {
                image.put_pixel(x, y, THUMB);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{growing_preview, viewport_preview, GrowingPreviewRequest, ViewportPreviewRequest};
    use crate::capture_miss::CapturedEdge;

    fn numbered_rows(width: u32, height: u32) -> image::RgbaImage {
        let mut image = image::RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                image.put_pixel(x, y, image::Rgba([(y % 251) as u8, 80, 120, 255]));
            }
        }
        image
    }

    fn numbered_cols(width: u32, height: u32) -> image::RgbaImage {
        let mut image = image::RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                image.put_pixel(x, y, image::Rgba([(x % 251) as u8, 80, 120, 255]));
            }
        }
        image
    }

    #[test]
    fn growing_preview_scales_full_image_to_fixed_width() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_rows(20, 40)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = growing_preview(
            &mut stitcher,
            GrowingPreviewRequest {
                fixed_width: 10,
                max_height: 100,
                edge: CapturedEdge::Bottom,
            },
        )
        .expect("growing preview");

        assert_eq!((preview.width, preview.height), (10, 20));
        assert_eq!(preview.scaled_full_height, 20);
        assert_eq!((preview.total_width, preview.total_height), (20, 40));
    }

    #[test]
    fn growing_preview_caps_height_and_returns_bottom_slice() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_rows(10, 60)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = growing_preview(
            &mut stitcher,
            GrowingPreviewRequest {
                fixed_width: 10,
                max_height: 20,
                edge: CapturedEdge::Bottom,
            },
        )
        .expect("growing preview");

        assert_eq!((preview.width, preview.height), (10, 20));
        assert_eq!(preview.scaled_full_height, 60);
        assert_eq!(
            preview.pixels[0], 40,
            "bottom slice starts at scaled row 40"
        );
    }

    #[test]
    fn growing_preview_top_edge_returns_top_slice() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_rows(10, 60)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = growing_preview(
            &mut stitcher,
            GrowingPreviewRequest {
                fixed_width: 10,
                max_height: 20,
                edge: CapturedEdge::Top,
            },
        )
        .expect("growing preview");

        assert_eq!((preview.width, preview.height), (10, 20));
        assert_eq!(preview.pixels[0], 0, "top slice starts at scaled row 0");
    }

    #[test]
    fn growing_preview_clamps_zero_requested_size_to_one_pixel() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_rows(20, 20)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = growing_preview(
            &mut stitcher,
            GrowingPreviewRequest {
                fixed_width: 0,
                max_height: 0,
                edge: CapturedEdge::Bottom,
            },
        )
        .expect("growing preview");

        assert_eq!((preview.width, preview.height), (1, 1));
    }

    #[test]
    fn viewport_preview_bottom_edge_shows_bottom_rows() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_rows(20, 120)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = viewport_preview(
            &mut stitcher,
            ViewportPreviewRequest {
                viewport_width: 100,
                viewport_height: 80,
                frame_width: 20,
                frame_height: 40,
                edge: CapturedEdge::Bottom,
            },
        )
        .expect("preview");

        assert_eq!((preview.width, preview.height), (100, 80));
        assert_eq!((preview.viewport_x, preview.viewport_y), (0, 80));
        assert_eq!(
            (
                preview.viewport_width_in_canvas,
                preview.viewport_height_in_canvas
            ),
            (20, 40)
        );
    }

    #[test]
    fn viewport_preview_top_edge_shows_top_rows() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_rows(20, 120)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = viewport_preview(
            &mut stitcher,
            ViewportPreviewRequest {
                viewport_width: 100,
                viewport_height: 80,
                frame_width: 20,
                frame_height: 40,
                edge: CapturedEdge::Top,
            },
        )
        .expect("preview");

        assert_eq!((preview.viewport_x, preview.viewport_y), (0, 0));
        assert_eq!(
            (
                preview.viewport_width_in_canvas,
                preview.viewport_height_in_canvas
            ),
            (20, 40)
        );
    }

    #[test]
    fn viewport_preview_right_edge_shows_right_columns() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_cols(120, 20)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = viewport_preview(
            &mut stitcher,
            ViewportPreviewRequest {
                viewport_width: 100,
                viewport_height: 80,
                frame_width: 40,
                frame_height: 20,
                edge: CapturedEdge::Right,
            },
        )
        .expect("preview");

        assert_eq!((preview.viewport_x, preview.viewport_y), (80, 0));
        assert_eq!(
            (
                preview.viewport_width_in_canvas,
                preview.viewport_height_in_canvas
            ),
            (40, 20)
        );
    }

    #[test]
    fn viewport_preview_clamps_zero_requested_size_to_one_pixel() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
        assert_eq!(
            stitcher.push_frame(numbered_rows(20, 20)),
            rollshot_core::StitchOutcome::FirstFrame
        );

        let preview = viewport_preview(
            &mut stitcher,
            ViewportPreviewRequest {
                viewport_width: 0,
                viewport_height: 0,
                frame_width: 20,
                frame_height: 20,
                edge: CapturedEdge::Bottom,
            },
        )
        .expect("preview");

        assert_eq!((preview.width, preview.height), (1, 1));
    }

    #[test]
    fn viewport_preview_returns_none_before_first_frame() {
        let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());

        assert!(viewport_preview(
            &mut stitcher,
            ViewportPreviewRequest {
                viewport_width: 100,
                viewport_height: 80,
                frame_width: 20,
                frame_height: 40,
                edge: CapturedEdge::Bottom,
            },
        )
        .is_none());
    }
}
