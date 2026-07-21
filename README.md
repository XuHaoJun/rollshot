# rollshot

`rollshot` is a Rust rewrite of the long screenshot workflow described in
`docs/rollshot_mvp_design.md`. The project has a platform-independent stitching
core, fixture-backed capture tests, and a macOS ScreenCaptureKit backend
(platform-default on macOS) built through a `scap`-compatible crate. The
Linux Wayland portal backend is available on systems with ScreenCast portal
and PipeWire support.

## Features

- **Scrolling screenshots**: capture a selected region and watch the stitched
  image grow live as you scroll.
- **Regular screenshots**: capture a region or fullscreen shot from the app,
  tray, or global shortcut where supported.
- **Native macOS and Linux capture**: uses ScreenCaptureKit on macOS and
  Wayland portal/PipeWire or KDE-native capture on Linux.
- **In-app editing**: zoom, pan, copy, save, reveal, annotate with
  numbers or text, and redact sensitive areas with safe export controls.
- **Smart Redaction**: use an LLM agent to propose redactions, then save the
  reviewed logic as a reusable code preset for repeatable edits.
- **Action Guide recording**: optionally record a desktop workflow, edit the
  detected steps, and export a guide or GIF.

## Workspace

- `crates/rollshot-core`: platform-independent stitching (matcher, canvas,
  verifier, metrics, `Stitcher`)
- `crates/rollshot-capture`: capture traits, frame metadata, and the Linux
  portal/PipeWire/KWin and macOS ScreenCaptureKit backends
- `crates/rollshot-dev`: internal developer diagnostics and offline
  `stitch-folder` tooling
- `crates/rollshot-app`: iced interactive capture app and post-capture result
  workspace
- `crates/rollshot-iced-overlay`: iced capture overlay renderer used by `rollshot-app`
- `crates/rollshot-overlay-core`: framework-neutral overlay logic shared by the overlay crates
- `crates/rollshot-image-document`: headless non-destructive image/annotation document engine
- `crates/rollshot-edit-proposal`: typed candidate-edit proposals, review decisions,
  and lowering to image-document operations
- `crates/rollshot-automation`: restricted-JavaScript validation, Workflow IR,
  capability contracts, and strict proposal decoding
- `crates/rollshot-automation-rquickjs`: hardened QuickJS executor for validated
  automation; currently internal infrastructure and not yet wired into the product UI
- `crates/rollshot-agent`: provider-neutral bounded agent control plane (Bounded
  Agent Core) — in-memory agent sessions/runs, Anthropic/OpenAI streaming
  adapters, typed tool registry, run budgets and cancellation, and run-local
  automation draft orchestration. Currently internal infrastructure and not yet
  wired into the product UI
- `crates/rollshot-macos-oneshot`: isolated macOS ScreenCaptureKit one-shot capture (Objective-C FFI)
- `crates/rollshot-ocr`: unsafe-isolation crate for RapidOCR/ONNX Runtime OCR (bundled PP-OCRv4 models, safe API, excluded from default workspace builds)
- `crates/rollshot-vision`: visual inspection host (template matching, region features, OCR behind the off-by-default `ocr` feature)
- `crates/rollshot-action`, `crates/rollshot-linux-input`, `crates/rollshot-macos-input`:
  Action Guide recording and platform input observation (built behind the
  `action-guide` feature)

## Local Development

Install Rust 1.94 or newer with `rustup`.

On Ubuntu, install the system packages required by the PipeWire and D-Bus
dependencies:

```bash
sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-18-dev \
  libdbus-1-dev libxkbcommon-dev
```

`libclang-18-dev` provides the `libclang.so` symlink that `bindgen` (used by
the `pipewire` crate) needs. Without it, set `LIBCLANG_PATH=/usr/lib/llvm-18/lib`
before building. `libdbus-1-dev` and `libxkbcommon-dev` are required by the iced
desktop app on Linux. (CI installs `libclang-dev`, which pulls the distro
default LLVM; `libclang-18-dev` pins version 18.)

### Build & Test

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Useful developer-tool commands:

```bash
cargo run -p rollshot-dev -- probe
mkdir -p target/test-artifacts
cargo run -p rollshot-dev -- stitch-folder \
  crates/rollshot-core/tests/fixtures/linearscroll_v2/linear_vertical_down/frames \
  --output target/test-artifacts/stitch-folder.png
```

`rollshot-dev` is not a product entry point and does not start capture.
`stitch-folder` works only with existing image frames.

`stitch-folder` stitches pre-recorded frames without using a capture backend,
which makes it useful for matcher and stitching iteration. Core golden fixtures
live under `crates/rollshot-core/tests/fixtures/linearscroll_v2`.

## Smart Redaction Eval Harness

`docs/smart-redaction-eval.md` documents the two-layer evaluation gate for the
smart-redaction pipeline — fixture layout, recording cassettes from a live model,
scoring thresholds, and the deferred six-fixture seeding workflow.

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
| `rollshot::capture::linux::kwin` | Linux KWin native screencast authorization and capture |
| `rollshot::capture::linux::pipewire` | Linux PipeWire stream and frame handling |
| `rollshot::capture::macos::sck` | macOS ScreenCaptureKit capture behavior |
| `rollshot::stitch` | Stitch session lifecycle and outcomes |
| `rollshot::stitch::matcher` | Candidate search and match selection |
| `rollshot::stitch::verifier` | Match verification decisions |
| `rollshot::stitch::canvas` | Canvas growth, append, and memory behavior |
| `rollshot::save` | Auto-save, explicit save, and result handoff |
| `rollshot::app::quick_ocr` | Region text capture lifecycle (assembly, clipboard, feedback) |

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

It installs PipeWire/D-Bus development packages on the Ubuntu runner, then runs:

```bash
./scripts/check-tracing-targets.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

It then repeats clippy and the test suite with the `action-guide` feature
enabled, and runs per-crate `cargo check` for the macOS-specific crates on the
macOS runner:

```bash
cargo clippy --workspace --all-targets --features rollshot-app/action-guide -- -D warnings
cargo test --workspace --features rollshot-app/action-guide
```

`.github/workflows/matcher-perf.yml` is manual-only and runs the release-mode
large-frame matcher smoke on `ubuntu-24.04`. It complements the deterministic
structural budget test in the normal suite.

`.github/workflows/ci-ocr.yml` is a separate path-filtered lane for the OCR
backend (`rollshot-ocr` / `rollshot-vision --features ocr`). It is excluded from
default CI because it provisions a static ONNX Runtime library and downloads
RapidOCR models. It runs on `ubuntu-24.04` and `macos-14`.

### Known issue: macOS ONNX Runtime teardown abort

On `macos-14`, an OCR test binary aborts with `libc++abi: ... mutex lock failed:
Invalid argument` (SIGABRT) **after all tests pass**, when it returns from `main`
and runs ONNX Runtime's C++ static destructors at process exit. This is an
upstream ONNX Runtime 1.21+ bug ([pykeio/ort#409](https://github.com/pykeio/ort/issues/409),
[microsoft/onnxruntime#24579](https://github.com/microsoft/onnxruntime/issues/24579)),
fixed in `ort >= 2.0.0-rc.11` — which we cannot adopt because rc.11 requires
`ndarray 0.17` while `paddle-ocr-rs` pins `ndarray ^0.16`.

The crash is harmless and test-only: the tests themselves pass, and the product
(`rollshot-app`) never hits it because it exits via `process::exit` while the OCR
engine is still live, so the teardown path never runs. Accordingly the macOS OCR
test steps run through `scripts/ci/run-ocr-test.sh`, which tolerates a non-zero
exit **only** when the test summary shows tests ran and passed; any real test
failure still fails the job.

Hosted PR CI does not run real desktop capture. KDE Wayland capture needs a real
interactive desktop session, xdg-desktop-portal-kde, PipeWire, and user
selection. macOS ScreenCaptureKit needs Screen Recording permission. Those
conditions belong on manual or self-hosted smoke runners.

## Manual Testing: Bootstrap

Use this checklist after changing workspace, CI, or crate wiring:

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo run -p rollshot-dev -- probe` prints the OS and real capture status.
- [ ] `mkdir -p target/test-artifacts`.
- [ ] `cargo run -p rollshot-dev -- stitch-folder crates/rollshot-core/tests/fixtures/linearscroll_v2/linear_vertical_down/frames --output target/test-artifacts/stitch-folder.png` writes a PNG.


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
- [ ] `cargo run -p rollshot-dev -- probe --json` reports `linux-portal` availability.
- [ ] `cargo run -p rollshot-app -- capture --backend linux-portal --workflow scrolling --scope region` opens the portal picker and writes a PNG.
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
~/.local/bin/rollshot-app capture --backend auto --fps 5
~/.local/bin/rollshot-app capture --backend linux-kwin --fps 5
~/.local/bin/rollshot-app capture --backend linux-portal --fps 5
```

**One-shot dev run.** This single command builds the release binary, installs
it to `~/.local/bin`, registers a desktop entry whose `Exec` points at that
absolute path, refreshes the desktop database, then launches the installed
binary in region mode — copy-paste it from the repo root to capture on KDE:

```bash
cargo build --release -p rollshot-app \
  && install -Dm755 target/release/rollshot-app ~/.local/bin/rollshot-app \
  && sed "s|^Exec=.*|Exec=$HOME/.local/bin/rollshot-app|" packaging/linux/dev.rollshot.io.desktop \
       > ~/.local/share/applications/dev.rollshot.io.desktop \
  && update-desktop-database ~/.local/share/applications 2>/dev/null; \
   ~/.local/bin/rollshot-app capture --backend auto --fps 30 --workflow screenshot --scope region
```

#### `--workflow` and `--scope`

`rollshot-app capture` takes two orthogonal axes — **workflow** (what we do with
the frames) and **scope** (what area we capture):

- `--workflow <screenshot|scrolling>` (default `scrolling`)
- `--scope <region|fullscreen>` (default `region`)

```bash
rollshot-app capture --backend auto --fps 5 --workflow scrolling --scope region
rollshot-app capture --backend auto --fps 5 --workflow screenshot --scope region
rollshot-app capture --backend auto --fps 5 --workflow screenshot --scope fullscreen
```

The default is `--workflow scrolling --scope region`; running `rollshot-app`
with no subcommand uses these defaults.

`--workflow scrolling --scope fullscreen` is expressible but not wired — passing
it returns an error.

Fullscreen scope captures the display containing the pointer, skipping the
selection overlay. It is supported on macOS and KDE/KWin. On other Linux
environments without portal fallback, fullscreen returns an `Unsupported` error.

#### Non-KDE portal region-mode limitations

On non-KDE desktops, rollshot uses the freedesktop Screenshot portal for region
captures. This mode has two restrictions:

- **Single-output only.** The portal may return a multi-monitor composite image.
  Rollshot rejects composites that do not match the overlay surface dimensions,
  so only provable single-output results are accepted.
- **No cursor inclusion.** The Screenshot portal has no cursor-inclusion option.
  Passing `show_cursor = true` returns an `Unsupported` error. Use
  `show_cursor = false` (the default).

### System tray daemon (KDE Plasma 6)

Start the daemon:

```bash
rollshot-app daemon
```

The tray provides **Capture Region** and **Quit Rollshot**. The default KDE
global shortcut request is `Alt+Shift+6`; KDE may ask you to approve or replace
it. Region capture starts in Screenshot mode, and the capture toolbar can
switch the selected crop to Scrolling mode.

Optional configuration:

```toml
[daemon]
capture_region_hotkey = "Alt+Shift+6"

[provider]
provider = "Anthropic"
model = "claude-sonnet-4-6"
key_source = { Env = "ANTHROPIC_API_KEY" }
# base_url = "https://api.anthropic.com"
```

Save it as `$XDG_CONFIG_HOME/rollshot/config.toml` (normally
`~/.config/rollshot/config.toml`) and restart the daemon. See
`docs/config.md` for every supported parameter, default, and syntax rule. The
first release targets KDE Plasma 6 on Wayland. If portal shortcut registration
fails, the tray remains usable.

### System tray daemon (macOS)

Run `rollshot-app daemon` to start a menu-bar status item (no Dock icon). The
item's menu has two actions:

- **Capture Region** — opens region selection in Screenshot mode (switch to
  Scrolling from the overlay toolbar after selecting a crop).
- **Quit Rollshot** — terminates any active capture and exits the daemon.

The global shortcut defaults to **Command+Shift+6**. Override it in
`~/Library/Application Support/rollshot/config.toml`:

    [daemon]
    capture_region_hotkey = "Command+Shift+6"

See `docs/config.md` for every supported parameter, default, and syntax rule.
If the shortcut cannot be registered (e.g. another app owns it), the daemon
logs a warning and keeps working through the menu. Starting a second daemon
exits immediately without a second menu item. The shortcut uses Carbon hotkey
registration and does not require Accessibility permission.

### Open an Existing Image

Open one static PNG or JPEG directly in the Result Workspace:

```bash
cargo run -p rollshot-app -- open ./screenshot.png
```

The source image is read-only. Annotation exports use **Save As**, default to
`<source-stem>-annotated.png` beside the source, and are flattened PNG files.
Rollshot refuses destinations that resolve to the imported source.

Opening and annotation work in the default build. Selectable **OCR Text** is
available when `rollshot-app` is built with the `ocr` feature:

```bash
cargo run -p rollshot-app --features ocr -- open ./screenshot.png
```

The first release accepts one local static PNG or JPEG. It does not add a GUI
file picker, drag-and-drop, multi-image tabs, or other image formats.

### Region Text Capture (OCR)

Rollshot can recognize text in a selected screen region and copy it to the
clipboard. OCR is gated behind the `ocr` Cargo feature on `rollshot-app`.

**CLI usage** (direct mode):

```bash
cargo run -p rollshot-app --features ocr -- ocr
```

This opens a region selector, runs OCR on the captured image, writes the
recognized text to stdout (one line per text block, trailing newline), and
copies it to the clipboard. Press Esc to cancel silently — no text is written
to stdout or clipboard. If the selection is empty or no text is recognized,
the clipboard is left untouched.

**Daemon shortcuts:**

| Platform | Region capture | Text capture |
|----------|---------------|--------------|
| Linux (KDE) | `Alt+Shift+6` | `Alt+Shift+7` |
| macOS | `Command+Shift+6` | `Command+Shift+7` |

The text-capture shortcut triggers region selection, runs OCR, and copies the
result to the clipboard. On success, a desktop notification confirms the copy.
On failure, a notification or error dialog reports the failure without
revealing recognized text. The text-capture shortcut is disabled when the
`ocr` feature is not enabled at build time.

**Configuration:**

```toml
[daemon]
capture_text_hotkey = "Alt+Shift+7"   # Linux
# capture_text_hotkey = "Command+Shift+7"  # macOS
```

Both the region and text tray/shortcut entries require an OCR-enabled build.
Without the `ocr` feature, the text shortcut is not registered and the `ocr`
CLI subcommand returns an error.

## Manual Testing: macOS ScreenCaptureKit Capture

Use this checklist after changing the macOS `macos-sck` backend or before
validating a release on macOS:

- [ ] Test machine is running macOS 12.3 or newer.
- [ ] Rust 1.94 or newer is installed.
- [ ] The terminal or test binary has Screen Recording permission:
  `System Settings -> Privacy & Security -> Screen & System Audio Recording`.
- [ ] Main display is visible and unlocked.
- [ ] `mkdir -p target/test-artifacts` creates the artifact directory.
- [ ] `cargo run -p rollshot-dev -- probe --json` reports `macos-sck`.
- [ ] `cargo run -p rollshot-app -- capture --backend macos-sck --workflow scrolling --scope region` writes a PNG.
- [ ] `ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture` passes.
- [ ] `target/test-artifacts/macos_sck_first_frame.png` exists and is visually plausible.

If permission was just granted, restart the terminal before rerunning the
commands. If `probe` reports missing Screen Recording permission, run a capture
command once to trigger the permission prompt, grant access, restart the
terminal, and rerun `probe`.

## Action Guide (optional)

Action Guide is gated behind the non-default `action-guide` Cargo feature on
`rollshot-app`:

```bash
cargo build --release -p rollshot-app --features action-guide
```

Bare `action-guide` opens the **Home** screen — a project browser with recent
projects, a **Record New** button, and an **Open** picker:

```bash
rollshot-app action-guide
```

Use `--record` to start a new recording directly, and `--record --fullscreen`
to capture the whole display instead of selecting a region:

```bash
rollshot-app action-guide --record
rollshot-app action-guide --record --fullscreen
```

Use `--open` to reopen an existing `.rollshot-guide` project directory, or
`--open PATH` to open a specific one:

```bash
rollshot-app action-guide --open
rollshot-app action-guide --open /path/to/my-guide.rollshot-guide
```

### Projects and Safe Copy

New recordings are saved into a `.rollshot-guide` directory — the private,
editable source for your guide. This directory contains the session metadata,
keyframe images, and annotations. Treat it as your working copy.

When you are ready to share, **Save** (or **Save As**) produces a standalone
folder with `index.html`, `steps.md`, `session.json`, and `keyframes/` — the
**Safe Copy** artifact intended for distribution. The Safe Copy folder is
self-contained and works offline.

New recordings default to **save-first** behavior: the project is committed to
disk as soon as the recording finishes, before you enter the editor. You can
also choose **Save later** to defer the first commit until you explicitly save.

### Writer lock and read-only access

Only one process can write to a `.rollshot-guide` project at a time. A second
process opening the same project gets **read-only** access — editing is
disabled, but the guide can still be viewed. If a required keyframe fails to
decode (e.g. a corrupt or truncated image), the workspace downgrades to
read-only and disables Save.

### Legacy export handoff

Older standalone exports (a folder with `index.html` and `session.json` but
without the `.rollshot-guide` internal structure) are detected as legacy
exports. Opening one offers to launch the `index.html` reader in your browser
rather than entering the editor, since legacy exports are not writable project
directories.

### Local filesystem limitation

Action Guide projects are stored on the local filesystem. Network shares,
cloud-synced folders, and other non-local paths are not currently supported for
project storage.

### Input access

Action Guide works in **visual-only** mode out of the box. Granting temporary
input-device access upgrades it to **semantic** detection (clicks, scroll,
typing, Enter/Tab improve step timing). Input is observed **only** while an
Action Guide recording is active, and Rollshot never persists raw key codes,
typed text, device names, or device paths.

### Linux (KDE Wayland and others)

Rollshot reads kernel input devices directly via evdev; it does **not** use
`sudo`, `pkexec`, Polkit, or a privileged daemon. You grant your own user
temporary read access with an ACL.

> ⚠️ **Security warning:** read access to `/dev/input/event*` lets *any* process
> running as your user observe **all** keyboard and pointer activity, including
> passwords typed into other applications. Grant it only while you need it and
> remove it afterward. ACLs may disappear after a reboot or when a device is
> recreated (e.g. replugging a keyboard), and may need to be reapplied.

1. **Identify your input devices:**

   ```bash
   cat /proc/bus/input/devices   # find your keyboard/mouse "Handlers=... eventN"
   # or:
   ls -l /dev/input/by-id/
   ```

2. **Grant your user temporary read access** (replace `eventN`):

   ```bash
   sudo setfacl -m u:$USER:r /dev/input/eventN
   ```

3. **Verify access:**

   ```bash
   getfacl /dev/input/eventN     # should list user:<you>:r--
   # quick read test (Ctrl-C to stop):
   head -c 1 /dev/input/eventN >/dev/null && echo "readable"
   ```

4. **Remove the ACL when done:**

   ```bash
   sudo setfacl -x u:$USER /dev/input/eventN
   ```

If no device is readable, Action Guide stays in visual-only mode and shows a
persistent advisory — recording, detection, review, and export still work.

### macOS

Semantic input uses **Input Monitoring** (System Settings → Privacy & Security →
Input Monitoring). Rollshot requests it just-in-time when an Action Guide
recording starts; it never requests Accessibility or input injection. If you
deny it, Action Guide stays in visual-only mode with an advisory and an **Open
System Settings** shortcut. macOS may require restarting Rollshot before a newly
granted permission takes effect.

**Screen Recording** permission is separate and **required** to capture frames —
denying it is a capture failure, not a visual-only degradation.

### Fullscreen recording (Linux/KDE)

`rollshot-app action-guide --record --fullscreen` records the whole display.
Click the temporary system-tray icon to finish recording. Requires a system tray
(StatusNotifierItem host); KDE Plasma provides one.

### Safe Copy structure

The Safe Copy produced by Save/Save As is a self-contained folder:

```text
<guide-title>-<YYYY-MM-DD-HHMMSS>/
  index.html
  steps.md
  session.json
  keyframes/
```

Recipients double-click `index.html` to open the interactive guide reader in a
browser. The reader works entirely offline — no server, CDN, or network access
is required. The whole folder must be moved or shared together; `index.html`
reads `session.json` and `keyframes/` relative to its own location.

`steps.md` is a plain Markdown fallback that works without JavaScript.

After a successful Save, the timeline workspace shows **Open Guide** (launches
`index.html`) and **Show in Folder** (reveals the directory in the system file
manager).

### Publishing and outputs

When you save a project, Rollshot commits the editable content (project.json,
keyframe assets, annotations) first, then derives published outputs in the
background. Save always succeeds independently of publish — if a publish worker
fails, your editable content is already committed.

Published outputs are split into two tiers:

- **Core** (always on): `index.html`, `steps.md`, `session.json`, and
  `keyframes/`. This is the self-contained Safe Copy tree. Core is mandatory
  and cannot be toggled off.
- **Optional derivatives**: Storyboard (`storyboard.png`), GIF (`guide.gif`),
  and MP4 (`summary.mp4`). Each has an independent toggle in the timeline
  workspace. Toggling an output on schedules it for the next publish; toggling
  it off removes the artifact from future publishes.

Each output tracks its own freshness against the current project revision.
The per-output states are:

| State | Meaning |
| --- | --- |
| Current | Output was built from the current revision. |
| Stale | Output exists but was built from an older revision (or is missing). |
| Updating | A publish worker is running for this output. |
| Failed | The last publish attempt for this output failed. |

The aggregate publish status shows **Published** only when all enabled outputs
are Current. If any enabled output is Stale or Failed, the aggregate status
reflects that. You can retry individual failed outputs or all stale/failed
outputs at once from the timeline workspace.

Closing the project or navigating away cancels any active publish workers.
Cancelled or superseded workers cannot mutate visible state — only the final
committed result is persisted.

**MP4 output** requires ffmpeg. If ffmpeg is not found on `PATH` or at the
`ROLLSHOT_FFMPEG` environment variable, Rollshot offers to download a managed
copy. You can also point `ROLLSHOT_FFMPEG` at an existing installation.

### Sharing

The timeline workspace offers three sharing options with different content and
privacy trade-offs:

| Share option | What it copies | Editable | Privacy |
| --- | --- | --- | --- |
| **Safe Viewer Copy** | Published outputs only (`publish/` tree: index.html, steps.md, session.json, keyframes, optional derivatives) | No | Recipients see only the rendered guide — no internal project state, annotations, or recording metadata. |
| **Editable Project** | Full `.rollshot-guide` directory (project.json, assets, publish-state.json, and the publish tree) | Yes | Recipients can open and edit the guide. Includes all internal state. Shows a confirmation warning before copying. |
| **Issue Pack** | A standalone Markdown report with embedded keyframe images and optional storyboard | No | Designed for filing bug reports or issues. Evidence images are included; internal project state is not. |

Safe Viewer Copy and Editable Project require all enabled outputs to be current
before sharing — if any output is stale, Rollshot publishes first. Issue Pack
does not depend on publish state.

All sharing uses atomic copy operations that never silently overwrite an
existing destination. Temporary artifacts are cleaned up on failure or
cancel.

`.github/workflows/real-capture.yml` reserves the manual smoke-test path for
self-hosted runners:

- Linux runner labels: `self-hosted`, `linux`, `kde6`, `wayland`
- macOS runner labels: `self-hosted`, `macos`, `screencapturekit`

Run it from GitHub Actions with `workflow_dispatch`. In bootstrap phase the jobs
run the ignored real-capture smoke tests on machines with the required desktop
permissions.
