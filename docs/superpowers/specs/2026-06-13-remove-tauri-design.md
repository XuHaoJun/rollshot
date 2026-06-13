# Remove Tauri Design

## Goal

Remove the deprecated Tauri application and all active Tauri-specific repository
support in one change. After removal, `rollshot-app` and
`rollshot-iced-overlay` are the only desktop product path, and the workspace no
longer builds, tests, documents, or carries dependencies for Tauri.

Linux and macOS capture-flow smoke verification has already been completed and
is a prerequisite for this removal.

## Scope

### Remove the deprecated application

Delete the complete tracked `crates/rollshot-tauri-app/` tree, including:

- The Rust/Tauri host and its capture session implementation.
- The React frontend and frontend tests.
- Tauri configuration, generated schemas, icons, and capabilities.
- The pnpm lockfile and all app-local JavaScript tooling configuration.

No Tauri code is migrated during this change. Equivalent active product
behavior already lives in the iced application and overlay crates.

### Remove build and CI support

- Remove `crates/rollshot-tauri-app/src-tauri` from the Cargo workspace.
- Regenerate `Cargo.lock` so packages used only by the removed Tauri crate no
  longer remain.
- Delete the CI frontend job that installs pnpm and builds/tests the deprecated
  React frontend.
- Remove the Tauri crate from macOS target checks.
- Stop installing Linux system packages that were required only by Tauri and
  WebKitGTK. Keep packages required by active capture backends.

### Remove the unused launch API

Delete `rollshot_capture::OverlayMode` and
`InteractiveLaunchOptions::overlay_mode`. Update all active constructors and
tests to use the smaller launch-options contract.

This is an intentional Rust API break. Existing JSON launch payloads that still
contain `overlay_mode` remain accepted because `InteractiveLaunchOptions` does
not deny unknown Serde fields. No compatibility shim or deprecated replacement
field is added.

### Remove active references

Update active repository guidance and source comments so they describe iced as
the sole desktop architecture:

- Remove deprecated-app setup, build, verification, and project-map content
  from `README.md` and `AGENTS.md`.
- Rewrite active Rust comments that describe shared Tauri behavior, retained
  Tauri coexistence, or Tauri-era sources of truth.
- Rename CLI test fixtures that still use `rollshot-tauri-app` as the fake GUI
  binary name.

Comments should describe current ownership directly rather than narrating the
migration.

### Remove the unused reference submodule

Remove the `learn-projects/tauri-template` gitlink and its entry from
`.gitmodules`. The other reference submodules and ignore rules remain
unchanged.

## Preserved History

Do not edit or delete historical and research artifacts solely because they
mention Tauri:

- `docs/**`, except this new live spec and any non-historical active
  documentation explicitly required by implementation.
- `spikes/**`.
- Existing git history.

These files may continue to mention removed paths and former architecture. They
are snapshots, not active build or product guidance.

## Implementation Shape

Perform the removal as one coherent change:

1. Delete the deprecated application and reference submodule.
2. Remove workspace, CI, and active documentation references.
3. Remove `OverlayMode` and update active callers/tests.
4. Regenerate dependency metadata.
5. Verify that active repository surfaces no longer depend on Tauri and that
   the remaining workspace passes its normal checks.

There is no intermediate supported state and no archive branch or compatibility
crate. Git history is the recovery mechanism if old implementation details are
needed later.

## Success Criteria

- `crates/rollshot-tauri-app/` is absent from tracked files.
- `learn-projects/tauri-template` and its `.gitmodules` entry are absent.
- The Cargo workspace and `Cargo.lock` contain no Tauri crate or Tauri package
  entries.
- Active CI contains no frontend/pnpm job, Tauri target check, or
  Tauri-only/WebKitGTK package installation.
- `OverlayMode` and `overlay_mode` are absent from active Rust source.
- `README.md`, `AGENTS.md`, active source comments, and active test fixture
  names no longer describe or depend on Tauri.
- Historical docs and spikes remain unchanged.
- The remaining workspace builds and tests without Node, pnpm, WebKitGTK, or
  Tauri tooling.

## Verification

- Search active surfaces, excluding `docs/**`, `spikes/**`, and
  `learn-projects/**`, for stale `tauri`, `Tauri`, and `rollshot-tauri-app`
  references; investigate every remaining match.
- Confirm `.gitmodules` and submodule status no longer list
  `learn-projects/tauri-template`.
- Confirm `Cargo.lock` contains no package named `tauri` or
  `rollshot-tauri-app`.
- `rtk cargo test --workspace`
- `rtk cargo fmt --check`
- `rtk cargo clippy --workspace --all-targets -- -D warnings`
- `rtk git diff --check`

The previously completed Linux and macOS runtime smoke checks are accepted as
the behavioral parity gate. This removal does not alter active capture or
result-workspace behavior and does not require stitching benchmarks.

## Non-Goals

- Reworking active iced capture, overlay, save, thumbnail, or Result Workspace
  behavior.
- Removing or rewriting historical specs, plans, research documents, or spikes.
- Creating an archive branch, tag, compatibility crate, or Tauri migration
  shim.
- Removing JavaScript tooling outside the deleted Tauri application if another
  active repository surface uses it.
- Refactoring unrelated code discovered while removing stale references.
