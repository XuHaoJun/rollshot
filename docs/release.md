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
