use crate::annotation::TwoPointKind;
use crate::geometry::{ImagePoint, ImageRect};

pub fn arrowhead_points(start: ImagePoint, end: ImagePoint, width: f32) -> [ImagePoint; 3] {
    let length = start.distance(end);
    debug_assert!(length > 0.0);
    let direction = ((end.x - start.x) / length, (end.y - start.y) / length);
    let perpendicular = (-direction.1, direction.0);
    let head_length = (width * 6.0).clamp(16.0, 32.0);
    let half_width = (width * 3.0).clamp(8.0, 16.0);
    let base = ImagePoint::new(
        end.x - direction.0 * head_length,
        end.y - direction.1 * head_length,
    );
    [
        end,
        ImagePoint::new(
            base.x + perpendicular.0 * half_width,
            base.y + perpendicular.1 * half_width,
        ),
        ImagePoint::new(
            base.x - perpendicular.0 * half_width,
            base.y - perpendicular.1 * half_width,
        ),
    ]
}

pub fn segment_distance(point: ImagePoint, start: ImagePoint, end: ImagePoint) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    debug_assert!(length_squared > 0.0);
    let t =
        (((point.x - start.x) * dx + (point.y - start.y) * dy) / length_squared).clamp(0.0, 1.0);
    point.distance(ImagePoint::new(start.x + t * dx, start.y + t * dy))
}

pub fn point_in_triangle(point: ImagePoint, triangle: [ImagePoint; 3]) -> bool {
    fn edge(a: ImagePoint, b: ImagePoint, point: ImagePoint) -> f32 {
        (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x)
    }

    let d1 = edge(triangle[0], triangle[1], point);
    let d2 = edge(triangle[1], triangle[2], point);
    let d3 = edge(triangle[2], triangle[0], point);
    let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_negative && has_positive)
}

pub fn two_point_bounds(
    kind: TwoPointKind,
    start: ImagePoint,
    end: ImagePoint,
    width: f32,
) -> ImageRect {
    let shaft = ImageRect::from_corners(start, end).expanded(width / 2.0);
    if kind == TwoPointKind::Line {
        return shaft;
    }

    let triangle = arrowhead_points(start, end, width);
    let mut x0 = shaft.x;
    let mut y0 = shaft.y;
    let mut x1 = shaft.x + shaft.width;
    let mut y1 = shaft.y + shaft.height;
    for point in triangle {
        x0 = x0.min(point.x);
        y0 = y0.min(point.y);
        x1 = x1.max(point.x);
        y1 = y1.max(point.y);
    }
    ImageRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::ImagePoint;

    #[test]
    fn default_horizontal_arrowhead_matches_reviewed_geometry() {
        let points = arrowhead_points(
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(100.0, 50.0),
            4.0,
        );
        assert_eq!(points[0], ImagePoint::new(100.0, 50.0));
        assert_eq!(points[1], ImagePoint::new(76.0, 62.0));
        assert_eq!(points[2], ImagePoint::new(76.0, 38.0));
    }

    #[test]
    fn arrowhead_clamps_at_minimum_and_maximum() {
        let thin = arrowhead_points(ImagePoint::new(0.0, 0.0), ImagePoint::new(100.0, 0.0), 1.0);
        assert_eq!(thin[1], ImagePoint::new(84.0, 8.0));
        let thick = arrowhead_points(ImagePoint::new(0.0, 0.0), ImagePoint::new(100.0, 0.0), 16.0);
        assert_eq!(thick[1], ImagePoint::new(68.0, 16.0));
    }

    #[test]
    fn segment_distance_clamps_to_finite_endpoints() {
        let a = ImagePoint::new(10.0, 10.0);
        let b = ImagePoint::new(20.0, 10.0);
        assert_eq!(segment_distance(ImagePoint::new(15.0, 14.0), a, b), 4.0);
        assert_eq!(segment_distance(ImagePoint::new(25.0, 10.0), a, b), 5.0);
    }

    #[test]
    fn triangle_membership_includes_edges_and_rejects_outside_points() {
        let triangle = [
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(4.0, 0.0),
            ImagePoint::new(0.0, 4.0),
        ];
        assert!(point_in_triangle(ImagePoint::new(1.0, 1.0), triangle));
        assert!(point_in_triangle(ImagePoint::new(2.0, 0.0), triangle));
        assert!(!point_in_triangle(ImagePoint::new(3.0, 3.0), triangle));
    }

    #[test]
    fn bounds_include_shaft_width_and_arrowhead() {
        let start = ImagePoint::new(10.0, 50.0);
        let end = ImagePoint::new(100.0, 50.0);
        assert_eq!(
            two_point_bounds(TwoPointKind::Line, start, end, 4.0),
            ImageRect {
                x: 8.0,
                y: 48.0,
                width: 94.0,
                height: 4.0,
            }
        );
        assert_eq!(
            two_point_bounds(TwoPointKind::Arrow, start, end, 4.0),
            ImageRect {
                x: 8.0,
                y: 38.0,
                width: 94.0,
                height: 24.0,
            }
        );
    }
}
