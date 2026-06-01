use image::{Rgba, RgbaImage};

use crate::capture_miss::CapturedEdge;

/// Fixed preview width (matches wayscrollshot's PREVIEW_MAX_WIDTH). Keeps the
/// per-frame preview texture small enough to upload stably on the
/// iced_layershell/wgpu path.
pub const PREVIEW_WIDTH: u32 = 280;
/// Cap on the preview height: the preview grows up to this, then follows the
/// bottom of the stitch.
pub const PREVIEW_MAX_HEIGHT: u32 = 480;

/// Black-overlay alpha applied outside the current-frame window (snow-shot uses
/// `rgba(0,0,0,0.32)`). Pixels outside the window keep `1.0 - 0.32` of their RGB.
const SPOTLIGHT_DIM: f32 = 0.32;

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

/// Build the whole-canvas "position spotlight" preview (snow-shot
/// `captuer-edge-mask` parity). The entire stitched `image` is aspect-fit into
/// `max_width` x `max_height`, then every pixel OUTSIDE the current-frame window
/// is darkened. Height-bound previews are letterboxed back to `max_width` so the
/// native overlay's image handle stays a stable width while the stitch grows.
/// The window is the current screenful (`frame_width` x `frame_height`, the crop
/// region size in canvas px) anchored at `edge`; its size along the scroll axis
/// is `frame_extent / canvas_extent` of the preview.
///
/// `edge` selects the scroll axis: `Top`/`Bottom`/`Unknown` => vertical (window
/// spans full width at top/bottom; `Unknown` defaults to bottom), `Left`/`Right`
/// => horizontal (window spans full height at left/right).
///
/// On a miss the canvas does not grow, so the same image (same spotlight) is
/// produced again — the indicator freezes, which is the intended miss signal.
pub fn preview_with_spotlight(
    image: &RgbaImage,
    frame_width: u32,
    frame_height: u32,
    edge: CapturedEdge,
    max_width: u32,
    max_height: u32,
) -> RgbaImage {
    let max_width = max_width.max(1);
    let max_height = max_height.max(1);
    let w = image.width().max(1);
    let h = image.height().max(1);

    // Aspect-preserving fit of the WHOLE canvas into the box.
    let scale = (max_width as f32 / w as f32).min(max_height as f32 / h as f32);
    let out_w = ((w as f32 * scale).round() as u32).max(1);
    let out_h = ((h as f32 * scale).round() as u32).max(1);
    let mut view = if out_w == w && out_h == h {
        image.clone()
    } else {
        image::imageops::resize(image, out_w, out_h, image::imageops::FilterType::Triangle)
    };

    let vertical = !matches!(edge, CapturedEdge::Left | CapturedEdge::Right);
    let (canvas_extent, frame_extent, out_extent) = if vertical {
        (h, frame_height, out_h)
    } else {
        (w, frame_width, out_w)
    };

    // Window length along the scroll axis, in preview px. fraction >= 1 (e.g.
    // the first frame) means the window covers everything: no dimming.
    let fraction = (frame_extent as f32 / canvas_extent as f32).clamp(0.0, 1.0);
    let window_len = ((out_extent as f32 * fraction).round() as u32).clamp(1, out_extent);

    if window_len < out_extent {
        // [win_start, win_end) is the bright window; everything else is dimmed.
        let at_far_edge = matches!(
            edge,
            CapturedEdge::Bottom | CapturedEdge::Right | CapturedEdge::Unknown
        );
        let win_start = if at_far_edge {
            out_extent - window_len
        } else {
            0
        };
        let win_end = win_start + window_len;

        let keep = 1.0 - SPOTLIGHT_DIM;
        let dim = |c: u8| (c as f32 * keep).round() as u8;
        for y in 0..out_h {
            for x in 0..out_w {
                let pos = if vertical { y } else { x };
                if pos >= win_start && pos < win_end {
                    continue;
                }
                let p = view.get_pixel_mut(x, y);
                p.0[0] = dim(p.0[0]);
                p.0[1] = dim(p.0[1]);
                p.0[2] = dim(p.0[2]);
            }
        }
    }
    if out_w >= max_width {
        return view;
    }

    let x_offset = (max_width - out_w) / 2;
    let mut boxed = RgbaImage::from_pixel(max_width, out_h, Rgba([255, 255, 255, 255]));
    for y in 0..out_h {
        for x in 0..out_w {
            boxed.put_pixel(x + x_offset, y, *view.get_pixel(x, y));
        }
    }
    boxed
}

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
    use super::{preview_with_spotlight, viewport_preview, ViewportPreviewRequest};
    use crate::capture_miss::CapturedEdge;
    use image::{Rgba, RgbaImage};

    #[test]
    fn spotlight_keeps_fixed_width_for_tall_canvas() {
        // 100x400 canvas, box 280x480: height-bound fit, scale = 1.2.
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Bottom, 280, 480);
        assert_eq!((view.width(), view.height()), (280, 480));
    }

    #[test]
    fn spotlight_dims_outside_window_and_keeps_window_bright() {
        // region height 100 of a 400-tall canvas => 1/4 window at the bottom.
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Bottom, 280, 480);
        let content_x = view.width() / 2;
        // Bottom 1/4 (rows >= 360) is the current-frame window: full brightness.
        assert_eq!(view.get_pixel(content_x, 470).0, [255, 255, 255, 255]);
        // Above the window is dimmed to 0.68 (255 * 0.68 -> 173), alpha intact.
        assert_eq!(view.get_pixel(content_x, 10).0, [173, 173, 173, 255]);
    }

    #[test]
    fn spotlight_first_frame_is_not_dimmed() {
        // region == whole canvas (fraction 1.0): nothing is dimmed.
        let canvas = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Bottom, 280, 480);
        assert_eq!(view.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(view.get_pixel(0, view.height() - 1).0, [255, 255, 255, 255]);
    }

    #[test]
    fn spotlight_top_edge_window_sits_at_top() {
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Top, 280, 480);
        let content_x = view.width() / 2;
        // Top 1/4 bright, bottom dimmed.
        assert_eq!(view.get_pixel(content_x, 10).0, [255, 255, 255, 255]);
        assert_eq!(view.get_pixel(content_x, 470).0, [173, 173, 173, 255]);
    }

    #[test]
    fn spotlight_unknown_edge_defaults_to_bottom() {
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Unknown, 280, 480);
        let content_x = view.width() / 2;
        assert_eq!(view.get_pixel(content_x, 470).0, [255, 255, 255, 255]);
        assert_eq!(view.get_pixel(content_x, 10).0, [173, 173, 173, 255]);
    }

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
