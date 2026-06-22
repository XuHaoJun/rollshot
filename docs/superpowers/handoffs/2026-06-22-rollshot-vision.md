PR1 done — `rollshot-vision` crate exists, `RealAutomationHost` implements `AutomationHost` returning `capability_unavailable` for all four capabilities; `imageproc` is pinned in the workspace at 0.26 with default features disabled. Next: PR2 `VisualIndex` + `rect.rs`.

PR2 done — `VisualIndex::build` caches grayscale and rejects empty input; rectangle conversion uses floor-min/ceil-max, validates finite endpoints, and enforces empty/area rules. Next: PR3 template store.

PR3 done — checked raw-RGBA templates; bounded in-memory store; explicit local save/load and export records; corrupt local data is rejected; export strips `Sensitive` bytes; asset/store types have compile-time no-Serialize assertions. Next: PR4 prepared template matching.

PR4 done — template work is prepared outside QuickJS; callbacks only perform cached lookup/truncation. Matching validates low-information templates, equal-dimension imageproc edge cases, score finiteness, score-position/pixel-visit budgets, bounded candidate extraction, deterministic ordering, and NMS. Next: PR5 self-validation.
