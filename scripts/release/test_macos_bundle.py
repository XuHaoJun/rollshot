import plistlib
from pathlib import Path

import macos_bundle


def test_create_bundle_layout_without_signing(tmp_path):
    binary = tmp_path / "rollshot-app"
    binary.write_bytes(b"fake-binary")
    iconset = tmp_path / "rollshot.iconset"
    iconset.mkdir()
    (iconset / "icon_16x16.png").write_bytes(b"fake-png")

    app = macos_bundle.create_bundle(
        binary=binary,
        iconset=iconset,
        out_dir=tmp_path / "dist",
        version="0.1.0_internal.latest.20260703.gabcdef0",
    )

    assert app == tmp_path / "dist" / "Rollshot.app"
    assert (app / "Contents" / "MacOS" / "rollshot-app").read_bytes() == b"fake-binary"
    assert (app / "Contents" / "Resources" / "rollshot.iconset" / "icon_16x16.png").exists()
    plist = plistlib.loads((app / "Contents" / "Info.plist").read_bytes())
    assert plist["CFBundleExecutable"] == "rollshot-app"
    assert plist["CFBundleIdentifier"] == "io.rollshot.dev"
    assert plist["CFBundleName"] == "Rollshot"
    assert plist["CFBundleShortVersionString"] == "0.1.0_internal.latest.20260703.gabcdef0"
    assert plist["NSPrincipalClass"] == "NSApplication"


def test_checksum_file_uses_sha256_format(tmp_path):
    artifact = tmp_path / "artifact.zip"
    artifact.write_bytes(b"rollshot")
    checksum = macos_bundle.write_sha256(artifact)

    text = checksum.read_text()
    assert checksum.name == "artifact.zip.sha256"
    assert text.endswith("  artifact.zip\n")
    assert len(text.split()[0]) == 64
