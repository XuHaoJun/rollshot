# rollshot-vision regionFeatures v0 (Sub-project 2) — Design

**Date:** 2026-06-23
**Status:** Approved design
**Parent design:** `docs/superpowers/specs/2026-06-22-rollshot-vision-runtime-host-design.md` (SP1)
**Idea note (decisions D1–D2, roadmap):** `docs/ideas/2026-06-22-smart-redaction-auto-detection-architecture.md`

## 1. Summary & Scope

SP1 建好了 `crates/rollshot-vision`：`VisualIndex`、`RealAutomationHost`、`templateMatch` v0 + NMS、`TemplateSelfValidation`，以及「prepare 在 QuickJS 外、callback 只查快取」的契約。`region_features()` 目前是 stub，回 `capability_unavailable`。

本 sub-project（SP2）把 `regionFeatures` 補成可用的 **numeric sanity filter**：detector 對「一個已知 / 固定幾何的 region」問它的 `dominant_rgba` 與 `edge_density`，據此決定要不要輸出該 region 的 redaction 候選。對應 idea-doc D1（`regionFeatures` 是 runtime-allowed 的量測 sanity filter，非語意標籤）與 D2（v1 detector 單一來源，不做跨來源 fusion）。

### In scope (SP2)

- `regionFeatures({region, limit})` → 回傳描述**整個 requested rect** 的單一 `RegionFeatures { bounds, dominant_rgba, edge_density }`。
- per-region prepare：`RealAutomationHost::prepare_region_features`（QuickJS 外算、快取），callback 只 lookup + truncate。
- `region_features.rs`：deterministic 純函式 `dominant_rgba` / `edge_density`。
- 完整 error model（複用既有 `CapabilityError` 碼，不新增碼）。
- role-free QuickJS fixture 整合測試（單一來源 = regionFeatures）。

### Explicitly NOT in scope (deferred)

- **子區域切割 / connected-components / grid**（regionFeatures 不是 detector，只回單一 region 的聚合值）→ 永不在 v0；未來若真有腳本需要再評估。
- **`RegionFeaturesV2` 任何新欄位**（`text_density` / `component_count` / `brightness` / `saturation` / `aspect_ratio`）→ idea-doc §「不改 capability API 直到真實腳本證明需要」。
- **manifest-gated 全圖 edge map / `VisualIndexBuildOptions::from_manifest`** → 不引入；per-region 直接從已快取 grayscale 算。
- **跨來源 fusion**（layout + template + regionFeatures union/IoU）→ D2，deferred。
- author-time `inspectLayout`（SP4）、OCR（SP5）、product 接線（SP6）。
- **capability API 型別不動**：`rollshot-automation` 的 `RegionFeaturesQuery` / `RegionFeatures` 三欄位（`bounds` / `dominant_rgba: [u8;4]` / `edge_density: f32`）已存在，照用、不改。

本 sub-project 的成功定義：**「detector 用單一 regionFeatures 來源、在本機 deterministic 跑出可 review 的候選框」這條路可測、零 LLM。**

## 2. Context & Existing Boundaries

已存在、SP2 依賴而**不修改契約**的東西：

- `rollshot-automation/src/capability.rs` — `Region`（`Full` | `Rect{bounds}`）、`RegionFeaturesQuery { region, limit }`、`RegionFeatures { bounds: ImageRect, dominant_rgba: [u8;4], edge_density: f32 }`、`CapabilityError`。
- `rollshot-automation/src/host.rs` — `AutomationHost::region_features(&mut self, RegionFeaturesQuery) -> Result<Vec<RegionFeatures>, CapabilityError>`。
- `rollshot-automation/src/frontend/normalize.rs` — `("regionFeatures", CapabilityName::RegionFeatures)` 已在 allowlist（runtime-allowed，符合 D1 政策表）。
- `crates/rollshot-vision/src/index.rs` — `VisualIndex`：`image(): &RgbaImage`（來源真相）、`gray(): &GrayImage`（已快取）。
- `crates/rollshot-vision/src/rect.rs` — `region_to_pixel_rect(region, w, h, max_area) -> Result<PixelRect, CapabilityError>`、`PixelRect`、`MAX_SEARCH_AREA`。
- `crates/rollshot-vision/src/host.rs` — `RealAutomationHost`，既有 `prepare_template_match` + `prepared_template_matches` 的對稱結構。

座標型別：`ImageRect` 來自 `rollshot-image-document`（f32 像素座標）。

## 3. Architecture

### 3.1 Module layout

沿用 SP1 邊界，新增一個檔案、補一個 prepare 方法。`unsafe_code = "forbid"` 不變；無新 workspace 依賴（`image` / `imageproc` 已在）。

```
crates/rollshot-vision/src/
  region_features.rs   # 新增：純函式 dominant_rgba() / edge_density()
  host.rs              # 補 prepare_region_features()；region_features() 改 cached lookup
  index.rs             # 不動（沿用已快取 gray + RGBA image）
  rect.rs              # 不動（沿用 region_to_pixel_rect / PixelRect）
  lib.rs               # 視需要 re-export region_features 常數
```

`RealAutomationHost` 新增 `prepared_region_features: Vec<PreparedRegionFeatures>`，與既有 `prepared_template_matches` 對稱：

```rust
#[derive(Debug, Clone)]
struct PreparedRegionFeatures {
    region: rollshot_automation::Region,
    max_limit: u32,
    results: Vec<RegionFeatures>, // v0：長度恆為 1
}
```

### 3.2 Data flow（鏡像 SP1 的 prepared-callback 契約）

```
[QuickJS 外，expensive]                         [QuickJS 內，callback 只查快取]
prepare_region_features(index, query):          region_features(query):
  rect = region_to_pixel_rect(                     limit == 0 → InvalidInput "invalid_query"
           query.region, w, h,                     找 prepared(region 相符)
           MAX_REGION_FEATURES_AREA)                 無 → Failed "vision_index_unavailable"
  edge = edge_density(index.gray(), rect)          limit > max_limit → LimitExceeded
  dom  = dominant_rgba(index.image(), rect)        回 results.take(limit)（v0 長度恆為 1）
  cache PreparedRegionFeatures{
    region, max_limit: query.limit,
    results: [RegionFeatures{ bounds: rect→ImageRect, dom, edge }] }
```

QuickJS callback 維持 SP1 invariant：**只 lookup + truncate，不在 callback 做任何影像運算**。prepare 以 `tracing`（`target: "rollshot::vision::region_features"`）記 duration 與 result_count，與 `prepare_template_match` 一致。

> **prepare 何時被呼叫：** 與 SP1 `prepare_template_match` 相同 —— 由呼叫端在進 `QuickJsExecutor` 前準備。query-plan 自動抽取（從 manifest 抽 capability 呼叫的 region 參數）屬 SP6 product 接線；SP2 的整合測試手動 `prepare_region_features`，鏡像 SP1 PR6 的作法。

## 4. 演算法（deterministic、cheap、單次 pass）

常數以 named `const` 定義於 `region_features.rs`；確切數值於 plan 階段定。

- **`dominant_rgba(image: &RgbaImage, rect: PixelRect) -> [u8; 4]`**
  對 rect 內像素做量化 RGB histogram：每通道量化到固定 bin（`QUANTIZE_STEP`，例如 16 → 每通道 16 bins → 16³ 個）。取累計最多的 bin；回傳該 bin 的代表色（bin 中心值），alpha 固定 `255`（screenshot 不透明）。tie-break：bin index 最小者勝（deterministic）。

- **`edge_density(gray: &GrayImage, rect: PixelRect) -> f32`**
  對 rect 內每個有右/下鄰居的像素算 `|g(x+1,y) - g(x,y)| + |g(x,y+1) - g(x,y)|`，超過 `EDGE_THRESHOLD` 即記為 edge pixel。`edge_density = edge_count / counted_pixels`，落在 `[0,1]`。rect 的右/下邊界 1px 不計（無鄰居）；rect 寬或高 <2 時 `counted_pixels` 對應方向為 0 → 該情形回 `0.0`（不 panic、不除零）。不引 `imageproc` sobel，避免邊界與 dependency 行為變數。

兩個函式都吃 `PixelRect`（已 clip 進影像），純讀、無 alloc 影像、deterministic。

## 5. Error model（複用既有碼，不新增）

- `invalid_query`（`limit == 0`，與 `template_match` 一致）。
- `non_finite_region` / `empty_region` / `region_too_large`（來自 `region_to_pixel_rect`）。
- `vision_index_unavailable`（`Failed`，未 prepare 對應 region 時）。
- `LimitExceeded`（`limit > prepared.max_limit`，與 `template_match` 一致；v0 結果恆 1，此檢查多為形式上一致）。

新增常數 `MAX_REGION_FEATURES_AREA: u64`（直接沿用 `rect::MAX_SEARCH_AREA` 值即可）。**不新增** `VisionError` variant 或 `CapabilityError` 碼。

## 6. Privacy

`regionFeatures` 只回**量測聚合值**：一個代表色（`dominant_rgba`）＋一個純量（`edge_density`）。不含像素資料、不持久化、runtime 無 LLM 不上傳 —— 對應 D1「measured values, non-semantic labels, safe to use」與 D1 政策表（`regionFeatures`: author yes / runtime yes）。**無需任何同意或 gate。** 不涉及 `TemplateSensitivity` / 任何序列化路徑。

## 7. Verification

- **單元（合成圖，deterministic）**
  - `dominant_rgba`：純色 region → 該色；半紅半藍 region → 多數色；量化 tie-break 走最小 bin。
  - `edge_density`：純色 → `≈0`；高頻棋盤 → 高值；rect 寬/高 <2 → `0.0` 不 panic。
  - region 解析：clip 出界 rect；`non_finite` / `empty` / `region_too_large` 皆 typed error。
  - host：未 prepare → `vision_index_unavailable`；`limit == 0` → `invalid_query`；prepared 後查得且 `take(limit)` 正確。
- **整合（鏡像 SP1 PR6）**
  一支 role-free QuickJS fixture detector（單一來源 = `regionFeatures`，如 menu-bar 型：對固定頂端 strip 問 `edge_density`，低於門檻才輸出候選），經 `QuickJsExecutor` + 已 `prepare_region_features` 的 `RealAutomationHost`，在合成 scene 上產出預期候選。fixture JS 先過 `validate_source`（`filter` / `map` 在允許 subset 內）。
- **指令**：`rtk cargo test -p rollshot-vision`、`rtk cargo fmt --check`、`rtk cargo clippy -p rollshot-vision --all-targets -- -D warnings`。非 stitching 路徑，不需 bench。

## 8. PR breakdown（Model C：一份 spec + 一份 plan + per-PR handoff，沿用 SP1 慣例）

| PR | 內容 | 獨立 done-state |
|---|---|---|
| PR1 | `region_features.rs` 純函式 `dominant_rgba` / `edge_density` + named const + 合成圖單元測試 | 演算法在合成圖上 deterministic；無 host 接線；clippy/fmt 過 |
| PR2 | `prepare_region_features` + `prepared_region_features` 快取；`region_features()` callback 改 cached lookup（取代 stub）；完整 error model + 單元測試 | prepared 後查得；未 prepare / `limit==0` / region 錯誤皆 typed；callback 不做影像運算 |
| PR3 | role-free QuickJS regionFeatures fixture 整合測試（dev-dep rquickjs，鏡像 SP1 PR6） | demo detector 本機過、候選正確；handoff note: SP2 complete |

每個 PR 收尾在 `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`（沿用同一檔）追加一則 handoff note。

## 9. Open questions（plan 階段決，不阻擋本 spec）

1. `QUANTIZE_STEP` 與 `EDGE_THRESHOLD` 的確切數值（先給保守預設，整合測試上微調）。
2. fixture detector 的 region 取得方式：固定像素 rect vs `input` 是否提供 `imageWidth`（依 SP1 input 形狀決定；不影響 spec 不變量）。
