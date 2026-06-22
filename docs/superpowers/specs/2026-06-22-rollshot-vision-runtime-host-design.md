# rollshot-vision Runtime Host (Sub-project 1) — Design

**Date:** 2026-06-22
**Status:** Approved design
**Parent design:** `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`
**Idea note (decisions D1–D4, roadmap):** `docs/ideas/2026-06-22-smart-redaction-auto-detection-architecture.md`

## 1. Summary & Scope

Smart Redaction 的 automation skeleton(`rollshot-automation` + `rollshot-automation-rquickjs`)已經完成:restricted-JS frontend、Workflow IR、QuickJS executor、`AutomationHost` capability boundary、`EditProposal` 輸出驗證都在。**缺的是真正的 Rust-side 偵測 host** —— 目前只有 `FakeAutomationHost`(回預先塞的假資料)。

本 sub-project(SP1)建立 `crates/rollshot-vision`:一個 **agent-independent、deterministic、template-first 的 runtime 偵測 host**,實作 `AutomationHost`,讓手寫的 template-first detector(見 §A 的兩個已驗證範例)經 `QuickJsExecutor` 跑出真實的 redaction 候選框。

### In scope (SP1)

- `crates/rollshot-vision` crate skeleton。
- `VisualIndex`(每次 run 建一次,持有影像 + grayscale 預算)。
- `TemplateAsset` / `TemplateStore` / `TemplateSensitivity`(沿用 idea-doc D4 資料模型 + 本機儲存 + serialize gate + store 查詢 API)。
- `templateMatch` v0 + NMS(NCC via `imageproc`)。
- `TemplateSelfValidation`(純函式,author-time 用途,SP1 獨立建+測)。
- `RealAutomationHost`(實作完整 `AutomationHost` trait)。
- role-free QuickJS fixture 整合測試(PR6)。

### Explicitly NOT in scope (deferred)

- `regionFeatures` 實作 → **SP2**。
- author-time template acquisition pipeline(LLM 粗定位 → snap → 接 self-validation → auto/NeedsUserInput)→ **SP3**(依賴 bounded agent core)。
- author-time `inspectLayout` v0 → **SP4**。
- OCR provider 實作(Tesseract / macOS Vision)→ **SP5**。
- product 接線(`ImageDocument` → host,Result Workspace)→ **SP6**。
- OpenCV、object detector、scale/rotation-invariant matching → optional / later。

本 sub-project 的成功定義:**「teach 一個 template handle + 門檻 → 在本機跑出可 review 的候選框」這條路全程零 LLM、deterministic、可測**(idea-doc 成功條件 #10 的基礎)。

## 2. Context & Existing Boundaries

已存在、SP1 依賴而**不修改契約**的東西:

- `rollshot-automation/src/host.rs` — `AutomationHost` trait(`ocr` / `layout` / `region_features` / `template_match` → `Result<Vec<_>, CapabilityError>`);`FakeAutomationHost`。
- `rollshot-automation/src/capability.rs` — `Region`(`Full` | `Rect{bounds}`)、`TemplateMatchQuery{ template_handle, region, limit }`、`TemplateMatch{ bounds: ImageRect, score: f32, anchor: ImagePoint }`、`CapabilityError`。
- `rollshot-automation/src/executor.rs` — `execute_to_proposal(executor, automation, input, ctx, host, policy, cancellation)`。
- `rollshot-automation/src/policy.rs` — `ExecutionPolicy::smart_redaction_default(...)`、`ValidationLimits`。
- `rollshot-automation/src/frontend` — `validate_source(src, limits) -> ValidatedAutomation`。
- `rollshot-automation-rquickjs` — `QuickJsExecutor`、bridge(限額強制、freeze、host 呼叫派發)。

座標型別:`ImageRect` / `ImagePoint` 來自 `rollshot-image-document`(f32 像素座標)。

## 3. Architecture

### 3.1 Crate & dependency direction

新 crate `crates/rollshot-vision`,`unsafe_code = "forbid"`(純影像處理,無 FFI,不是 isolation crate)。

依賴:

```toml
[dependencies]
image = { workspace = true }                 # 0.25
imageproc = { workspace = true }             # 對齊 rollshot-core 既有 0.26（不採舊稿 0.27，避免雙版本）
rollshot-automation = { path = "../rollshot-automation" }
rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }
serde = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
rollshot-automation-rquickjs = { path = "../rollshot-automation-rquickjs" }  # 僅 PR6 整合測試
```

**依賴方向:** rollshot-vision 只依賴 `AutomationHost` trait + capability 型別,**不**依賴 `rollshot-automation-rquickjs`(executor/bridge);host 由呼叫端注入 `&mut dyn AutomationHost`。PR6 以 dev-dependency 引 rquickjs 跑真 JS。無循環依賴(rquickjs 不依賴 vision)。

**workspace 變更:** 新增 `imageproc = { version = "0.26", default-features = false }` 到 `[workspace.dependencies]`(**明確帶 `default-features = false`** 對齊 `rollshot-core` 現況,否則 workspace 版本雖同、features 可能不一致);`rollshot-core` 與 `rollshot-vision` 都改用 `imageproc = { workspace = true }`,確保單一版本 + 一致 features。

### 3.2 Module layout (SP1)

```
crates/rollshot-vision/src/
  lib.rs
  error.rs           # VisionError
  host.rs            # RealAutomationHost
  index.rs           # VisualIndex（SP1 只需 grayscale）
  rect.rs            # to_pixel_rect / clip / pad / iou / union
  template.rs        # TemplateAsset / TemplateStore / TemplateSensitivity / match_template_image / NMS
  self_validation.rs # TemplateSelfValidation
```

**不含** `ocr.rs` / `layout.rs` / `region_features.rs` / `confidence.rs`(SP2/SP4/SP5)。`image_source.rs`(ImageDocument adapter)延後到 SP6。

## Implementation Guardrails

開 PR1 前必須成立(逐項對應 §4–§5,2026-06-22 review 補):

- `match_template_image` 回 `Result<Vec<TemplateMatch>, CapabilityError>`,不是裸 `Vec`。
- `self_validate` 吃 `candidate_bounds: ImageRect`(內部自 `index.image()` 裁),不是只吃像素;回 `Result<_, VisionError>`。
- `TemplateAsset` / `TemplateStore` **不得**有會寫出 bytes 的 generic serialize path;本機/匯出走各自 record 型別,匯出對 `Sensitive` strip。
- `TemplateBytes` 是 raw RGBA,經 checked constructor(`w>0,h>0,len==w*h*4,w*h<=MAX_TEMPLATE_AREA`)。
- `to_pixel_rect` 用 floor-min / ceil-max rounding,拒絕 non-finite(`non_finite_region`)與 empty(`empty_region`)、超限(`region_too_large`);回 `Result`。
- NCC 分數必須有限;低資訊量 template 以 `template_low_information` 拒絕;非有限分數視為非命中。

## 4. Component Design

### 4.1 `RealAutomationHost`

```rust
pub struct RealAutomationHost {
    index: VisualIndex,
    templates: TemplateStore,
}

impl AutomationHost for RealAutomationHost {
    fn template_match(&mut self, q: TemplateMatchQuery)
        -> Result<Vec<TemplateMatch>, CapabilityError>
    { self.index.template_match(&self.templates, q) }

    // SP1：未實作 capability 回明確錯誤，不回空 Ok(vec![])
    fn ocr(&mut self, _q: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }
    fn layout(&mut self, _q: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }
    fn region_features(&mut self, _q: RegionFeaturesQuery) -> Result<Vec<RegionFeatures>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })  // SP2 填入
    }
}
```

**安全決定:** 未實作的 capability 回**明確 `capability_unavailable`**,不回空 `Ok(vec![])` —— redaction 工具裡靜默回空會讓 detector 誤判「沒有結果」而漏掉敏感區。validator/manifest 也會擋掉 SP1 不該呼叫的 capability,雙保險。

> 註:trait 方法簽名以 `rollshot-automation` 現況為準;`RealAutomationHost` 不再泛型化 OCR provider(SP1 無 OCR)。OCR provider 注入在 SP5 引入。

### 4.2 `VisualIndex`

每個 automation run 建一次,run 內所有 capability 呼叫共用(避免重算 grayscale)。

```rust
pub struct VisualIndex {
    image: image::RgbaImage,   // 來源真相（SP2 regionFeatures 會用 RGBA）
    width: u32,
    height: u32,
    gray: image::GrayImage,    // SP1 唯一預算：NCC 需要；eager 計算
}

impl VisualIndex {
    pub fn build(image: image::RgbaImage) -> Result<Self, VisionError>; // 拒絕 0 面積影像
    pub fn width(&self) -> u32;
    pub fn height(&self) -> u32;
    pub fn image(&self) -> &image::RgbaImage;
    pub(crate) fn gray(&self) -> &image::GrayImage;
}
```

**YAGNI 決定:** 舊稿的 `VisualIndexBuildOptions::from_manifest(...)` 在 SP1 **不做**(唯一 capability 是 templateMatch,grayscale 一律要,gate 永遠 true)。manifest 驅動 / lazy precompute 機制等 SP2 引入 edge map / connected components(昂貴且條件性)時再加。

### 4.3 Template store (沿用 idea-doc D4 + store API)

資料模型沿用 D4(不重述細節):

```rust
pub type TemplateHandle = String;             // 對齊 capability 的 template_handle

pub enum TemplateSensitivity { Chrome, Sensitive }

pub struct TemplateAsset {
    pub handle: TemplateHandle,
    pub sensitivity: TemplateSensitivity,
    pub source: TemplateSource,
    pub created_at_ms: u64,                    // 由呼叫端傳入（無 ambient clock）
    pub bounds_in_source_image: Option<ImageRect>,
    pub bytes: TemplateBytes,                  // RGBA 裁切（可供「檢視」）
}

pub struct TemplateStore { /* handle -> TemplateAsset */ }
impl TemplateStore {
    pub fn get(&self, handle: &str) -> Option<&TemplateAsset>;
    pub fn insert(&mut self, asset: TemplateAsset);
    pub fn save_local(&self, dst: &Path) -> Result<(), VisionError>;  // 本機存全部（chrome+sensitive）
    pub fn export(&self, dst: &Path) -> Result<(), VisionError>;      // D4 強制句：Sensitive 必 strip/gate
}
```

**`TemplateBytes`(明確 invariant,避免 PR3 不確定該存 PNG / raw RGBA / RgbaImage):** raw RGBA,只能經 checked constructor 建立。

```rust
pub struct TemplateBytes { width: u32, height: u32, rgba: Vec<u8> } // invariant: rgba.len() == width*height*4
impl TemplateBytes {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, VisionError>; // 檢查 w>0,h>0,len==w*h*4,w*h<=MAX_TEMPLATE_AREA
    pub fn to_rgba_image(&self) -> image::RgbaImage;                                 // invariant 保證有效，infallible
}
```

**D4 強制句(規格):** *Any code path that serializes presets outside local storage must inspect `TemplateSensitivity` before writing template bytes.* → SP1 的 `export()` 對 `Sensitive` 一律 **strip**(輸出不含其 bytes),deterministic、不需 UI;互動式「確認後包含」屬 SP6。`save_local()`(本機)不受限,寫全部。

**序列化隱私硬規則:** `TemplateAsset` / `TemplateStore` **不得** derive 一條會寫出 bytes 的 generic `Serialize`(否則 `serde_json::to_writer(f, &store)` 會繞過 `export()` 的 strip)。本機儲存與匯出走**各自明確的 record 型別**:

```rust
struct LocalTemplateAssetRecord  { handle, sensitivity, source, created_at_ms: u64, bounds_in_source_image: Option<ImageRect>, bytes: TemplateBytes }
struct ExportTemplateAssetRecord { handle, sensitivity, source, created_at_ms: u64, bounds_in_source_image: Option<ImageRect>, bytes: Option<TemplateBytes> } // Sensitive → None
```

PR3 測試:不只測 `export()` strip,也測**沒有任何 generic serialize path 能意外寫出 `Sensitive` bytes**。

SP1 store 用 in-memory + 本機序列化即可測;product 接線到 SP6。

### 4.4 `templateMatch` v0 + NMS

```rust
impl VisualIndex {
    pub(crate) fn template_match(&self, store: &TemplateStore, q: TemplateMatchQuery)
        -> Result<Vec<TemplateMatch>, CapabilityError>;
}

// 第 4 節 factoring：capability 與 self-validation 共用的核心（吃 template 影像，不經 handle）
pub(crate) fn match_template_image(index: &VisualIndex, tpl_gray: &image::GrayImage, region: Region, limit: u32)
    -> Result<Vec<TemplateMatch>, CapabilityError>;   // 回 Result（含 region/template 錯誤），不是裸 Vec
```

演算法:

1. `store.get(handle)` → 缺則 `Err(Failed{code:"template_not_found"})`。
2. **template 資訊量檢查**:variance/entropy 低於地板 → `Err(InvalidInput{code:"template_low_information"})`(防純色 template 在 NCC 下產生無意義高分/NaN)。
3. 搜尋區:`Full` = 整圖;`Rect` 經 `rect::to_pixel_rect`(規則見下)轉像素。template 比搜尋區大 → `Err(InvalidInput{code:"template_larger_than_region"})`。
4. template 轉灰階(一次);場景灰階取 `VisualIndex.gray()` 裁到搜尋區。
5. NCC:`imageproc::template_matching`,`CrossCorrelationNormalized`。**非有限(NaN/Inf)分數一律視為非命中,永不排在有限分數之上。**
6. 找峰 → **greedy NMS**(分數降序、stable;IoU > 0.4 後續抑制,用 `rect::iou`)→ 合併同實例群聚峰。
7. 排序取前 `limit`。每個 → `TemplateMatch{ bounds=(absX,absY,tplW,tplH), score, anchor=bounds 中心 }`。

`rect::to_pixel_rect(ImageRect, image_bounds) -> Result<PixelRect, CapabilityError>` 規則(寫死,避免 off-by-one / flaky 的 ±2px 測試):

- `x0=floor(x)`, `y0=floor(y)`, `x1=ceil(x+w)`, `y1=ceil(y+h)`,再 clamp 到影像邊界。
- non-finite → `InvalidInput{code:"non_finite_region"}`。
- clamp 前後 `width<=0 || height<=0` → `InvalidInput{code:"empty_region"}`。
- 面積溢位或超過 `MAX_SEARCH_AREA` → `InvalidInput{code:"region_too_large"}`。

**決定:**

- **host 不 threshold** —— capability 無 threshold 參數;回前 `limit` 名(分數降序、NMS 後),JS detector 自行 `.filter(m => m.score >= …)`。host 不設分數下限,保持可預測。
- **anchor = bounds 中心**(對齊 idea-doc JSON 範例)。
- NMS IoU 門檻、搜尋面積上限為帶預設值的常數(SP1 寫死,之後 config 化)。
- **防守 NCC 病態:** 低資訊量 template → `template_low_information` 拒絕;非有限分數視為非命中、永不排在有限分數之上(防純色 template 與過乾淨合成圖被怪峰打穿)。
- **決定論:** NCC + greedy NMS(stable sort,score → 位置 tie-break)可重現。

### 4.5 `TemplateSelfValidation`

純函式、deterministic。**author-time 用途,但 SP1 獨立建+測**;orchestration(LLM 粗定位 → snap → 呼叫此函式 → 依 decision 存/問)在 SP3。

```rust
pub enum ExpectedCount { Unique, Repeating, AtLeast(u32) }
pub enum TemplateDecision { Pass, NeedsConfirm, Reject }

pub struct SelfValidationConfig {
    pub expected_count: ExpectedCount,
    pub target_bounds: Option<ImageRect>,   // target_coverage gate
}

pub struct TemplateSelfValidation {
    pub self_score: f32,
    pub second_best_score: Option<f32>,
    pub peak_margin: f32,
    pub false_positive_count: u32,
    pub edge_density: f32,
    pub entropy: f32,
    pub stable_under_jitter: bool,
    pub decision: TemplateDecision,
}

pub fn self_validate(index: &VisualIndex, candidate_bounds: ImageRect, cfg: &SelfValidationConfig)
    -> Result<TemplateSelfValidation, VisionError>;
```

> `self_validate` 內部自 `index.image()` 依 `candidate_bounds` 裁出 candidate,因此知道它在原圖的原始位置 —— self_score「最佳命中是否回到原 bounds 附近」、jitter 穩定性、`target_coverage` 都需要這個(只給像素無從比對位置)。bounds 出界 → `VisionError`;內部 `match_template_image` 回的 `CapabilityError` 在此映射為 Reject / `VisionError`。

訊號:

- **self_score / second_best / peak_margin** — 對來源圖跑 `match_template_image`;`peak_margin = score[k] − score[k+1]`,`k` = 期望命中數(`Unique`→1)。乾淨的「懸崖」= 好。
- **false_positive_count** — 超過期望數、且分數 ≥ 警戒線(預設 ~0.7)的命中數。高 = 到處亂中。
- **edge_density / entropy** — 量 **template 本身**結構(梯度幅值佔比 + 強度直方圖熵)。**用 self_validation.rs 內部小 helper 算,不是(延後的)`regionFeatures` capability** —— author-time 內部指標,不開成 runtime capability。
- **stable_under_jitter** — template 微擾(裁切 ±1px、亮度 ±5%)重跑,最佳命中仍落原位且分數掉幅 < 容差。
- **gates** — `area bounds`(不可過大/過小)、`target_coverage`(若給 `target_bounds`,命中需覆蓋目標夠大比例,用 `rect::iou`)。

decision(deterministic 門檻,非 ML):

- **Reject** — self_score 不 ≈ 1、edge_density/entropy 低於地板、area 出界、false_positive_count 高、jitter 不穩,任一成立。
- **Pass** — 結構足夠 + peak_margin 乾淨 + false positive 低 + 穩定 + area OK +(`Repeating`/`AtLeast` 時)命中數達標 + target_coverage OK。
- **NeedsConfirm** — 中間地帶(SP3 接成 `NeedsUserInput`)。

**N≥2 nuance:** 由 `expected_count` 參數化 —— `Unique` 不要求多命中(多出的強命中算 false positive);`Repeating`/`AtLeast(n)` 才要求 ≥ n。

## 5. Error Model

- `VisionError`(建置/儲存期):空影像、IO/序列化失敗等。`build()` / `save_local()` / `export()` 回這個。(`export()` 對 `Sensitive` 是 strip 而非 error —— 見 §4.3。)
- `CapabilityError`(來自 `rollshot-automation`,capability 呼叫期):`template_not_found` / `template_larger_than_region` / `region_too_large` / `non_finite_region` / `empty_region` / `template_low_information` / `capability_unavailable` / `vision_index_unavailable`。對齊 idea-doc Resource Control level 3 的拒絕碼。
- 兩者分開:build/store 在 host 建構前/外發生,不在 capability 呼叫鏈內。

## 6. Security & Privacy (carry from D4)

- runtime 比對:敏感 template 在本機跑 NCC,**永遠不上傳**(runtime 無 LLM)→ 無需同意。
- 本機保存敏感裁切:自動 + 主動告知,不詢問(告知 UI 屬 SP6 product)。
- `Sensitive` 裁切:`export()` 必 strip/gate(§4.3 強制句);不進測試 fixture(PR6 fixtures 為合成、非敏感)。
- 對 §9.5 的範圍限定調整只針對 template 裁切,不動 fixture 政策。

## 7. Verification

### 7.1 Unit tests (PR1–PR5,各自)

- **rect.rs** — `to_pixel_rect` 的 floor-min/ceil-max rounding、non-finite(`non_finite_region`)、empty(`empty_region`)、超限(`region_too_large`);pad、iou、union deterministic 測試。
- **VisualIndex** — build 拒絕 0 面積;grayscale 正確性(合成圖)。
- **TemplateBytes** — checked constructor 拒絕 `len != w*h*4`、0 維、超 `MAX_TEMPLATE_AREA`。
- **TemplateStore** — by-handle get;missing → `None`(host 層轉 typed error);`save_local`/`export` round-trip;`export` strip `Sensitive`;**無任何 generic serialize path 能寫出 `Sensitive` bytes**。
- **templateMatch** — 合成 fixture 找到貼入 template;重疊去重(NMS);`limit` 生效;`template_larger_than_region` / `region_too_large` 錯誤;**低資訊量 template → `template_low_information`**;**非有限 NCC 分數視為非命中**。
- **self_validate** — distinctive 裁切 Pass;純色塊 Reject(edge/entropy 地板);重複紋理 Reject(false positive 高);jitter 穩定性;area-bounds gate;**`candidate_bounds` 出界 → `VisionError`**。

### 7.2 Integration tests (PR6 — role-free QuickJS fixtures)

pipeline:`合成 fixture → VisualIndex::build → 種 TemplateStore → validate_source(detector.js) → execute_to_proposal(…, &mut RealAutomationHost, policy, …) → EditProposal → 比對候選框`。

| 案例 | 期望 |
|---|---|
| browser top + bookmark strip | 1 候選,bounds ≈ 已知位置(±2px) |
| desktop folder grid(N 個) | N 候選,落在已知格點 |
| blank / 無關圖 | 0 候選(NCC 低分,JS `.filter` 濾掉) |
| 平移變體 | 仍命中(translation-invariant)→ pass |
| 縮放變體 | **known miss**:斷言找不到,鎖住 NCC 非尺度不變的已知行為 |
| `template_not_found` | `Err(Failed{code:"template_not_found"})` |
| SP1 呼叫 `layout` | `Err(Failed{code:"capability_unavailable"})`(鎖住 §4.1 安全決定) |

斷言:bounds ±2px;count/label;JS 門檻行為;跑兩次決定論一致;輸出是 `EditProposal`(transient `ProposedCandidate`),**不 mutate `ImageDocument`**(「候選可編輯」由 proposal 模型滿足,編輯 UI 屬 SP6)。

fixtures:**程式化合成**(畫 strip / 貼 icon glyph 成 grid),不放二進位 PNG;真實截圖 demo 屬手動 / SP6。detector JS = §A 兩個已驗證範例,放 `tests/fixtures/`。

## 8. PR Phase Breakdown (Model C)

整個 SP1 = **1 份本 spec + 1 份 plan**;以下 PR1–PR6 是 plan 內**可獨立測試 / commit 的 phases**,**每個 PR 收尾留一則 handoff note**。不為每個 PR 各寫一份 spec。

| PR | 內容 | 獨立測試 done-state |
|---|---|---|
| PR1 | crate skeleton(`rollshot-vision` + workspace 接線 + `imageproc` workspace dep + stub `RealAutomationHost`,所有 capability 回 `capability_unavailable`) | 編譯;`AutomationHost` 實作;無 OCR/OpenCV native dep |
| PR2 | `VisualIndex` + `rect.rs`(to_pixel_rect/clip/pad/iou/union) | 合成圖 deterministic 單元測試;build 拒絕 0 面積 |
| PR3 | `TemplateAsset`/`TemplateStore`/`TemplateSensitivity`(本機儲存 + serialize gate + store API) | by-handle 載入;missing→typed error;`export` strip `Sensitive` |
| PR4 | `templateMatch` v0 + NMS + `match_template_image` 核心 | 找到貼入 template;NMS 去重;limit 生效;錯誤碼 |
| PR5 | `TemplateSelfValidation`(純函式 + 內部 edge/entropy helper) | 好 template Pass;純色塊 Reject;到處亂中 Reject;jitter |
| PR6 | role-free QuickJS fixture 整合測試 | `hide bookmarks` / `hide folders` demo 本機過;負向案例;決定論 |

## 9. Risks & Carry-forward

- **長截圖 NCC 效能(spike 未驗):** naive NCC 是 O(搜尋面積 × template 面積);長圖(如 4000×12000)全圖搜尋為數秒級。SP1 緩解:裁搜尋區 + 搜尋面積上限 + CI fixtures 用一般解析度。延後優化:coarse-downscale→refine / FFT-based NCC / tiling。
- **NCC 非尺度/旋轉不變:** 換 DPI/主題/縮放會掉分。屬已知限制;未來以 `akaze`(純 Rust)或 object detector 處理,不在 SP1。
- **template_handle 範圍(idea-doc Open Q):** SP1 採 per-preset;global 重用 deferred。

## 10. Success Criteria

1. `cargo test -p rollshot-vision` 全綠(含 PR6 整合測試)。
2. 兩個 §A 已驗證 detector 經 `QuickJsExecutor` + `RealAutomationHost` 在合成 fixture 上產出預期候選框(±2px)。
3. 未實作 capability 回 `capability_unavailable`;`template_not_found` 路徑明確。
4. `Sensitive` template 不被 `export()` 寫出。
5. 全程零 LLM、deterministic、跑兩次一致。
6. workspace `unsafe_code = "forbid"` 不破;`imageproc` 單一版本(0.26)。

## Appendix A. Validated role-free detectors (fixtures)

兩段皆已通過 `rollshot-automation::validate_source`(本 spec 撰寫時驗證)。候選物件 `deny_unknown_fields`,僅 `kind`/`bounds`/`confidence`/`label`/選填 `rationale`。

```js
// Hide browser bookmarks
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
// Hide document folders on desktop
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

## References

- Parent: `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`
- Decisions/roadmap: `docs/ideas/2026-06-22-smart-redaction-auto-detection-architecture.md`
- `AutomationHost` / capability schema: `crates/rollshot-automation/src/{host,capability}.rs`
- Executor / policy / frontend: `crates/rollshot-automation/src/{executor,policy,frontend}.rs`
- QuickJS bridge: `crates/rollshot-automation-rquickjs/src/bridge.rs`
- `imageproc::template_matching`: <https://docs.rs/imageproc/latest/imageproc/template_matching/index.html>
