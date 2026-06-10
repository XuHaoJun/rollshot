#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Bottom,
    Top,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeRequirements {
    pub toolbar: Size,
    pub preview: Option<Size>,
    /// Reserved for future viewport-edge clamping. Not yet used by `place_chrome`.
    pub margin: f32,
    pub spacing: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChromePlacement {
    Separate {
        toolbar_band: Band,
        toolbar: Rect,
        preview_band: Option<Band>,
        preview: Option<Rect>,
    },
    Combined {
        band: Band,
        toolbar: Rect,
        preview: Rect,
    },
    ActivityAutoHide {
        overlay_toolbar: Rect,
        overlay_preview: Option<Rect>,
    },
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }
}

impl Size {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

impl ChromePlacement {
    pub fn toolbar_band(&self) -> Option<Band> {
        match self {
            ChromePlacement::Separate { toolbar_band, .. } => Some(*toolbar_band),
            ChromePlacement::Combined { band, .. } => Some(*band),
            ChromePlacement::ActivityAutoHide { .. } => None,
        }
    }

    pub fn toolbar_rect(&self) -> Rect {
        match self {
            ChromePlacement::Separate { toolbar, .. } => *toolbar,
            ChromePlacement::Combined { toolbar, .. } => *toolbar,
            ChromePlacement::ActivityAutoHide {
                overlay_toolbar, ..
            } => *overlay_toolbar,
        }
    }

    pub fn preview_band(&self) -> Option<Band> {
        match self {
            ChromePlacement::Separate { preview_band, .. } => *preview_band,
            ChromePlacement::Combined { band, .. } => Some(*band),
            ChromePlacement::ActivityAutoHide { .. } => None,
        }
    }

    pub fn preview_rect(&self) -> Option<Rect> {
        match self {
            ChromePlacement::Separate { preview, .. } => *preview,
            ChromePlacement::Combined { preview, .. } => Some(*preview),
            ChromePlacement::ActivityAutoHide {
                overlay_preview, ..
            } => *overlay_preview,
        }
    }
}

fn band_rect(band: Band, viewport: Rect, crop: Rect) -> Rect {
    match band {
        Band::Bottom => Rect::new(
            viewport.x,
            crop.y + crop.height,
            viewport.width,
            (viewport.y + viewport.height) - (crop.y + crop.height),
        ),
        Band::Top => Rect::new(viewport.x, viewport.y, viewport.width, crop.y - viewport.y),
        Band::Left => Rect::new(viewport.x, viewport.y, crop.x - viewport.x, viewport.height),
        Band::Right => Rect::new(
            crop.x + crop.width,
            viewport.y,
            (viewport.x + viewport.width) - (crop.x + crop.width),
            viewport.height,
        ),
    }
}

fn fits_in_band(band: Band, band_r: Rect, size: Size) -> bool {
    match band {
        Band::Bottom | Band::Top => size.width <= band_r.width && size.height <= band_r.height,
        Band::Left | Band::Right => size.height <= band_r.height && size.width <= band_r.width,
    }
}

fn position_in_band(band: Band, band_r: Rect, crop: Rect, size: Size) -> Rect {
    match band {
        Band::Bottom | Band::Top => Rect::new(crop.x, band_r.y, size.width, size.height),
        Band::Left | Band::Right => Rect::new(band_r.x, crop.y, size.width, size.height),
    }
}

fn adjacent_preview_rect(
    band: Band,
    viewport: Rect,
    crop: Rect,
    size: Size,
    margin: f32,
) -> Option<Rect> {
    let rect = match band {
        Band::Right => Rect::new(
            crop.x + crop.width + margin,
            crop.y
                .clamp(viewport.y, viewport.y + viewport.height - size.height),
            size.width,
            size.height,
        ),
        Band::Left => Rect::new(
            crop.x - margin - size.width,
            crop.y
                .clamp(viewport.y, viewport.y + viewport.height - size.height),
            size.width,
            size.height,
        ),
        Band::Top => Rect::new(
            crop.x
                .clamp(viewport.x, viewport.x + viewport.width - size.width),
            crop.y - margin - size.height,
            size.width,
            size.height,
        ),
        Band::Bottom => Rect::new(
            crop.x
                .clamp(viewport.x, viewport.x + viewport.width - size.width),
            crop.y + crop.height + margin,
            size.width,
            size.height,
        ),
    };

    (rect.x >= viewport.x
        && rect.y >= viewport.y
        && rect.x + rect.width <= viewport.x + viewport.width
        && rect.y + rect.height <= viewport.y + viewport.height)
        .then_some(rect)
}

fn stack_fits_in_band(band_r: Rect, s1: Size, s2: Size, spacing: f32) -> bool {
    s1.width <= band_r.width
        && s2.width <= band_r.width
        && s1.height + spacing + s2.height <= band_r.height
}

fn stack_in_band(
    band: Band,
    band_r: Rect,
    crop: Rect,
    first: Size,
    second: Size,
    spacing: f32,
) -> (Rect, Rect) {
    match band {
        Band::Bottom | Band::Top => {
            let r1 = Rect::new(crop.x, band_r.y, first.width, first.height);
            let r2 = Rect::new(
                crop.x,
                band_r.y + first.height + spacing,
                second.width,
                second.height,
            );
            (r1, r2)
        }
        Band::Left | Band::Right => {
            let r1 = Rect::new(band_r.x, crop.y, first.width, first.height);
            let r2 = Rect::new(
                band_r.x,
                crop.y + first.height + spacing,
                second.width,
                second.height,
            );
            (r1, r2)
        }
    }
}

pub fn place_chrome(viewport: Rect, crop: Rect, req: ChromeRequirements) -> ChromePlacement {
    let bands = [Band::Bottom, Band::Top, Band::Left, Band::Right];
    let band_rects: Vec<(Band, Rect)> = bands
        .iter()
        .map(|&b| (b, band_rect(b, viewport, crop)))
        .collect();

    let toolbar_band = band_rects
        .iter()
        .find(|(b, r)| fits_in_band(*b, *r, req.toolbar))
        .map(|(b, _)| *b);

    let Some(toolbar_band) = toolbar_band else {
        return ChromePlacement::ActivityAutoHide {
            overlay_toolbar: Rect::new(
                crop.x,
                crop.y + crop.height - req.toolbar.height,
                req.toolbar.width,
                req.toolbar.height,
            ),
            overlay_preview: req
                .preview
                .map(|p| Rect::new(crop.x, crop.y, p.width, p.height)),
        };
    };

    let toolbar_band_rect = band_rects
        .iter()
        .find(|(b, _)| *b == toolbar_band)
        .map(|(_, r)| *r)
        .unwrap();
    let toolbar_rect = position_in_band(toolbar_band, toolbar_band_rect, crop, req.toolbar);

    let Some(preview_size) = req.preview else {
        return ChromePlacement::Separate {
            toolbar_band,
            toolbar: toolbar_rect,
            preview_band: None,
            preview: None,
        };
    };

    let preview = [Band::Right, Band::Left, Band::Top, Band::Bottom]
        .into_iter()
        .filter_map(|band| {
            adjacent_preview_rect(band, viewport, crop, preview_size, req.margin)
                .map(|rect| (band, rect))
        })
        .find(|(_, rect)| !rect.intersects(&toolbar_rect));

    if let Some((preview_band, preview_rect)) = preview {
        return ChromePlacement::Separate {
            toolbar_band,
            toolbar: toolbar_rect,
            preview_band: Some(preview_band),
            preview: Some(preview_rect),
        };
    }

    let num_bands_that_fit_both = band_rects
        .iter()
        .filter(|(_, r)| stack_fits_in_band(*r, req.toolbar, preview_size, req.spacing))
        .count();

    if num_bands_that_fit_both >= 1 {
        let (toolbar, preview) = stack_in_band(
            toolbar_band,
            toolbar_band_rect,
            crop,
            req.toolbar,
            preview_size,
            req.spacing,
        );
        if num_bands_that_fit_both == 1 {
            return ChromePlacement::Combined {
                band: toolbar_band,
                toolbar,
                preview,
            };
        }
        return ChromePlacement::Separate {
            toolbar_band,
            toolbar,
            preview_band: Some(toolbar_band),
            preview: Some(preview),
        };
    }

    ChromePlacement::ActivityAutoHide {
        overlay_toolbar: Rect::new(
            crop.x,
            crop.y + crop.height - req.toolbar.height,
            req.toolbar.width,
            req.toolbar.height,
        ),
        overlay_preview: Some(Rect::new(
            crop.x,
            crop.y,
            preview_size.width,
            preview_size.height,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(toolbar: Size, preview: Option<Size>) -> ChromeRequirements {
        ChromeRequirements {
            toolbar,
            preview,
            margin: 8.0,
            spacing: 4.0,
        }
    }

    #[test]
    fn toolbar_uses_bottom_top_left_right_priority() {
        let viewport = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let toolbar = Size::new(260.0, 48.0);

        assert_eq!(
            place_chrome(
                viewport,
                Rect::new(250.0, 200.0, 500.0, 300.0),
                req(toolbar, None)
            )
            .toolbar_band(),
            Some(Band::Bottom)
        );
        assert_eq!(
            place_chrome(
                viewport,
                Rect::new(250.0, 550.0, 500.0, 230.0),
                req(toolbar, None)
            )
            .toolbar_band(),
            Some(Band::Top)
        );
        assert_eq!(
            place_chrome(
                viewport,
                Rect::new(300.0, 10.0, 680.0, 780.0),
                req(toolbar, None)
            )
            .toolbar_band(),
            Some(Band::Left)
        );
        assert_eq!(
            place_chrome(
                viewport,
                Rect::new(10.0, 10.0, 680.0, 780.0),
                req(toolbar, None)
            )
            .toolbar_band(),
            Some(Band::Right)
        );
    }

    #[test]
    fn preview_uses_right_left_top_bottom_priority_when_space_is_available() {
        let placement = place_chrome(
            Rect::new(0.0, 0.0, 1200.0, 900.0),
            Rect::new(250.0, 180.0, 600.0, 500.0),
            req(Size::new(280.0, 48.0), Some(Size::new(240.0, 320.0))),
        );
        assert_eq!(placement.toolbar_band(), Some(Band::Bottom));
        assert_eq!(placement.preview_band(), Some(Band::Right));
        assert!(!placement
            .toolbar_rect()
            .intersects(&placement.preview_rect().unwrap()));
        assert_eq!(placement.preview_rect().unwrap().x, 858.0);
    }

    #[test]
    fn preview_uses_left_when_right_is_too_narrow() {
        let placement = place_chrome(
            Rect::new(0.0, 0.0, 1200.0, 900.0),
            Rect::new(350.0, 180.0, 700.0, 500.0),
            req(Size::new(280.0, 48.0), Some(Size::new(240.0, 320.0))),
        );

        assert_eq!(placement.preview_band(), Some(Band::Left));
        assert_eq!(placement.preview_rect().unwrap().x, 102.0);
    }

    #[test]
    fn preview_uses_top_then_bottom_when_sides_are_too_narrow() {
        let top = place_chrome(
            Rect::new(0.0, 0.0, 1000.0, 900.0),
            Rect::new(100.0, 350.0, 800.0, 400.0),
            req(Size::new(280.0, 48.0), Some(Size::new(280.0, 300.0))),
        );
        assert_eq!(top.preview_band(), Some(Band::Top));

        let bottom = place_chrome(
            Rect::new(0.0, 0.0, 1000.0, 900.0),
            Rect::new(100.0, 50.0, 800.0, 400.0),
            req(Size::new(280.0, 48.0), Some(Size::new(280.0, 300.0))),
        );
        assert_eq!(bottom.preview_band(), Some(Band::Bottom));
    }

    #[test]
    fn one_band_combines_without_duplicating_toolbar() {
        let placement = place_chrome(
            Rect::new(0.0, 0.0, 1000.0, 800.0),
            Rect::new(0.0, 0.0, 720.0, 800.0),
            req(Size::new(240.0, 48.0), Some(Size::new(240.0, 500.0))),
        );
        assert!(matches!(
            placement,
            ChromePlacement::Combined {
                band: Band::Right,
                ..
            }
        ));
    }

    #[test]
    fn no_outside_space_uses_activity_auto_hide() {
        let placement = place_chrome(
            Rect::new(0.0, 0.0, 1000.0, 800.0),
            Rect::new(0.0, 0.0, 1000.0, 800.0),
            req(Size::new(260.0, 48.0), Some(Size::new(240.0, 500.0))),
        );
        assert!(matches!(
            placement,
            ChromePlacement::ActivityAutoHide { .. }
        ));
    }
}
