//! Freehand gesture helpers: pointer sampling filter, commit-time RDP
//! simplification, minimum-gesture rule, and rigid body movement
//! (Slice 4 spec §7). All screen-space thresholds divide by the viewport
//! scale so behavior is zoom-independent.

use rollshot_image_document::ImagePoint;

use super::two_point::MIN_GESTURE_SCREEN;

/// A new pointer sample must travel at least this many SCREEN pixels from
/// the last accepted point (spec §7.1).
pub const MIN_SAMPLE_DISTANCE_SCREEN: f32 = 2.0;
/// Ramer–Douglas–Peucker epsilon in SCREEN pixels (spec §7.2).
pub const RDP_EPSILON_SCREEN: f32 = 1.0;

pub fn should_accept_point(last: ImagePoint, candidate: ImagePoint, scale: f32) -> bool {
    last.distance(candidate) * scale >= MIN_SAMPLE_DISTANCE_SCREEN
}

/// Distance from `p` to the SEGMENT `a`..`b` (clamped projection, matching
/// the document hit-test metric; point distance when a == b). Segment — not
/// infinite-line — distance is required so a stroke that retraces along its
/// own line keeps points beyond the endpoint chord.
fn segment_distance(p: ImagePoint, a: ImagePoint, b: ImagePoint) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len_sq).clamp(0.0, 1.0);
    p.distance(ImagePoint::new(a.x + t * dx, a.y + t * dy))
}

/// Iterative Ramer–Douglas–Peucker. Keeps first and last points; the output
/// deviates from the input by at most `epsilon` (image-space units).
pub fn simplify_rdp(points: &[ImagePoint], epsilon: f32) -> Vec<ImagePoint> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let mut max_d = 0.0f32;
        let mut index = first;
        for i in (first + 1)..last {
            let d = segment_distance(points[i], points[first], points[last]);
            if d > max_d {
                max_d = d;
                index = i;
            }
        }
        if max_d > epsilon {
            keep[index] = true;
            stack.push((first, index));
            stack.push((index, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Minimum gesture: the larger bounding-box dimension must reach 4 screen
/// pixels. Uses one axis (not both) so a straight horizontal or vertical
/// stroke still commits (spec §7.2; differs from the box tool's two-axis
/// rule on purpose).
pub fn path_meets_threshold(points: &[ImagePoint], scale: f32) -> bool {
    if points.len() < 2 {
        return false;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    (x1 - x0).max(y1 - y0) * scale >= MIN_GESTURE_SCREEN
}

/// Rigid translation of the whole path so its bounding box stays within the
/// source image. Mirrors the TwoPoint body-move clamp (no deformation).
pub fn translated_points(
    points: &[ImagePoint],
    point: ImagePoint,
    grab_offset: (f32, f32),
    width: u32,
    height: u32,
) -> Vec<ImagePoint> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let anchor = points[0];
    let dx = (point.x - grab_offset.0 - anchor.x).clamp(-x0, width as f32 - x1);
    let dy = (point.y - grab_offset.1 - anchor.y).clamp(-y0, height as f32 - y1);
    points
        .iter()
        .map(|p| ImagePoint::new(p.x + dx, p.y + dy))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_filter_is_zoom_independent() {
        let last = ImagePoint::new(0.0, 0.0);
        // 1.5 image px at scale 1.0 → below 2 screen px.
        assert!(!should_accept_point(last, ImagePoint::new(1.5, 0.0), 1.0));
        // Same image distance at scale 2.0 → 3 screen px, accepted.
        assert!(should_accept_point(last, ImagePoint::new(1.5, 0.0), 2.0));
    }

    #[test]
    fn rdp_collapses_collinear_points() {
        let pts: Vec<ImagePoint> = (0..=10).map(|i| ImagePoint::new(i as f32, 0.0)).collect();
        assert_eq!(
            simplify_rdp(&pts, 1.0),
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)]
        );
    }

    #[test]
    fn rdp_preserves_corners_and_drops_small_wiggles() {
        // An L-shaped stroke with a 0.5-px wiggle on the horizontal leg:
        // the corner (10, 0) is kept (7.07 px off the end-to-end chord);
        // the wiggle (5, 0.5) is 0.5 px off the (0,0)-(10,0) sub-chord and
        // drops at epsilon 1.0.
        let pts = vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(5.0, 0.5),
            ImagePoint::new(10.0, 0.0),
            ImagePoint::new(10.0, 10.0),
        ];
        let out = simplify_rdp(&pts, 1.0);
        assert_eq!(
            out,
            vec![
                ImagePoint::new(0.0, 0.0),
                ImagePoint::new(10.0, 0.0),
                ImagePoint::new(10.0, 10.0),
            ]
        );
    }

    #[test]
    fn rdp_output_within_epsilon_of_input() {
        let pts: Vec<ImagePoint> = (0..100)
            .map(|i| {
                let x = i as f32;
                ImagePoint::new(x, (x / 6.0).sin() * 20.0)
            })
            .collect();
        let out = simplify_rdp(&pts, 1.0);
        assert!(out.len() < pts.len());
        // Every dropped input point stays within epsilon of the output path.
        for p in &pts {
            let d = out
                .windows(2)
                .map(|w| {
                    // Reuse the same distance definition as the document hit
                    // path: clamped projection onto each output segment.
                    let dx = w[1].x - w[0].x;
                    let dy = w[1].y - w[0].y;
                    let len_sq = dx * dx + dy * dy;
                    let t = (((p.x - w[0].x) * dx + (p.y - w[0].y) * dy) / len_sq).clamp(0.0, 1.0);
                    p.distance(ImagePoint::new(w[0].x + t * dx, w[0].y + t * dy))
                })
                .fold(f32::MAX, f32::min);
            assert!(d <= 1.0 + 1e-3, "point deviates by {d}");
        }
    }

    #[test]
    fn rdp_keeps_retrace_overshoot_beyond_endpoint_chord() {
        // Draw right to x=100, then retrace back to x=50: (100, 0) lies on
        // the infinite line through the (0,0)-(50,0) endpoints but 50 px past
        // the segment. It must survive simplification — dropping it would
        // halve the committed stroke.
        let pts = vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(100.0, 0.0),
            ImagePoint::new(50.0, 0.0),
        ];
        let out = simplify_rdp(&pts, 1.0);
        assert!(
            out.contains(&ImagePoint::new(100.0, 0.0)),
            "retrace overshoot must be kept, got {out:?}"
        );
    }

    #[test]
    fn rdp_keeps_near_full_retrace_to_start() {
        // Draw right to x=100 and release almost back at the start: without
        // segment distance the whole stroke collapses to a 2-px chord and is
        // then cancelled by the minimum-gesture rule.
        let pts = vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(100.0, 0.0),
            ImagePoint::new(2.0, 0.0),
        ];
        let out = simplify_rdp(&pts, 1.0);
        assert!(path_meets_threshold(&out, 1.0));
        assert!(out.contains(&ImagePoint::new(100.0, 0.0)));
    }

    #[test]
    fn short_strokes_survive_rdp() {
        let pts = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(3.0, 1.0)];
        assert_eq!(simplify_rdp(&pts, 1.0), pts);
    }

    #[test]
    fn threshold_uses_larger_dimension() {
        // Straight 5-px horizontal stroke (zero height) commits at scale 1.
        let flat = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(5.0, 0.0)];
        assert!(path_meets_threshold(&flat, 1.0));
        // 3-px stroke fails at scale 1, passes at scale 2.
        let tiny = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(3.0, 0.0)];
        assert!(!path_meets_threshold(&tiny, 1.0));
        assert!(path_meets_threshold(&tiny, 2.0));
    }

    #[test]
    fn translation_clamps_bbox_without_deforming() {
        let pts = vec![ImagePoint::new(10.0, 10.0), ImagePoint::new(20.0, 30.0)];
        // Drag far past the left edge: dx clamps to -10 (bbox min x → 0).
        let out = translated_points(&pts, ImagePoint::new(-100.0, 10.0), (0.0, 0.0), 100, 100);
        assert_eq!(out[0], ImagePoint::new(0.0, 10.0));
        assert_eq!(out[1], ImagePoint::new(10.0, 30.0));
        // Relative geometry preserved.
        assert_eq!(out[1].x - out[0].x, 10.0);
        assert_eq!(out[1].y - out[0].y, 20.0);
    }

    #[test]
    fn large_collinear_input_is_bounded_without_recursion() {
        let points: Vec<_> = (0..20_000)
            .map(|x| ImagePoint::new(x as f32, 10.0))
            .collect();
        assert_eq!(simplify_rdp(&points, 1.0).len(), 2);
    }
}
