use rollshot_capture::{Region, Size};

/// A crop rectangle in overlay logical pixels (layer-surface-local).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Map a crop rectangle from overlay logical coordinates to captured-frame
/// pixel coordinates, clamped to `source_size`. Implements spec P3.5.
pub fn map_crop_to_frame(crop: LogicalRect, overlay_logical: Size, source_size: Size) -> Region {
    if overlay_logical.width == 0 || overlay_logical.height == 0 {
        return Region {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let scale_x = source_size.width as f32 / overlay_logical.width as f32;
    let scale_y = source_size.height as f32 / overlay_logical.height as f32;

    let x = (crop.x.max(0.0) * scale_x).round() as i64;
    let y = (crop.y.max(0.0) * scale_y).round() as i64;
    let w = (crop.width.max(0.0) * scale_x).round() as i64;
    let h = (crop.height.max(0.0) * scale_y).round() as i64;

    let sw = source_size.width as i64;
    let sh = source_size.height as i64;
    let x = x.clamp(0, sw);
    let y = y.clamp(0, sh);
    let w = w.clamp(0, sw - x);
    let h = h.clamp(0, sh - y);

    Region {
        x: x as i32,
        y: y as i32,
        width: w as u32,
        height: h as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::{map_crop_to_frame, LogicalRect};
    use rollshot_capture::{Region, Size};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> LogicalRect {
        LogicalRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn maps_one_to_one_at_100_percent() {
        let out = map_crop_to_frame(
            rect(100.0, 200.0, 300.0, 400.0),
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 1920,
                height: 1080,
            },
        );
        assert_eq!(
            out,
            Region {
                x: 100,
                y: 200,
                width: 300,
                height: 400
            }
        );
    }

    #[test]
    fn scales_up_at_150_percent() {
        let out = map_crop_to_frame(
            rect(100.0, 100.0, 200.0, 200.0),
            Size {
                width: 1280,
                height: 720,
            }, // logical
            Size {
                width: 1920,
                height: 1080,
            }, // 1.5x device pixels
        );
        assert_eq!(
            out,
            Region {
                x: 150,
                y: 150,
                width: 300,
                height: 300
            }
        );
    }

    #[test]
    fn scales_up_at_125_percent() {
        let out = map_crop_to_frame(
            rect(80.0, 40.0, 160.0, 80.0),
            Size {
                width: 1536,
                height: 864,
            },
            Size {
                width: 1920,
                height: 1080,
            },
        );
        assert_eq!(
            out,
            Region {
                x: 100,
                y: 50,
                width: 200,
                height: 100
            }
        );
    }

    #[test]
    fn clamps_to_source_bounds() {
        let out = map_crop_to_frame(
            rect(1800.0, 1000.0, 400.0, 400.0),
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 1920,
                height: 1080,
            },
        );
        assert_eq!(
            out,
            Region {
                x: 1800,
                y: 1000,
                width: 120,
                height: 80
            }
        );
    }

    #[test]
    fn zero_overlay_size_yields_empty_region() {
        let out = map_crop_to_frame(
            rect(10.0, 10.0, 20.0, 20.0),
            Size {
                width: 0,
                height: 0,
            },
            Size {
                width: 1920,
                height: 1080,
            },
        );
        assert_eq!(
            out,
            Region {
                x: 0,
                y: 0,
                width: 0,
                height: 0
            }
        );
    }
}
