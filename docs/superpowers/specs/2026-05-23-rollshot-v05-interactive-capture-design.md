# rollshot v0.5 — Interactive Capture Session Design

> 狀態：draft  
> 前置：v0.4 (FAST+KNN fallback) 已完成  
> 參考：snow-shot (Tauri scrolling screenshot)、wayscrollshot (Wayland layer-shell)

---

## 0. 問題與動機

v0.1–v0.4 的 capture flow：

```text
rollshot capture --fps 5 --max-frames 100 --region "X,Y WxH" --output out.png
```

問題：
1. 使用者必須事先知道 fps、max-frames、region 座標
2. KDE 6 portal region picker 的 tooltip 會被截進 first frame
3. 沒有即時 preview，不知道截到什麼
4. 沒有 stop 機制，只能等 max-frames 到

v0.5 目標：

```text
rollshot capture
→ portal 選 source (monitor/window)
→ Tauri overlay 顯示截取畫面 + 選區 UI
→ 使用者確認區域 → 開始 stitching
→ 使用者捲動內容
→ 使用者按 stop
→ 輸出圖片
```

---

## 1. 決策總覽

| 決策 | 結論 |
|------|------|
| GUI framework | Tauri（穩定、跨平台、參考 snow-shot） |
| region selection | Tauri 透明 webview overlay，Canvas2D 畫選區 |
| portal 改動 | 不改。portal 仍可能顯示 region picker，不阻止 |
| tooltip 問題 | 自然解決——Tauri overlay 的選區動作產生延遲，tooltip 已消失 |
| preview (region capture) | overlay 放選區外側，不會被截進去 |
| preview (全螢幕 macOS) | overlay 正常顯示，SCK excluded_targets 排除 |
| preview (全螢幕 Linux) | 不顯示 overlay，避免被 PipeWire 截進去 |
| CLI 模式 | 保留 headless CLI（`--headless`），不啟動 Tauri |
| 舊 CLI 參數 | fps / max-frames / region 保留為 headless / expert options |

---

## 2. Workspace 架構變更

### 2.1 新增 crate（參考 snow-shot 結構）

```text
crates/
  rollshot-app/          # 改造：Tauri application（取代現有 placeholder）
    src-tauri/
      Cargo.toml         # 依賴 tauri, rollshot-core, rollshot-capture
      tauri.conf.json
      src/
        main.rs          # Tauri entry point
        lib.rs           # app setup, plugin/command registration
        commands.rs      # Tauri IPC commands
        state.rs         # shared app state
    src/                 # 前端（React + shadcn v4）
      index.html
      main.tsx
      App.tsx
      components/
        ui/              # shadcn components
        SelectionOverlay.tsx
        PreviewStrip.tsx
        ControlBar.tsx
      commands/
        capture.ts       # Tauri invoke wrappers
      lib/
        utils.ts
    package.json         # React, shadcn, tailwind
    components.json      # shadcn v4 config（參考 tauri-template）
    vite.config.ts       # or rsbuild.config.ts
    tsconfig.json
    tailwind.config.ts
```

前端技術選型：
- **UI 框架**：shadcn v4 + Tailwind CSS（參考 tauri-template 的設定）
- **框架**：React（參考 snow-shot）
- **不用**：Ant Design、styled-jsx

### 2.2 現有 crate 不動

```text
rollshot-core      — stitching，不改
rollshot-capture   — capture backend，小幅擴充（見 §3）
rollshot-cli       — 保留 headless mode，加 --headless flag
```

### 2.3 Workspace Cargo.toml 變更

```toml
[workspace.dependencies]
# 新增
tauri = { version = "2", features = ["protocol-asset"] }
tauri-build = "2"
serde = { version = "1", features = ["derive"] }
```

### 2.4 Binary 結構

```text
rollshot          # CLI binary（rollshot-cli）
rollshot-app      # Tauri GUI binary（rollshot-app）
```

CLI 預設行為改變：

```text
v0.4:  rollshot capture → 直接開始 headless capture
v0.5:  rollshot capture → 啟動 Tauri app（interactive mode）
       rollshot capture --headless → 舊行為（headless，需要 --region / --max-frames）
```

或者保持兩個獨立 binary，不合併。取決於是否要讓 CLI 依賴 Tauri。

---

## 3. Capture 層擴充

### 3.1 FrameStream 不改

```rust
pub trait FrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}
```

Tauri app 和 CLI 都消費同一個 `FrameStream`。

### 3.2 新增 client-side crop utility

```rust
// rollshot-capture/src/crop.rs

/// Crop a CapturedFrame to a sub-region.
/// Returns a new CapturedFrame with cropped image and updated metadata.
pub fn crop_frame(frame: &CapturedFrame, region: &Region) -> Result<CapturedFrame, CaptureError> {
    // validate region within frame bounds
    // image::imageops::crop_imm → to_image
    // update metadata.effective_region
}
```

用途：Tauri overlay 選區確認後，每幀過 `crop_frame` 再送進 stitcher。

### 3.3 Portal 程式碼不改

現有 `select_sources` + PipeWire 流程完全保留。
portal 可能回傳 VideoCrop metadata（使用者在 KDE 選了 region），也可能不回傳（選了整個 monitor）。
都不影響——Tauri overlay 拿到的是 PipeWire 已經送出的 frame，選區在上層處理。

---

## 4. Interactive Session Flow

### 4.1 Phase diagram

```text
┌─────────────┐
│   Launch     │  rollshot-app 啟動
└──────┬──────┘
       ▼
┌─────────────┐
│  Portal      │  portal select_sources + start
│  Source Pick  │  使用者選 monitor / window
└──────┬──────┘
       ▼
┌─────────────┐
│  PipeWire    │  frame stream 開始
│  Streaming   │  frames 送到 Tauri backend
└──────┬──────┘
       ▼
┌─────────────────┐
│  Region Select   │  Tauri overlay 顯示 live frame
│  (Tauri overlay) │  使用者拖拉選區
│                  │  portal tooltip 此時已消失
└──────┬──────────┘
       ▼
┌─────────────┐
│  Stitching   │  使用者確認選區 → 開始 stitch loop
│  Active      │  顯示 preview strip + progress
│              │  使用者捲動目標內容
└──────┬──────┘
       ▼ (使用者按 Stop / hotkey)
┌─────────────┐
│  Finish      │  stop stitch → save / clipboard
│              │  顯示結果預覽
└─────────────┘
```

### 4.2 State machine

```rust
enum SessionState {
    /// Portal source selection in progress
    SelectingSource,
    
    /// PipeWire streaming, user drawing region in Tauri overlay
    SelectingRegion {
        stream: Box<dyn FrameStream>,
    },
    
    /// Stitching active, user scrolling content
    Stitching {
        stream: Box<dyn FrameStream>,
        region: Region,
        stitcher: Stitcher,
    },
    
    /// Capture complete, showing result
    Done {
        image: RgbaImage,
    },
}
```

---

## 5. Tauri App 架構

### 5.1 Tauri Commands (IPC)

```rust
// commands.rs

#[tauri::command]
async fn start_capture(state: State<AppState>) -> Result<CaptureInfo, String>;

#[tauri::command]
async fn get_latest_frame(state: State<AppState>) -> Result<FrameData, String>;

#[tauri::command]
async fn confirm_region(state: State<AppState>, region: Region) -> Result<(), String>;

#[tauri::command]
async fn stop_capture(state: State<AppState>) -> Result<StitchResult, String>;

#[tauri::command]
async fn save_image(state: State<AppState>, path: String) -> Result<(), String>;

#[tauri::command]
async fn copy_to_clipboard(state: State<AppState>) -> Result<(), String>;
```

### 5.2 Frame 傳輸策略（參考 snow-shot）

snow-shot 的做法：
- Tauri v2 `invoke` 原生支援 `ArrayBuffer` 回傳（不需要 base64）
- macOS：PNG encode → `Response::new(png_buffer)` → 前端收到 ArrayBuffer
- Windows：WebView2 SharedBuffer 零拷貝（`PostSharedBufferToScript`）+ PNG fallback

rollshot v0.5 策略：

**Phase 1（足夠用）：Binary ArrayBuffer via Tauri invoke**
```
frame → resize to preview size → PNG encode → Response::new(buffer)
→ 前端 invoke 收到 ArrayBuffer → URL.createObjectURL(new Blob([buffer]))
→ <img src={blobUrl}>
```

不需要 base64。Tauri v2 的 invoke 直接回傳 binary。

**Scroll screenshot 每幀：大圖留 backend，只送 thumbnail**
```
frame → backend 存原圖 + stitch
     → resize to 128px 寬 thumbnail → PNG encode
     → Response::new(thumbnail_buffer + 20 bytes metadata)
     → 前端解析 thumbnail + metadata
```

參考 snow-shot：原圖不過 IPC，只送壓縮後的縮圖。

### 5.3 前端元件（shadcn + Tailwind）

```text
App
├── SelectionOverlay     # 全螢幕透明 canvas，拖拉選區
│   ├── 半透明遮罩（選區外變暗）
│   ├── 選區框 + 8 個 control points
│   └── 尺寸標示
├── PreviewStrip         # 選區外側，縮圖列
│   ├── 縮圖 (128px 寬，Blob URL)
│   ├── 負 margin 模擬重疊效果（參考 snow-shot）
│   └── 進度邊緣指示
├── ControlBar           # shadcn Button 元件
│   ├── Start / Stop
│   ├── Save / Copy
│   └── Cancel
└── StatusBar            # 狀態文字
    ├── frame count
    └── stitched height
```

### 5.4 Window 配置（參考 snow-shot create_draw_window）

snow-shot 的 draw window 是動態建立的獨立視窗，不是 main window：

```rust
// commands.rs — 動態建立 overlay 視窗

#[tauri::command]
async fn create_overlay_window(app: AppHandle) -> Result<(), String> {
    tauri::WebviewWindowBuilder::new(&app, "overlay", tauri::WebviewUrl::App("/overlay".into()))
        .transparent(true)
        .decorations(false)
        .skip_taskbar(true)
        .visible(false)         // 等 portal 完成後再 show
        .always_on_top(true)
        .inner_size(1.0, 1.0)  // 先建 1x1，之後 resize
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// portal 選完 source 後：
// 1. resize overlay window 到 source 大小
// 2. set position
// 3. show window
```

---

## 6. Overlay 顯示策略

### 6.1 Region capture（所有平台）

```text
┌─────────────────────────────────┐ Screen
│                                 │
│   ┌──────────────┐ ┌────────┐  │
│   │  capture     │ │Preview │  │
│   │  region      │ │ strip  │  │
│   │  (user pick) │ │ 128px  │  │
│   │              │ │        │  │
│   └──────────────┘ └────────┘  │
│                     gap 8px    │
│   ┌─────────────────────────┐  │
│   │ ControlBar: Stop | Save │  │
│   └─────────────────────────┘  │
└─────────────────────────────────┘
```

Preview strip 放選區右側（放不下放左側）。
不在 capture region 內 → 不會被截進去。

### 6.2 全螢幕 — macOS

```text
overlay 正常顯示。
capture 時用 SCK excluded_targets 排除 rollshot-app 視窗。
```

### 6.3 全螢幕 — Linux Wayland

```text
overlay 不顯示。
進度資訊走 terminal stdout / notification。
使用者按 hotkey (e.g. Ctrl+Shift+S) 停止。
結果在 capture 結束後顯示。
```

或者如果使用者有雙螢幕，preview 可以放到另一個螢幕。

---

## 7. Self-capture 防護

### 7.1 macOS

```rust
// 參考 snow-shot: excluded_targets
let options = scap::capturer::Options {
    excluded_targets: Some(vec![
        scap::Target::Window(rollshot_window)
    ]),
    ..
};
```

### 7.2 Linux Wayland

PipeWire 沒有 window exclusion 機制。

策略：
- Region capture: overlay 放選區外 → 不影響
- 全螢幕: 不顯示 overlay → 不影響

### 7.3 capture 前 UI 隱藏（參考 snow-shot）

每次 stitch frame 前：
1. 發事件讓 UI 元素暫時隱藏
2. 等 17ms（1 frame @ 60fps）
3. 讀取 frame
4. 恢復 UI

**但 rollshot 不需要這個**——因為 PipeWire 是持續串流，
frame 從 compositor 取得時 overlay 如果在選區外就不影響。
只有全螢幕 Linux 需要完全不顯示 overlay。

---

## 8. 實作順序

分兩份 implementation plan 執行。Plan 1 完成驗證後再產 Plan 2。

---

### Plan 1：Tauri scaffold + frame display + region selection（Phase 1 + 2）

> Plan 1 完成條件：能在 KDE 6 上啟動 rollshot-app → portal 選 source → 看到 live frame → 拖拉選區 → 回傳正確 Region。

#### Phase 1：Tauri scaffold + frame display

```text
目標：能在 Tauri 視窗中看到 PipeWire / SCK 的 live frame
驗證：啟動 rollshot-app → 選 source → 看到畫面

步驟：
[ ] rollshot-app 改造為 Tauri 專案（src-tauri/ + src/ 結構）
[ ] tauri.conf.json 基礎配置（transparent, asset protocol）
[ ] Rust: main.rs / lib.rs Tauri 啟動 + 視窗設置
[ ] Rust: start_capture command — 呼叫 CaptureBackend::start
[ ] Rust: get_latest_frame command — 從 FrameStream 讀最新幀，PNG encode 回傳 ArrayBuffer
[ ] 前端: React + shadcn scaffold（參考 tauri-template 的 shadcn 設定）
[ ] 前端: <img> 顯示 Blob URL frame
[ ] 驗證 Linux 能看到畫面
```

#### Phase 2：Region selection UI

```text
目標：在 live frame 上拖拉選區
驗證：選完區域後 console.log 出 Region，座標正確

步驟：
[ ] 前端 SelectionOverlay：透明 canvas + 拖拉邏輯
[ ] 半透明遮罩 + 選區框 + 8 個 control points
[ ] confirm_region command：前端送 Region 到 backend
[ ] crop_frame utility：backend 驗證 + crop 每幀
[ ] 驗證選區座標正確（特別是 HiDPI）
```

---

### Plan 2：Stitch + preview + stop + polish（Phase 3 + 4）

> Plan 2 完成條件：完整 interactive capture session — 選區 → 開始 → 捲動 → stop → 輸出 PNG。
> Plan 1 完成後再產此 plan，內容可能根據 Plan 1 的實際情況調整。

#### Phase 3：Stitch + preview + stop

```text
目標：完整 interactive capture session
驗證：選區 → 開始 → 捲動 → stop → 輸出 PNG

步驟：
[ ] SessionState state machine
[ ] Stitch loop 在 background thread
[ ] preview strip：縮圖列顯示在選區外（大圖留 backend，只送 thumbnail）
[ ] stop_capture command
[ ] save / clipboard
[ ] 全螢幕 Linux：suppress overlay
[ ] 全螢幕 macOS：SCK excluded_targets
```

#### Phase 4：Polish

```text
[ ] hotkey support（stop / save）
[ ] 狀態文字（frame count, stitched height）
[ ] error handling（capture 失敗、stitch 異常）
[ ] --headless CLI 保持可用
[ ] 雙螢幕 preview 支援（optional）
```

---

## 9. 不在 v0.5 範圍

```text
- 自建 layer-shell overlay（已決定用 Tauri）
- Windows 支援
- auto-scroll（snow-shot 有，rollshot 延後）
- Mosaic2D
- auto-scroll-through（snow-shot 有，rollshot 延後）
```

---

## 10. 風險與對策

| 風險 | 對策 |
|------|------|
| Tauri binary size 太大 | 接受；CLI headless mode 不依賴 Tauri（獨立 binary） |
| Tauri 啟動慢 | portal 可以在 Tauri 初始化時同步進行 |
| Frame 傳輸太慢 | Phase 1 用 PNG ArrayBuffer；不夠時加 asset protocol 或降 preview fps |
| HiDPI 座標錯誤 | 參考 snow-shot 的 scale factor 處理 |
| Linux 全螢幕沒 preview | 可接受；使用者看 terminal 輸出；雙螢幕時可放別的螢幕 |
| 使用者在 portal 選了 region | 不影響——Tauri overlay 顯示 portal 裁切後的畫面，使用者可以選全部 |
