#!/usr/bin/env bash
#
# Run an OCR `cargo test` invocation, tolerating ONLY the macOS-specific
# ONNX Runtime process-exit teardown abort that fires *after* all tests pass.
#
# Usage: scripts/ci/run-ocr-test.sh <cargo test command...>
#   e.g. scripts/ci/run-ocr-test.sh cargo test -p rollshot-ocr
#
# ── Why this wrapper exists ───────────────────────────────────────────────────
# ONNX Runtime 1.21+ has a macOS-only static-destructor ordering bug: its global
# logger mutex is destroyed before `OrtEnv`, so a binary that loaded `ort` aborts
# with `libc++abi: ... mutex lock failed: Invalid argument` (SIGABRT) when it
# returns from `main` and runs C++ static destructors at process exit.
# Refs: pykeio/ort#409, microsoft/onnxruntime#24579.
#
# It is fixed in `ort` >= 2.0.0-rc.11 ("Don't store Environment as a static"),
# which we CANNOT adopt: rc.11 requires `ndarray 0.17`, but `paddle-ocr-rs` (the
# OCR engine wrapper) pins `ndarray ^0.16` in every published version, and the
# `ndarray` array type crosses the paddle-ocr-rs <-> ort boundary
# (`Tensor::from_array`). Bumping `ort` is therefore blocked without forking
# paddle-ocr-rs.
#
# The crash is harmless and TEST-ONLY:
#   * The tests themselves pass (the abort happens after the test summary).
#   * The product never hits it. `rollshot-app` exits via `process::exit` while
#     the OCR engine is still live (like snow-shot's `handle.exit(0)`), so the
#     "drop all ort sessions then unwind main" teardown path never runs.
#   * snow-shot — the validated OCR-stack reference on the identical stack — has
#     no OCR tests and never `cargo test`s an ort binary, so it never observes
#     this latent bug. Our OCR CI lane is the first place that asserts on the
#     exit code of an ort-loading test binary on macOS, which is what exposes it.
#
# So on macOS we decline to assert on the teardown-only exit code, while STILL
# failing the job on any real test failure. A non-zero exit is tolerated IFF the
# test summary shows tests ran and passed (`test result: ok.`) and none failed
# (`test result: FAILED.`). Real failures, build errors, and "no tests ran" all
# still fail. On non-macOS this is a transparent pass-through to cargo.

set -uo pipefail

log="$(mktemp)"
# shellcheck disable=SC2064  # expand $log now so the trap removes the right file
trap "rm -f '$log'" EXIT

# Run the cargo command, teeing combined stdout+stderr so we can inspect the
# libtest summary even when the process aborts.
"$@" 2>&1 | tee "$log"
code=${PIPESTATUS[0]}

# Clean exit: nothing to second-guess.
if [ "$code" -eq 0 ]; then
  exit 0
fi

# Past here the run exited non-zero. Only the macOS teardown abort is tolerated.
if [ "${RUNNER_OS:-}" != "macOS" ]; then
  exit "$code"
fi

# A real test failure must always fail the job, even alongside a passing binary.
if grep -qE 'test result: FAILED' "$log"; then
  exit "$code"
fi

# Require positive evidence that a test binary ran to completion and passed.
# This guards against build errors or panics that abort before the summary is
# printed (neither "FAILED" nor "ok." appears in those cases).
if grep -qE 'test result: ok\.' "$log"; then
  echo "::warning title=ONNX Runtime macOS teardown::Tolerated ONNX Runtime teardown SIGABRT (exit ${code}) after tests passed; see pykeio/ort#409 and scripts/ci/run-ocr-test.sh."
  exit 0
fi

# No recognizable passing summary → treat as a genuine failure.
exit "$code"
