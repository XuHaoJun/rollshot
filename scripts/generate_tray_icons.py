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
