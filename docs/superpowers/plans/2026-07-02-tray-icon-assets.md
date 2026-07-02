# Tray Icon Assets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Rollshot's normal Linux and macOS daemon tray presentation with the provided Rollshot tray icon while leaving recording tray states unchanged.

**Architecture:** Keep tray artwork in a root-level `assets/tray/` tree. Generate deterministic PNG, hicolor, macOS, and Windows outputs from the canonical SVG with a Python script. Embed the runtime PNG into `rollshot-app`, decode it once per tray startup, and adapt it to `tray-icon` RGBA on macOS and `ksni` ARGB pixmap on Linux.

**Tech Stack:** Rust 2021, `image` crate, `tray-icon` 0.24 on macOS, `ksni` 0.3 on Linux, Python 3, CairoSVG 2.9.0, Pillow 12.3.0.

---

## File Structure

- Create: `assets/tray/source/rollshot-tray-normal.svg`
  - Canonical hand-authored normal tray source copied from the provided `rollshot_tray_icon_smooth.svg`.
- Create generated outputs under `assets/tray/generated/`
  - Script-owned runtime and packaging outputs. The implementation commits generated files that product code embeds.
- Create: `scripts/generate_tray_icons.py`
  - Deterministic generator for runtime PNG, hicolor PNG/SVG, macOS iconset/icns when `iconutil` is available, and Windows ico.
- Create: `scripts/requirements-tray-icons.txt`
  - Pinned Python dependencies for reproducible local regeneration.
- Create: `crates/rollshot-app/src/daemon/tray_icon.rs`
  - Shared normal daemon tray icon decoding and format conversion.
- Modify: `crates/rollshot-app/src/daemon/mod.rs`
  - Expose the shared tray icon helper on Linux/macOS.
- Modify: `crates/rollshot-app/src/daemon/linux/tray.rs`
  - Store a generated `ksni::Icon` and expose it through `icon_pixmap()`.
- Modify: `crates/rollshot-app/src/daemon/macos/tray.rs`
  - Use `tray_icon::Icon` instead of title-only status item.

## Task 1: Establish Asset Tree And Generator

**Files:**
- Create: `assets/tray/source/rollshot-tray-normal.svg`
- Create: `scripts/generate_tray_icons.py`
- Create: `scripts/requirements-tray-icons.txt`
- Generate: `assets/tray/generated/runtime/rollshot-tray-normal-32.png`
- Generate: `assets/tray/generated/hicolor/...`
- Generate: `assets/tray/generated/macos/...`
- Generate: `assets/tray/generated/windows/rollshot.ico`

- [ ] **Step 1: Move the provided SVG into the canonical source path**

Run:

```bash
rtk mkdir -p assets/tray/source
rtk mv rollshot_tray_icon_smooth.svg assets/tray/source/rollshot-tray-normal.svg
```

Expected: `assets/tray/source/rollshot-tray-normal.svg` exists and the root-level `rollshot_tray_icon_smooth.svg` no longer exists.

- [ ] **Step 2: Add pinned generator dependencies**

Create `scripts/requirements-tray-icons.txt`:

```text
CairoSVG==2.9.0
Pillow==12.3.0
```

Expected: the requirements file pins the SVG rasterizer and ICO writer used by the generator.

- [ ] **Step 3: Add the generator script**

Create `scripts/generate_tray_icons.py`:

```python
#!/usr/bin/env python3
from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

try:
    import cairosvg
except ModuleNotFoundError as error:
    raise SystemExit(
        "Missing Python dependency: cairosvg. Install generator dependencies with "
        "`python3 -m pip install --user -r scripts/requirements-tray-icons.txt`."
    ) from error

try:
    from PIL import Image
except ModuleNotFoundError as error:
    raise SystemExit(
        "Missing Python dependency: Pillow. Install generator dependencies with "
        "`python3 -m pip install --user -r scripts/requirements-tray-icons.txt`."
    ) from error


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "tray" / "source" / "rollshot-tray-normal.svg"
GENERATED = ROOT / "assets" / "tray" / "generated"
RUNTIME = GENERATED / "runtime"
HICOLOR = GENERATED / "hicolor"
MACOS = GENERATED / "macos"
WINDOWS = GENERATED / "windows"

RUNTIME_SIZE = 32
HICOLOR_SIZES = (16, 22, 24, 32, 48, 64, 128, 256)
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
MACOS_ICONSET = (
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
)


def source_svg() -> bytes:
    if not SOURCE.exists():
        raise SystemExit(f"Missing source SVG: {SOURCE}")
    text = SOURCE.read_text(encoding="utf-8")
    text = text.replace("currentColor", "#000000")
    return text.encode("utf-8")


def render_png(svg: bytes, size: int, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    cairosvg.svg2png(
        bytestring=svg,
        write_to=str(output),
        output_width=size,
        output_height=size,
    )


def reset_generated() -> None:
    if GENERATED.exists():
        shutil.rmtree(GENERATED)
    RUNTIME.mkdir(parents=True)
    HICOLOR.mkdir(parents=True)
    MACOS.mkdir(parents=True)
    WINDOWS.mkdir(parents=True)


def generate_runtime(svg: bytes) -> None:
    render_png(svg, RUNTIME_SIZE, RUNTIME / "rollshot-tray-normal-32.png")


def generate_hicolor(svg: bytes) -> None:
    for size in HICOLOR_SIZES:
        render_png(svg, size, HICOLOR / f"{size}x{size}" / "apps" / "rollshot.png")
    scalable = HICOLOR / "scalable" / "apps" / "rollshot.svg"
    scalable.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(SOURCE, scalable)


def generate_macos(svg: bytes) -> None:
    iconset = MACOS / "rollshot.iconset"
    iconset.mkdir(parents=True, exist_ok=True)
    for filename, size in MACOS_ICONSET:
        render_png(svg, size, iconset / filename)

    iconutil = shutil.which("iconutil")
    if iconutil is None:
        print("iconutil not found; generated macOS iconset without rollshot.icns", file=sys.stderr)
        return

    subprocess.run(
        [iconutil, "-c", "icns", "-o", str(MACOS / "rollshot.icns"), str(iconset)],
        check=True,
    )


def generate_windows() -> None:
    source = HICOLOR / "256x256" / "apps" / "rollshot.png"
    image = Image.open(source).convert("RGBA")
    image.save(WINDOWS / "rollshot.ico", sizes=[(size, size) for size in ICO_SIZES])


def main() -> int:
    svg = source_svg()
    reset_generated()
    generate_runtime(svg)
    generate_hicolor(svg)
    generate_macos(svg)
    generate_windows()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 4: Make the script executable**

Run:

```bash
rtk chmod +x scripts/generate_tray_icons.py
```

Expected: `rtk ls -l scripts/generate_tray_icons.py` shows executable bits for the owner.

- [ ] **Step 5: Install missing local Python generator dependencies if needed**

Run:

```bash
rtk python3 -m pip install --user -r scripts/requirements-tray-icons.txt
```

Expected: pip exits 0. If both pinned packages are already installed, pip reports they are satisfied.

- [ ] **Step 6: Generate tray assets**

Run:

```bash
rtk python3 scripts/generate_tray_icons.py
```

Expected:

- `assets/tray/generated/runtime/rollshot-tray-normal-32.png`
- `assets/tray/generated/hicolor/16x16/apps/rollshot.png`
- `assets/tray/generated/hicolor/22x22/apps/rollshot.png`
- `assets/tray/generated/hicolor/24x24/apps/rollshot.png`
- `assets/tray/generated/hicolor/32x32/apps/rollshot.png`
- `assets/tray/generated/hicolor/48x48/apps/rollshot.png`
- `assets/tray/generated/hicolor/64x64/apps/rollshot.png`
- `assets/tray/generated/hicolor/128x128/apps/rollshot.png`
- `assets/tray/generated/hicolor/256x256/apps/rollshot.png`
- `assets/tray/generated/hicolor/scalable/apps/rollshot.svg`
- `assets/tray/generated/macos/rollshot.iconset/icon_16x16.png`
- `assets/tray/generated/windows/rollshot.ico`

On macOS machines with `iconutil`, `assets/tray/generated/macos/rollshot.icns` also exists.
On non-macOS machines without `iconutil`, stderr includes `iconutil not found; generated macOS iconset without rollshot.icns`.

- [ ] **Step 7: Inspect generated runtime PNG**

Run:

```bash
rtk python3 -c 'from PIL import Image; p="assets/tray/generated/runtime/rollshot-tray-normal-32.png"; im=Image.open(p); print(im.size, im.mode)'
```

Expected: `(32, 32) RGBA`

- [ ] **Step 8: Commit asset source, generator, and generated outputs**

Run:

```bash
rtk git add assets/tray scripts/generate_tray_icons.py scripts/requirements-tray-icons.txt
rtk git commit -m "feat: add generated tray icon assets"
```

Expected: commit succeeds and includes only the asset tree, generator script, and pinned requirements file.

## Task 2: Add Shared Tray Icon Decoder

**Files:**
- Create: `crates/rollshot-app/src/daemon/tray_icon.rs`
- Modify: `crates/rollshot-app/src/daemon/mod.rs`

- [ ] **Step 1: Add a failing helper test first**

Create `crates/rollshot-app/src/daemon/tray_icon.rs` with tests and function signatures:

```rust
const NORMAL_TRAY_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tray/generated/runtime/rollshot-tray-normal-32.png"
));

pub(crate) fn normal_tray_rgba() -> Result<(u32, u32, Vec<u8>), String> {
    Err("normal tray icon decoding is not implemented".into())
}

#[cfg(target_os = "macos")]
pub(crate) fn normal_tray_icon() -> Result<tray_icon::Icon, String> {
    let (width, height, rgba) = normal_tray_rgba()?;
    tray_icon::Icon::from_rgba(rgba, width, height)
        .map_err(|error| format!("failed to create Rollshot tray icon: {error}"))
}

#[cfg(target_os = "linux")]
pub(crate) fn normal_ksni_icon() -> Result<ksni::Icon, String> {
    let (width, height, mut data) = normal_tray_rgba()?;
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }
    Ok(ksni::Icon {
        width: width as i32,
        height: height as i32,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_tray_png_decodes_with_expected_size() {
        let (width, height, rgba) = normal_tray_rgba().unwrap();
        assert_eq!((width, height), (32, 32));
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn normal_ksni_icon_converts_rgba_to_argb() {
        let (_width, _height, rgba) = normal_tray_rgba().unwrap();
        let icon = normal_ksni_icon().unwrap();
        let rgba_pixel = rgba.chunks_exact(4).find(|pixel| pixel[3] > 0).unwrap();
        let argb_pixel = icon
            .data
            .chunks_exact(4)
            .find(|pixel| pixel[0] > 0)
            .unwrap();
        assert_eq!(argb_pixel, [rgba_pixel[3], rgba_pixel[0], rgba_pixel[1], rgba_pixel[2]]);
    }
}
```

- [ ] **Step 2: Expose the helper module**

Modify `crates/rollshot-app/src/daemon/mod.rs` near the existing module declarations:

```rust
pub mod config;
pub mod core;
pub mod instance;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod tray_icon;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod process;
```

- [ ] **Step 3: Run the focused failing test**

Run:

```bash
rtk cargo test -p rollshot-app daemon::tray_icon::tests::normal_tray_png_decodes_with_expected_size
```

Expected: FAIL with `normal tray icon decoding is not implemented`.

- [ ] **Step 4: Implement PNG decoding**

Replace `normal_tray_rgba()` in `crates/rollshot-app/src/daemon/tray_icon.rs`:

```rust
pub(crate) fn normal_tray_rgba() -> Result<(u32, u32, Vec<u8>), String> {
    let image = image::load_from_memory_with_format(NORMAL_TRAY_PNG, image::ImageFormat::Png)
        .map_err(|error| format!("failed to decode embedded Rollshot tray icon: {error}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}
```

- [ ] **Step 5: Run focused helper tests**

Run:

```bash
rtk cargo test -p rollshot-app daemon::tray_icon::tests
```

Expected: PASS on Linux. On macOS, the Linux-only ARGB test is skipped and the PNG decode test passes.

- [ ] **Step 6: Commit helper**

Run:

```bash
rtk git add crates/rollshot-app/src/daemon/mod.rs crates/rollshot-app/src/daemon/tray_icon.rs
rtk git commit -m "feat: embed normal tray icon"
```

Expected: commit succeeds with the helper and module declaration.

## Task 3: Wire Normal Icon Into Linux Daemon Tray

**Files:**
- Modify: `crates/rollshot-app/src/daemon/linux/tray.rs`

- [ ] **Step 1: Update Linux tray tests to require a pixmap**

Modify the test module in `crates/rollshot-app/src/daemon/linux/tray.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_icon() -> ksni::Icon {
        ksni::Icon {
            width: 1,
            height: 1,
            data: vec![255, 0, 0, 0],
        }
    }

    #[test]
    fn capture_menu_item_sends_capture_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx, test_icon());
        item.activate_capture();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::CaptureRegion));
    }

    #[test]
    fn quit_menu_item_sends_quit_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx, test_icon());
        item.activate_quit();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::Quit));
    }

    #[test]
    fn menu_contains_only_capture_and_quit() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let item = DaemonTrayItem::new(tx, test_icon());
        let menu = ksni::Tray::menu(&item);
        let labels: Vec<&str> = menu
            .iter()
            .map(|item| match item {
                ksni::MenuItem::Standard(item) => item.label.as_str(),
                _ => panic!("daemon tray only uses standard items"),
            })
            .collect();
        assert_eq!(labels, ["Capture Region", "Quit Rollshot"]);
    }

    #[test]
    fn tray_exposes_embedded_icon_pixmap() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let item = DaemonTrayItem::new(tx, test_icon());
        let pixmaps = ksni::Tray::icon_pixmap(&item);
        assert_eq!(pixmaps.len(), 1);
        assert_eq!(pixmaps[0].width, 1);
        assert_eq!(pixmaps[0].height, 1);
        assert_eq!(pixmaps[0].data, [255, 0, 0, 0]);
    }
}
```

- [ ] **Step 2: Run Linux tray tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app daemon::linux::tray::tests
```

Expected: FAIL because `DaemonTrayItem::new` still accepts only `Sender<DaemonEvent>` and `icon_pixmap()` still returns the default empty vector.

- [ ] **Step 3: Store the icon pixmap and load it during startup**

Modify `crates/rollshot-app/src/daemon/linux/tray.rs`:

```rust
use crate::daemon::core::DaemonEvent;
use std::sync::mpsc::Sender;

pub struct DaemonTrayItem {
    events: Sender<DaemonEvent>,
    icon: ksni::Icon,
}

impl DaemonTrayItem {
    pub(crate) fn new(events: Sender<DaemonEvent>, icon: ksni::Icon) -> Self {
        Self { events, icon }
    }

    fn activate_capture(&mut self) {
        let _ = self.events.send(DaemonEvent::CaptureRegion);
    }

    fn activate_quit(&mut self) {
        let _ = self.events.send(DaemonEvent::Quit);
    }
}

impl ksni::Tray for DaemonTrayItem {
    fn id(&self) -> String {
        "rollshot-daemon".into()
    }

    fn title(&self) -> String {
        "Rollshot".into()
    }

    fn icon_name(&self) -> String {
        "rollshot".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Capture Region".into(),
                icon_name: "camera-photo".into(),
                activate: Box::new(Self::activate_capture),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit Rollshot".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(Self::activate_quit),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct TrayGuard {
    handle: ksni::blocking::Handle<DaemonTrayItem>,
}

impl TrayGuard {
    pub fn start(events: Sender<DaemonEvent>) -> Result<Self, String> {
        if !rollshot_linux_desktop::sni_host_available() {
            return Err("KDE StatusNotifierHost is unavailable".into());
        }
        let icon = crate::daemon::tray_icon::normal_ksni_icon()?;
        use ksni::blocking::TrayMethods;
        let handle = DaemonTrayItem::new(events, icon)
            .spawn()
            .map_err(|error| format!("failed to register Rollshot tray: {error}"))?;
        Ok(Self { handle })
    }
}
```

Keep the existing `Drop` implementation unchanged.

- [ ] **Step 4: Run Linux tray tests**

Run:

```bash
rtk cargo test -p rollshot-app daemon::linux::tray::tests
```

Expected: PASS.

- [ ] **Step 5: Commit Linux tray wiring**

Run:

```bash
rtk git add crates/rollshot-app/src/daemon/linux/tray.rs
rtk git commit -m "feat: use embedded icon for linux daemon tray"
```

Expected: commit succeeds and recording tray files remain unmodified.

## Task 4: Wire Normal Icon Into macOS Daemon Tray

**Files:**
- Modify: `crates/rollshot-app/src/daemon/macos/tray.rs`

- [ ] **Step 1: Add a failing macOS tray visual test first**

Add this test in the existing test module in `crates/rollshot-app/src/daemon/macos/tray.rs`:

```rust
#[test]
fn normal_tray_uses_template_icon_without_title() {
    let config = normal_tray_visual_config();
    assert!(config.uses_icon);
    assert!(config.icon_is_template);
    assert!(!config.uses_title);
}
```

- [ ] **Step 2: Run the macOS tray unit tests and verify they fail on macOS**

Run:

```bash
rtk cargo test -p rollshot-app daemon::macos::tray::tests
```

Expected on macOS: FAIL with a missing `normal_tray_visual_config` function. Expected on Linux: this target-specific macOS module is not compiled, so Cargo reports no matching macOS tests; continue to the implementation step and rely on the final macOS runtime check.

- [ ] **Step 3: Add the visual config helper and replace title-only status item with embedded template icon**

Add this helper after `daemon_event_for`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayVisualConfig {
    uses_icon: bool,
    icon_is_template: bool,
    uses_title: bool,
}

fn normal_tray_visual_config() -> TrayVisualConfig {
    TrayVisualConfig {
        uses_icon: true,
        icon_is_template: true,
        uses_title: false,
    }
}
```

Modify the builder section in `TrayGuard::start`:

```rust
        let icon = crate::daemon::tray_icon::normal_tray_icon()?;
        let visual = normal_tray_visual_config();
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Rollshot");
        if visual.uses_icon {
            builder = builder.with_icon(icon);
        }
        if visual.icon_is_template {
            builder = builder.with_icon_as_template(true);
        }
        if visual.uses_title {
            builder = builder.with_title("Rollshot");
        }
        let tray = builder
            .build()
            .map_err(|error| format!("failed to create macOS tray icon: {error}"))?;
```

Remove the old comment and `.with_title("Rollshot")` chain from the title-only status item.

- [ ] **Step 4: Run macOS tray tests again**

Run:

```bash
rtk cargo test -p rollshot-app daemon::macos::tray::tests
```

Expected on macOS: PASS. Expected on Linux: no macOS test binary is built.

- [ ] **Step 5: Commit macOS tray wiring**

Run:

```bash
rtk git add crates/rollshot-app/src/daemon/macos/tray.rs
rtk git commit -m "feat: use embedded icon for macos daemon tray"
```

Expected: commit succeeds and `crates/rollshot-app/src/macos_recording_tray.rs` remains unmodified.

## Task 5: Full Verification And Notes

**Files:**
- Inspect only unless a verification failure points to a specific changed file.

- [ ] **Step 1: Confirm recording tray files were not changed**

Run:

```bash
rtk git diff --stat main...HEAD -- crates/rollshot-app/src/macos_recording_tray.rs crates/rollshot-iced-overlay/src/recording_tray.rs
```

Expected: no output.

- [ ] **Step 2: Run rollshot-app tests**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: PASS.

- [ ] **Step 3: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 4: Run clippy if code changes remain limited and dependencies are available**

Run:

```bash
rtk cargo clippy -p rollshot-app --all-targets -- -D warnings
```

Expected: PASS. If platform target dependencies prevent this on the current OS, record the exact dependency or linker error in the final response.

- [ ] **Step 5: Run Linux daemon tray manual smoke if KDE/SNI is available**

Run:

```bash
rtk cargo run -p rollshot-app -- daemon
```

Expected on Linux/KDE with a StatusNotifierHost: the normal daemon tray appears with the Rollshot icon, the menu still contains **Capture Region** and **Quit Rollshot**, and **Quit Rollshot** exits the daemon. If no SNI host is available, record the exact startup error and mark this runtime check as not run in the final response.

- [ ] **Step 6: Run macOS daemon tray manual smoke on macOS**

Run:

```bash
rtk cargo run -p rollshot-app -- daemon
```

Expected on macOS: the normal menu-bar status item is an icon instead of the `Rollshot` text title, the menu still contains **Capture Region** and **Quit Rollshot**, and **Quit Rollshot** exits the daemon. If the current machine is not macOS, record this runtime check as not run in the final response.

- [ ] **Step 7: Verify generator is deterministic**

Run:

```bash
rtk python3 scripts/generate_tray_icons.py
rtk git diff -- assets/tray/generated
```

Expected: no diff in `assets/tray/generated`.

- [ ] **Step 8: Inspect final status**

Run:

```bash
rtk git status --short
```

Expected: only unrelated pre-existing untracked files remain, or a clean tree if those files were removed by the user outside this plan.

## Plan Review Addendum

### NOT In Scope

- Recording tray icon changes: explicitly deferred because the request keeps recording/recoding states unchanged.
- Installer/package-manager integration for generated hicolor, `.icns`, or `.ico` assets: generated outputs are prepared, but packaging wiring is a follow-up distribution task.
- New tray states beyond normal and existing recording state: no product behavior asks for them.
- Replacing menu item icons: the request targets the system tray/status item, not menu rows.

### What Already Exists

- `crates/rollshot-app/src/daemon/linux/tray.rs` already owns the normal Linux daemon SNI item and menu; the plan reuses it and only adds `icon_pixmap()`.
- `crates/rollshot-app/src/daemon/macos/tray.rs` already owns the normal macOS status item and menu; the plan reuses it and replaces the title-only visual with an embedded icon.
- `crates/rollshot-app/src/macos_recording_tray.rs` and `crates/rollshot-iced-overlay/src/recording_tray.rs` already own recording tray states; the plan intentionally does not reuse or modify them.
- The workspace already depends on the Rust `image` crate in `rollshot-app`; the plan reuses it instead of adding a new runtime decoder.
- `ksni` already supports ARGB32 pixmaps through `Tray::icon_pixmap()`; the plan uses that instead of adding a new Linux tray crate.

### Test Coverage Table

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / generator emits runtime PNG, hicolor assets, iconset, ico | - | ✓ | - | no |
| Task 1 / runtime PNG is 32x32 RGBA | - | ✓ | - | no |
| Task 1 / generator rerun is deterministic | - | ✓ | - | no |
| Task 2 / embedded PNG decodes to non-empty RGBA | ✓ | - | - | no |
| Task 2 / Linux RGBA-to-ARGB conversion | ✓ | - | - | no |
| Task 3 / Linux daemon tray exposes pixmap | ✓ | - | - | no |
| Task 3 / Linux daemon tray menu behavior preserved | ✓ | - | - | no |
| Task 4 / macOS tray visual config uses template icon and no title | ✓ on macOS | - | - | no |
| Task 4 / macOS status item renders as icon in menu bar | - | - | ✓ | yes |
| Task 5 / recording tray files unchanged | - | ✓ | - | no |
| Task 5 / Linux daemon tray runtime smoke | - | - | ✓ | yes |

### Failure Modes

| Codepath | Realistic failure | Covered by test | Handling in plan | User-visible outcome |
|---|---|---|---|---|
| `scripts/generate_tray_icons.py` dependency import | CairoSVG or Pillow missing | Task 1 / Step 5 exercises install path | Script exits with an explicit dependency message | Clear terminal error |
| `scripts/generate_tray_icons.py` source read | Source SVG missing | Task 1 / Step 6 fails before outputs are checked | `source_svg()` raises `SystemExit` with the missing path | Clear terminal error |
| `scripts/generate_tray_icons.py` `iconutil` call | Non-macOS machine lacks `iconutil` | Task 1 / Step 6 expects this on non-macOS | Script prints stderr and still emits iconset PNGs | Clear terminal note; `.icns` absent by design |
| `daemon::tray_icon::normal_tray_rgba()` | Embedded PNG corrupt or missing at compile time | Task 2 / Step 5 decodes embedded PNG | Compile fails if missing; decode returns `Err(String)` if corrupt | Daemon startup fails with clear error |
| `daemon::tray_icon::normal_ksni_icon()` | RGBA-to-ARGB conversion regresses | Task 2 / Step 5 Linux ARGB test | Test guards byte order | Caught before release |
| Linux `DaemonTrayItem::icon_pixmap()` | SNI host ignores pixmap and falls back to icon name | Task 3 covers pixmap data, Task 5 manual smoke covers runtime | `icon_name()` remains `rollshot` and generated hicolor assets are available for packaging | Possible generic/missing icon until packaging installs hicolor assets |
| macOS `TrayIconBuilder` startup | `tray-icon` fails to create the status item | Existing `Result<Self, String>` path preserved in Task 4 | `map_err` returns startup error | Clear daemon startup error |

Critical gaps: none. The macOS rendered icon is manual-only because the active macOS status item cannot be meaningfully asserted from Linux CI.

### Worktree / Subagent Parallelization Strategy

Sequential execution, no parallelization opportunity. Task 1 produces assets required by Task 2, Task 2 creates the helper required by Tasks 3 and 4, and Tasks 3 and 4 both depend on the helper plus touch the same `crates/rollshot-app/src/daemon/` module area.
