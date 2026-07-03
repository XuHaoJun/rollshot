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
