# rollshot Stitching Optimization Roadmap

> 目標：在不犧牲 rollshot 目前最重要的魯棒性設計，也就是 **coarse-to-fine template matching + pixel verifier + overlap-and-overwrite** 的前提下，降低長截圖時的 CPU、記憶體與 latency。  
> rollshot 目前是 streaming、axis-locked、只與 `last_good_frame` 匹配，主路徑為 downsampled MAD → NCC refine → edge projection → FAST+KNN fallback → two-stage verifier → overlap-and-overwrite paste。這些特性應視為基礎架構，不應被整套替換。

---

## 0. 核心結論

### 優化優先順序

| Priority | 項目 | 主要收益 | 風險 | 是否改變輸出 |
|---|---|---:|---:|---|
| P0 | Benchmark harness | 讓後續改動可驗證 | 低 | 否 |
| P1 | `StripCanvas` / lazy compose | 解決長圖越拼越慢、append 記憶體峰值 | 中 | 理論上否 |
| P2 | `PreparedFrame` cache | 減少每幀重算灰階、coarse、projection | 低 | 否 |
| P3 | Fast NCC: integral stats + SIMD dot | 降低 matcher 熱點 | 中 | 否 |
| P4 | Axis-locked fast path | 減少已鎖軸後的無效雙軸搜尋 | 中 | 否 |
| P5 | 真 image pyramid | 提升大位移與 4K frame 場景 | 中 | 可能影響候選排序 |
| P6 | Indexed feature fallback / HNSW | 降低 fallback 最壞延遲 | 中 | 可能影響 fallback 行為 |
| P7 | Sub-pixel peak fit + fractional accumulator | 減少 1px jitter | 中 | 可能輕微改變 slice 決策 |
| P8 | Phase correlation experimental path | 大跳躍 / 低紋理 recovery | 高 | 只作實驗 |
| P9 | Capture Y-plane direct | 省 RGBA → gray | 中高 | 否 |

---

## 1. 設計原則

### 1.1 不整套改成 snow-shot / ORB primary

snow-shot 的 FAST + descriptor + HNSW edge index 很快，且有 lazy edge-index rebuild 與 final one-shot paste 的設計值得借鑑；它維護 top/bottom edge indices，並在 export 時一次性 last-write-wins 合成。

但 rollshot 不應整套換成 snow-shot，原因是：

1. rollshot 現有的 **PixelOverlapVerifier** 是誤匹配防線。
2. rollshot 的 **overlap-and-overwrite** 對 sticky header/footer 有被動遮蔽效果。
3. rollshot 的 NCC + verifier 對低特徵、文字頁面、細線條內容更可控。
4. HNSW / ORB / AKAZE 應放在 fallback 或實驗模式，而不是主路徑。

wayscrollshot 的 ORB primary 也不適合直接照搬。它的 default path 是 OpenCV ORB + BFMatcher + affine-partial-2D RANSAC，並且仍需要 template fallback；其 append 也有 growing canvas reallocation 問題。

### 1.2 驗證器與 overwrite topology 是不可破壞 invariant

以下行為必須在所有 phase 後保持：

```text
Input frame stream
  -> estimate motion against last accepted / prepared frame
  -> rank candidates
  -> PixelOverlapVerifier
  -> final PixelOverlapVerifier
  -> append slice with overlap-and-overwrite semantics
```

不可破壞的語義：

- `Duplicate` 不應進 matcher。
- `DimensionMismatch` 不應污染 canvas。
- `NoMatch` 不應更新 anchor / last_good state。
- `ReverseDirection` 預設仍拒絕，除非進入未來的 bidirectional mode。
- `OverlapVerificationFailed` 不應 append。
- `full_image()` 對外仍回傳單張 `RgbaImage`。
- 預設輸出應盡量與舊版 byte-level 一致，至少 visual-level 一致。

---

## 2. P0 — Benchmark Harness

### 2.1 目標

在改 matcher / canvas 前，先建立可重複的 benchmark。否則無法判斷 integral NCC、pyramid、HNSW、phase correlation 哪個真的有收益。

### 2.2 新增 crate / test target

建議新增：

```text
crates/
  rollshot-core/
    benches/
      stitch_sequences.rs
    tests/
      golden_sequences.rs
    testdata/
      sequences/
        vertical_text_fast/
        vertical_text_slow/
        sticky_header/
        low_texture/
        repeated_grid/
        horizontal_table/
        reverse_scroll_noise/
```

如果不想把真實 screenshots 放 repo，可以先建立 synthetic generator：

```text
crates/rollshot-core/src/test_utils/
  synthetic_page.rs
  synthetic_scroll.rs
  golden.rs
```

### 2.3 Sequence fixture 格式

每個 sequence 用一個 manifest：

```toml
# testdata/sequences/sticky_header/manifest.toml

name = "sticky_header"
axis = "vertical"
direction = "bottom"
frame_width = 900
frame_height = 700
expected_total_height = 4200
expected_appended_frames = 18
allow_pixel_diff_ratio = 0.002
allow_max_abs_diff = 4

[scroll]
offsets = [0, 42, 87, 133, 180, 228]

[features]
sticky_header = true
low_texture = false
repeated_pattern = false
lazy_load_mutation = true
```

### 2.4 必須量測的 metrics

在 `Stitcher::push_frame()` 外層加 lightweight instrumentation：

```rust
pub struct StitchMetrics {
    pub frame_index: usize,
    pub outcome: StitchOutcomeKind,
    pub total_us: u64,

    pub duplicate_us: u64,
    pub prepare_frame_us: u64,
    pub coarse_us: u64,
    pub template_ncc_us: u64,
    pub edge_projection_us: u64,
    pub verifier_us: u64,
    pub fallback_us: u64,
    pub append_us: u64,

    pub coarse_candidates: usize,
    pub ncc_offsets_scored: usize,
    pub ncc_pixel_visits: usize,
    pub verifier_candidates: usize,

    pub canvas_logical_pixels: u64,
    pub canvas_allocated_bytes: u64,
    pub append_copied_bytes: u64,

    pub best_dx: i32,
    pub best_dy: i32,
    pub best_score: f32,
    pub second_best_score: Option<f32>,
}
```

### 2.5 Benchmark 報表輸出

每次 bench 輸出 JSONL：

```json
{"seq":"sticky_header","frame":12,"total_us":3421,"template_ncc_us":1880,"append_us":900,"outcome":"Appended","dy":42}
```

再用一個簡單 script 聚合：

```text
Sequence: sticky_header
Frames: 60
Accepted: 42
Duplicate: 12
NoMatch: 6
p50 total: 3.4 ms
p95 total: 7.8 ms
p99 total: 11.2 ms
peak RSS: 182 MB
output diff vs golden: 0.04%
```

### 2.6 P0 驗收條件

- 至少 6 組 sequence。
- 每組 sequence 有 expected outcome。
- bench 能分別報告 matcher、verifier、append 耗時。
- 所有後續 PR 都必須跑：
  - unit tests
  - golden sequence tests
  - criterion benchmark
- CI 可先只跑小圖，full benchmark 可手動跑。

---

## 3. P1 — Replace `LinearCanvas` with `StripCanvas`

### 3.1 問題

目前 append 會重新配置一張更大的 `RgbaImage`，再 copy 舊 canvas 與新 slice。這讓 append 成本隨 `canvas_h` 增長；長截圖越長，append 越慢，append 瞬間記憶體也會接近雙倍 canvas。

### 3.2 目標

把 append 從：

```text
O(W * current_canvas_h)
```

改成：

```text
O(W * stored_strip_h)
```

最後只有在 `full_image()` / export 時做一次：

```text
O(W * final_canvas_h)
```

### 3.3 新資料結構

新增 `canvas2.rs` 或直接重構 `canvas.rs`：

```rust
pub struct StripCanvas {
    axis: Axis,
    direction: Option<Direction>,
    base_size: (u32, u32),

    /// Logical output rectangle.
    logical_width: u32,
    logical_height: u32,

    /// For one-way scrolling, this can be a Vec.
    /// For future bidirectional scrolling, use VecDeque.
    strips: Vec<CanvasStrip>,

    /// Optional cache for full_image().
    composed_cache: Option<ComposedCache>,
}

pub struct CanvasStrip {
    /// RGBA crop from incoming frame.
    image: RgbaImage,

    /// Where this strip should be pasted in final logical canvas.
    x: i64,
    y: i64,

    /// Net growth contributed by this append.
    slice_px: u32,

    /// Overlap region that intentionally overwrites older pixels.
    overlap_px: u32,

    /// Original incoming frame index, useful for debugging.
    frame_index: usize,
}

pub struct ComposedCache {
    image: RgbaImage,
    dirty_from_strip: usize,
}
```

### 3.4 Append 語義

對 vertical bottom append，舊邏輯大致是：

```text
overlap_size = max(0, H/2 - slice_px)
total_slice = slice_px + overlap_size
take rows [H - total_slice, H)
paste at canvas_y = canvas_h - overlap_size
logical canvas height += slice_px
```

新邏輯應保持同樣 paste position：

```rust
pub fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> Result<()> {
    let h = frame.height();
    let w = frame.width();

    let overlap_px = h.saturating_div(2).saturating_sub(slice_px);
    let total_slice = (slice_px + overlap_px).min(h);

    let crop_y = h - total_slice;
    let crop = imageops::crop_imm(frame, 0, crop_y, w, total_slice).to_image();

    let paste_y = self.logical_height as i64 - overlap_px as i64;

    self.strips.push(CanvasStrip {
        image: crop,
        x: 0,
        y: paste_y,
        slice_px,
        overlap_px,
        frame_index: self.strips.len(),
    });

    self.logical_height += slice_px;
    self.composed_cache = None;

    Ok(())
}
```

Horizontal append 做同樣對稱處理。

### 3.5 First frame

第一張 frame 不應特殊存在於 `image` 欄位，而是作為第一個 strip：

```rust
CanvasStrip {
    image: first_frame.clone(),
    x: 0,
    y: 0,
    slice_px: first_frame.height(),
    overlap_px: 0,
    frame_index: 0,
}
```

這樣 `full_image()` 只要依序 paste strips。

### 3.6 `full_image()` compose

```rust
pub fn full_image(&mut self) -> RgbaImage {
    if let Some(cache) = &self.composed_cache {
        return cache.image.clone();
    }

    let mut out = RgbaImage::new(self.logical_width, self.logical_height);

    for strip in &self.strips {
        overlay_copy(&mut out, &strip.image, strip.x, strip.y);
    }

    self.composed_cache = Some(ComposedCache {
        image: out.clone(),
        dirty_from_strip: self.strips.len(),
    });

    out
}
```

如果 `full_image()` 被頻繁呼叫，後續可以做 incremental compose；第一版先全量 compose，因為它已經比每 append 重拷好。

### 3.7 Row copy 實作

避免 per-pixel `put_pixel`：

```rust
fn overlay_copy(dst: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64) {
    let dst_w = dst.width() as i64;
    let dst_h = dst.height() as i64;

    for sy in 0..src.height() as i64 {
        let dy = y + sy;
        if dy < 0 || dy >= dst_h {
            continue;
        }

        let copy_x0 = x.max(0);
        let copy_x1 = (x + src.width() as i64).min(dst_w);
        if copy_x1 <= copy_x0 {
            continue;
        }

        let sx0 = (copy_x0 - x) as usize;
        let len_px = (copy_x1 - copy_x0) as usize;

        let src_row = src.as_raw();
        let dst_row = dst.as_mut();

        let src_start = ((sy as usize * src.width() as usize) + sx0) * 4;
        let dst_start = ((dy as usize * dst.width() as usize) + copy_x0 as usize) * 4;
        let len = len_px * 4;

        dst_row[dst_start..dst_start + len]
            .copy_from_slice(&src_row[src_start..src_start + len]);
    }
}
```

### 3.8 測試

新增：

```rust
#[test]
fn strip_canvas_matches_linear_canvas_append_bottom() {}

#[test]
fn strip_canvas_matches_linear_canvas_append_top() {}

#[test]
fn strip_canvas_matches_linear_canvas_append_right() {}

#[test]
fn strip_canvas_matches_linear_canvas_append_left() {}

#[test]
fn strip_canvas_overlap_overwrite_matches_legacy() {}

#[test]
fn strip_canvas_full_image_is_stable_after_multiple_calls() {}
```

測試方式：

1. 用 deterministic synthetic frames。
2. 同時跑 legacy `LinearCanvas` 與 new `StripCanvas`。
3. 每 append 一次比較 `full_image()`。
4. 允許 0 diff，因為這是純資料結構替換。

### 3.9 驗收條件

- `full_image()` output 與 legacy byte-identical。
- append p95 latency 不隨 `canvas_h` 線性上升。
- 長圖 peak RSS 明顯下降。
- 所有 existing tests pass。
- `StripCanvas` 可先藏在 feature flag：

```rust
#[cfg(feature = "strip-canvas")]
type CanvasImpl = StripCanvas;

#[cfg(not(feature = "strip-canvas"))]
type CanvasImpl = LinearCanvas;
```

---

## 4. P2 — `PreparedFrame` Cache

### 4.1 問題

rollshot 現在每次 motion estimation 都會對 prev / curr 轉灰階，並建立 coarse samples；但成功 append 後，這一輪的 `curr` 會成為下一輪的 `prev`。因此 prev 的灰階、coarse、projection 可以保留。

### 4.2 新資料結構

```rust
pub struct PreparedFrame {
    pub rgba: RgbaImage,
    pub width: u32,
    pub height: u32,

    /// First version: keep f32 to minimize behavior change.
    pub gray_f32: Vec<f32>,

    pub coarse_step: u32,
    pub coarse_width: u32,
    pub coarse_height: u32,
    pub coarse_f32: Vec<f32>,

    pub signature: DuplicateSignature,

    pub edge_projection_v: Option<Vec<f32>>,
    pub edge_projection_h: Option<Vec<f32>>,
}
```

第一階段不要同時改成 `u8`，避免太多變數。先保留 `f32`，只做 cache。

### 4.3 API 調整

目前：

```rust
estimate_motion(prev: &RgbaImage, curr: &RgbaImage, ...)
```

改成：

```rust
estimate_motion(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<Axis>,
    last_motion: Option<Motion>,
    config: &StitchConfig,
) -> MotionEstimate
```

`Stitcher` state 改成：

```rust
pub struct Stitcher {
    canvas: CanvasImpl,
    last_good: PreparedFrame,
    locked_axis: Option<Axis>,
    locked_direction: Option<Direction>,
    last_motion: Option<Motion>,
}
```

### 4.4 Lazy projection

edge projection 不是每次一定需要，可以 lazy build：

```rust
impl PreparedFrame {
    pub fn edge_projection(&mut self, axis: Axis, roi: Rect) -> &[f32] {
        match axis {
            Axis::Vertical => {
                if self.edge_projection_v.is_none() {
                    self.edge_projection_v = Some(build_edge_projection(...));
                }
                self.edge_projection_v.as_ref().unwrap()
            }
            Axis::Horizontal => { ... }
        }
    }
}
```

如果 borrow checker 讓 `estimate_motion(&PreparedFrame, &PreparedFrame)` 不方便 lazy mut，可把 projection cache 移到 `MotionWorkspace`。

### 4.5 後續可選：gray 改 `u8`

等 cache 版本穩定後，再評估：

```rust
pub enum GrayBuffer {
    U8(Vec<u8>),
    F32(Vec<f32>),
}
```

MAD、coarse、projection 都可用 `u8` / `u16 accumulator`；NCC dot product 再轉 accumulator。這能降低 memory bandwidth，但可能造成分數微小變化，所以應獨立 PR。

### 4.6 測試

```rust
#[test]
fn prepared_frame_signature_matches_old_signature() {}

#[test]
fn prepared_frame_gray_matches_old_to_grayscale() {}

#[test]
fn prepared_frame_coarse_matches_old_coarse_samples() {}

#[test]
fn prepared_frame_does_not_update_on_no_match() {}

#[test]
fn prepared_frame_updates_only_after_appended() {}
```

### 4.7 驗收條件

- 成功 append 後，下一輪不再重算 prev gray / prev coarse。
- `NoMatch` 後 anchor 仍是同一個 `last_good`。
- `Duplicate` 不觸發 full `PreparedFrame` 建立；可先只算 signature。
- 每幀 `prepare_frame_us` 至少下降接近 30–50%，具體以 benchmark 為準。

---

## 5. P3 — Fast NCC: Integral Stats + SIMD Cross Term

### 5.1 問題

rollshot 的 NCC refine 在 512px match band 上，最多約 161 個 offsets per axis，且 `ncc_score_shifted` 是兩 pass：先算 mean，再算 correlation / variance。

Fast normalized cross-correlation 的經典做法是用 image integral 與 squared integral 快速取得 window sum / sumsq；Lewis 的 fast NCC paper 也使用 precomputed integrals 來加速 normalization 項。

但要注意：integral image 只能讓 `Σx`、`Σx²`、`Σy`、`Σy²` 變快，`Σxy` cross term 仍需掃 pixels，除非改 FFT 或其他 correlation method。

### 5.2 目標

把 NCC 從：

```text
for each offset:
  pass 1: scan overlap to compute mean
  pass 2: scan overlap to compute dot / var
```

改成：

```text
precompute integral(gray), integral(gray²)
for each offset:
  O(1): sum_x, sum_x2, sum_y, sum_y2
  SIMD scan: sum_xy
  compute NCC
```

### 5.3 新資料結構

```rust
pub struct IntegralImage {
    width: usize,
    height: usize,

    /// Size = (width + 1) * (height + 1)
    /// Use f64 or i64 depending on gray format.
    sum: Vec<f64>,
    sum_sq: Vec<f64>,
}

pub struct NccWorkspace {
    prev_integral: IntegralImage,
    curr_integral: IntegralImage,
}
```

如果 `PreparedFrame.gray_f32` 保持 f32，integral 用 f64 避免大圖累積誤差。

### 5.4 Integral 建構

```rust
impl IntegralImage {
    pub fn from_gray_f32(gray: &[f32], width: usize, height: usize) -> Self {
        let stride = width + 1;
        let mut sum = vec![0.0; (width + 1) * (height + 1)];
        let mut sum_sq = vec![0.0; (width + 1) * (height + 1)];

        for y in 0..height {
            let mut row_sum = 0.0;
            let mut row_sum_sq = 0.0;

            for x in 0..width {
                let v = gray[y * width + x] as f64;
                row_sum += v;
                row_sum_sq += v * v;

                let idx = (y + 1) * stride + (x + 1);
                sum[idx] = sum[idx - stride] + row_sum;
                sum_sq[idx] = sum_sq[idx - stride] + row_sum_sq;
            }
        }

        Self { width, height, sum, sum_sq }
    }

    #[inline]
    pub fn rect_sum(&self, x: usize, y: usize, w: usize, h: usize) -> f64 {
        let stride = self.width + 1;
        let x2 = x + w;
        let y2 = y + h;

        self.sum[y2 * stride + x2]
            - self.sum[y * stride + x2]
            - self.sum[y2 * stride + x]
            + self.sum[y * stride + x]
    }
}
```

### 5.5 SIMD cross term

先做 portable scalar baseline，再加 feature-gated SIMD。

```rust
#[inline]
fn dot_scalar(
    a: &[f32],
    b: &[f32],
    width: usize,
    rect_a: Rect,
    rect_b: Rect,
) -> f64 {
    let mut acc = 0.0;

    for row in 0..rect_a.h {
        let ay = rect_a.y + row;
        let by = rect_b.y + row;

        let a0 = ay * width + rect_a.x;
        let b0 = by * width + rect_b.x;

        for i in 0..rect_a.w {
            acc += (a[a0 + i] as f64) * (b[b0 + i] as f64);
        }
    }

    acc
}
```

SIMD 版本可用：

- `std::simd`：如果專案可接受 nightly 或等 portable SIMD 狀態。
- `wide` crate：較穩定。
- 手寫 `#[cfg(target_arch = "x86_64")]` SSE/AVX：最高效但維護成本高。

建議順序：

1. scalar + integral stats。
2. `wide` crate SIMD。
3. 只有 benchmark 顯示需要時，再做 target-specific intrinsics。

### 5.6 NCC 公式

```rust
fn ncc_from_sums(
    n: f64,
    sum_x: f64,
    sum_x2: f64,
    sum_y: f64,
    sum_y2: f64,
    sum_xy: f64,
) -> f32 {
    let numerator = sum_xy - (sum_x * sum_y / n);
    let var_x = sum_x2 - (sum_x * sum_x / n);
    let var_y = sum_y2 - (sum_y * sum_y / n);

    if var_x <= 1e-9 || var_y <= 1e-9 {
        return -1.0;
    }

    (numerator / (var_x * var_y).sqrt()) as f32
}
```

### 5.7 注意事項

- Integral image 只針對 NCC band / ROI 建也可以，不一定要整張 frame。
- 若只對 `match_width_region` 建 integral，可降低 memory，但 offset shifting 會讓 rect 不同；第一版建整張 gray 最簡單。
- `sum_xy` 仍是熱點，SIMD 與 row-contiguous layout 最重要。
- 保持 `second_best_score` 行為，避免 repeated pattern regression。
- NCC 分數可能有浮點微差；測試用 tolerance，例如 `1e-5`。

### 5.8 測試

```rust
#[test]
fn integral_rect_sum_matches_naive_sum() {}

#[test]
fn integral_rect_sum_sq_matches_naive_sum_sq() {}

#[test]
fn fast_ncc_matches_old_ncc_for_random_rects() {}

#[test]
fn fast_ncc_preserves_best_offset_on_synthetic_scroll() {}

#[test]
fn repeated_grid_still_rejected_by_second_best_margin() {}
```

### 5.9 驗收條件

- 所有 golden sequence output 不劣化。
- `template_ncc_us` p50 / p95 下降。
- `best_offset` 與 legacy 相同或 verifier 後 outcome 相同。
- repeated-grid 測試仍拒絕 ambiguous match。
- 若 SIMD feature 關閉，scalar fast NCC 仍可運作。

---

## 6. P4 — Axis-Locked Fast Path

### 6.1 問題

目前 coarse search 即使 axis locked 仍會搜尋 V / H 兩軸，用來偵測 cross-axis change。這很保守，但 steady-state 時多數 frame 只會沿 locked axis 移動。

### 6.2 目標

已鎖軸後先跑主軸 fast path：

```text
if locked_axis exists:
  try main axis only
  if candidate passes verifier and cross-axis sentinel:
      accept
  else:
      fallback to existing dual-axis search
else:
  run existing dual-axis search
```

### 6.3 Cross-axis sentinel

不能完全不看副軸，否則會錯過 diagonal / region drift。建議用便宜 sentinel：

```rust
pub struct CrossAxisCheck {
    pub estimated_cross_px: i32,
    pub residual_score: f32,
    pub suspicious: bool,
}
```

第一版可簡單做：

- locked vertical 時，對 `dx ∈ [-max_cross_axis_px, +max_cross_axis_px]` 做小範圍 NCC / MAD check。
- 若最佳 dx 超過 config，或 residual 明顯改善，標記 suspicious。
- suspicious 時回到原雙軸完整流程。

### 6.4 Config

```rust
pub struct AxisFastPathConfig {
    pub enabled: bool,
    pub cross_axis_probe_radius: i32, // default 6
    pub fallback_to_dual_axis_on_suspicious: bool,
}
```

### 6.5 測試

```rust
#[test]
fn locked_vertical_uses_main_axis_fast_path() {}

#[test]
fn cross_axis_drift_falls_back_to_dual_axis() {}

#[test]
fn axis_changed_is_still_reported() {}

#[test]
fn ambiguous_first_motion_still_rejected() {}
```

### 6.6 驗收條件

- steady vertical sequence 中 coarse / NCC scored offsets 減少。
- diagonal sequence 不被誤 append。
- `AxisChanged` / `CrossAxisTooLarge` 行為不 regression。
- matcher p95 latency 下降。

---

## 7. P5 — True Image Pyramid

### 7.1 問題

目前 rollshot 只有單層 4× coarse downsample，不是真正 multi-level pyramid。單層 coarse 在大位移、4K frame、快速滾動時仍可能需要較多 range search。

### 7.2 目標

新增 3–4 層 pyramid：

```text
level 3: 1/8 resolution, full range search
level 2: 1/4 resolution, ±2–4 px refine
level 1: 1/2 resolution, ±2–4 px refine
level 0: 1/1 resolution, NCC refine
```

### 7.3 資料結構

```rust
pub struct PyramidLevel {
    pub scale_log2: u8, // 0, 1, 2, 3
    pub width: usize,
    pub height: usize,
    pub gray: Vec<f32>,
    pub integral: Option<IntegralImage>,
}

pub struct FramePyramid {
    pub levels: Vec<PyramidLevel>,
}
```

放進 `PreparedFrame`：

```rust
pub struct PreparedFrame {
    // ...
    pub pyramid: Option<FramePyramid>,
}
```

### 7.4 Downsample filter

第一版：

```text
level n+1 = 2x2 box average(level n)
```

原因：

- 實作簡單。
- deterministic。
- 對文字頁面通常足夠。

第二版可測 Gaussian：

```text
[1, 4, 6, 4, 1] separable blur + decimate
```

### 7.5 Candidate propagation

```rust
fn pyramid_search(prev: &FramePyramid, curr: &FramePyramid, axis: Axis) -> MotionCandidate {
    let top = prev.levels.last().unwrap();

    let mut offset = search_full_range_at_top_level(...);

    for level in (0..top_level).rev() {
        offset *= 2;
        offset = refine_around(prev[level], curr[level], offset, radius = 3);
    }

    offset
}
```

### 7.6 與現有 coarse/NCC 的整合

不要第一版直接取代現有 coarse。先新增候選來源：

```text
candidates =
  coarse_candidates()
  + pyramid_candidates()
  + template_candidates()
  + edge_projection_candidates()
```

之後都交給既有 `rank_verified_candidates()`。

這樣 verifier 仍是最終防線。

### 7.7 Config

```rust
pub struct PyramidConfig {
    pub enabled: bool,
    pub max_levels: u8,          // default 3
    pub min_level_side: u32,     // default 96
    pub refine_radius: i32,      // default 3
    pub use_box_filter: bool,    // default true
}
```

### 7.8 測試

```rust
#[test]
fn pyramid_downsample_dimensions_are_correct() {}

#[test]
fn pyramid_large_jump_finds_correct_candidate() {}

#[test]
fn pyramid_candidate_passes_existing_verifier() {}

#[test]
fn pyramid_does_not_accept_repeated_grid_alias() {}
```

### 7.9 驗收條件

- 大位移 sequence `NoMatch` 下降。
- 小位移 steady sequence 不變慢太多。
- repeated pattern 不 regression。
- 如果 pyramid candidate 與 coarse candidate 衝突，verifier / ranking 能選對。

---

## 8. P6 — Indexed Feature Fallback / HNSW

### 8.1 問題

rollshot default fallback 是 FAST corners + 8-D descriptor + symmetric linear KNN + bucket vote，只有在 coarse/template/edge projection 都失敗後才跑。

線性 KNN 在 N ≤ 1200 時不一定糟，但 miss path 會造成 latency spike。HNSW 的理論價值是用 hierarchical small-world graph 做 approximate nearest-neighbor search。

### 8.2 原則

不要每次 fallback 臨時建 HNSW。那樣 build cost 可能吃掉 query benefit。

應借鑑 snow-shot 的 edge index 設計：

- 維護 current edge descriptors。
- 只有 edge 移動超過一定距離才重建。
- fallback 時查 index，而不是查整張舊 frame。

### 8.3 新資料結構

```rust
pub struct FeatureEdgeIndex {
    pub direction: Direction,
    pub frame_position: i64,

    pub corners: Vec<FeaturePoint>,
    pub descriptors: Vec<Descriptor8>,

    #[cfg(feature = "hnsw")]
    pub hnsw: Option<HnswIndex>,

    pub last_rebuild_logical_pos: i64,
}

pub struct Descriptor8(pub [f32; 8]);

pub struct FeaturePoint {
    pub x: u16,
    pub y: u16,
}
```

### 8.4 fallback 策略

```text
if feature_index.enabled:
  query edge index
  run ratio/distance gates
  vote dominant translation
  verify candidate with PixelOverlapVerifier
else:
  use existing linear KNN fallback
```

### 8.5 HNSW crate 選擇

候選：

- `hora`：snow-shot 使用，概念已驗證。
- `hnsw_rs`：Rust 生態常見。
- `instant-distance`：較輕，但不一定是 HNSW。

第一版建議：

```text
feature = "hnsw-fallback"
default off
```

不要讓 core default 依賴變重。

### 8.6 Rebuild 條件

```rust
pub struct FeatureIndexConfig {
    pub enabled: bool,
    pub backend: FeatureIndexBackend, // Linear, Hnsw
    pub rebuild_after_scroll_ratio: f32, // default 0.8
    pub min_rebuild_px: u32,             // default 256
    pub max_features: usize,             // default 1200
    pub min_inliers: usize,              // default 16
}
```

### 8.7 測試

```rust
#[test]
fn hnsw_fallback_matches_linear_knn_on_synthetic_features() {}

#[test]
fn hnsw_fallback_candidate_still_requires_pixel_verifier() {}

#[test]
fn feature_index_rebuilds_after_threshold() {}

#[test]
fn feature_index_does_not_update_on_no_match() {}
```

### 8.8 驗收條件

- fallback p95 / p99 latency 下降。
- fallback success rate 不低於 linear KNN。
- HNSW candidate 必須仍通過 PixelOverlapVerifier。
- default build 可以不開 HNSW feature。

---

## 9. P7 — Sub-pixel NCC Peak Fit

### 9.1 目標

rollshot 目前 offset 是 integer。可在 NCC peak 週圍做 1D / 2D parabolic interpolation，估計 fractional offset。

### 9.2 第一版只用於 fractional accumulator

不要立即做 subpixel resampling。只做：

```rust
pub struct Motion {
    pub dx: i32,
    pub dy: i32,
    pub sub_dx: f32,
    pub sub_dy: f32,
}
```

`slice_px` 仍是 integer，但可累積 fractional residual：

```rust
self.frac_scroll_accum += motion.sub_dy;

if self.frac_scroll_accum.abs() >= 1.0 {
    let correction = self.frac_scroll_accum.round() as i32;
    slice_px = (slice_px as i32 + correction).max(0) as u32;
    self.frac_scroll_accum -= correction as f32;
}
```

### 9.3 Parabolic fit

對主軸 scores：

```text
score[-1], score[0], score[+1]
```

若 score 是 NCC，越大越好：

```rust
fn parabolic_peak_offset(left: f32, center: f32, right: f32) -> f32 {
    let denom = left - 2.0 * center + right;
    if denom.abs() < 1e-6 {
        return 0.0;
    }
    0.5 * (left - right) / denom
}
```

Clamp：

```rust
sub = sub.clamp(-0.5, 0.5)
```

### 9.4 啟用條件

只在以下條件成立時用：

- best candidate 來自 NCC，不是 coarse-only / feature-only。
- second-best margin 足夠。
- verifier score 很好。
- peak 不是邊界 offset。
- `abs(sub) <= 0.5`。

### 9.5 測試

```rust
#[test]
fn parabolic_fit_returns_zero_for_symmetric_peak() {}

#[test]
fn fractional_accumulator_eventually_applies_one_pixel() {}

#[test]
fn subpixel_disabled_keeps_legacy_integer_behavior() {}

#[test]
fn subpixel_does_not_change_low_confidence_matches() {}
```

### 9.6 驗收條件

- default 可以先關閉。
- 開啟後 jitter sequence 視覺更穩。
- 不增加 false append。
- 不破壞 verifier。

---

## 10. P8 — Phase Correlation Experimental Candidate

### 10.1 目標

Phase correlation 適合純 translation，可作為 large jump / low texture 的候選產生器。第一版不要取代主路徑，只作 experimental fallback candidate。

### 10.2 放置位置

```text
coarse MAD
template NCC
edge projection
relaxed coarse
phase correlation candidate
feature fallback
```

或：

```text
if coarse/template miss:
  run phase correlation
  verify candidate
  if pass -> accept
  else -> feature fallback
```

### 10.3 實作選項

Rust crate：

- `rustfft`
- `realfft`

流程：

```text
1. 取 content ROI 或 match band。
2. apply window function，降低邊界效應。
3. FFT(prev), FFT(curr)。
4. cross power spectrum = A * conj(B) / abs(A * conj(B)).
5. inverse FFT。
6. peak location -> dx/dy。
7. peak ratio / peak sharpness 作 confidence。
8. PixelOverlapVerifier 驗證。
```

### 10.4 風險

- sticky header/footer 會產生靜止區 peak。
- lazy-loaded 圖片 / animation 會讓 phase peak 分裂。
- FFT padding 增加 memory。
- 小 frame 常數成本可能比 spatial search 高。

### 10.5 Config

```rust
pub struct PhaseCorrelationConfig {
    pub enabled: bool,
    pub min_peak_ratio: f32,
    pub use_content_roi: bool,
    pub window: PhaseWindow, // None, Hann
}
```

### 10.6 測試

```rust
#[test]
fn phase_corr_finds_large_translation_on_synthetic_page() {}

#[test]
fn phase_corr_candidate_must_pass_verifier() {}

#[test]
fn phase_corr_does_not_override_good_ncc_candidate() {}
```

### 10.7 驗收條件

- 只在 feature flag 或 experimental config 下啟用。
- 對 large jump sequence 有 recovery improvement。
- 對 sticky / dynamic content 不增加 false positive。
- 不作為 default，除非 benchmark 明確證明優於現有 relaxed coarse。

---

## 11. P9 — Capture Y-plane Direct

### 11.1 目標

如果 capture backend 能提供 YUV / NV12，直接使用 Y plane，避免每幀 RGBA → gray。

### 11.2 架構

```rust
pub enum CapturedFrame {
    Rgba(RgbaImage),
    Luma {
        y_plane: Vec<u8>,
        width: u32,
        height: u32,
        stride: u32,
        rgba_for_canvas: Option<RgbaImage>,
    },
}
```

注意：stitching matcher 可以用 Y plane，但 canvas append 仍需要 RGBA。如果 backend 只提供 YUV，就需要：

- matcher 使用 Y。
- canvas 使用 RGBA conversion。
- 或 capture 同時提供 RGBA + Y，避免重算。

### 11.3 實作順序

1. 不動現有 capture。
2. 先讓 `PreparedFrame` 支援 external gray。
3. 再讓平台 backend optionally 填入 gray。
4. 若沒有 gray，fallback to current luma conversion。

### 11.4 驗收條件

- 不影響不支援 YUV 的平台。
- matcher 結果與 RGBA luma 接近。
- 大 frame `prepare_frame_us` 下降。

---

## 12. 不建議現在做的事

### 12.1 不做 GPU NCC

原因：

- 現有 frame size 與 match band 先用 SIMD / cache / axis fast path 就能大幅改善。
- GPU path 會引入 upload/download/sync。
- cross-platform Tauri app 維護成本高。
- 等 4K/120Hz 即時 preview 成為硬需求再評估。

### 12.2 不把 ORB / AKAZE 升為 primary

原因：

- OpenCV 依賴重。
- ORB matching + RANSAC 對純 1D scroll 是過度模型。
- feature-poor text pages 仍需要 template fallback。
- rollshot 已有 template-first pipeline 與 verifier。

### 12.3 不先做大型 algorithm abstraction

目前已有 candidate chain。先用 config / feature flag 加候選來源即可。等 phase correlation / pyramid 真的需要 A/B 時，再整理 trait。

---

## 13. 建議 PR 切分

### PR 1 — Benchmark Harness

包含：

- synthetic sequence generator
- criterion benches
- JSONL metrics
- golden output comparison

不改 production behavior。

### PR 2 — StripCanvas behind feature flag

包含：

- `StripCanvas`
- legacy equivalence tests
- `full_image()` lazy compose
- metrics: append copied bytes

預設可先 off。

### PR 3 — Enable StripCanvas by default

條件：

- PR 2 benchmark 通過。
- output byte-identical 或 visual-identical。
- app / CLI integration OK。

### PR 4 — PreparedFrame Cache

包含：

- `PreparedFrame`
- matcher API 改造
- no behavior change tests
- metrics: prepare time

### PR 5 — Fast NCC Scalar

包含：

- integral sum / sumsq
- scalar cross term
- old NCC vs new NCC tests

### PR 6 — Fast NCC SIMD

包含：

- `wide` 或 target-specific SIMD
- feature flag
- fallback scalar

### PR 7 — Axis Fast Path

包含：

- locked-axis main path
- cross-axis sentinel
- fallback to old dual-axis

### PR 8 — Pyramid Candidate

包含：

- pyramid builder
- candidate generator
- verifier integration
- default off，benchmark only

### PR 9 — Feature Index / HNSW Fallback

包含：

- edge feature index
- linear indexed backend first
- optional HNSW backend
- default off or conservative config

### PR 10 — Subpixel Experimental

包含：

- peak fit
- fractional accumulator
- default off

### PR 11 — Phase Correlation Experimental

包含：

- FFT candidate generator
- default off
- benchmark sequences

---

## 14. 最低實作檢查清單

每個 PR 合併前都要確認：

```text
[ ] Duplicate frame 不進 matcher
[ ] DimensionMismatch 不污染 state
[ ] NoMatch 不更新 last_good
[ ] Appended 才更新 last_good / prepared cache
[ ] PixelOverlapVerifier 仍是 final gate
[ ] ReverseDirection 預設仍拒絕
[ ] Sticky header sequence 無 ghost regression
[ ] Repeated grid sequence 不誤匹配
[ ] Low texture sequence 不比 baseline 差
[ ] Long canvas benchmark append time 不線性惡化
[ ] Peak RSS 沒有明顯增加
[ ] Feature flags off 時行為等同 baseline
```

---

## 15. 最終預期狀態

完成 P1–P4 後，rollshot 的架構應該變成：

```text
Incoming frame
  -> cheap duplicate signature
  -> PreparedFrame(curr)
       gray / coarse / optional projections / optional integral
  -> estimate_motion(last_good_prepared, curr_prepared)
       locked-axis fast path
       pyramid/coarse candidates
       fast NCC refine
       edge projection
       verifier ranking
       fallback if needed
  -> final PixelOverlapVerifier
  -> StripCanvas append slice + overlap
  -> update last_good_prepared only after append
  -> full_image() lazy compose on demand
```

完成 P5–P8 後，實驗候選會變成：

```text
Candidate sources:
  1. coarse MAD
  2. pyramid
  3. fast NCC
  4. edge projection
  5. phase correlation, experimental
  6. indexed FAST/HNSW fallback

All candidates:
  -> same rank_verified_candidates
  -> same PixelOverlapVerifier
  -> same final verifier
```

最重要的設計點是：**可以增加候選來源與加速實作，但不要繞過 verifier，也不要破壞 overlap-and-overwrite 的輸出語義。**

---

## 16. 參考方向

- rollshot 目前架構：streaming、axis lock、coarse MAD、NCC refinement、edge projection、FAST+KNN fallback、PixelOverlapVerifier、LinearCanvas overlap-and-overwrite。
- snow-shot 可借鑑：FAST descriptor + HNSW edge index、top/bottom strip list、lazy rebuild、one-shot export paste。
- wayscrollshot 可借鑑：ORB + RANSAC 作為 feature-based fallback 的參考，但不建議作為 rollshot primary。
- Fast NCC：integral image / summed-area table 可加速 NCC normalization terms。
- HNSW：適合可重用 edge index，不建議每次 fallback 臨時建 index。
- Phase correlation：適合作為 large-jump experimental candidate，不建議直接取代現有主路徑。
