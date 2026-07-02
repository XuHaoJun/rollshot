# Tray Icon Assets Design

## Context

Rollshot's normal daemon tray currently uses text or generic themed icons:

- Linux daemon SNI: `crates/rollshot-app/src/daemon/linux/tray.rs` returns the
  theme icon name `camera-photo`.
- macOS daemon tray: `crates/rollshot-app/src/daemon/macos/tray.rs` builds a
  title-only status item with `Rollshot`.
- Recording tray states are separate and remain unchanged:
  - Linux fullscreen Action Guide recording uses `media-record` in
    `crates/rollshot-iced-overlay/src/recording_tray.rs`.
  - macOS Action Guide recording uses `● Rollshot` in
    `crates/rollshot-app/src/macos_recording_tray.rs`.

The provided normal-state source artwork is currently
`rollshot_tray_icon_smooth.svg`. This change moves that artwork into the
project asset tree as the canonical normal tray source. This change only
applies that artwork to the normal daemon tray state.

## Flameshot Reference

Flameshot keeps app and tray artwork under `data/img/app/`, registers them in
`data/graphics.qrc`, and uses Qt's resource system for runtime fallback. Its
tray code builds:

- a theme lookup: `QIcon::fromTheme("flameshot-tray", ...)`
- a bundled fallback from `GlobalValues::trayIconPath()`
- a macOS Big Sur mask/template path for menu bar adaptation

Packaging also installs hicolor icons under `share/icons/hicolor/...` so Linux
desktops can resolve themed icon names.

Rollshot does not use Qt resources. The equivalent runtime pattern is to embed
generated PNG bytes into `rollshot-app` with `include_bytes!`, decode them into
RGBA, and pass them to `tray_icon::Icon::from_rgba`.

## Scope

In scope:

- Add generated normal tray icon assets derived from the canonical SVG source
  in `assets/tray/source/`.
- Add a Python script in `scripts/` that regenerates platform icon outputs.
- Use an embedded generated PNG for the normal macOS daemon tray icon.
- Add the same asset layout needed for Linux hicolor/theme packaging later.
- Keep the Linux daemon tray behavior compatible with the current `ksni` path.

Out of scope:

- Changing recording tray visuals or behavior.
- Adding new tray states.
- Changing tray menu items.
- Implementing installer or package manager integration.
- Replacing the Linux recording tray's `media-record` icon.

## Asset Generation

Add `scripts/generate_tray_icons.py`.

Inputs:

- `assets/tray/source/rollshot-tray-normal.svg`

Outputs:

- Runtime PNG assets under `assets/tray/generated/runtime/`, including a
  normal-state PNG sized for tray use.
- Linux hicolor-ready outputs under `assets/tray/generated/hicolor/`, using
  standard sizes such as 16, 22, 24, 32, 48, 64, 128, 256, and a scalable SVG
  copy.
- macOS `.iconset`/`.icns` output if the available local toolchain supports it;
  otherwise the script should generate `assets/tray/generated/macos/iconset/`
  and explain the missing platform tool.
- Windows `.ico` output under `assets/tray/generated/windows/` if the available
  Python dependencies support it.

The script should fail loudly when required Python dependencies for SVG
rasterization are missing. It should be deterministic and safe to rerun.

## Asset Layout

Use a root-level asset tree so source artwork, generated runtime assets, and
packaging-ready outputs stay together:

```text
assets/
  tray/
    source/
      rollshot-tray-normal.svg
    generated/
      runtime/
        rollshot-tray-normal-32.png
      hicolor/
        16x16/apps/rollshot.png
        22x22/apps/rollshot.png
        24x24/apps/rollshot.png
        32x32/apps/rollshot.png
        48x48/apps/rollshot.png
        64x64/apps/rollshot.png
        128x128/apps/rollshot.png
        256x256/apps/rollshot.png
        scalable/apps/rollshot.svg
      macos/
        rollshot.iconset/
        rollshot.icns
      windows/
        rollshot.ico
```

`assets/tray/source/` is hand-authored input. `assets/tray/generated/` is
script-owned output and may be deleted and recreated by
`scripts/generate_tray_icons.py`.

## Runtime Design

Add a small helper module in `rollshot-app`, for example
`crates/rollshot-app/src/daemon/tray_icon.rs`.

Responsibilities:

- Load the normal tray PNG with `include_bytes!`.
- Decode it with the existing `image` crate.
- Convert it to RGBA bytes and return `tray_icon::Icon`.
- Keep decoding errors local and report them as `String` for daemon startup
  error messages.

macOS daemon tray:

- Replace `.with_title("Rollshot")` with `.with_icon(normal_icon)`.
- Keep `.with_tooltip("Rollshot")`.
- Use `.with_icon_as_template(true)` so macOS can recolor the icon for menu bar
  light/dark backgrounds.
- Do not change menu item ids or event mapping.

The embedded path should point to
`assets/tray/generated/runtime/rollshot-tray-normal-32.png` from the repository
root rather than duplicating assets under `crates/rollshot-app/`.

Linux daemon tray:

- Prefer the generated Rollshot icon if `ksni` can provide pixmap data for the
  tray item without changing the daemon architecture.
- If `ksni` only supports reliable themed-name behavior for this path, keep the
  current SNI behavior and use the generated hicolor assets as packaging-ready
  outputs. In that case, document the remaining packaging step in the final
  implementation notes.

Recording trays:

- Leave existing recording tray code unchanged.

## Testing And Verification

Automated checks:

- Add unit coverage for the asset helper so the embedded PNG decodes and has
  valid non-zero dimensions.
- Preserve existing tray menu tests for Linux and macOS daemon trays.
- Run `rtk cargo test -p rollshot-app`.
- Run `rtk cargo fmt --check`.

Manual/runtime checks:

- On macOS, launch the daemon tray and verify the normal menu bar item is an
  icon, not the `Rollshot` text title.
- On Linux/KDE, launch the daemon tray and verify the tray still registers and
  menu actions still work. If runtime still depends on icon theme names, verify
  the generated hicolor outputs exist and are named for future packaging.

## Risks

- macOS template icons render best from single-color alpha artwork. If the
  source SVG uses `currentColor`, generation should rasterize it as a solid
  mask rather than a multicolor image.
- Linux SNI icon support differs between `ksni` and `tray-icon`; the normal
  Linux daemon currently uses `ksni`, so runtime pixmap support must be
  verified against the crate API before changing behavior.
- Embedding PNG bytes increases the binary size slightly. The tray asset is
  small enough that this is acceptable.
