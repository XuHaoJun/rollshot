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

#[cfg(test)]
mod tests {
    use super::preview_with_spotlight;
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
}
