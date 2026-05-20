use image::RgbaImage;

/// Number of horizontal samples taken per row when building a duplicate signature.
const SIGNATURE_COLS: u32 = 18;
/// Number of rows sampled when building a duplicate signature.
const SIGNATURE_ROWS: u32 = 24;

/// Builds a tiny grayscale signature of the frame for cheap duplicate detection.
///
/// The frame is sampled on a fixed `rows x cols` grid (no smoothing). The result
/// is a stable fingerprint that catches frames the user has not scrolled.
pub fn signature(frame: &RgbaImage) -> Vec<u8> {
    sample(frame, SIGNATURE_COLS, SIGNATURE_ROWS)
}

/// Returns `true` when the mean absolute difference between two signatures,
/// normalized into the `[0.0, 1.0]` range, is at or below `threshold`.
pub fn is_duplicate(prev: &[u8], curr: &[u8], threshold: f32) -> bool {
    if prev.len() != curr.len() || prev.is_empty() {
        return false;
    }

    let mut sum = 0.0f32;
    for (&a, &b) in prev.iter().zip(curr.iter()) {
        sum += a.abs_diff(b) as f32;
    }

    let mad = sum / (prev.len() as f32 * 255.0);
    mad <= threshold
}

fn sample(frame: &RgbaImage, cols: u32, rows: u32) -> Vec<u8> {
    let width = frame.width().max(1);
    let height = frame.height().max(1);
    let cols = cols.max(1);
    let rows = rows.max(1);
    let mut out = Vec::with_capacity((cols * rows) as usize);

    for row in 0..rows {
        let y = ((row * height) / rows).min(height - 1);
        for col in 0..cols {
            let x = ((col * width) / cols).min(width - 1);
            let p = frame.get_pixel(x, y);
            let gray = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
            out.push(gray as u8);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{is_duplicate, signature};
    use image::{Rgba, RgbaImage};

    fn checkerboard(width: u32, height: u32, shift: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            let on = ((x + shift) ^ y) & 1 == 0;
            if on {
                Rgba([220, 220, 220, 255])
            } else {
                Rgba([20, 20, 20, 255])
            }
        })
    }

    #[test]
    fn identical_frames_are_duplicates() {
        let frame = checkerboard(64, 64, 0);
        let sig_a = signature(&frame);
        let sig_b = signature(&frame);

        assert!(is_duplicate(&sig_a, &sig_b, 0.01));
    }

    #[test]
    fn very_different_frames_are_not_duplicates() {
        let a = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
        let b = RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255]));

        let sig_a = signature(&a);
        let sig_b = signature(&b);

        assert!(!is_duplicate(&sig_a, &sig_b, 0.01));
    }

    #[test]
    fn mismatched_signature_lengths_are_not_duplicates() {
        let short = vec![10u8; 4];
        let long = vec![10u8; 8];

        assert!(!is_duplicate(&short, &long, 0.5));
    }

    #[test]
    fn signature_length_matches_grid_size() {
        let frame = checkerboard(64, 64, 0);
        let sig = signature(&frame);

        assert_eq!(sig.len(), 18 * 24);
    }
}
