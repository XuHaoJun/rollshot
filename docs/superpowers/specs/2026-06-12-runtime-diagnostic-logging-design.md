# Runtime Diagnostic Logging Design

## Status

Approved design. This spec is live for the next implementation plan.

## Problem

When a capture or stitching bug occurs in a release build, Rollshot usually
does not retain enough diagnostic context to identify the failure. Investigation
then requires adding targeted prints, asking the user to reproduce the issue,
and waiting for another report.

Rollshot needs structured diagnostic events on critical paths that remain
available in release builds and can be enabled at runtime. The default behavior
must remain quiet, and users must be able to write one diagnostic session to an
explicit file that they can share.

## Goals

- Retain debug-level diagnostic instrumentation in release builds.
- Use standard Rust runtime filtering through `RUST_LOG`.
- Keep console logging enabled for every launch.
- Add `--log-file <PATH>` to write the same session to an explicit file.
- Instrument the active capture, stitching, completion, and save paths.
- Record enough structured context to diagnose failures without logging screen
  contents or other sensitive user data.
- Avoid blocking capture and stitching work on file I/O.

## Non-Goals

- Do not add rotating logs, retention policies, or automatic cleanup.
- Do not write log files by default.
- Do not add an in-app log viewer or an "export diagnostics" workflow.
- Do not dynamically change filters after process startup.
- Do not add telemetry, remote collection, crash reporting, or OpenTelemetry.
- Do not instrument the deprecated Tauri capture path in the first phase.
- Do not log captured pixels, image payloads, clipboard contents, or annotation
  contents.

## Selected Approach

Use `tracing` for structured spans and events and initialize
`tracing-subscriber` once in each user-facing executable.

`RUST_LOG` controls which events are enabled. `--log-file <PATH>` controls
whether a second output destination is added. These controls remain independent:

```bash
# Default release behavior: warnings and errors on the console.
rollshot-app

# Debug diagnostics on the console.
RUST_LOG=warn,rollshot=debug rollshot-app

# Debug diagnostics on the console and in one explicit file.
RUST_LOG=warn,rollshot=debug rollshot-app --log-file ./rollshot-debug.log
```

The file is created or truncated at startup. Each invocation therefore produces
one self-contained diagnostic session. File output uses a non-blocking writer
and retains its flush guard until process exit.

No compile-time `release_max_level_*` feature may remove debug or trace events
from release binaries.

## Alternatives Considered

### Custom `DEBUG` Environment Variable

A custom variable could toggle all debug output, but it would duplicate an
established Rust convention and would not support module-specific filtering.
`RUST_LOG` is more expressive and works directly with `EnvFilter`.

### Default Rotating Log Directory

Rotating logs are useful for long-running servers and background daemons. They
would require Rollshot to define storage locations, retention, cleanup, and
support instructions. Rollshot's capture sessions are short, so an explicit
single-session file is simpler and better matches the current diagnostic need.

### Console-Only Logging

Console-only logging is sufficient during development but unreliable for
desktop GUI launches, where the user may not have access to the process stderr.
An explicit file gives users a predictable artifact to share.

## Configuration And CLI Behavior

The application subscriber uses an `EnvFilter` with:

- `RUST_LOG` directives when the variable is present and valid.
- A default directive appropriate for normal use when `RUST_LOG` is absent.
- Lossy parsing for an invalid `RUST_LOG` value: invalid directives are
  ignored, startup proceeds, and one `warn` event listing the ignored
  directives is emitted immediately after initialization so the problem is
  visible on the console and in any diagnostic file. `RUST_LOG` is a shared
  environment variable; a value set for another program must not prevent
  Rollshot from launching.

Filter selection is implemented as a pure function that accepts the raw
`RUST_LOG` value as `Option<&str>` and returns the chosen filter plus any
ignored directives. Only `main` reads the real environment; unit tests pass
strings directly and never mutate process environment variables.

The default filter is `warn` for release and debug builds. Developers who want
debug output opt in with `RUST_LOG`. This keeps behavior consistent between
build profiles and avoids noisy output during ordinary development commands.

`--log-file <PATH>`:

- Adds file output without disabling console output.
- Does not create missing parent directories; a missing parent is reported as
  a startup error.
- Creates or truncates the named file at startup.
- Fails startup with a clear error if the file cannot be opened.
- Does not imply a log level. A user normally combines it with `RUST_LOG`.

The logging arguments belong to the top-level active product executable,
`rollshot-app`. Standalone developer binaries may initialize console logging,
but do not need `--log-file` unless an existing use case requires it during
implementation.

## Architecture

Subscriber initialization and the file-writer guard live in a module inside
`rollshot-app` (for example `src/diagnostics.rs`). Today only `rollshot-app`
needs the full console-plus-file initialization: `rollshot-cli` keeps its own
user-facing progress output, and the deprecated Tauri app is out of scope.
Extract a shared crate only if a second active executable later needs
identical initialization.

Library crates emit `tracing` spans and events but never initialize a global
subscriber. User-facing executables parse the logging options first, initialize
the subscriber, and then run capture behavior.

Console and optional file output use separate formatting layers with the same
runtime filter:

- Console output is compact and human-readable.
- File output is structured JSON Lines so fields remain reliably searchable.
- File writes use `tracing-appender`'s non-blocking writer.

Every diagnostic session starts with one event containing safe execution
context: Rollshot version, operating system, architecture, selected backend,
requested FPS, cursor setting, and whether file logging is enabled. It must not
include environment dumps or arbitrary command-line arguments.

## Stable Diagnostic Targets

Rollshot defines explicit `tracing` targets instead of exposing Rust crate and
module paths as its diagnostic interface. These target names are stable support
controls: internal modules may move without changing the documented
`RUST_LOG` directives used to investigate a subsystem.

The first-phase target taxonomy is:

| Target | Scope |
| --- | --- |
| `rollshot::app` | Product launch, completion, and top-level failures |
| `rollshot::overlay` | Shared iced overlay lifecycle and interaction state |
| `rollshot::capture` | Backend-independent capture decisions and outcomes |
| `rollshot::capture::linux::portal` | Linux portal lifecycle and negotiation |
| `rollshot::capture::linux::pipewire` | Linux PipeWire stream and frame handling |
| `rollshot::capture::macos::sck` | macOS ScreenCaptureKit capture behavior |
| `rollshot::stitch` | Stitch session lifecycle and outcomes |
| `rollshot::stitch::matcher` | Candidate search and match selection |
| `rollshot::stitch::verifier` | Match verification decisions |
| `rollshot::stitch::canvas` | Canvas growth, append, and memory behavior |
| `rollshot::save` | Auto-save, explicit save, and result handoff |

Examples:

```bash
# Diagnose capture without enabling stitching details.
RUST_LOG=warn,rollshot::capture=debug rollshot-app

# Diagnose stitch decisions and inspect matcher frame-level details.
RUST_LOG=warn,rollshot::stitch=debug,rollshot::stitch::matcher=trace rollshot-app

# Enable debug events across all Rollshot targets.
RUST_LOG=warn,rollshot=debug rollshot-app
```

Every first-phase diagnostic event must use the narrowest applicable explicit
target. When no child target applies, the event uses its nearest parent target,
such as `rollshot::capture` or `rollshot::stitch`. New targets should only be
added for a distinct support-facing diagnostic domain, not merely because a new
Rust module exists.

Explicit targets are enforced mechanically, not by convention. The default
`tracing` target is the Rust module path (for example
`rollshot_core::matcher`), which lies outside the `rollshot::` namespace, so
an event that omits `target:` silently escapes every documented directive,
including `rollshot=debug`. Each instrumented crate therefore defines its
documented targets as string constants, diagnostic events reference those
constants, and a test or CI check scans instrumented crates for `tracing`
macro calls that omit an explicit `target:`.

Renaming or removing a documented target is a compatibility change for support
instructions and must be intentional. Adding a child target is compatible
because parent directives continue to enable its events.

## Critical Path Instrumentation

The first phase instruments the active product flow across both Linux and macOS:

### Application And Overlay Lifecycle

- Product launch and parsed capture configuration.
- Overlay start, cancellation, successful completion, and failure.
- Capture mode and important phase transitions.
- Final image dimensions and completion disposition.

### Capture

- Backend selection and backend startup outcome.
- Permission, portal, ScreenCaptureKit, stream, timeout, and end-of-stream
  outcomes where they affect control flow.
- Frame dimensions, format metadata, sequence number, and elapsed timing at
  sampled or transition-based points.
- Crop region dimensions and coordinate mapping outcomes.

Per-frame success events must not create uncontrolled high-volume output at
`debug`. Repetitive frame-level details belong at `trace`, or must be sampled.
Errors and significant state transitions belong at `warn`, `error`, `info`, or
`debug` as appropriate.

### Stitching

- Stitch session start and final statistics.
- Significant `StitchOutcome` transitions.
- `NoMatch`, axis changes, capture misses, and terminal errors.
- Existing safe `StitchMetrics`, including timing, candidate counts, selected
  offsets, scores, canvas dimensions, and allocated bytes.

Detailed per-frame stitching metrics belong at `trace`. `debug` should remain
useful for a normal reproduction session without producing excessive output.

### Completion And Save

- Capture completion disposition.
- Auto-save and explicit save start/success/failure.
- Output dimensions and encoded byte counts where available.

File paths are sensitive. Log only a coarse destination category or a redacted
filename unless the user explicitly supplied the diagnostic log path itself.

## Migration Of Existing Diagnostics

The instrumented paths already emit ad-hoc `eprintln!` diagnostics (portal and
PipeWire errors in `rollshot-capture`, launch and product errors in
`rollshot-app`, overlay diagnostics in `rollshot-iced-overlay`). The first
phase migrates these diagnostic prints to `tracing` events so the console does
not double-report and the diagnostic file does not miss information that only
`eprintln!` carried.

The per-frame progress output and diagnostics summary in `rollshot-cli` are a
user-facing interface controlled by `--quiet`, not diagnostics; they remain
`eprintln!` and are out of scope.

## Event Design

Prefer structured fields over interpolated prose:

```text
debug!(
    target: TARGET_STITCH,
    frame_index,
    outcome = ?outcome_kind,
    best_dx,
    best_dy,
    best_score,
    total_us,
    "processed stitch frame"
)
```

Use stable event messages and field names so logs from different reports can be
compared. Attach related events to spans such as a capture session or stitch
session when that improves causality. Do not create spans around tiny hot-path
operations solely for timing; use existing metrics where available.

Levels have these meanings:

- `error`: the operation cannot continue or the result is unusable.
- `warn`: recoverable abnormal behavior relevant to a user-visible problem.
- `info`: low-volume lifecycle milestones.
- `debug`: diagnostic decisions and significant state changes.
- `trace`: high-volume frame-level details and deep algorithm metrics.

## Privacy And Safety

Diagnostic events must not contain:

- Captured image pixels or encoded image data.
- OCR-like text or data derived from screen contents.
- Clipboard or annotation contents.
- Full environment-variable dumps.
- Authentication tokens or IPC payloads.
- Full user file paths by default.

Safe fields include dimensions, coordinates, backend names, enum variants,
counts, durations, scores, error categories, and stable diagnostic targets.
When an underlying error may contain sensitive payloads, log a classified error
and expose the original message only after reviewing that error type.

## Failure Handling

Subscriber initialization failures are startup errors because silently losing a
requested diagnostic file defeats the feature.

After initialization, logging failures must not interrupt capture or stitching.
The non-blocking writer may drop events if its bounded queue fills; this is
preferable to blocking the capture path. The implementation should expose a
lost-event counter or emit a final warning when supported without adding a
custom logging system.

The writer guard must live until application shutdown so buffered events are
flushed.

Executables that initialize the subscriber must terminate by returning an exit
code from `main` (for example `std::process::ExitCode`) rather than calling
`std::process::exit`, which skips `Drop` and discards buffered events on
exactly the failing paths diagnostics exist for. The existing
`std::process::exit(1)` call sites in `rollshot-app` are migrated as part of
this work.

## Testing And Verification

Automated tests should cover:

- Default filter selection when `RUST_LOG` is absent.
- Valid and invalid `RUST_LOG` parsing, including the post-initialization
  warning event that lists ignored directives. Filter tests exercise the pure
  filter-selection function and never mutate process environment variables.
- Representative events use their documented explicit diagnostic targets.
- A scan check that instrumented crates contain no `tracing` macro calls
  without an explicit `target:`.
- Parent target directives enable events from child targets.
- `--log-file <PATH>` argument parsing.
- File creation/truncation and startup failure for invalid paths.
- Console output remains configured when file output is enabled.
- Representative structured events do not expose image data or full save paths.
- A subprocess integration test that runs the binary with `--log-file` down a
  failing exit path and asserts a non-zero exit code and a complete file
  containing the session-start and final error events.

Integration verification should run a release build and confirm:

- Debug events are absent under the default filter.
- `RUST_LOG=warn,rollshot=debug` enables debug events across Rollshot.
- Fine-grained capture and stitching directives enable only their target
  subtrees.
- `--log-file <PATH>` writes a complete session while console logging remains
  active.
- Debug events are present in the release binary and visible when enabled.

Manual runtime verification must cover both active platform paths:

- Linux iced overlay capture through the portal/PipeWire path.
- macOS iced overlay capture through ScreenCaptureKit.

The implementation should run relevant workspace tests, formatting, and clippy.
Changes that add instrumentation inside `rollshot-core` stitching hot paths must
also run the stitching benchmark workflow to ensure disabled logging has no
material performance regression.
