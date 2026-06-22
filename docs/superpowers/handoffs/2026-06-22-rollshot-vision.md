PR1 done — `rollshot-vision` crate exists, `RealAutomationHost` implements `AutomationHost` returning `capability_unavailable` for all four capabilities; `imageproc` is pinned in the workspace at 0.26 with default features disabled. Next: PR2 `VisualIndex` + `rect.rs`.

PR2 done — `VisualIndex::build` caches grayscale and rejects empty input; rectangle conversion uses floor-min/ceil-max, validates finite endpoints, and enforces empty/area rules. Next: PR3 template store.

PR3 done — checked raw-RGBA templates; bounded in-memory store; explicit local save/load and export records; corrupt local data is rejected; export strips `Sensitive` bytes; asset/store types have compile-time no-Serialize assertions. Next: PR4 prepared template matching.

PR4 done — template work is prepared outside QuickJS; callbacks only perform cached lookup/truncation. Matching validates low-information templates, equal-dimension imageproc edge cases, score finiteness, score-position/pixel-visit budgets, bounded candidate extraction, deterministic ordering, and NMS. Next: PR5 self-validation.

PR5 done — self-validation verifies source-location overlap, expected-count behavior, area/target-coverage gates, edge/entropy, false positives, and brightness plus ±1 px crop/padding stability. Next: PR6 integration matrix.

PR6 done — SP1 complete. Hand-authored role-free detectors run through explicit vision preparation + `QuickJsExecutor` + cached `RealAutomationHost` and produce expected proposals on deterministic synthetic fixtures. Blank, translation, known-scale-miss, determinism, capability error, privacy, and resource-bound cases are covered. Deferred: query-plan extraction/product wiring (SP6), regionFeatures (SP2), author acquisition (SP3), inspectLayout (SP4), OCR (SP5).

SP2 PR1 done — `region_features.rs`: deterministic `dominant_rgba` (quantized RGB histogram, bin-center output, lowest-bin tie-break) and `edge_density` (`|dx|+|dy|` over `(w-1)*(h-1)`, u64 accumulators, 0.0 for sub-2px rects). Pure functions, no host wiring. Next: PR2 prepare + callback.

SP2 PR2 done — `prepare_region_features` computes the single clipped-rect feature outside QuickJS and caches it under a canonical `RegionFeaturesKey{rect: PixelRect}`; the callback canonicalizes `query.region` via stored image dimensions and only looks up + truncates. Errors: `invalid_query` / `vision_index_unavailable` / `region_*` / `LimitExceeded`. `PixelRect` gained `Hash`. Next: PR3 integration.

SP2 PR3 done — SP2 complete. A role-free single-source `regionFeatures` detector (dynamic `input.imageWidth` top strip; the harness prepares the matching canonical rect) runs through `QuickJsExecutor` + prepared `RealAutomationHost`: flat strip → one candidate with clipped measured bounds, noisy strip → none, deterministic across runs. Deferred to later sub-projects: subregion splitting / RegionFeaturesV2 fields, manifest-gated full-image edge map, author inspectLayout (SP4), OCR (SP5), product/query-plan wiring (SP6).
