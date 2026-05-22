# rollshot MVP 設計文件 v0.2

> 狀態：v0.1 已完成實作驗證。  
> v0.1 已確認 **macOS backend** 與 **Linux KDE 6 Wayland backend** 可用。  
> 本版文件更新重點：將 v0.2 收斂為 **優化過的 LinearScroll stitching**，包含自動偵測主軸、內部 hybrid matcher、AKAZE fallback 與 golden fixture 測試。  
> macOS select region、interactive recording UX、GUI/preview、Mosaic2D 均延後到後續版本。

---

## 0. 版本定位

rollshot 不是 wayscrollshot 的 fork，而是一個新的 Rust 專案。  
原 wayscrollshot、OBS Studio、scap、zed-scap-patches、rust-cv 都只作為參考來源。

目前專案方向：

```text
platform-native capture backend
→ 統一輸出 RgbaImage frame stream
→ rollshot stitching core
→ 輸出長截圖 / 未來輸出 mosaic
```

v0.1 已經打通兩個主要平台：

```text
macOS:
  ScreenCaptureKit / scap-style backend

Linux KDE 6 Wayland:
  xdg-desktop-portal ScreenCast + PipeWire
  OBS-style portal lifecycle
```

v0.2 的重點不是再擴 capture platform，而是強化 stitching：

```text
LinearScroll v2:
  自動偵測主軸
  支援垂直與水平長截圖
  單一最佳自動 matcher 策略
  AKAZE 作為內部 fallback
  不要求使用者手動選 AKAZE / template / fast
```

---

## 1. 0.1 實作後的修正結論

### 1.1 已驗證可行

v0.1 已通過：

```text
[done] macOS backend 可用
[done] Linux KDE 6 Wayland backend 可用
[done] capture backend 能輸出 RgbaImage
[done] core 與 capture layer 解耦
[done] learn-projects 放置 OBS / scap 供長期參考
```

因此 v0.2 不再把「能不能跨 macOS / KDE 6 Wayland 擷取畫面」視為主要風險。

### 1.2 舊設計需要調整的地方

舊版設計文件的 stitching 章節偏向：

```text
只估 dy
只 append bottom
使用者可能手動選 algorithm
```

v0.2 需要修正成：

```text
估計 MotionEstimate { dx, dy }
自動判斷主軸 vertical / horizontal
LinearScroll 仍只沿單一主軸延伸，但支援四個方向：
  bottom / top / right / left
演算法選擇由內部 auto strategy 決定，不暴露給一般使用者
```

### 1.3 仍然不進 v0.2 的項目

以下項目不放入 v0.2：

```text
macOS overlay select region
互動式錄影控制，例如隨時按 stop 結束
GUI / preview / floating control
Mosaic2D / minimap-style 2D stitching
Windows support
Linux X11 support
完整 loop closure / global pose optimization
```

---

## 2. 重要決策總覽

| 決策 | v0.2 結論 |
|---|---|
| v0.2 主目標 | 優化 LinearScroll stitching。 |
| 主軸 | 不寫死 y 軸；自動偵測 vertical / horizontal。 |
| 是否支援斜向 / 2D stitching | v0.2 不支援；安排到 Mosaic2D 版本。 |
| 使用者是否能選 algorithm | 一般 CLI 不暴露；內部只保留 auto / best strategy。 |
| AKAZE | 只作為 internal fallback，不作為使用者手動選項。 |
| OpenCV ORB | 不納入；避免 OpenCV 安裝與 Rust binding 發行成本。 |
| rust-cv | 優先使用 `akaze` crate / fork；不直接 copy 整包 rust-cv。 |
| matcher 策略 | HybridAuto：cheap matcher 優先，AKAZE fallback，最後一律 pixel verifier。 |
| LinearScroll | 支援 top / bottom / left / right append。 |
| Mosaic2D | 最後階段才做，獨立 `MosaicStitcher`，不混入 v0.2。 |
| capture backend | v0.1 已可用；v0.2 只做必要 bugfix。 |
| UX | v0.2 仍可沿用目前 capture flow；互動式停錄延後。 |

---

## 3. Roadmap

### 3.1 v0.1：雙平台 capture MVP（已完成）

目標：

```text
macOS backend pass
Linux KDE 6 Wayland backend pass
取得連續 RgbaImage frame stream
基礎 LinearScroll 可輸出 PNG
```

狀態：

```text
done
```

---

### 3.2 v0.2：LinearScroll v2

v0.2 是本文件的主要範圍。

目標：

```text
1. 改掉 dy-only 假設
2. 導入 MotionEstimate { dx, dy }
3. 自動判斷主軸
4. 支援 vertical / horizontal long screenshot
5. 支援 append bottom / top / right / left
6. 建立單一自動 hybrid matcher
7. 加入 AKAZE fallback
8. 加強 overlap verification
9. 建立 golden fixtures / benchmark fixtures
10. 不讓使用者手動選 template / fast / AKAZE
```

完成標準：

```text
[ ] 垂直往下 scroll 可穩定輸出長圖
[ ] 垂直往上 scroll 可穩定輸出長圖
[ ] 水平往右 scroll 可穩定輸出寬圖
[ ] 水平往左 scroll 可穩定輸出寬圖
[ ] 自動判斷主軸，不需使用者指定
[ ] pure template 失敗的 fixture 可由 AKAZE fallback 修正
[ ] sticky header / repeated rows / low feature frames 有測試覆蓋
[ ] default CLI 不提供 algorithm choice
```

---

### 3.2.1 v0.3：Overlap-and-overwrite stitch topology

v0.2 完成後，matcher 端透過 content ROI 已不再被 sticky / fixed UI 帶跑，但
`LinearCanvas::append_*` 仍把整條 slice 貼進 canvas，造成 sticky header /
footer / 裝飾性邊框在長圖上每個 slice 邊界重複出現。v0.2.1 曾經嘗試用
`StaticRegionDetector` 解決，但對 1 px 邊框因為「灰填灰」無效；於 commit
`1b16e8de` revert，改在 v0.3 用 snow-shot 的 overlap-and-overwrite slicing
topology 處理。

詳細 spec 見
`docs/superpowers/specs/2026-05-22-rollshot-overlap-stitch-topology-design.md`，
以下只列重點。

目標：

```text
1. 每個 slice 取 max(H/2, slice_px) 而非僅 slice_px。
2. Paste 位置往回 overlap_size = max(0, H/2 - slice_px) 像素，
   讓新 slice 的 overlap 部分覆蓋前一個 slice 的 trailing portion。
3. 只有最近一次 append 的 trailing pixel 殘留在 canvas 中。
4. 對 sticky header / footer / 純色 sidebar / 1 px 裝飾邊框
   自然 cover，無需偵測。
5. LinearCanvas::append 公開簽名回歸 v0.2（無 mask 參數）。
6. Pure-scroll fixture 輸出 byte-identical to v0.2 minimal-slice。
```

演算法（Append Bottom；其餘三方向對稱）：

```text
inputs: frame W×H, slice_px = |motion.dy|

overlap_size = (H / 2).saturating_sub(slice_px)
total_slice  = (slice_px + overlap_size).min(H)

slice  = frame.view(0, H - total_slice, W, total_slice).to_image()
paste_y = canvas.height() - overlap_size
combined = RgbaImage::new(W, canvas.height() + slice_px)
combined.copy_from(canvas.image, 0, 0)
combined.copy_from(&slice, 0, paste_y)
canvas.image = combined
```

不解決的 cases（已 documented 為限制）：

```text
- patterned / textured sidebar 內部 pattern 與 frame-local y 綁定
  → overlap boundary 處仍有 pattern seam（與 v0.2 相同數量、位置不同）
- 中間錨定的 sticky 元素（icon 在 frame-local y ≈ H/2）
  → 仍會重複；真實 web layout 中極為罕見
- motion > H/2 的單次滾動
  → overlap_size = 0，行為退化為 v0.2 minimal-slice append
```

完成標準：

```text
[ ] commit 1b16e8de 完整 revert
[ ] static_region 模組 / 測試 / 公開 type 移除
[ ] LinearCanvas::append signature 回歸 v0.2
[ ] 四個 slice helper 用 overlap-and-overwrite
[ ] tests/overlap_topology.rs 涵蓋 sticky header / footer / 1 px 邊框
    / 純色 sidebar / 頂底錨定 icon / scroll-up / 雙向 / horizontal
[ ] pure_scroll_byte_identical_to_v0_2_minimal_slice 通過
[ ] cargo test / fmt / clippy 全綠
```

---

### 3.3 v0.3：Capture UX / interactive session

v0.3 處理「使用者怎麼開始、調整、停止」。

目標：

```text
macOS overlay select region
更好的 region selector
像錄影一樣開始 capture 後可隨時停止
不再要求使用者精確輸入 fps / max frames 來控制範圍
progress / stop control
clipboard output
preview / minimal UI
```

v0.3 的 UX 方向：

```text
rollshot capture
→ 選區
→ 開始擷取
→ 使用者捲動
→ 使用者按 stop / hotkey
→ 輸出圖片
```

而不是：

```text
rollshot capture --fps 30 --max-frames 300
```

fps / max frames 可保留為 debug / expert options，但不應是主流程。

---

### 3.4 v0.4：Platform polish / packaging

目標：

```text
macOS app bundle / permission UX
Linux desktop entry / portal diagnostics
release packaging
self-hosted smoke tests
更多 capture edge case
```

---

### 3.5 v0.5+：Mosaic2D / Map mode

Mosaic2D 是目前已知最後階段，不放入 v0.2。

目標：

```text
任意 dx/dy 2D movement
可以先 x 後 y
可以斜向
可以像 minimap / map panning 一樣合成已知區域
canvas 任意方向擴張
未來可加入 drift correction / loop closure
```

Mosaic2D 會獨立成：

```rust
pub enum StitchMode {
    LinearScroll,
    Mosaic2D,
}
```

而不是把 2D canvas 邏輯塞進 LinearScroll。

---

## 4. Workspace 架構

建議 repo：

```text
rollshot/
  Cargo.toml
  README.md
  LICENSE

  crates/
    rollshot-core/
      src/
        lib.rs
        stitcher.rs
        linear/
          mod.rs
          stitcher.rs
          motion.rs
          axis.rs
          canvas.rs
        matcher/
          mod.rs
          hybrid.rs
          template.rs
          edge.rs
          fast.rs
          akaze.rs
          verifier.rs
          roi.rs
        duplicate.rs
        frame_signature.rs
        image_ext.rs
        config.rs
        types.rs

    rollshot-capture/
      src/
        lib.rs
        backend.rs
        frame_stream.rs
        region.rs
        error.rs
        pixel_format.rs
        linux/
          mod.rs
          portal.rs
          pipewire.rs
          pipewire_format.rs
          pipewire_metadata.rs
          portal_types.rs
          kde.rs
        macos/
          mod.rs
          sck.rs
          permission.rs
          pixel_buffer.rs

    rollshot-cli/
      src/
        main.rs
        args.rs
        command_capture.rs
        command_probe.rs
        command_stitch_folder.rs
        command_dump_frames.rs
        logging.rs

    rollshot-app/          # v0.3+，可先保留或延後建立
      src/
        main.rs
        selector.rs
        preview.rs

  learn-projects/
    obs-studio/
    scap/
    scap-zed-patches/
    wayscrollshot/
    rust-cv/

  docs/
    architecture.md
    stitching-linearscroll.md
    stitching-mosaic2d.md
    linux-kde-wayland.md
    macos-screencapturekit.md
    roadmap.md

  tests/
    fixtures/
      linear_vertical/
      linear_horizontal/
      repeated_rows/
      sticky_header/
      low_feature/
      akaze_fallback/
```

### 4.1 是否需要獨立 AKAZE crate

v0.2 建議有兩種可接受方案。

方案 A：放在 `rollshot-core` 的 optional feature：

```toml
[features]
default = []
akaze = ["dep:akaze", "dep:cv-core", "dep:space", "dep:bitarray"]
```

方案 B：獨立 crate：

```text
crates/rollshot-feature-akaze/
```

建議：  
若 AKAZE dependency 造成 core build time 或 feature 管理複雜，使用方案 B。  
若想保持開發速度，先用方案 A。

---

## 5. Root Cargo.toml 建議

```toml
[workspace]
members = [
  "crates/rollshot-core",
  "crates/rollshot-capture",
  "crates/rollshot-cli",
  "crates/rollshot-app",
]
resolver = "2"

[workspace.package]
edition = "2021"
license = "MIT"
repository = "https://github.com/<your-name>/rollshot"

[workspace.dependencies]
anyhow = "1"
thiserror = "1"
log = "0.4"
env_logger = "0.11"
clap = { version = "4", features = ["derive"] }
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
rayon = "1.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = "0.3"

# Optional AKAZE path. Exact versions should follow tested rust-cv crates.
akaze = { version = "0.7", optional = true }
cv-core = { version = "0.15", optional = true }
space = { version = "0.17", optional = true }
bitarray = { version = "0.9", optional = true }
```

---

## 6. Core crate 職責

`rollshot-core` 不應該知道：

```text
KDE
macOS
PipeWire
ScreenCaptureKit
portal
window selector
CLI
```

它只處理：

```text
RgbaImage frame in
→ duplicate detection
→ motion estimation
→ axis-aware LinearScroll append
→ output image
```

v0.2 之後，core 不應再以 `dy` 為唯一概念。  
核心語言應改為：

```text
motion
axis
append direction
overlap verification
```

---

## 7. LinearScroll v2 核心型別

### 7.1 Stitch mode

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchMode {
    LinearScroll,
    // v0.5+
    Mosaic2D,
}
```

v0.2 只實作 `LinearScroll`。

---

### 7.2 Motion estimate

```rust
#[derive(Debug, Clone, Copy)]
pub struct MotionEstimate {
    /// Current frame top-left position relative to previous frame
    /// in content coordinates.
    pub dx: i32,
    pub dy: i32,

    pub axis: ScrollAxis,
    pub direction: AppendDirection,

    /// Lower is better if using diff-like score,
    /// or normalized via ScoreKind.
    pub score: f32,

    pub confidence: f32,
    pub method: MatchMethod,

    pub overlap: OverlapRegion,
    pub inliers: Option<usize>,
    pub raw_matches: Option<usize>,
}
```

### 7.3 Axis and direction

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendDirection {
    Bottom,
    Top,
    Right,
    Left,
}
```

語意：

```text
dx / dy = current frame 在全域內容座標中，相對 previous frame 的位移。
```

例子：

```text
使用者往下捲頁面：
  current frame 看到更下面的內容
  dx = 0
  dy = +N
  direction = Bottom

使用者往上捲頁面：
  current frame 看到更上面的內容
  dx = 0
  dy = -N
  direction = Top

使用者往右捲水平內容：
  current frame 看到更右邊的內容
  dx = +N
  dy = 0
  direction = Right

使用者往左捲水平內容：
  current frame 看到更左邊的內容
  dx = -N
  dy = 0
  direction = Left
```

---

### 7.4 Config

v0.2 不讓一般使用者選 matcher algorithm。  
因此不應設計：

```rust
pub enum MatchAlgorithm {
    Template,
    Fast,
    Akaze,
}
```

作為 CLI 主選項。

建議：

```rust
#[derive(Debug, Clone)]
pub struct StitchConfig {
    pub mode: StitchMode,

    /// v0.2 default: AutoHybrid.
    /// This is for internal tuning / debug only.
    pub strategy: MatchStrategy,

    pub min_overlap: u32,
    pub min_append: u32,
    pub duplicate_threshold: f32,

    pub axis_ratio_threshold: f32,
    pub second_best_margin: f32,

    pub max_search_ratio: f32,
    pub roi: RoiConfig,
    pub verifier: VerifierConfig,

    pub akaze: AkazeConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrategy {
    AutoHybrid,
}
```

一般 CLI 只使用 `AutoHybrid`。  
debug CLI 可以提供：

```bash
rollshot stitch-folder ./frames --debug-dump-matches
rollshot stitch-folder ./frames --disable-akaze
```

但不把 `--algorithm akaze` 變成正式使用者選項。

---

## 8. LinearScroll v2 流程

### 8.1 整體流程

```text
first frame:
  output = frame
  last_frame = frame
  axis = Unknown

next frame:
  duplicate detection

  estimate = AutoHybridMatcher.estimate_motion(last_frame, current_frame)

  if estimate invalid:
    return NoMatch

  if estimate append distance < min_append:
    return NoProgress

  apply axis lock / axis detection policy

  verify overlap using estimate.dx/dy

  append new slice by estimate.direction

  update output
  update last_frame
```

### 8.2 Axis detection

第一個有效 motion 決定 axis：

```rust
if abs(dx) > abs(dy) * axis_ratio_threshold {
    axis = Horizontal;
} else if abs(dy) > abs(dx) * axis_ratio_threshold {
    axis = Vertical;
} else {
    reject as Ambiguous;
}
```

建議值：

```text
axis_ratio_threshold = 1.5
```

### 8.3 Axis lock

LinearScroll 應該是一個單軸模式。  
自動偵測主軸後，應鎖定該軸，避免中途因雜訊誤切換。

```text
initial:
  axis = Unknown

after first reliable estimate:
  axis = Vertical or Horizontal

later estimates:
  only accept same axis
  cross-axis movement must be below tolerance
```

例如 vertical 模式：

```text
abs(dx) <= cross_axis_tolerance
abs(dy) >= min_append
```

horizontal 模式：

```text
abs(dy) <= cross_axis_tolerance
abs(dx) >= min_append
```

若中途真的偵測到另一個主軸，v0.2 不應自動切換成 2D。  
應回傳：

```text
NoMatch / AxisChanged
```

並建議未來使用 Mosaic2D mode。

---

## 9. AutoHybridMatcher

v0.2 只保留一個最佳內部方案：

```text
AutoHybridMatcher
```

使用者不需要知道它底下用了哪些 matcher。

### 9.1 Matcher pipeline

建議順序：

```text
1. DuplicateDetector
2. CoarseDownscaled2DMatcher
3. AxisAwareTemplateMatcher
4. EdgeOrColumnMatcher
5. AkazeFeatureMatcher fallback
6. PixelOverlapVerifier
7. ConfidenceRanker
```

### 9.2 為什麼是 hybrid，而不是只用 AKAZE

AKAZE 對圖片、地圖、卡片型內容有幫助，但對純文字、重複 row、表格不一定比 template 穩。  
而且 AKAZE 成本較高，不適合每一幀第一順位都跑。

因此 v0.2 的原則：

```text
便宜 matcher 先找候選 motion
AKAZE 只在不確定或低信心時 fallback
所有候選都必須經過 pixel verifier
最終只輸出一個 MotionEstimate
```

---

## 10. CoarseDownscaled2DMatcher

### 10.1 目的

即使 LinearScroll 最終只支援單軸，也應該先估 2D motion，才能自動判斷主軸。

流程：

```text
downscale prev/current，例如 4x 或 8x
在小圖上搜尋 dx/dy
得到 rough motion
回原圖在 rough ± N pixels 做精修
```

### 10.2 搜尋限制

避免搜尋整張圖造成過慢：

```text
max_dx = frame.width  * max_search_ratio
max_dy = frame.height * max_search_ratio
```

例如：

```text
max_search_ratio = 0.75
```

v0.2 可以根據 axis lock 優化：

```text
axis unknown:
  search both dx/dy

axis vertical:
  search dy，dx 只允許小範圍

axis horizontal:
  search dx，dy 只允許小範圍
```

---

## 11. AxisAwareTemplateMatcher

### 11.1 Vertical candidate

```text
取 current frame 的上方 content ROI template
在 previous frame 中搜尋對應位置
估 dy
驗證 prev[dy..height] vs curr[0..height-dy]
```

同時支援：

```text
dy > 0 append bottom
dy < 0 append top
```

### 11.2 Horizontal candidate

```text
取 current frame 左側 / 右側 content ROI template
在 previous frame 中搜尋對應位置
估 dx
驗證 prev[dx..width] vs curr[0..width-dx]
```

同時支援：

```text
dx > 0 append right
dx < 0 append left
```

### 11.3 Second-best margin

若最佳候選與第二候選差距太小，代表畫面可能有重複 row / grid / code blocks。  
此時不要貿然 append，而應：

```text
1. 降低 confidence
2. 進入 AKAZE fallback
3. 或回傳 NoMatch
```

---

## 12. AKAZE fallback

### 12.1 角色

AKAZE 在 v0.2 是 internal fallback：

```text
不是使用者手動選項
不是唯一 matcher
不是每一幀必跑
```

適用場景：

```text
圖片型頁面
卡片型列表
地圖 / diagram
template 分數不穩
重複文字導致 template ambiguous
```

### 12.2 不做 OpenCV ORB 移植

v0.2 不移植 OpenCV ORB。理由：

```text
ORB 移植成本高
完整 ORB 包含 FAST / Harris ranking / pyramid / orientation / rotated BRIEF / rBRIEF
對 rollshot 而言不是核心價值
AKAZE crate 已可作為 feature extractor
rollshot 只需要 motion voting，不需要完整 affine / homography
```

### 12.3 AKAZE motion voting

AKAZE 不做 affine。  
只做 2D translation voting。

每個 match：

```text
prev point: (px, py)
curr point: (cx, cy)

dx = px - cx
dy = py - cy
```

投票：

```text
bucket(dx, dy)
找 dominant movement vector
用 inliers 算 median dx/dy
```

過濾：

```text
abs(dx) <= max_dx
abs(dy) <= max_dy
若 axis 已鎖定，cross-axis movement 必須小
raw_matches >= threshold
inliers >= threshold
inlier_ratio >= threshold
```

最後仍要：

```text
PixelOverlapVerifier
```

### 12.4 AKAZE config

```rust
#[derive(Debug, Clone)]
pub struct AkazeConfig {
    pub enabled: bool,
    pub max_features: usize,
    pub detector_threshold: f32,
    pub min_raw_matches: usize,
    pub min_inliers: usize,
    pub min_inlier_ratio: f32,
    pub max_cross_axis_px: i32,
}
```

預設：

```text
enabled = true if feature compiled
max_features = 1200
detector_threshold = 0.001
min_raw_matches = 24
min_inliers = 16
min_inlier_ratio = 0.35
```

---

## 13. PixelOverlapVerifier

### 13.1 目的

不管 motion 來源是 template、coarse search、edge、AKAZE，最後都要用像素重疊驗證。

```text
candidate MotionEstimate
→ compute overlap region
→ compare prev overlap vs curr overlap
→ accept / reject
```

### 13.2 Overlap region

對 `dx, dy`，兩張 frame 的重疊區域為：

```text
prev_rect = intersection(prev frame rect, current frame rect shifted by dx/dy)
curr_rect = prev_rect shifted back to current coordinate
```

不能只寫 vertical 的：

```text
prev[dy..height] vs curr[0..height-dy]
```

要統一成 2D overlap 計算。

### 13.3 Verifier score

可以組合：

```text
mean absolute difference
normalized cross correlation
edge difference
downsampled grayscale diff
masked ROI diff
```

v0.2 建議先用：

```text
downsampled grayscale MAD
full-resolution ROI MAD
second-best margin
```

### 13.4 動態區域處理

應降低以下區域權重：

```text
cursor
sticky header
loading spinner
video / animation area
scrollbar
portal crop 邊界
```

v0.2 先以 ROI 排除為主。v0.2.1 在 canvas append 端加上 static region mask（見 3.2.1），處理 sticky header / footer / sidebar 在輸出長圖上的視覺重複。其餘動態區域（cursor / loading spinner / video / animation）仍延後處理；semantic mask 留到 v0.5+。

---

## 14. Axis-aware append

### 14.1 Linear canvas

LinearScroll 仍然輸出一張長圖或寬圖。  
不需要 Mosaic2D 的任意位置貼圖，但要支援四個方向 append。

```rust
pub struct LinearCanvas {
    image: RgbaImage,
    axis: Option<ScrollAxis>,
}
```

### 14.2 Append bottom

```text
dy > 0
new_slice = current bottom non-overlap region
append to output bottom
```

### 14.3 Append top

```text
dy < 0
new_slice = current top non-overlap region
prepend to output top
```

### 14.4 Append right

```text
dx > 0
new_slice = current right non-overlap region
append to output right
```

### 14.5 Append left

```text
dx < 0
new_slice = current left non-overlap region
prepend to output left
```

### 14.6 Axis lock mismatch

若目前 output 已是 vertical，但收到 reliable horizontal motion：

```text
return AxisChanged / NoMatch
```

不要在 LinearScroll 裡轉成 2D。

---

## 15. Stitch outcome

```rust
#[derive(Debug)]
pub enum StitchOutcome {
    FirstFrame,

    Appended {
        direction: AppendDirection,
        added: u32,
        estimate: MotionEstimate,
    },

    NoProgress {
        estimate: Option<MotionEstimate>,
    },

    Duplicate,

    NoMatch {
        reason: NoMatchReason,
        best_estimate: Option<MotionEstimate>,
    },

    AxisChanged {
        previous_axis: ScrollAxis,
        new_axis: ScrollAxis,
        estimate: MotionEstimate,
    },
}

#[derive(Debug)]
pub enum NoMatchReason {
    LowConfidence,
    AmbiguousAxis,
    InsufficientOverlap,
    OverlapVerificationFailed,
    NotEnoughFeatures,
    MotionTooSmall,
}
```

---

## 16. Capture abstraction 現況

v0.1 capture backend 已完成，v0.2 不重寫 capture layer。

仍維持：

```rust
pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions) -> anyhow::Result<Box<dyn FrameStream>>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> anyhow::Result<CapturedFrame>;
}
```

v0.2 只做必要修正：

```text
frame timestamp 更穩定
dump-frames 更完整
probe output 更容易附 issue
capture backend 錯誤訊息改善
```

---

## 17. CLI 行為

### 17.1 v0.2 capture

v0.2 保持目前可用模式：

```bash
rollshot capture --output out.png
rollshot capture --backend linux-portal --region portal --output out.png
rollshot capture --backend macos-sck --region "100,200 900x700" --output out.png
```

### 17.2 不暴露 algorithm

不建議：

```bash
rollshot capture --algorithm akaze
rollshot capture --algorithm template
```

建議：

```bash
rollshot capture --output out.png
```

內部使用 `AutoHybridMatcher`。

可保留 debug-only：

```bash
rollshot stitch-folder ./frames --debug-match-report report.json
rollshot stitch-folder ./frames --disable-akaze
rollshot stitch-folder ./frames --dump-overlap-debug ./debug
```

### 17.3 v0.3 interactive capture

v0.3 才處理：

```bash
rollshot capture
# select region
# start
# user scrolls
# user stops
# output
```

v0.2 不要求完成 stop hotkey / floating control。

---

## 18. Fixture 與測試策略

### 18.1 v0.2 必備 fixtures

```text
tests/fixtures/
  linear_vertical_down/
  linear_vertical_up/
  linear_horizontal_right/
  linear_horizontal_left/
  sticky_header/
  repeated_rows/
  repeated_grid/
  low_feature_text/
  image_cards/
  akaze_fallback/
  bad_frame/
  duplicate_frames/
```

### 18.2 Golden expected data

每組 fixture 建議包含：

```text
frames/
  frame_000.png
  frame_001.png
  frame_002.png

expected/
  output.png
  motions.json
```

`motions.json`：

```json
[
  { "frame": 1, "dx": 0, "dy": 180, "direction": "Bottom" },
  { "frame": 2, "dx": 0, "dy": 176, "direction": "Bottom" }
]
```

### 18.3 核心測試

```text
duplicate frame should not append
vertical down should append bottom
vertical up should prepend top
horizontal right should append right
horizontal left should prepend left
ambiguous axis should reject
axis change in LinearScroll should reject
AKAZE fallback should recover when template is ambiguous
bad frame should not poison anchor
sticky header should not dominate motion
```

### 18.4 Capture 測試不作為 v0.2 重點

v0.1 已驗證 capture 可用。  
v0.2 capture test 維持 smoke：

```text
macOS smoke
KDE 6 Wayland smoke
pixel format conversion
VideoCrop
stride
```

---

## 19. CI/CD 更新

PR 必跑：

```text
cargo fmt
cargo clippy
cargo test --workspace
core golden fixture tests
fake backend integration tests
```

AKAZE 可以有兩種策略：

```text
A. PR 必跑含 --features akaze
B. 一般 PR 跑 core，nightly 跑 --features akaze
```

建議 v0.2 早期採 A，確保 fallback 不壞：

```bash
cargo test --workspace --features akaze
```

若 build time 過高，再改為 nightly / targeted job。

### 19.1 Match report artifacts

v0.2 建議在失敗時輸出：

```text
target/test-artifacts/
  fixture-name/
    report.json
    overlap_prev.png
    overlap_curr.png
    diff.png
    matches.png
```

這會大幅加速 tuning。

---

## 20. 風險與對策

| 風險 | 對策 |
|---|---|
| 方向自動偵測錯誤 | axis_ratio_threshold + second-best margin + overlap verifier。 |
| LinearScroll 中途真的換軸 | v0.2 直接 reject，提示未來 Mosaic2D。 |
| AKAZE 太慢 | 只作 fallback；限制 max_features；只在低信心時執行。 |
| AKAZE 對文字頁不穩 | 不把 AKAZE 放第一順位；template / edge 先跑。 |
| 重複 row / grid 誤判 | second-best margin、AKAZE fallback、overlap verifier。 |
| sticky header / sidebar 干擾 | matcher：content ROI 排除 top/bottom/side；canvas append：v0.3 overlap-and-overwrite topology（見 3.2.1）對 sticky header / footer / 純色 sidebar / 裝飾邊框天生 cover。 |
| 使用者期待 2D stitching | 明確區分 LinearScroll 與 Mosaic2D；v0.5+ 再做。 |
| 使用者不想輸入 max frames / fps | v0.3 做 interactive stop UX。 |
| OpenCV ORB 依賴痛苦 | 不納入；AKAZE + rollshot-specific motion voting。 |

---

## 21. v0.2 實作順序

### Step 1：型別重構

```text
dy-only OffsetEstimate
→ MotionEstimate { dx, dy, axis, direction }

append-bottom only
→ append top / bottom / left / right

StitchOutcome
→ 加入 direction / AxisChanged / NoMatchReason
```

### Step 2：2D overlap verifier

```text
實作 generic overlap rect
支援任意 dx/dy
支援 MAD / NCC score
支援 debug diff output
```

### Step 3：Axis-aware LinearScroll

```text
axis unknown
→ 第一個 reliable estimate 決定 axis
→ axis lock
→ cross-axis tolerance
→ append direction
```

### Step 4：Template / coarse matcher 改成 motion candidate

```text
不要只回 dy
回 MotionCandidate { dx, dy, score, method }
```

### Step 5：AKAZE fallback

```text
加 optional akaze feature
extract keypoints
Hamming matching
2D vector voting
axis-aware filtering
overlap verifier
```

### Step 6：AutoHybridMatcher

```text
整合候選
排序
fallback
verifier
回傳單一最佳 MotionEstimate
```

### Step 7：Fixtures / CI

```text
vertical down/up
horizontal left/right
ambiguous axis
akaze fallback
sticky header
bad frame
```

---

## 22. v0.3 規劃：interactive capture

v0.3 重點：

```text
選區 UX
像錄影一樣隨時停止
progress / preview
clipboard
macOS overlay selector
Linux portal picker polish
```

核心行為：

```text
rollshot capture
→ select source / region
→ start frame stream
→ user scrolls
→ user stops
→ finish stitching
→ output / clipboard
```

不再要求一般使用者理解：

```text
fps
max frames
capture duration
algorithm
```

這些只留給 debug / expert options。

---

## 23. v0.5+ 規劃：Mosaic2D

Mosaic2D 是獨立模式。

### 23.1 為什麼不放 v0.2

LinearScroll 的本質：

```text
單一主軸
append long image
```

Mosaic2D 的本質：

```text
任意 dx/dy
frame pose
global canvas
canvas 可向四周擴張
可能需要 drift correction
```

兩者資料模型不同。  
v0.2 不應把 Mosaic2D 提早混進 LinearScroll。

### 23.2 未來資料模型

```rust
pub struct FramePose {
    pub frame_id: usize,
    pub x: i32,
    pub y: i32,
}

pub struct MosaicCanvas {
    pub image: RgbaImage,
    pub origin_x: i32,
    pub origin_y: i32,
}
```

### 23.3 未來功能

```text
任意平移
斜向移動
minimap / map panning
whiteboard / diagram panning
canvas unknown area
loop closure
global pose optimization
blending
```

---

## 24. 推薦最終方向

v0.2 應該聚焦：

```text
讓 LinearScroll 變成可靠、聰明、自動。
```

不是：

```text
做更多平台
做 GUI
做 Mosaic2D
讓使用者選一堆算法
```

最重要的設計原則：

```text
使用者只需要操作畫面，不需要理解算法。
rollshot 內部自己選最佳 matcher。
LinearScroll 自己判斷垂直或水平。
Mosaic2D 獨立到未來版本。
```

---

## 25. v0.2 Checklist

```text
[ ] MotionEstimate 取代 dy-only OffsetEstimate
[ ] ScrollAxis / AppendDirection 型別完成
[ ] LinearCanvas 支援 top / bottom / left / right
[ ] generic 2D overlap verifier
[ ] AutoHybridMatcher
[ ] CoarseDownscaled2DMatcher
[ ] AxisAwareTemplateMatcher
[ ] AKAZE fallback
[ ] axis detection
[ ] axis lock
[ ] AxisChanged outcome
[ ] fixture: vertical down
[ ] fixture: vertical up
[ ] fixture: horizontal right
[ ] fixture: horizontal left
[ ] fixture: repeated rows
[ ] fixture: sticky header
[ ] fixture: AKAZE fallback
[ ] match debug report
[ ] CI 跑 core + fixtures + optional AKAZE
```

