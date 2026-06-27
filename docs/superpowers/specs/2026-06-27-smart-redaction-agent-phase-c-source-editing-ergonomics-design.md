# Smart Redaction Agent Phase C Source Editing Ergonomics

**Date:** 2026-06-27
**Status:** As-built; documents an implementation that ran ahead of its spec and
records the resolution of the Phase C open design decision for user ratification

## Why This Spec Exists

Phases A, B1, B2, and B3 each landed with a design spec before implementation.
Phase C was implemented on `feat/smart-redaction-agent-harness-roadmap` directly
in the working tree without that step, and in doing so it silently resolved the
one open design decision the roadmap had explicitly deferred to Phase C:

> Whether source editing should use exact-replace, unified diff, or AST-aware
> operations in Phase C.

The implementation is sound and well-tested. This spec backfills the missing
design record so the exact-replace decision is documented and ratified rather
than chosen by code alone. It describes the design **as built**, not a new
direction.

## Goal

Move the authoring agent from whole-source replacement toward code-agent-style
editing:

- The agent can read the current source and its environment before editing.
- The agent can make small, anchored edits instead of regenerating the whole
  program every turn.
- Source changes are surfaced as diffs in the run stream and the review UI so a
  user can inspect preset edits like code.

`replace_source` stays as a low-level escape hatch for full rewrites; normal
model behavior is steered toward smaller edits.

## Open Decision Resolution — Exact-Replace

The roadmap left edit semantics open between exact-replace, unified diff, and
AST-aware operations. Phase C chose **exact-replace with a uniqueness
requirement** (`edit_source { generation, old, new }`, where `old` must match
exactly once).

Rationale:

- **Preset detectors are short.** Generated detector scripts are typically a
  single `main(input)` function under ~50 lines. Unique-anchor exact replacement
  is reliable on programs this small, and a full rewrite via `replace_source`
  remains cheap when a structural change is clearer.
- **Clean, unambiguous failure modes.** Exact-replace either finds a unique
  anchor or it does not. Zero matches and multiple matches each return a
  `Recoverable` outcome with actionable text, so the model can self-correct
  without a fuzzy patch applier guessing intent.
- **Matches how code agents already edit.** This is the same contract as the
  harness's own code-editing Edit tool: read, then replace a unique exact
  string. LLMs produce anchored old/new pairs reliably; they produce unified
  diffs with wrong line numbers and non-applying context far more often.
- **Minimal new surface.** No patch-format parser, no hunk fuzzing, no JS AST
  edit layer. `rollshot-automation`'s oxc usage is parse/validate only; an
  AST-aware edit layer would be a large, speculative lift that AGENTS.md §2
  (Simplicity First) argues against for scripts this small.

Rejected alternatives:

- **Unified diff** — compact for multi-hunk edits, but models frequently emit
  diffs whose line numbers and context do not apply cleanly, forcing a fuzzy
  applier and ambiguous failure handling. Not worth it for short scripts.
- **AST-aware operations** — structurally safe but heavy; premature for
  single-function detectors and a new maintenance burden.

This decision is revisitable in a later phase if detector programs grow large
enough that anchored edits become unwieldy.

## Success Criteria

- `read_current_source` (no args) returns the current `generation`, `source`,
  `source_bytes`, the most recent evidence records (kind + source generation),
  and the `validation_summary` only when it matches the current source.
- `edit_source { generation, old, new }`:
  - rejects a stale `generation` with an error and does not mutate source,
  - returns a `Recoverable` outcome when `old` is empty, not found, or matches
    more than once, without mutating source,
  - on a unique match, performs a single exact replacement, advances the
    generation, invalidates evidence/caches from older generations, and returns
    `{ new_generation, diff }`.
- `replace_source` is retained and now also returns a bounded `diff`.
- `edit_source` and `replace_source` success emit
  `RunEvent::SourceChanged { tool, diff }` on the run stream.
- Diffs are bounded: 2 context lines, at most 40 changed lines, at most 160
  characters per line, with an explicit omitted-line count.
- The workbench activity drawer renders `SourceChanged` as a monospace
  `+`/`-`/context/omitted diff (`ActivityEntry::SourceDiff`).
- The authoring prompt steers the loop: `read_current_source` → prefer
  `edit_source` for small changes → `validate_source` → `dry_run` → re-read and
  edit on failure → `submit_for_review` only after successful validate + dry-run
  evidence on the current generation.

## Scope

### In Scope

- `ReadCurrentSourceTool` and `EditSourceTool` in `rollshot-agent`.
- `SourceDiffSummary` / `SourceDiffLine` / `SourceDiffLineKind` and the
  `build_source_diff` line-diff helper.
- `RunEvent::SourceChanged` and the driver wiring that emits it from
  `replace_source` / `edit_source` tool results.
- `replace_source` returning a diff and sharing `clear_generation_caches`.
- Workbench `ActivityEntry::SourceDiff` plus its view rendering.
- Driver authoring-prompt update describing the new loop.
- Registering `read_current_source` and `edit_source` in the product authoring
  registry alongside the retained `replace_source`.

### Out of Scope

- Unified diff or AST-aware edit semantics.
- Multi-edit batching in a single tool call.
- Undo/redo or revision navigation UI beyond the activity diff entry.
- Any change to JavaScript validation, dry-run, or capability inspection.

## Design

### 1. Read Before Editing

`read_current_source` returns `ReadCurrentSourceResult { generation, source,
source_bytes, evidence, validation_summary }`. Evidence is the last 8 records as
`SourceEvidenceSummary { kind, generation }`. `validation_summary` is populated
only when the cached validation's source equals the current source, so the model
never sees a summary for stale source.

### 2. Exact-Replace Editing

`edit_source` locks the draft, checks `generation`, counts matches of `old`, and:

- stale generation → `ToolError::ArgumentDecode` (no mutation),
- `old` empty / 0 matches / >1 matches → `ToolOutcome::Recoverable` with
  actionable text (no mutation),
- unique match → `old_source.replacen(old, new, 1)`, `invalidate_evidence_after`,
  `next_generation`, `build_source_diff`, store new source,
  `clear_generation_caches`, return `{ new_generation, diff }`.

Requiring a unique `old` removes the ambiguity that a first-match-wins replace
would introduce.

### 3. Bounded Diffs

`build_source_diff` computes a common prefix/suffix line range and emits context
(2 lines), removed, added, and a trailing omitted-count line, with per-line
character truncation. The same summary feeds both the tool result and the run
event, so the run stream and the review UI show identical diffs.

### 4. Run Event and Review UI

The driver, on a successful `replace_source` / `edit_source` result, decodes the
`diff` field and emits `RunEvent::SourceChanged { tool, diff }`.
`event_to_activity_entry` maps it to `ActivityEntry::SourceDiff { tool, lines }`,
which the workbench view renders as a labeled monospace block.

### 5. Prompt Contract

The authoring guide's inspection loop is updated to: read current source first,
prefer `edit_source` with unique exact old/new text for small changes, fall back
to `replace_source` only for full rewrites, then validate, dry-run, and submit
under the existing generation-evidence rule.

## Error Handling

- Stale generation is a hard error; the model must re-read before retrying.
- Empty, missing, or non-unique `old` is recoverable, not fatal.
- Replacement invalidates all older-generation validation, dry-run, proposal,
  and submission state via `clear_generation_caches`.

## Testing

- `read_current_source_returns_source_generation_and_evidence`.
- `edit_source_exact_replace_advances_generation_and_returns_diff`.
- `edit_source_rejects_stale_generation_without_mutating_source`.
- `edit_source_recovers_when_exact_text_is_missing`.
- `replace_source` result includes a diff with old/new generation.
- Driver test asserts `RunEvent::SourceChanged` is emitted with removed/added
  lines.
- Workbench `event_to_activity_entry` maps `SourceChanged` to a `SourceDiff`
  entry containing a `+ new` line.

## Verification Commands

```bash
rtk cargo test -p rollshot-agent edit_source
rtk cargo test -p rollshot-agent read_current_source
rtk cargo test -p rollshot-agent source_changed
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo fmt --check
```

## Risks

- A non-unique `old` could be a frequent model error on repetitive scripts; the
  recoverable "matched N ranges" message must stay actionable so the model adds
  more anchor context rather than thrashing.
- The bounded diff truncates large changes; the omitted-line count must remain
  visible so reviewers know a change was larger than shown.
- Exact-replace becomes awkward if detector scripts grow large; revisit edit
  semantics before that happens rather than after.

## Deferred Work

- Unified-diff or AST-aware editing, if detector programs outgrow anchored
  edits.
- Revision navigation / undo UI in the workbench.
- Multi-edit batching.
