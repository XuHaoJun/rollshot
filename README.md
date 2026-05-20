# rollshot

`rollshot` is a Rust rewrite of the long screenshot workflow described in
`rollshot_mvp_design.md`. The project has a platform-independent stitching
core, fixture-backed capture tests, and a macOS ScreenCaptureKit backend built
through `scap`. The Linux Wayland portal backend is available on systems with
ScreenCast portal and PipeWire support.

## Workspace

- `crates/rollshot-core`: platform-independent stitching concepts
- `crates/rollshot-capture`: capture traits and frame metadata
- `crates/rollshot-cli`: command-line interface
- `crates/rollshot-app`: future app entry point

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

Then run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Useful smoke commands:

```bash
cargo run -p rollshot-cli -- probe
cargo run -p rollshot-cli -- stitch-folder tests/fixtures
```

`stitch-folder` is intentionally a bootstrap smoke command until the stitching
core phase adds image fixtures and golden output tests.

## GitHub Actions

`.github/workflows/ci.yml` runs on `ubuntu-24.04` and `macos-14` for pushes to
`main` and pull requests.

It runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

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
- [ ] `cargo run -p rollshot-cli -- stitch-folder tests/fixtures` exits successfully with bootstrap status text.

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
