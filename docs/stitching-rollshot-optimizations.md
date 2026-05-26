# rollshot — 滾動截圖 Stitching 優化建議

> 基於 `stitching-analysis-rollshot.md` / `stitching-analysis-wayscrollshot.md` / `stitching-analysis-snow-shot.md` 三方對照分析,針對 rollshot 主路徑與 fallback 的改善空間。

## TL;DR

**不建議整套換成 snow-shot 算法**,因為會失去 rollshot 兩個架構性優勢:
1. **Overlap-and-overwrite 拓撲被動隱藏 sticky UI bar**(`overlap_size = max(0, H/2 − slice_px)`)
2. **兩階段像素驗證**(downsampled MAD over overlap → 160-row sample-band MAD)對誤匹配的拒判能力

**最高 CP 值**:integral image + SIMD NCC、真 image pyramid。可在不動驗證器與 sticky-bar 拓撲的前提下把熱點砍掉,估計 2-5× 改善。

---

## 為什麼不整套換成 snow-shot

| | rollshot | snow-shot |
|---|---|---|
| Sticky UI bar | overlap-and-overwrite 被動隱藏 | 無對策,純 paste 會反覆疊上 |
| 誤匹配拒判 | 兩階段像素驗證(MAD) | mode-vote + 嚴格 axis-lock,失敗就丟幀 |
| 內容變動容忍 | NCC + 多候選 + verifier | 28-D row/col-mean 描述子,動畫/lazy-load/AA 抖動下脆 |
| 匹配漸近成本 | `O(R·M·H)` NCC + coarse | `O(C·log N)` HNSW |

snow-shot 算法理論最快,但**靠丟幀換速度**。在 UI 動態變化較多的頁面上會出現「滾動沒抓到」的視覺斷檔。rollshot 走 NCC + verifier 路線是為了魯棒性,不應為了速度犧牲。

---

## 改善建議(依 ROI 排序)

### 1. Integral Image + SIMD NCC

**現況**:NCC 模板比對在中央 512-px 帶上,搜尋窗口 `R = ±80 px × M overlap rows`,每個候選位置都要重算 sum、sum²、cross-correlation,內部熱迴圈成本 `O(R·M·H)`。

**改法**:
- 對 `last_frame` 與 `new_frame` 各預計算 integral image(summed-area table)與 squared integral image,一次性 `O(W·H)`。
- NCC 公式中的 `Σx`、`Σx²`、`Σy`、`Σy²` 在任意矩形上變成 `O(1)` 4-point lookup。
- 剩下的 `Σxy` cross-correlation 項用 SIMD(`std::simd` 或 `wide` crate)向量化,單條 256-bit lane 一次處理 8 個 f32 / 32 個 u8。

**預期**:NCC 步驟單幀成本從 `O(R·M·H)` 降到 `O(R·H)`(以行為單位,常數小很多),M 是 overlap 的高度,通常 64-256 px。實測上 4-8× 加速可期。

**風險**:極低。不動匹配邏輯,只換實作。

**參考實作**:OpenCV `matchTemplate` 用的就是這個套路;`imageproc::template_matching` 沒有 integral image 加速,自寫即可。

---

### 2. 真正的 Image Pyramid(3-4 層)

**現況**:目前 coarse MAD 只用單層 4× 下採樣。

**改法**:
- 建立 Gaussian 或 Box pyramid,3-4 層(原始 / ½ / ¼ / ⅛)。
- 最頂層(⅛)做全範圍搜尋,offset 候選 propagation 到下層,每層只做 ±2 px refinement。
- 搜尋範圍 `S` 的成本從 `O(S)` 變 `O(log S)`。

**預期**:
- 對於大幀(4K 螢幕)與大位移(快速滾動 200+ px),效果最顯著。
- 與 (1) 結合使用:在每一層用 integral image + SIMD NCC,理論上能在每幀 < 5 ms 內完成主路徑。

**風險**:中。需要小心 pyramid 邊界條件(downsample filter 選擇、aliasing)。Box filter 最快但對 high-freq 內容(細字)會 alias;Gaussian 較慢但穩定。建議從 Box 開始,測誤匹配率。

---

### 3. FFT Phase Correlation 作為主路徑或第二候選

**現況**:三路候選 coarse MAD / NCC / 1-D 邊緣 MAD,都靠 spatial-domain 搜尋。

**改法**:
- FFT-based phase correlation: `F(I_a) · conj(F(I_b)) / |...|` 再 inverse FFT,peak 位置直接是 (dx, dy)。
- 全範圍、不需要 search window;sub-pixel 精度可從 peak 鄰域擬合取得;對亮度變化魯棒(只取相位)。
- 漸近 `O(W·H·log(W·H))`,對大幀比 NCC + S 階搜尋好。

**預期**:
- 可取代「coarse MAD + 1-D 邊緣 MAD」兩支,主路徑簡化為 phase correlation + NCC verifier。
- Sub-pixel 精度免費,可以累積分數位移在 slice 計算中(雖然 paste 仍是整數,但能消除累積誤差)。

**風險**:高。
- 動到主流程,需要先建 benchmark harness 比較三種主路徑(現行 / phase corr / pyramid NCC)。
- FFT 對 power-of-2 size 友好,可能需要 zero-pad,常數變大。
- 對部分滾動內容(只有頁面一小部分滾動,其他靜止)peak 會分裂,需要額外處理。

**建議時機**:在 (1)(2) 都做完後評估。

---

### 4. HNSW ANN 取代 Fallback 的 Linear-KNN

**現況**:Fallback 用 FAST corners + 8-D row/col-mean descriptor + symmetric linear-KNN + 4-px bucket voting。Linear-KNN 是 `O(K²)` brute-force。

**改法**:
- 換成 HNSW(`hora` 或 `instant-distance` crate),query `O(K·log N)`。
- 僅在 fallback path 改,主路徑 NCC 不動。

**預期**:
- 主路徑 miss 後的恢復速度更快。
- 影響面小,風險低。

**風險**:低。Fallback 觸發頻率本身就低,影響的是「最壞情況延遲」,不是 steady-state。但這對使用者體感的卡頓抑制有幫助。

---

### 5. Sub-Pixel Parabolic Fit on NCC Peak

**現況**:NCC 取整數 offset。

**改法**:對 NCC peak 的 3×3 鄰域做拋物線擬合,取得 sub-pixel (dx, dy)。

**預期**:
- 雖然 paste 仍是整數 slice,但可以**累積分數誤差**到下一幀做補償,避免大字級內文滾動時的 1px 視覺抖動。
- 計算成本幾乎為零(9 個值的二次擬合)。

**風險**:極低。

---

### 6. 跳過 RGBA→Gray(如果 capture 後端支援)

**現況**:每幀都要做 RGBA → grayscale 轉換,`O(W·H)` 全幀。

**改法**:
- 如果 capture backend(scap 或平台原生)能直接給 NV12 / YUV 平面,拿 Y plane 即可,省掉一次全幀 luma 計算。
- 主要是 macOS ScreenCaptureKit 與 Wayland portal 都可選 YUV format。

**預期**:省一次 `O(W·H)` 全幀掃描;對大幀(4K @ 60fps)影響顯著。

**風險**:中。要動 capture 層,且不是所有 backend 都支援。建議列為 capture 層的後續改善,不要綁進 stitching 改造。

---

## 建議的執行順序

1. **建 benchmark harness** — 用一組固定的 capture sequence(理想是錄製過的真實 wheel-driven 序列)當 ground truth,測:每幀延遲、誤匹配率、視覺品質(可人工 spot check)。
2. **(1) Integral image + SIMD NCC** — 最安全的熱點優化,先做。
3. **(2) Pyramid** — 在 (1) 基礎上再上一層,大位移場景特別有感。
4. **(5) Sub-pixel + (4) HNSW fallback** — 兩個小型獨立改善,可平行做。
5. **(3) Phase correlation** — 在前面都做完後,看看主路徑是否還值得換。需要實驗 vs 現行 pipeline。
6. **(6) YUV 直通** — 配合 capture 層改造節奏,獨立規劃。

---

## 不建議現在做的事

- **整套換成 snow-shot 風格**:失去 sticky-bar 處理與 verifier 魯棒性,得不償失。
- **GPU(wgpu compute)做 NCC**:除非未來目標延伸到 4K/120Hz 即時 stitching,否則目前 CPU + SIMD 已能在 budget 內。GPU 引入 PCIe transfer overhead 與額外的同步複雜度。
- **改用 ORB / AKAZE 主路徑**:OpenCV 依賴 + brute-force kHamming kNN 漸近最差(見 wayscrollshot 分析);AKAZE 目前 opt-in 已是合理位置,不必動。
- **預先設計可切換多算法的抽象層**:目前 rollshot 已有 fallback chain,再加抽象只是過度設計。等 (3) 真要評估時,benchmark harness 就是切換點。

---

## 附錄:相關文件

- 主路徑與 fallback 細節:`docs/stitching-analysis-rollshot.md`
- 對照參考:`docs/stitching-analysis-snow-shot.md`、`docs/stitching-analysis-wayscrollshot.md`
