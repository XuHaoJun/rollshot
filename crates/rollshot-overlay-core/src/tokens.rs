//! Crop selection visual design tokens consumed by the iced overlay's
//! `CropCanvas`.

/// An sRGB color: 8-bit channels + float alpha — the form both CSS
/// (`#rrggbb` / `rgba(r,g,b,a)`) and `iced::Color::from_rgba8` can express.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// CSS-compatible spelling: `#rrggbb` when opaque, else `rgba(r, g, b, a)`.
    pub fn to_css(&self) -> String {
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("rgba({}, {}, {}, {})", self.r, self.g, self.b, self.a)
        }
    }
}

/// Crop rectangle border (sky-blue).
pub const CROP_BORDER: Rgba = Rgba::new(0x38, 0xbd, 0xf8, 1.0);
pub const CROP_BORDER_WIDTH: f32 = 2.0;
/// 1px white halo just outside the border.
pub const CROP_BORDER_HALO: Rgba = Rgba::new(255, 255, 255, 0.72);
/// Dark mask over everything outside the crop once a rect exists.
pub const CROP_MASK: Rgba = Rgba::new(0, 0, 0, 0.24);
/// Dim over the whole layer before any rect is drawn.
pub const CROP_DIM: Rgba = Rgba::new(0, 0, 0, 0.22);
/// Cursor crosshair guides.
pub const CROP_GUIDE: Rgba = Rgba::new(147, 197, 253, 0.48);
pub const CROP_GUIDE_WIDTH: f32 = 1.0;
/// Recovery edge guide (amber) — drawn on the captured edge while paused.
pub const RECOVERY_EDGE: Rgba = Rgba::new(0xf5, 0x9e, 0x0b, 1.0);
pub const RECOVERY_EDGE_WIDTH: f32 = 4.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_css_opaque_is_hex() {
        assert_eq!(CROP_BORDER.to_css(), "#38bdf8");
    }

    #[test]
    fn to_css_translucent_is_rgba() {
        assert_eq!(CROP_MASK.to_css(), "rgba(0, 0, 0, 0.24)");
        assert_eq!(CROP_GUIDE.to_css(), "rgba(147, 197, 253, 0.48)");
    }

    #[test]
    fn recovery_edge_is_amber() {
        assert_eq!(RECOVERY_EDGE.to_css(), "#f59e0b");
    }
}
