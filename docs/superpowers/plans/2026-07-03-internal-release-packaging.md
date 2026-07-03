# Internal Release Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add internal Rollshot release packaging with a mutable `internal-latest` prerelease, optional immutable `v*` milestone tags, Arch Linux x86_64 packages, macOS Apple Silicon ad-hoc artifacts, checksums, metadata, and release documentation.

**Architecture:** Keep release-specific logic in small Python helpers under `scripts/release/` so version normalization and macOS bundle layout can be tested outside GitHub Actions. Use a native Arch `PKGBUILD` for Linux installation semantics, and a single GitHub Actions workflow for both `workflow_dispatch` rolling releases and `v*` tag releases.

**Tech Stack:** GitHub Actions, Python 3 stdlib, Arch `makepkg`, Cargo/Rust, macOS `codesign`, `hdiutil`, `ditto`, GitHub CLI `gh`.

---

## File Structure

- Create `scripts/release/metadata.py`: compute release channel, GitHub release tag/name, Arch-safe package version, source commit metadata, and write both JSON metadata and GitHub Actions outputs.
- Create `scripts/release/test_metadata.py`: pytest-style unit tests for tag and rolling metadata.
- Create `scripts/release/macos_bundle.py`: build `Rollshot.app`, generate `Info.plist`, copy binary/icon, ad-hoc sign, zip, DMG, checksum.
- Create `scripts/release/test_macos_bundle.py`: unit tests for bundle layout and `Info.plist` without invoking `codesign` or `hdiutil`.
- Create `packaging/arch/PKGBUILD`: native Arch package definition that installs `/usr/bin/rollshot-app`, desktop entry, and hicolor icons.
- Create `.github/workflows/internal-release.yml`: release workflow for rolling `internal-latest` and immutable `v*` prereleases.
- Create `docs/release.md`: maintainer and tester release guide.
- Modify no product Rust code unless verification reveals a build-only issue.

## Task 1: Release Metadata Helper

**Files:**
- Create: `scripts/release/metadata.py`
- Create: `scripts/release/test_metadata.py`

- [ ] **Step 1: Write failing tests for release metadata**

Create `scripts/release/test_metadata.py`:

```python
import json
from pathlib import Path

import metadata


def test_versioned_tag_metadata_normalizes_arch_pkgver(tmp_path):
    cargo = tmp_path / "Cargo.toml"
    cargo.write_text('[workspace.package]\nversion = "0.1.0"\n')
    out = tmp_path / "metadata.json"

    result = metadata.build_metadata(
        cargo_toml=cargo,
        ref_type="tag",
        ref_name="v0.1.0-internal.7",
        sha="abcdef1234567890",
        run_number="42",
        date="20260703",
        output_json=out,
    )

    assert result["channel"] == "versioned"
    assert result["release_tag"] == "v0.1.0-internal.7"
    assert result["release_name"] == "Rollshot v0.1.0-internal.7"
    assert result["package_version"] == "0.1.0_internal.7"
    assert result["short_sha"] == "abcdef1"
    assert json.loads(out.read_text()) == result


def test_rolling_metadata_uses_workspace_version_date_and_sha(tmp_path):
    cargo = tmp_path / "Cargo.toml"
    cargo.write_text('[workspace.package]\nversion = "0.1.0"\n')
    out = tmp_path / "metadata.json"

    result = metadata.build_metadata(
        cargo_toml=cargo,
        ref_type="branch",
        ref_name="main",
        sha="fac1c86e2f4b85abcdef1234567890abcdef1234",
        run_number="108",
        date="20260703",
        output_json=out,
    )

    assert result["channel"] == "rolling"
    assert result["release_tag"] == "internal-latest"
    assert result["release_name"] == "Rollshot internal latest"
    assert result["package_version"] == "0.1.0_internal.latest.20260703.gfac1c86"
    assert result["source_ref"] == "main"
    assert result["source_sha"] == "fac1c86e2f4b85abcdef1234567890abcdef1234"


def test_invalid_tag_fails_clearly(tmp_path):
    cargo = tmp_path / "Cargo.toml"
    cargo.write_text('[workspace.package]\nversion = "0.1.0"\n')

    try:
        metadata.build_metadata(
            cargo_toml=cargo,
            ref_type="tag",
            ref_name="nightly",
            sha="abcdef1234567890",
            run_number="1",
            date="20260703",
            output_json=tmp_path / "metadata.json",
        )
    except ValueError as error:
        assert "expected tag starting with v" in str(error)
    else:
        raise AssertionError("invalid tag should fail")
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
rtk pytest -q scripts/release/test_metadata.py
```

Expected: FAIL because `scripts/release/metadata.py` does not exist.

- [ ] **Step 3: Implement metadata helper**

Create `scripts/release/metadata.py`:

```python
#!/usr/bin/env python3
import argparse
import json
import os
import re
import tomllib
from datetime import datetime, timezone
from pathlib import Path


def workspace_version(cargo_toml: Path) -> str:
    data = tomllib.loads(cargo_toml.read_text())
    return data["workspace"]["package"]["version"]


def normalize_tag_version(ref_name: str) -> str:
    if not ref_name.startswith("v"):
        raise ValueError(f"expected tag starting with v, got {ref_name!r}")
    version = ref_name[1:]
    if not version:
        raise ValueError("tag version is empty")
    normalized = version.replace("-", "_")
    if not re.fullmatch(r"[A-Za-z0-9._+]+", normalized):
        raise ValueError(f"tag {ref_name!r} cannot be converted to an Arch pkgver")
    return normalized


def build_metadata(
    *,
    cargo_toml: Path,
    ref_type: str,
    ref_name: str,
    sha: str,
    run_number: str,
    date: str,
    output_json: Path,
) -> dict[str, str]:
    short_sha = sha[:7]
    base_version = workspace_version(cargo_toml)
    if ref_type == "tag":
        package_version = normalize_tag_version(ref_name)
        channel = "versioned"
        release_tag = ref_name
        release_name = f"Rollshot {ref_name}"
        display_version = ref_name
    else:
        package_version = f"{base_version}_internal.latest.{date}.g{short_sha}"
        channel = "rolling"
        release_tag = "internal-latest"
        release_name = "Rollshot internal latest"
        display_version = f"internal-latest ({short_sha})"

    metadata = {
        "channel": channel,
        "release_tag": release_tag,
        "release_name": release_name,
        "display_version": display_version,
        "workspace_version": base_version,
        "package_version": package_version,
        "source_ref_type": ref_type,
        "source_ref": ref_name,
        "source_sha": sha,
        "short_sha": short_sha,
        "github_run_number": run_number,
        "build_date_utc": date,
    }

    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_json.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    return metadata


def write_github_output(path: Path, metadata: dict[str, str]) -> None:
    with path.open("a") as f:
        for key, value in metadata.items():
            f.write(f"{key}={value}\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cargo-toml", type=Path, default=Path("Cargo.toml"))
    parser.add_argument("--output-json", type=Path, required=True)
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--ref-type", default=os.environ.get("GITHUB_REF_TYPE", "branch"))
    parser.add_argument("--ref-name", default=os.environ.get("GITHUB_REF_NAME", "main"))
    parser.add_argument("--sha", default=os.environ.get("GITHUB_SHA", "unknown"))
    parser.add_argument("--run-number", default=os.environ.get("GITHUB_RUN_NUMBER", "0"))
    parser.add_argument(
        "--date",
        default=datetime.now(timezone.utc).strftime("%Y%m%d"),
    )
    args = parser.parse_args(argv)

    metadata = build_metadata(
        cargo_toml=args.cargo_toml,
        ref_type=args.ref_type,
        ref_name=args.ref_name,
        sha=args.sha,
        run_number=args.run_number,
        date=args.date,
        output_json=args.output_json,
    )
    if args.github_output is not None:
        write_github_output(args.github_output, metadata)
    print(json.dumps(metadata, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 4: Run metadata tests**

Run:

```bash
rtk pytest -q scripts/release/test_metadata.py
```

Expected: PASS.

- [ ] **Step 5: Commit metadata helper**

Run:

```bash
rtk git add scripts/release/metadata.py scripts/release/test_metadata.py
rtk git commit -m "chore(release): add release metadata helper"
```

## Task 2: macOS Bundle Helper

**Files:**
- Create: `scripts/release/macos_bundle.py`
- Create: `scripts/release/test_macos_bundle.py`

- [ ] **Step 1: Write failing tests for bundle layout**

Create `scripts/release/test_macos_bundle.py`:

```python
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
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
rtk pytest -q scripts/release/test_macos_bundle.py
```

Expected: FAIL because `scripts/release/macos_bundle.py` does not exist.

- [ ] **Step 3: Implement macOS bundle helper**

Create `scripts/release/macos_bundle.py`:

```python
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
```

- [ ] **Step 4: Run macOS bundle tests**

Run:

```bash
rtk pytest -q scripts/release/test_macos_bundle.py
```

Expected: PASS.

- [ ] **Step 5: Commit macOS helper**

Run:

```bash
rtk git add scripts/release/macos_bundle.py scripts/release/test_macos_bundle.py
rtk git commit -m "chore(release): add macos bundle helper"
```

## Task 3: Arch PKGBUILD

**Files:**
- Create: `packaging/arch/PKGBUILD`

- [ ] **Step 1: Create Arch package definition**

Create `packaging/arch/PKGBUILD`:

```bash
# Maintainer: Rollshot maintainers
pkgname=rollshot
pkgver=${ROLLSHOT_PKGVER:-0.1.0_internal.latest.local}
pkgrel=1
pkgdesc="Screenshot and scrolling-capture tool"
arch=('x86_64')
url="https://github.com/xuhaojun/rollshot"
license=('MIT')
depends=(
  'dbus'
  'gcc-libs'
  'glibc'
  'libpipewire'
  'libxkbcommon'
  'mesa'
  'wayland'
)
makedepends=(
  'cargo'
  'clang'
  'pkgconf'
  'rust'
)
source=()
sha256sums=()

build() {
  cd "$startdir/../.."
  cargo build --release -p rollshot-app
}

package() {
  cd "$startdir/../.."

  install -Dm755 target/release/rollshot-app \
    "$pkgdir/usr/bin/rollshot-app"

  install -Dm644 packaging/linux/dev.rollshot.io.desktop \
    "$pkgdir/usr/share/applications/dev.rollshot.io.desktop"

  find assets/tray/generated/hicolor -type f | while read -r icon; do
    install -Dm644 "$icon" "$pkgdir/usr/share/icons/hicolor/${icon#assets/tray/generated/hicolor/}"
  done
}
```

- [ ] **Step 2: Validate PKGBUILD shell syntax**

Run:

```bash
rtk bash -n packaging/arch/PKGBUILD
```

Expected: PASS with no output.

- [ ] **Step 3: Verify desktop entry path in package definition**

Run:

```bash
rtk rg -n 'Exec=/usr/bin/rollshot-app|dev.rollshot.io.desktop|/usr/bin/rollshot-app' packaging/linux/dev.rollshot.io.desktop packaging/arch/PKGBUILD
```

Expected: output includes `Exec=/usr/bin/rollshot-app` and both install paths.

- [ ] **Step 4: Commit Arch PKGBUILD**

Run:

```bash
rtk git add packaging/arch/PKGBUILD
rtk git commit -m "chore(release): add arch package definition"
```

## Task 4: Internal Release Workflow

**Files:**
- Create: `.github/workflows/internal-release.yml`

- [ ] **Step 1: Create release workflow**

Create `.github/workflows/internal-release.yml`:

```yaml
name: Internal Release

on:
  workflow_dispatch:
  push:
    tags:
      - "v*"

permissions:
  contents: write

jobs:
  metadata:
    name: Release metadata
    runs-on: ubuntu-24.04
    outputs:
      channel: ${{ steps.meta.outputs.channel }}
      release_tag: ${{ steps.meta.outputs.release_tag }}
      release_name: ${{ steps.meta.outputs.release_name }}
      package_version: ${{ steps.meta.outputs.package_version }}
      source_sha: ${{ steps.meta.outputs.source_sha }}
      short_sha: ${{ steps.meta.outputs.short_sha }}
    steps:
      - uses: actions/checkout@v4
      - name: Generate release metadata
        id: meta
        run: |
          mkdir -p dist
          python3 scripts/release/metadata.py \
            --output-json dist/rollshot-release-metadata.json \
            --github-output "$GITHUB_OUTPUT"
      - name: Upload metadata
        uses: actions/upload-artifact@v4
        with:
          name: release-metadata
          path: dist/rollshot-release-metadata.json

  arch:
    name: Arch package
    runs-on: ubuntu-24.04
    needs: metadata
    container: archlinux:base-devel
    steps:
      - uses: actions/checkout@v4
      - name: Install build dependencies
        run: |
          pacman -Syu --noconfirm
          pacman -S --noconfirm git cargo rust clang pkgconf dbus libpipewire libxkbcommon mesa wayland
      - name: Build package
        env:
          ROLLSHOT_PKGVER: ${{ needs.metadata.outputs.package_version }}
        run: |
          useradd -m builder
          chown -R builder:builder "$GITHUB_WORKSPACE"
          cd packaging/arch
          su builder -c "ROLLSHOT_PKGVER=$ROLLSHOT_PKGVER makepkg --noconfirm"
          mkdir -p "$GITHUB_WORKSPACE/dist"
          cp rollshot-*.pkg.tar.zst "$GITHUB_WORKSPACE/dist/rollshot-arch-x86_64.pkg.tar.zst"
          cd "$GITHUB_WORKSPACE/dist"
          sha256sum rollshot-arch-x86_64.pkg.tar.zst > rollshot-arch-x86_64.pkg.tar.zst.sha256
      - name: Verify package contents
        run: |
          pacman -Qlp dist/rollshot-arch-x86_64.pkg.tar.zst | tee dist/arch-package-files.txt
          grep -F '/usr/bin/rollshot-app' dist/arch-package-files.txt
          grep -F '/usr/share/applications/dev.rollshot.io.desktop' dist/arch-package-files.txt
          grep -F '/usr/share/icons/hicolor/scalable/apps/rollshot.svg' dist/arch-package-files.txt
      - name: Upload Arch artifacts
        uses: actions/upload-artifact@v4
        with:
          name: arch-release
          path: |
            dist/rollshot-arch-x86_64.pkg.tar.zst
            dist/rollshot-arch-x86_64.pkg.tar.zst.sha256

  macos:
    name: macOS Apple Silicon artifacts
    runs-on: macos-14
    needs: metadata
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build rollshot-app
        run: cargo build --release -p rollshot-app
      - name: Build macOS artifacts
        run: |
          python3 scripts/release/macos_bundle.py \
            --binary target/release/rollshot-app \
            --out-dir dist \
            --version "${{ needs.metadata.outputs.package_version }}"
      - name: Upload macOS artifacts
        uses: actions/upload-artifact@v4
        with:
          name: macos-release
          path: |
            dist/Rollshot-macos-aarch64.app.zip
            dist/Rollshot-macos-aarch64.app.zip.sha256
            dist/Rollshot-macos-aarch64.dmg
            dist/Rollshot-macos-aarch64.dmg.sha256

  publish:
    name: Publish GitHub prerelease
    runs-on: ubuntu-24.04
    needs: [metadata, arch, macos]
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true
      - name: Write release notes
        run: |
          cat > release-notes.md <<'NOTES'
          Internal Rollshot prerelease.

          Source commit: `${{ needs.metadata.outputs.source_sha }}`
          Channel: `${{ needs.metadata.outputs.channel }}`

          Supported artifacts:
          - Arch Linux x86_64 package
          - macOS Apple Silicon app zip and DMG

          macOS artifacts are ad-hoc signed and not notarized. Use right-click Open or System Settings > Privacy & Security > Open Anyway on first launch.
          NOTES
      - name: Replace rolling release assets
        if: needs.metadata.outputs.channel == 'rolling'
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          if gh release view "${{ needs.metadata.outputs.release_tag }}" >/dev/null 2>&1; then
            gh release edit "${{ needs.metadata.outputs.release_tag }}" \
              --title "${{ needs.metadata.outputs.release_name }}" \
              --notes-file release-notes.md \
              --prerelease \
              --target "${{ needs.metadata.outputs.source_sha }}"
            for asset in $(gh release view "${{ needs.metadata.outputs.release_tag }}" --json assets --jq '.assets[].name'); do
              gh release delete-asset "${{ needs.metadata.outputs.release_tag }}" "$asset" --yes
            done
          fi
      - name: Publish release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release create "${{ needs.metadata.outputs.release_tag }}" \
            --title "${{ needs.metadata.outputs.release_name }}" \
            --notes-file release-notes.md \
            --prerelease \
            --target "${{ needs.metadata.outputs.source_sha }}" \
            dist/rollshot-release-metadata.json \
            dist/rollshot-arch-x86_64.pkg.tar.zst \
            dist/rollshot-arch-x86_64.pkg.tar.zst.sha256 \
            dist/Rollshot-macos-aarch64.app.zip \
            dist/Rollshot-macos-aarch64.app.zip.sha256 \
            dist/Rollshot-macos-aarch64.dmg \
            dist/Rollshot-macos-aarch64.dmg.sha256 \
          || gh release upload "${{ needs.metadata.outputs.release_tag }}" \
            --clobber \
            dist/rollshot-release-metadata.json \
            dist/rollshot-arch-x86_64.pkg.tar.zst \
            dist/rollshot-arch-x86_64.pkg.tar.zst.sha256 \
            dist/Rollshot-macos-aarch64.app.zip \
            dist/Rollshot-macos-aarch64.app.zip.sha256 \
            dist/Rollshot-macos-aarch64.dmg \
            dist/Rollshot-macos-aarch64.dmg.sha256
```

- [ ] **Step 2: Validate workflow YAML structure locally**

Run:

```bash
rtk python3 - <<'PY'
from pathlib import Path
path = Path(".github/workflows/internal-release.yml")
text = path.read_text()
required = [
    "workflow_dispatch:",
    'tags:',
    "internal-latest",
    "archlinux:base-devel",
    "macos-14",
    "gh release",
]
missing = [item for item in required if item not in text]
if missing:
    raise SystemExit(f"missing workflow markers: {missing}")
PY
```

Expected: PASS with no output.

- [ ] **Step 3: Commit workflow**

Run:

```bash
rtk git add .github/workflows/internal-release.yml
rtk git commit -m "ci(release): add internal release workflow"
```

## Task 5: Release Documentation

**Files:**
- Create: `docs/release.md`

- [ ] **Step 1: Write release guide**

Create `docs/release.md`:

```markdown
# Release Guide

Rollshot release packaging is currently for internal prereleases only.

## Supported Artifacts

- Arch Linux x86_64: `rollshot-arch-x86_64.pkg.tar.zst`
- macOS Apple Silicon: `Rollshot-macos-aarch64.app.zip`
- macOS Apple Silicon: `Rollshot-macos-aarch64.dmg`

Every artifact has a `.sha256` checksum file. Every release also includes
`rollshot-release-metadata.json`, which records the source commit.

## Unsupported

- AUR
- Hosted pacman repository
- Debian packages
- RPM packages
- AppImage
- Flatpak
- Snap
- macOS Intel
- macOS notarization

## Release Channels

`internal-latest` is the default internal testing release. It is mutable: running
the internal release workflow manually replaces the artifacts on the same GitHub
prerelease. Use `rollshot-release-metadata.json` or the release body to identify
the exact source commit.

Versioned internal milestone tags are immutable:

```bash
v0.1.0-internal.1
v0.1.0-internal.2
```

Do not delete and recreate versioned tags for normal testing. If code changes
and a stable historical build is needed, increment the internal suffix.

## Update `internal-latest`

1. Confirm the `main` branch CI is green.
2. Optionally run the manual `Real Capture Smoke` workflow on self-hosted Linux
   KDE Wayland and macOS ScreenCaptureKit runners.
3. Open GitHub Actions.
4. Run the `Internal Release` workflow manually from the branch or commit to
   publish.
5. Open the `internal-latest` GitHub prerelease.
6. Confirm these assets exist:
   - `rollshot-release-metadata.json`
   - `rollshot-arch-x86_64.pkg.tar.zst`
   - `rollshot-arch-x86_64.pkg.tar.zst.sha256`
   - `Rollshot-macos-aarch64.app.zip`
   - `Rollshot-macos-aarch64.app.zip.sha256`
   - `Rollshot-macos-aarch64.dmg`
   - `Rollshot-macos-aarch64.dmg.sha256`

## Create A Versioned Internal Milestone

1. Confirm the `main` branch CI is green.
2. Create a tag:

   ```bash
   git tag v0.1.0-internal.1
   ```

3. Push the tag:

   ```bash
   git push origin v0.1.0-internal.1
   ```

4. Open the GitHub prerelease for the tag and confirm the artifacts and
   checksums exist.

## Arch Tester Install

Download:

- `rollshot-arch-x86_64.pkg.tar.zst`
- `rollshot-arch-x86_64.pkg.tar.zst.sha256`

Verify:

```bash
sha256sum -c rollshot-arch-x86_64.pkg.tar.zst.sha256
```

Install:

```bash
sudo pacman -U ./rollshot-arch-x86_64.pkg.tar.zst
```

Launch:

```bash
rollshot-app
```

The package installs `/usr/bin/rollshot-app` and
`/usr/share/applications/dev.rollshot.io.desktop`. The desktop entry keeps
`Exec=/usr/bin/rollshot-app`, which is required for KDE native capture
authorization.

## macOS Tester Install

Download either:

- `Rollshot-macos-aarch64.app.zip`
- `Rollshot-macos-aarch64.dmg`

The macOS artifacts are ad-hoc signed and not notarized. On first launch, macOS
may block the app as being from an unidentified developer.

Open it by using one of these paths:

- Right-click `Rollshot.app`, choose Open, then confirm.
- Open System Settings > Privacy & Security, then choose Open Anyway after the
  first blocked launch.

Grant Screen Recording permission when prompted, or through System Settings >
Privacy & Security > Screen & System Audio Recording.
```

- [ ] **Step 2: Verify release guide mentions fixed rolling release and platforms**

Run:

```bash
rtk rg -n 'internal-latest|Arch Linux x86_64|macOS Apple Silicon|not notarized|pacman -U|v0.1.0-internal.1' docs/release.md
```

Expected: all patterns are found.

- [ ] **Step 3: Commit release guide**

Run:

```bash
rtk git add docs/release.md
rtk git commit -m "docs: add internal release guide"
```

## Task 6: Local Verification And Final Commit Hygiene

**Files:**
- Verify all files from Tasks 1-5.

- [ ] **Step 1: Run Python unit tests**

Run:

```bash
rtk pytest -q scripts/release/test_metadata.py scripts/release/test_macos_bundle.py
```

Expected: PASS.

- [ ] **Step 2: Run shell syntax checks**

Run:

```bash
rtk bash -n packaging/arch/PKGBUILD
```

Expected: PASS.

- [ ] **Step 3: Run Rust formatting check**

Run:

```bash
rtk cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 4: Run workspace tests**

Run:

```bash
rtk cargo test --workspace --exclude rollshot-ocr
```

Expected: PASS.

- [ ] **Step 5: Verify changed files**

Run:

```bash
rtk git status --short
rtk git log --oneline -6
```

Expected: working tree clean after the task commits; recent commits include release metadata helper, macOS bundle helper, Arch package definition, release workflow, and release guide.

## Task 7: GitHub Workflow Smoke After Merge

**Files:**
- No local file edits.

- [ ] **Step 1: Run rolling release manually**

Use GitHub Actions UI:

```text
Actions > Internal Release > Run workflow
```

Expected: workflow completes and creates or updates the `internal-latest` prerelease.

- [ ] **Step 2: Verify rolling release assets**

Download and inspect `rollshot-release-metadata.json`.

Expected:

```json
{
  "channel": "rolling",
  "release_tag": "internal-latest"
}
```

The JSON also contains the current source commit SHA.

- [ ] **Step 3: Verify Arch package install on an Arch-family test machine**

Run on the tester machine:

```bash
sha256sum -c rollshot-arch-x86_64.pkg.tar.zst.sha256
sudo pacman -U ./rollshot-arch-x86_64.pkg.tar.zst
pacman -Ql rollshot | grep /usr/bin/rollshot-app
grep '^Exec=/usr/bin/rollshot-app$' /usr/share/applications/dev.rollshot.io.desktop
```

Expected: checksum passes, package installs, binary exists, and desktop entry Exec matches `/usr/bin/rollshot-app`.

- [ ] **Step 4: Verify macOS artifact on Apple Silicon**

Run on an Apple Silicon Mac:

```bash
shasum -a 256 -c Rollshot-macos-aarch64.app.zip.sha256
unzip Rollshot-macos-aarch64.app.zip
codesign --verify --deep --strict --verbose=2 Rollshot.app
```

Expected: checksum passes, unzip creates `Rollshot.app`, and ad-hoc signature verifies.
