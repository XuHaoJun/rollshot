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
