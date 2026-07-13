use rollshot_image_document::ImagePoint;

pub const MIN_GESTURE_SCREEN: f32 = 4.0;

pub fn snap_endpoint(fixed: ImagePoint, moving: ImagePoint) -> ImagePoint {
    let dx = moving.x - fixed.x;
    let dy = moving.y - fixed.y;
    let distance = fixed.distance(moving);
    if distance == 0.0 {
        return moving;
    }
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    ImagePoint::new(
        fixed.x + angle.cos() * distance,
        fixed.y + angle.sin() * distance,
    )
}

pub fn constrained_endpoint(fixed: ImagePoint, moving: ImagePoint, shift: bool) -> ImagePoint {
    if shift {
        snap_endpoint(fixed, moving)
    } else {
        moving
    }
}

pub fn bounded_constrained_endpoint(
    fixed: ImagePoint,
    moving: ImagePoint,
    shift: bool,
    width: u32,
    height: u32,
) -> ImagePoint {
    let moving = moving.clamp_to(width, height);
    let constrained = constrained_endpoint(fixed, moving, shift);
    let delta_x = constrained.x - fixed.x;
    let delta_y = constrained.y - fixed.y;
    let mut factor = 1.0_f32;

    if delta_x > 0.0 {
        factor = factor.min((width as f32 - fixed.x) / delta_x);
    } else if delta_x < 0.0 {
        factor = factor.min(-fixed.x / delta_x);
    }
    if delta_y > 0.0 {
        factor = factor.min((height as f32 - fixed.y) / delta_y);
    } else if delta_y < 0.0 {
        factor = factor.min(-fixed.y / delta_y);
    }

    ImagePoint::new(
        fixed.x + delta_x * factor.clamp(0.0, 1.0),
        fixed.y + delta_y * factor.clamp(0.0, 1.0),
    )
    .clamp_to(width, height)
}

pub fn gesture_meets_threshold(start: ImagePoint, end: ImagePoint, scale: f32) -> bool {
    start.distance(end) * scale >= MIN_GESTURE_SCREEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_all_octants_and_preserves_distance() {
        let fixed = ImagePoint::new(100.0, 100.0);
        for moving in [
            ImagePoint::new(140.0, 103.0),
            ImagePoint::new(138.0, 139.0),
            ImagePoint::new(97.0, 140.0),
            ImagePoint::new(61.0, 138.0),
            ImagePoint::new(60.0, 97.0),
            ImagePoint::new(62.0, 61.0),
            ImagePoint::new(103.0, 60.0),
            ImagePoint::new(139.0, 62.0),
        ] {
            let snapped = snap_endpoint(fixed, moving);
            assert!((fixed.distance(snapped) - fixed.distance(moving)).abs() < 0.001);
            let angle = (snapped.y - fixed.y).atan2(snapped.x - fixed.x);
            let eighth_turn = std::f32::consts::FRAC_PI_4;
            assert!((angle / eighth_turn - (angle / eighth_turn).round()).abs() < 0.001);
        }
    }

    #[test]
    fn constraint_only_snaps_when_shift_is_held() {
        let fixed = ImagePoint::new(10.0, 10.0);
        let moving = ImagePoint::new(30.0, 17.0);
        assert_eq!(constrained_endpoint(fixed, moving, false), moving);
        assert_eq!(
            constrained_endpoint(fixed, moving, true),
            snap_endpoint(fixed, moving)
        );
    }

    #[test]
    fn bounded_shift_constraint_stays_on_snapped_ray() {
        let fixed = ImagePoint::new(90.0, 50.0);
        let endpoint =
            bounded_constrained_endpoint(fixed, ImagePoint::new(100.0, 70.0), true, 100, 100);

        assert_eq!(endpoint, ImagePoint::new(100.0, 60.0));
        assert_eq!(endpoint.x - fixed.x, endpoint.y - fixed.y);
    }

    #[test]
    fn four_screen_pixel_threshold_is_zoom_independent() {
        let start = ImagePoint::new(10.0, 10.0);
        assert!(!gesture_meets_threshold(
            start,
            ImagePoint::new(13.9, 10.0),
            1.0
        ));
        assert!(gesture_meets_threshold(
            start,
            ImagePoint::new(14.0, 10.0),
            1.0
        ));
        assert!(!gesture_meets_threshold(
            start,
            ImagePoint::new(17.9, 10.0),
            0.5
        ));
        assert!(gesture_meets_threshold(
            start,
            ImagePoint::new(18.0, 10.0),
            0.5
        ));
    }
}
