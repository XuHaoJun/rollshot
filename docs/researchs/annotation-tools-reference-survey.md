# Annotation tools reference survey

## 摘要

本文件盤點 Snow Shot、Flameshot、mark-shot 與 KDE Spectacle 的 annotation／markup 功能，並對照 Rollshot 目前能力，作為後續 annotation 規格的研究輸入。調查以各 reference project 的本地 checkout 程式碼為準，未執行 GUI；「未見」表示在本次靜態調查範圍內沒有找到實作證據，不等同經 runtime 證明不存在。

四個專案共同呈現一組成熟截圖編輯器的核心能力：可設定顏色與粗細、自由筆、線／箭頭、矩形／橢圓、文字、選取既有物件、刪除與 undo/redo。Highlighter、number、pixelate/blur 也高度常見。Rollshot 已有良好的非破壞式 document、選取、歷史與安全 flatten 基礎，但目前只有 Number、Text、Opaque Redaction 三種 annotation，且外觀完全由固定常數決定。

建議先完善既有工具的 style model 與屬性編輯，再加入 Arrow、Rectangle、Ellipse、Pen、Highlighter、Line、Pixelate。多選、旋轉、圖層與 magnifier 等進階能力應後置，避免在資料模型與基本 shape editing 尚未穩定前擴大互動複雜度。

## 調查基準

調查時間均為 Asia/Taipei（UTC+08:00）。

| Project | Git commit | Commit time | Commit subject | 調查時間 |
|---|---|---|---|---|
| Rollshot（現況對照） | `ae7714a2a22934777e1a817b825b7de00c78da37` | `2026-07-12T12:26:24+08:00` | `chore: bump learn-projects` | `2026-07-12T12:32:07+08:00` |
| Snow Shot | `c7f2d9fe3114ad0dba6e5efdfe4bd8ecbc1f1de3` | `2025-11-08T01:35:56+08:00` | `feat: 编译离线版本 (#792)` | `2026-07-12T12:29:49+08:00` |
| Flameshot | `bd2e6d3a0ee665470bd05f614f5087a36c076cfc` | `2026-07-11T23:09:19+03:00` | `Translated using Weblate (Bulgarian) (#4816)` | `2026-07-12T12:29:58+08:00` |
| mark-shot | `7b4878ac85f1229766314444fe4de0b9b3fb8d1b` | `2026-07-10T07:29:43+08:00` | `release: prepare v0.1.39` | `2026-07-12T12:31:06+08:00` |
| KDE Spectacle | `e70545fc4931d715a189800f0cecee4a61ce4a54` | `2026-07-11T01:50:51Z` | `GIT_SILENT Sync po/docbooks with svn` | `2026-07-12T12:30:01+08:00` |

## Rollshot 現況與缺口

Rollshot 的 `Annotation` graph 只有 `NumberCallout`、`TextNote`、`OpaqueRedaction` 三種 variant；tool bar 對應 Select、Number、Text、Redact，OCR 則由 off-by-default feature 提供。現有 Select 支援既有物件移動、redaction resize、刪除，並有 undo/redo、annotation navigator，以及 Copy/Save 時 full-resolution flatten。

主要缺口不是只有「缺新工具」，而是 annotation model 沒有 style data。`style.rs` 明確指出 UI 不提供 style controls，Number 的紅色、Text 的白字黑底、Redaction 的純黑都由全域常數決定。因此若直接新增 shapes 而不先建立 style model，之後每個工具都會重複碰資料模型、history、hit testing、live preview 與 flatten rendering。

證據：

- Annotation variants：`crates/rollshot-image-document/src/annotation.rs:11-31`
- 固定 style 與「UI exposes no style controls」：`crates/rollshot-image-document/src/style.rs:1-41`
- 產品工具列：`crates/rollshot-app/src/result_workspace/view.rs:58-105`
- 工具與 drag/edit state：`crates/rollshot-app/src/result_workspace/canvas.rs:24-69`
- Undo、redo、delete：`crates/rollshot-app/src/result_workspace/update.rs:790-817`
- 鍵盤快捷鍵：`crates/rollshot-app/src/result_workspace/update.rs:1715-1750`
- Copy/Save flatten boundary：`crates/rollshot-image-document/src/document.rs:110-139`

## 跨專案功能矩陣

符號：✓ = checkout 中有明確實作證據；△ = 相近但語意不同或僅部分確認；— = 本次未見。

| Feature | Snow Shot | Flameshot | mark-shot | Spectacle | Rollshot 現況 |
|---|:---:|:---:|:---:|:---:|:---:|
| Select / move existing annotation | ✓ | ✓ | ✓ | ✓ | ✓ |
| Color control | ✓ | ✓ | ✓ | ✓ | — |
| Stroke / size control | ✓ | ✓ | ✓ | ✓ | — |
| Pen / freehand | ✓ | ✓ | ✓ | ✓ | — |
| Line | ✓ | ✓ | ✓ | ✓ | — |
| Arrow | ✓ | ✓ | ✓ | ✓ | — |
| Rectangle | ✓ | ✓ | ✓ | ✓ | — |
| Ellipse / circle | ✓ | ✓ | ✓ | ✓ | — |
| Highlighter / marker | ✓ | ✓ | ✓ | ✓ | — |
| Text | ✓ | ✓ | ✓ | ✓ | ✓ |
| Number / counter | ✓ | ✓ | ✓ | ✓ | ✓ |
| Pixelate / mosaic | ✓ | ✓ | ✓ | ✓ | — |
| Blur | ✓ | △ insecure legacy mode | — | ✓ | — |
| Opaque secure redaction | △ mask/highlight | — | — | — | ✓ |
| Invert | △ filter | ✓ | ✓ rectangle mode | — | — |
| Magnifier | — | — | ✓ | — | — |
| Watermark | ✓ | — | — | — | — |
| Crop in annotation workspace | capture crop | capture selection | capture selection | ✓ | — |
| Fill control | ✓ | △ rectangle is solid | ✓ | ✓ | — |
| Font family/style control | △ external Excalidraw | ✓ | ✓ | ✓ | — |
| Opacity control | ✓ | △ marker fixed | ✓ | via color alpha | — |
| Multi-select | Excalidraw-dependent | — | ✓ | external component | — |
| Rotate annotation | Excalidraw-dependent | — | ✓ | external component | — |
| Layer reorder | 未完整確認 | ✓ | — | external component | — |
| Undo / redo | ✓ | ✓ | ✓ | ✓ | ✓ |
| Delete selected | ✓ | ✓ | ✓ | external component | ✓ |
| Editable project format | — | — | — | — | — |
| Flattened Copy / Save | ✓ | ✓ | ✓ | ✓ | ✓ |

## Reference project findings

### Snow Shot

Snow Shot 以自訂 Excalidraw 加 PIXI image layers 組成最廣的工具集合：rectangle、diamond、ellipse、arrow、line、freehand、text、number、region/freehand filters、eraser、watermark 與 highlight/mask。它也提供完整 color picker、stroke/fill palettes、線寬、字級、opacity、filter strength 與多種 filter 類型。

值得 Rollshot 借鏡的是「同一屬性面板同時編輯 tool defaults 與 selected object」以及 tool lock 連續繪製。濾鏡除了 blur/pixelate，還包含 grayscale、negative、noise 等大量效果；這些不是 annotation MVP 的必要範圍。

主要證據：

- `DrawState` 完整狀態：`learn-projects/snow-shot/src/types/draw.ts:1-72`
- Tool mapping：`learn-projects/snow-shot/src/components/drawCore/index.tsx:95-127`
- Toolbar composition：`learn-projects/snow-shot/src/pages/draw/components/drawToolbar/index.tsx:760-884`
- Stroke/fill palettes：`learn-projects/snow-shot/src/components/drawCore/excalidrawRenders/index.tsx:33-83`
- Line width / font size sliders：`learn-projects/snow-shot/src/components/drawCore/index.tsx:379-466`
- Filter types：`learn-projects/snow-shot/src/components/drawCore/excalidrawRenders/radioSelection.tsx:288-375`
- Highlight/mask style：`learn-projects/snow-shot/src/pages/draw/components/drawToolbar/components/tools/highlightTool.tsx:24-43,84-173`
- Undo/redo：`learn-projects/snow-shot/src/core/canvas/canvasHistory.ts:35-167`
- Flatten、copy 與 save：`learn-projects/snow-shot/src/pages/draw/actions.ts:39-246,360-511`

限制：`package.json` 的 `@mg-chao/excalidraw` 指向 checkout 外的 sibling repository，且 Snow Shot 關閉上游 key events/context menu。因此 font family、fill/stroke style、arrowhead、layer actions，以及 annotation object copy/paste 不能只由此 checkout 完整確認。

### Flameshot

Flameshot 有 pencil、line、arrow、rectangle、circle、marker、text、circle counter、pixelate 與 invert。它的屬性系統較精簡：共用 active color、每類工具記憶 size，文字另有 font family、粗斜體、底線、刪除線與 alignment。Rectangle 是同色實心圓角矩形，Marker 的 opacity 固定為 0.35，沒有通用 fill/stroke 分離。

它的既有物件可選取、移動、改 color/size；Text 可雙擊重編。Layers panel 支援選取、刪除與上下重排，但本次未找到一般 annotation resize handles 或 rotate。

主要證據：

- Tool types/factory：`learn-projects/flameshot/src/tools/capturetool.h:25-54`、`learn-projects/flameshot/src/tools/toolfactory.cpp:35-71`
- Color picker 與 selected-object recolor：`learn-projects/flameshot/src/widgets/capture/capturewidget.cpp:795-813,876-896,1031-1041`
- Size control：`learn-projects/flameshot/src/widgets/panel/sidepanelwidget.cpp:33-59`
- Text formatting：`learn-projects/flameshot/src/tools/text/textconfig.cpp:15-113`
- Selected-object move/style edits：`learn-projects/flameshot/src/widgets/capture/capturewidget.cpp:973-1003,1507-1559`
- Layers：`learn-projects/flameshot/src/widgets/panel/utilitypanel.cpp:151-188,225-264`
- Flatten/export：`learn-projects/flameshot/src/widgets/capture/capturewidget.cpp:317-329,497-500`

### mark-shot

mark-shot 的 13 個工具為 Move、Select、Pen、Line、Highlighter、Rectangle、Ellipse、Arrow、Text、Number、Mosaic、Magnifier、Laser。它在 shape editing 上最完整：框選多個 annotation、群組 move/resize、多數 annotation rotate；line/arrow 可編輯端點與骨架點。

屬性包括 color/alpha、各工具獨立 size、shape fill、rectangle corner radius/style、四種 arrow style、freehand/line highlighter、七種 number style、font/background color，以及 magnifier shape/zoom。Laser 是 1.8 秒後消失且不進 undo history 的暫態效果，不適合直接併入 Rollshot 的非破壞式 annotation graph。

主要證據：

- Tool enum/list：`learn-projects/mark-shot/src/shot_window.h:60-107`、`learn-projects/mark-shot/src/shot_window_setup.cpp:10-71,422-434`
- Property panels：`learn-projects/mark-shot/src/shot_window_setup.cpp:197-358`
- Persistent defaults：`learn-projects/mark-shot/src/annotation_state_store.h:10-57`
- Multi-select/group editing：`learn-projects/mark-shot/src/shot_window_canvas.cpp:618-651`、`learn-projects/mark-shot/src/shot_window_annotation_editing.cpp:253-278`
- Rotation and control points：`learn-projects/mark-shot/src/shot_window_hit_testing.cpp:220-247`
- Undo/redo：`learn-projects/mark-shot/src/shot_window_annotation_editing.cpp:303-383`
- Flatten/save/copy：`learn-projects/mark-shot/src/shot_window_actions.cpp:276-445`

### KDE Spectacle

Spectacle 提供 crop、select、freehand、highlighter、line、arrow、rectangle、ellipse、pixelate、blur、text 與 number。Freehand/Highlighter 可用 Shift snap 成直線；Line/Arrow 可 snap 至 45°；Rectangle/Ellipse/Pixelate/Blur 支援維持比例與從中心 resize。

它的 options toolbar 會依 tool/selection capability 顯示 stroke on/off、stroke width 0/1–99 px、stroke color、fill on/off/color、effect strength、font family/style/size/color、number 值與 shadow。Selected item 的屬性修改會被 commit 成 undoable edit。

主要證據：

- Tool toolbar 與 modifier hints：`learn-projects/spectacle/src/Gui/AnnotationsToolBarContents.qml:117-214`
- Stroke/fill options：`learn-projects/spectacle/src/Gui/AnnotationOptionsToolBarContents.qml:46-198`
- Effect strength：`learn-projects/spectacle/src/Gui/AnnotationOptionsToolBarContents.qml:200-244`
- Font options：`learn-projects/spectacle/src/Gui/AnnotationOptionsToolBarContents.qml:246-310`
- Number/shadow：`learn-projects/spectacle/src/Gui/AnnotationOptionsToolBarContents.qml:312-398`
- Undo/redo shortcuts：`learn-projects/spectacle/src/Gui/AnnotationEditor.qml:42-51`
- Save/copy flatten sync：`learn-projects/spectacle/src/Gui/SpectacleWindow.cpp:231-269`

限制：annotation engine 來自 `org.kde.kquickimageeditor`，其完整 C++ 實作不在本 checkout。本文只把 Spectacle checkout 明確暴露的 toolbar、options 與 integration 行為列為確定證據；multi-select、rotate、delete 等 engine 細節不據此推定。

## 建議的 Rollshot annotation requirements

### P0：先完善既有 annotation

1. **建立 per-annotation style data，而非 UI-only defaults。** 至少涵蓋 primary color、secondary/fill color、stroke width、opacity；Text 另有 font size/style、text/background color；Number 另有 bubble/leader/text color。Style 必須同時驅動 live preview 與 full-resolution flatten，並進入 undo/redo snapshot。
2. **屬性面板同時服務 active tool defaults 與 selected annotation。** 無選取時修改下次建立的預設；有選取時修改既有物件並形成單一語意化 undo entry。這是 Snow Shot、Flameshot、Spectacle 共同模式。
3. **保留安全 redaction 的明確語意。** `OpaqueRedaction` 必須維持不透明、flatten 後不可逆且不受一般 opacity 控制；若加入 pixelate/blur，UI 與型別不得把它們稱為 secure redaction。
4. **完善 Text 與 Number。** Text 至少可調字級、文字色與背景色，能重編既有文字；Number 至少可調 accent color、size，並能重設／指定下一個序號。Font family/完整 typography 可後置，以控制跨平台字型重現風險。
5. **Style persistence。** 記住最近使用的 tool defaults；選取物件的 style 不應偷偷覆蓋其他工具 defaults，除非使用者明確要求「套用為預設」。

### P1：新增核心 drawing tools

1. **Arrow**：最高價值的新工具；支援 color、width、端點 drag，Shift snap 至 45°。第一版只需單一清晰 arrowhead，不先做多種 arrow style。
2. **Rectangle + Ellipse**：共用 shape geometry/style；支援 stroke、optional fill、resize handles、Shift 維持比例。Rectangle corner radius 後置。
3. **Pen / Freehand**：支援 color、width、pointer sampling 與合理 path simplification；一次 stroke 對應一筆 history transaction。
4. **Highlighter**：可與 Pen 共用 path geometry，但有獨立預設 width/opacity 與清楚的 alpha compositing；不需要第一版就提供多種 blend modes。
5. **Line**：可與 Arrow 共用 two-point geometry，只有 arrowhead 不同；同樣支援 45° snap。
6. **Pixelate / Mosaic**：作為非安全視覺遮蔽工具，保留 effect strength/block size。Blur 可在效能、跨 preview/flatten 一致性與隱私文案驗證後再加入。

### P2：精修能力

- 框選多個 annotation、群組 move/resize。
- Rotate handles；只在 shapes/path/text 的 bounds 與 hit testing 穩定後加入。
- Layer reorder（forward/backward）與可選的 layers/navigator integration。
- Shape 特有 style：corner radius、arrowhead variants、dash/stroke style、number formats。
- Magnifier。它對教學截圖有價值，但資料模型、source sampling、長圖效能與 export fidelity 成本均高於基本 shapes。
- Export frame effects、watermark、invert、laser 等屬於相鄰功能，不納入 annotation 核心第一階段。

## 必要的跨層驗收條件

每一種新增 annotation 或 style 必須同時滿足：

1. `rollshot-image-document` 有 framework-neutral data model、bounds、hit testing、edit operation 與 deterministic flatten。
2. iced canvas 的 live preview 與 flattened PNG 在 full-resolution image coordinates 下視覺一致。
3. Create、move、resize、style edit、delete 都有清楚的單次 undo/redo 語意；drag preview 不應產生大量 history entries。
4. 長截圖在縮放顯示 copy 上編輯時，geometry、stroke size 與輸出仍以原圖座標正確還原。
5. Opaque redaction、pixelate、blur 的 UI 名稱與安全承諾不混淆；Copy/Save 的安全 flatten 行為不得退化。
6. Linux 與 macOS 使用同一 Result Workspace annotation 行為；若之後把 annotation 移入 capture overlay，必須另行檢查兩條平台 overlay path。

## 調查限制

- 本次為靜態原始碼調查，沒有建置或 runtime UI 驗證。
- Reference projects 的 checkout 可能不是正式 release tag；上表 commit 是本次可重現基準。
- Snow Shot 的 custom Excalidraw 與 Spectacle 的 KQuickImageEditor 部分實作不在各自 checkout，因此相關能力只採用整合層能直接證實的部分。
- Copy 在四個 reference projects 中都明確表示「複製 flatten 後的完成圖片」，不是 annotation object copy。本文未把 object copy/paste 列為共同需求。
- 本文件是研究快照，不是已核准的產品 spec；實作前仍應決定 P0/P1 的精確 UX、資料模型與分階段範圍。
