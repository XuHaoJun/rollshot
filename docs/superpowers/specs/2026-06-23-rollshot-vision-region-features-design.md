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

`RealAutomationHost` 新增 `prepared_region_features: Vec<PreparedRegionFeatures>`，與既有 `prepared_template_matches` 對稱，但**快取鍵用 canonical pixel rect，不用 raw `Region`**：

```rust
// 加在 rect.rs：PixelRect 目前只有 PartialEq, Eq；補上 Hash（u32 欄位，trivially derivable）。
// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub struct PixelRect { ... }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RegionFeaturesKey {
    rect: PixelRect, // 已解析、已 clip 進影像的整數 rect
}

#[derive(Debug, Clone)]
struct PreparedRegionFeatures {
    key: RegionFeaturesKey,
    max_limit: u32,
    results: Vec<RegionFeatures>, // v0：長度恆為 1
}
```

**為何不用 raw `Region`：** `Region::Rect` 內是 f32 `ImageRect`，直接比 raw region 會踩到浮點表示差、clip 後等價、`Full` vs「等價 full rect」等問題。prepare 與 callback 都先跑同一條 `region_to_pixel_rect(...) -> PixelRect` 收斂成同一 pixel-space 語意，再用 `RegionFeaturesKey` 比對，cache 命中才穩定。

> **SP1 latent 對照（不在 SP2 修）：** SP1 `prepare_template_match` 仍用 raw `Region` 相等比對（`prepared.region != query.region`），有同類脆弱性。SP2 改用 canonical key；SP1 那條屬既有 latent issue，本 spec 不擴散範圍去動它，僅在此記錄。

### 3.2 Data flow（鏡像 SP1 的 prepared-callback 契約）

prepare 與 callback **跑同一條 region 解析**，再用 canonical key 比對：

```
[QuickJS 外，expensive]                         [QuickJS 內，callback 只查快取]
prepare_region_features(index, query):          region_features(query):
  rect = region_to_pixel_rect(                     limit == 0 → InvalidInput "invalid_query"
           query.region, w, h,                     rect = region_to_pixel_rect(query.region, w, h,
           MAX_REGION_FEATURES_AREA)                          MAX_REGION_FEATURES_AREA)?  // 同一條
  key  = RegionFeaturesKey{ rect }                 key  = RegionFeaturesKey{ rect }
  edge = edge_density(index.gray(), rect)          找 prepared(key 相符)
  dom  = dominant_rgba(index.image(), rect)          無 → Failed "vision_index_unavailable"
  cache PreparedRegionFeatures{                     limit > max_limit → LimitExceeded（見 §5）
    key, max_limit: query.limit,                    回 results.take(limit)（v0 長度恆為 1）
    results: [RegionFeatures{
      bounds: rect→ImageRect,  // clipped measured bounds
      dom, edge }] }
```

`RegionFeatures.bounds` 回的是 **clip 後的量測 rect**（`PixelRect` 直接 cast 回 `ImageRect`），不是原始 requested bounds。QuickJS callback 維持 SP1 invariant：**只 lookup + truncate，不在 callback 做任何影像運算**（解析 region 成 key 是純整數運算，不碰像素）。prepare 以 `tracing`（`target: "rollshot::vision::region_features"`）記 duration 與 result_count，與 `prepare_template_match` 一致。

> **prepare 何時被呼叫 / SP2 的 dynamic query 硬限制：**
>
> SP2 **不從 JavaScript 推斷 `regionFeatures` 的 query。** 每一個 `regionFeatures` 呼叫，在進 `QuickJsExecutor` 前都必須有一個**已 prepare 的對應 canonical pixel rect**；沒有對應 key 的呼叫一律 `vision_index_unavailable`。
>
> 像 `input.imageWidth` 這種 dynamic region **允許**，但前提是 caller / test harness 在執行前用影像尺寸建出並 prepare**同一個 canonical query**。自動 query planning（從 manifest 抽 region 參數）是 **SP6**。SP2 整合測試手動 `prepare_region_features`，鏡像 SP1 PR6。

## 4. 演算法（deterministic、cheap、單次 pass）

常數以 named `const` 定義於 `region_features.rs`；確切數值於 plan 階段定。

- **`dominant_rgba(image: &RgbaImage, rect: PixelRect) -> [u8; 4]`**
  對 rect 內像素做量化 RGB histogram：每通道量化到固定 bin（`QUANTIZE_STEP`，例如 16 → 每通道 256/16 = 16 bins → 16³ 個）。取累計最多的 bin；回傳該 bin 的代表色（bin 中心 = `bin_index * QUANTIZE_STEP + QUANTIZE_STEP/2`）。tie-break：bin index 最小者勝（deterministic）。
  - **`QUANTIZE_STEP` 必須整除 256**（如 16 / 32 / 64），bin center / bin index 規則才乾淨；plan 階段選值時鎖死此約束。
  - **alpha：** SP2 假設 screenshot-like 不透明輸入，回傳 alpha `255`。若未來 caller 傳非不透明影像（composited `ImageDocument` layer / imported image），alpha 處理需重新檢視（屆時可改 dominant quantized RGB + majority/median alpha）。先寫明此 assumption，避免變成永久隱性契約。

- **`edge_density(gray: &GrayImage, rect: PixelRect) -> f32`**
  對 rect 內每個**同時有右鄰居與下鄰居**的像素算 `|g(x+1,y) - g(x,y)| + |g(x,y+1) - g(x,y)|`，超過 `EDGE_THRESHOLD` 即記為 edge pixel。
  - **分母明確**：counted set = rect 內 `x < x0+w-1` 且 `y < y0+h-1` 的像素，即 `(w-1)*(h-1)` 個；`edge_density = edge_count / counted`，落在 `[0,1]`。PR1 測試鎖住此定義。
  - rect 寬 <2 或高 <2 → `counted == 0` → 回 `0.0`（不 panic、不除零）。
  - 累加器用 `u64`（`edge_count` 與 `counted`），避免大圖 overflow；`GrayImage` 是 `u8`，無 non-finite 問題。
  - 不引 `imageproc` sobel，避免邊界與 dependency 行為變數。

兩個函式都吃已 clip 的 `PixelRect`，純讀、無 alloc 影像、deterministic。

## 5. Error model 與 limit 語意（複用既有碼，不新增）

**limit 語意（v0 結果長度恆為 0 或 1）：**

- `limit == 0` → `invalid_query`（與 `template_match` 一致）。
- `limit >= 1` → 回傳那唯一一個 prepared feature。
- `limit > prepared.max_limit` → 仍回 `LimitExceeded`，但**僅作為 manifest / bridge 的一致性 guard**，**不代表會產生多個 feature**。

> v0 **不**用 `limit` 控制 tiling 或 result count。`limit: 20` 不是「幫我切 20 塊」；它最多就是一個 feature。多 feature / 切割是永久 deferred（見 §1）。

**錯誤碼（全部複用既有）：**

- `invalid_query`（`limit == 0`）。
- `non_finite_region` / `empty_region` / `region_too_large`（來自 `region_to_pixel_rect`）。
- `vision_index_unavailable`（`Failed`，無對應 canonical key 時）。
- `LimitExceeded`（如上，consistency guard）。

新增常數 `MAX_REGION_FEATURES_AREA: u64`（直接沿用 `rect::MAX_SEARCH_AREA` 值即可）。**不新增** `VisionError` variant 或 `CapabilityError` 碼。

## 6. Privacy

`regionFeatures` 只回**量測聚合值**：一個代表色（`dominant_rgba`）＋一個純量（`edge_density`）。不含像素資料、不持久化、runtime 無 LLM 不上傳 —— 對應 D1「measured values, non-semantic labels, safe to use」與 D1 政策表（`regionFeatures`: author yes / runtime yes）。**無需任何同意或 gate。** 不涉及 `TemplateSensitivity` / 任何序列化路徑。

## 7. Verification

- **單元（合成圖，deterministic）**
  - `dominant_rgba`：純色 region → 該色；半紅半藍 region → 多數色；量化 tie-break 走最小 bin。
  - `edge_density`：純色 → `≈0`；高頻棋盤 → 高值；rect 寬/高 <2 → `0.0` 不 panic。
  - region 解析：clip 出界 rect 後 `bounds` 為 clipped measured rect；`non_finite` / `empty` / `region_too_large` 皆 typed error。
  - host：未 prepare → `vision_index_unavailable`；`limit == 0` → `invalid_query`；prepared 後**用 canonical key 查得**（含「raw region 不等但 canonical 等價」如 `Full` vs 等價 full rect 仍命中）且 `take(limit)` 正確。
- **整合（鏡像 SP1 PR6）**
  一支 role-free QuickJS fixture detector（單一來源 = `regionFeatures`，如 menu-bar 型：對固定頂端 strip 問 `edge_density`，低於門檻才輸出候選），經 `QuickJsExecutor` + 已 `prepare_region_features` 的 `RealAutomationHost`，在合成 scene 上產出預期候選。fixture JS 先過 `validate_source`（`filter` / `map` 在允許 subset 內）。
- **指令**：`rtk cargo test -p rollshot-vision`、`rtk cargo fmt --check`、`rtk cargo clippy -p rollshot-vision --all-targets -- -D warnings`。非 stitching 路徑，不需 bench。

## 8. PR breakdown（Model C：一份 spec + 一份 plan + per-PR handoff，沿用 SP1 慣例）

| PR | 內容 | 獨立 done-state |
|---|---|---|
| PR1 | `region_features.rs` 純函式 `dominant_rgba` / `edge_density` + named const（`QUANTIZE_STEP` 整除 256、`EDGE_THRESHOLD`）+ 合成圖單元測試 | 演算法在合成圖上 deterministic；`edge_density` 分母定義 `(w-1)*(h-1)` 被測試鎖住、u64 累加；無 host 接線；clippy/fmt 過 |
| PR2 | `PixelRect` 補 `Hash`；`RegionFeaturesKey` + `prepare_region_features` + `prepared_region_features` 快取；`region_features()` callback 改 cached lookup（取代 stub）；完整 error / limit model + 單元測試 | prepared 後查得且 **lookup 用 canonical `PixelRect` key、非 raw `Region` 相等**；未 prepare / `limit==0` / region 錯誤皆 typed；callback 不做影像運算；`bounds` 回 clipped measured rect |
| PR3 | role-free QuickJS regionFeatures fixture 整合測試（dev-dep rquickjs，鏡像 SP1 PR6） | demo detector 本機過、候選正確；**dynamic `imageWidth`-based fixture 在執行前明確 prepare 對應的 canonical rect**；handoff note: SP2 complete |

每個 PR 收尾在 `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`（沿用同一檔）追加一則 handoff note。

## 9. Open questions（plan 階段決，不阻擋本 spec）

1. `QUANTIZE_STEP`（整除 256）與 `EDGE_THRESHOLD` 的確切數值（先給保守預設，整合測試上微調）。
2. fixture detector 的 region 取得方式：固定像素 rect vs `input.imageWidth`（依 SP1 input 形狀決定）。無論哪種，test harness 都必須在執行前 prepare 對應 canonical rect（§3.2 硬限制），不影響 spec 不變量。

> alpha 處理已在 §4 定為「v0 假設 opaque、回 255、並寫明 assumption」，非 open question。
