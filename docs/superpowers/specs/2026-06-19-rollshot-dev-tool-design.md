# Rollshot Developer Tool Boundary Design

## Summary

Rollshot will stop shipping a general-purpose `rollshot` command-line product.
The existing `rollshot-cli` crate will become an explicitly internal developer
tool named `rollshot-dev`. Interactive capture, daemon operation, and Action
Guide recording remain exclusively owned by `rollshot-app`.

This is an intentional breaking change. No compatibility binary, deprecated
alias, or command forwarding layer will remain.

## Goals

- Make `rollshot-app` the only product-facing executable.
- Keep useful repository diagnostics and offline stitching workflows available
  to developers and CI.
- Remove the duplicate headless capture pipeline and GUI-launcher behavior from
  the command-line crate.
- Make package, crate directory, binary, documentation, and test names describe
  the tool's internal purpose consistently.

## Non-Goals

- Adding new developer commands.
- Moving the removed CLI capture pipeline into another crate.
- Changing capture, stitching, or UI behavior in `rollshot-app`.
- Changing capture backend implementations in `rollshot-capture`.
- Preserving compatibility with the `rollshot` binary or `rollshot-cli`
  package.
- Updating frozen historical files under `docs/superpowers/`.

## Package and Binary Boundary

Rename:

- Directory: `crates/rollshot-cli` to `crates/rollshot-dev`
- Cargo package: `rollshot-cli` to `rollshot-dev`
- Binary: `rollshot` to `rollshot-dev`

The workspace member list and all active build, CI, documentation, and test
references will use the new name.

`rollshot-dev` will expose exactly two subcommands:

- `probe`: report host and capture backend diagnostics in text or JSON form.
- `stitch-folder`: stitch an existing directory of image frames and optionally
  emit matcher diagnostics.

The tool may depend on `rollshot-capture` for backend capability reporting in
`probe`, but it will not start a capture backend.

## Product Ownership

`rollshot-app` remains the sole owner of:

- Interactive screenshot and scrolling capture
- Capture overlays and region selection
- Post-capture editing and save handoff
- System tray and menu-bar daemon modes
- Action Guide recording and editing
- Product-facing capture backend selection

`rollshot-dev` will not locate, launch, or forward arguments to
`rollshot-app`. The `ROLLSHOT_APP` environment variable and sibling-binary
resolution logic will be removed.

## Removed Code

Delete the following command implementations and support code:

- `cmd_capture.rs`
- `cmd_capture_launcher.rs`
- `cmd_action_guide.rs`
- `frame_slot.rs`
- `CaptureArgs`
- `ActionGuideArgs`
- `capture` and `action-guide` command variants and dispatch
- The `rollshot-cli/action-guide` feature

Dependencies used only by the removed code will also be removed. Dependencies
still required by `probe` or `stitch-folder` remain.

## Test Strategy

Delete tests whose subject is the removed CLI behavior:

- Interactive application launching
- Headless and fixture capture commands
- Capture progress and summary output
- CLI backend-start and unsupported-backend behavior
- Action Guide argument forwarding

Keep and rename tests for:

- `probe` text and JSON output
- `stitch-folder` PNG output
- Matcher report output
- Overlap debug artifacts
- Feature-fallback behavior

All retained binary integration tests will use
`CARGO_BIN_EXE_rollshot-dev`. Shared test artifact prefixes will use
`rollshot-dev`.

The removed headless capture implementation will not be preserved merely to
retain its tests. Backend and fixture behavior belongs in `rollshot-capture`;
stitching behavior belongs in `rollshot-core` and the retained
`stitch-folder` tests; product capture behavior belongs in `rollshot-app`.

## Documentation and Automation

Update active references in:

- Workspace `Cargo.toml`
- `.github/workflows/ci.yml`
- `README.md`
- Root `AGENTS.md`
- Active source and test files

README changes will:

- Describe `rollshot-dev` as an internal development and diagnostic tool.
- Show `cargo run -p rollshot-dev -- probe` and `stitch-folder` examples.
- Remove CLI headless capture instructions and manual test checklist entries.
- Direct interactive capture and Action Guide usage to `rollshot-app`.

`AGENTS.md` changes will update the project map, source-of-truth description,
and feature references to reflect the new boundary.

Frozen historical plans and specs under `docs/superpowers/` will not be
rewritten. The new design document itself remains the live specification while
this work is implemented.

CI feature invocations will remove `rollshot-cli/action-guide` and retain only
`rollshot-app/action-guide`.

## Compatibility

The following interfaces disappear without aliases:

- Cargo package `rollshot-cli`
- Executable `rollshot`
- `rollshot capture`
- `rollshot action-guide`
- `ROLLSHOT_APP`

Developer invocations become:

```sh
cargo run -p rollshot-dev -- probe
cargo run -p rollshot-dev -- stitch-folder <frames-dir> --output <output.png>
```

## Verification

Implementation is complete when:

1. `cargo test -p rollshot-dev` passes.
2. `cargo fmt --check` passes.
3. `cargo clippy --workspace --all-targets --features rollshot-app/action-guide -- -D warnings`
   passes.
4. `cargo test --workspace --features rollshot-app/action-guide` passes.
5. `rollshot-dev --help` lists only `probe` and `stitch-folder`.
6. Active repository files contain no stale `rollshot-cli`,
   `CARGO_BIN_EXE_rollshot`, CLI headless-capture, CLI Action Guide, or
   `ROLLSHOT_APP` references.
7. Historical `docs/superpowers/` snapshots remain unchanged.

