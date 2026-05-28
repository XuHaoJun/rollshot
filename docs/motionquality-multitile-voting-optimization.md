# rollshot — MotionQuality + Multi-tile Voting Optimization

> Status: proposed as a **P5.5 / P5.6** optimization after the failed default `True Image Pyramid` attempt.
>
> Goal: improve rollshot's motion-estimation reliability and observability without replacing the existing
> `coarse → NCC → verifier → append` architecture, and without polluting the common small-frame hot path.

---

## 0. Executive summary

The next optimization should be **MotionQuality + Multi-tile voting before P6 Indexed Feature Fallback / HNSW**.

P6 HNSW is still useful, but its benefit is mostly limited to miss/fallback frames. MotionQuality is broader: it turns rollshot's existing signals—NCC score, second-best margin, verifier MAD, axis consistency, and later tile consensus—into a unified confidence model. This lets rollshot answer:

```text
I found an offset, but how trustworthy is it?
```

The recommended rollout is deliberately conservative:

```text
P5.5  MotionQuality Observe-Only
      - add diagnostics
      - no behavior change
      - output hash must remain identical

P5.6  Multi-tile Verify-Only
      - use 3–5 local tiles to validate the main candidate
      - do not create a new primary offset yet
      - only produce consensus_q

P5.7  MotionQuality Fallback Gate
      - low quality can trigger existing fallback/retry before accepting
      - still cannot bypass PixelOverlapVerifier

P6    Indexed Feature Fallback / HNSW
      - revisit after quality metrics prove fallback is frequent or expensive
```

The key rule:

```text
MotionQuality may explain or gate decisions.
Multi-tile may validate the main candidate.
Neither may bypass PixelOverlapVerifier or final verifier.
```

---

## 1. Why do this before P6 HNSW?

### 1.1 P6 is a fallback-path optimization

The current roadmap describes P6 as an indexed feature fallback that improves fallback p95/p99 latency and keeps HNSW behind a feature flag or conservative config. That is a good direction, but it mainly helps when the current matcher has already missed or fallen through to FAST+KNN.

That means P6 is valuable when these are common:

```text
coarse/NCC fail
edge projection fails
relaxed retry fails
FAST+KNN fallback is invoked
fallback latency affects p95/p99
```

It does less for the common steady-state case where NCC succeeds.

### 1.2 P5 taught an important lesson

The failed `True Image Pyramid` attempt proved a practical point:

```text
A recovery feature can work functionally and still fail as an optimization
if it slows down the common path.
```

Your P5 result was especially informative:

- `pyramid_us` p50 was 0, so the median frame did not actually run pyramid recovery.
- Search counters and output hashes did not change.
- Yet multiple small-frame common paths regressed in wall time.

That strongly suggests the danger is not just algorithmic work. It can also be:

```text
larger hot structs
extra config branches
larger hot functions
changed LLVM inlining/code layout
i-cache or branch-predictor behavior
diagnostic allocations
```

MotionQuality should therefore start as an **observe-only diagnostic layer**. Multi-tile should start as **verify-only**, not as a new search path.

### 1.3 MotionQuality makes P6 smarter

If MotionQuality exists first, P6 can be triggered only when it is actually justified:

```text
if quality is high:
    accept normal NCC path
elif quality is low but verifier passed:
    try multi-tile / existing fallback
elif quality is low and fallback is frequently used:
    then HNSW may be worth implementing
```

Without MotionQuality, P6 is blind: it accelerates a fallback path without first proving when fallback should run.

---

## 2. Existing rollshot context

rollshot already has a strong architecture:

```text
duplicate signature
  -> Prepared/current frame
  -> coarse downsampled MAD
  -> central-band NCC refinement
  -> edge projection
  -> relaxed retry
  -> FAST+KNN fallback
  -> PixelOverlapVerifier
  -> final verifier
  -> overlap-and-overwrite append
```

The important existing properties are:

1. **Streaming anchor:** each frame is matched against the last accepted frame, not the whole canvas.
2. **Central-band NCC:** template refinement uses a centered match-width band, not the full ROI.
3. **Second-best margin:** template matching records second-best score to reject periodic aliases.
4. **Two-stage verifier:** candidates must pass downsampled MAD over the overlap and full-resolution sample-band MAD.
5. **Overlap-and-overwrite:** append semantics intentionally let the newest overlap overwrite older overlap, which passively hides sticky UI.

MotionQuality should reuse those signals instead of replacing them.

---

## 3. Related evidence and design inspiration

### 3.1 Optical mouse sensors: motion plus surface quality

Optical mouse sensors are a close conceptual match. They repeatedly capture a small surface image and estimate frame-to-frame displacement. Importantly, many sensors expose not only `Delta_X` / `Delta_Y`, but also a surface-quality metric such as `SQUAL`.

In the ADNS-2080 datasheet, `SQUAL` is described as a measure of the number of valid features visible by the sensor in the current frame. That is exactly the kind of signal rollshot currently lacks as a unified concept: rollshot can estimate an offset, but it should also expose "how much valid evidence supported that offset."

Implication for rollshot:

```text
motion = dx/dy
quality = how reliable this motion estimate is
```

### 3.2 PIV: interrogation windows and peak-ratio confidence

Particle Image Velocimetry (PIV) estimates motion between image pairs by dividing images into local interrogation windows and using cross-correlation to find the likely displacement. OpenPIV and PIV literature describe this window-based cross-correlation model as the core mechanism for obtaining velocity fields from image pairs.

This maps naturally to rollshot multi-tile voting:

```text
PIV interrogation windows  -> rollshot content tiles
PIV local displacement     -> tile offset vote
PIV peak ratio / SNR       -> margin_q / peak_sharpness_q
```

Xue et al. discuss the primary peak ratio (PPR), defined as the ratio between the primary correlation peak and the second tallest peak, as a signal-to-noise measure for PIV cross-correlation. This directly supports rollshot's use of second-best margin and a future `margin_q`.

### 3.3 DIC / sub-pixel correlation: peak shape matters

Digital Image Correlation (DIC) and PIV both use correlation-peak interpolation and uncertainty estimation. A common lesson is that the highest score alone is not enough; the shape and ambiguity of the correlation peak matter.

For rollshot:

```text
A high NCC score is not enough.
The best peak must also be clearly separated and reasonably sharp.
```

That motivates:

```text
peak_q
margin_q
peak_sharpness_q
```

### 3.4 Fast NCC: multi-tile must reuse the optimized NCC path

Lewis' fast normalized cross-correlation shows that normalized cross-correlation can be accelerated using precomputed integral tables for image and image². Your roadmap already includes integral statistics + SIMD cross term. Multi-tile must reuse that fast NCC machinery. It must not reintroduce slow per-tile full NCC.

### 3.5 HNSW remains useful, but after quality gating

HNSW is a strong approximate nearest-neighbor index. Malkov and Yashunin describe HNSW as a graph-based ANN structure with hierarchical proximity graphs and logarithmic complexity scaling. This is useful for P6, but only if feature fallback is frequent or expensive enough to justify index maintenance.

MotionQuality should answer that first.

---

## 4. Non-goals

This optimization is **not**:

```text
not a replacement for NCC
not a replacement for PixelOverlapVerifier
not a replacement for FAST+KNN fallback
not a new default feature matcher
not a deep optical-flow system
not a sub-pixel resampler
not a reason to accept candidates that fail verification
```

Multi-tile voting is not allowed to say:

```text
tile votes agree, so append even though verifier failed
```

The correct rule is:

```text
verifier failure always wins
```

---

## 5. High-level architecture

After this optimization, the matcher should conceptually look like:

```text
estimate_motion()
  -> normal candidate generation
       coarse MAD
       NCC refine
       edge projection
       relaxed retry
       feature fallback if needed
  -> rank_verified_candidates()
  -> PixelOverlapVerifier diagnostics
  -> MotionQuality diagnostics
  -> optional Multi-tile Verify-Only diagnostics
  -> candidate decision
  -> final verifier in Stitcher
  -> append
```

First rollout:

```text
MotionQualityMode::ObserveOnly
MultiTileMode::Disabled
```

Second rollout:

```text
MotionQualityMode::ObserveOnly
MultiTileMode::VerifyOnly
```

Third rollout:

```text
MotionQualityMode::FallbackGate
MultiTileMode::VerifyOnly
```

Only later, if benchmarks justify it:

```text
MotionQualityMode::Ranking
MultiTileMode::CandidateSource
```

---

## 6. Config design

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionQualityMode {
    Disabled,

    /// Compute and record quality, but do not affect decisions.
    ObserveOnly,

    /// Low-quality accepted candidates may trigger additional fallback/retry
    /// before being accepted. Still cannot bypass verifier.
    FallbackGate,

    /// Experimental: quality participates in candidate ranking.
    Ranking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiTileMode {
    Disabled,

    /// Validate the already-selected main candidate near its offset.
    VerifyOnly,

    /// Experimental: tiles may create their own candidate.
    CandidateSource,
}

#[derive(Debug, Clone)]
pub struct MotionQualityConfig {
    pub mode: MotionQualityMode,

    pub high_threshold: f32,   // default 0.80
    pub medium_threshold: f32, // default 0.60
    pub low_threshold: f32,    // default 0.40

    pub min_axis_q: f32,       // default 0.05
    pub min_margin_q: f32,     // default 0.05

    pub ncc_score_accept: f32, // default config.accept_confidence
    pub margin_low: f32,       // default 0.002
    pub margin_high: f32,      // default 0.030

    pub verifier_a_weight: f32, // default 0.40
    pub verifier_b_weight: f32, // default 0.60

    pub multi_tile: MultiTileConfig,
}

#[derive(Debug, Clone)]
pub struct MultiTileConfig {
    pub mode: MultiTileMode,

    /// Number of tiles on the cross-scroll axis.
    /// Vertical scroll: horizontal tile count.
    /// Horizontal scroll: vertical tile count.
    pub tile_count: usize, // default 3

    /// Tile size cap on the cross axis.
    pub tile_cross_px: u32, // default 256

    /// Fraction of content ROI used along the scroll axis.
    pub tile_main_ratio: f32, // default 0.60

    /// Where to place the tile band within the ROI along the scroll axis.
    /// 0.20 means start around 20% into the ROI.
    pub tile_main_start_ratio: f32, // default 0.20

    /// Verify only near the already-selected main offset.
    pub verify_radius_px: i32, // default 4 or 6

    /// A tile agrees if its local best offset is within this distance.
    pub agree_tolerance_px: i32, // default 2

    /// Do not run multi-tile if the frame is too small.
    pub min_tile_area: u32, // default 64 * 64

    /// Keep hard budget small.
    pub max_tile_ncc_calls: usize, // default tile_count * (2*radius+1)
}

impl Default for MotionQualityConfig {
    fn default() -> Self {
        Self {
            mode: MotionQualityMode::ObserveOnly,

            high_threshold: 0.80,
            medium_threshold: 0.60,
            low_threshold: 0.40,

            min_axis_q: 0.05,
            min_margin_q: 0.05,

            ncc_score_accept: 0.15,
            margin_low: 0.002,
            margin_high: 0.030,

            verifier_a_weight: 0.40,
            verifier_b_weight: 0.60,

            multi_tile: MultiTileConfig::default(),
        }
    }
}

impl Default for MultiTileConfig {
    fn default() -> Self {
        Self {
            mode: MultiTileMode::Disabled,
            tile_count: 3,
            tile_cross_px: 256,
            tile_main_ratio: 0.60,
            tile_main_start_ratio: 0.20,
            verify_radius_px: 4,
            agree_tolerance_px: 2,
            min_tile_area: 64 * 64,
            max_tile_ncc_calls: 3 * 9,
        }
    }
}
```

---

## 7. MotionQuality data structures

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionConfidence {
    High,
    Medium,
    Low,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    CoarseMad,
    TemplateNcc,
    EdgeProjection,
    RelaxedCoarse,
    FeatureFastKnn,
    Akaze,
    PyramidRecovery,
    PhaseCorrelation,
}

#[derive(Debug, Clone)]
pub struct MotionQuality {
    pub texture_q: Option<f32>,
    pub peak_q: f32,
    pub margin_q: f32,
    pub sharpness_q: Option<f32>,
    pub verifier_q: f32,
    pub axis_q: f32,
    pub consensus_q: Option<f32>,
    pub feature_q: Option<f32>,

    pub combined_q: f32,
    pub level: MotionConfidence,

    /// Optional reason when a hard gate clamps quality.
    pub hard_gate: Option<QualityHardGate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityHardGate {
    VerifierFailed,
    AxisInconsistent,
    AmbiguousPeak,
    InsufficientTextureAndConsensus,
}

#[derive(Debug, Clone)]
pub struct CandidateDiagnostics {
    pub source: CandidateSource,
    pub dx: i32,
    pub dy: i32,

    /// rollshot's lower-is-better score, when available.
    pub score: Option<f32>,

    /// NCC higher-is-better score, when available.
    pub ncc: Option<f32>,
    pub second_best_score: Option<f32>,

    pub verifier: VerifierDiagnostics,
    pub axis: AxisDiagnostics,

    pub cost_curve: Option<CostCurve>,
    pub tile: Option<MultiTileDiagnostics>,
    pub feature: Option<FeatureDiagnostics>,
}

#[derive(Debug, Clone)]
pub struct VerifierDiagnostics {
    pub passed: bool,
    pub pass_a_mad: f32,
    pub pass_b_mad: f32,
    pub pass_a_threshold: f32,
    pub pass_b_threshold: f32,
}

#[derive(Debug, Clone)]
pub struct AxisDiagnostics {
    pub locked_axis: Option<Axis>,
    pub main_px: i32,
    pub cross_px: i32,
    pub max_cross_axis_px: i32,
}
```

---

## 8. Quality scoring algorithms

### 8.1 Utility functions

```rust
#[inline]
fn clamp01(x: f32) -> f32 {
    x.max(0.0).min(1.0)
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }

    let t = clamp01((x - edge0) / (edge1 - edge0));
    t * t * (3.0 - 2.0 * t)
}
```

### 8.2 `peak_q`

For rollshot's lower-is-better confidence score:

```rust
fn peak_quality_from_lower_score(score: f32, accept_confidence: f32) -> f32 {
    clamp01(1.0 - score / accept_confidence)
}
```

For direct NCC, higher is better:

```rust
fn peak_quality_from_ncc(ncc: f32) -> f32 {
    smoothstep(0.72, 0.96, ncc)
}
```

### 8.3 `margin_q`

For lower-is-better score:

```rust
fn margin_quality_lower_is_better(
    best_score: f32,
    second_best_score: Option<f32>,
    margin_low: f32,
    margin_high: f32,
) -> f32 {
    let Some(second) = second_best_score else {
        return 0.5;
    };

    let margin = second - best_score;
    smoothstep(margin_low, margin_high, margin)
}
```

For NCC higher-is-better:

```rust
fn margin_quality_higher_is_better(
    best_ncc: f32,
    second_ncc: Option<f32>,
    margin_low: f32,
    margin_high: f32,
) -> f32 {
    let Some(second) = second_ncc else {
        return 0.5;
    };

    let margin = best_ncc - second;
    smoothstep(margin_low, margin_high, margin)
}
```

Interpretation:

```text
margin_q high:
  best candidate clearly beats the runner-up

margin_q low:
  repeated/periodic content may be ambiguous
```

### 8.4 `sharpness_q`

If a cost curve is available, measure whether the peak is sharp.

```rust
#[derive(Debug, Clone)]
pub struct CostPoint {
    pub offset: i32,
    pub score: f32, // higher is better for NCC
}

#[derive(Debug, Clone)]
pub struct CostCurve {
    pub axis: Axis,
    pub points: Vec<CostPoint>,
}

impl CostCurve {
    pub fn best_index(&self) -> Option<usize> {
        self.points
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.score.partial_cmp(&b.score).unwrap())
            .map(|(i, _)| i)
    }
}

fn peak_sharpness_quality(curve: &CostCurve) -> f32 {
    let Some(best_idx) = curve.best_index() else {
        return 0.3;
    };

    if best_idx == 0 || best_idx + 1 >= curve.points.len() {
        return 0.3;
    }

    let left = curve.points[best_idx - 1].score;
    let center = curve.points[best_idx].score;
    let right = curve.points[best_idx + 1].score;

    let sharpness = center - 0.5 * (left + right);
    smoothstep(0.002, 0.040, sharpness)
}
```

### 8.5 `verifier_q`

Verifier failure is a hard reject. If it passes, quality is based on how far both MAD scores are from their thresholds.

```rust
fn verifier_quality(v: &VerifierDiagnostics, a_weight: f32, b_weight: f32) -> f32 {
    if !v.passed {
        return 0.0;
    }

    let a = clamp01(1.0 - v.pass_a_mad / v.pass_a_threshold);
    let b = clamp01(1.0 - v.pass_b_mad / v.pass_b_threshold);

    let sum = a_weight + b_weight;
    if sum <= 1e-6 {
        return 0.5 * a + 0.5 * b;
    }

    (a_weight * a + b_weight * b) / sum
}
```

Sample-band MAD is closer to the seam, so it should usually get higher weight.

### 8.6 `axis_q`

Vertical example:

```rust
fn axis_quality(axis: &AxisDiagnostics) -> f32 {
    let cross = axis.cross_px.abs() as f32;
    let main = axis.main_px.abs() as f32;

    if main < 1.0 {
        return 0.0;
    }

    let cross_q = clamp01(1.0 - cross / axis.max_cross_axis_px.max(1) as f32);
    let dominance = main / (cross + 1.0);
    let dominance_q = smoothstep(1.5, 4.0, dominance);

    cross_q.min(dominance_q)
}
```

### 8.7 `feature_q`

Only for FAST+KNN or future HNSW fallback candidates.

```rust
#[derive(Debug, Clone)]
pub struct FeatureDiagnostics {
    pub total_features: usize,
    pub matched_pairs: usize,
    pub inliers: usize,
    pub best_bucket_votes: usize,
    pub second_bucket_votes: usize,
}

fn feature_quality(f: &FeatureDiagnostics) -> f32 {
    if f.total_features == 0 {
        return 0.0;
    }

    let match_rate = f.matched_pairs as f32 / f.total_features as f32;
    let inlier_rate = f.inliers as f32 / f.total_features as f32;

    let vote_ratio = if f.second_bucket_votes == 0 {
        999.0
    } else {
        f.best_bucket_votes as f32 / f.second_bucket_votes as f32
    };

    let match_q = smoothstep(0.05, 0.30, match_rate);
    let inlier_q = smoothstep(0.03, 0.20, inlier_rate);
    let ratio_q = smoothstep(1.2, 2.5, vote_ratio);

    0.3 * match_q + 0.4 * inlier_q + 0.3 * ratio_q
}
```

---

## 9. Combining quality scores

### 9.1 NCC/template candidates

```rust
fn combine_quality_for_ncc(
    peak_q: f32,
    margin_q: f32,
    sharpness_q: f32,
    verifier_q: f32,
    axis_q: f32,
    consensus_q: Option<f32>,
) -> f32 {
    let consensus = consensus_q.unwrap_or(0.5);

    clamp01(
        0.25 * peak_q +
        0.15 * margin_q +
        0.10 * sharpness_q +
        0.30 * verifier_q +
        0.15 * axis_q +
        0.05 * consensus
    )
}
```

### 9.2 Feature candidates

```rust
fn combine_quality_for_feature(
    feature_q: f32,
    verifier_q: f32,
    axis_q: f32,
    consensus_q: Option<f32>,
) -> f32 {
    let consensus = consensus_q.unwrap_or(0.5);

    clamp01(
        0.30 * feature_q +
        0.35 * verifier_q +
        0.20 * axis_q +
        0.15 * consensus
    )
}
```

### 9.3 Confidence levels

```rust
fn confidence_level(q: f32, cfg: &MotionQualityConfig) -> MotionConfidence {
    if q >= cfg.high_threshold {
        MotionConfidence::High
    } else if q >= cfg.medium_threshold {
        MotionConfidence::Medium
    } else if q >= cfg.low_threshold {
        MotionConfidence::Low
    } else {
        MotionConfidence::Reject
    }
}
```

---

## 10. Hard gates

Quality is not allowed to save a candidate that violates core invariants.

```rust
fn apply_hard_gates(
    mut q: MotionQuality,
    d: &CandidateDiagnostics,
    cfg: &MotionQualityConfig,
) -> MotionQuality {
    if !d.verifier.passed {
        q.combined_q = 0.0;
        q.level = MotionConfidence::Reject;
        q.hard_gate = Some(QualityHardGate::VerifierFailed);
        return q;
    }

    if q.axis_q <= cfg.min_axis_q {
        q.combined_q = 0.0;
        q.level = MotionConfidence::Reject;
        q.hard_gate = Some(QualityHardGate::AxisInconsistent);
        return q;
    }

    if matches!(d.source, CandidateSource::TemplateNcc) && q.margin_q <= cfg.min_margin_q {
        q.combined_q = q.combined_q.min(0.35);
        if q.level != MotionConfidence::Reject {
            q.level = MotionConfidence::Low;
        }
        q.hard_gate = Some(QualityHardGate::AmbiguousPeak);
    }

    q
}
```

---

## 11. Multi-tile Verify-Only

### 11.1 Purpose

The current central NCC band is efficient, but it can be fragile when the center region is:

```text
blank
animated
lazy-loaded
repeating
covered by dynamic content
not representative of the whole scrollable content
```

Multi-tile Verify-Only checks whether several local regions agree with the already-selected candidate.

It should answer:

```text
The main matcher says dy = 42.
Do other content tiles also support dy ≈ 42?
```

It should not answer yet:

```text
What offset should the whole frame use?
```

That later behavior belongs to `MultiTileMode::CandidateSource`, which should remain experimental.

### 11.2 Tile placement for vertical scroll

For vertical scroll, create tiles along the cross axis:

```text
content ROI
+--------------------------------------------------+
|                                                  |
|     [ left tile ] [ center tile ] [ right tile ] |
|                                                  |
+--------------------------------------------------+
```

Suggested initial parameters:

```text
tile_count = 3
tile_cross_px = min(256, roi_w / 4)
tile_main_h = roi_h * 0.60
tile_main_y = roi.y + roi_h * 0.20
```

For horizontal scroll, transpose the logic.

### 11.3 Tile generation pseudocode

```rust
fn build_vertical_tiles(roi: Rect, cfg: &MultiTileConfig) -> Vec<Rect> {
    let count = cfg.tile_count.max(1);
    let tile_w = cfg.tile_cross_px.min(roi.w / count as u32).max(32);
    let tile_h = ((roi.h as f32) * cfg.tile_main_ratio).round() as u32;

    if tile_w * tile_h < cfg.min_tile_area {
        return Vec::new();
    }

    let y = roi.y + ((roi.h as f32) * cfg.tile_main_start_ratio).round() as u32;
    let y = y.min(roi.y + roi.h - tile_h);

    let mut tiles = Vec::new();

    for i in 0..count {
        let t = if count == 1 {
            0.5
        } else {
            i as f32 / (count as f32 - 1.0)
        };

        let center_x = roi.x as f32 + t * roi.w as f32;
        let x = (center_x - tile_w as f32 / 2.0).round() as i32;
        let x = x.clamp(roi.x as i32, (roi.x + roi.w - tile_w) as i32) as u32;

        tiles.push(Rect { x, y, w: tile_w, h: tile_h });
    }

    tiles.dedup();
    tiles
}
```

### 11.4 Verify-only search window

If main candidate is `dy = 42`:

```text
tile offsets checked:
  38, 39, 40, 41, 42, 43, 44, 45, 46
```

Do not run full ±80 search per tile.

```rust
fn tile_offsets_around(main_offset: i32, radius: i32) -> impl Iterator<Item = i32> {
    (main_offset - radius)..=(main_offset + radius)
}
```

### 11.5 Tile vote structure

```rust
#[derive(Debug, Clone)]
pub struct TileVote {
    pub rect: Rect,
    pub best_offset: i32,
    pub best_score: f32,
    pub second_score: Option<f32>,
    pub peak_q: f32,
    pub margin_q: f32,
    pub texture_q: Option<f32>,
    pub agrees: bool,
    pub weight: f32,
}

#[derive(Debug, Clone)]
pub struct MultiTileDiagnostics {
    pub votes: Vec<TileVote>,
    pub consensus_q: f32,
    pub agreed_weight: f32,
    pub total_weight: f32,
    pub best_offsets: Vec<i32>,
}
```

### 11.6 Tile scoring

Use the same fast NCC API as the main template matcher.

```rust
fn score_tile_offsets(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    tile: Rect,
    axis: Axis,
    main_offset: i32,
    cfg: &MultiTileConfig,
    ncc: &NccWorkspace,
) -> TileVote {
    let mut scores = Vec::new();

    for off in tile_offsets_around(main_offset, cfg.verify_radius_px) {
        let (dx, dy) = match axis {
            Axis::Vertical => (0, off),
            Axis::Horizontal => (off, 0),
        };

        let score = score_shifted_rect(prev, curr, tile, dx, dy, ncc);
        scores.push((off, score));
    }

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let best = scores[0];
    let second = scores.get(1).copied();

    let peak_q = peak_quality_from_ncc(best.1);
    let margin_q = second
        .map(|s| smoothstep(0.002, 0.030, best.1 - s.1))
        .unwrap_or(0.5);

    let agrees = (best.0 - main_offset).abs() <= cfg.agree_tolerance_px;
    let weight = peak_q * margin_q;

    TileVote {
        rect: tile,
        best_offset: best.0,
        best_score: best.1,
        second_score: second.map(|s| s.1),
        peak_q,
        margin_q,
        texture_q: None,
        agrees,
        weight,
    }
}
```

### 11.7 Consensus scoring

```rust
fn consensus_quality(votes: &[TileVote]) -> (f32, f32, f32) {
    let mut agreed = 0.0;
    let mut total = 0.0;

    for v in votes {
        total += v.weight;

        if v.agrees {
            agreed += v.weight;
        }
    }

    if total <= 1e-6 {
        return (0.0, agreed, total);
    }

    (agreed / total, agreed, total)
}
```

Interpretation:

```text
consensus_q ≈ 1.0:
  most good tiles support the main candidate

consensus_q ≈ 0.5:
  mixed evidence

consensus_q ≈ 0.0:
  local evidence disagrees or tiles are too weak
```

### 11.8 When to run multi-tile

Do not run tile verification for every candidate initially. Run it only for the candidate that has already passed `rank_verified_candidates`.

Recommended initial logic:

```rust
if cfg.multi_tile.mode == MultiTileMode::VerifyOnly
    && candidate.source == CandidateSource::TemplateNcc
    && candidate.verifier.passed
{
    diagnostics.tile = Some(run_multi_tile_verify_only(...));
}
```

Do not run it for:

```text
duplicates
dimension mismatch
obvious no progress
verifier-failed candidates
every candidate in a large list
```

### 11.9 Cost budget

Initial budget should be tiny.

Example:

```text
tile_count = 3
verify_radius = 4
NCC calls = 3 * 9 = 27
```

If each tile is `~256 x 0.6*roi_h`, this is not free, but it is bounded and much smaller than full independent tile searches.

If overhead is too high:

```text
reduce tile_count to 2 or 3
reduce tile_h ratio
run only when quality is Medium/Low
run only when margin_q is suspicious
run only on selected scenarios in debug builds
```

---

## 12. Decision policy by rollout phase

### 12.1 Phase A — ObserveOnly

```rust
let quality = compute_motion_quality(...);
metrics.motion_quality = Some(quality);

// decision remains exactly the same
return old_decision;
```

Acceptance:

```text
output hash identical
outcome counts identical
p50 total regression < 1–2%
no new allocations in hot path except metrics collection when enabled
```

### 12.2 Phase B — MultiTile VerifyOnly metrics

```rust
let quality = compute_motion_quality(...);

if cfg.multi_tile.mode == MultiTileMode::VerifyOnly {
    quality.consensus_q = Some(run_multi_tile_verify_only(...).consensus_q);
}

metrics.motion_quality = Some(quality);

// decision remains exactly the same
return old_decision;
```

Acceptance:

```text
output hash identical
NCC call count increase bounded and reported
p50 regression acceptable only if mode is enabled
disabled mode must match baseline
```

### 12.3 Phase C — FallbackGate

```rust
match quality.level {
    MotionConfidence::High | MotionConfidence::Medium => {
        accept_candidate_as_before(candidate)
    }

    MotionConfidence::Low => {
        if cfg.motion_quality.mode == MotionQualityMode::FallbackGate {
            try_existing_recovery_before_accepting(candidate)
        } else {
            accept_candidate_as_before(candidate)
        }
    }

    MotionConfidence::Reject => {
        reject_candidate()
    }
}
```

Recommended first gate:

```text
Only Low quality triggers extra recovery.
Do not gate Medium yet.
```

### 12.4 Phase D — Ranking, experimental

Later only:

```rust
final_rank_score =
    old_rank_score * 0.75 +
    (1.0 - quality.combined_q) * 0.25;
```

This phase can change output and should require visual-equivalence rather than byte-identical output.

---

## 13. Interaction with existing optimizations

### 13.1 P1 StripCanvas

No conflict. MotionQuality and multi-tile operate before append. They should not affect strip compaction or full-image composition.

### 13.2 P2 PreparedFrame cache

Strong synergy. Multi-tile should read `PreparedFrame.gray_f32` and reuse existing prepared buffers. It must not re-convert RGBA to gray.

### 13.3 P3 Fast NCC

Strong requirement. Multi-tile must use the same fast NCC path. If it calls a slow standalone NCC implementation, it can erase P3's benefit.

### 13.4 P4 Axis-locked fast path

Multi-tile must respect axis lock.

Vertical scroll:

```text
tile search varies dy only
dx is fixed at 0 or checked by a tiny cross-axis sentinel
```

Horizontal scroll:

```text
tile search varies dx only
dy is fixed at 0 or checked by a tiny cross-axis sentinel
```

### 13.5 P5 Pyramid

Do not integrate multi-tile with pyramid initially. P5 should be demoted to cold-path recovery/default-off after its common-path regression.

### 13.6 P6 HNSW

MotionQuality should come first. Later, P6 can use:

```rust
if quality.level == MotionConfidence::Low && quality.consensus_q.unwrap_or(0.5) < 0.4 {
    try_feature_index_fallback()
}
```

### 13.7 P7 Sub-pixel

MotionQuality can gate sub-pixel use:

```text
only allow sub-pixel when:
  source is TemplateNcc
  margin_q high
  sharpness_q high
  verifier_q high
  consensus_q not low
```

---

## 14. Metrics additions

Add fields to `StitchMetrics`:

```rust
pub struct StitchMetrics {
    // existing fields...

    pub motion_quality_combined: Option<f32>,
    pub motion_quality_level: Option<MotionConfidence>,

    pub peak_q: Option<f32>,
    pub margin_q: Option<f32>,
    pub sharpness_q: Option<f32>,
    pub verifier_q: Option<f32>,
    pub axis_q: Option<f32>,
    pub consensus_q: Option<f32>,
    pub feature_q: Option<f32>,

    pub multi_tile_enabled: bool,
    pub multi_tile_tiles: usize,
    pub multi_tile_agree_tiles: usize,
    pub multi_tile_ncc_calls: usize,
    pub multi_tile_us: u64,
}
```

JSONL example:

```json
{
  "seq": "sticky_header",
  "frame": 42,
  "outcome": "Appended",
  "dy": 41,
  "score": 0.032,
  "second_best_score": 0.050,
  "quality": {
    "combined": 0.83,
    "level": "High",
    "peak_q": 0.79,
    "margin_q": 0.61,
    "verifier_q": 0.92,
    "axis_q": 0.98,
    "consensus_q": 0.88
  },
  "multi_tile": {
    "tiles": 3,
    "agree": 3,
    "ncc_calls": 27,
    "us": 180
  }
}
```

---

## 15. Benchmark plan

### 15.1 Required scenarios

Use existing P0 scenarios and add/label:

```text
linear_vertical_down
linear_vertical_up
linear_horizontal_left
duplicate_frames
sticky_header
low_feature_text
repeated_grid
lazy_load_mutation
animated_center_region
blank_center_with_text_sides
large_jump
```

### 15.2 Compare variants

```text
A baseline current P4
B MotionQuality ObserveOnly
C MotionQuality ObserveOnly + MultiTile disabled
D MotionQuality ObserveOnly + MultiTile VerifyOnly
E MotionQuality FallbackGate + MultiTile VerifyOnly
```

### 15.3 Required report fields

```text
total p50 / p95 / p99
template_ncc_us p50 / p95
multi_tile_us p50 / p95
ncc_offsets_scored
ncc_pixel_visits
verifier candidates
fallback count
NoMatch count
false append count
output hash
peak RSS
```

### 15.4 Expected success

ObserveOnly:

```text
output hash identical
fallback count identical
NoMatch count identical
p50 regression < 1–2%
```

VerifyOnly:

```text
output hash identical if not used for decision
multi_tile_us bounded
NCC call count increase matches tile_count * offsets
disabled mode exactly matches baseline
```

FallbackGate:

```text
No false append increase
repeated_grid improves or remains safe
low_feature_text not worse
p95 may improve if it prevents bad accepts or triggers recovery
p50 regression remains within budget
```

---

## 16. Tests

### 16.1 Unit tests

```rust
#[test]
fn peak_quality_is_high_for_low_rollshot_score() {}

#[test]
fn margin_quality_is_low_when_second_best_is_close() {}

#[test]
fn verifier_quality_is_zero_when_verifier_failed() {}

#[test]
fn axis_quality_rejects_large_cross_axis_motion() {}

#[test]
fn confidence_thresholds_map_to_expected_levels() {}

#[test]
fn tile_generation_stays_inside_roi_vertical() {}

#[test]
fn tile_generation_stays_inside_roi_horizontal() {}

#[test]
fn consensus_quality_high_when_tiles_agree() {}

#[test]
fn consensus_quality_low_when_tiles_disagree() {}

#[test]
fn hard_gate_verifier_failed_overrides_high_peak() {}
```

### 16.2 Integration tests

```rust
#[test]
fn observe_only_does_not_change_output_hashes() {}

#[test]
fn verify_only_does_not_change_output_hashes() {}

#[test]
fn repeated_grid_has_low_margin_or_low_consensus() {}

#[test]
fn sticky_header_keeps_high_verifier_quality() {}

#[test]
fn animated_center_region_benefits_from_side_tiles() {}

#[test]
fn disabled_multi_tile_has_zero_extra_ncc_calls() {}
```

### 16.3 Perf regression tests

```text
observe_only common-path p50 regression < 2%
multi_tile disabled = baseline
multi_tile enabled reports bounded NCC calls
fallback_gate does not increase false append
```

---

## 17. PR plan

### PR MQ-1 — MotionQuality data model and metrics

Scope:

```text
add MotionQuality structs
compute peak_q, margin_q, verifier_q, axis_q
add metrics fields
mode = ObserveOnly
no behavior change
```

Acceptance:

```text
all output hashes identical
p50 regression < 1–2%
quality JSON appears in benchmark output
```

### PR MQ-2 — VerifierDiagnostics

Scope:

```text
make PixelOverlapVerifier return diagnostic MAD values
preserve existing boolean/pass behavior
wire verifier_q
```

Acceptance:

```text
verifier pass/fail unchanged
quality records pass_a/pass_b margins
```

### PR MQ-3 — CostCurve optional diagnostics

Scope:

```text
record NCC cost curve only when metrics/quality needs it
compute sharpness_q
ensure disabled path does not allocate
```

Acceptance:

```text
cost curve disabled = baseline
cost curve enabled overhead reported
```

### PR MT-1 — MultiTile VerifyOnly disabled by default

Scope:

```text
tile generation
score tiles near main candidate
consensus_q diagnostics
mode default Disabled
```

Acceptance:

```text
disabled = baseline
enabled output hash unchanged
ncc call budget bounded
```

### PR MT-2 — MultiTile VerifyOnly observe experiments

Scope:

```text
run benchmark suite
tune tile_count, tile size, radius
document results
```

Acceptance:

```text
identify scenarios where consensus adds signal
no common-path unexpected regression
```

### PR MQ-4 — FallbackGate default off

Scope:

```text
low confidence can trigger existing recovery path
default off
benchmark only
```

Acceptance:

```text
no false append increase
quality gating improves or preserves low-texture/repeated scenarios
```

---

## 18. Risk register

### Risk: common-path regression

Mitigation:

```text
ObserveOnly first
mode disabled/default-off
avoid hot struct bloat
avoid allocations unless metrics enabled
measure p50 and instruction count
```

### Risk: tile NCC cost erases P3 gains

Mitigation:

```text
reuse fast NCC
verify only near main offset
limit tile count and radius
run only after candidate is selected
```

### Risk: consensus rejects valid frames

Mitigation:

```text
initially diagnostics only
then fallback gate only
do not hard reject solely on consensus_q in first decision version
```

### Risk: dynamic content causes tile disagreement

Mitigation:

```text
use weighted consensus
low-quality tiles contribute little
do not require all tiles to agree
```

### Risk: repeated pattern still fools all tiles

Mitigation:

```text
keep second-best margin
keep verifier
do not let consensus bypass ambiguity gates
```

---

## 19. Recommended initial constants

```text
MotionQuality:
  high_threshold    = 0.80
  medium_threshold  = 0.60
  low_threshold     = 0.40
  min_axis_q        = 0.05
  min_margin_q      = 0.05
  margin_low        = 0.002
  margin_high       = 0.030

MultiTile:
  mode              = Disabled by default
  tile_count        = 3
  tile_cross_px     = 256
  tile_main_ratio   = 0.60
  tile_start_ratio  = 0.20
  verify_radius_px  = 4
  agree_tolerance   = 2
  max_ncc_calls     = 27
```

These should be treated as starting points, not final constants. Use the benchmark harness to tune them.

---

## 20. How this changes the roadmap

Replace this section:

```text
P5 True Image Pyramid
P6 Indexed Feature Fallback / HNSW
```

with:

```text
P5    Pyramid Recovery
      - status: failed as default optimization
      - move to cold-path / default-off

P5.5  MotionQuality Observe-Only
      - zero behavior change
      - quality metrics

P5.6  Multi-tile Verify-Only
      - consensus diagnostics
      - no candidate replacement yet

P5.7  MotionQuality FallbackGate
      - default off
      - low-quality accepted candidates can trigger existing fallback

P6    Indexed Feature Fallback / HNSW
      - only after quality metrics show fallback is frequent/expensive
```

---

## 21. Reference notes

### Optical mouse quality

- Broadcom/Avago ADNS-2080 optical mouse sensor datasheet.  
  URL: https://www.mouser.com/datasheet/2/678/avagotechnologies_ADNS-2080-908926.pdf
  - `SQUAL` is a surface-quality metric measuring valid features visible in the current frame.
  - Relevance: rollshot should similarly expose a quality signal for each estimated motion.

### PIV / interrogation windows

- OpenPIV documentation, "Basics of the PIV algorithms."  
  URL: https://openpiv.readthedocs.io/en/latest/src/piv_basics.html
- Dabiri, "Cross-Correlation Digital Particle Image Velocimetry – A Review."  
  URL: https://www.aa.washington.edu/sites/aa/files/faculty/dabiri/pubs/piV.Review.Paper.final.pdf
  - Relevance: rollshot multi-tile voting is a scrolling-screenshot version of local interrogation windows.

### PIV peak ratio / uncertainty

- Xue et al., "Particle image velocimetry correlation signal-to-noise ratio metrics and measurement uncertainty quantification."  
  URL: https://arxiv.org/pdf/1405.3023
  - Discusses primary peak ratio (PPR) as a cross-correlation SNR metric comparing the highest peak with the second highest peak.
  - Relevance: rollshot already has second-best margin; MotionQuality formalizes it as `margin_q`.

### Fast NCC

- J. P. Lewis, "Fast Normalized Cross-Correlation."  
  URL: https://scribblethink.org/Work/nvisionInterface/nip.pdf
  - Relevance: multi-tile must reuse fast NCC/integral-statistics infrastructure and must not reintroduce slow NCC.

### HNSW

- Malkov and Yashunin, "Efficient and robust approximate nearest neighbor search using Hierarchical Navigable Small World graphs."  
  URL: https://arxiv.org/abs/1603.09320
  - Relevance: P6 remains useful for indexed feature fallback, but should be gated by MotionQuality data.

---

## 22. Final recommendation

Implement now:

```text
1. MotionQuality Observe-Only
2. Multi-tile Verify-Only
3. MotionQuality FallbackGate, default off
```

Delay:

```text
P6 Indexed Feature Fallback / HNSW
```

until MotionQuality metrics show that feature fallback is frequent enough or expensive enough to justify a new indexed fallback system.

The core engineering principle is:

```text
First make matching confidence visible.
Then use confidence to decide when recovery is needed.
Only then optimize the recovery path.
```
