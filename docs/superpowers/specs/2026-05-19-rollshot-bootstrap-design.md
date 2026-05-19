# Rollshot Bootstrap Design

Date: 2026-05-19

## Scope

This phase establishes the engineering foundation for `rollshot`.

It creates a Rust Cargo workspace, baseline crates, CI workflows, and testing
documentation. It does not implement real KDE Wayland, PipeWire, macOS
ScreenCaptureKit, or stitching algorithm behavior beyond minimal compile-time
and smoke-test structure.

## Goals

- Create a Cargo workspace that matches the MVP architecture.
- Add crate boundaries for core logic, capture abstraction, CLI, and future app UI.
- Provide baseline tests so the workspace has a working verification loop.
- Add GitHub Actions CI for formatting, linting, and tests.
- Document local and manual testing in `README.md`.
- Reserve real capture workflows for manual or self-hosted execution.

## Non-Goals

- No real Linux portal or PipeWire capture implementation.
- No real macOS ScreenCaptureKit implementation.
- No production stitching algorithm implementation.
- No GUI, overlay selector, clipboard output, or package release automation.
- No vendored OBS, scap, or wayscrollshot source in this phase.

## Repository Structure

The bootstrap creates this structure:

```text
rollshot/
  Cargo.toml
  README.md
  .gitignore
  .github/
    workflows/
      ci.yml
      real-capture.yml
  crates/
    rollshot-core/
    rollshot-capture/
    rollshot-cli/
    rollshot-app/
```

The workspace uses resolver v2 and centralizes common package metadata and
dependencies in the root `Cargo.toml`.

## Crate Responsibilities

`rollshot-core` owns platform-independent image and stitching concepts. In this
phase it exposes a small public API and unit tests only.

`rollshot-capture` owns capture-facing data types and traits:

- `CaptureBackend`
- `FrameStream`
- `CapturedFrame`
- `CaptureOptions`
- `RegionMode`
- `Region`
- `FrameMetadata`

It may include a fake backend for tests, but no OS capture backend is required
yet.

`rollshot-cli` owns command parsing and user-facing command entry points. The
initial commands are:

- `rollshot probe`
- `rollshot stitch-folder`

`probe` reports a basic backend/platform status. `stitch-folder` may start as a
minimal smoke command until the core stitching phase.

`rollshot-app` is a future GUI crate. It should compile as a minimal binary, but
it has no UX responsibilities in this phase.

## CLI Behavior

The bootstrap CLI should be useful for validating the project layout without
pretending that real capture exists.

`rollshot probe` should print deterministic diagnostic information such as:

- rollshot version
- operating system
- whether real capture is implemented in this build

If a command reaches functionality that is intentionally deferred, it should
return a clear message rather than failing with an opaque panic.

## CI Design

GitHub Actions is the CI/CD system.

The PR CI workflow runs on:

- `ubuntu-24.04`
- `macos-14`

The required checks are:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

The CI workflow does not run real desktop capture. Real capture depends on
interactive desktop sessions, portal permissions, PipeWire, and macOS Screen
Recording permissions, which are not stable assumptions for hosted PR runners.

## Real Capture Workflow

The repository includes a separate `real-capture.yml` workflow for future manual
or scheduled smoke tests on self-hosted runners.

It is not expected to pass until real backend tests exist. It documents the
intended runner labels and commands:

- Linux KDE Wayland self-hosted smoke tests
- macOS ScreenCaptureKit self-hosted smoke tests

The workflow should use `workflow_dispatch` so it can be started manually.

## README Requirements

`README.md` must describe:

- what rollshot is
- current bootstrap status
- how to install or use the Rust toolchain
- local verification commands
- what GitHub Actions runs
- why PR CI does not run real capture
- how to manually test future KDE Wayland capture
- how to manually test future macOS capture
- how to run future self-hosted real-capture smoke tests

Manual testing sections should be explicit checklists so they can become release
candidate QA steps later.

## Testing Strategy

Phase 0 tests prove that the workspace, public APIs, and CLI wiring compile and
run.

Required baseline tests:

- unit tests in `rollshot-core`
- unit tests for capture data types or fake stream behavior
- CLI smoke tests for at least one command

Future phases will add:

- golden image stitching tests
- fake backend integration tests
- ignored real Linux KDE Wayland smoke tests
- ignored real macOS ScreenCaptureKit smoke tests

## Error Handling

Deferred functionality should be represented as explicit unsupported behavior.
The CLI should prefer clear messages such as:

```text
Real capture backends are not implemented in this bootstrap phase.
```

This avoids confusing bootstrap users with partial backend failures.

## Implementation Order

1. Add workspace metadata and shared dependencies.
2. Add crate skeletons.
3. Add capture/core public API stubs and tests.
4. Add CLI command parsing and smoke tests.
5. Add GitHub Actions CI.
6. Add real-capture manual workflow skeleton.
7. Add README local and manual testing instructions.
8. Run local verification.

## Risks

The main risk is overbuilding backend scaffolding before the project has a
stable verification loop. This phase avoids that by keeping real capture out of
scope and focusing on build, test, lint, and documentation.

Another risk is CI drift between Linux and macOS. The matrix workflow catches
workspace-level build issues early, while manual real capture remains isolated
until backend implementations exist.
