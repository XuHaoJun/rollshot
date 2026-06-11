//! Image-space geometry. All coordinates are full-resolution image pixels,
//! independent of any viewport zoom or scroll.

/// A point in full-resolution image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImagePoint {
    pub x: f32,
    pub y: f32,
}

impl ImagePoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Self) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    /// Clamp into the bounds of a `width × height` image.
    pub fn clamp_to(self, width: u32, height: u32) -> Self {
        Self {
            x: self.x.clamp(0.0, width as f32),
            y: self.y.clamp(0.0, height as f32),
        }
    }
}

/// An axis-aligned rectangle in full-resolution image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ImageRect {
    /// Normalized rect spanning two corners (handles inverted drags).
    pub fn from_corners(a: ImagePoint, b: ImagePoint) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self {
            x,
            y,
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }

    pub fn contains(&self, p: ImagePoint) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }

    pub fn intersects(&self, other: &ImageRect) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }

    pub fn center(&self) -> ImagePoint {
        ImagePoint::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Intersect with the image bounds, clipping overflow.
    pub fn clamp_to(self, width: u32, height: u32) -> Self {
        let x0 = self.x.clamp(0.0, width as f32);
        let y0 = self.y.clamp(0.0, height as f32);
        let x1 = (self.x + self.width).clamp(0.0, width as f32);
        let y1 = (self.y + self.height).clamp(0.0, height as f32);
        Self {
            x: x0,
            y: y0,
            width: (x1 - x0).max(0.0),
            height: (y1 - y0).max(0.0),
        }
    }

    /// Sub-pixel rects are treated as zero-area (spec §6: not committed).
    pub fn is_empty(&self) -> bool {
        self.width < 1.0 || self.height < 1.0
    }

    pub fn expanded(&self, margin: f32) -> Self {
        Self {
            x: self.x - margin,
            y: self.y - margin,
            width: self.width + margin * 2.0,
            height: self.height + margin * 2.0,
        }
    }
}

/// An 8-bit sRGB color with alpha, the form both `image::Rgba` and
/// `iced::Color::from_rgba8` can consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_corners_normalizes_inverted_drag() {
        let r = ImageRect::from_corners(ImagePoint::new(10.0, 20.0), ImagePoint::new(4.0, 6.0));
        assert_eq!(r, ImageRect { x: 4.0, y: 6.0, width: 6.0, height: 14.0 });
    }

    #[test]
    fn contains_and_center() {
        let r = ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 4.0 };
        assert!(r.contains(ImagePoint::new(5.0, 2.0)));
        assert!(!r.contains(ImagePoint::new(11.0, 2.0)));
        assert_eq!(r.center(), ImagePoint::new(5.0, 2.0));
    }

    #[test]
    fn intersects_overlapping_and_disjoint() {
        let a = ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let b = ImageRect { x: 5.0, y: 5.0, width: 10.0, height: 10.0 };
        let c = ImageRect { x: 20.0, y: 20.0, width: 5.0, height: 5.0 };
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn point_clamp_to_image_bounds() {
        assert_eq!(
            ImagePoint::new(-5.0, 900.0).clamp_to(100, 200),
            ImagePoint::new(0.0, 200.0)
        );
    }

    #[test]
    fn rect_clamp_to_keeps_size_when_inside_and_clips_when_outside() {
        let inside = ImageRect { x: 5.0, y: 5.0, width: 10.0, height: 10.0 };
        assert_eq!(inside.clamp_to(100, 100), inside);
        let overflow = ImageRect { x: 95.0, y: 95.0, width: 10.0, height: 10.0 };
        let clipped = overflow.clamp_to(100, 100);
        assert_eq!(clipped, ImageRect { x: 95.0, y: 95.0, width: 5.0, height: 5.0 });
    }

    #[test]
    fn zero_area_rect_is_empty() {
        assert!(ImageRect { x: 0.0, y: 0.0, width: 0.5, height: 10.0 }.is_empty());
        assert!(!ImageRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }.is_empty());
    }

    #[test]
    fn distance_is_euclidean() {
        assert_eq!(ImagePoint::new(0.0, 0.0).distance(ImagePoint::new(3.0, 4.0)), 5.0);
    }
}
