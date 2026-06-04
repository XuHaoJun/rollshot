# Rollshot App Tauri Deprecation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `rollshot-app` the interactive product app, deprecate the retained Tauri app, and make macOS ScreenCaptureKit platform-default instead of feature-gated.

**Architecture:** Keep the Tauri crate buildable but remove it from active launch decisions. `rollshot-cli` resolves `rollshot-app` by default, `rollshot-app` directly runs the iced overlay, and `rollshot-capture` exposes the macOS ScreenCaptureKit backend on every macOS build. Shared launch JSON keeps `overlay_mode` for migration compatibility, but `rollshot-app` no longer uses it.

**Tech Stack:** Rust workspace, Cargo target-specific dependencies, existing unit tests, README documentation.

---

## File Map

- Modify `crates/rollshot-cli/src/cmd_capture_launcher.rs`: default interactive app name, build hint, tests.
- Modify `crates/rollshot-app/src/main.rs`: remove `overlay_selection` module use and run iced capture directly.
- Delete `crates/rollshot-app/src/overlay_selection.rs`: obsolete Tauri/Iced selector.
- Modify `crates/rollshot-capture/Cargo.toml`: make `scap` a normal macOS target dependency and remove `macos-sck`.
- Modify `crates/rollshot-capture/src/lib.rs`: expose macOS backend on all macOS builds.
- Modify `crates/rollshot-capture/src/backend.rs`: make macOS backend/default independent of `macos-sck`.
- Modify `crates/rollshot-capture/tests/macos_sck_smoke.rs`: gate only on macOS.
- Modify `crates/rollshot-cli/Cargo.toml`: remove `macos-sck` feature forwarding.
- Modify `crates/rollshot-cli/src/cmd_probe.rs`: remove `macos-sck` feature cfg gates.
- Modify `crates/rollshot-cli/tests/capture_stubs.rs`: update macOS cfg/expectations.
- Modify `crates/rollshot-app/Cargo.toml`: remove `macos-sck` feature forwarding.
- Modify `crates/rollshot-iced-overlay/Cargo.toml`: remove `macos-sck` feature forwarding.
- Modify `crates/rollshot-tauri-app/src-tauri/Cargo.toml`: remove obsolete `macos-sck` feature forwarding.
- Modify `crates/rollshot-tauri-app/src-tauri/tauri.macos.conf.json`: remove obsolete build feature list.
- Modify `README.md`: describe `rollshot-app` as active and `rollshot-tauri-app` as deprecated; update macOS commands.

---

### Task 1: CLI Defaults To `rollshot-app`

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture_launcher.rs`

- [ ] **Step 1: Write the failing test expectations**

In `crates/rollshot-cli/src/cmd_capture_launcher.rs`, update the existing tests so they expect the new app name and hint.

Replace the missing env path in `resolve_app_binary_env_missing_file`:

```rust
let env_path = PathBuf::from("/no/such/rollshot-app");
```

Replace the dev missing-app assertion:

```rust
assert!(err.message.contains("not found"), "{}", err.message);
assert!(err.message.contains("cargo build -p rollshot-app"), "{}", err.message);
assert!(err.message.contains("--headless"), "{}", err.message);
assert!(!err.message.contains("tauri"), "{}", err.message);
```

Replace the prod missing-app assertion:

```rust
assert!(err.message.contains("not found"), "{}", err.message);
assert!(err.message.contains("ROLLSHOT_APP"), "{}", err.message);
assert!(!err.message.contains("Tauri toolchain"), "{}", err.message);
```

- [ ] **Step 2: Run CLI launcher tests and verify RED**

Run:

```bash
rtk cargo test -p rollshot-cli cmd_capture_launcher
```

Expected: tests fail because production code still mentions `rollshot-tauri-app`, `tauri`, or the Tauri build hint.

- [ ] **Step 3: Implement the default app rename and hint**

In `resolve_app_binary_from_env_and_exe`, replace the empty override error with:

```rust
format!("{APP_ENV} is set but empty; expected path to rollshot-app")
```

Replace the dev hint with:

```rust
"hint: the GUI app must be built separately:\n  \
 cargo build -p rollshot-app\n\
 or use --headless to skip the GUI"
```

Replace `default_app_binary_name()` implementations:

```rust
#[cfg(windows)]
fn default_app_binary_name() -> &'static str {
    "rollshot-app.exe"
}

#[cfg(not(windows))]
fn default_app_binary_name() -> &'static str {
    "rollshot-app"
}
```

- [ ] **Step 4: Run CLI launcher tests and verify GREEN**

Run:

```bash
rtk cargo test -p rollshot-cli cmd_capture_launcher
```

Expected: PASS.

---

### Task 2: Remove Tauri Overlay Selection From `rollshot-app`

**Files:**
- Modify: `crates/rollshot-app/src/main.rs`
- Delete: `crates/rollshot-app/src/overlay_selection.rs`

- [ ] **Step 1: Write the failing compile expectation**

Remove `mod overlay_selection;` and the import:

```rust
use overlay_selection::{resolve_overlay_runner, OverlayRunner};
```

Change the capture branch in `main` to call `run_iced_capture(options)` directly:

```rust
LaunchMode::Capture(options) => {
    run_iced_capture(options);
}
```

Do not delete `overlay_selection.rs` yet.

- [ ] **Step 2: Run app tests and verify RED**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: compile fails if any stale `resolve_overlay_runner` or `OverlayRunner` references remain. If it passes immediately, continue because the next step is deleting obsolete code and confirming no test target depends on it.

- [ ] **Step 3: Delete obsolete selector module**

Delete `crates/rollshot-app/src/overlay_selection.rs`.

Ensure `crates/rollshot-app/src/main.rs` starts with only these local modules:

```rust
mod launch;
mod save;
```

Ensure `main` has this capture branch:

```rust
LaunchMode::Capture(options) => {
    run_iced_capture(options);
}
```

- [ ] **Step 4: Run app tests and verify GREEN**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: PASS.

---

### Task 3: Make macOS ScreenCaptureKit Platform-Default In `rollshot-capture`

**Files:**
- Modify: `crates/rollshot-capture/Cargo.toml`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-capture/src/backend.rs`
- Modify: `crates/rollshot-capture/tests/macos_sck_smoke.rs`

- [ ] **Step 1: Write failing backend expectations**

In `crates/rollshot-capture/src/backend.rs`, update the `default_backend_for_decision_matrix` test so macOS always expects ScreenCaptureKit:

```rust
assert_eq!(
    default_backend_for("macos", None),
    BackendKind::MacosScreenCaptureKit
);
assert_eq!(
    default_backend_for("macos", Some("wayland")),
    BackendKind::MacosScreenCaptureKit
);
```

In `crates/rollshot-capture/tests/macos_sck_smoke.rs`, replace the file cfg with:

```rust
#![cfg(target_os = "macos")]
```

- [ ] **Step 2: Run capture tests and verify RED**

Run:

```bash
rtk cargo test -p rollshot-capture
```

Expected on non-macOS: may still pass because cfg-dependent branches are not compiled. Expected on macOS before implementation: backend/default assertions fail or compile gates reject `MacosScreenCaptureKitBackend` without the feature.

- [ ] **Step 3: Remove the feature gate from Cargo dependency wiring**

In `crates/rollshot-capture/Cargo.toml`, remove:

```toml
[features]
default = []
macos-sck = ["dep:scap"]
```

Replace the macOS target dependency:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
scap = { workspace = true }
```

- [ ] **Step 4: Remove feature gates from exports**

In `crates/rollshot-capture/src/lib.rs`, replace:

```rust
#[cfg(all(target_os = "macos", feature = "macos-sck"))]
pub mod macos;
```

with:

```rust
#[cfg(target_os = "macos")]
pub mod macos;
```

Replace:

```rust
#[cfg(all(target_os = "macos", feature = "macos-sck"))]
pub use macos::MacosScreenCaptureKitBackend;
```

with:

```rust
#[cfg(target_os = "macos")]
pub use macos::MacosScreenCaptureKitBackend;
```

- [ ] **Step 5: Remove feature gates from backend creation/default**

In `BackendKind::create`, replace the `MacosScreenCaptureKit` match arm with:

```rust
BackendKind::MacosScreenCaptureKit => {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(crate::macos::MacosScreenCaptureKitBackend::new()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(CaptureError::Unsupported {
            message: "macos-sck backend requires a macOS host".to_string(),
        })
    }
}
```

In `default_backend_for`, replace the macOS arm with:

```rust
"macos" => BackendKind::MacosScreenCaptureKit,
```

- [ ] **Step 6: Run capture tests and verify GREEN**

Run:

```bash
rtk cargo test -p rollshot-capture
```

Expected: PASS.

---

### Task 4: Remove Obsolete `macos-sck` Feature Forwarding

**Files:**
- Modify: `crates/rollshot-cli/Cargo.toml`
- Modify: `crates/rollshot-cli/src/cmd_probe.rs`
- Modify: `crates/rollshot-cli/tests/capture_stubs.rs`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Modify: `crates/rollshot-tauri-app/src-tauri/Cargo.toml`
- Modify: `crates/rollshot-tauri-app/src-tauri/tauri.macos.conf.json`

- [ ] **Step 1: Search feature references**

Run:

```bash
rtk rg -n "macos-sck|feature = \"macos-sck\"|features macos-sck" Cargo.toml crates README.md docs -g '!docs/superpowers/**'
```

Expected: shows current feature forwarding and cfg references.

- [ ] **Step 2: Remove crate feature sections that only forward macOS SCK**

In each of these files, delete the listed feature block:

`crates/rollshot-cli/Cargo.toml`:

```toml
[features]
default = []
macos-sck = ["rollshot-capture/macos-sck"]
```

`crates/rollshot-app/Cargo.toml`:

```toml
[features]
default = []
macos-sck = [
  "rollshot-capture/macos-sck",
  "rollshot-iced-overlay/macos-sck",
]
```

`crates/rollshot-iced-overlay/Cargo.toml`:

```toml
[features]
default = []
macos-sck = ["rollshot-capture/macos-sck"]
```

`crates/rollshot-tauri-app/src-tauri/Cargo.toml`:

```toml
[features]
default = []
macos-sck = ["rollshot-capture/macos-sck"]
```

- [ ] **Step 3: Remove Tauri macOS feature config**

In `crates/rollshot-tauri-app/src-tauri/tauri.macos.conf.json`, remove the `features` list that contains `macos-sck`.

The file should no longer mention `macos-sck`.

- [ ] **Step 4: Remove cfg gates in CLI probe**

In `crates/rollshot-cli/src/cmd_probe.rs`, replace:

```rust
#[cfg(any(target_os = "linux", all(target_os = "macos", feature = "macos-sck")))]
```

with:

```rust
#[cfg(any(target_os = "linux", target_os = "macos"))]
```

Replace any:

```rust
#[cfg(all(target_os = "macos", feature = "macos-sck"))]
```

with:

```rust
#[cfg(target_os = "macos")]
```

- [ ] **Step 5: Update CLI capture stub cfg expectations**

In `crates/rollshot-cli/tests/capture_stubs.rs`, replace:

```rust
#[cfg(all(target_os = "macos", feature = "macos-sck"))]
```

with:

```rust
#[cfg(target_os = "macos")]
```

Replace:

```rust
let expected_code = if cfg!(all(target_os = "macos", feature = "macos-sck")) {
```

with:

```rust
let expected_code = if cfg!(target_os = "macos") {
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-cli
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-iced-overlay
```

Expected: PASS.

- [ ] **Step 7: Confirm no obsolete feature references remain in active code**

Run:

```bash
rtk rg -n "feature = \"macos-sck\"|rollshot-capture/macos-sck|features macos-sck|--features macos-sck" Cargo.toml crates README.md docs -g '!docs/superpowers/**'
```

Expected: only historical or issue docs outside active code may remain. README should be updated in Task 5. No Cargo files should reference `macos-sck`.

---

### Task 5: Document Tauri Deprecation And New macOS Commands

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README workspace descriptions**

Replace the workspace app bullets with:

```markdown
- `crates/rollshot-app`: iced interactive capture app
- `crates/rollshot-tauri-app`: deprecated Tauri v2 app retained temporarily as
  legacy/reference code during the iced migration
```

Replace the "Desktop app crates during iced migration" section so it states:

```markdown
- `rollshot-app` is the active iced product app for interactive capture.
- `rollshot-tauri-app` is deprecated and retained temporarily as legacy/reference
  code. It is no longer the default product launch path.
- `rollshot-iced-overlay` is the iced overlay renderer used by the active app.
```

- [ ] **Step 2: Update Tauri dependency wording**

In the Tauri app section, add one sentence near the top:

```markdown
This crate is deprecated and retained temporarily for reference during the iced
migration.
```

Replace wording that says Tauri packages are required for the app path with wording that says they are required only to build the deprecated Tauri crate.

- [ ] **Step 3: Update macOS manual commands**

In the macOS ScreenCaptureKit manual checklist, remove `--no-default-features --features macos-sck` from active commands.

Use these command forms:

```bash
cargo run -p rollshot-cli -- probe --json
cargo run -p rollshot-cli -- capture --backend macos-sck --region full --max-frames 3 --output target/test-artifacts/macos_full.png
cargo run -p rollshot-cli -- capture --backend macos-sck --region "0,0 320x240" --max-frames 3 --output target/test-artifacts/macos_region.png
cargo run -p rollshot-cli -- capture --backend macos-sck --region "0,0 320x240" --max-frames 3 --dump-frames target/test-artifacts/macos_frames --output target/test-artifacts/macos_region_stitched.png
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

- [ ] **Step 4: Search README for stale Tauri/default feature wording**

Run:

```bash
rtk rg -n "retained Tauri|fallback|--features macos-sck|Tauri toolchain|rollshot-tauri-app" README.md
```

Expected: `rollshot-tauri-app` mentions describe deprecated legacy/reference status. No README command uses `--features macos-sck`.

---

### Task 6: Final Verification

**Files:**
- All files touched by Tasks 1-5

- [ ] **Step 1: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 2: Run focused test suite**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-cli
rtk cargo test -p rollshot-capture
rtk cargo test -p rollshot-iced-overlay
```

Expected: PASS.

- [ ] **Step 3: Run full stale-reference scan**

Run:

```bash
rtk rg -n "rollshot-tauri-app|macos-sck|overlay_mode|OverlayRunner|resolve_overlay_runner" Cargo.toml crates README.md docs -g '!docs/superpowers/**'
```

Expected:

- `rollshot-tauri-app` remains in workspace membership, its own crate files, and deprecated README text.
- `macos-sck` remains as a backend name, command value, smoke-test name, and historical issue-doc term.
- `overlay_mode` remains only in shared migration JSON/types and retained Tauri code.
- `OverlayRunner` and `resolve_overlay_runner` do not remain.

- [ ] **Step 4: Run clippy if code changes are broader than expected**

Run if edits touch more than the files listed in the File Map:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Summarize macOS runtime risk**

In the final response, state that compile-time/unit verification passed locally, and that real macOS capture still requires manual interactive smoke testing for Screen Recording permission, ScreenCaptureKit capture, iced focus, and passthrough behavior.
