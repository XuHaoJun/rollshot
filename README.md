# rollshot

`rollshot` is a Rust rewrite of the long screenshot workflow described in
`rollshot_mvp_design.md`. The project has a platform-independent stitching
core, fixture-backed capture tests, and a macOS ScreenCaptureKit backend
(platform-default on macOS) built through a `scap`-compatible crate. The
Linux Wayland portal backend is available on systems with ScreenCast portal
and PipeWire support.

## Workspace

- `crates/rollshot-core`: platform-independent stitching concepts
- `crates/rollshot-capture`: capture traits and frame metadata
- `crates/rollshot-cli`: command-line interface
- `crates/rollshot-app`: iced interactive capture app
- `crates/rollshot-tauri-app`: deprecated Tauri v2 app retained temporarily as
  legacy/reference code during the iced migration

### Desktop app crates during iced migration

- `rollshot-app` is the active iced product app for interactive capture.
- `rollshot-tauri-app` is deprecated and retained temporarily as legacy/reference
  code. It is no longer the default product launch path.
- `rollshot-iced-overlay` is the iced overlay renderer used by the active app.

## Local Development

Install Rust 1.85 or newer with `rustup`.

On Ubuntu, install the system packages required by the PipeWire and D-Bus
dependencies:

```bash
sudo apt-get install -y pkg-config libpipewire-0.3-dev libspa-0.2-dev libclang-18-dev
```

`libclang-18-dev` provides the `libclang.so` symlink that `bindgen` (used by
the `pipewire` crate) needs. Without it, set `LIBCLANG_PATH=/usr/lib/llvm-18/lib`
before building.

### Tauri App

The `rollshot-tauri-app` crate is a Tauri v2 app that needs WebKit, GTK, and X11
development headers on Linux. On macOS it needs Xcode (or Xcode Command Line
Tools) but no extra packages.

This crate is deprecated and retained temporarily for reference during the iced
migration.

On Debian/Ubuntu:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libxdo-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

These packages are not needed for the CLI (`rollshot-cli`) or capture library
(`rollshot-capture`). They are only required to build the deprecated Tauri crate.

### Build & Test

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Useful smoke commands:

```bash
cargo run -p rollshot-cli -- probe
mkdir -p target/test-artifacts
cargo run -p rollshot-cli -- stitch-folder \
  crates/rollshot-core/tests/fixtures/linearscroll_v2/linear_vertical_down/frames \
  --output target/test-artifacts/stitch-folder.png
```

`rollshot capture` prints per-frame progress to stderr by default:
`frame N/MAX: OUTCOME elapsed=SECONDS`. The final capture summary and output
path remain on stdout. Pass `--quiet` to suppress progress output when stderr
must stay empty for scripts.

`stitch-folder` stitches pre-recorded frames without using a capture backend,
which makes it useful for matcher and stitching iteration. Core golden fixtures
live under `crates/rollshot-core/tests/fixtures/linearscroll_v2`.

## Matcher Performance Checks

The ordinary test suite includes a structural matcher budget test for a
retina-sized synthetic frame pair. It checks searched offsets and
full-resolution NCC work instead of wall-clock time, so it is stable across
developer machines and GitHub-hosted runners.

For a release-mode wall-clock smoke check, run:

```bash
cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```

To enforce the current hosted-runner threshold locally or in the manual
`Matcher Perf Smoke` GitHub workflow, set:

```bash
rtk env ROLLSHOT_PERF_STRICT=1 cargo test --release -p rollshot-core large_retina_pair_perf_smoke -- --ignored --nocapture
```

## GitHub Actions

`.github/workflows/ci.yml` runs on `ubuntu-24.04` and `macos-14` for pushes to
`main` and pull requests.

It installs PipeWire/D-Bus development packages and Tauri Linux system
dependencies on the Ubuntu runner, then runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`.github/workflows/matcher-perf.yml` is manual-only and runs the release-mode
large-frame matcher smoke on `ubuntu-24.04`. It complements the deterministic
structural budget test in the normal suite.

Hosted PR CI does not run real desktop capture. KDE Wayland capture needs a real
interactive desktop session, xdg-desktop-portal-kde, PipeWire, and user
selection. macOS ScreenCaptureKit needs Screen Recording permission. Those
conditions belong on manual or self-hosted smoke runners.

## Manual Testing: Bootstrap

Use this checklist after changing workspace, CI, or crate wiring:

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo run -p rollshot-cli -- probe` prints the version, OS, and real capture status.
- [ ] `mkdir -p target/test-artifacts`.
- [ ] `cargo run -p rollshot-cli -- stitch-folder crates/rollshot-core/tests/fixtures/linearscroll_v2/linear_vertical_down/frames --output target/test-artifacts/stitch-folder.png` writes a PNG.
- [ ] `cargo check -p rollshot-tauri-app` passes (requires Tauri Linux deps on Linux, Xcode on macOS).

## Manual Testing: Linux Wayland Portal Capture

Linux capture uses the XDG Desktop Portal ScreenCast interface and PipeWire.
KDE Plasma 6 on Wayland is the first validated target. Other Wayland desktops
can work when their portal implements the standard ScreenCast flow, but
rectangular portal-region picking is desktop-specific.

Install development packages on Debian/Ubuntu:

```bash
sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-18-dev
```

Required services:

- PipeWire (`libpipewire-0.3`)
- WirePlumber or equivalent session manager
- `xdg-desktop-portal`
- a desktop portal implementation such as `xdg-desktop-portal-kde`

Manual checks:

- [ ] `XDG_SESSION_TYPE=wayland`.
- [ ] PipeWire and WirePlumber are running.
- [ ] `xdg-desktop-portal` is running.
- [ ] On KDE, `xdg-desktop-portal-kde` is running.
- [ ] `cargo run -p rollshot-cli -- probe --json` reports `linux-portal` availability.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region portal --max-frames 3 --output target/test-artifacts/linux_portal.png` opens the portal picker and writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region full --max-frames 3 --output target/test-artifacts/linux_full.png` writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region "0,0 900x700" --max-frames 3 --output target/test-artifacts/linux_manual.png` writes a locally cropped PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region portal --max-frames 3 --dump-frames target/test-artifacts/linux-frames --output target/test-artifacts/linux_dumped.png` writes frame dumps.
- [ ] `ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture` captures at least three frames and writes `target/test-artifacts/linux_portal_first_frame.png`.

The smoke test requires a live human-driven desktop session because the portal
picker must be clicked. Hosted CI must not run it.

### KDE Normal Screenshot Permission

KDE Plasma requires a desktop entry that declares the restricted KWin
`ScreenShot2` DBus interface. The file
`packaging/linux/dev.rollshot.io.desktop` contains this declaration. Without
it installed, KWin denies the screenshot request with a permission error.

**User install** (no root required):

```bash
install -Dm644 packaging/linux/dev.rollshot.io.desktop \
  ~/.local/share/applications/dev.rollshot.io.desktop
```

**System install**:

```bash
sudo install -Dm644 packaging/linux/dev.rollshot.io.desktop \
  /usr/share/applications/dev.rollshot.io.desktop
```

After installing the desktop entry, the launched binary must match the `Exec`
identity (`rollshot-app`). Running a development binary directly (e.g.
`cargo run`) may receive an explicit permission error from KWin because the
binary path does not match the registered desktop entry.

#### `initial_mode` JSON

Interactive launch options accept an `initial_mode` field to choose between
scrolling capture and single-screenshot mode:

```json
{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"scrolling"}
{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"screenshot"}
```

The default is `"scrolling"` when the field is omitted.

#### Non-KDE portal screenshot limitations

On non-KDE desktops, rollshot uses the freedesktop Screenshot portal. This
mode has two restrictions:

- **Single-output only.** The portal may return a multi-monitor composite image.
  Rollshot rejects composites that do not match the overlay surface dimensions,
  so only provable single-output results are accepted.
- **No cursor inclusion.** The Screenshot portal has no cursor-inclusion option.
  Passing `show_cursor = true` returns an `Unsupported` error. Use
  `show_cursor = false` (the default).

## Manual Testing: macOS ScreenCaptureKit Capture

Use this checklist after changing the macOS `macos-sck` backend or before
validating a release on macOS:

- [ ] Test machine is running macOS 12.3 or newer.
- [ ] Rust 1.85 or newer is installed.
- [ ] The terminal or test binary has Screen Recording permission:
  `System Settings -> Privacy & Security -> Screen & System Audio Recording`.
- [ ] Main display is visible and unlocked.
- [ ] `mkdir -p target/test-artifacts` creates the artifact directory.
- [ ] `cargo run -p rollshot-cli -- probe --json` reports `macos-sck`.
- [ ] `cargo run -p rollshot-cli -- capture --backend macos-sck --region full --max-frames 3 --output target/test-artifacts/macos_full.png` writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend macos-sck --region "0,0 320x240" --max-frames 3 --output target/test-artifacts/macos_region.png` writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend macos-sck --region "0,0 320x240" --max-frames 3 --dump-frames target/test-artifacts/macos_frames --output target/test-artifacts/macos_region_stitched.png` writes frame dumps.
- [ ] `ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture` passes.
- [ ] `target/test-artifacts/macos_sck_first_frame.png` exists and is visually plausible.

If permission was just granted, restart the terminal before rerunning the
commands. If `probe` reports missing Screen Recording permission, run a capture
command once to trigger the permission prompt, grant access, restart the
terminal, and rerun `probe`.

## Manual Self-Hosted Workflow

`.github/workflows/real-capture.yml` reserves the manual smoke-test path for
self-hosted runners:

- Linux runner labels: `self-hosted`, `linux`, `kde6`, `wayland`
- macOS runner labels: `self-hosted`, `macos`, `screencapturekit`

Run it from GitHub Actions with `workflow_dispatch`. In bootstrap phase the jobs
run the ignored real-capture smoke tests on machines with the required desktop
permissions.
