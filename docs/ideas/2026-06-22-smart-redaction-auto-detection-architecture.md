# Smart Redaction Auto Detection Architecture

**Date:** 2026-06-22  
**Status:** Implementation planning note  
**Related:** `docs/ideas/2026-06-14-smart-redaction-presets.md`

## Summary

Smart Redaction 的自動偵測不應該一開始導入完整 OpenCV，也不應該讓 QuickJS / JavaScript 直接操作底層影像處理 API。

比較合理的方向是：

1. 保持目前已經成形的 `AutomationHost` capability boundary。
2. 新增 Rollshot-owned 的 `RealAutomationHost`。
3. 在 Rust 端建立 `VisualIndex`，集中處理 grayscale、edge map、connected components、OCR cache、template cache 等可重用資料。
4. JavaScript detector 只負責組合規則、閾值與輸出候選框。
5. OpenCV 只保留成 spike 或 optional backend，等 `image` / `imageproc` / 現有 matcher 能力明確不夠時再導入。

核心判斷：**Rollshot 需要的是一個可控、可審查、可測試的 UI-oriented vision adapter layer，不是通用 CV framework。**

## Design Decisions (2026-06-22 Architecture Review)

> 以下決策在 2026-06-22 的架構檢視中拍板，**優先級高於本文件其餘較早的提案**。被取代的較早段落已加指標連到這裡。本節所有 JS 範例都實際通過 `rollshot-automation` 的 `validate_source`（restricted-JS frontend 驗證器）。

### D1. `role` 僅限 author 階段；runtime detector 以 template 為主

`layout()` 的 `role`（`topBar`、`leftSidebar`…）是啟發式定義、stringly-typed、語意薄弱的契約，**不可凍進持久化的 preset JS**：

- 它把 durable 產物版本耦合到「當下那版 layout 啟發式吐什麼字串」；heuristic 一改，舊 preset 行為默默變動。
- 純幾何連書籤列 / 網址列都分不開，`role ===` 等於用一個猜測當篩選條件。

**決定：**

- `layout` / `role` 降級為 **author 階段的 inspection tool**（對應設計 §7.2 的 `inspect_layout`）—— LLM 在 chat session 看圖理解版面用。
- **持久化 / runtime detector** 只用：`templateMatch`（主力）、`ocr`（文字定義的目標）、`regionFeatures`（數值 sanity filter，如 `edgeDensity`、`dominantRgba`；這些是量測值、非語意標籤，可安全使用）、必要時固定 `region` 幾何。
- `rollshot-vision` 的 layout v0 因此**降優先級、降精度**（只是 author 輔助，不是 runtime 精準路徑）；`templateMatch` 與 `regionFeatures` 升為首要實作。

**取捨（明確記錄）：** runtime `role` 唯一不可取代的價值，是對「沒教過、第一次見到」的 UI 也能猜結構。但那只有「出貨內建 / marketplace preset」才需要，屬 deferred scope。first-release 定位是「teach once, reuse locally」，**不需要 runtime role**。未來若要零教學的內建 preset，改用訓練過的 object-detector capability（設計保留的擴充點），仍不是 runtime LLM。

#### Author vs Runtime Capability Policy（D1 的可執行版）

| Capability | Author session | Persisted runtime preset | Notes |
|---|---:|---:|---|
| `inspectLayout` / layout `role` | yes | **no** | author-only heuristic, not durable contract |
| `templateMatch` | yes | yes | primary runtime detection path |
| `regionFeatures` | yes | yes | numeric sanity filter only |
| `ocr` | yes | yes（optional backend） | text-defined targets |

**強制方式（實作注意）：** 目前 `ALLOWED_ROLLSHOT_CAPABILITIES`（`frontend/validate.rs`）對 author / runtime 不分，仍把 `layout` 暴露給 persisted JS。要硬性貫穿此政策，需把 capability allowlist 變成**情境相關**：runtime executor 的 allowlist **不含 `layout`**（author-time inspect 走另一條 host/工具路徑）。在那之前，「persisted preset 不得依賴 layout role」只是政策，需靠 review / lint 把關。

### D2. Restricted-JS 無法合併來源 → v1 runtime detector 單一來源；跨來源 fusion deferred

frontend 驗證器只允許 `map` / `filter` / `some` / `every`，**沒有 `concat`、spread、`push`、mutation、loop**（見 `crates/rollshot-automation/src/frontend/validate.rs`）。因此一次 `main` return 的 `candidates` 只能來自**單一** capability 呼叫鏈；較早「layout + template 合併」那種 pseudo-JS（`for...of` + `candidates.push`）**在已實作的 subset 裡表達不出來**。

**決定（v1）：**

- v1 persisted detector 一律**單一來源**。
- Rust host 端**只做單一 capability 結果內部的** NMS / 去重 / clipping / padding / scoring（特別是 `templateMatch`）。
- **跨來源融合（layout + template + ocr 的 union / IoU 合併）deferred。**

**考慮過但暫不採用：** 新增 Rust 端 composite capability（例如 `rollshot.detect({ kind, templateHandle, padding, sanity })`）由 host 做完整偵測 + 融合、JS 只收結果。等「單一來源 + capability 內部 NMS」證實不夠用時再評估，不在 v1。（後面「Candidate Merging」的 `iou`/`union`/`pad_and_clip` 在 v1 僅用於**單一 capability 結果內部**，不做跨來源。）

### D3. Author 階段的 template 取得 pipeline（teach 自動化）

「template 哪來」的決定：author 階段由多模態 LLM 看圖取得，**高信心自動採用、不打斷使用者**；但「信心」必須是 **Rust 量出來的 self-match 分數，不是 LLM 自稱的**。

理由：(1) 多模態 LLM 給精確像素座標不準、且 self-reported confidence 偏高；(2) 在 redaction 工具裡，「自動選了爛 template → 默默漏掉敏感資料」是最危險的失敗模式，踩到設計「絕不宣稱圖是安全的」的底線。

```text
author 階段（看得到圖，只跑一次）：

1. LLM 粗定位（語意）            「書籤列 ≈ 這塊」框歪一點沒關係
2. Rust 吸附到真實結構（geometry） 用 CV 原語把粗框 snap 到分隔線 / 連通元件 bbox
                                  ← layout/regionFeatures 在這裡發揮價值（author，不是 runtime）
3. Rust 自我驗證（信心來源，見下方訊號）
4. 依自驗 decision 決定 UX：
     Pass         → 自動採用，不問使用者
     NeedsConfirm → NeedsUserInput（設計 §7.4）請使用者確認或微調
     Reject       → 換 chrome / 換來源，或回到步驟 1
```

**自驗不能只看 self-score。** 任何裁切對原圖都「找得到自己」(tautology)，真正要量的是 peak margin、是否到處亂中、結構是否足夠、抖動後是否穩：

```rust
pub struct TemplateSelfValidation {
    pub self_score: f32,             // 命中自己的分數
    pub second_best_score: Option<f32>,
    pub peak_margin: f32,            // 第一名 vs 第二名差距（夠大才穩）
    pub false_positive_count: u32,   // 是否到處亂中
    pub edge_density: f32,           // template 本身結構是否足夠
    pub entropy: f32,                // 非純色塊
    pub stable_under_jitter: bool,   // 微小 padding/resize/亮度位移後仍穩
    pub decision: TemplateDecision,  // Pass / NeedsConfirm / Reject
}
```

額外 gate：**area bounds**（template 不可過大或過小）、**target coverage**（裁切要真的覆蓋「要隱藏的 UI 區」，不只是其中一個小 icon）。

**`N >= 2` 要看情況，不是硬性條件：**

- 預設：**高 self-score + peak margin 乾淨 + template 結構足夠** → 可自動採用。
- **只有當目標本應重複出現**（例如桌面多個資料夾圖示）才額外要求 `N >= 2`。
- 像 bookmark strip 整張圖可能只有一條，`N >= 2` 不成立屬正常 → 不可因此 reject。

- **不違反 review-first：** 拿掉的是「前置手動框選」，輸出的候選框仍照 §2.4 由使用者 review。自動化輸入 ≠ 跳過輸出審查。
- 這收掉本文件 Open Questions 的 #2（template 存哪）與 #3（template 怎麼選）。

### D4. Template 隱私模型

把隱私面分清楚——風險等級差很多：

| 面 | 發生什麼 | 規則 |
|---|---|---|
| runtime 比對 | 敏感 template 在本機跑 NCC，**永遠不上傳**（runtime 無 LLM） | 無需任何同意 |
| 本機保存 template 裁切 | 小圖片存進 preset（本機磁碟） | **自動做 + 主動告知，不詢問** |
| author 階段上傳 | 整張截圖送 provider 給 LLM 看 | 維持 §9.1 硬揭露（不可鬆） |
| 匯出 / 分享 preset（deferred） | 內嵌敏感裁切會外流 | 必須 gate 或 strip（不可鬆） |

- **chrome-first、品質驅動的自動選擇：** 先試不變的 chrome 模板（favicon / 邊緣 / OS glyph）並自驗；自驗夠好就用 chrome（非敏感，連告知都不必）。chrome 不夠、但含內容的裁切自驗夠好時，**自動採用敏感裁切並主動告知**；都不行才 `NeedsUserInput`。
- **告知 = 非阻斷、可反悔、誠實**：例如 review 介面一行「🔒 此 preset 在本機存了含你內容的小裁切以便辨識，不會上傳。〔檢視〕〔刪除〕」。不是彈窗確認。
- **資料模型（規格，非僅 UX 文案）：**

```rust
pub enum TemplateSensitivity {
    Chrome,
    Sensitive,
}

pub struct TemplateAsset {
    pub handle: TemplateHandle,
    pub sensitivity: TemplateSensitivity,
    pub source: TemplateSource,
    pub created_at: Timestamp,
    pub bounds_in_source_image: Option<ImageRect>,
    pub bytes: TemplateBytes,
}
```

  規則：
  - template asset 必須**可檢視、可刪除、可重新產生**。
  - `Sensitive` template **預設不得同步、不得匯出、不得進測試 fixture**。
  - 未來 preset export / share 時，`Sensitive` template 必須 **strip 或要求明確確認**。
  - **強制句：** Any code path that serializes presets outside local storage must inspect `TemplateSensitivity` before writing template bytes.
- **對 §9.5 的範圍限定調整（刻意）：** 現行 §9.5「fixture 只有使用者明確標非敏感才保存」維持不變；本放寬**只針對 template 裁切**（小、本機、runtime 不上傳），不動 fixture（大、回歸測試用、維持 opt-in）。

### 驗證過的 role-free 範例（取代較早用 `role` 的 pseudo-JS）

兩段皆已通過 `validate_source`。候選物件 `deny_unknown_fields`，只能有 `kind` / `bounds` / `confidence` / `label` / 選填 `rationale`。

```js
// Hide browser bookmarks —— 純 template（teach-once，runtime 零 LLM）
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.bookmarkStrip,
    region: { kind: "full" },
    limit: 40,
  });

  return {
    candidates: matches
      .filter((match) => match.score >= 0.82)
      .map((match) => ({
        kind: "addRedaction",
        bounds: match.bounds,
        confidence: Math.min(0.95, match.score),
        label: "bookmark-strip-template",
      })),
  };
}
```

```js
// Hide document folders —— 純 template + 純幾何 padding 蓋住名稱
function padToCaption(bounds) {
  return {
    x: Math.max(0, bounds.x - 8),
    y: Math.max(0, bounds.y - 8),
    width: bounds.width + 16,
    height: bounds.height + 36,
  };
}

function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.folderIcon,
    region: { kind: "full" },
    limit: 80,
  });

  return {
    candidates: matches
      .filter((match) => match.score >= 0.8)
      .map((match) => ({
        kind: "addRedaction",
        bounds: padToCaption(match.bounds),
        confidence: Math.min(0.94, match.score),
        label: "desktop-folder-icon",
      })),
  };
}
```

## Current Context

原始 Smart Redaction Presets idea 已經做了幾個關鍵產品與技術決策：

- 使用者明確執行 named preset。
- 第一次可用 LLM 產生輕量 JavaScript detector。
- 後續在本機 QuickJS runtime 執行 detector。
- detector run 只產生 editable redaction candidates。
- redaction candidates 永遠需要使用者 review。
- Rust 擁有 expensive / privileged / security-sensitive operations。
- JavaScript 只負責 combining detector results、conditions、confidence thresholds、candidate rectangles。

目前程式碼也已經有接近完整的 automation skeleton：

- `crates/rollshot-automation/src/capability.rs`
  - `CapabilityName::{Ocr, Layout, RegionFeatures, TemplateMatch}`
  - `OcrQuery`, `LayoutQuery`, `RegionFeaturesQuery`, `TemplateMatchQuery`
  - `OcrMatch`, `LayoutRegion`, `RegionFeatures`, `TemplateMatch`
- `crates/rollshot-automation/src/host.rs`
  - `AutomationHost` trait
  - `FakeAutomationHost`
- `crates/rollshot-automation-rquickjs/src/bridge.rs`
  - exposes frozen `rollshot.ocr`, `rollshot.layout`, `rollshot.regionFeatures`, `rollshot.templateMatch`
  - validates query limits against capability manifest
  - charges capability calls and host allocation
  - freezes returned values before giving them to QuickJS
- `crates/rollshot-automation/src/executor.rs`
  - `execute_to_proposal(...)`
  - compatibility checks
  - cancellation and execution metrics
- `crates/rollshot-automation/src/output.rs`
  - output validation
  - finite bounds checks
  - confidence / label / rationale limits
  - conversion to `EditProposal`

Therefore the missing piece is not the script sandbox. The missing piece is the real Rust-side detection host.

## Recommendation

Do **not** expose OpenCV to JavaScript.

Do **not** rewrite a general-purpose computer vision stack from scratch.

Do build a thin, Rollshot-specific vision layer:

```text
Captured screenshot / ImageDocument
        ↓
VisualIndex
        ↓
RealAutomationHost
        ↓
AutomationHost capabilities
        ↓
QuickJS detector script
        ↓
EditProposal candidates
        ↓
Human review
        ↓
ImageDocument redactions / safe export
```

The immediate implementation should use:

- existing `image = 0.25` workspace dependency
- new optional `imageproc` dependency for common primitives
- existing Rollshot matcher knowledge where useful, especially NCC / edge projection concepts
- OCR provider trait, with real OCR backends added behind feature flags
- OpenCV only as spike or optional backend

## Why Not OpenCV First?

OpenCV is powerful, but it is not the right default dependency for first-release Smart Redaction.

### Reasons

1. **Packaging and native dependency cost**

   OpenCV introduces a large native dependency surface. This matters for a cross-platform desktop screenshot tool that already cares about Linux/macOS packaging and low-friction installation.

2. **Rust binding maturity risk**

   The Rust `opencv` crate describes its API as usable but unstable and not very battle-tested. That is acceptable for spikes, but it is a poor default for a security-sensitive redaction feature.

3. **Wrong abstraction level**

   Smart Redaction does not need arbitrary image processing from scripts. It needs a small set of bounded, validated, semantic-ish capabilities:

   - OCR matches
   - layout regions
   - region features
   - template matches

4. **Security boundary clarity**

   The current design treats QuickJS as a constrained rule-composition runtime. Exposing raw CV operations would expand the sandbox surface and make resource control harder.

5. **Product behavior is review-first**

   The feature does not need perfect unattended detection. It needs useful candidates that are easy to inspect and edit.

## Why Not Write Everything From Scratch?

Do not write a generic CV framework. Write only the minimum Rollshot-specific adapters.

Good candidates for small self-owned code:

- rectangle clipping and padding
- simple histogram / dominant color
- edge density aggregation
- region scoring
- UI strip detection
- grid grouping
- candidate merging and de-duplication
- confidence calibration

Good candidates for library-backed code:

- grayscale conversion
- thresholding
- gradients / edges
- connected components
- contours
- template matching
- morphology

`imageproc` is a good first library to evaluate because it is built around the Rust `image` crate and already contains primitives such as template matching, connected components, contours, and edge/corner-related operations.

## Proposed Crate Split

Add a new crate:

```text
crates/rollshot-vision/
```

Rationale: this should not live inside `rollshot-automation-rquickjs`, because QuickJS is only one consumer. The same visual primitives may later support OCR search, action guides, keyframe labeling, or post-capture assistant features.

Initial structure:

```text
crates/rollshot-vision/
  Cargo.toml
  src/
    lib.rs
    host.rs              # RealAutomationHost
    index.rs             # VisualIndex and precompute cache
    image_source.rs      # ImageDocument / RgbaImage adapters
    rect.rs              # clipping, padding, IoU, merge helpers
    ocr.rs               # OcrProvider trait and fake provider
    layout.rs            # layout detection v0
    region_features.rs   # dominant color, edge density, component count
    template.rs          # template store and match adapter
    confidence.rs        # scoring and calibration helpers
```

Workspace dependency additions:

```toml
[workspace.dependencies]
imageproc = "0.27"
```

Then:

```toml
# crates/rollshot-vision/Cargo.toml
[dependencies]
image = { workspace = true }
imageproc = { workspace = true }
rollshot-automation = { path = "../rollshot-automation" }
rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }
serde = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

OCR backends should be feature-gated:

```toml
[features]
default = []
ocr-tesseract = []
ocr-macos-vision = []
opencv-backend = []
```

Do not enable OCR or OpenCV by default until packaging is proven.

## Core Types

### `RealAutomationHost`

```rust
pub struct RealAutomationHost<P> {
    index: VisualIndex,
    ocr_provider: P,
    templates: TemplateStore,
}

impl<P: OcrProvider> AutomationHost for RealAutomationHost<P> {
    fn ocr(&mut self, query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
        self.index.ocr(&mut self.ocr_provider, query)
    }

    fn layout(&mut self, query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError> {
        self.index.layout(query)
    }

    fn region_features(
        &mut self,
        query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        self.index.region_features(query)
    }

    fn template_match(
        &mut self,
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        self.index.template_match(&self.templates, query)
    }
}
```

### `VisualIndex`

`VisualIndex` should be built once per automation run and reused across capability calls.

```rust
pub struct VisualIndex {
    image: image::RgbaImage,
    width: u32,
    height: u32,
    gray: Option<image::GrayImage>,
    edge_map: Option<image::GrayImage>,
    components: Option<Vec<ComponentRegion>>,
    ocr_cache: OcrCache,
    layout_cache: LayoutCache,
}
```

Build options should be derived from `CapabilityManifest`:

```rust
pub struct VisualIndexBuildOptions {
    pub need_ocr: bool,
    pub need_edges: bool,
    pub need_components: bool,
    pub need_template_matching: bool,
}
```

Example:

```rust
let options = VisualIndexBuildOptions::from_manifest(
    &automation.workflow_ir.capability_manifest,
);
let index = VisualIndex::build(image, options)?;
let mut host = RealAutomationHost::new(index, ocr_provider, template_store);

let (proposal, metrics) = execute_to_proposal(
    &executor,
    &automation,
    &input,
    &proposal_context,
    &mut host,
    &policy,
    &cancellation,
)?;
```

Important rule: do not run OCR, edge extraction, connected components, or template preparation unless the manifest requires them.

## Detection Primitives

The first useful version should implement four bounded primitives. These map exactly to the existing `AutomationHost` API.

### 1. OCR Selector

Target use cases:

- hide email addresses
- hide API tokens
- hide names near labels such as `Owner:`, `Created by:`, `Assignee:`
- hide document folder captions
- hide bookmark text

Design:

```rust
pub trait OcrProvider {
    fn recognize(
        &mut self,
        image: &image::RgbaImage,
        region: ImageRect,
        limit: u32,
    ) -> Result<Vec<OcrMatch>, OcrError>;
}
```

Start with:

- `FakeOcrProvider` for fixtures and deterministic tests
- `NoopOcrProvider` for builds without OCR support
- optional `TesseractOcrProvider` later
- optional macOS Vision provider later

Do not block the first layout/template demo on real OCR.

### 2. Layout Selector

> **取代於 Design Decision D1：** `layout` / `role` 降級為 author 階段的 `inspect_layout`，**不是** runtime capability。以下內容描述 author 輔助用的 layout v0（降優先級、降精度）；持久化 detector 不依賴 `role`。

`layout()` should not immediately claim app-specific semantics like `chromeBookmarkBar` or `finderDesktopFolder`.

It should return reusable, intermediate UI regions:

```text
topBar
leftSidebar
horizontalToolbar
textLine
iconGridItem
desktopIconCandidate
listRow
card
buttonLike
```

First implementation can be heuristic:

- detect top horizontal strips with high text/icon density
- detect left sidebars by vertical grouping and uniform background
- group OCR boxes into text lines when OCR is available
- group connected components into repeated rows or grids
- identify desktop-icon-like candidates by icon-above-caption geometry
- ignore tiny noise and very large full-screen background regions

This is enough for early presets because they only need candidate proposals, not final truth.

### 3. Region Features

Current API:

```rust
pub struct RegionFeatures {
    pub bounds: ImageRect,
    pub dominant_rgba: [u8; 4],
    pub edge_density: f32,
}
```

First implementation:

- split the requested region into candidate subregions, or return one feature object for the requested rectangle
- compute dominant color via small histogram / quantized RGB bins
- compute edge density via precomputed edge map or local gradient threshold
- keep values deterministic and cheap

Future fields worth considering after v0:

```rust
pub struct RegionFeaturesV2 {
    pub bounds: ImageRect,
    pub dominant_rgba: [u8; 4],
    pub edge_density: f32,
    pub text_density: f32,
    pub component_count: u32,
    pub brightness: f32,
    pub saturation: f32,
    pub aspect_ratio: f32,
}
```

Do not change the capability API until real scripts demonstrate that these fields are needed.

### 4. Template Match

Template matching is likely the highest-leverage primitive for Smart Redaction.

It supports the product thesis: teach Rollshot once, then reuse locally.

Target use cases:

- recurring folder icon
- recurring bookmark favicon area
- app logo
- avatar
- sidebar item shape
- toolbar button group
- repeated sensitive widget

Implementation direction:

- store templates as local handles
- keep template metadata separate from image bytes
- clip search region before matching
- return top matches with score and anchor
- de-duplicate overlapping matches
- enforce query limit and output limit through the existing bridge

Example returned match:

```json
{
  "bounds": { "x": 24, "y": 96, "width": 32, "height": 32 },
  "score": 0.91,
  "anchor": { "x": 40, "y": 112 }
}
```

## Deprecated Earlier Sketches

> **Historical only — do not implement or copy these scripts.** 這些早期 pseudo-JS 用了 `role` 篩選與多來源合併，**無法通過已實作的 restricted-JS subset**（`for...of` / `push` / union / runtime `role` 皆不允許）。可用且已驗證的 role-free 範例見上方「Design Decisions → 驗證過的 role-free 範例」。

### Hide Browser Bookmarks

Use a combination of layout and template matching.

Detector logic:

1. Ask `layout()` for top horizontal bars.
2. Ask `regionFeatures()` on the top 200 px of the screenshot.
3. If available, run `templateMatch()` for a previously taught bookmark-bar or favicon-strip template.
4. Merge candidates.
5. Emit redactions only when confidence passes threshold.

Pseudo-JS:

```js
function main(input) {
  const top = { kind: "rect", bounds: { x: 0, y: 0, width: input.imageWidth, height: 200 } };

  const layout = rollshot.layout({ region: top, limit: 40 });
  const features = rollshot.regionFeatures({ region: top, limit: 20 });

  const candidates = [];

  for (const r of layout) {
    if ((r.role === "topBar" || r.role === "horizontalToolbar") && r.confidence >= 0.65) {
      candidates.push({
        kind: "addRedaction",
        bounds: r.bounds,
        confidence: Math.min(0.8, r.confidence),
        label: "browser-bookmark-bar",
        rationale: "Top horizontal UI strip likely contains bookmark labels or favicons."
      });
    }
  }

  return { candidates };
}
```

Optional template-boosted version:

```js
function main(input) {
  const top = { kind: "rect", bounds: { x: 0, y: 0, width: input.imageWidth, height: 220 } };
  const layout = rollshot.layout({ region: top, limit: 40 });
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.bookmarkStrip,
    region: top,
    limit: 10
  });

  const candidates = [];

  for (const m of matches) {
    if (m.score >= 0.82) {
      candidates.push({
        kind: "addRedaction",
        bounds: m.bounds,
        confidence: Math.min(0.95, m.score),
        label: "bookmark-strip-template"
      });
    }
  }

  for (const r of layout) {
    if (r.role === "topBar" && r.confidence >= 0.70) {
      candidates.push({
        kind: "addRedaction",
        bounds: r.bounds,
        confidence: 0.70,
        label: "bookmark-strip-layout-fallback"
      });
    }
  }

  return { candidates };
}
```

### Hide Document Folders On Desktop

Use icon-grid layout, OCR captions, and optional folder-icon template.

Detector logic:

1. Ask `layout()` for `desktopIconCandidate` / `iconGridItem`.
2. Run `templateMatch()` for taught folder icon handle when available.
3. Optionally run `ocr()` near candidate captions.
4. Redact the icon + caption as one padded rectangle.

Pseudo-JS:

```js
function main(input) {
  const full = { kind: "full" };
  const layout = rollshot.layout({ region: full, limit: 120 });
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.folderIcon,
    region: full,
    limit: 80
  });

  const candidates = [];

  for (const item of layout) {
    if (item.role === "desktopIconCandidate" && item.confidence >= 0.62) {
      candidates.push({
        kind: "addRedaction",
        bounds: item.bounds,
        confidence: item.confidence,
        label: "desktop-folder-candidate"
      });
    }
  }

  for (const m of matches) {
    if (m.score >= 0.80) {
      candidates.push({
        kind: "addRedaction",
        bounds: {
          x: Math.max(0, m.bounds.x - 8),
          y: Math.max(0, m.bounds.y - 8),
          width: m.bounds.width + 16,
          height: m.bounds.height + 36
        },
        confidence: Math.min(0.94, m.score),
        label: "folder-icon-template"
      });
    }
  }

  return { candidates };
}
```

## Confidence Model

Initial confidence should be intentionally conservative. Do not present confidence as a safety guarantee.

**v1 confidence is source-local:**

- `templateMatch`: score after NMS / self-validation-derived threshold
- `ocr`: backend confidence if available, otherwise a conservative heuristic
- `regionFeatures`: used as a sanity filter, **not** a standalone safety confidence

Cross-source confidence combination is **deferred** (see D2).

Suggested bands (UI behavior, source-agnostic):

| Confidence | Meaning | UI behavior |
|---:|---|---|
| `>= 0.85` | Strong candidate | preselected in review |
| `0.65 - 0.85` | Plausible candidate | shown, but visually marked as medium confidence |
| `0.45 - 0.65` | Weak candidate | optional / collapsed / only if user asks for more |
| `< 0.45` | Too weak | do not emit |

**Deferred (cross-source fusion — not v1)** — combining signals across detectors, e.g.

```text
base layout score
+ template match boost
+ OCR/context boost
+ geometry consistency boost
- overlap/noise penalty
- out-of-expected-region penalty
```

```rust
fn combine_confidence(signals: &[f32]) -> f32 {
    // Avoid pretending this is calibrated probability.
    let max = signals.iter().copied().fold(0.0, f32::max);
    let agreement = signals.iter().filter(|s| **s >= 0.65).count() as f32;
    (max + agreement * 0.04).min(0.96)
}
```

## Candidate Merging / NMS

In v1, candidate merging only happens **within a single capability result**, primarily `templateMatch`.

Cross-source merging between layout / template / OCR is **deferred** (see D2).

Initial host-side helpers:

```rust
pub fn iou(a: ImageRect, b: ImageRect) -> f32;
pub fn intersects(a: ImageRect, b: ImageRect) -> bool;
pub fn union(a: ImageRect, b: ImageRect) -> ImageRect;
pub fn pad_and_clip(rect: ImageRect, padding: f32, bounds: ImageRect) -> ImageRect;
```

v1 (single-source NMS) policy:

- if IoU is high within one result, keep the higher-confidence match and union bounds
- never emit out-of-bounds or zero-area rectangles

Deferred (cross-source — needs D2 fusion):

- if a template match is inside a layout region, prefer the layout region when the goal is to hide the entire semantic strip
- if a layout region is too large, prefer grouped template matches

The existing output decoder already rejects invalid finite ranges and non-positive output bounds, but earlier clipping improves UX and diagnostics.

## Resource Control

Keep resource limits at three levels:

1. **Validation-time manifest**
   - which capabilities are allowed
   - max calls per capability
   - max results per call
   - max aggregate results

2. **Runtime bridge**
   - wall-clock limit
   - memory limit
   - stack limit
   - capability call count
   - host allocation accounting

3. **Vision host**
   - max OCR region area
   - max template search area
   - max component count
   - max returned regions
   - max precompute bytes

Add host-side rejection codes such as:

```rust
CapabilityError::InvalidInput { code: "region_too_large" }
CapabilityError::InvalidInput { code: "template_not_found" }
CapabilityError::LimitExceeded
CapabilityError::Failed { code: "ocr_backend_unavailable" }
CapabilityError::Failed { code: "vision_index_unavailable" }
```

## Implementation Plan

> **排序與 sub-project 邊界以「Roadmap — Sub-projects & PRs」為準。** 以下 P0–P6 為設計細節；其中 P4（inspectLayout）已改劃到 **SP4**，P5（OCR）改劃到 **SP5**，regionFeatures 改劃到 **SP2**。Sub-project 1 只取 template 路徑。

### P0 — Keep Current Automation Boundary

Goal: confirm no design drift.

Tasks:

- keep `AutomationHost` capability boundary
- **persisted runtime presets must not depend on layout roles**（見 Author vs Runtime Capability Policy）
- author-time tooling may use `inspectLayout` / `layout` internally
- do not expose arbitrary CV calls, filesystem, network, async, timers, modules, or DOM
- keep output going through `EditProposal`

Exit criteria:

- existing automation tests still pass
- no new direct image-processing API appears in QuickJS
- no persisted preset filters on a `layout` `role`

### P1 — Add `rollshot-vision` Skeleton

Goal: create the place where real detection lives.

Tasks:

- add `crates/rollshot-vision`
- add `VisualIndex`
- add `RealAutomationHost`
- add `NoopOcrProvider` and `FakeOcrProvider`
- wire unit tests with `FakeAutomationHost`-like deterministic behavior

Exit criteria:

- `RealAutomationHost` compiles and implements `AutomationHost`
- capability calls return empty but valid results
- no OCR/OpenCV native dependency required

### P2 — Implement Region Features v0

Goal: get the cheapest useful visual signal working.

Tasks:

- crop region safely
- compute dominant RGBA using quantized histogram
- compute edge density using simple luma gradient threshold
- optionally cache grayscale / edge map in `VisualIndex`

Exit criteria:

- deterministic tests on synthetic images
- edge density distinguishes blank area vs text/toolbar-like area
- dominant color stable enough for UI strips

### P3 — Implement Template Matching v0

Goal: support teach-once reuse.

Tasks:

- define `TemplateStore`
- load template by `template_handle`
- clip search region
- use `imageproc::template_matching` or Rollshot-owned NCC utility
- sort by score
- non-max suppress overlapping matches

Exit criteria:

- synthetic fixture finds pasted template
- overlapping matches de-duplicate
- query `limit` respected
- missing template returns typed capability error

### P4 — Implement Author-Time Inspect Layout v0

Goal: 給 author 階段的 agent 一個看圖理解版面的 **inspection** 工具（**非** runtime capability，見 D1 / Policy）。降優先級：排在 template 路徑（store / match / self-validation）之後。

Tasks:

- detect top horizontal strips
- detect left sidebar-like strips
- detect repeated icon/grid components
- return roles like `topBar`, `horizontalToolbar`, `desktopIconCandidate`, `iconGridItem`（**僅供 author session 使用**）
- add conservative confidence values

Exit criteria:

- synthetic toolbar fixture produces top-bar candidate
- synthetic desktop-icon grid produces icon candidates
- blank image produces no confident candidates
- **此能力不出現在 runtime executor 的 capability allowlist**

### P5 — Add OCR Provider Trait Integration

Goal: allow OCR-backed detectors without making OCR mandatory.

Tasks:

- implement cache key by region + image generation/version
- fake OCR provider for tests
- optional Tesseract provider spike
- optional macOS Vision provider research/spike
- map backend errors into `CapabilityError`

Exit criteria:

- OCR fixtures can drive `layout()` grouping
- builds without OCR still work
- OCR backend unavailable state is explicit

### P6 — Golden Smart Redaction Fixtures

Goal: verify end-to-end behavior.

Fixtures:

- browser-like top toolbar / bookmark bar
- desktop-like icon grid with folder candidates
- negative case with no bookmarks/folders
- shifted/scaled layout variants

Tests:

- run generated JS through QuickJS executor
- use `RealAutomationHost`
- compare emitted proposal rectangles against expected bounds with tolerance
- verify low-confidence or no-match semantics

Exit criteria:

- `hide browser bookmarks` demo works locally
- `hide document folders` demo works locally
- every candidate remains reviewable/editable

## Roadmap — Sub-projects & PRs

整個 auto-detection 拆成數個**獨立 sub-project**。每個 sub-project = **1 份 spec + 1 份 plan**，plan 內含**可獨立測試 / commit 的 PR phases**，**每個 PR 收尾留一則 handoff note**（不為每個 PR 各寫一份 spec —— 六個 PR 共享同一套互相依賴的型別設計，拆成六份 spec 多半是互相 cross-ref 的空殼；亦吻合 repo 既有 subproject-level spec + `docs/superpowers/handoffs/` 慣例）。

### Sub-project 1 — `rollshot-vision` runtime host（template-first）

agent-independent、現在就能做：用 fixtures + 手寫 template detector 經 `QuickJsExecutor` 驗證。**範圍只含 template 路徑**；`regionFeatures` / author-time `inspectLayout` / OCR / OpenCV 全部 deferred 到後續 sub-project。

| PR | 內容 | 獨立測試 done-state |
|---|---|---|
| PR1 | `crates/rollshot-vision` skeleton（crate + workspace 接線 + stub `RealAutomationHost` 實作 `AutomationHost`，回空但合法） | 編譯；capability 回空合法值；無 OCR/OpenCV native dep |
| PR2 | `VisualIndex` + `ImageRect` clipping/padding helpers（grayscale 預算、clip、pad、IoU、union） | 合成圖上的 deterministic 單元測試 |
| PR3 | `TemplateAsset` / `TemplateStore` / `TemplateSensitivity`（本機儲存 + 隱私旗標 + serialize gate） | by-handle 載入；missing handle → typed error；`Sensitive` 不進匯出路徑 |
| PR4 | `templateMatch` v0 + NMS（NCC via `imageproc` 或自有；clip region、score 排序、重疊抑制、limit 生效） | 合成 fixture 找到貼入的 template；重疊去重；limit 生效 |
| PR5 | `TemplateSelfValidation`（self / peak-margin / false-positive / edge / entropy / jitter → Pass/NeedsConfirm/Reject 的純函式） | 好 template Pass；純色塊 Reject；到處亂中 Reject |
| PR6 | role-free QuickJS fixture tests（用本 sub-project 的 `RealAutomationHost` 跑兩個已驗證 detector，比對候選框） | `hide bookmarks` / `hide folders` demo 本機過；候選可編輯 |

### 後續 sub-projects（各自 spec / plan / handoff）

- **SP2** `regionFeatures` v0（dominant color + edge density，數值 sanity filter）
- **SP3** author-time template acquisition pipeline（LLM 粗定位 → snap → 接 self-validation → auto/NeedsUserInput）—— **依賴 bounded agent core**
- **SP4** author-time `inspectLayout` v0（降精度版面啟發式，僅 author session）
- **SP5** OCR provider 整合（`NoopOcrProvider`/`FakeOcrProvider` → optional Tesseract / macOS Vision）
- **SP6** product 接線（把 `RealAutomationHost` 接進 Result Workspace 執行路徑）
- **(opt)** OpenCV spike（僅在 NCC 不夠時）

Keep each PR independently testable.

## When To Add OpenCV

Add OpenCV only if at least one of these becomes true:

- layout v0 cannot handle a validated preset use case after reasonable `imageproc` heuristics
- template matching needs scale/rotation robustness that simple NCC cannot provide
- ORB/feature matching becomes necessary for recurring UI elements under theme/scale changes
- optical flow or homography becomes useful for a non-redaction feature and can be shared
- an optional backend can be packaged reliably on target platforms

Even then, OpenCV should remain behind a feature flag:

```toml
[features]
opencv-backend = ["dep:opencv"]
```

And the public boundary should stay the same:

```text
QuickJS detector → AutomationHost capability → backend implementation
```

Do not let presets depend on `opencv` directly.

## Non-Goals

Do not implement these in the first release:

- unattended safe export
- generic object detection
- YOLO / model download / model training
- script-level OpenCV API
- browser-specific hardcoded detectors for every browser
- OS-specific desktop detectors for every desktop environment
- automatic preset execution on every capture
- claiming that all sensitive information was found

## Open Questions

> 先前的 #1–#4 已由 D1 / D3 收掉，移除以免矛盾。

1. Should `RegionFeatures` v1 stay tiny, or should we version a v2 shape before scripts proliferate?
2. Should template handles be per-preset only in v1, with global reuse deferred?

## Practical Default Answer

排序見「Roadmap — Sub-projects & PRs」。Sub-project 1（`rollshot-vision` runtime host，PR1–PR6）是第一個要做、且 agent-independent 的可 demo 切片。

這保留了原始產品論點：**teach once, reuse locally, inspect every candidate.**

## References

- Smart Redaction Presets idea: `docs/ideas/2026-06-14-smart-redaction-presets.md`
- Current automation host: `crates/rollshot-automation/src/host.rs`
- Current capability schema: `crates/rollshot-automation/src/capability.rs`
- Current QuickJS bridge: `crates/rollshot-automation-rquickjs/src/bridge.rs`
- `imageproc` crate docs: <https://docs.rs/imageproc>
- `imageproc::template_matching`: <https://docs.rs/imageproc/latest/imageproc/template_matching/index.html>
- `opencv` crate: <https://crates.io/crates/opencv>
- `leptess` crate: <https://crates.io/crates/leptess>
