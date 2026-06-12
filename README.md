# rollshot

`rollshot` is a Rust rewrite of the long screenshot workflow described in
`rollshot_mvp_design.md`. The project has a platform-independent stitching
core, fixture-backed capture tests, and a macOS ScreenCaptureKit backend
(platform-default on macOS) built through a `scap`-compatible crate. The
Linux Wayland portal backend is available on systems with ScreenCast portal
and PipeWire support.

## Features

- **Interactive screenshots and scrolling captures**: use the active iced
  desktop app to switch capture modes, drag-select a region, and finish or
  cancel from the overlay.
- **Live stitched preview**: watch the long screenshot grow while scrolling
  inside the selected region.
- **Bidirectional stitching**: the Rust stitching core supports vertical and
  horizontal movement in either direction, ignores duplicate frames, and uses
  multiple matching strategies for difficult content.
- **Native platform capture**: ScreenCaptureKit is the default backend on macOS;
  Linux supports Wayland capture through XDG Desktop Portal and PipeWire, with
  KDE-specific native screenshot integration.
- **Result workflow**: completed captures are auto-saved as PNG files and opened
  in a result workspace with zoom, pan, Save As, and reveal-in-file-manager
  controls. macOS also presents a draggable floating thumbnail after capture.
- **Headless and debugging CLI**: capture and stitch without the GUI, inspect
  backend availability with `probe`, stitch pre-recorded frame folders, dump
  captured frames, and write matcher debug reports.

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

## Runtime Diagnostics

Rollshot uses structured diagnostic logging through the `tracing` ecosystem.
Release builds retain debug and trace events; they are enabled at runtime
through `RUST_LOG`.

### Quick Start

```bash
# Default: warnings and errors on the console.
rollshot-app

# Debug all Rollshot subsystems on the console.
RUST_LOG=warn,rollshot=debug rollshot-app

# Capture-only diagnostics in an explicit JSONL file and on the console.
RUST_LOG=warn,rollshot::capture=debug rollshot-app --log-file ./rollshot-debug.jsonl

# Stitch decisions plus detailed matcher events.
RUST_LOG=warn,rollshot::stitch=debug,rollshot::stitch::matcher=trace rollshot-app --log-file ./rollshot-debug.jsonl
```

### `--log-file <PATH>`

Writes the same diagnostic session to a JSONL file alongside console output.
The file is truncated per launch. Parent directories must exist. Console output
remains enabled when file output is active. Normally combined with `RUST_LOG`
to enable the desired diagnostic level.

### Stable Diagnostic Targets

These target names are stable support controls. Use them in `RUST_LOG`
directives to enable diagnostics for specific subsystems:

| Target | Scope |
| --- | --- |
| `rollshot::app` | Product launch, completion, and top-level failures |
| `rollshot::app::filter` | Invalid `RUST_LOG` directive warnings (additive) |
| `rollshot::overlay` | Shared iced overlay lifecycle and interaction state |
| `rollshot::capture` | Backend-independent capture decisions and outcomes |
| `rollshot::capture::linux::portal` | Linux portal lifecycle and negotiation |
| `rollshot::capture::linux::pipewire` | Linux PipeWire stream and frame handling |
| `rollshot::capture::macos::sck` | macOS ScreenCaptureKit capture behavior |
| `rollshot::stitch` | Stitch session lifecycle and outcomes |
| `rollshot::stitch::matcher` | Candidate search and match selection |
| `rollshot::stitch::verifier` | Match verification decisions |
| `rollshot::stitch::canvas` | Canvas growth, append, and memory behavior |
| `rollshot::save` | Auto-save, explicit save, and result handoff |

Parent directives enable events from child targets. For example,
`rollshot::stitch=debug` enables debug events from `rollshot::stitch`,
`rollshot::stitch::matcher`, `rollshot::stitch::verifier`, and
`rollshot::stitch::canvas`.

### Privacy

Diagnostic events intentionally omit captured pixels, full save paths, and
environment dumps. Logs contain only safe fields: dimensions, coordinates,
backend names, enum variants, counts, durations, scores, and error categories.

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

### KDE Native Capture Permission

KDE Plasma restricts both the KWin `ScreenShot2` DBus interface and the
`zkde_screencast_unstable_v1` Wayland protocol. KWin authorizes a caller by
reading `/proc/<pid>/exe`, then searching installed desktop entries for one
whose `Exec` first token canonicalizes to that exact executable path and
declares both required keys:

```ini
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1
```

There is **no `PATH` lookup** — the `Exec` path must be the absolute path of
the running binary. Without a matching entry, KWin returns
`org.kde.KWin.ScreenShot2.Error.NoAuthorized` or denies Wayland screencast.

`packaging/linux/dev.rollshot.io.desktop` declares both interfaces with
`Exec=/usr/bin/rollshot-app`.

**System install** (binary path matches `Exec`):

```bash
sudo install -Dm755 target/release/rollshot-app /usr/bin/rollshot-app
sudo install -Dm644 packaging/linux/dev.rollshot.io.desktop \
  /usr/share/applications/dev.rollshot.io.desktop
```

**Local/dev install** (no root): install the binary under `~/.local/bin` and
rewrite `Exec` to that absolute path so it matches `/proc/<pid>/exe`:

```bash
install -Dm755 target/release/rollshot-app ~/.local/bin/rollshot-app
sed "s|^Exec=.*|Exec=$HOME/.local/bin/rollshot-app|" \
  packaging/linux/dev.rollshot.io.desktop \
  > ~/.local/share/applications/dev.rollshot.io.desktop
```

Either way, launch the **installed** binary (the one whose path matches `Exec`),
not a `cargo run` / `target/...` build, or KWin denies the request. You do not
need to launch from the application menu — running the installed binary
directly from a terminal is sufficient.

**Backend selection and fallback behavior.** The `auto` backend resolves to
`linux-kwin` when native authorization succeeds. Running from `cargo run` or
`target/...` normally causes `auto` to fall back to the portal path because
KWin cannot match the binary to a desktop entry. An installed `auto` scrolling
capture should not show a picker. Use `linux-portal` to always test the picker
path, or `linux-kwin` to diagnose native authorization without fallback. Under
`auto`, a portal fallback captures the full source and crops locally — it does
not honor a portal-picked region.

**Verification commands** (after local install):

```bash
~/.local/bin/rollshot-app --capture '{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
~/.local/bin/rollshot-app --capture '{"backend":"linux-kwin","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
~/.local/bin/rollshot-app --capture '{"backend":"linux-portal","fps":5,"show_cursor":false,"initial_mode":"scrolling"}'
```

**One-shot dev run.** This single command builds the release binary, installs
it to `~/.local/bin`, registers a desktop entry whose `Exec` points at that
absolute path, refreshes the desktop database, then launches the installed
binary in screenshot mode — copy-paste it from the repo root to capture on KDE:

```bash
cargo build --release -p rollshot-app \
  && install -Dm755 target/release/rollshot-app ~/.local/bin/rollshot-app \
  && sed "s|^Exec=.*|Exec=$HOME/.local/bin/rollshot-app|" packaging/linux/dev.rollshot.io.desktop \
       > ~/.local/share/applications/dev.rollshot.io.desktop \
  && update-desktop-database ~/.local/share/applications 2>/dev/null; \
  ~/.local/bin/rollshot-app --capture '{"backend":"auto","fps":30,"show_cursor":false,"initial_mode":"screenshot"}'
```

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
