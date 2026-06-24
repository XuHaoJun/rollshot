#!/usr/bin/env bash
set -euo pipefail
os="$1"; dest="$2"
ver="1.22.2"   # Pinned: supertone static lib snow-shot ships with ort 2.0.0-rc.10 (linux-x64 + osx-universal2 assets confirmed). CI lane gates the link; do not bump without it.
base="https://github.com/supertone-inc/onnxruntime-build/releases/download/v${ver}"
case "$os" in
  Linux)  asset="onnxruntime-linux-x64-static_lib-${ver}.tgz" ;;
  macOS)  asset="onnxruntime-osx-universal2-static_lib-${ver}.tgz" ;;
  *) echo "unsupported os: $os" >&2; exit 1 ;;
esac
tmp="$(mktemp -d)"
curl -fL "${base}/${asset}" -o "${tmp}/ort.tgz"
tar -xzf "${tmp}/ort.tgz" -C "${tmp}"
libdir="$(find "${tmp}" -type d -name lib | head -n1)"
mkdir -p "${dest}"
cp -r "${libdir}" "${dest}/lib"
