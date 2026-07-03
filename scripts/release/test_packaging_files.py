from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_arch_package_installs_desktop_icon_name():
    pkgbuild = (ROOT / "packaging/arch/PKGBUILD").read_text()
    workflow = (ROOT / ".github/workflows/internal-release.yml").read_text()

    assert "dev.rollshot.io.${icon##*.}" in pkgbuild
    assert "/usr/share/icons/hicolor/scalable/apps/dev.rollshot.io.svg" in workflow


def test_rolling_release_workflow_moves_internal_latest_tag():
    workflow = (ROOT / ".github/workflows/internal-release.yml").read_text()

    assert "git tag -f internal-latest" in workflow
    assert "git push --force origin refs/tags/internal-latest" in workflow
