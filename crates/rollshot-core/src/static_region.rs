use image::{Rgba, RgbaImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StickyBand {
    pub thickness: u32,
    pub bg_color: [u8; 4],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticMask {
    pub top: Option<StickyBand>,
    pub bottom: Option<StickyBand>,
    pub left: Option<StickyBand>,
    pub right: Option<StickyBand>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StaticRegionConfig {
    pub enabled: bool,
    pub min_observations: usize,
    pub static_mad_threshold: f32,
    pub motion_margin: f32,
    pub max_band_ratio: f32,
}

impl Default for StaticRegionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_observations: 3,
            static_mad_threshold: 4.0 / 255.0,
            motion_margin: 4.0 / 255.0,
            max_band_ratio: 0.30,
        }
    }
}

fn pixel_gray(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let Rgba([r, g, b, _]) = *img.get_pixel(x, y);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

pub(super) fn compute_row_static(prev: &RgbaImage, curr: &RgbaImage) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width();
    let h = prev.height();
    let mut out = vec![0.0; h as usize];
    for y in 0..h {
        let mut sum = 0.0;
        for x in 0..w {
            sum += (pixel_gray(prev, x, y) - pixel_gray(curr, x, y)).abs();
        }
        out[y as usize] = sum / (w as f32 * 255.0);
    }
    out
}

pub(super) fn compute_col_static(prev: &RgbaImage, curr: &RgbaImage) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width();
    let h = prev.height();
    let mut out = vec![0.0; w as usize];
    for x in 0..w {
        let mut sum = 0.0;
        for y in 0..h {
            sum += (pixel_gray(prev, x, y) - pixel_gray(curr, x, y)).abs();
        }
        out[x as usize] = sum / (h as f32 * 255.0);
    }
    out
}

pub(super) fn compute_row_motion(prev: &RgbaImage, curr: &RgbaImage, dx: i32, dy: i32) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width() as i32;
    let h = prev.height() as i32;
    let mut out = vec![f32::NAN; h as usize];
    for y in 0..h {
        let py = y + dy;
        if py < 0 || py >= h {
            continue;
        }
        let mut sum = 0.0;
        let mut count = 0u32;
        for x in 0..w {
            let px = x + dx;
            if px < 0 || px >= w {
                continue;
            }
            sum += (pixel_gray(prev, px as u32, py as u32) - pixel_gray(curr, x as u32, y as u32))
                .abs();
            count += 1;
        }
        out[y as usize] = if count == 0 {
            f32::NAN
        } else {
            sum / (count as f32 * 255.0)
        };
    }
    out
}

pub(super) fn compute_col_motion(prev: &RgbaImage, curr: &RgbaImage, dx: i32, dy: i32) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width() as i32;
    let h = prev.height() as i32;
    let mut out = vec![f32::NAN; w as usize];
    for x in 0..w {
        let px = x + dx;
        if px < 0 || px >= w {
            continue;
        }
        let mut sum = 0.0;
        let mut count = 0u32;
        for y in 0..h {
            let py = y + dy;
            if py < 0 || py >= h {
                continue;
            }
            sum += (pixel_gray(prev, px as u32, py as u32) - pixel_gray(curr, x as u32, y as u32))
                .abs();
            count += 1;
        }
        out[x as usize] = if count == 0 {
            f32::NAN
        } else {
            sum / (count as f32 * 255.0)
        };
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EdgeExtents {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

fn is_static_line(static_score: f32, motion_score: f32, cfg: &StaticRegionConfig) -> bool {
    if !static_score.is_finite() {
        return false;
    }
    if static_score >= cfg.static_mad_threshold {
        return false;
    }
    if !motion_score.is_finite() {
        return static_score < cfg.static_mad_threshold / 4.0;
    }
    if motion_score < cfg.static_mad_threshold / 4.0 {
        return true;
    }
    (motion_score - static_score) > cfg.motion_margin
}

fn scan_from_start(static_scores: &[f32], motion_scores: &[f32], cfg: &StaticRegionConfig) -> u32 {
    let mut extent = 0u32;
    for i in 0..static_scores.len() {
        if is_static_line(static_scores[i], motion_scores[i], cfg) {
            extent += 1;
        } else {
            break;
        }
    }
    extent
}

fn scan_from_end(static_scores: &[f32], motion_scores: &[f32], cfg: &StaticRegionConfig) -> u32 {
    let mut extent = 0u32;
    for i in (0..static_scores.len()).rev() {
        if is_static_line(static_scores[i], motion_scores[i], cfg) {
            extent += 1;
        } else {
            break;
        }
    }
    extent
}

pub(super) fn scan_edges(
    row_static: &[f32],
    row_motion: &[f32],
    col_static: &[f32],
    col_motion: &[f32],
    cfg: &StaticRegionConfig,
) -> EdgeExtents {
    let h = row_static.len() as u32;
    let w = col_static.len() as u32;
    let mut top = scan_from_start(row_static, row_motion, cfg);
    let mut bottom = scan_from_end(row_static, row_motion, cfg);
    let mut left = scan_from_start(col_static, col_motion, cfg);
    let mut right = scan_from_end(col_static, col_motion, cfg);

    let max_row = (h as f32 * cfg.max_band_ratio) as u32;
    let max_col = (w as f32 * cfg.max_band_ratio) as u32;
    if top > max_row {
        top = 0;
    }
    if bottom > max_row {
        bottom = 0;
    }
    if left > max_col {
        left = 0;
    }
    if right > max_col {
        right = 0;
    }

    EdgeExtents {
        top,
        bottom,
        left,
        right,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

pub(super) fn median_u8(mut v: Vec<u8>) -> u8 {
    v.sort_unstable();
    v[v.len() / 2]
}

pub(super) fn sample_band_bg_color(img: &RgbaImage, edge: Edge, thickness: u32) -> Option<[u8; 4]> {
    if thickness == 0 {
        return None;
    }
    let w = img.width();
    let h = img.height();
    let (x0, y0, sw, sh) = match edge {
        Edge::Top => (0, thickness.saturating_sub(1), w, 1.min(thickness)),
        Edge::Bottom => (0, h.saturating_sub(thickness), w, 1.min(thickness)),
        Edge::Left => (thickness.saturating_sub(1), 0, 1.min(thickness), h),
        Edge::Right => (w.saturating_sub(thickness), 0, 1.min(thickness), h),
    };
    let mut rs = Vec::new();
    let mut gs = Vec::new();
    let mut bs = Vec::new();
    let mut as_ = Vec::new();
    for y in y0..(y0 + sh).min(h) {
        for x in x0..(x0 + sw).min(w) {
            let Rgba([r, g, b, a]) = *img.get_pixel(x, y);
            rs.push(r);
            gs.push(g);
            bs.push(b);
            as_.push(a);
        }
    }
    if rs.is_empty() {
        return None;
    }
    Some([median_u8(rs), median_u8(gs), median_u8(bs), median_u8(as_)])
}

#[derive(Debug, Clone, Copy)]
struct BandObs {
    thickness: u32,
    color: [u8; 4],
}

#[derive(Debug, Clone, Copy)]
struct EdgeObservation {
    top: BandObs,
    bottom: BandObs,
    left: BandObs,
    right: BandObs,
}

pub(crate) struct StaticRegionDetector {
    config: StaticRegionConfig,
    observations: Vec<EdgeObservation>,
    locked: Option<StaticMask>,
}

impl StaticRegionDetector {
    pub(crate) fn new(config: StaticRegionConfig) -> Self {
        Self {
            config,
            observations: Vec::new(),
            locked: None,
        }
    }

    pub(crate) fn observe(&mut self, prev: &RgbaImage, curr: &RgbaImage, dx: i32, dy: i32) {
        if self.locked.is_some() {
            return;
        }
        if prev.dimensions() != curr.dimensions() {
            return;
        }

        let row_static = compute_row_static(prev, curr);
        let row_motion = compute_row_motion(prev, curr, dx, dy);
        let col_static = compute_col_static(prev, curr);
        let col_motion = compute_col_motion(prev, curr, dx, dy);
        let extents = scan_edges(
            &row_static,
            &row_motion,
            &col_static,
            &col_motion,
            &self.config,
        );

        let top = sample_band_bg_color(prev, Edge::Top, extents.top).unwrap_or([0, 0, 0, 0]);
        let bottom =
            sample_band_bg_color(prev, Edge::Bottom, extents.bottom).unwrap_or([0, 0, 0, 0]);
        let left = sample_band_bg_color(prev, Edge::Left, extents.left).unwrap_or([0, 0, 0, 0]);
        let right = sample_band_bg_color(prev, Edge::Right, extents.right).unwrap_or([0, 0, 0, 0]);

        self.observations.push(EdgeObservation {
            top: BandObs {
                thickness: extents.top,
                color: top,
            },
            bottom: BandObs {
                thickness: extents.bottom,
                color: bottom,
            },
            left: BandObs {
                thickness: extents.left,
                color: left,
            },
            right: BandObs {
                thickness: extents.right,
                color: right,
            },
        });

        if self.observations.len() >= self.config.min_observations {
            self.locked = Some(self.aggregate_mask());
        }
    }

    pub(crate) fn mask(&self) -> Option<&StaticMask> {
        self.locked.as_ref()
    }

    fn aggregate_mask(&self) -> StaticMask {
        StaticMask {
            top: self.aggregate_band(|o| o.top),
            bottom: self.aggregate_band(|o| o.bottom),
            left: self.aggregate_band(|o| o.left),
            right: self.aggregate_band(|o| o.right),
        }
    }

    fn aggregate_band(&self, pick: impl Fn(&EdgeObservation) -> BandObs) -> Option<StickyBand> {
        let mut thicknesses: Vec<u32> = self
            .observations
            .iter()
            .map(|o| pick(o).thickness)
            .collect();
        thicknesses.sort_unstable();
        let median_thickness = thicknesses[thicknesses.len() / 2];
        if median_thickness == 0 {
            return None;
        }

        let mut rs = Vec::new();
        let mut gs = Vec::new();
        let mut bs = Vec::new();
        let mut as_ = Vec::new();
        for obs in &self.observations {
            let b = pick(obs);
            if b.thickness == 0 {
                continue;
            }
            rs.push(b.color[0]);
            gs.push(b.color[1]);
            bs.push(b.color[2]);
            as_.push(b.color[3]);
        }
        let color = [median_u8(rs), median_u8(gs), median_u8(bs), median_u8(as_)];
        Some(StickyBand {
            thickness: median_thickness,
            bg_color: color,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_mask_default_is_all_none() {
        let mask = StaticMask::default();
        assert!(mask.top.is_none());
        assert!(mask.bottom.is_none());
        assert!(mask.left.is_none());
        assert!(mask.right.is_none());
    }

    #[test]
    fn static_region_config_default_values() {
        let cfg = StaticRegionConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.min_observations, 3);
        assert!((cfg.static_mad_threshold - 4.0 / 255.0).abs() < 1e-9);
        assert!((cfg.motion_margin - 4.0 / 255.0).abs() < 1e-9);
        assert!((cfg.max_band_ratio - 0.30).abs() < 1e-9);
    }
}

#[cfg(test)]
mod detector_tests {
    use super::*;
    use image::imageops;

    fn black(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]))
    }

    fn textured_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
        for y in 0..height {
            for x in 0..width {
                if (x / 4 + y / 6) % 2 == 0 {
                    img.put_pixel(
                        x,
                        y,
                        Rgba([30, ((x * 7) % 200) as u8, ((y * 11) % 200) as u8, 255]),
                    );
                }
            }
        }
        img
    }

    fn crop(canvas: &RgbaImage, y: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, 0, y, canvas.width(), h).to_image()
    }

    fn paint_left_sidebar(frame: &mut RgbaImage, width: u32, color: [u8; 4]) {
        for y in 0..frame.height() {
            for x in 0..width.min(frame.width()) {
                frame.put_pixel(x, y, Rgba(color));
            }
        }
    }

    fn paint_top_band(frame: &mut RgbaImage, height: u32, color: [u8; 4]) {
        for y in 0..height.min(frame.height()) {
            for x in 0..frame.width() {
                frame.put_pixel(x, y, Rgba(color));
            }
        }
    }

    use crate::static_region::{
        compute_col_motion, compute_col_static, compute_row_motion, compute_row_static,
        sample_band_bg_color, scan_edges, Edge, EdgeExtents,
    };

    #[test]
    fn detector_returns_none_before_any_observation() {
        let d = StaticRegionDetector::new(StaticRegionConfig::default());
        assert!(d.mask().is_none());
    }

    #[test]
    fn detector_returns_none_below_min_observations() {
        let cfg = StaticRegionConfig {
            min_observations: 3,
            ..StaticRegionConfig::default()
        };
        let mut d = StaticRegionDetector::new(cfg);
        let prev = black(4, 4);
        let curr = black(4, 4);
        d.observe(&prev, &curr, 0, 1);
        d.observe(&prev, &curr, 0, 1);
        assert!(
            d.mask().is_none(),
            "must not lock with fewer than min_observations"
        );
    }

    #[test]
    fn row_static_zero_for_identical_frames() {
        let prev = textured_canvas(20, 30);
        let curr = prev.clone();
        let row_static = compute_row_static(&prev, &curr);
        assert_eq!(row_static.len(), 30);
        for v in row_static {
            assert!(v < 1e-6);
        }
    }

    #[test]
    fn row_motion_zero_for_aligned_vertical_scroll() {
        let canvas = textured_canvas(40, 200);
        let prev = crop(&canvas, 0, 80);
        let curr = crop(&canvas, 20, 80);
        let row_motion = compute_row_motion(&prev, &curr, 0, 20);
        let middle = row_motion[40];
        assert!(
            middle.is_finite(),
            "middle row should have a defined motion-aligned MAD"
        );
        assert!(
            middle < 1e-3,
            "aligned content should produce near-zero MAD, got {middle}"
        );
    }

    #[test]
    fn col_static_zero_for_identical_frames() {
        let prev = textured_canvas(30, 20);
        let curr = prev.clone();
        let col_static = compute_col_static(&prev, &curr);
        assert_eq!(col_static.len(), 30);
        for v in col_static {
            assert!(v < 1e-6);
        }
    }

    #[test]
    fn col_motion_zero_for_aligned_horizontal_scroll() {
        let canvas = textured_canvas(200, 40);
        let prev = imageops::crop_imm(&canvas, 0, 0, 80, 40).to_image();
        let curr = imageops::crop_imm(&canvas, 20, 0, 80, 40).to_image();
        let col_motion = compute_col_motion(&prev, &curr, 20, 0);
        let middle = col_motion[40];
        assert!(middle.is_finite());
        assert!(
            middle < 1e-3,
            "aligned content should produce near-zero MAD, got {middle}"
        );
    }

    #[test]
    fn scan_returns_zero_when_no_static_lines() {
        let h = 10usize;
        let w = 8usize;
        let row_static = vec![0.5; h];
        let row_motion = vec![0.5; h];
        let col_static = vec![0.5; w];
        let col_motion = vec![0.5; w];
        let cfg = StaticRegionConfig::default();
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(
            e,
            EdgeExtents {
                top: 0,
                bottom: 0,
                left: 0,
                right: 0
            }
        );
    }

    #[test]
    fn scan_finds_top_band_up_to_first_non_static_row() {
        let row_static = vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
        let row_motion = vec![0.4, 0.4, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let col_static = vec![0.5; 8];
        let col_motion = vec![0.0; 8];
        let cfg = StaticRegionConfig::default();
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(e.top, 3);
        assert_eq!(e.bottom, 0);
    }

    #[test]
    fn scan_finds_bottom_band() {
        let row_static = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0];
        let row_motion = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.4, 0.4];
        let col_static = vec![0.5; 8];
        let col_motion = vec![0.0; 8];
        let cfg = StaticRegionConfig::default();
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(e.bottom, 3);
    }

    #[test]
    fn scan_finds_left_and_right_columns() {
        let row_static = vec![0.5; 10];
        let row_motion = vec![0.0; 10];
        let col_static = vec![0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0];
        let col_motion = vec![0.4, 0.4, 0.0, 0.0, 0.0, 0.0, 0.4, 0.4];
        let cfg = StaticRegionConfig::default();
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(e.left, 2);
        assert_eq!(e.right, 2);
    }

    #[test]
    fn scan_clamps_extent_above_max_band_ratio() {
        let row_static = vec![0.0; 10];
        let row_motion = vec![0.4; 10];
        let col_static = vec![0.5; 8];
        let col_motion = vec![0.0; 8];
        let cfg = StaticRegionConfig {
            max_band_ratio: 0.3,
            ..StaticRegionConfig::default()
        };
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(
            e.top, 0,
            "extent above max_band_ratio must be zeroed, got {}",
            e.top
        );
    }

    #[test]
    fn scan_treats_nan_motion_as_static_only_when_static_score_is_very_low() {
        let row_static = vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
        let row_motion = vec![
            f32::NAN,
            f32::NAN,
            f32::NAN,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let col_static = vec![0.5; 8];
        let col_motion = vec![0.0; 8];
        let cfg = StaticRegionConfig::default();
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(
            e.top, 3,
            "NaN motion + very low static should classify as static"
        );
    }

    #[test]
    fn scan_rejects_nan_motion_when_static_score_not_negligible() {
        let row_static = vec![0.01, 0.01, 0.01, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
        let row_motion = vec![
            f32::NAN,
            f32::NAN,
            f32::NAN,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let col_static = vec![0.5; 8];
        let col_motion = vec![0.0; 8];
        let cfg = StaticRegionConfig::default();
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(
            e.top, 0,
            "NaN motion + moderate static should NOT count as static"
        );
    }

    #[test]
    fn scan_treats_both_scores_below_quarter_threshold_as_static() {
        // Case (c): uniform-color sticky element. Both static and motion-aligned
        // MAD collapse to ~0 because the line's pixels are constant, so
        // (motion - static) cannot exceed motion_margin. The dedicated
        // `motion_score < threshold/4` branch must accept these as static —
        // without it, flat sidebars / footers go undetected.
        let row_static = vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
        // Default threshold = 4/255 ≈ 0.01568; threshold/4 ≈ 0.00392.
        // motion_score 0.002 sits below threshold/4 for the first 3 rows.
        let row_motion = vec![0.002, 0.002, 0.002, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let col_static = vec![0.5; 8];
        let col_motion = vec![0.0; 8];
        let cfg = StaticRegionConfig::default();
        let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
        assert_eq!(
            e.top, 3,
            "uniform-color sticky: both scores below threshold/4 should classify as static"
        );
    }

    #[test]
    fn bg_color_returns_uniform_band_color() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        for y in 0..4 {
            for x in 0..20 {
                img.put_pixel(x, y, Rgba([100, 110, 120, 255]));
            }
        }
        let bg = sample_band_bg_color(&img, Edge::Top, 4).expect("non-zero band");
        assert_eq!(bg, [100, 110, 120, 255]);
    }

    #[test]
    fn bg_color_is_channel_wise_median_for_noisy_band() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        for y in 0..4 {
            for x in 0..20 {
                img.put_pixel(x, y, Rgba([100, 110, 120, 255]));
            }
        }
        img.put_pixel(0, 0, Rgba([255, 0, 255, 255]));
        img.put_pixel(19, 3, Rgba([0, 255, 0, 255]));
        let bg = sample_band_bg_color(&img, Edge::Top, 4).expect("non-zero band");
        assert_eq!(bg, [100, 110, 120, 255]);
    }

    #[test]
    fn bg_color_zero_thickness_returns_none() {
        let img = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        assert!(sample_band_bg_color(&img, Edge::Top, 0).is_none());
    }

    #[test]
    fn pure_scroll_input_locks_with_all_none_bands() {
        let canvas = textured_canvas(40, 300);
        let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
        for i in 0..4 {
            let prev = crop(&canvas, (i * 20) as u32, 80);
            let curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
            d.observe(&prev, &curr, 0, 20);
        }
        let mask = d.mask().expect("detector must lock after min_observations");
        assert!(mask.top.is_none());
        assert!(mask.bottom.is_none());
        assert!(mask.left.is_none());
        assert!(mask.right.is_none());
    }

    #[test]
    fn detector_locks_left_sidebar_with_median_thickness() {
        let bg = [100, 110, 120, 255];
        let canvas = textured_canvas(40, 300);
        let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
        for i in 0..4 {
            let mut prev = crop(&canvas, (i * 20) as u32, 80);
            let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
            paint_left_sidebar(&mut prev, 6, bg);
            paint_left_sidebar(&mut curr, 6, bg);
            d.observe(&prev, &curr, 0, 20);
        }
        let mask = d.mask().expect("must lock");
        let left = mask.left.expect("left band detected");
        assert_eq!(left.thickness, 6);
        assert_eq!(left.bg_color, bg);
        assert!(mask.top.is_none());
        assert!(mask.right.is_none());
        assert!(mask.bottom.is_none());
    }

    #[test]
    fn detector_locks_top_band() {
        let bg = [80, 80, 80, 255];
        let canvas = textured_canvas(40, 300);
        let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
        for i in 0..4 {
            let mut prev = crop(&canvas, (i * 20) as u32, 80);
            let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
            paint_top_band(&mut prev, 5, bg);
            paint_top_band(&mut curr, 5, bg);
            d.observe(&prev, &curr, 0, 20);
        }
        let mask = d.mask().expect("must lock");
        let top = mask.top.expect("top band detected");
        assert_eq!(top.thickness, 5);
        assert_eq!(top.bg_color, bg);
    }

    #[test]
    fn detector_single_outlier_does_not_shift_locked_thickness() {
        let bg = [100, 110, 120, 255];
        let canvas = textured_canvas(40, 300);
        let cfg = StaticRegionConfig {
            min_observations: 4,
            ..StaticRegionConfig::default()
        };
        let mut d = StaticRegionDetector::new(cfg);
        let widths = [6, 6, 10, 6];
        for (i, w) in widths.iter().enumerate() {
            let mut prev = crop(&canvas, (i * 20) as u32, 80);
            let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
            paint_left_sidebar(&mut prev, *w, bg);
            paint_left_sidebar(&mut curr, *w, bg);
            d.observe(&prev, &curr, 0, 20);
        }
        let mask = d.mask().expect("must lock");
        let left = mask.left.expect("left band");
        assert_eq!(left.thickness, 6, "median should suppress single outlier");
    }

    #[test]
    fn subsequent_observations_after_lock_are_noops() {
        let bg = [100, 110, 120, 255];
        let canvas = textured_canvas(40, 300);
        let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
        for i in 0..3 {
            let mut prev = crop(&canvas, (i * 20) as u32, 80);
            let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
            paint_left_sidebar(&mut prev, 6, bg);
            paint_left_sidebar(&mut curr, 6, bg);
            d.observe(&prev, &curr, 0, 20);
        }
        let locked = *d.mask().unwrap();
        let mut prev = crop(&canvas, 60, 80);
        let mut curr = crop(&canvas, 80, 80);
        paint_left_sidebar(&mut prev, 18, [1, 1, 1, 255]);
        paint_left_sidebar(&mut curr, 18, [1, 1, 1, 255]);
        d.observe(&prev, &curr, 0, 20);
        assert_eq!(d.mask().copied().unwrap(), locked);
    }
}
