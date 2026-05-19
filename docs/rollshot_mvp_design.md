# rollshot MVP 設計文件

> 目標：以 Rust 重新設計 wayscrollshot。保留原 原 wayscrollshot 的 scroll stitching 演算法概念，但重寫 capture layer，使第一階段至少支援 **KDE 6 Wayland** 與 **macOS**。  
> 專案採用 Cargo workspace，並將 `obs-studio`、`scap` 作為 submodule 放在 `learn-projects/` 下，作為長期參考資料。

---

## 1. 背景與核心問題

原本 wayscrollshot 的 Linux Wayland capture 流程主要依賴：

```text
slurp  → 使用者選區
grim   → 對該區域連續截圖
```

這在 wlroots compositor，例如 Sway、Hyprland 上較常見，但在 **KDE Plasma 6 / KWin Wayland** 上不穩定，因為 KDE 不一定支援 `grim` 所依賴的 wlroots screencopy 路線。

重寫後的目標不是「找到 grim 的另一個 CLI 替代品」，而是改成：

```text
platform-native capture backend
→ 統一輸出 RgbaImage frame stream
→ rollshot stitching core
→ 輸出長截圖
```

第一階段明確支援：

```text
Linux KDE 6 Wayland：xdg-desktop-portal ScreenCast + PipeWire
macOS：ScreenCaptureKit / scap-style backend
```

---

## 2. 重要決策總覽

| 決策 | 結論 |
|---|---|
| 是否直接 fork scap | 不作為第一選擇。scap 可參考 / macOS 可包裝，但 Linux KDE backend 建議自己寫。 |
| 是否直接照搬 OBS | 不直接搬程式碼；OBS GPL 授權與 OBS 架構都不適合直接嵌入。以 OBS Linux PipeWire 流程作為主要參考。 |
| macOS backend | 優先用 scap 或 scap-style ScreenCaptureKit backend。 |
| Linux KDE 6 Wayland backend | 參考 OBS `plugins/linux-pipewire`，自己以 Rust 重寫 portal + PipeWire backend。 |
| select region 誰負責 | KDE Wayland 第一版交給 xdg-desktop-portal-kde picker；macOS 第一版可先手動 region，後續再做 overlay selector。 |
| stitching core | 從 wayscrollshot 抽出，與 capture backend 解耦。 |
| first-class output format | `image::RgbaImage`。所有 backend 都必須轉成 RGBA。 |
| workspace | 使用 Cargo workspace 拆 crates。 |
| learn-projects | 將 OBS / scap 放在 `learn-projects/`，只作參考與對照。 |

---

## 3. Repo 命名與產品定位

本專案名稱：

```text
rollshot
```

命名理由：

```text
roll  → 滾動 / 捲動
shot  → screenshot
```

定位：

```text
rollshot 是新的跨平台長截圖工具。
它不是 wayscrollshot 的 fork，也不應在 package / crate / binary 名稱中沿用 wayscrollshot。
```

建議命名：

```text
GitHub repo: rollshot
CLI binary: rollshot
workspace crates:
  rollshot-core
  rollshot-capture
  rollshot-cli
  rollshot-app
```

原 wayscrollshot 僅作為演算法參考：

```text
learn-projects/wayscrollshot/   # optional reference only
```

## 4. 參考專案定位

### 4.1 原 wayscrollshot

用途：參考 scroll stitching 演算法、CLI 行為、session 狀態流程。

重要檔案：

```text
wayscrollshot-master/src/capture.rs    # slurp + grim，目前要替換
wayscrollshot-master/src/session.rs    # capture loop + stitcher push frame
wayscrollshot-master/src/stitch.rs     # 核心 stitching 演算法
wayscrollshot-master/src/types.rs      # state / stats 型別
```

原設計中最值得保留的是：

```text
RgbaImage frame
→ Stitcher::push_frame(frame)
→ StitchOutcome
→ append new slice
```

原設計中要替換的是：

```text
select_region() 依賴 slurp
capture_frame() 依賴 grim
overlay 依賴 Linux Wayland layer-shell
```

---

### 4.2 scap

用途：

```text
macOS backend 主要參考 / 可能直接包裝使用
Linux backend 僅參考，不建議直接依賴 as-is
```

重要檔案：

```text
scap-main/src/capturer/engine/mac/mod.rs
scap-main/src/capturer/engine/mac/pixel_buffer.rs
scap-main/src/capturer/engine/mac/pixelformat.rs
scap-main/src/capturer/engine/linux/portal.rs
scap-main/src/capturer/engine/linux/mod.rs
scap-main/src/frame/video.rs
```

macOS 端值得參考：

```text
SCShareableContent
SCContentFilter
SCStreamConfiguration
SCStream
crop_area
fps
show_cursor
BGRA / RGB frame conversion
screen recording permission
```

Linux 端目前要小心：

```text
OpenPipeWireRemote 流程疑似未完整接上
Linux target list 不完整
crop_area 未完整落地
frame enum / type wrapper 可能需要確認
format / stride / metadata handling 不如 OBS 完整
```

因此：

```text
macOS：scap 可直接參考甚至先包裝
Linux：scap 不作為主 backend 依賴
```

---

### 4.3 OBS Studio

用途：

```text
Linux KDE 6 Wayland backend 的主要參考
```

重要檔案：

```text
obs-studio-master/plugins/linux-pipewire/screencast-portal.c
obs-studio-master/plugins/linux-pipewire/portal.c
obs-studio-master/plugins/linux-pipewire/pipewire.c
obs-studio-master/plugins/linux-pipewire/formats.c
obs-studio-master/plugins/linux-pipewire/pipewire.h
```

OBS 值得參考的點：

```text
CreateSession
SelectSources
Start
OpenPipeWireRemote
PipeWire stream connect
KDE portal 多 stream workaround
SPA_META_VideoCrop
cursor metadata
format negotiation
BGRA / RGBA / BGRx / RGBx
```

OBS 不適合直接搬的點：

```text
OBS 是 GPL-2.0 授權，需要避免直接複製大量程式碼到 MIT 專案
OBS capture output 是 GPU texture / render pipeline，不是 RgbaImage
OBS lifecycle 綁 OBS source / settings / render 系統
```

本專案應該學 OBS 的「流程與坑點」，而不是直接 copy C code。

---

## 5. 授權策略

目前參考專案授權概況：

```text
wayscrollshot：MIT
scap：MIT
OBS Studio：GPL-2.0
```

建議：

```text
1. 本專案若想維持 MIT / Apache / dual license，不要直接複製 OBS 程式碼。
2. OBS 只能作為行為參考、流程參考、bug workaround 參考。
3. scap 是 MIT，macOS backend 若要搬用，授權風險較低，但仍要保留 notice。
4. 若大量使用 OBS 程式碼，本專案可能需要 GPL 相容授權。
```

建議文件化：

```text
learn-projects/obs-studio/ 僅供研究參考，不編譯進本專案。
learn-projects/scap/ 可視情況作為依賴、submodule、fork 或 vendor 來源。
```

---

## 6. 專案目標

### 6.1 v0.1 目標

```text
KDE 6 Wayland 可使用 portal picker 選 region
macOS 可使用手動 region 或簡單 selector
兩平台都能取得連續 frame stream
兩平台都輸出 RgbaImage
接上 rollshot stitching core
輸出 PNG 長截圖
```

### 6.2 非 v0.1 目標

```text
Windows 支援
Linux X11 支援
GNOME 特別調校
跨平台漂亮 overlay selector
錄影輸出 mp4
音訊 capture
OBS 等級 DMA-BUF / GPU zero-copy
完整 GUI
完整 window list browser
```

---

## 7. Workspace 架構

建議 repo 結構：

```text
rollshot/
  Cargo.toml
  README.md
  LICENSE

  crates/
    rollshot-core/
      Cargo.toml
      src/
        lib.rs
        stitcher.rs
        matcher.rs
        duplicate.rs
        frame_signature.rs
        image_ext.rs
        config.rs
        types.rs

    rollshot-capture/
      Cargo.toml
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
          scap_backend.rs
          permission.rs
          pixel_buffer.rs

    rollshot-cli/
      Cargo.toml
      src/
        main.rs
        args.rs
        command_capture.rs
        command_probe.rs
        logging.rs

    rollshot-app/
      Cargo.toml
      src/
        main.rs
        selector.rs
        preview.rs

  learn-projects/
    obs-studio/      # git submodule, reference only
    scap/            # git submodule, reference / possible backend source
    wayscrollshot/   # optional original project reference

  docs/
    architecture.md
    linux-kde-wayland.md
    macos-screencapturekit.md
    stitching-algorithm.md
    roadmap.md

  examples/
    dump_frames.rs
    capture_region.rs
    stitch_from_folder.rs

  tests/
    fixtures/
      scroll_frames/
      repeated_frames/
      low_feature_frames/
```

---

## 8. Root Cargo.toml

建議：

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
```

---

## 9. Core crate 設計

### 9.1 rollshot-core 職責

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
→ offset estimation
→ append new slice
→ long image out
```

---

### 9.2 核心型別

```rust
use image::RgbaImage;

#[derive(Debug, Clone)]
pub struct StitchConfig {
    pub algorithm: MatchAlgorithm,
    pub min_overlap: u32,
    pub min_append: u32,
    pub accept_diff: f32,
    pub match_width: u32,
    pub duplicate_threshold: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAlgorithm {
    Fast,
    Template,
    ColumnSample,
    Edge,
    Orb,
}

#[derive(Debug)]
pub enum StitchOutcome {
    FirstFrame,
    Appended { added: u32 },
    NoProgress,
    NoMatch { confidence: f32 },
    Duplicate,
}

pub struct Stitcher {
    config: StitchConfig,
    full_image: Option<RgbaImage>,
    last_frame: Option<RgbaImage>,
    last_offset: i32,
    stats: StitchStats,
}
```

---

## 10. Stitching 演算法設計

### 10.1 核心觀念

Scrollshot 的基本假設：

```text
使用者滾動畫面時，下一個 frame 與上一個 frame 有大段垂直重疊。
只需要估算新 frame 相對於 previous frame 的 y offset。
若 offset > min_append，代表底部有新內容可以 append。
```

流程：

```text
first frame:
  full_image = frame
  last_frame = frame

next frame:
  if duplicate(frame, last_frame):
    return Duplicate

  offset, confidence = estimate_vertical_offset(last_frame, frame)

  if confidence too bad:
    return NoMatch

  if offset < min_append:
    return NoProgress

  overlap = frame.height - offset
  new_slice = frame[overlap..frame.height]
  full_image.append_bottom(new_slice)
  last_frame = frame
```

---

### 10.2 演算法優先順序

建議 v0.1：

```text
1. duplicate detection
2. template matching
3. content-aware template fallback
4. optional Fast matcher
```

建議暫緩：

```text
OpenCV ORB
```

原因：

```text
OpenCV Rust binding 跨平台發行成本高
macOS / Linux CI 建置成本高
第一版不需要 ORB 就可以驗證整體可行性
```

---

### 10.3 Duplicate detection

目的：避免使用者還沒滾動時，同一畫面反覆 append 或誤判。

建議做法：

```text
1. 將 frame downsample 到小尺寸，例如 64x64 或 96x96
2. 轉 grayscale
3. 計算 mean absolute difference 或 hash distance
4. 若差異低於 threshold，視為 Duplicate
```

Pseudo code：

```rust
pub fn is_duplicate(a: &RgbaImage, b: &RgbaImage, threshold: f32) -> bool {
    let sig_a = FrameSignature::from_image(a);
    let sig_b = FrameSignature::from_image(b);
    sig_a.mean_abs_diff(&sig_b) < threshold
}
```

---

### 10.4 Template matching

沿用原 wayscrollshot 的概念：

```text
從 current frame 上方略過 top noise，切出一段 template
在 previous frame 中沿著 y 軸搜尋最相似位置
用 NCC score 或 mean absolute difference 找最佳 offset
```

建議：

```text
template_y = frame.height * 0.05
template_h = frame.height * 0.20
search_y = [template_y, frame.height - template_h]
```

結果：

```text
best_offset = search_y - template_y
confidence = 1.0 - best_score
```

---

### 10.5 Content-aware ROI

有些頁面上方有 sticky header、透明 overlay、游標、動畫。直接拿全寬 template 容易誤判。

建議增加 content ROI：

```text
排除最上方 5% ~ 10%
排除最下方 5%
排除左右邊界少量 padding
可根據邊緣密度或像素變化找到有效內容區
```

流程：

```text
roi = content_roi(width, height)
template = current_frame[roi.x..roi.x+roi.w, roi.y..roi.y+template_h]
在 previous frame 的 roi 區域內做 NCC
用 second-best margin 避免多個相似區塊誤判
用 overlap verification 再確認
```

---

### 10.6 Confidence / NoMatch 策略

每次 offset estimation 回傳：

```rust
pub struct OffsetEstimate {
    pub dy: i32,
    pub confidence: f32,
    pub method: MatchAlgorithm,
}
```

判斷：

```text
confidence > accept_diff → NoMatch
0 < dy < min_append       → NoProgress
dy >= min_append          → Appended
```

若連續多次 NoMatch：

```text
保留 anchor frame，不要立刻更新 last_frame
避免因為壞 frame 讓後續都接不上
```

這是原 wayscrollshot 在 ORB 模式中很重要的想法，也可以搬到 template fallback 中。

---

## 11. Capture abstraction

### 11.1 Capture crate 職責

`rollshot-capture` 負責：

```text
平台原生 capture API
select source / region
frame stream lifecycle
pixel format conversion
stride / crop metadata
轉成 image::RgbaImage
```

不負責：

```text
stitching
output PNG
CLI parsing
preview UI
```

---

### 11.2 核心 trait

```rust
use image::RgbaImage;

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions) -> anyhow::Result<Box<dyn FrameStream>>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> anyhow::Result<CapturedFrame>;
}

pub struct CapturedFrame {
    pub image: RgbaImage,
    pub timestamp: std::time::SystemTime,
    pub metadata: FrameMetadata,
}

#[derive(Debug, Clone)]
pub struct CaptureOptions {
    pub region: RegionMode,
    pub fps: u32,
    pub show_cursor: bool,
    pub prefer_portal_region: bool,
}

#[derive(Debug, Clone)]
pub enum RegionMode {
    Manual(Region),
    PortalPicker,
    FullSource,
}

#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
```

---

### 11.3 Frame metadata

```rust
#[derive(Debug, Clone)]
pub struct FrameMetadata {
    pub source_size: Option<Size>,
    pub effective_region: Option<Region>,
    pub scale_factor: Option<f64>,
    pub pixel_format: Option<PixelFormat>,
    pub stride: Option<u32>,
    pub backend: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    Rgba,
    Bgra,
    Bgrx,
    Rgbx,
    Rgb,
}
```

---

## 12. Linux KDE 6 Wayland backend

### 12.1 核心決策

Linux KDE 6 Wayland 第一版不使用：

```text
grim
slurp
自製 Wayland overlay selector
X11 fallback
```

使用：

```text
xdg-desktop-portal ScreenCast
xdg-desktop-portal-kde picker
PipeWire stream
OBS-style lifecycle
```

---

### 12.2 select region 誰負責

KDE 6 Wayland 第一版：

```text
select region 由 KDE portal picker 負責
```

也就是：

```text
rollshot 呼叫 ScreenCast portal
→ KDE 出現 source picker
→ 使用者選 Rectangular Region
→ portal 回傳 PipeWire stream
→ wayscrollshot 讀 frame
```

這樣避免自己做 Wayland 全螢幕透明 overlay。

注意：

```text
XDG ScreenCast 標準 source type 主要是 monitor / window / virtual。
KDE 的 Rectangular Region 是 xdg-desktop-portal-kde 實作能力。
因此 backend 要能處理：
A. stream 已經是 region size
B. stream 是 full source，但帶 SPA_META_VideoCrop
```

---

### 12.3 Linux portal lifecycle

參考 OBS：

```text
ensure portal proxy
→ CreateSession
→ SelectSources
→ Start
→ parse streams
→ OpenPipeWireRemote
→ connect PipeWire core using fd
→ connect stream by node id
→ receive buffers
```

Rust module 對應：

```text
linux/portal.rs
  - PortalConnection
  - ScreencastSession
  - create_session()
  - select_sources()
  - start()
  - open_pipewire_remote()

linux/pipewire.rs
  - PipeWireContext
  - PipeWireStream
  - next_frame()

linux/pipewire_format.rs
  - format negotiation
  - BGRA/RGBA/BGRx/RGBx conversion

linux/pipewire_metadata.rs
  - SPA_META_VideoCrop
  - cursor metadata if needed
  - transform metadata if needed later
```

---

### 12.4 SelectSources options

建議：

```text
types = MONITOR | WINDOW
multiple = false
cursor_mode = HIDDEN by default
persist_mode = 2 if portal version supports it
restore_token = optional future feature
```

cursor 建議預設 hidden，因為游標會干擾 stitching。

---

### 12.5 OBS KDE workaround

OBS 有處理 KDE portal 可能回傳多個 streams 的情況。

本專案也應該做：

```text
if streams.len() == 0:
  error
if streams.len() == 1:
  use streams[0]
if streams.len() > 1:
  log warning
  use streams.last()
```

原因：OBS source 內有註解指出 KDE Desktop portal 有時會回傳多個 stream，且最後一個才是需要的 stream。

---

### 12.6 OpenPipeWireRemote 是必須流程

不要只拿 portal 回傳的 node id 直接接 default PipeWire context。

正確流程：

```text
Start 回傳 stream node id
OpenPipeWireRemote 回傳 fd
用 fd 建立 PipeWire remote connection
用 node id connect stream
```

這是 OBS 的完整做法，也是 Linux Wayland portal 模型下較正確的做法。

---

### 12.7 VideoCrop metadata

PipeWire buffer 可能帶：

```text
SPA_META_VideoCrop
```

本專案必須支援：

```text
if crop metadata exists:
  crop buffer to metadata region before returning RgbaImage
else:
  use full frame or manual region crop
```

不要假設：

```text
frame.width == selected_region.width
frame.height == selected_region.height
```

---

### 12.8 Pixel format support

v0.1 支援：

```text
BGRA → RGBA
RGBA → RGBA
BGRx → RGBA
RGBx → RGBA
RGB  → RGBA
```

暫緩：

```text
YUV
NV12
10-bit formats
DMA-BUF GPU zero-copy
```

Reason：rollshot 需要 CPU 上的 `RgbaImage`，不是 GPU texture。

---

### 12.9 Stride handling

PipeWire buffer 的每行 bytes 可能不是：

```text
width * bytes_per_pixel
```

必須使用 stride。

轉換流程：

```text
for y in 0..height:
  src_row = data + y * stride
  for x in 0..width:
    convert pixel to RGBA
```

不要直接假設 raw buffer 是緊密排列。

---

### 12.10 Linux probe command

建議做：

```bash
rollshot probe
```

輸出：

```text
OS: Linux
Session: wayland
Desktop: KDE
Portal Desktop: available
ScreenCast interface: available
PipeWire: available
Cursor modes: metadata/hidden/embedded
Source types: monitor/window
KDE region picker: unknown until Start
```

這對 debug KDE 6 很重要。

---

## 13. macOS backend

### 13.1 核心決策

macOS 使用：

```text
ScreenCaptureKit
```

第一版可選：

```text
A. 直接包裝 scap macOS backend
B. 參考 scap 寫自己的 macOS backend
```

建議先走 A，加速 MVP。

---

### 13.2 macOS capture flow

```text
check screen recording permission
→ request permission if needed
→ get shareable content
→ select display
→ build content filter
→ build stream configuration
→ set source rect / crop area
→ start SCStream
→ receive sample buffers
→ convert BGRA to RGBA
→ return RgbaImage
```

---

### 13.3 macOS select region

macOS 沒有 KDE portal picker 這種同等抽象。

v0.1：

```text
手動 region
wayscrollshot --region "100,200 900x700"
```

v0.2：

```text
自製透明 overlay selector
```

macOS overlay selector 相對 Linux Wayland 容易一些，但仍需處理：

```text
Retina scale factor
多螢幕座標
Screen Recording 權限
視窗層級
notch / menu bar
```

---

### 13.4 Retina / scale

要明確區分：

```text
logical points
physical pixels
```

`RgbaImage` 應該使用 physical pixel 尺寸，因為 stitching 是 pixel-level matching。

Region selector 若回傳 logical coordinates，必須轉換成 physical pixel coordinates。

---

## 14. CLI 設計

### 14.1 Capture command

```bash
# auto backend
rollshot capture --output out.png

# KDE 6 Wayland，使用 portal picker 選 region
rollshot capture --backend linux-portal --region portal --output out.png

# macOS, 手動 region
rollshot capture --backend macos-sck --region "100,200 900x700" --output out.png

# debug dump frames
rollshot capture --backend linux-portal --region portal --dump-frames ./frames
```

---

### 14.2 Probe command

```bash
rollshot probe
```

目的：

```text
檢查平台、backend、權限、portal、PipeWire、ScreenCaptureKit 狀態
```

---

### 14.3 Stitch from folder

```bash
rollshot stitch-folder ./frames --output out.png
```

用途：

```text
debug 演算法
capture backend 與 stitching core 解耦測試
CI 測試 fixture
```

---

## 15. backend auto detection

```rust
pub fn default_backend() -> BackendKind {
    #[cfg(target_os = "macos")]
    {
        return BackendKind::MacosScreenCaptureKit;
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland") {
            return BackendKind::LinuxPortalPipeWire;
        }
        return BackendKind::Unsupported;
    }
}
```

v0.1 不支援 X11，可明確提示：

```text
Linux X11 is not supported in rollshot v0.1. Please use KDE Wayland or macOS.
```

---

## 16. 錯誤處理策略

### 16.1 使用者取消選取

```text
Portal picker cancelled
→ return UserCancelled
→ CLI 顯示簡短訊息，不視為 crash
```

### 16.2 權限不足

Linux：

```text
xdg-desktop-portal unavailable
PipeWire unavailable
ScreenCast interface unavailable
```

macOS：

```text
Screen Recording permission denied
```

### 16.3 frame format 不支援

```text
Unsupported pixel format: NV12
```

並建議：

```text
Please file an issue with backend probe output.
```

---

## 17. 測試策略

### 17.1 Core tests

```text
duplicate frame should not append
small scroll should be NoProgress
normal scroll should append expected height
bad frame should not poison anchor
low-feature frames use fallback
sticky header does not break matching
```

### 17.2 Capture tests

capture backend 很難在 CI 完整跑，但可以做：

```text
pixel format conversion unit tests
stride handling tests
crop metadata tests
manual region crop tests
portal response parser tests
```

### 17.3 Integration debug tools

```bash
rollshot capture --dump-frames ./debug_frames
rollshot stitch-folder ./debug_frames --output debug.png
rollshot probe --json
```

---


---

## 18. CI/CD 與 Integration Test 策略

### 18.1 核心原則

rollshot 的 CI/CD 必須分層，不能把所有測試都當成一般 PR 必跑項目。

原因：

```text
KDE 6 Wayland portal picker 是互動式桌面 UI
PipeWire / xdg-desktop-portal 需要真實 user session
macOS ScreenCaptureKit 需要 Screen Recording 權限
GitHub hosted runner 通常無法穩定完成真實 capture E2E
```

因此測試分成：

```text
Layer 1: pure core algorithm tests
Layer 2: fake backend integration tests
Layer 3: real Linux KDE 6 Wayland smoke tests
Layer 4: real macOS ScreenCaptureKit smoke tests
```

PR 必跑 Layer 1 / Layer 2；Layer 3 / Layer 4 放到 self-hosted runner、nightly 或 manual workflow。

---

### 18.2 Layer 1：Core algorithm tests

這層完全不碰 OS capture，也不需要 OBS / scap / PipeWire / ScreenCaptureKit。

測試範圍：

```text
stitch algorithm
duplicate detection
template matching
overlap offset estimation
crop / append behavior
content-aware ROI
bad frame handling
low-feature frame fallback
```

建議放在：

```text
crates/rollshot-core/tests/
  stitch_golden.rs
  duplicate_detection.rs
  overlap_matching.rs
  sticky_header.rs
  bad_frame_anchor.rs
```

測試資料：

```text
tests/fixtures/
  long_pages/
    simple_page.png
    sticky_header_page.png
    low_feature_page.png
  scroll_frames/
    simple/
      frame_000.png
      frame_001.png
      frame_002.png
  expected/
    simple_stitched.png
```

推薦同時支援 synthetic fixtures：

```rust
fn make_scroll_frames(long_image: &RgbaImage, viewport_h: u32, step: u32) -> Vec<RgbaImage> {
    // Crop long_image into viewport-sized frames.
    // This makes algorithm tests deterministic and OS-independent.
    todo!()
}
```

這層是 rollshot 最重要的穩定性基礎。

---

### 18.3 Layer 2：Fake backend integration tests

Fake backend 用來測「完整 rollshot session flow」，但不接真實螢幕。

設計：

```text
FakeCaptureBackend
→ FakeFrameStream
→ 從 fixtures 逐張吐 frame
→ rollshot session consume frame stream
→ stitcher output long image
→ compare expected output
```

建議放在：

```text
crates/rollshot-capture/tests/fake_backend.rs
crates/rollshot-cli/tests/cli_fake_flow.rs
```

Fake frame stream：

```rust
pub struct FakeFrameStream {
    frames: Vec<RgbaImage>,
    index: usize,
}

impl FrameStream for FakeFrameStream {
    fn next_frame(&mut self) -> anyhow::Result<CapturedFrame> {
        let frame = self.frames
            .get(self.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("end of fake stream"))?;
        self.index += 1;

        Ok(CapturedFrame {
            image: frame,
            timestamp: std::time::SystemTime::now(),
            metadata: FrameMetadata::fake(),
        })
    }
}
```

CLI fake flow：

```bash
rollshot stitch-folder tests/fixtures/scroll_frames/simple --output target/test-output/simple.png
```

或：

```bash
rollshot capture --backend fake --fixture tests/fixtures/scroll_frames/simple --output target/test-output/simple.png
```

這層應該在 Linux / macOS hosted CI 都必跑。

---

### 18.4 Layer 3：Linux KDE 6 Wayland real capture smoke tests

這層測真實 KDE 6 Wayland + portal + PipeWire。

建議不要放在一般 GitHub hosted runner 必跑，應放在：

```text
self-hosted Linux runner
labels: self-hosted, linux, kde6, wayland
```

測試前提：

```text
KDE Plasma 6 Wayland session
PipeWire running
WirePlumber running
xdg-desktop-portal running
xdg-desktop-portal-kde running
KWin screencast 可用
```

測試命令：

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture
```

測試內容不要一開始就要求完整長截圖，先只要求能拿 frame：

```text
1. 檢查 XDG_SESSION_TYPE=wayland
2. 檢查 XDG_CURRENT_DESKTOP 包含 KDE / plasma
3. 檢查 org.freedesktop.portal.ScreenCast 存在
4. CreateSession
5. SelectSources
6. Start
7. OpenPipeWireRemote
8. PipeWire connect stream
9. 收到至少 3 frames
10. frame width / height > 0
11. frame 可以轉成 RgbaImage
12. optional: 存第一張 frame 到 target/test-artifacts/
```

測試檔案：

```text
crates/rollshot-capture/tests/linux_portal_smoke.rs
```

測試應標記為 ignored：

```rust
#[test]
#[ignore = "requires real KDE 6 Wayland session, portal picker, PipeWire, and user permission"]
fn linux_portal_receives_frames() {
    if std::env::var("ROLLSHOT_REAL_CAPTURE").ok().as_deref() != Some("1") {
        return;
    }

    // Start LinuxPortalPipeWire backend using RegionMode::PortalPicker.
    // User may need to select Rectangular Region in KDE portal dialog.
    todo!()
}
```

KDE portal picker 是互動式 UI，因此這層適合：

```text
manual workflow_dispatch
nightly on self-hosted machine
release candidate smoke test
```

---

### 18.5 Layer 4：macOS ScreenCaptureKit real capture smoke tests

macOS 真實 capture 也不適合放在一般 hosted PR 必跑，因為 ScreenCaptureKit 需要 Screen Recording 權限。

建議放在：

```text
self-hosted macOS runner
labels: self-hosted, macos, screencapturekit
```

測試前提：

```text
該 runner 的 Terminal / test binary 已授予 Screen Recording 權限
可存取 main display
```

測試命令：

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

測試內容：

```text
1. check_permission() 回傳 true，或清楚回傳 permission denied
2. start ScreenCaptureKit / scap-style backend
3. manual region 或固定小 region
4. 收到至少 3 frames
5. frame format 正確
6. BGRA → RGBA conversion 正確
7. crop_area 正確
8. optional: 存第一張 frame 到 target/test-artifacts/
```

測試檔案：

```text
crates/rollshot-capture/tests/macos_sck_smoke.rs
```

Hosted macOS CI 可以做：

```text
build macOS backend
run permission-denied behavior test
run core / fake integration tests
```

但不要要求 hosted runner 一定能真正擷取螢幕。

---

### 18.6 GitHub Actions：PR CI

PR CI 目標：快速、穩定、無互動權限依賴。

建議 `.github/workflows/ci.yml`：

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, macos-14]
    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Install Linux dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            pkg-config \
            libdbus-1-dev \
            libwayland-dev \
            libpipewire-0.3-dev

      - name: Format
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --workspace --all-targets --features fake -- -D warnings

      - name: Unit and fake integration tests
        run: cargo test --workspace --features fake
```

這個 workflow 不應打開 `ROLLSHOT_REAL_CAPTURE=1`。

---

### 18.7 GitHub Actions：self-hosted real capture workflow

建議 `.github/workflows/real-capture.yml`：

```yaml
name: Real Capture Smoke

on:
  workflow_dispatch:
  schedule:
    - cron: "0 18 * * *"

jobs:
  kde6-wayland:
    runs-on: [self-hosted, linux, kde6, wayland]
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable

      - name: KDE portal smoke test
        env:
          ROLLSHOT_REAL_CAPTURE: "1"
        run: |
          cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture

  macos-sck:
    runs-on: [self-hosted, macos, screencapturekit]
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable

      - name: macOS ScreenCaptureKit smoke test
        env:
          ROLLSHOT_REAL_CAPTURE: "1"
        run: |
          cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

這個 workflow 是 smoke test，不是完整 correctness test。

完整 correctness 仍然靠：

```text
core algorithm tests
fake backend integration tests
golden image fixtures
```

---

### 18.8 Headless strategy 結論

OBS / scap 都不應被理解成「提供現成 headless capture CI recipe」。

本專案策略：

```text
OBS：參考 Linux portal + PipeWire implementation details
scap：參考 macOS ScreenCaptureKit implementation details
rollshot：自行設計 fake backend + self-hosted real capture smoke tests
```

不要把以下項目放入 PR 必跑：

```text
KDE portal 真實選區
PipeWire 真實 frame stream
macOS ScreenCaptureKit 真實 capture
多螢幕真實 E2E
Retina 真實 E2E
```

這些應該放在：

```text
self-hosted runner
nightly smoke
release candidate checklist
manual QA
```

---

### 18.9 Test artifact policy

real capture smoke test 若成功，建議輸出 artifact：

```text
target/test-artifacts/
  linux_portal_first_frame.png
  linux_portal_probe.json
  macos_sck_first_frame.png
  macos_sck_probe.json
```

artifact 不應包含敏感資訊，因此：

```text
1. 測試機最好開專用測試頁面或空白視窗
2. real smoke test 只存小範圍 frame
3. artifact retention 設短一點，例如 3 到 7 天
4. public repo 若有隱私疑慮，可只保存 probe json，不保存 screenshot
```

---

### 18.10 Release checklist

每次 release candidate 前至少手動確認：

```text
[ ] macOS main display manual region capture 可用
[ ] macOS Screen Recording permission denied 時錯誤訊息清楚
[ ] KDE 6 Wayland portal picker 可開啟
[ ] KDE 6 Wayland Rectangular Region 可收到 frames
[ ] KDE 6 若回傳 VideoCrop metadata，crop 結果正確
[ ] KDE 6 若 portal 回多 stream，使用最後一個 stream 的 workaround 正常
[ ] stitch-folder 對 golden fixtures 通過
[ ] fake backend capture flow 通過
[ ] dump-frames 可輸出 debug frames
[ ] probe --json 可輸出可附在 issue 的診斷資訊
```


## 19. 實作順序

### Phase 0：repo 初始化

```text
建立 workspace
建立 core / capture / cli crates
建立 learn-projects submodules
搬入原 rollshot stitcher 概念
加入 stitch-folder debug command
```

---

### Phase 1：core 先穩

```text
RgbaImage stitching core
duplicate detection
template matching
content-aware ROI
fixture tests
```

完成標準：

```text
stitch-folder 可以把測試 frames 合成長圖
```

---

### Phase 2：macOS backend

```text
先包裝 scap macOS backend 或 scap-style SCK backend
手動 region
BGRA → RGBA
輸出 dump frames
接 stitcher
```

完成標準：

```text
macOS 上可以手動 region 取得 scrollshot
```

---

### Phase 3：Linux KDE 6 Wayland backend

```text
實作 portal CreateSession / SelectSources / Start
實作 OpenPipeWireRemote
實作 PipeWire stream connect
支援 KDE multiple-stream workaround
支援 BGRA/RGBA/BGRx/RGBx conversion
支援 stride
支援 SPA_META_VideoCrop
接 stitcher
```

完成標準：

```text
KDE 6 Wayland 上 portal picker 選 Rectangular Region 後，可以取得 scrollshot
```

---

### Phase 4：UX polish

```text
backend auto detection
probe diagnostics
better error messages
progress output
clipboard output
macOS overlay region selector
```

---

## 20. Linux backend implementation checklist

```text
[ ] 檢查 XDG_SESSION_TYPE=wayland
[ ] 檢查 XDG_CURRENT_DESKTOP 包含 KDE / plasma
[ ] 建立 DBus connection
[ ] 建立 ScreenCast proxy
[ ] 讀 AvailableSourceTypes
[ ] 讀 AvailableCursorModes
[ ] CreateSession
[ ] SelectSources(types=MONITOR|WINDOW, multiple=false, cursor hidden)
[ ] Start
[ ] parse streams
[ ] 如果多個 streams，取最後一個並 log warning
[ ] OpenPipeWireRemote
[ ] 用 fd connect PipeWire remote
[ ] connect stream node id
[ ] negotiate formats: BGRA/RGBA/BGRx/RGBx
[ ] request SPA_META_VideoCrop
[ ] receive buffer
[ ] handle stride
[ ] apply crop metadata
[ ] convert to RgbaImage
[ ] return CapturedFrame
```

---

## 21. macOS backend implementation checklist

```text
[ ] 檢查 Screen Recording permission
[ ] request permission if needed
[ ] 取得 SCShareableContent
[ ] 選擇 main display
[ ] 建立 SCContentFilter
[ ] 建立 SCStreamConfiguration
[ ] 設定 fps
[ ] 設定 show_cursor=false
[ ] 設定 source_rect / crop_area
[ ] start stream
[ ] receive sample buffer
[ ] lock pixel buffer
[ ] read BGRA bytes
[ ] convert to RGBA
[ ] return CapturedFrame
[ ] 處理 Retina scale
```

---

## 22. 風險與對策

| 風險 | 對策 |
|---|---|
| KDE portal region picker 行為不穩 | 支援 VideoCrop metadata；支援 full stream + manual crop fallback。 |
| PipeWire 回傳 DMA-BUF | v0.1 優先 negotiate MemPtr / CPU-readable format；不做 GPU zero-copy。 |
| OBS GPL 授權 | 只參考流程，不直接複製 C code。 |
| macOS Retina 座標錯誤 | 所有 stitching 使用 physical pixels；selector 坐標要轉換。 |
| sticky header / 動畫造成 stitching 錯誤 | content-aware ROI、duplicate detection、second-best margin、verification。 |
| OpenCV ORB 發行困難 | v0.1 不納入必要路徑。 |
| 多螢幕 | v0.1 先 portal picker / main display；v0.2 再完整處理。 |

---

## 23. 推薦最終方向

最推薦的整體設計是：

```text
macOS：scap / ScreenCaptureKit style backend
Linux KDE 6 Wayland：OBS-style portal + PipeWire backend
共同抽象：CaptureBackend + FrameStream
共同輸出：image::RgbaImage
共同核心：rollshot stitching core（參考原 wayscrollshot 演算法概念）
```

不要追求所有平台共用同一個第三方 library。真正要一致的是：

```text
backend API 一致
frame output 一致
stitching input 一致
錯誤模型一致
CLI 行為一致
```

而不是底層 capture 實作一致。

---

## 24. 下一步建議

最小可行開發順序：

```text
1. 建 workspace
2. 建 core crate，先支援 stitch-folder
3. 搬 template matching / duplicate detection
4. 建 capture trait
5. macOS 先接 scap/SCK，手動 region 跑通
6. Linux 參考 OBS 寫 portal + PipeWire backend
7. KDE 6 實機驗證 portal picker + VideoCrop
8. 加 probe 與 dump-frames，方便 debug
```

當 `stitch-folder` 與 `dump-frames` 都穩後，再進入 selector / GUI / clipboard。這樣可以把問題拆成：

```text
capture 是否正確
pixel format 是否正確
stitching 是否正確
UX 是否好用
```

每一層都能獨立 debug。
