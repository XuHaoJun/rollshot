# Post-Capture Viewer Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix secondary-display thumbnail placement, oversized thumbnail GPU uploads, and Linux file reveal behavior found during review of the post-capture image viewer.

**Architecture:** Keep platform integrations thin and move decision-making into portable helpers. Reuse the Result Workspace display-downscale implementation for the macOS thumbnail, calculate macOS coordinates through a pure helper, and isolate Linux D-Bus reveal/fallback selection behind an injected-operation helper.

**Tech Stack:** Rust, iced 0.14, objc2/AppKit, zbus 4.4 blocking API, url 2, Cargo tests

---

## File Map

- Modify `crates/rollshot-app/src/macos_native_drag.rs`
  - Add portable active-screen-to-winit coordinate conversion.
  - Query the main screen height in the macOS adapter.
- Modify `crates/rollshot-app/src/result_workspace/mod.rs`
  - Expose the existing display-handle builder within `rollshot-app`.
- Modify `crates/rollshot-app/src/macos_product.rs`
  - Build the floating-thumbnail handle from a bounded display copy.
- Modify `crates/rollshot-app/src/result_workspace/actions.rs`
  - Add Linux FileManager1 reveal and fallback selection.
- Modify `crates/rollshot-app/Cargo.toml`
  - Add Linux-only `url` and `zbus` workspace dependencies.
- Modify `Cargo.lock`
  - Record dependency-edge changes.

### Task 1: Correct macOS Secondary-Display Thumbnail Coordinates

**Files:**
- Modify: `crates/rollshot-app/src/macos_native_drag.rs:62-78`
- Modify: `crates/rollshot-app/src/macos_native_drag.rs:129-189`
- Test: `crates/rollshot-app/src/macos_native_drag.rs:389-423`

- [ ] **Step 1: Write failing portable coordinate-conversion tests**

Replace the tests that call `thumbnail_origin` with tests for a new helper:

```rust
#[test]
fn active_screen_origin_on_primary_display() {
    assert_eq!(
        active_screen_thumbnail_origin_in_main_space(
            ScreenFrame::new(0.0, 0.0, 1920.0, 1080.0),
            1080.0,
            Size::new(300.0, 200.0),
            16.0,
        ),
        Point::new(1604.0, 864.0)
    );
}

#[test]
fn active_screen_origin_on_secondary_display_left_of_primary() {
    assert_eq!(
        active_screen_thumbnail_origin_in_main_space(
            ScreenFrame::new(-1440.0, 0.0, 1440.0, 900.0),
            1080.0,
            Size::new(280.0, 220.0),
            24.0,
        ),
        Point::new(-304.0, 836.0)
    );
}

#[test]
fn active_screen_origin_on_secondary_display_right_of_primary() {
    assert_eq!(
        active_screen_thumbnail_origin_in_main_space(
            ScreenFrame::new(1920.0, 0.0, 1600.0, 900.0),
            1080.0,
            Size::new(300.0, 200.0),
            16.0,
        ),
        Point::new(3204.0, 864.0)
    );
}

#[test]
fn active_screen_origin_on_secondary_display_above_primary() {
    assert_eq!(
        active_screen_thumbnail_origin_in_main_space(
            ScreenFrame::new(0.0, 1080.0, 1920.0, 1200.0),
            1080.0,
            Size::new(300.0, 200.0),
            16.0,
        ),
        Point::new(1604.0, -216.0)
    );
}

#[test]
fn active_screen_origin_on_secondary_display_below_primary() {
    assert_eq!(
        active_screen_thumbnail_origin_in_main_space(
            ScreenFrame::new(0.0, -900.0, 1600.0, 900.0),
            1080.0,
            Size::new(300.0, 200.0),
            16.0,
        ),
        Point::new(1284.0, 1764.0)
    );
}
```

The expected Y coordinate is:

```rust
main_display_height - (frame.y + margin + size.height)
```

This is the winit top-left position that converts back to the desired AppKit
bottom-left origin on any display.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
rtk cargo test -p rollshot-app active_screen_origin
```

Expected: compilation fails because
`active_screen_thumbnail_origin_in_main_space` does not exist.

- [ ] **Step 3: Implement the pure coordinate helper**

Replace `thumbnail_origin` with:

```rust
pub fn active_screen_thumbnail_origin_in_main_space(
    frame: ScreenFrame,
    main_display_height: f32,
    size: Size,
    margin: f32,
) -> Point {
    Point::new(
        frame.x + frame.width - size.width - margin,
        main_display_height - frame.y - size.height - margin,
    )
}
```

Document that the returned point is in the main-display top-left coordinate
space consumed by winit.

- [ ] **Step 4: Update the macOS adapter**

In `active_screen_thumbnail_origin_impl`:

1. Query `NSScreen::mainScreen(mtm)`.
2. Read `main_screen.frame().size.height`.
3. Keep the existing active-screen selection.
4. Return `active_screen_thumbnail_origin_in_main_space(frame, main_height, size, margin)`.
5. Remove the documented multi-display limitation.

Return a descriptive error if `NSScreen::mainScreen` is unavailable.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run:

```bash
rtk cargo test -p rollshot-app active_screen_origin
```

Expected: all coordinate tests pass.

### Task 2: Bound the Floating-Thumbnail GPU Texture

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs:228-238`
- Modify: `crates/rollshot-app/src/macos_product.rs:120-136`
- Test: `crates/rollshot-app/src/macos_product.rs:612-619`

- [ ] **Step 1: Write a failing oversized-thumbnail test**

Add this test to `macos_product.rs`:

```rust
#[test]
fn completed_oversized_capture_downscales_thumbnail_handle_only() {
    let image = RgbaImage::from_pixel(100, 9000, image::Rgba([10, 20, 30, 255]));
    let mut product = product_in_capture_phase();

    product.apply_capture_completion(image, Ok(PathBuf::from("/tmp/cap.png")));

    let Phase::Thumbnail(state) = &product.phase else {
        panic!("expected thumbnail phase");
    };
    let (width, height) = match state.image_handle.clone() {
        iced::widget::image::Handle::Rgba { width, height, .. } => (width, height),
        _ => panic!("expected rgba thumbnail handle"),
    };
    assert!(width <= crate::result_workspace::viewport::DEFAULT_MAX_TEXTURE_DIM);
    assert!(height <= crate::result_workspace::viewport::DEFAULT_MAX_TEXTURE_DIM);
    assert_eq!(
        product
            .document
            .as_ref()
            .expect("saved document")
            .source_image
            .dimensions(),
        (100, 9000)
    );
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
rtk cargo test -p rollshot-app completed_oversized_capture_downscales_thumbnail_handle_only
```

Expected: FAIL because the thumbnail handle height remains `9000`.

- [ ] **Step 3: Expose the existing display-handle helper**

Change the Result Workspace helper signature to:

```rust
pub(crate) fn build_display_handle(source: &RgbaImage, scale: f32) -> ImageHandle
```

Do not duplicate resize or filter-selection logic.

- [ ] **Step 4: Build the thumbnail from a bounded display copy**

In the `Presentation::MacosSavedThumbnail` branch:

```rust
let source_size = Size::new(image.width() as f32, image.height() as f32);
let scale = crate::result_workspace::viewport::display_downscale_scale(
    source_size,
    crate::result_workspace::viewport::DEFAULT_MAX_TEXTURE_DIM,
);
let handle = crate::result_workspace::build_display_handle(&image, scale);
```

Then move the original `image` into `ResultDocument::saved` as before.

- [ ] **Step 5: Run focused product tests and verify GREEN**

Run:

```bash
rtk cargo test -p rollshot-app completed_capture
```

Expected: normal and oversized capture-completion tests pass.

### Task 3: Prefer FileManager1 ShowItems on Linux Reveal

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/rollshot-app/src/result_workspace/actions.rs:42-76`
- Test: `crates/rollshot-app/src/result_workspace/actions.rs`

- [ ] **Step 1: Write failing fallback-decision tests**

Add a portable helper test module using closures that record which operation
ran:

```rust
#[test]
fn reveal_with_fallback_skips_fallback_after_primary_success() {
    let mut fallback_called = false;
    let result = reveal_with_fallback(
        || Ok(()),
        || {
            fallback_called = true;
            Ok(())
        },
    );
    assert_eq!(result, Ok(()));
    assert!(!fallback_called);
}

#[test]
fn reveal_with_fallback_runs_fallback_after_primary_failure() {
    let mut fallback_called = false;
    let result = reveal_with_fallback(
        || Err("D-Bus unavailable".to_string()),
        || {
            fallback_called = true;
            Ok(())
        },
    );
    assert_eq!(result, Ok(()));
    assert!(fallback_called);
}

#[test]
fn reveal_with_fallback_reports_both_failures() {
    let result = reveal_with_fallback(
        || Err("D-Bus unavailable".to_string()),
        || Err("xdg-open unavailable".to_string()),
    )
    .expect_err("both operations failed");
    assert!(result.contains("D-Bus unavailable"));
    assert!(result.contains("xdg-open unavailable"));
}
```

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
rtk cargo test -p rollshot-app reveal_with_fallback
```

Expected: compilation fails because `reveal_with_fallback` does not exist.

- [ ] **Step 3: Implement the fallback-decision helper**

Add:

```rust
fn reveal_with_fallback(
    primary: impl FnOnce() -> Result<(), String>,
    fallback: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    match primary() {
        Ok(()) => Ok(()),
        Err(primary_error) => fallback().map_err(|fallback_error| {
            format!("{primary_error}; fallback failed: {fallback_error}")
        }),
    }
}
```

- [ ] **Step 4: Add Linux-only dependencies**

In `crates/rollshot-app/Cargo.toml` add:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
url = { workspace = true }
zbus = { workspace = true }
```

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected: dependencies resolve and `Cargo.lock` updates without errors.

- [ ] **Step 5: Implement the Linux FileManager1 adapter**

Add a Linux-only helper:

```rust
#[cfg(target_os = "linux")]
fn reveal_with_file_manager1(path: &Path) -> Result<(), String> {
    let uri = url::Url::from_file_path(path)
        .map_err(|_| format!("cannot convert path to file URI: {}", path.display()))?
        .to_string();
    let connection =
        zbus::blocking::Connection::session().map_err(|e| format!("D-Bus session failed: {e}"))?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1",
    )
    .map_err(|e| format!("FileManager1 proxy failed: {e}"))?;
    proxy
        .call::<_, _, ()>("ShowItems", &(vec![uri], ""))
        .map_err(|e| format!("FileManager1 ShowItems failed: {e}"))
}
```

Keep `xdg-open <parent>` in a separate Linux-only helper. Change the Linux
branch of `reveal` to:

```rust
return reveal_with_fallback(
    || reveal_with_file_manager1(path),
    || reveal_with_xdg_open(path),
);
```

Update the function documentation to state that Linux prefers FileManager1 and
falls back to `xdg-open`.

- [ ] **Step 6: Run focused action tests and verify GREEN**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::actions
```

Expected: fallback-decision tests pass without requiring a live D-Bus session.

### Task 4: Repository Verification

**Files:**
- Verify all modified files.

- [ ] **Step 1: Run the app test suite**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: all `rollshot-app` tests pass.

- [ ] **Step 2: Run the workspace test suite**

Run:

```bash
rtk cargo test --workspace
```

Expected: all workspace tests pass.

- [ ] **Step 3: Run formatting and lint checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk git diff --check
```

Expected: all commands exit successfully with no warnings or whitespace errors.

- [ ] **Step 4: Record remaining runtime verification**

The final implementation report must explicitly state that these checks still
require a macOS host:

1. Thumbnail placement on secondary displays above, below, left, and right of
   the primary display.
2. Rendering a capture whose long edge exceeds the device texture limit in the
   floating thumbnail.
3. Finder reveal selection through FileManager1 is Linux-only; macOS Finder
   reveal remains covered by `open -R`.
