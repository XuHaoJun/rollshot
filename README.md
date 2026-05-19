# rollshot

`rollshot` is a Rust rewrite of the long screenshot workflow described in
`rollshot_mvp_design.md`. The project is in bootstrap phase: the workspace,
crate boundaries, CI, and tests exist, while real KDE Wayland and macOS capture
backends are not available yet.

## Workspace

- `crates/rollshot-core`: platform-independent stitching concepts
- `crates/rollshot-capture`: capture traits and frame metadata
- `crates/rollshot-cli`: command-line interface
- `crates/rollshot-app`: future app entry point

## Local Development

Install a stable Rust toolchain with `rustup`, then run:

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

## Manual Testing: Future Linux KDE Wayland Capture

Use this checklist when the Linux backend phase adds real tests:

- [ ] Test machine is running KDE Plasma 6 on Wayland.
- [ ] `XDG_SESSION_TYPE=wayland`.
- [ ] `XDG_CURRENT_DESKTOP` mentions KDE or Plasma.
- [ ] PipeWire is running.
- [ ] WirePlumber is running.
- [ ] `xdg-desktop-portal` is running.
- [ ] `xdg-desktop-portal-kde` is running.
- [ ] `rollshot probe` reports portal and PipeWire availability.
- [ ] Portal source picker opens.
- [ ] Rectangular Region selection returns frames.
- [ ] At least three frames are captured.
- [ ] Captured frames have non-zero width and height.
- [ ] The first frame can be saved under `target/test-artifacts/`.

Expected future command:

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture
```

## Manual Testing: Future macOS ScreenCaptureKit Capture

Use this checklist when the macOS backend phase adds real tests:

- [ ] Test runner has Screen Recording permission.
- [ ] Main display is visible and unlocked.
- [ ] `rollshot probe` reports macOS capture status.
- [ ] A small manual region can be selected or configured.
- [ ] At least three frames are captured.
- [ ] Captured frames have non-zero width and height.
- [ ] BGRA to RGBA conversion is visually correct.
- [ ] The first frame can be saved under `target/test-artifacts/`.

Expected future command:

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

## Manual Self-Hosted Workflow

`.github/workflows/real-capture.yml` reserves the manual smoke-test path for
self-hosted runners:

- Linux runner labels: `self-hosted`, `linux`, `kde6`, `wayland`
- macOS runner labels: `self-hosted`, `macos`, `screencapturekit`

Run it from GitHub Actions with `workflow_dispatch`. In bootstrap phase the jobs
only explain that real backend smoke tests are added in later backend phases.
