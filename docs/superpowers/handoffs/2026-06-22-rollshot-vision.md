PR1 done — `rollshot-vision` crate exists, `RealAutomationHost` implements `AutomationHost` returning `capability_unavailable` for all four capabilities; `imageproc` is pinned in the workspace at 0.26 with default features disabled. Next: PR2 `VisualIndex` + `rect.rs`.

PR2 done — `VisualIndex::build` caches grayscale and rejects empty input; rectangle conversion uses floor-min/ceil-max, validates finite endpoints, and enforces empty/area rules. Next: PR3 template store.
