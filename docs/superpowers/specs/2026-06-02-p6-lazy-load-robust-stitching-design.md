# P6 — Lazy-load Robust Stitching: Robust Verifier + Routine Feature Consensus

> 本 spec 對應 `docs/stitching-rollshot-optimizations-2.md` 的 **P6（Indexed feature
> fallback / HNSW）**，但**修訂並擴張**了它的範圍：原 P6 只把 HNSW 當作「降低 fallback
> 最壞延遲」的純效能項、default-off feature flag、且堅持 feature 候選仍走嚴格 verifier。
> 本 spec 在效能之外，**把 P6 重新定位為一個正確性修復**：讓長截圖對 **lazy-load /
> 載一次就定的動態圖片** 魯棒，同時保留 §1.2「verifier 是最終防線」的精神。
>
> 前情：我們剛修掉「第一張 frame 是 lazy-load 佔位 → 永遠 scroll too fast、卡在第一張」
> 的 bug（`stitcher.rs` 的 `REANCHOR_MISS_THRESHOLD`，scoped to `frame_count == 1`）。
> 那是最小止血，**只**蓋第一張。本 spec 處理更一般的 lazy-load 魯棒性。

---

## 1. 背景與動機

### 1.1 已確認的 root cause（重現過）

長截圖逐幀以影像比對縫合，錨點（`last_good`）只在成功 append 時前進。當**重疊區的內容
在兩幀之間改變**（典型成因：商品圖 lazy-load，第一幀拍下時還是佔位/模糊，之後才載入），
嚴格的 `PixelOverlapVerifier`（`verifier.rs`，full-res sample band MAD ≤ 18/255）會**否決
幾何上正確的位移**，因為 sample band 剛好壓在那塊變動的圖上。matcher 其實找得到正確位移
（重現中 coarse 找到 dy=160），但被 verifier 默默丟掉，最後對外回報誤導性的
`FeatureLowInliers`。

### 1.2 為什麼第一張的 re-anchor 修法不夠

lazy-load 會在內容捲入視窗時觸發，所以它**常常發生在 capture 中段**，不只第一張：
錨點是好的 frame N，frame N 的下緣圖還沒載入，frame N+1 才載入 → 重疊區（錨點下緣）對不
起來 → NoMatch。而錨點下緣**永遠**落在重疊區內，所以中段的 stale 錨點會跟第一張一樣卡住。
我們已 ship 的 re-anchor 只在 `frame_count == 1` 觸發，**不蓋中段**。

### 1.3 與參考實作的對照（為何 feature path 是關鍵）

snow-shot / wayscrollshot 對此天生較免疫，原因有二（見先前調查）：
1. **比對對局部內容變化的容忍**：feature inlier 投票用全幀多數特徵共識，局部變動只是少數
   outlier；不像逐像素 MAD 會被一塊變動區拉爆。
2. **錨點持續前進**：reference 隨內容前進（snow-shot edge-index 依位移 lazy rebuild；
   wayscrollshot 多數演算法每幀推進 reference），stale 幀自然被取代。

本 spec 借鑑這兩點，但**不**整套照搬：保留 verifier 當最終防線（§1.2 invariant）。

---

## 2. 目標 / 非目標

### 2.1 目標（success criteria）

- **load-once lazy 圖**（載一次就穩定）在重疊區造成的內容變化，**第一張與中段**都能：
  - 正確找到位移並 append（不卡 NoMatch），且
  - 輸出正確（靠 overlap-and-overwrite，stale 像素被較新一幀覆寫）。
- matcher 找不到位移的「大塊變動」case，由 routine feature 共識救回。
- **不引入誤匹配**：repeated grid / low texture / sticky header / 全域錯位 不得 regression。
- **非動態序列輸出 byte-identical**（靠 §4.2 的 monotonic 設計）。

### 2.2 非目標（明確排除）

- **永久動畫 / autoplay video / GIF / carousel**：重疊區像素每幀都不同，逆像素驗證永遠
  對不起來；本 spec 不處理（只能靠 ③ 安全網不卡死）。
- **版面 reflow / CLS（內容位置跨位）**：破壞純平移假設，超出範圍。
- **把 feature / HNSW 升為 primary matcher**：仍是候選來源之一（roadmap §12.2）。
- **繞過 verifier 接受**（即先前 brainstorming 的選項 b）：明確不採用；保留 (a)。

---

## 3. 與既有 invariant 的關係

roadmap §1.2 / §14 的 invariant 大多**保留**，本 spec 只做兩處**有意識、有界、會被記錄**的
修改：

| Invariant（roadmap §1.2 / §14） | 本 spec | 說明 |
|---|---|---|
| Duplicate 不進 matcher | 不變 | |
| DimensionMismatch 不污染 state | 不變 | |
| `OverlapVerificationFailed` 不 append | 不變 | |
| ReverseDirection 預設拒絕 | 不變 | |
| `full_image()` 回傳單張 RgbaImage | 不變 | |
| **PixelOverlapVerifier 仍是 final gate** | **修改（保留精神）** | 統計量由「mean MAD」→「mean MAD **∨** confidence-gated tile-vote」。仍是最終防線、仍有 majority floor，只是對**局部**變動容忍。見 §4.2。 |
| **NoMatch 不更新 anchor** | **有界例外（擴張既有例外）** | 已 ship 的 first-frame re-anchor 是此例外；本 spec 擴張到中段（連續 K 次 NoMatch 才觸發，且為 last-resort）。見 §4.4。 |
| 非動態序列輸出與 baseline 一致 | 保證 | monotonic superset 設計，§4.2。 |

---

## 4. 設計

### 4.0 整體 acceptance pipeline

```text
Incoming frame
  -> duplicate signature gate (不變)
  -> PreparedFrame(curr)
  -> candidates = coarse + template + edge + [feature/HNSW (routine, §4.3)]
  -> rank by confidence
  -> ROBUST PixelOverlapVerifier (§4.2)
        accept ⇔ (legacy strict-mean passes) OR (confidence-gated tile-vote passes)
  -> overlap-and-overwrite append  (stale 像素被較新幀覆寫 → 輸出正確)
  -> [若連續 K 次仍 NoMatch] ③ mid-capture re-anchor (§4.4, last-resort)
```

分工：
- **①（robust verifier）**：救「matcher 找到位移、但舊 verifier 因局部變動否決」的常見 case。
- **②（routine feature）**：救「大塊變動讓 matcher 連位移都找不到」的 case + 一般 recall；
  並提供 inlier-ratio 餵給 ① 的 confidence gate。
- **③（mid-capture re-anchor）**：①②都救不回時的安全網，保證永不永久卡死。

### 4.1 不採用的方案（記錄理由）

- **(b) feature 共識可覆蓋 verifier 否決**（snow-shot 模型）：破壞 §1.2 final-gate
  invariant，且高自信但錯誤的 feature 共識會在無像素 sanity check 下縫進去（roadmap §1.1
  對 snow-shot 的疑慮）。對 load-once lazy 圖，①+② 已足夠，故不採用。可作為未來 escalation
  保留討論。

### 4.2 ①：Robust / confidence-gated PixelOverlapVerifier

**現況**（`verifier.rs`）兩段都是 mean MAD：
- Stage A（cheap）：`downsampled_mad` 全重疊區，step 4，閾值 `downsample_max_mad = 24/255`。
- Stage B（strict）：`sample_band_mad` 底部 ~160 列（seam），閾值 `full_res_max_mad = 18/255`。

mean 會被一塊局部變動拉爆 → 否決正確對齊。

**改法 — tile-vote（加在既有 mean 之上，不取代）：**

```text
把重疊區切成 tiles（例如 32–64px）。
一個 tile「同意」⇔ 其 mean MAD < tile_tol。
tile-vote 通過 ⇔ 同意 tile 比例 ≥ accept_ratio。

最終接受 ⇔ (legacy mean Stage A+B 通過)  OR  (confidence-gated tile-vote 通過)
```

**Monotonic superset（風險上界的關鍵）：** 接受條件是「舊嚴格 mean **OR** tile-vote」。
乾淨內容仍走原 mean 路徑 → **非動態序列輸出 byte-identical**。tile-vote 只**新增**接受
（lazy-load case），永不否決舊 verifier 接受過的東西。所有誤匹配風險集中在 tile-vote 路徑，
由下列兩道控制把關：

1. **Majority floor**：`accept_ratio` 不得低於 ~0.6。全域錯位（多數 tile 不同意）**永遠**
   失敗，無論 confidence。← 這就是保留下來的誤匹配防線。
2. **Confidence-gating**：放寬程度（容許多少壞 tile）只在**位移本身**有獨立佐證時才放大：
   - 來自 template/coarse 的 NCC confidence 高（`candidate.score`），或
   - feature inlier-ratio 高（來自 §4.3，PR C 後可用）。
   弱佐證的位移維持嚴格。

**Seam-band 細節（重要）：** Stage B 專盯底部 band，而 lazy-load 圖常正好在底部 → 對 band
做 naive tile-vote 可能看到 ~50% 壞 tile。故**主決策移到「全重疊區」的 tile-agreement**
（圖在整塊重疊區是明顯少數）；seam band 改為**次要、confidence-gated** 的 tile 檢查，而非
硬 mean gate。

**Config（新增到 `VerifierConfig`，全部給保守 default）：**

```rust
pub struct VerifierConfig {
    // 既有欄位保留（downsample_max_mad / full_res_max_mad / downsample_step / sample_band）
    pub robust_tile_px: u32,        // tile 邊長，default 48（tuning 對象）
    pub robust_tile_tol: f32,       // 單 tile 同意門檻 MAD，default ~ full_res_max_mad
    pub robust_accept_ratio: f32,   // 嚴格佐證時的同意比例下限，default 0.85
    pub robust_accept_ratio_floor: f32, // majority floor，default 0.6（任何情況不得低於此）
}
```

實際 `robust_tile_px / tol / accept_ratio` 在實作期對 golden 調參，本 spec 不釘死數值。

### 4.3 ②：Routine feature path + edge-index reuse

**現況**（`feature_matcher.rs`）：FAST corners + 8-D descriptor + 對稱 linear KNN +
dominant-translation vote，**只在 coarse/template/edge/relaxed 全失敗後**才跑（last-resort），
runtime 由 `FastHnswConfig.enabled = true` 控制；名稱裡的 "Hnsw" 是保留字，**目前是 linear
scan，沒有真 HNSW，也沒有任何 Cargo feature flag**。

**改法：**

1. **Routine（first-class candidate source）**：feature 比對改成**每幀都跑**，與
   coarse/template/edge 並列加入候選池，並**永遠**提供：
   - 一個 feature 候選位移，及
   - 一個 **inlier-ratio**，餵給 §4.2 的 confidence gate。
   （取代「只在其他全失敗才跑」的 last-resort 定位。）

2. **Edge-index reuse（效能關鍵，借鑑 snow-shot lazy rebuild）**：不每幀重抽錨點特徵。
   維護錨點邊緣區的 descriptor index，**只在錨點前進超過門檻時 lazy rebuild**（錨點只在
   append 時變）。每幀只對 curr 抽特徵 + 查 index。

3. **後端可換介面（3b = (B)）**：先用 **brute-force / SIMD KNN**（N ≤ 1200、8-D，
   每幀僅 ~1–2M 距離運算，多半 <1ms；roadmap §8.1 也承認線性在此 N 不一定糟）。把後端藏在
   介面後：

   ```rust
   enum FeatureIndexBackend { BruteForce, /* Hnsw 之後 drop-in */ }
   trait NearestDescriptors { fn knn(&self, q: &Descriptor8, k: usize) -> ...; }
   ```

   **HNSW graph 之後再 drop-in**（當 N 變大 / 4K frame 才有收益），不改上層。

4. **永久編譯、無 Cargo feature gate（明確覆蓋 roadmap §8.5/§13 的 default-off 建議）**：
   因為 ② 的魯棒性必須在 default build（真正的 app）裡生效。理由：① 的 confidence-gate 在
   lazy-load 幀要靠 feature inlier-ratio，且 routine feature 要夠快才可行——這需要 feature
   path（及其 reuse 加速）**永遠在場**。代價是 core 多一個常駐路徑；我們**不**引入 OpenCV 級
   重依賴（自家小 ANN / 輕量 crate）。

5. 既有 runtime gate（`min_keypoints` / `min_raw_matches` / `min_inliers` /
   `second_best_ratio`）保留為 config，必要時調整。

### 4.4 ③：Mid-capture re-anchor（擴張已 ship 的修法）

把 re-anchor 的觸發條件**從 `frame_count == 1` 擴張到任意位置**：連續
`REANCHOR_MISS_THRESHOLD` 次（目前 = 2，中段可另設獨立門檻 tuning）NoMatch → re-anchor 到
最新一幀。

- **⚠ 兩種 re-anchor 的語義不同，不能共用實作**：
  - **First-frame（已 ship）**：canvas 只有那張 stale 幀，故 `reanchor_to` 走
    `accept_first_frame`，**重建 canvas** 是對的（丟掉的本來就是垃圾）。
  - **Mid-capture（本 spec 新增）**：canvas 已有真實縫合內容，**絕不可**走
    `accept_first_frame`（那會清掉整張已縫好的圖）。中段 re-anchor 必須**保留既有 canvas**，
    只把 match anchor（`last_good`）重置成最新一幀、並重置 `last_motion` / 留下一段 logged
    gap，讓後續幀與新 anchor 比對接續。實作時必須區分這兩條路徑。
- **嚴格 last-resort**：只在 ①+② 都救不回時才到這（recovery ladder 見 §4.0）。①+② 做真正的
  工作並產生正確輸出；③ 只防止硬卡死。
- **有界例外**：這是對「NoMatch 不更新 anchor」invariant 的有意識、有界擴張（既有 first-frame
  例外的延伸）。
- **代價**：mid-capture re-anchor 會丟掉一段內容（gap/seam），必須 `log` 出來（roadmap
  「no silent caps」精神），不可靜默截斷。

---

## 5. 測試與驗收

### 5.1 Monotonicity 保證（自動化）

非動態 golden 序列輸出與 baseline **byte-identical**（接受條件是 mean ∨ tile-vote，
乾淨內容仍走 mean）。此為硬 gate。

### 5.2 新增 lazy-load golden（用 §2.3 manifest 既有的 `lazy_load_mutation` flag，
synthetic generator 在幀間 mutate 一塊區域）

- `lazy_load_first_frame`：第 0 幀佔位→載入；斷言**正確縫出**（重疊區被覆寫成載入內容），
  非只是不卡。
- `lazy_load_mid_capture`：好錨點與下一幀之間圖載入；斷言正確 append + 覆寫。
- `lazy_load_large_region`：變動區大到打敗 template；斷言 ②（feature）救回位移、① 驗證通過。
- `lazy_load_unrecoverable`：病態；斷言 ③ re-anchor（不卡死）、有 content-gap log，且
  **先前已縫好的 canvas 內容不被清掉**（驗證中段 re-anchor 保留 canvas，見 §4.4）。

### 5.3 誤匹配防線 golden（**不得 regression**，這是放寬 verify 的 gate）

- `repeated_grid` 仍被拒（放寬後不得開始接受 aliased 位移）。
- `low_texture` 不比 baseline 差。
- `sticky_header` 無 ghost regression。
- 合成的**全域錯位**位移仍被拒（majority-floor 證明）。

### 5.4 Benchmark（routine feature 每幀有成本）

- 乾淨幀 p50 維持在預算內 —— **edge-index reuse** 是讓 routine feature 可負擔的關鍵；量測
  `fallback_us` / 新增的 `feature_us` 與 total。
- 非動態序列 output diff vs golden = 0（自動化 monotonicity 檢查）。
- roadmap §14 檢查清單仍過，附本 spec 兩處有記錄的 invariant 變更（§3）。

### 5.5 TDD 順序

1. 誤匹配防線 golden 先寫（鎖住底線）。
2. lazy-load golden（RED）。
3. ① robust verifier（用 NCC confidence 即可讓 `lazy_load_first_frame` /
   `lazy_load_mid_capture` 通過；誤匹配 golden 維持綠；既有 golden monotonic）。
4. ② routine feature + edge-index reuse（讓 `lazy_load_large_region` 通過；提供 inlier-ratio
   豐富 ① 的 confidence gate）。
5. ③ mid-capture re-anchor（`lazy_load_unrecoverable`）。
6. benchmark 全程把關。

---

## 6. PR 切分

對應 roadmap §13 的 PR 9，但拆成可獨立驗證的步驟：

- **PR A — Goldens & fixtures**：誤匹配防線 golden + lazy-load golden（RED）+ benchmark
  序列（含 `lazy_load_mutation`）。不改 production。
- **PR B — ① Robust verifier**：mean ∨ confidence-gated tile-vote、majority floor、seam-band
  次要化。用 NCC confidence 驅動。過 `lazy_load_first_frame`/`mid_capture`；誤匹配 golden 綠；
  既有 golden monotonic（byte-identical）。
- **PR C — ② Routine feature + edge-index reuse**：feature 改 routine first-class、edge-index
  lazy rebuild、brute-force/SIMD KNN、後端介面化、**永久編譯（移除/不加 Cargo gate）**、
  inlier-ratio 餵入 ① 的 confidence gate。過 `lazy_load_large_region`。
- **PR D — ③ Mid-capture re-anchor**：擴張 `REANCHOR_MISS_THRESHOLD` 條件、recovery ladder、
  content-gap log。過 `lazy_load_unrecoverable`。
- **PR E（本 spec 範圍外，介面預留）— HNSW graph drop-in**：當 N / 4K frame 證明有收益時，
  在 `FeatureIndexBackend` 後換上 HNSW，不改上層。

---

## 7. 風險與未決

- **誤匹配 vs 魯棒性的 tile-vote 調參**：`accept_ratio` / `tile_tol` 太鬆會在 repeated_grid /
  低紋理開始誤接受。由 §5.3 golden 鎖死；majority floor 是硬下限。
- **routine feature 的每幀成本**：靠 edge-index reuse 壓低；若乾淨幀 p50 仍超預算，退而求其次
  改「borderline-only 觸發」（但已選 routine，先量再說）。
- **常駐 feature path 的依賴重量**：堅持自家小 ANN / 輕量 crate，不引 OpenCV。
- **③ 的 content-gap**：只在 ①+② 都失敗時；必須 log，不可靜默。
- **與其他 P 項的順序**：本 spec 不依賴 P1/P5；與 P3（fast NCC）共用 `VerifierConfig` 時注意
  欄位相容。

---

## 8. 與 roadmap P6 的差異摘要（給未來讀者）

| 面向 | roadmap §8（原 P6） | 本 spec（修訂後 P6） |
|---|---|---|
| 定位 | 純效能（降 fallback 延遲） | **效能 + 正確性（lazy-load 魯棒）** |
| feature path | last-resort fallback | **routine first-class candidate** |
| HNSW | default-off Cargo feature | **永久編譯、無 gate；HNSW graph 之後 drop-in** |
| verifier | feature 候選仍走嚴格 verifier，不動 verifier | **新增 confidence-gated tile-vote（mean ∨ tile-vote），保留 final-gate 精神** |
| re-anchor | 無 | **中段 re-anchor 安全網（擴張已 ship 的 first-frame 修法）** |

本 spec 落地後，roadmap §8 視為被本 spec 取代；roadmap 其餘 P 項不受影響。
