# Internal Release Packaging Design

## Context

Rollshot is ready for internal testing releases, but it does not yet have a
tag-triggered release pipeline or installable release artifacts.

Existing automation:

- `.github/workflows/ci.yml` runs hosted Linux and macOS checks for normal
  workspace builds.
- `.github/workflows/ci-ocr.yml` runs the heavier OCR lane separately.
- `.github/workflows/real-capture.yml` runs manual self-hosted capture smoke
  tests for real Linux KDE Wayland and macOS ScreenCaptureKit environments.
- `.github/workflows/matcher-perf.yml` runs a manual release-mode matcher
  performance smoke.

Existing packaging assets:

- `packaging/linux/dev.rollshot.io.desktop` installs the desktop entry needed
  by KDE native capture authorization.
- `assets/tray/generated/hicolor/` contains Linux hicolor icons.
- `assets/tray/generated/macos/rollshot.iconset/` contains macOS iconset
  inputs.

KDE native capture is path-sensitive. KWin authorizes Rollshot by reading the
running executable path and matching it to an installed desktop entry. The
release package must therefore install the binary at the same absolute path
declared by the desktop entry: `/usr/bin/rollshot-app`.

## Goals

- Add an internal release pipeline triggered by version tags.
- Add a manually triggered rolling internal release channel that can overwrite
  the same GitHub prerelease when testers just need the newest build.
- Produce installable Arch Linux x86_64 packages for internal Linux testers.
- Produce macOS Apple Silicon artifacts that work without a Developer ID
  certificate, accepting the Gatekeeper warning as an internal-testing tradeoff.
- Publish artifacts and checksums to a GitHub prerelease.
- Document the release process, tag naming, supported platforms, and install
  steps in `docs/release.md`.

## Non-Goals

- No AUR publication.
- No hosted pacman repository.
- No Debian, RPM, AppImage, Flatpak, or Snap artifacts.
- No macOS notarization.
- No macOS Developer ID signing until a certificate exists.
- No macOS Intel artifact in the first internal release pipeline.
- No self-hosted real capture smoke as a required release blocker.

## Supported Internal Platforms

Supported:

- Arch Linux x86_64, packaged as a pacman-installable `.pkg.tar.zst`.
- macOS Apple Silicon, packaged as an ad-hoc signed `.app.zip` and an ad-hoc
  signed DMG.

Unsupported in this release scope:

- Arch ARM / aarch64.
- Linux distributions outside the Arch family.
- macOS Intel.
- Notarized macOS distribution.

## Release Channels

Use two internal release channels:

- Rolling internal latest: a mutable GitHub prerelease named
  `internal-latest`, updated whenever a maintainer manually runs the release
  workflow from the desired branch or commit.
- Versioned internal milestones: immutable prerelease tags such as
  `v0.1.0-internal.1`, used when a build needs a stable historical identifier.

The rolling channel is the default internal testing path. Testers can keep
downloading from the same GitHub Release page while maintainers replace the
assets behind that prerelease.

The workflow must record the source commit SHA in the rolling release body and
in a small metadata artifact so mutable releases remain traceable.

## Tag And Version Policy

Versioned internal releases use prerelease tags:

```text
v0.1.0-internal.1
v0.1.0-internal.2
```

The release workflow should trigger on `v*` tags and also support
`workflow_dispatch` for the rolling `internal-latest` release. GitHub releases
produced by the workflow must be marked as prereleases.

For versioned tag releases, the workflow should reject or clearly fail if the
tag version cannot be mapped to package versions. Arch package versions cannot
contain hyphens, so the pipeline should normalize the package version from the
tag:

```text
v0.1.0-internal.1 -> pkgver=0.1.0_internal.1
```

The source tag remains the human-facing release identifier. The Arch package
version uses the normalized form required by pacman tooling.

For the rolling `internal-latest` release, the workflow should derive the Arch
package version from the workspace package version plus commit identity, for
example:

```text
0.1.0_internal.latest.20260703.gfac1c86
```

This keeps the GitHub release page mutable while ensuring the package metadata
still changes between builds. The exact format may vary, but it must avoid
hyphens and must be accepted by `makepkg`.

## Arch Package Design

Add `packaging/arch/PKGBUILD`.

The package should:

- Use `pkgname=rollshot`.
- Use `arch=('x86_64')`.
- Build `rollshot-app` in release mode.
- Install `target/release/rollshot-app` to `/usr/bin/rollshot-app`.
- Install `packaging/linux/dev.rollshot.io.desktop` to
  `/usr/share/applications/dev.rollshot.io.desktop`.
- Install generated hicolor icons from `assets/tray/generated/hicolor/` under
  `/usr/share/icons/hicolor/`.
- Preserve `Exec=/usr/bin/rollshot-app` in the installed desktop entry.

The dependency list should use Arch package names and cover runtime libraries
needed by Rollshot's Linux app and capture stack. Build dependencies should
include Rust and the C/native tooling needed by workspace dependencies.

The release workflow should run the Arch package build in an Arch environment
with `makepkg`. The resulting package should be uploaded as:

```text
rollshot-arch-x86_64.pkg.tar.zst
rollshot-arch-x86_64.pkg.tar.zst.sha256
rollshot-release-metadata.json
```

## macOS Artifact Design

The first macOS internal release should not require Apple Developer Program
credentials.

The macOS release job should:

- Run on a GitHub-hosted Apple Silicon macOS runner such as `macos-14`.
- Build `rollshot-app` in release mode.
- Create `Rollshot.app` with a valid `Contents/Info.plist`,
  `Contents/MacOS/rollshot-app`, and app icon if available.
- Ad-hoc sign the app bundle with `codesign --force --deep --sign -`.
- Produce `Rollshot-macos-aarch64.app.zip`.
- Produce an ad-hoc signed `Rollshot-macos-aarch64.dmg`.
- Produce SHA256 checksum files for every macOS artifact.

The release notes and `docs/release.md` must state that the macOS build is not
notarized. Testers should expect Gatekeeper to show an unidentified-developer
warning and should use right-click Open or System Settings > Privacy & Security
> Open Anyway for the first launch.

Future notarized distribution requires:

- Apple Developer Program membership.
- Developer ID Application certificate.
- Hardened runtime signing.
- `notarytool submit`.
- Stapled notarization ticket.
- GitHub Actions secrets for signing and notarization credentials.

## Release Workflow Design

Add a tag-triggered release workflow under `.github/workflows/`.

The workflow should:

- Trigger on `push` tags matching `v*`.
- Trigger manually with `workflow_dispatch` to update `internal-latest`.
- Build the Arch x86_64 package.
- Build macOS Apple Silicon artifacts.
- Generate SHA256 checksum files.
- Create or update a GitHub Release for the tag or `internal-latest`.
- Mark the GitHub Release as a prerelease.
- Upload all artifacts, checksum files, and release metadata.
- For `internal-latest`, delete or replace old assets before uploading the new
  artifacts so the release page only exposes one current build per artifact
  name.

The workflow should not duplicate all normal CI work inside each packaging job.
It should rely on the repository's normal CI checks before a release tag is
created or the manual rolling workflow is run from `main`. The release
documentation should make "main CI is green" a required human precondition
before tagging or updating `internal-latest`.

The manual self-hosted real-capture workflow remains recommended before wider
testing, but it is not a release workflow dependency. Real capture requires
interactive desktop permissions and does not belong on hosted release runners.

## Documentation Design

Add `docs/release.md`.

The document should cover:

- Current release status: internal prerelease only.
- Supported artifacts and platforms.
- Unsupported platforms and package ecosystems.
- Release channels and tag naming rules.
- Step-by-step maintainer release flow:
  - Confirm `main` CI is green.
  - Optionally run manual real-capture smoke on self-hosted runners.
  - For rolling internal latest, run the release workflow manually and update
    `internal-latest`.
  - For versioned milestones, create and push an internal tag.
  - Check the GitHub prerelease artifacts and checksums.
- Arch tester install flow:
  - Download `rollshot-arch-x86_64.pkg.tar.zst`.
  - Verify checksum.
  - Install with `sudo pacman -U`.
  - Launch `rollshot-app`.
- macOS tester install flow:
  - Download `.app.zip` or DMG.
  - Open using the documented Gatekeeper override path.
  - Grant Screen Recording permission when prompted or through System Settings.
- Notes about why Flatpak/Snap/AppImage/AUR/notarization are deferred.

`docs/release.md` is the user-facing source for internal release operation. It
must match the implemented workflow names, artifact names, and tag policy.

## Testing And Verification

Implementation should verify:

- `rtk cargo fmt --all -- --check`.
- `rtk cargo test --workspace --exclude rollshot-ocr` when workflow changes or
  packaging scripts touch build behavior.
- Arch package build through `makepkg` in an Arch environment.
- The built Arch package contains:
  - `/usr/bin/rollshot-app`
  - `/usr/share/applications/dev.rollshot.io.desktop`
  - `/usr/share/icons/hicolor/.../apps/rollshot.*`
- The installed desktop entry keeps `Exec=/usr/bin/rollshot-app`.
- macOS app bundle structure exists and `codesign --verify` passes for the
  ad-hoc signature.
- The release workflow can upload artifacts in a dry-run or test tag run before
  relying on it for internal testers.
- Re-running the manual rolling workflow replaces `internal-latest` artifacts
  and updates the release metadata with the new source commit.

## Risks

- Arch dependency names may need iteration after the first clean package build.
  Keep the dependency list explicit and verify with `makepkg` plus a pacman
  install smoke.
- Hosted macOS runners may not match all internal tester machines. The first
  artifact only supports Apple Silicon.
- Ad-hoc signed macOS builds are intentionally not trusted by Gatekeeper. This
  is acceptable for internal testing but unsuitable for public release.
- Tag-triggered and rolling releases can publish broken artifacts if maintainers
  release before checking CI. The release guide must make the pre-release
  checklist explicit.
- Mutable `internal-latest` artifacts can confuse debugging if testers report
  only the release name. The release metadata and release body must include the
  exact source commit.
