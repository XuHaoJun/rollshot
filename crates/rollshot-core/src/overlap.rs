//! Generic 2D overlap rectangle math, independent of any matcher.

use crate::types::OverlapRegion;

/// Computes the rectangular overlap between `prev` and `curr` frames given a
/// candidate motion `(dx, dy)` where the current frame's top-left sits at
/// `(dx, dy)` in the previous frame's content coordinate space.
///
/// Returns `None` when there is no positive-area overlap.
pub fn compute_overlap(
    prev_w: u32,
    prev_h: u32,
    curr_w: u32,
    curr_h: u32,
    dx: i32,
    dy: i32,
) -> Option<OverlapRegion> {
    let prev_w_i = prev_w as i32;
    let prev_h_i = prev_h as i32;
    let curr_w_i = curr_w as i32;
    let curr_h_i = curr_h as i32;

    let x_lo = dx.max(0);
    let y_lo = dy.max(0);
    let x_hi = (dx + curr_w_i).min(prev_w_i);
    let y_hi = (dy + curr_h_i).min(prev_h_i);

    if x_hi <= x_lo || y_hi <= y_lo {
        return None;
    }

    let width = (x_hi - x_lo) as u32;
    let height = (y_hi - y_lo) as u32;
    let prev_x = x_lo as u32;
    let prev_y = y_lo as u32;
    let curr_x = (x_lo - dx) as u32;
    let curr_y = (y_lo - dy) as u32;

    Some(OverlapRegion {
        prev_x,
        prev_y,
        curr_x,
        curr_y,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::compute_overlap;

    #[test]
    fn vertical_down_overlap_lives_at_bottom_of_prev_and_top_of_curr() {
        let r = compute_overlap(100, 100, 100, 100, 0, 30).expect("overlap exists");
        assert_eq!(r.prev_x, 0);
        assert_eq!(r.prev_y, 30);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 100);
        assert_eq!(r.height, 70);
    }

    #[test]
    fn vertical_up_overlap_lives_at_top_of_prev_and_bottom_of_curr() {
        let r = compute_overlap(100, 100, 100, 100, 0, -25).expect("overlap exists");
        assert_eq!(r.prev_x, 0);
        assert_eq!(r.prev_y, 0);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 25);
        assert_eq!(r.width, 100);
        assert_eq!(r.height, 75);
    }

    #[test]
    fn horizontal_right_overlap_lives_at_right_of_prev_and_left_of_curr() {
        let r = compute_overlap(120, 80, 120, 80, 40, 0).expect("overlap exists");
        assert_eq!(r.prev_x, 40);
        assert_eq!(r.prev_y, 0);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 80);
    }

    #[test]
    fn horizontal_left_overlap_lives_at_left_of_prev_and_right_of_curr() {
        let r = compute_overlap(120, 80, 120, 80, -50, 0).expect("overlap exists");
        assert_eq!(r.prev_x, 0);
        assert_eq!(r.prev_y, 0);
        assert_eq!(r.curr_x, 50);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 70);
        assert_eq!(r.height, 80);
    }

    #[test]
    fn motion_larger_than_frame_yields_no_overlap() {
        assert!(compute_overlap(100, 100, 100, 100, 0, 200).is_none());
        assert!(compute_overlap(100, 100, 100, 100, 0, -200).is_none());
        assert!(compute_overlap(100, 100, 100, 100, 200, 0).is_none());
        assert!(compute_overlap(100, 100, 100, 100, -200, 0).is_none());
    }

    #[test]
    fn zero_motion_returns_full_frame_overlap() {
        let r = compute_overlap(80, 60, 80, 60, 0, 0).expect("overlap exists");
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 60);
        assert_eq!((r.prev_x, r.prev_y, r.curr_x, r.curr_y), (0, 0, 0, 0));
    }

    #[test]
    fn diagonal_motion_returns_inner_rectangle() {
        let r = compute_overlap(100, 100, 100, 100, 20, 30).expect("overlap exists");
        assert_eq!(r.prev_x, 20);
        assert_eq!(r.prev_y, 30);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 70);
    }
}
