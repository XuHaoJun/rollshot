//! Freehand polyline geometry: bounds and distance used by hit testing and
//! culling (Slice 4 spec §6.4/§6.5).

use crate::geometry::{ImagePoint, ImageRect};
use crate::two_point::segment_distance;

/// Minimum distance from `point` to any segment of the polyline. Consecutive
/// duplicate points contribute a point-distance (no zero-length segment math).
/// A single-point slice degenerates to point distance.
pub(crate) fn polyline_distance(point: ImagePoint, points: &[ImagePoint]) -> f32 {
    debug_assert!(!points.is_empty());
    if points.len() == 1 {
        return point.distance(points[0]);
    }
    points
        .windows(2)
        .map(|pair| {
            if pair[0] == pair[1] {
                point.distance(pair[0])
            } else {
                segment_distance(point, pair[0], pair[1])
            }
        })
        .fold(f32::MAX, f32::min)
}

/// Conservative visual bounds: AABB of the points expanded by half the
/// stroke width (round caps extend half a width past the endpoints).
pub(crate) fn freehand_bounds(points: &[ImagePoint], width: f32) -> ImageRect {
    debug_assert!(!points.is_empty());
    let mut x0 = f32::MAX;
    let mut y0 = f32::MAX;
    let mut x1 = f32::MIN;
    let mut y1 = f32::MIN;
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    ImageRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
    .expanded(width / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_path() -> Vec<ImagePoint> {
        vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(10.0, 0.0),
            ImagePoint::new(10.0, 10.0),
        ]
    }

    #[test]
    fn distance_on_segment_is_zero() {
        assert_eq!(polyline_distance(ImagePoint::new(5.0, 0.0), &l_path()), 0.0);
    }

    #[test]
    fn distance_uses_nearest_segment() {
        // Point near the vertical leg, far from the horizontal leg.
        let d = polyline_distance(ImagePoint::new(13.0, 8.0), &l_path());
        assert!((d - 3.0).abs() < 1e-4);
    }

    #[test]
    fn distance_in_empty_corner_is_not_zero() {
        // Inside the AABB but far from both legs (the bounding-box-only trap):
        // (2, 8) is 8.0 from the horizontal leg (projects to (2, 0)) and 8.0
        // from the vertical leg (projects to (10, 8)).
        let d = polyline_distance(ImagePoint::new(2.0, 8.0), &l_path());
        assert!((d - 8.0).abs() < 1e-4);
    }

    #[test]
    fn duplicate_consecutive_points_do_not_panic() {
        let pts = vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(4.0, 0.0),
        ];
        assert_eq!(polyline_distance(ImagePoint::new(2.0, 0.0), &pts), 0.0);
    }

    #[test]
    fn single_point_degenerates_to_point_distance() {
        let point = ImagePoint::new(3.0, 4.0);
        assert_eq!(polyline_distance(ImagePoint::new(0.0, 0.0), &[point]), 5.0);
    }

    #[test]
    fn bounds_expand_by_half_width() {
        let b = freehand_bounds(&l_path(), 4.0);
        assert_eq!(
            b,
            ImageRect {
                x: -2.0,
                y: -2.0,
                width: 14.0,
                height: 14.0
            }
        );
    }
}
