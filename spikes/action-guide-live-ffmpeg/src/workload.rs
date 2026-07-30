use crate::RunConfig;
use image::RgbaImage;

/// Render a deterministic desktop-like frame for the given frame index.
///
/// The scene contains a dark desktop background, two contrasting window panels,
/// a vertically scrolling list of text-like bars, a changing chart region, and
/// a moving cursor. Every position and color is derived from `frame_index`;
/// no random numbers are used. All pixels have alpha = 255.
pub(crate) fn render_frame(cfg: &RunConfig, frame_index: u64) -> RgbaImage {
    let w = cfg.width;
    let h = cfg.height;
    let mut img = RgbaImage::from_pixel(w, h, image::Rgba([28, 28, 30, 255]));

    // Panel 1: left sidebar (darker)
    let sidebar_w = w / 5;
    for y in 0..h {
        for x in 0..sidebar_w {
            img.put_pixel(x, y, image::Rgba([45, 45, 48, 255]));
        }
    }

    // Sidebar text-like bars (scrolling vertically with frame_index)
    let bar_count = (h as u64) / 16;
    for i in 0..bar_count {
        let base_y = ((i * 16 + frame_index % 16) % h as u64) as u32;
        let bar_w = (sidebar_w * 3) / 4;
        let offset_x = (sidebar_w - bar_w) / 2;
        let shade = ((i * 37 + frame_index / 30 * 5) % 80 + 60) as u8;
        for y in base_y..(base_y + 8).min(h) {
            for x in offset_x..(offset_x + bar_w).min(sidebar_w) {
                img.put_pixel(x, y, image::Rgba([shade, shade, shade + 20, 255]));
            }
        }
    }

    // Panel 2: main content area with chart
    let content_x = sidebar_w + 8;
    let content_w = w - content_x - 8;
    let content_h = h - 60;
    for y in 20..(20 + content_h).min(h) {
        for x in content_x..(content_x + content_w).min(w) {
            img.put_pixel(x, y, image::Rgba([36, 36, 40, 255]));
        }
    }

    // Chart region: bar chart whose heights change with frame_index
    let chart_x = content_x + 16;
    let chart_bottom = 20 + content_h - 10;
    let chart_top = 20 + 40;
    let bar_count_chart = (content_w / 18).min(30);
    let chart_height = chart_bottom - chart_top;
    for i in 0..bar_count_chart {
        let bx = chart_x + i * 18;
        let fraction = ((i as u64 * 7 + frame_index) % 100) as f32 / 100.0;
        let bar_h = (chart_height as f32 * fraction) as u32;
        let r = (100u32 + (i as u32) * 13) as u8;
        let g = (140u64 + (frame_index * 7 + i as u64) % 116) as u8;
        let b = (200u32).wrapping_sub((i as u32).wrapping_mul(5)) as u8;
        for y in (chart_bottom - bar_h)..chart_bottom {
            for x in bx..(bx + 12).min(w) {
                img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
            }
        }
    }

    // Moving cursor: a small bright rectangle that moves diagonally
    let cursor_x = ((frame_index * 3) % w as u64) as u32;
    let cursor_y = ((frame_index * 2) % h as u64) as u32;
    let cursor_size: u32 = 6;
    for dy in 0..cursor_size {
        for dx in 0..cursor_size {
            let px = cursor_x + dx;
            let py = cursor_y + dy;
            if px < w && py < h {
                img.put_pixel(px, py, image::Rgba([255, 255, 255, 255]));
            }
        }
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RunConfig {
        RunConfig::for_test(320, 180, 30)
    }

    #[test]
    fn frame_dimensions_and_alpha_are_stable() {
        let frame = render_frame(&config(), 17);
        assert_eq!(frame.dimensions(), (320, 180));
        assert!(frame.pixels().all(|pixel| pixel.0[3] == 255));
    }

    #[test]
    fn workload_is_deterministic_but_changes_each_second() {
        let cfg = config();
        let first = render_frame(&cfg, 0);
        assert_eq!(first, render_frame(&cfg, 0));
        assert_ne!(first, render_frame(&cfg, cfg.fps as u64));
    }

    #[test]
    fn workload_changes_more_than_a_cursor_patch() {
        let cfg = config();
        let first = render_frame(&cfg, 0);
        let next = render_frame(&cfg, cfg.fps as u64);
        let changed = first
            .pixels()
            .zip(next.pixels())
            .filter(|(left, right)| left != right)
            .count();
        assert!(changed > (cfg.width as usize * cfg.height as usize) / 20);
    }
}
