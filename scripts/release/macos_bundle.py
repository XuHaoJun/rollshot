#!/usr/bin/env python3
import argparse
import hashlib
import os
import plistlib
import shutil
import subprocess
from pathlib import Path


BUNDLE_ID = "io.rollshot.dev"


def create_bundle(*, binary: Path, iconset: Path, out_dir: Path, version: str) -> Path:
    app = out_dir / "Rollshot.app"
    contents = app / "Contents"
    macos = contents / "MacOS"
    resources = contents / "Resources"

    if app.exists():
        shutil.rmtree(app)
    macos.mkdir(parents=True, exist_ok=True)
    resources.mkdir(parents=True, exist_ok=True)

    target_binary = macos / "rollshot-app"
    shutil.copy2(binary, target_binary)
    target_binary.chmod(0o755)

    if iconset.exists():
        shutil.copytree(iconset, resources / "rollshot.iconset")

    plist = {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleExecutable": "rollshot-app",
        "CFBundleIconFile": "rollshot",
        "CFBundleIdentifier": BUNDLE_ID,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": "Rollshot",
        "CFBundleDisplayName": "Rollshot",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "14.0",
        "LSUIElement": True,
        "NSHighResolutionCapable": True,
        "NSPrincipalClass": "NSApplication",
    }
    with (contents / "Info.plist").open("wb") as f:
        plistlib.dump(plist, f)

    return app


def run(command: list[str]) -> None:
    subprocess.run(command, check=True)


def sign_bundle(app: Path) -> None:
    run(["codesign", "--force", "--deep", "--sign", "-", str(app)])
    run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)])


def zip_bundle(app: Path, out_dir: Path) -> Path:
    artifact = out_dir / "Rollshot-macos-aarch64.app.zip"
    artifact.unlink(missing_ok=True)
    run(["ditto", "-c", "-k", "--keepParent", str(app), str(artifact)])
    return artifact


def create_dmg(app: Path, out_dir: Path) -> Path:
    staging = out_dir / "dmg-staging"
    dmg = out_dir / "Rollshot-macos-aarch64.dmg"
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True)
    shutil.copytree(app, staging / app.name)
    applications = staging / "Applications"
    applications.symlink_to("/Applications")
    dmg.unlink(missing_ok=True)
    run([
        "hdiutil",
        "create",
        "-srcfolder",
        str(staging),
        "-volname",
        "Rollshot",
        "-format",
        "UDZO",
        str(dmg),
    ])
    run(["codesign", "--force", "--sign", "-", str(dmg)])
    shutil.rmtree(staging)
    return dmg


def write_sha256(path: Path) -> Path:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    checksum = path.with_name(path.name + ".sha256")
    checksum.write_text(f"{digest}  {path.name}\n")
    return checksum


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--iconset", type=Path, default=Path("assets/tray/generated/macos/rollshot.iconset"))
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args(argv)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    app = create_bundle(
        binary=args.binary,
        iconset=args.iconset,
        out_dir=args.out_dir,
        version=args.version,
    )
    sign_bundle(app)
    artifacts = [zip_bundle(app, args.out_dir), create_dmg(app, args.out_dir)]
    for artifact in artifacts:
        write_sha256(artifact)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
