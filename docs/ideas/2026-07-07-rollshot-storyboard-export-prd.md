# Rollshot PRD: Storyboard Export（關鍵步驟大圖匯出）

- **Status:** Draft
- **Author:** ChatGPT
- **Date:** 2026-07-07
- **Area:** `rollshot-action`, `rollshot-app`, export pipeline
- **Related:** Action Guide, Export GIF, Export MP4 Summary, Local Issue Pack

---

## 1. Summary

新增一個新的匯出能力：**Storyboard Export**。

Storyboard Export 會將使用者在 Action Guide 中保留下來的 keyframes / steps，整理成一張可分享的大圖（初版為 PNG），讓使用者可以直接貼到 Slack、Discord、GitHub issue comment、PR 討論或文件中，快速傳達「這幾個步驟發生了什麼」。

這個功能不是單純把 N 張圖拼接，而是將關鍵步驟輸出成**可閱讀的流程版面**：

- 顯示 step number
- 顯示 keyframe
- 可選顯示 step title / caption
- 以一致的卡片式 layout 組成單張長圖

一句話定位：

> 將關鍵操作步驟整理成一張可分享、可快速理解的流程大圖。

---

## 2. Why now

Rollshot 已經具備：

- Action Guide（steps / keyframes / timeline）
- reviewed guide export（Markdown）
- GIF summary export
- MP4 summary export

MP4 已經補上了「動畫摘要」的輸出能力；下一步很自然是補上「**靜態但適合聊天分享**」的輸出能力。

相較於 Guide Markdown：

- Storyboard 更輕量
- 更適合聊天室與即時討論
- 預覽成本更低
- 不需要對方播放影片或打開附件

相較於 MP4/GIF：

- 在 Slack / Discord / issue comment 中更容易快速理解
- 分享後可直接在聊天視窗中預覽完整流程
- 對「幾個關鍵步驟」的溝通更有效率

---

## 3. Problem statement

目前 Rollshot 已能輸出 Guide、GIF、MP4，但在日常溝通中仍缺少一個最輕量的 artifact：

> 「把這幾個關鍵步驟濃縮成一張圖，直接貼出去。」

使用者現在若想達成這件事，通常只能：

1. 手動把數張 keyframes 匯出
2. 再用其他工具拼接
3. 手動標 step number / 說明
4. 匯出後貼到 Slack

這個流程繁瑣，且結果往往：

- 排版不一致
- 沒有清楚的步驟編號
- 難以快速閱讀
- 不利於重複使用

Rollshot 應提供一個內建、低摩擦的方式，直接將 reviewed steps 變成一張可分享的流程大圖。

---

## 4. Goals

### Product goals

1. 讓使用者可以在 **1 次操作內** 匯出可分享的大圖。
2. 讓圖像在 **Slack / Discord / issue comment / docs** 中有良好的閱讀體驗。
3. 讓輸出內容清楚呈現「流程 / 關鍵步驟」，而不是單純的圖庫拼接。
4. 與既有 Action Guide / GIF / MP4 能力形成互補。

### User goals

使用者可以：

- 快速分享幾個操作步驟
- 快速說明 bug reproduction flow
- 快速展示功能流程或 onboarding 流程
- 以單張圖取代影片或一堆零散截圖

---

## 5. Non-goals

第一版**不做**：

- 不做完整的 rich editor / 自由排版器
- 不做 PDF / HTML 多格式輸出（第一版只做 PNG）
- 不做影片時間軸回放
- 不做 full custom template system
- 不做 click animation / cursor overlay
- 不做複雜註解工具（箭頭、框線、callout 等）
- 不做自動分欄 / 自動分頁的太多版型
- 不保證單張圖適合超大量 steps（超過一定數量可先簡單處理）

---

## 6. Primary use cases

### Use case A: Slack 分享關鍵流程

使用者完成一段錄製後，在 review timeline 中只保留 4 個關鍵步驟，然後匯出 Storyboard PNG，直接拖進 Slack，讓同事快速理解流程。

### Use case B: Bug reproduction summary

使用者用 Action Guide 錄下 bug 重現流程，匯出 Storyboard，貼到 GitHub issue comment 或內部聊天中，作為快速證據與說明。

### Use case C: Feature walkthrough

使用者錄下一個簡短操作流程，例如「如何開啟設定並儲存」，用 Storyboard 作為輕量 onboarding 圖。

### Use case D: 搭配 Issue Pack

使用者稍後可在 Local Issue Pack 中，將 Storyboard 當成主要預覽圖，取代或輔助單張 final screenshot。

---

## 7. User personas

### 1. 開發者 / 工程師

需要快速向同事說明 bug、重現步驟或某個 UI 流程。

### 2. QA

需要把操作步驟與異常畫面濃縮成可分享的證據圖。

### 3. PM / Designer

需要快速討論產品流程，而不是傳一串零散圖片或一支影片。

### 4. Support / Internal ops

需要分享簡短的操作教學或故障排查步驟。

---

## 8. UX principles

1. **Readable first**：輸出重點是可閱讀，不是花俏。
2. **Chat-friendly**：適合 Slack / Discord 預覽。
3. **Guide-aware**：以 reviewed steps 為準，不重新發明資料模型。
4. **Low-friction**：應像 Export GIF / Export MP4 一樣容易理解。
5. **Safe defaults**：版面與尺寸有合理預設，避免使用者需要設定太多參數。

---

## 9. Proposed feature

### Feature name

建議命名：

- **Export Storyboard**（推薦）

備選：

- Export Stepboard
- Export Step Sheet
- Export Flow Sheet
- Export Contact Sheet

`Storyboard` 的好處是語義清楚，使用者容易理解這是一種「多步驟流程圖像」。

---

## 10. Output format (V1)

### File format

- `PNG`

### Default filename examples

- `storyboard.png`
- `rollshot-storyboard.png`
- `guide-storyboard.png`

### Rationale

PNG 是第一版最穩定的選擇：

- Slack / Discord 預覽友善
- 實作相對簡單
- 畫質穩定
- 不需處理 PDF / HTML renderer 複雜性

---

## 11. V1 layout

### Chosen layout: vertical single-column storyboard

每個 step 以一個卡片 block 呈現，自上而下排列：

- Step number
- optional title
- keyframe image
- optional one-line caption（若已有標題則 caption 可先不做）

示意：

```text
Storyboard

Step 1
[ image ]

Step 2
[ image ]

Step 3
[ image ]
```

### Why V1 chooses single-column

- 最符合「流程」心智模型
- 最適合 Slack / chat 預覽
- 與目前 guide steps 順序一致
- 實作與驗證成本最低

---

## 12. Visual design guidelines

### Canvas

- 白色背景
- 固定外邊距（例如 24 px）
- 合理卡片間距（例如 20 px）

### Header

第一版可選擇：

- 無 header，直接輸出 step cards
- 或簡單 header：`Storyboard` / `Rollshot Storyboard`

為避免品牌與文字渲染複雜度，第一版可採**極簡 header 或無 header**。

### Step card

每張卡片包含：

- `Step N`
- step title（若存在）
- keyframe image
- 淡色邊框或簡單分隔

### Typography

- 系統字體或應用內已存在可穩定使用的字體
- Step number 要明顯
- Title 次之

### Image sizing

- 所有 keyframes 以統一顯示寬度輸出
- 高度保持原比例
- 若原始圖像過大則縮放
- 初版不做複雜裁切

---

## 13. V1 scope details

### Required content

1. reviewed step order
2. each step's retained keyframe
3. step number label
4. optional step title if available

### Optional content in V1

- `Created with Rollshot` footer（可 defer）
- recording timestamp（可 defer）
- session title（可 defer）

### Explicitly deferred from V1

- grid layout
- custom theme / dark theme export
- user-editable captions within export modal
- callouts / arrows / annotations
- multiple pages
- auto split very long output into multiple images

---

## 14. UX flow

### Entry point

在 Action Guide review screen 中新增：

- `Export Storyboard`

可放在與 `Export GIF` / `Export MP4` / `Export Guide` 同一區。

### Basic flow

1. 使用者錄製流程
2. 在 review timeline 中刪減 / 保留 steps
3. 點擊 `Export Storyboard`
4. 選擇輸出位置
5. 匯出 PNG
6. 成功後顯示 success message

### Error cases

- 無 steps：不可匯出，顯示 empty state message
- step 對應 keyframe 不存在：匯出失敗，顯示錯誤
- 使用者取消 save dialog：無事發生
- 寫檔失敗：顯示 export failed message

---

## 15. Functional requirements

### FR1. Export command

系統必須提供一個 `Export Storyboard` action。

### FR2. Reviewed steps as source of truth

Storyboard 必須以目前 reviewed / visible steps 順序作為資料來源。

### FR3. Keyframe rendering

每個 step 必須渲染對應 keyframe 圖像。

### FR4. Step numbering

每個 step 必須有清楚的步驟編號。

### FR5. Optional title

若 step model 已存在 title / label，輸出中應顯示；若沒有，至少顯示 `Step N`。

### FR6. Single PNG output

第一版輸出必須為單一 PNG 檔案。

### FR7. Stable layout

輸出圖片中的各 step block 必須有一致的寬度、間距與排版。

### FR8. Failure handling

匯出失敗時，應回傳具體錯誤並在 UI 顯示非崩潰式提示。

### FR9. Non-destructive export

匯出不應修改 guide state，也不應關閉 timeline review 畫面。

---

## 16. Non-functional requirements

### NFR1. Deterministic output

相同 guide 與相同參數應產出一致結果。

### NFR2. Reasonable performance

一般 3–8 steps 的輸出應在可接受時間內完成（目標 < 1s～2s，視圖片大小而定）。

### NFR3. Memory safety

匯出過程不可無限制持有過多 full-size image copies。

### NFR4. Output readability

輸出圖片在常見聊天工具縮圖預覽下，仍應大致可辨識步驟順序。

### NFR5. Recoverable failure

若某步驟圖像缺失或輸出失敗，應以 recoverable error 呈現，而不是 panic。

---

## 17. Data source and model assumptions

V1 不引入新的核心資料模型；應重用現有 Action Guide / FrameStore 資料。

可假設存在或可導出：

- `Guide`
- `GuideStep`
- `FrameStore`
- retained keyframe lookup

建議新增 export-specific model：

```rust
pub struct StoryboardOptions {
    pub max_width: u32,
    pub outer_padding: u32,
    pub card_spacing: u32,
    pub card_padding: u32,
    pub show_titles: bool,
}

pub struct StoryboardExportResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}
```

---

## 18. Technical design direction

### High-level pipeline

```text
Guide
  → resolve reviewed steps
  → fetch retained keyframes
  → compute layout
  → rasterize storyboard canvas
  → save PNG
```

### Proposed module

可新增：

```text
crates/rollshot-action/src/storyboard.rs
```

或

```text
crates/rollshot-export/src/storyboard.rs
```

視目前 export 組織而定。

### Internal stages

1. **Resolve steps**
   - 取得 reviewed steps
   - 驗證 step count > 0

2. **Resolve images**
   - 根據 step keyframe id 從 store 取出圖像
   - 必要時縮放

3. **Measure layout**
   - 計算每張卡片高度
   - 計算總畫布高度

4. **Render**
   - 建立空白 canvas
   - 依序繪製每張卡片背景、文字、keyframe

5. **Save**
   - 寫入 PNG
   - 成功後回傳 metadata

---

## 19. Rendering approach

StoryBoard 不是 HTML/PDF template；第一版建議直接 image composition。

### Why direct raster rendering

- 依賴少
- 跨平台較穩定
- 易於控制像素輸出
- 與 GIF / MP4 pipeline 心智接近

### Likely dependencies / building blocks

- `image` crate 進行畫布與圖片處理
- 文字渲染沿用 app 內既有可用方案，或新增簡單 text rasterization 能力

### Important note

第一版可將文字需求壓低：

- 最少只渲染 `Step N`
- 若 title rendering 成本過高，可先讓 title 為 optional/deferred

不應為了漂亮字體而延誤 MVP。

---

## 20. Layout constraints

### Default width

建議提供一個合理預設畫布寬度，例如：

- 1200 px 左右

足以兼顧：

- Slack 預覽清晰
- 單張圖片不要過寬
- 檔案大小可控

### Keyframe width

- 卡片內圖像寬度 = 畫布寬度扣除左右 padding
- 高度依比例縮放

### Very tall outputs

初版可接受長圖；但應有一個基本 guard：

- 若 steps 非常多，仍允許匯出
- 但可能在 UI 或後續版本中提醒「輸出會很長」

V1 不需要自動分頁。

---

## 21. Success metrics

此功能上線後，若有 telemetry，可觀察：

1. `Export Storyboard` 使用次數
2. Storyboard 與 GIF / MP4 / Guide 的相對使用率
3. 匯出成功率
4. 平均匯出 step count
5. 匯出失敗原因分布

若目前未做 telemetry，至少應先觀察內部使用回饋：

- 是否比 MP4 更常被丟到 Slack
- 是否明顯降低分享摩擦
- 是否有使用者要求加標題 / 分欄 / annotations

---

## 22. Risks and tradeoffs

### Risk 1: 只是拼圖，沒有可讀性

如果輸出只是簡單疊圖，使用者會覺得價值不足。

**Mitigation:**
- 一定要有 step cards
- 明確 step number
- 一致 spacing 與版面

### Risk 2: 文字渲染成本高於預期

**Mitigation:**
- V1 只要求 `Step N`
- title 可選 / 可 defer

### Risk 3: 圖太長，不適合某些平台

**Mitigation:**
- V1 先接受這個限制
- V2 再考慮 grid / pagination / compact mode

### Risk 4: 與 Guide export / MP4 重疊

**Mitigation:**
- 明確定位：Storyboard = 最適合聊天分享的靜態摘要圖

---

## 23. Open questions

1. Step title 是否在目前資料模型中已穩定存在？
2. 文字渲染能力是否可直接重用現有 app stack？
3. 是否要在 V1 加簡單 header？
4. Storyboard 是否需要支援 redaction-aware export？
   - 若當前 keyframes 已經是 safe/export-safe 版本則較簡單
   - 若否，之後與 Smart Redaction / Issue Pack 會需要整合
5. 匯出檔名是否要包含 timestamp？

---

## 24. Rollout plan

### Phase 1 (MVP)

- Export Storyboard button
- single-column vertical layout
- PNG output
- step number
- keyframe rendering
- optional title if easy

### Phase 2

- compact mode
- grid layout / 2-column mode
- better header/footer
- title/caption improvements

### Phase 3

- redaction-aware storyboard
- Issue Pack integration
- copy image to clipboard
- split into multiple images when too long

---

## 25. Priority recommendation

建議優先級：**High**

理由：

- 技術風險低於完整新子系統
- 與 Action Guide / MP4 export 直接互補
- 日常使用情境清晰
- 很可能成為高頻分享格式

若要排在近期開發序列中，我會建議：

1. MP4 summary 完成與穩定
2. **Storyboard Export（本 PRD）**
3. Storyboard 納入 Local Issue Pack
4. 再考慮更重型的 full recording / richer annotation 能力

---

## 26. Final recommendation

Storyboard Export 是一個典型的高價值、低風險、與現有架構高度契合的功能。

它能補上 Rollshot 在「輕量分享」上的最後一塊拼圖：

- Guide：正式文件
- GIF / MP4：動畫摘要
- **Storyboard：聊天分享的靜態摘要圖**

建議立刻以 V1 形式推進，先做出一個穩定、簡潔、可分享的 PNG storyboard 匯出能力。
