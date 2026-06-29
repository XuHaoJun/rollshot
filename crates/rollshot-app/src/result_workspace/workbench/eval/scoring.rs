use rollshot_image_document::ImageRect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ExpectedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Thresholds {
    pub min_coverage: f32,
    pub max_false_positive_ratio: f32,
}

impl Thresholds {
    pub fn lenient() -> Self {
        Self {
            min_coverage: 0.6,
            max_false_positive_ratio: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreReport {
    pub per_rect_coverage: Vec<(String, f32)>,
    pub min_coverage: f32,
    pub false_positive_ratio: f32,
    pub candidate_count: usize,
    pub gate_failures: Vec<String>,
}

impl ScoreReport {
    pub fn passed(&self) -> bool {
        self.gate_failures.is_empty()
    }
}

fn clipped_rect(a: &ImageRect, bx: f32, by: f32, bw: f32, bh: f32) -> Option<ImageRect> {
    let x0 = a.x.max(bx);
    let y0 = a.y.max(by);
    let x1 = (a.x + a.width).min(bx + bw);
    let y1 = (a.y + a.height).min(by + bh);
    let width = x1 - x0;
    let height = y1 - y0;
    (width > 0.0 && height > 0.0).then_some(ImageRect {
        x: x0,
        y: y0,
        width,
        height,
    })
}

fn rect_union_area(rects: &[ImageRect]) -> f32 {
    let rects: Vec<ImageRect> = rects
        .iter()
        .copied()
        .filter(|r| r.width > 0.0 && r.height > 0.0)
        .collect();
    if rects.is_empty() {
        return 0.0;
    }

    let mut xs = Vec::with_capacity(rects.len() * 2);
    let mut ys = Vec::with_capacity(rects.len() * 2);
    for r in &rects {
        xs.push(r.x);
        xs.push(r.x + r.width);
        ys.push(r.y);
        ys.push(r.y + r.height);
    }
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    ys.sort_by(f32::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    let mut area = 0.0;
    for xw in xs.windows(2) {
        for yw in ys.windows(2) {
            let (x0, x1) = (xw[0], xw[1]);
            let (y0, y1) = (yw[0], yw[1]);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let covered = rects
                .iter()
                .any(|r| r.x <= x0 && r.x + r.width >= x1 && r.y <= y0 && r.y + r.height >= y1);
            if covered {
                area += (x1 - x0) * (y1 - y0);
            }
        }
    }
    area
}

fn coverage_of(expected: &ExpectedRect, candidates: &[ImageRect]) -> f32 {
    let area = expected.width * expected.height;
    if area <= 0.0 {
        return 0.0;
    }
    let clipped: Vec<ImageRect> = candidates
        .iter()
        .filter_map(|c| clipped_rect(c, expected.x, expected.y, expected.width, expected.height))
        .collect();
    let covered = rect_union_area(&clipped);
    (covered / area).min(1.0)
}

pub(crate) fn score_candidates(
    expected: &[ExpectedRect],
    candidates: &[ImageRect],
    thresholds: &Thresholds,
) -> ScoreReport {
    let per_rect_coverage: Vec<(String, f32)> = expected
        .iter()
        .map(|e| (e.label.clone(), coverage_of(e, candidates)))
        .collect();
    let min_coverage = per_rect_coverage
        .iter()
        .map(|(_, c)| *c)
        .fold(f32::INFINITY, f32::min);
    let min_coverage = if min_coverage.is_finite() {
        min_coverage
    } else {
        0.0
    };

    let total_expected_area: f32 = expected.iter().map(|e| e.width * e.height).sum();
    let total_candidate_area = rect_union_area(candidates);
    let clipped_inside: Vec<ImageRect> = candidates
        .iter()
        .flat_map(|c| {
            expected
                .iter()
                .filter_map(|e| clipped_rect(c, e.x, e.y, e.width, e.height))
        })
        .collect();
    let inside_area = rect_union_area(&clipped_inside);
    let outside_area = (total_candidate_area - inside_area).max(0.0);
    let false_positive_ratio = if total_expected_area > 0.0 {
        outside_area / total_expected_area
    } else {
        0.0
    };

    let mut gate_failures = Vec::new();
    if min_coverage < thresholds.min_coverage {
        gate_failures.push(format!(
            "coverage {min_coverage:.3} < {:.3}",
            thresholds.min_coverage
        ));
    }
    if false_positive_ratio > thresholds.max_false_positive_ratio {
        gate_failures.push(format!(
            "false_positive {false_positive_ratio:.3} > {:.3}",
            thresholds.max_false_positive_ratio
        ));
    }

    ScoreReport {
        per_rect_coverage,
        min_coverage,
        false_positive_ratio,
        candidate_count: candidates.len(),
        gate_failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width: w,
            height: h,
        }
    }
    fn expected(label: &str, x: f32, y: f32, w: f32, h: f32) -> ExpectedRect {
        ExpectedRect {
            x,
            y,
            width: w,
            height: h,
            label: label.into(),
        }
    }

    #[test]
    fn full_cover_no_false_positive_passes() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 100.0, 10.0)];
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert_eq!(report.min_coverage, 1.0);
        assert_eq!(report.false_positive_ratio, 0.0);
        assert!(report.passed(), "{:?}", report.gate_failures);
    }

    #[test]
    fn missed_rect_fails_coverage_gate() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 40.0, 10.0)]; // 40% coverage
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert!((report.min_coverage - 0.4).abs() < 1e-4);
        assert!(!report.passed());
        assert!(report.gate_failures.iter().any(|f| f.contains("coverage")));
    }

    #[test]
    fn excess_area_counts_as_false_positive() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        // covers the bar fully, plus a 100x10 region entirely outside it
        let cands = vec![rect(0.0, 0.0, 100.0, 10.0), rect(0.0, 50.0, 100.0, 10.0)];
        let mut th = Thresholds::lenient();
        th.max_false_positive_ratio = 0.5;
        let report = score_candidates(&exp, &cands, &th);
        assert!((report.false_positive_ratio - 1.0).abs() < 1e-4);
        assert!(!report.passed());
        assert!(report
            .gate_failures
            .iter()
            .any(|f| f.contains("false_positive")));
    }

    #[test]
    fn overlapping_candidates_do_not_double_count_coverage() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 60.0, 10.0), rect(40.0, 0.0, 60.0, 10.0)];
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert!((report.min_coverage - 1.0).abs() < 1e-4);
        assert!((report.false_positive_ratio - 0.0).abs() < 1e-4);
    }

    #[test]
    fn zero_area_expected_rect_fails_cleanly() {
        let exp = vec![expected("empty", 0.0, 0.0, 0.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 100.0, 10.0)];
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert_eq!(report.min_coverage, 0.0);
        assert!(!report.passed());
    }
}
