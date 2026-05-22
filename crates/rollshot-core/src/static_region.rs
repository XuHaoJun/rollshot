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
