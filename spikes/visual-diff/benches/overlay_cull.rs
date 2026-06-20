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
    id: usize, // stable identity for before/after diff
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

/// Deterministic candidates spread across [0, img_w) × [0, img_h).
/// `id_offset` shifts the stable identity, allowing before/after sets to differ.
fn make_candidates_with_offset(count: usize, img_w: f32, img_h: f32, id_offset: usize) -> Vec<Rect> {
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        let id = i + id_offset;
        let frac_x = (id as f32 * 1.618_033) % 1.0;
        let frac_y = (id as f32 * 2.718_281) % 1.0;
        let x = frac_x * img_w;
        let y = frac_y * img_h;
        let bw = 100.0_f32;
        let bh = 24.0_f32;
        v.push(Rect {
            id,
            x,
            y,
            w: bw.min(img_w - x),
            h: bh.min(img_h - y),
        });
    }
    v
}

fn make_candidates(count: usize, img_w: f32, img_h: f32) -> Vec<Rect> {
    make_candidates_with_offset(count, img_w, img_h, 0)
}

/// Build an "after" set with genuine churn: ~20% removed, ~20% added, ~60% kept.
/// `before` has IDs 0..n; `after` keeps IDs 0..floor(0.8*n) and adds IDs n..n+floor(0.2*n).
/// Both slices are sorted by id, so the O(n+m) merge in bench_diff exercises all three branches.
fn make_candidates_after(before: &[Rect], img_w: f32, img_h: f32) -> Vec<Rect> {
    let n = before.len();
    let keep = (n * 4 / 5).max(1);   // ~80% kept
    let add  = n - keep;             // ~20% new
    let mut v: Vec<Rect> = before[..keep].to_vec();
    // New candidates get IDs starting at n (never appear in before)
    let new_cands = make_candidates_with_offset(add, img_w, img_h, n);
    v.extend_from_slice(&new_cands);
    // v is already sorted by id: kept ids 0..keep, then new ids n..n+add
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
        id: 0,
        x: 0.0,
        y: 0.0,
        w: 1920.0_f32.min(img_w),
        h: 1080.0_f32.min(img_h),
    }
}

/// Viewport scrolled to mid-document (y ≈ img_h/2).
fn mid_document_rect(img_w: f32, img_h: f32) -> Rect {
    let vw = 1920.0_f32.min(img_w);
    let vh = 1080.0_f32.min(img_h);
    let y = ((img_h - vh) / 2.0).max(0.0);
    Rect { id: 0, x: 0.0, y, w: vw, h: vh }
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
    // Both slices must be sorted by id (guaranteed by make_candidates* helpers).
    // O(n+m) sorted-merge set diff: exercises added, removed, and kept branches.
    let before_len = before.len();
    let after_len = after.len();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut kept = Vec::new();

    let mut bi = 0usize;
    let mut ai = 0usize;
    while bi < before_len && ai < after_len {
        let b_id = before[bi].id;
        let a_id = after[ai].id;
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
        removed.push(before[bi].id);
        bi += 1;
    }
    while ai < after_len {
        added.push(after[ai].id);
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
    // Mid-document scroll case for the tall image (viewport at y≈img_h/2).
    let tall = &IMAGES[1]; // tall_4000x12000
    for &n in COUNTS {
        let candidates = make_candidates(n, tall.w, tall.h);
        let viewport = mid_document_rect(tall.w, tall.h);
        let id = BenchmarkId::new("tall_4000x12000_mid", n);
        group.bench_with_input(id, &n, |b, _| {
            b.iter(|| bench_cull(black_box(&candidates), black_box(&viewport)));
        });
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
            // after: ~80% kept (ids 0..keep), ~20% removed (ids keep..n), ~20% new (ids n..n+add).
            // Exercises all three branches of the sorted-merge diff (added + removed + kept).
            let after = make_candidates_after(&before, img.w, img.h);
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
