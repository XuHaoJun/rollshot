# rollshot v0.5 Plan 2: Tauri App, Live Preview, and Region Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert `rollshot-app` into a Tauri v2 React app that starts the capture backend, displays a bounded-cadence live preview, and returns a validated source-pixel region.

**Architecture:** Move the placeholder Rust app into a Tauri `src-tauri` crate named `rollshot-app`, with a small React frontend in `crates/rollshot-app/src`. Rust owns capture state and stores the latest full-resolution frame; the capture stream is constructed inside the worker thread so platform stream objects never need to cross thread boundaries. Region selection math lives in TypeScript as pure functions so HiDPI/source-pixel conversion is tested without a running portal.

**Tech Stack:** Rust 2021, Tauri v2, React, TypeScript, Vite, Vitest, `rollshot-capture`, `image`, serde/serde_json.

---

## Source Spec

Plan 2 implements only this section of the replacement spec:

```text
Plan 2: Tauri App Scaffold, Live Preview, and Region Selection

- convert `rollshot-app` from placeholder to Tauri v2 app
- wire React frontend scaffold
- call capture backend from Tauri commands
- display bounded-cadence live preview
- implement source-pixel region selection and HiDPI tests

Do not implement full stitching lifecycle in this plan.
```

Source: `docs/superpowers/specs/2026-05-23-rollshot-v05-interactive-capture-replacement-design.md`

## Assumptions

- Plan 1 is implemented: `rollshot capture` launches `rollshot-app --capture <json>` and `InteractiveLaunchOptions` already exists in `rollshot-capture`.
- Plan 2 does not add save, copy, crop, stitch, or final image state. Region confirmation only stores a valid source-frame rectangle for Plan 3.
- Use `npm` for the app scaffold because this repository has no existing frontend lockfile. The first implementation run should create `crates/rollshot-app/package-lock.json`.
- Linux Wayland portal verification is manual because automated CI cannot grant portal capture permission.

---

## File Structure

Modify:

- `Cargo.toml`
  - Change the app workspace member from `crates/rollshot-app` to `crates/rollshot-app/src-tauri`.

- `.gitignore`
  - Ignore frontend dependency and build output under `crates/rollshot-app`.

- `crates/rollshot-app/Cargo.toml`
  - Delete this placeholder Rust package manifest after creating `src-tauri/Cargo.toml`.

- `crates/rollshot-app/src/main.rs`
  - Delete this placeholder binary after creating `src-tauri/src/main.rs`.

Create:

- `crates/rollshot-app/package.json`
  - Frontend scripts and Tauri CLI entrypoint.

- `crates/rollshot-app/index.html`
  - Vite HTML entrypoint.

- `crates/rollshot-app/tsconfig.json`
  - TypeScript config for app and Vitest.

- `crates/rollshot-app/vite.config.ts`
  - React/Vite/Vitest config.

- `crates/rollshot-app/src/main.tsx`
  - React root mounting.

- `crates/rollshot-app/src/App.tsx`
  - Minimal interactive capture UI shell.

- `crates/rollshot-app/src/styles.css`
  - Functional layout for preview, controls, and selection overlay.

- `crates/rollshot-app/src/api/capture.ts`
  - Tauri command wrappers.

- `crates/rollshot-app/src/region/geometry.ts`
  - Source-pixel conversion and drag rectangle math.

- `crates/rollshot-app/src/region/geometry.test.ts`
  - HiDPI and drag/resize unit tests.

- `crates/rollshot-app/src/components/RegionOverlay.tsx`
  - Canvas/image overlay for selection.

- `crates/rollshot-app/src-tauri/Cargo.toml`
  - Tauri Rust app manifest named `rollshot-app`.

- `crates/rollshot-app/src-tauri/build.rs`
  - Tauri build hook.

- `crates/rollshot-app/src-tauri/tauri.conf.json`
  - Tauri v2 app config.

- `crates/rollshot-app/src-tauri/capabilities/default.json`
  - Minimal capability allowlist for app commands.

- `crates/rollshot-app/src-tauri/src/main.rs`
  - Native binary entrypoint.

- `crates/rollshot-app/src-tauri/src/lib.rs`
  - Tauri builder setup and command registration.

- `crates/rollshot-app/src-tauri/src/commands.rs`
  - Tauri commands used by the frontend.

- `crates/rollshot-app/src-tauri/src/launch.rs`
  - Parse `--capture <json>` launch arguments.

- `crates/rollshot-app/src-tauri/src/session.rs`
  - Capture session state, capture worker lifecycle, preview encoding, and region validation.

---

## Task 1: Replace Placeholder App With A Minimal Tauri Scaffold

**Files:**
- Modify: `Cargo.toml`
- Modify: `.gitignore`
- Delete: `crates/rollshot-app/Cargo.toml`
- Delete: `crates/rollshot-app/src/main.rs`
- Create: `crates/rollshot-app/package.json`
- Create: `crates/rollshot-app/index.html`
- Create: `crates/rollshot-app/tsconfig.json`
- Create: `crates/rollshot-app/vite.config.ts`
- Create: `crates/rollshot-app/src/main.tsx`
- Create: `crates/rollshot-app/src/App.tsx`
- Create: `crates/rollshot-app/src/styles.css`
- Create: `crates/rollshot-app/src-tauri/Cargo.toml`
- Create: `crates/rollshot-app/src-tauri/build.rs`
- Create: `crates/rollshot-app/src-tauri/tauri.conf.json`
- Create: `crates/rollshot-app/src-tauri/capabilities/default.json`
- Create: `crates/rollshot-app/src-tauri/src/main.rs`
- Create: `crates/rollshot-app/src-tauri/src/lib.rs`

- [ ] **Step 1: Move the workspace member to the Tauri Rust crate**

Edit the workspace members in `Cargo.toml`:

```toml
[workspace]
members = [
    "crates/rollshot-core",
    "crates/rollshot-capture",
    "crates/rollshot-cli",
    "crates/rollshot-app/src-tauri",
]
resolver = "2"
```

- [ ] **Step 2: Add app build output ignores**

Append these lines to `.gitignore`:

```gitignore
crates/rollshot-app/node_modules/
crates/rollshot-app/dist/
crates/rollshot-app/src-tauri/target/
```

- [ ] **Step 3: Create the frontend package manifest**

Create `crates/rollshot-app/package.json`:

```json
{
  "name": "rollshot-app",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "typecheck": "tsc --noEmit",
    "test": "vitest run",
    "tauri": "tauri",
    "tauri:dev": "tauri dev",
    "tauri:build": "tauri build",
    "tauri:check": "tauri build --debug --no-bundle"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^5.0.0",
    "typescript": "^5.0.0",
    "vite": "^7.0.0",
    "vitest": "^3.0.0"
  }
}
```

- [ ] **Step 4: Create the Vite entry files**

Create `crates/rollshot-app/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>rollshot</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Create `crates/rollshot-app/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "allowJs": false,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "strict": true,
    "forceConsistentCasingInFileNames": true,
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "types": ["vitest/globals"]
  },
  "include": ["src", "vite.config.ts"]
}
```

Create `crates/rollshot-app/vite.config.ts`:

```ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
  },
})
```

- [ ] **Step 5: Create the first React screen**

Create `crates/rollshot-app/src/main.tsx`:

```tsx
import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './styles.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
```

Create `crates/rollshot-app/src/App.tsx`:

```tsx
export default function App() {
  return (
    <main className="app-shell">
      <section className="capture-surface">
        <div className="empty-preview">No preview yet</div>
      </section>
      <aside className="control-panel" aria-label="Capture controls">
        <h1>rollshot</h1>
        <p className="status-text">Ready to start capture</p>
        <button type="button">Start</button>
        <button type="button" disabled>
          Confirm Region
        </button>
        <button type="button" disabled>
          Stop
        </button>
      </aside>
    </main>
  )
}
```

Create `crates/rollshot-app/src/styles.css`:

```css
:root {
  color: #1b1f23;
  background: #f5f7f9;
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI",
    sans-serif;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  min-width: 320px;
  min-height: 100vh;
}

button {
  min-height: 36px;
  border: 1px solid #9aa4b2;
  border-radius: 6px;
  background: #ffffff;
  color: #111827;
  font: inherit;
  cursor: pointer;
}

button:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.app-shell {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  min-height: 100vh;
}

.capture-surface {
  min-width: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  background: #dce3ea;
}

.empty-preview {
  color: #4b5563;
}

.control-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 20px;
  border-left: 1px solid #c7d0dc;
  background: #ffffff;
}

.control-panel h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 650;
}

.status-text {
  min-height: 44px;
  margin: 0;
  color: #4b5563;
  line-height: 1.4;
}

@media (max-width: 720px) {
  .app-shell {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(320px, 1fr) auto;
  }

  .control-panel {
    border-left: 0;
    border-top: 1px solid #c7d0dc;
  }
}
```

- [ ] **Step 6: Create the Tauri Rust manifest and config**

Create `crates/rollshot-app/src-tauri/Cargo.toml`:

```toml
[package]
name = "rollshot-app"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "rollshot-app"
path = "src/main.rs"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
image = { workspace = true }
rollshot-capture = { path = "../../rollshot-capture" }
serde = { workspace = true }
serde_json = { workspace = true }
tauri = { version = "2", features = [] }
thiserror = { workspace = true }

[features]
default = []
macos-sck = ["rollshot-capture/macos-sck"]

[lints]
workspace = true
```

Create `crates/rollshot-app/src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

Create `crates/rollshot-app/src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "rollshot",
  "version": "0.1.0",
  "identifier": "dev.rollshot.app",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "rollshot",
        "width": 1180,
        "height": 780,
        "minWidth": 760,
        "minHeight": 520,
        "resizable": true,
        "fullscreen": false,
        "decorations": true,
        "transparent": false
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' blob: data:; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost"
    }
  },
  "bundle": {
    "active": false,
    "targets": "all"
  }
}
```

Create `crates/rollshot-app/src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default rollshot app permissions",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

- [ ] **Step 7: Create the native entrypoint**

Create `crates/rollshot-app/src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rollshot_app::run()
}
```

Create `crates/rollshot-app/src-tauri/src/lib.rs`:

```rust
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running rollshot app");
}
```

- [ ] **Step 8: Delete the placeholder Rust package**

Delete:

```text
crates/rollshot-app/Cargo.toml
crates/rollshot-app/src/main.rs
```

- [ ] **Step 9: Install frontend dependencies**

Run:

```bash
cd crates/rollshot-app && npm install
```

Expected: `package-lock.json` is created and dependencies install successfully.

- [ ] **Step 10: Verify the scaffold builds**

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected: PASS.

Run:

```bash
cd crates/rollshot-app && npm run typecheck
```

Expected: PASS.

- [ ] **Step 11: Commit Task 1**

Run:

```bash
rtk git add Cargo.toml .gitignore crates/rollshot-app
rtk git add -u crates/rollshot-app
rtk git commit -m "feat(app): scaffold tauri capture app"
```

---

## Task 2: Parse CLI Launch Options In rollshot-app

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs`
- Create: `crates/rollshot-app/src-tauri/src/launch.rs`

- [ ] **Step 1: Add failing launch parser tests**

Create `crates/rollshot-app/src-tauri/src/launch.rs`:

```rust
use rollshot_capture::InteractiveLaunchOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
}

pub fn parse_launch_args<I, S>(_args: I) -> Result<LaunchMode, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Err("not implemented".to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_launch_args, LaunchMode};

    #[test]
    fn parses_capture_launch_options() {
        let mode = parse_launch_args([
            "rollshot-app",
            "--capture",
            r#"{"backend":"linux-portal","fps":7,"show_cursor":true}"#,
        ])
        .expect("parse launch args");

        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "linux-portal");
                assert_eq!(options.fps, 7);
                assert!(options.show_cursor);
            }
        }
    }

    #[test]
    fn rejects_missing_capture_payload() {
        let err = parse_launch_args(["rollshot-app", "--capture"]).expect_err("missing payload");
        assert!(err.contains("--capture requires a JSON payload"), "err = {err}");
    }

    #[test]
    fn rejects_unknown_args() {
        let err = parse_launch_args(["rollshot-app", "--bogus"]).expect_err("unknown arg");
        assert!(err.contains("unknown rollshot-app argument"), "err = {err}");
    }
}
```

- [ ] **Step 2: Run the failing parser tests**

Run:

```bash
rtk cargo test -p rollshot-app launch::
```

Expected: FAIL because `parse_launch_args` returns the temporary error.

- [ ] **Step 3: Implement the parser**

Replace `parse_launch_args` in `crates/rollshot-app/src-tauri/src/launch.rs`:

```rust
pub fn parse_launch_args<I, S>(args: I) -> Result<LaunchMode, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let Some(flag) = args.next() else {
        return Err("rollshot-app must be launched by `rollshot capture` with --capture".to_string());
    };

    if flag != "--capture" {
        return Err(format!("unknown rollshot-app argument '{flag}'"));
    }

    let Some(payload) = args.next() else {
        return Err("--capture requires a JSON payload".to_string());
    };

    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument after capture payload: '{extra}'"));
    }

    let options: InteractiveLaunchOptions = serde_json::from_str(&payload)
        .map_err(|err| format!("invalid --capture JSON payload: {err}"))?;
    Ok(LaunchMode::Capture(options))
}
```

- [ ] **Step 4: Register the module and parse at startup**

Replace `crates/rollshot-app/src-tauri/src/lib.rs` with:

```rust
mod launch;

use launch::LaunchMode;

pub fn run() {
    let launch_mode = match launch::parse_launch_args(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let _launch_options = match launch_mode {
        LaunchMode::Capture(options) => options,
    };

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running rollshot app");
}
```

- [ ] **Step 5: Verify parser tests pass**

Run:

```bash
rtk cargo test -p rollshot-app launch::
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/src/lib.rs crates/rollshot-app/src-tauri/src/launch.rs
rtk git commit -m "feat(app): parse capture launch options"
```

---

## Task 3: Add Rust Capture Session Commands And Preview PNG IPC

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs`
- Create: `crates/rollshot-app/src-tauri/src/commands.rs`
- Create: `crates/rollshot-app/src-tauri/src/session.rs`

- [ ] **Step 1: Add failing session tests**

Create `crates/rollshot-app/src-tauri/src/session.rs`:

```rust
use std::time::SystemTime;

use image::{Rgba, RgbaImage};
use rollshot_capture::{CapturedFrame, FrameMetadata, Region};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Previewing {
        frame_width: u32,
        frame_height: u32,
        region: Option<RegionDto>,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegionDto {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl From<RegionDto> for Region {
    fn from(value: RegionDto) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(Default)]
pub struct AppSession {
    latest_frame: Option<CapturedFrame>,
    selected_region: Option<Region>,
    error: Option<String>,
}

impl AppSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store_frame_for_test(&mut self, frame: CapturedFrame) {
        self.latest_frame = Some(frame);
    }

    pub fn status(&self) -> SessionStatus {
        SessionStatus::Idle
    }

    pub fn confirm_region(&mut self, _region: RegionDto) -> Result<RegionDto, String> {
        Err("not implemented".to_string())
    }

    pub fn latest_preview_png(&self, _max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        Err("not implemented".to_string())
    }
}

pub fn make_test_frame(width: u32, height: u32) -> CapturedFrame {
    CapturedFrame {
        image: RgbaImage::from_pixel(width, height, Rgba([10, 20, 30, 255])),
        timestamp: SystemTime::UNIX_EPOCH,
        metadata: FrameMetadata::fake(),
    }
}

#[cfg(test)]
mod tests {
    use super::{make_test_frame, AppSession, RegionDto, SessionStatus};

    #[test]
    fn status_reports_latest_frame_size() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        assert_eq!(
            session.status(),
            SessionStatus::Previewing {
                frame_width: 320,
                frame_height: 200,
                region: None
            }
        );
    }

    #[test]
    fn confirm_region_rejects_region_outside_frame() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        let err = session
            .confirm_region(RegionDto {
                x: 300,
                y: 10,
                width: 40,
                height: 40,
            })
            .expect_err("region outside frame");

        assert!(err.contains("outside frame bounds"), "err = {err}");
    }

    #[test]
    fn confirm_region_stores_source_pixel_region() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        let region = session
            .confirm_region(RegionDto {
                x: 10,
                y: 12,
                width: 100,
                height: 80,
            })
            .expect("valid region");

        assert_eq!(region.x, 10);
        assert_eq!(region.y, 12);
        assert_eq!(region.width, 100);
        assert_eq!(region.height, 80);
    }

    #[test]
    fn latest_preview_png_resizes_large_frame() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(800, 400));

        let bytes = session
            .latest_preview_png(200)
            .expect("encode preview")
            .expect("preview exists");
        let image = image::load_from_memory(&bytes).expect("decode png");

        assert_eq!(image.width(), 200);
        assert_eq!(image.height(), 100);
    }
}
```

- [ ] **Step 2: Run the failing session tests**

Run:

```bash
rtk cargo test -p rollshot-app session::
```

Expected: FAIL because `status`, `confirm_region`, and `latest_preview_png` are temporary implementations.

- [ ] **Step 3: Implement session status, region validation, and preview encoding**

Replace the `impl AppSession` block in `crates/rollshot-app/src-tauri/src/session.rs` with:

```rust
impl AppSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store_frame_for_test(&mut self, frame: CapturedFrame) {
        self.latest_frame = Some(frame);
    }

    pub fn status(&self) -> SessionStatus {
        if let Some(message) = &self.error {
            return SessionStatus::Failed {
                message: message.clone(),
            };
        }

        match &self.latest_frame {
            Some(frame) => SessionStatus::Previewing {
                frame_width: frame.image.width(),
                frame_height: frame.image.height(),
                region: self.selected_region.map(|region| RegionDto {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                }),
            },
            None => SessionStatus::Idle,
        }
    }

    pub fn confirm_region(&mut self, region: RegionDto) -> Result<RegionDto, String> {
        let frame = self
            .latest_frame
            .as_ref()
            .ok_or_else(|| "cannot confirm a region before a frame is available".to_string())?;

        if region.x < 0 || region.y < 0 || region.width == 0 || region.height == 0 {
            return Err("region must have non-negative origin and non-zero size".to_string());
        }

        let right = region.x as u32 + region.width;
        let bottom = region.y as u32 + region.height;
        if right > frame.image.width() || bottom > frame.image.height() {
            return Err(format!(
                "region x={},y={},w={},h={} is outside frame bounds {}x{}",
                region.x,
                region.y,
                region.width,
                region.height,
                frame.image.width(),
                frame.image.height()
            ));
        }

        self.selected_region = Some(region.into());
        Ok(region)
    }

    pub fn latest_preview_png(&self, max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        let Some(frame) = &self.latest_frame else {
            return Ok(None);
        };

        let max_edge = max_edge.max(1);
        let width = frame.image.width();
        let height = frame.image.height();
        let largest = width.max(height).max(1);
        let scale = (max_edge as f32 / largest as f32).min(1.0);
        let preview_width = ((width as f32 * scale).round() as u32).max(1);
        let preview_height = ((height as f32 * scale).round() as u32).max(1);

        let preview = if preview_width == width && preview_height == height {
            frame.image.clone()
        } else {
            image::imageops::resize(
                &frame.image,
                preview_width,
                preview_height,
                image::imageops::FilterType::Triangle,
            )
        };

        let mut cursor = std::io::Cursor::new(Vec::new());
        preview
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|err| format!("failed to encode preview png: {err}"))?;
        Ok(Some(cursor.into_inner()))
    }
}
```

- [ ] **Step 4: Add capture reader support and Tauri commands**

Append this code to `crates/rollshot-app/src-tauri/src/session.rs`:

```rust
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

use rollshot_capture::{BackendKind, CaptureOptions, InteractiveLaunchOptions, RegionMode};

pub struct SharedSession {
    inner: std::sync::Mutex<AppSession>,
    stop: AtomicBool,
    reader: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl SharedSession {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(AppSession::new()),
            stop: AtomicBool::new(false),
            reader: std::sync::Mutex::new(None),
        }
    }

    pub fn start_capture(self: &Arc<Self>, options: InteractiveLaunchOptions) -> Result<(), String> {
        let mut reader = self.reader.lock().map_err(|_| "reader lock poisoned".to_string())?;
        if reader.is_some() {
            return Err("capture is already running".to_string());
        }

        {
            let mut inner = self.inner.lock().map_err(|_| "session lock poisoned".to_string())?;
            inner.latest_frame = None;
            inner.selected_region = None;
            inner.error = None;
        }

        self.start_reader(options, &mut reader);
        Ok(())
    }

    fn start_reader(
        self: &Arc<Self>,
        options: InteractiveLaunchOptions,
        reader_slot: &mut Option<JoinHandle<()>>,
    ) {
        self.stop.store(false, Ordering::Relaxed);
        let session = Arc::clone(self);
        *reader_slot = Some(std::thread::spawn(move || {
            let kind = match BackendKind::from_cli_flag(&options.backend) {
                Ok(kind) => kind,
                Err(err) => {
                    session.store_error(err.to_string());
                    return;
                }
            };
            let mut backend = match kind.create() {
                Ok(backend) => backend,
                Err(err) => {
                    session.store_error(err.to_string());
                    return;
                }
            };
            let capture_options = CaptureOptions {
                region: RegionMode::FullSource,
                fps: options.fps,
                show_cursor: options.show_cursor,
                prefer_portal_region: false,
            };
            let mut stream = match backend.start(capture_options) {
                Ok(stream) => stream,
                Err(err) => {
                    session.store_error(err.to_string());
                    return;
                }
            };

            while !session.stop.load(Ordering::Relaxed) {
                match stream.next_frame() {
                    Ok(frame) => {
                        if let Ok(mut inner) = session.inner.lock() {
                            inner.latest_frame = Some(frame);
                            inner.error = None;
                        }
                    }
                    Err(rollshot_capture::CaptureError::EndOfStream) => break,
                    Err(err) => {
                        if let Ok(mut inner) = session.inner.lock() {
                            inner.error = Some(err.to_string());
                        }
                        break;
                    }
                }
            }
        }));
    }

    fn store_error(&self, message: String) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.error = Some(message);
        }
    }

    pub fn stop_capture(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut reader) = self.reader.lock() {
            if let Some(handle) = reader.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn status(&self) -> Result<SessionStatus, String> {
        let inner = self.inner.lock().map_err(|_| "session lock poisoned".to_string())?;
        Ok(inner.status())
    }

    pub fn confirm_region(&self, region: RegionDto) -> Result<RegionDto, String> {
        let mut inner = self.inner.lock().map_err(|_| "session lock poisoned".to_string())?;
        inner.confirm_region(region)
    }

    pub fn latest_preview_png(&self, max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        let inner = self.inner.lock().map_err(|_| "session lock poisoned".to_string())?;
        inner.latest_preview_png(max_edge)
    }
}

impl Drop for SharedSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
```

Create `crates/rollshot-app/src-tauri/src/commands.rs`:

```rust
use std::sync::Arc;

use rollshot_capture::InteractiveLaunchOptions;
use tauri::ipc::Response;

use crate::session::{RegionDto, SessionStatus, SharedSession};

#[tauri::command]
pub fn launch_options(
    options: tauri::State<'_, InteractiveLaunchOptions>,
) -> InteractiveLaunchOptions {
    options.inner().clone()
}

#[tauri::command]
pub fn start_capture(
    session: tauri::State<'_, Arc<SharedSession>>,
    options: InteractiveLaunchOptions,
) -> Result<(), String> {
    session.start_capture(options)
}

#[tauri::command]
pub fn stop_capture(session: tauri::State<'_, Arc<SharedSession>>) -> Result<(), String> {
    session.stop_capture();
    Ok(())
}

#[tauri::command]
pub fn session_status(
    session: tauri::State<'_, Arc<SharedSession>>,
) -> Result<SessionStatus, String> {
    session.status()
}

#[tauri::command]
pub fn confirm_region(
    session: tauri::State<'_, Arc<SharedSession>>,
    region: RegionDto,
) -> Result<RegionDto, String> {
    session.confirm_region(region)
}

#[tauri::command]
pub fn get_latest_preview(
    session: tauri::State<'_, Arc<SharedSession>>,
    max_edge: u32,
) -> Result<Response, String> {
    let bytes = session.latest_preview_png(max_edge)?.unwrap_or_default();
    Ok(Response::new(bytes))
}
```

- [ ] **Step 5: Register state and commands in Tauri**

Replace `crates/rollshot-app/src-tauri/src/lib.rs` with:

```rust
mod commands;
mod launch;
mod session;

use std::sync::Arc;

use launch::LaunchMode;
use session::SharedSession;

pub fn run() {
    let launch_mode = match launch::parse_launch_args(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let launch_options = match launch_mode {
        LaunchMode::Capture(options) => options,
    };
    let shared_session = Arc::new(SharedSession::new());

    tauri::Builder::default()
        .manage(launch_options)
        .manage(Arc::clone(&shared_session))
        .invoke_handler(tauri::generate_handler![
            commands::launch_options,
            commands::start_capture,
            commands::stop_capture,
            commands::session_status,
            commands::confirm_region,
            commands::get_latest_preview
        ])
        .run(tauri::generate_context!())
        .expect("error while running rollshot app");
}
```

- [ ] **Step 6: Verify Rust tests pass**

Run:

```bash
rtk cargo test -p rollshot-app session::
```

Expected: PASS.

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected: PASS.

- [ ] **Step 7: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/src
rtk git commit -m "feat(app): add preview capture session commands"
```

---

## Task 4: Add Frontend Command Wrappers And Region Math Tests

**Files:**
- Create: `crates/rollshot-app/src/api/capture.ts`
- Create: `crates/rollshot-app/src/region/geometry.ts`
- Create: `crates/rollshot-app/src/region/geometry.test.ts`

- [ ] **Step 1: Add failing region geometry tests**

Create `crates/rollshot-app/src/region/geometry.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import {
  clampSourceRegion,
  dragToCssRect,
  cssRectToSourceRegion,
  type CssRect,
} from './geometry'

describe('region geometry', () => {
  it('normalizes a drag in any direction', () => {
    expect(dragToCssRect({ x: 80, y: 70 }, { x: 20, y: 10 })).toEqual({
      left: 20,
      top: 10,
      width: 60,
      height: 60,
    })
  })

  it('converts CSS preview coordinates to source pixels with HiDPI scale', () => {
    const cssRect: CssRect = { left: 50, top: 25, width: 200, height: 100 }
    expect(
      cssRectToSourceRegion(cssRect, {
        renderedWidth: 500,
        renderedHeight: 250,
        sourceWidth: 1000,
        sourceHeight: 500,
      }),
    ).toEqual({ x: 100, y: 50, width: 400, height: 200 })
  })

  it('clamps source region to frame bounds', () => {
    expect(
      clampSourceRegion(
        { x: -4, y: 90, width: 40, height: 30 },
        { width: 100, height: 100 },
      ),
    ).toEqual({ x: 0, y: 90, width: 36, height: 10 })
  })
})
```

- [ ] **Step 2: Run the failing frontend test**

Run:

```bash
cd crates/rollshot-app && npm test -- --run src/region/geometry.test.ts
```

Expected: FAIL because `geometry.ts` does not exist.

- [ ] **Step 3: Implement region geometry**

Create `crates/rollshot-app/src/region/geometry.ts`:

```ts
export type Point = {
  x: number
  y: number
}

export type CssRect = {
  left: number
  top: number
  width: number
  height: number
}

export type SourceRegion = {
  x: number
  y: number
  width: number
  height: number
}

export type PreviewScale = {
  renderedWidth: number
  renderedHeight: number
  sourceWidth: number
  sourceHeight: number
}

export type SourceSize = {
  width: number
  height: number
}

export function dragToCssRect(start: Point, current: Point): CssRect {
  const left = Math.min(start.x, current.x)
  const top = Math.min(start.y, current.y)
  return {
    left,
    top,
    width: Math.abs(current.x - start.x),
    height: Math.abs(current.y - start.y),
  }
}

export function cssRectToSourceRegion(
  rect: CssRect,
  scale: PreviewScale,
): SourceRegion {
  const xScale = scale.sourceWidth / scale.renderedWidth
  const yScale = scale.sourceHeight / scale.renderedHeight
  return clampSourceRegion(
    {
      x: Math.round(rect.left * xScale),
      y: Math.round(rect.top * yScale),
      width: Math.round(rect.width * xScale),
      height: Math.round(rect.height * yScale),
    },
    { width: scale.sourceWidth, height: scale.sourceHeight },
  )
}

export function clampSourceRegion(
  region: SourceRegion,
  source: SourceSize,
): SourceRegion {
  const x = Math.max(0, Math.min(Math.round(region.x), source.width))
  const y = Math.max(0, Math.min(Math.round(region.y), source.height))
  const right = Math.max(x, Math.min(Math.round(region.x + region.width), source.width))
  const bottom = Math.max(y, Math.min(Math.round(region.y + region.height), source.height))
  return {
    x,
    y,
    width: right - x,
    height: bottom - y,
  }
}
```

- [ ] **Step 4: Add Tauri command wrappers**

Create `crates/rollshot-app/src/api/capture.ts`:

```ts
import { invoke } from '@tauri-apps/api/core'
import type { SourceRegion } from '../region/geometry'

export type InteractiveLaunchOptions = {
  backend: string
  fps: number
  show_cursor: boolean
}

export type RegionDto = {
  x: number
  y: number
  width: number
  height: number
}

export type SessionStatus =
  | { state: 'idle' }
  | {
      state: 'previewing'
      frame_width: number
      frame_height: number
      region: RegionDto | null
    }
  | { state: 'failed'; message: string }

export async function launchOptions(): Promise<InteractiveLaunchOptions> {
  return await invoke<InteractiveLaunchOptions>('launch_options')
}

export async function startCapture(options: InteractiveLaunchOptions): Promise<void> {
  await invoke('start_capture', { options })
}

export async function stopCapture(): Promise<void> {
  await invoke('stop_capture')
}

export async function sessionStatus(): Promise<SessionStatus> {
  return await invoke<SessionStatus>('session_status')
}

export async function getLatestPreview(maxEdge: number): Promise<Blob | null> {
  const bytes = await invoke<ArrayBuffer>('get_latest_preview', { maxEdge })
  if (bytes.byteLength === 0) {
    return null
  }
  return new Blob([bytes], { type: 'image/png' })
}

export async function confirmRegion(region: SourceRegion): Promise<RegionDto> {
  return await invoke<RegionDto>('confirm_region', {
    region: {
      x: region.x,
      y: region.y,
      width: region.width,
      height: region.height,
    },
  })
}
```

- [ ] **Step 5: Verify frontend tests pass**

Run:

```bash
cd crates/rollshot-app && npm test -- --run src/region/geometry.test.ts
```

Expected: PASS.

Run:

```bash
cd crates/rollshot-app && npm run typecheck
```

Expected: PASS.

- [ ] **Step 6: Commit Task 4**

Run:

```bash
rtk git add crates/rollshot-app/src/api crates/rollshot-app/src/region
rtk git commit -m "feat(app): add region geometry and capture api"
```

---

## Task 5: Wire Live Preview UI And Region Selection

**Files:**
- Modify: `crates/rollshot-app/src/App.tsx`
- Modify: `crates/rollshot-app/src/styles.css`
- Create: `crates/rollshot-app/src/components/RegionOverlay.tsx`

- [ ] **Step 1: Create the region overlay component**

Create `crates/rollshot-app/src/components/RegionOverlay.tsx`:

```tsx
import { useMemo, useRef, useState } from 'react'
import {
  cssRectToSourceRegion,
  dragToCssRect,
  type CssRect,
  type Point,
  type SourceRegion,
} from '../region/geometry'

type RegionOverlayProps = {
  imageUrl: string
  sourceWidth: number
  sourceHeight: number
  onRegionChange: (region: SourceRegion | null) => void
}

export function RegionOverlay({
  imageUrl,
  sourceWidth,
  sourceHeight,
  onRegionChange,
}: RegionOverlayProps) {
  const imageRef = useRef<HTMLImageElement | null>(null)
  const [start, setStart] = useState<Point | null>(null)
  const [rect, setRect] = useState<CssRect | null>(null)

  const overlayStyle = useMemo(() => {
    if (!rect) {
      return undefined
    }
    return {
      left: `${rect.left}px`,
      top: `${rect.top}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
    }
  }, [rect])

  function localPoint(event: React.PointerEvent<HTMLDivElement>): Point {
    const bounds = event.currentTarget.getBoundingClientRect()
    return {
      x: event.clientX - bounds.left,
      y: event.clientY - bounds.top,
    }
  }

  function publishRegion(nextRect: CssRect | null) {
    const image = imageRef.current
    if (!image || !nextRect || nextRect.width < 4 || nextRect.height < 4) {
      onRegionChange(null)
      return
    }

    onRegionChange(
      cssRectToSourceRegion(nextRect, {
        renderedWidth: image.clientWidth,
        renderedHeight: image.clientHeight,
        sourceWidth,
        sourceHeight,
      }),
    )
  }

  return (
    <div
      className="preview-wrap"
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId)
        const point = localPoint(event)
        setStart(point)
        const nextRect = dragToCssRect(point, point)
        setRect(nextRect)
        publishRegion(nextRect)
      }}
      onPointerMove={(event) => {
        if (!start) {
          return
        }
        const nextRect = dragToCssRect(start, localPoint(event))
        setRect(nextRect)
        publishRegion(nextRect)
      }}
      onPointerUp={(event) => {
        if (!start) {
          return
        }
        const nextRect = dragToCssRect(start, localPoint(event))
        setStart(null)
        setRect(nextRect)
        publishRegion(nextRect)
      }}
    >
      <img
        ref={imageRef}
        className="preview-image"
        src={imageUrl}
        alt="Live capture preview"
        draggable={false}
      />
      <div className="selection-dim" />
      {overlayStyle ? <div className="selection-box" style={overlayStyle} /> : null}
    </div>
  )
}
```

- [ ] **Step 2: Replace the app screen with preview polling**

Replace `crates/rollshot-app/src/App.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react'
import {
  confirmRegion,
  getLatestPreview,
  launchOptions,
  sessionStatus,
  startCapture,
  stopCapture,
  type InteractiveLaunchOptions,
  type SessionStatus,
} from './api/capture'
import { RegionOverlay } from './components/RegionOverlay'
import type { SourceRegion } from './region/geometry'

export default function App() {
  const [status, setStatus] = useState<SessionStatus>({ state: 'idle' })
  const [options, setOptions] = useState<InteractiveLaunchOptions | null>(null)
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [pendingRegion, setPendingRegion] = useState<SourceRegion | null>(null)
  const [message, setMessage] = useState('Ready to start capture')
  const previewUrlRef = useRef<string | null>(null)

  useEffect(() => {
    previewUrlRef.current = previewUrl
  }, [previewUrl])

  useEffect(() => {
    launchOptions()
      .then(setOptions)
      .catch((error) => setMessage(String(error)))
  }, [])

  useEffect(() => {
    return () => {
      if (previewUrlRef.current) {
        URL.revokeObjectURL(previewUrlRef.current)
      }
    }
  }, [])

  useEffect(() => {
    const timer = window.setInterval(async () => {
      try {
        const nextStatus = await sessionStatus()
        setStatus(nextStatus)

        if (nextStatus.state === 'previewing') {
          const blob = await getLatestPreview(1400)
          if (blob) {
            const nextUrl = URL.createObjectURL(blob)
            setPreviewUrl((oldUrl) => {
              if (oldUrl) {
                URL.revokeObjectURL(oldUrl)
              }
              return nextUrl
            })
          }
        }
      } catch (error) {
        setMessage(String(error))
      }
    }, 160)

    return () => window.clearInterval(timer)
  }, [])

  async function onStart() {
    if (!options) {
      setMessage('Launch options are not loaded yet')
      return
    }
    setMessage('Starting capture')
    await startCapture(options)
    setMessage('Select a region in the preview')
  }

  async function onConfirmRegion() {
    if (!pendingRegion) {
      setMessage('Select a region first')
      return
    }

    const confirmed = await confirmRegion(pendingRegion)
    setMessage(
      `Region ${confirmed.width}x${confirmed.height} at ${confirmed.x},${confirmed.y}`,
    )
  }

  async function onStop() {
    await stopCapture()
    setMessage('Capture stopped')
  }

  const canConfirm =
    status.state === 'previewing' &&
    pendingRegion !== null &&
    pendingRegion.width > 0 &&
    pendingRegion.height > 0

  return (
    <main className="app-shell">
      <section className="capture-surface">
        {status.state === 'previewing' && previewUrl ? (
          <RegionOverlay
            imageUrl={previewUrl}
            sourceWidth={status.frame_width}
            sourceHeight={status.frame_height}
            onRegionChange={setPendingRegion}
          />
        ) : (
          <div className="empty-preview">No preview yet</div>
        )}
      </section>
      <aside className="control-panel" aria-label="Capture controls">
        <h1>rollshot</h1>
        <p className="status-text">
          {status.state === 'failed' ? status.message : message}
        </p>
        <button type="button" onClick={onStart}>
          Start
        </button>
        <button type="button" disabled={!canConfirm} onClick={onConfirmRegion}>
          Confirm Region
        </button>
        <button type="button" onClick={onStop}>
          Stop
        </button>
      </aside>
    </main>
  )
}
```

- [ ] **Step 3: Add preview and selection styles**

Append this CSS to `crates/rollshot-app/src/styles.css`:

```css
.preview-wrap {
  position: relative;
  max-width: 100%;
  max-height: calc(100vh - 48px);
  line-height: 0;
  user-select: none;
  touch-action: none;
}

.preview-image {
  display: block;
  max-width: 100%;
  max-height: calc(100vh - 48px);
  object-fit: contain;
  border: 1px solid #8b95a5;
  background: #ffffff;
}

.selection-dim {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: rgba(17, 24, 39, 0.08);
}

.selection-box {
  position: absolute;
  pointer-events: none;
  border: 2px solid #00a884;
  background: rgba(0, 168, 132, 0.12);
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.9);
}
```

- [ ] **Step 4: Verify frontend typecheck and tests**

Run:

```bash
cd crates/rollshot-app && npm run typecheck
```

Expected: PASS.

Run:

```bash
cd crates/rollshot-app && npm test
```

Expected: PASS.

- [ ] **Step 5: Verify Rust app still checks**

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected: PASS.

- [ ] **Step 6: Commit Task 5**

Run:

```bash
rtk git add crates/rollshot-app/src
rtk git commit -m "feat(app): show live preview and select region"
```

---

## Task 6: End-To-End Build And Manual Linux Verification

**Files:**
- Modify only files needed to fix failures found by the verification commands in this task.

- [ ] **Step 1: Run Rust quality gates**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: PASS.

Run:

```bash
rtk cargo test
```

Expected: PASS.

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 2: Run frontend quality gates**

Run:

```bash
cd crates/rollshot-app && npm run typecheck
```

Expected: PASS.

Run:

```bash
cd crates/rollshot-app && npm test
```

Expected: PASS.

Run:

```bash
cd crates/rollshot-app && npm run build
```

Expected: PASS.

- [ ] **Step 3: Verify the app binary exists**

Run:

```bash
rtk cargo build -p rollshot-app
```

Expected: PASS and `target/debug/rollshot-app` exists.

- [ ] **Step 4: Verify CLI launch path reaches the app**

Run:

```bash
ROLLSHOT_APP=target/debug/rollshot-app rtk cargo run -p rollshot-cli -- capture --backend auto
```

Expected: The Tauri app starts. Close the app window to end the command.

- [ ] **Step 5: Manually verify Linux Wayland preview and region selection**

On KDE 6 Wayland, run:

```bash
rtk cargo run -p rollshot-cli -- capture
```

Expected:

```text
1. The rollshot GUI starts.
2. Pressing Start opens the portal source picker.
3. After source selection, the preview updates at a bounded cadence.
4. Dragging over the preview draws a selection rectangle.
5. Confirm Region succeeds and reports source-pixel coordinates.
6. No stitching starts and no PNG save UI appears in Plan 2.
```

- [ ] **Step 6: Commit verification fixes**

If verification required code changes, run:

```bash
rtk git add Cargo.toml .gitignore crates/rollshot-app
rtk git add -u crates/rollshot-app
rtk git commit -m "fix(app): complete preview scaffold verification"
```

If no changes were needed, do not create an empty commit.

---

## Self-Review Notes

Spec coverage:

- Tauri v2 app scaffold: Task 1.
- React frontend scaffold: Task 1.
- CLI launch payload parsing in `rollshot-app`: Task 2.
- Capture backend called from Tauri commands: Task 3.
- Bounded-cadence preview polling: Task 5 uses a 160 ms interval.
- Preview frame transfer through binary IPC: Task 3 returns `tauri::ipc::Response`; Task 4 converts `ArrayBuffer` to `Blob`.
- Source-pixel region selection and HiDPI conversion tests: Task 4.
- No full stitching lifecycle: Task 5 and Task 6 explicitly stop at confirmed region.

Deferred to Plan 3:

- Cropping confirmed regions before stitch input.
- Stitch loop, stop semantics for stitch output, final image state, save PNG, copy, and full-screen Linux fallback.

Red flag scan:

- The plan avoids placeholder markers and excludes speculative settings UI, updater, tray, Windows support, auto-scroll, and SharedBuffer work.
