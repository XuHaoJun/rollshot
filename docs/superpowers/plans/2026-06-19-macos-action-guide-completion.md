# macOS Action Guide Completion Plan

**Goal:** Complete the macOS Action Guide launch, fullscreen, and permission
recovery paths without adding another event loop.

**Architecture:** Reuse `MacosProduct`, `macos_capture::Component`, and
`Driver`. Fullscreen is a component boot mode with a full-source crop and a
temporary app-owned macOS recording tray feeding the existing iced daemon.

## Task 1: Repair launch surfaces

**Files:** `crates/rollshot-cli/src/cmd_action_guide.rs`,
`crates/rollshot-app/src/main.rs`, `crates/rollshot-app/src/macos_product.rs`

1. Update forwarding tests to expect `action-guide`.
2. Route macOS region Action Guide to `MacosProduct`.
3. Verify CLI/app feature tests.

## Task 2: Add macOS fullscreen component boot

**Files:** `crates/rollshot-iced-overlay/src/macos_capture.rs`,
`crates/rollshot-app/src/macos_product.rs`

1. Add failing component tests for fullscreen Action Guide acceptance and boot.
2. Permit only `ActionGuide × Fullscreen` through the component.
3. Initialize a full-source selected crop and begin recording after window boot.
4. Keep the overlay passthrough and render no fullscreen overlay controls.
5. Create a temporary menu-bar item with Finish and Cancel actions.
6. Verify Finish/Cancel resource cleanup and host effects.

## Task 3: Add Input Monitoring recovery action

**Files:** `crates/rollshot-app/src/timeline_workspace/mod.rs`,
`crates/rollshot-app/src/timeline_workspace/update.rs`,
`crates/rollshot-app/src/timeline_workspace/view.rs`

1. Add a macOS-only message for opening Input Monitoring settings.
2. Render the button only for visual-only capability on macOS.
3. Call `rollshot_macos_input::open_input_monitoring_settings`.
4. Add update/view tests.

## Task 4: Verify

1. `rtk cargo fmt --check`
2. `rtk cargo test -p rollshot-cli --features action-guide`
3. `rtk cargo test -p rollshot-iced-overlay --features action-guide`
4. `rtk cargo test -p rollshot-app --features action-guide`
5. `rtk cargo clippy --workspace --all-targets --features rollshot-cli/action-guide,rollshot-app/action-guide -- -D warnings`
6. Real macOS region and fullscreen smoke tests.
