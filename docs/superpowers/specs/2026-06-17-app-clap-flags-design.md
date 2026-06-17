# rollshot-app: unify launch args as clap flags

## Problem

`rollshot-app` parses its launch arguments by hand in `crates/rollshot-app/src/launch.rs`.
The capture path is driven by a single JSON blob:

```bash
rollshot-app --capture '{"backend":"auto","fps":5,"show_cursor":false,"initial_request":{"workflow":"scrolling","scope":"region"}}'
```

This is hard to read, hard to write, and inconsistent with `rollshot-cli` (binary
`rollshot`), which already uses a clean clap subcommand + `--flag value` surface.
The `README.md` examples mix the two styles, and several examples do not make it
obvious which binary they belong to.

## Goal

Replace `rollshot-app`'s hand-rolled JSON launch interface with a clap-derived
subcommand + flag surface that mirrors `rollshot-cli`, remove the `--capture`
JSON interface entirely, and clean up the `README.md` examples so every example
clearly states which binary it runs.

`rollshot-cli` is already flag-based and its behavior does not change.

## Decisions (resolved during brainstorming)

1. **Parser:** `rollshot-app` adopts `clap` (derive), consistent with `rollshot-cli`.
2. **JSON compatibility:** the `--capture '{json}'` interface is removed entirely,
   no deprecated alias. This is an internal/dev launch interface (README manual
   testing), so the break is acceptable.
3. **Command structure:** all subcommands, aligned with `rollshot-cli`. `capture`
   is an explicit subcommand; running `rollshot-app` with no subcommand defaults
   to capture with all default options.
4. **Workflow/scope:** two independent flags, `--workflow` and `--scope`.

## Command surface

```
rollshot-app [--log-file <PATH>]                         # no subcommand -> capture, all defaults
rollshot-app capture [--log-file <PATH>] \
    --backend <auto|fixture|linux-kwin|linux-portal|macos-sck> \
    --fps <N> --show-cursor \
    --workflow <screenshot|scrolling> --scope <region|fullscreen>
rollshot-app action-guide [--log-file <PATH>] [--fullscreen]
rollshot-app action-guide-probe [--log-file <PATH>]
```

Defaults match the current `InteractiveLaunchOptions::default_capture()`:
`backend=auto`, `fps=5`, `show_cursor=false`, `workflow=scrolling`, `scope=region`.

- `--log-file` is a top-level `#[arg(global = true)]` option, available on every
  subcommand and on the no-subcommand form.
- `--workflow` accepts only `screenshot` and `scrolling`. The `action-guide`
  workflow has its own subcommand and is not selectable via `capture --workflow`.
- The existing `CaptureRequest::is_supported()` check still rejects
  `scrolling + fullscreen` with a clear error.
- `action-guide` and `action-guide-probe` subcommands are gated behind the
  existing `action-guide` cargo feature, exactly as the current modes are.

## Implementation

### `crates/rollshot-app/src/launch.rs`

- Define clap-derived types:
  - `LaunchCli { #[arg(global)] log_file: Option<PathBuf>, #[command(subcommand)] command: Option<LaunchCommand> }`
  - `LaunchCommand` enum: `Capture(CaptureArgs)`, and (feature-gated)
    `ActionGuide { fullscreen: bool }`, `ActionGuideProbe`.
  - `CaptureArgs` with `backend` (restricted `value_parser`), `fps`, `show_cursor`,
    `workflow`, `scope`.
  - `workflow`/`scope` parsed via `clap::ValueEnum` mapping onto the existing
    `Workflow`/`CaptureScope` (only `screenshot`/`scrolling` exposed for workflow).
- Keep the existing `LaunchMode` enum and `LoggingArgs` shape as the boundary to
  the rest of the app so `main.rs` downstream code (`run_iced_capture`,
  `run_action_guide_record`, `run_action_guide_probe`) is untouched.
- Replace `parse_launch_args` with a function that parses via clap
  (`LaunchCli::try_parse_from`) and lowers `LaunchCommand` (or `None` ->
  `default_capture()`) into `LaunchMode`, applying the `is_supported()` check.
- Remove `extract_logging_args` (the JSON-era two-pass splitter), all JSON
  parsing, the `initial_mode`/`initial_request` string handling, and their
  bespoke error strings.

### `crates/rollshot-app/src/main.rs`

- New flow: clap-parse once -> read `log_file` -> init diagnostics -> dispatch
  `LaunchMode`.
- On clap parse error, clap prints usage/error to stderr and exits. This happens
  before the tracing subscriber is initialized, which is the allowed
  pre-subscriber stderr case (AGENTS.md §7).
- The `capture session started` tracing event and the `LaunchMode` dispatch match
  arms are preserved.

### `crates/rollshot-app/Cargo.toml`

- Add `clap = { workspace = true }`.
- Remove `serde_json = { workspace = true }` (only `launch.rs` used it).

### Tests (`launch.rs`)

Rewrite the existing unit tests onto the flag surface:

- no-args -> default capture (`auto`, `fps=5`, `!show_cursor`, scrolling+region)
- `capture --backend macos-sck --fps 30` -> parsed correctly
- `capture --workflow screenshot --scope fullscreen` -> screenshot_fullscreen
- `capture --workflow scrolling --scope fullscreen` -> rejected (`is_supported`)
- unknown flag / unknown subcommand -> error (clap)
- `action-guide` / `action-guide --fullscreen` -> correct `LaunchMode` (feature-gated)
- `action-guide-probe` -> correct `LaunchMode` (feature-gated)

Remove tests tied to the deleted JSON interface and the two-pass `--log-file`
splitter (`ignores_obsolete_capture_option`, `rejects_missing_capture_payload`,
`rejects_invalid_json`, `extracts_log_file_before_capture_args`,
`rejects_missing_log_file_path`, `rejects_duplicate_log_file`,
`fullscreen_capture_request_payload_parses`,
`legacy_initial_mode_payload_is_rejected_clearly`,
`unsupported_scrolling_fullscreen_payload_is_rejected`, and the `main.rs`
`save_dialog_temp` / unsupported-platform JSON tests — replace the platform test
with a flag-based equivalent).

### `README.md`

- Rewrite the KDE Native Capture verification/one-shot examples and the
  "Backend selection and fallback behavior" examples from `--capture '{json}'`
  to the new flags.
- Delete the `#### initial_request JSON` subsection; replace with a short
  `--workflow` / `--scope` description that keeps the existing semantics notes:
  - default is `scrolling` + `region`,
  - `scrolling + fullscreen` is expressible-but-unwired and returns an error,
  - fullscreen scope captures the pointer's display (macOS and KDE/KWin only;
    other Linux without portal fallback returns `Unsupported`).
- Ensure every command example states its binary: `rollshot` for `rollshot-cli`
  examples, `rollshot-app` for app examples. No behavior text beyond the flag
  rewrite.

## Out of scope

- No change to `rollshot-cli` flags or behavior.
- No change to `InteractiveLaunchOptions` / `CaptureRequest` types in
  `rollshot-capture` (they keep their serde derives for other consumers).
- No change to capture/overlay/stitch runtime behavior; this is a launch-surface
  refactor only.

## Verification

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p rollshot-app`
- Spot-check both default-feature and `--features action-guide` builds for
  `rollshot-app` so the feature-gated subcommands compile.
