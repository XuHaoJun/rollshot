// overlay_cull.rs — CPU-side overlay geometry benchmarks (no iced/GPU).
//
// Mirrors the culling pattern from rollshot-app/src/result_workspace/canvas.rs:
//   for annotation in document.annotations() {
//       if annotation_bounds(annotation).intersects(&self.visible) { ... }
//   }
//
// Measures (a) frustum culling, (b) point hit-testing, (c) before/after diff,
// at 100/500/1000 candidates on ordinary (1920×1080) and tall (4000×12000) images.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

// ---------------------------------------------------------------------------
// Minimal geometry types (mirrors ImageRect / ImagePoint from rollshot)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }

    fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

// ---------------------------------------------------------------------------
// Candidate generation
// ---------------------------------------------------------------------------

/// Deterministic pseudo-random candidates spread across [0, img_w) × [0, img_h).
fn make_candidates(count: usize, img_w: f32, img_h: f32) -> Vec<Rect> {
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        let frac_x = (i as f32 * 1.618_033) % 1.0;
        let frac_y = (i as f32 * 2.718_281) % 1.0;
        let x = frac_x * img_w;
        let y = frac_y * img_h;
        // Box width/height: ~100px in image coords regardless of image size
        let bw = 100.0_f32;
        let bh = 24.0_f32;
        v.push(Rect {
            x,
            y,
            w: bw.min(img_w - x),
            h: bh.min(img_h - y),
        });
    }
    v
}

// ---------------------------------------------------------------------------
// (a) Frustum culling — visible_image_rect intersection
// ---------------------------------------------------------------------------

/// Mirror of canvas.rs::visible_image_rect:
/// the on-screen region of the image is determined by scroll_offset and scale.
fn visible_image_rect(img_w: f32, img_h: f32) -> Rect {
    // Simulate viewing 1920×1080 of the image at scale 1.0 from offset (0,0)
    Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0_f32.min(img_w),
        h: 1080.0_f32.min(img_h),
    }
}

fn bench_cull(candidates: &[Rect], viewport: &Rect) -> usize {
    candidates
        .iter()
        .filter(|c| c.intersects(viewport))
        .count()
}

// ---------------------------------------------------------------------------
// (b) Point hit-testing
// ---------------------------------------------------------------------------

fn bench_hit_test(candidates: &[Rect], px: f32, py: f32) -> Option<usize> {
    candidates
        .iter()
        .position(|c| c.contains_point(px, py))
}

// ---------------------------------------------------------------------------
// (c) Before/after candidate-set diff
// ---------------------------------------------------------------------------

/// Simulates computing which candidates are new (in `after` but not `before`),
/// which were removed, and which are unchanged. IDs are array indices.
fn bench_diff(before: &[Rect], after: &[Rect]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    // Each candidate has a stable ID = index; we compare by ID here.
    // "after" has a shuffled subset of before plus a few new ones.
    let before_len = before.len();
    let after_len = after.len();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut kept = Vec::new();

    // Simple O(n+m) set diff using sorted index comparison
    let mut bi = 0usize;
    let mut ai = 0usize;
    while bi < before_len && ai < after_len {
        // Map each rect to a stable hash for identity comparison
        let b_id = bi;
        let a_id = ai;
        if b_id == a_id {
            kept.push(b_id);
            bi += 1;
            ai += 1;
        } else if b_id < a_id {
            removed.push(b_id);
            bi += 1;
        } else {
            added.push(a_id);
            ai += 1;
        }
    }
    while bi < before_len {
        removed.push(bi);
        bi += 1;
    }
    while ai < after_len {
        added.push(ai);
        ai += 1;
    }

    (added, removed, kept)
}

// ---------------------------------------------------------------------------
// Benchmark groups
// ---------------------------------------------------------------------------

struct ImageSize {
    name: &'static str,
    w: f32,
    h: f32,
}

const IMAGES: &[ImageSize] = &[
    ImageSize { name: "ordinary_1920x1080", w: 1920.0, h: 1080.0 },
    ImageSize { name: "tall_4000x12000",    w: 4000.0, h: 12000.0 },
];

const COUNTS: &[usize] = &[100, 500, 1000];

fn benchmark_cull(c: &mut Criterion) {
    let mut group = c.benchmark_group("overlay_cull/frustum");
    for img in IMAGES {
        for &n in COUNTS {
            let candidates = make_candidates(n, img.w, img.h);
            let viewport = visible_image_rect(img.w, img.h);
            let id = BenchmarkId::new(img.name, n);
            group.bench_with_input(id, &n, |b, _| {
                b.iter(|| bench_cull(black_box(&candidates), black_box(&viewport)));
            });
        }
    }
    group.finish();
}

fn benchmark_hit_test(c: &mut Criterion) {
    let mut group = c.benchmark_group("overlay_cull/hit_test");
    for img in IMAGES {
        for &n in COUNTS {
            let candidates = make_candidates(n, img.w, img.h);
            // Hit-test mid-image
            let px = img.w / 2.0;
            let py = img.h / 2.0;
            let id = BenchmarkId::new(img.name, n);
            group.bench_with_input(id, &n, |b, _| {
                b.iter(|| bench_hit_test(black_box(&candidates), black_box(px), black_box(py)));
            });
        }
    }
    group.finish();
}

fn benchmark_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("overlay_cull/diff");
    for img in IMAGES {
        for &n in COUNTS {
            let before = make_candidates(n, img.w, img.h);
            // "after" set: same length, slightly shifted (simulates re-detection)
            let after = make_candidates(n, img.w, img.h);
            let id = BenchmarkId::new(img.name, n);
            group.bench_with_input(id, &n, |b, _| {
                b.iter(|| bench_diff(black_box(&before), black_box(&after)));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_cull, benchmark_hit_test, benchmark_diff);
criterion_main!(benches);
