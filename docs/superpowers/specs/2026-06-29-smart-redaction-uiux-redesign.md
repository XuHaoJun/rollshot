# Smart Redaction UI/UX Redesign

Date: 2026-06-29

## Goal

Redesign the Smart Redaction workbench so it feels like one focused review
surface instead of three loosely related panes. The user should be able to:

- read agent output and type follow-up instructions in one stable place;
- understand which proposed redactions are pending, low-confidence, or rejected;
- map each candidate chip to the matching rectangle on the screenshot;
- apply the accepted redactions with one clearly primary action;
- revise the proposal without losing context.

This spec is standalone. It does not rely on any external critique HTML, PDF,
image, or mockup file.

## Current Problems

The current workbench has these user-facing issues:

1. The conversation is split. Agent activity appears in a left drawer, while the
   composer sits at the bottom of the right candidate pane.
2. The composer is not pinned. It is inside the same scrollable area as the
   candidate list, so it can move away as content grows.
3. The status model is duplicated. Run status, review status, warnings, and
   apply actions appear in stacked rows above the canvas.
4. Candidates are plain rows. They do not provide a strong visual connection to
   the canvas overlays.
5. Low-confidence candidates are counted but not visually prioritized.
6. The left activity drawer appears only after activity starts, changing the
   canvas width during the workflow.
7. Action hierarchy is weak. Apply, revise, and warning navigation are visually
   similar despite having different risk and importance.

## Target Layout

When Smart Redaction mode is active, the workbench uses one stable layout for
idle, running, and review states:

```text
+--------------------------------------------------------------------------+
| Existing result toolbar                                                   |
+------------------------------------------------------+-------------------+
|                                                      | Smart Redaction   |
|                                                      | panel             |
| Canvas / screenshot / candidate overlays             |                   |
|                                                      | activity stream   |
|                                                      |                   |
|                                                      | pinned composer   |
+------------------------------------------------------+-------------------+
| Candidate review bar                                                     |
+--------------------------------------------------------------------------+
```

The right Smart Redaction panel is always visible while the workbench is open.
It must not collapse to zero width before a run starts. The canvas width should
remain stable across idle, running, ready-for-review, error, and revised states.

The workbench should be built from standard iced widgets and style closures.
No custom iced widget is needed for the panel or review bar. The existing canvas
overlay remains the right surface for drawing candidate rectangles.

## Components

### Smart Redaction Panel

The right panel replaces both the current left activity drawer and the current
right candidate/composer pane.

It contains:

- a compact header with "Smart Redaction", provider/model label when available,
  and a state label such as `Ready`, `Running`, `Ready for review`, or an error;
- the agent conversation/activity stream in a scrollable middle region;
- a pinned composer at the bottom.

The composer must stay visible at the bottom of the panel. It is disabled while
a run is active, matching the existing send guard. When disabled, it should read
as unavailable rather than disappearing.

Activity entries keep the existing data model:

- user messages;
- assistant text;
- tool cards with running/success/failure status;
- source diffs;
- run status entries;
- terminal labels.

The UI can restyle these entries, but it should not require new agent runtime
events.

### Candidate Review Bar

The bottom review bar replaces the current stacked review/status rows and the
candidate list pane.

It contains:

- a short summary: total candidate count, apply count, rejected count, and
  low-confidence count;
- horizontally arranged candidate chips;
- secondary `Revise` action;
- primary `Apply N redactions` action;
- warning navigation only when low-confidence candidates exist.

The apply action is the only primary filled action in the bar. `Revise` is
secondary. Warning navigation is visually smaller than Apply and Revise because
it is a navigation aid, not a commit action.

If there are no candidates, the bar shows a calm empty state and does not expose
an enabled Apply action.

### Candidate Chips

Each non-rejected candidate is represented by a chip in the review bar.

Each chip shows:

- a stable sequence number, starting at 1 in proposal order;
- the candidate label;
- confidence as a percentage;
- warning treatment when confidence is below `0.75`.

Chip state:

- normal pending/accepted: green accent;
- low confidence: amber accent;
- selected: stronger outline or filled sequence badge;
- rejected: muted with strike-through or hidden from the active chip group, as
  long as the rejected count remains visible in the summary.

Clicking a chip selects that candidate, using the existing candidate selection
message path.

### Canvas Candidate Overlays

The existing proposal overlay should be updated so candidates are easy to see on
busy screenshots.

Each visible, non-rejected candidate rectangle uses:

- solid 2 px border;
- translucent fill;
- numbered badge matching the candidate chip sequence;
- green treatment for normal confidence;
- amber treatment for low confidence;
- blue emphasis for selected candidate.

Rejected candidates are not drawn as active overlays. If the implementation
keeps rejected chips visible, rejected overlays should remain hidden to avoid
implying they will apply.

Low-confidence is defined as `confidence < 0.75`, matching the current warning
count logic.

## Interaction Rules

- Entering Smart Redaction opens the stable workbench shell immediately.
- Sending a prompt appends the user message to the panel activity stream and
  disables the composer while the run is active.
- Agent activity streams in the panel, not in a separate left drawer.
- When a proposal is ready, candidate chips appear in the bottom review bar and
  matching numbered overlays appear on the canvas.
- Selecting a chip or canvas overlay sets the selected candidate.
- Rejecting or unrejecting a candidate updates the summary, chip treatment, and
  active overlay set.
- Applying candidates commits all non-rejected candidates through the existing
  apply path and clears pending proposal state.
- Revising remains available only when the reducer already considers revision
  valid: active revision, pending proposal, and non-empty corrections.
- Errors appear in or near the Smart Redaction panel so the error belongs to the
  agent workflow, not as another global banner above the canvas.

## Implementation Shape

Expected touched areas:

- `crates/rollshot-app/src/result_workspace/workbench/view.rs`
  - replace the left activity drawer plus right candidate pane with a stable
    right Smart Redaction panel;
  - move candidate list rendering into the bottom review bar as chips;
  - collapse run status, proposal status, warnings, and actions into one review
    bar;
  - keep disclosure and improve modals working through the existing stack.

- `crates/rollshot-app/src/result_workspace/canvas.rs`
  - update proposal overlay drawing to use confidence colors, translucent fill,
    solid borders, selected emphasis, and numbered badges.

- `crates/rollshot-app/src/result_workspace/workbench/state.rs`
  - add small pure helpers only if needed for counts, low-confidence checks, or
    display ordering.

- Tests in existing workbench/canvas modules
  - cover count/threshold helpers if new helpers are added;
  - cover overlay color/number mapping through pure helper tests when practical;
  - keep existing reducer tests unchanged unless behavior changes.

Do not introduce a new custom iced widget unless standard widgets cannot express
the layout. The expected implementation should be a view rewrite plus small
helpers.

## Non-Goals

- No changes to the agent prompt, provider adapters, automation runtime, or OCR.
- No new candidate review states beyond the existing pending, accepted,
  rejected, and modified states.
- No screenshot/fixture dependency on temporary critique artifacts.
- No redesign of the normal non-workbench editor layout.
- No animated drawer or collapsible panel behavior.

## Verification

Implementation should be verified with:

- `rtk cargo test -p rollshot-app result_workspace::workbench`
- `rtk cargo test -p rollshot-app result_workspace::canvas`
- `rtk cargo fmt --check`

If the implementation touches shared result-workspace behavior outside the
workbench view/canvas overlay, also run:

- `rtk cargo test -p rollshot-app`

Manual UI verification should confirm:

1. Smart Redaction layout is stable before, during, and after a run.
2. Composer remains pinned in the right panel.
3. Agent activity and composer appear in the same panel.
4. Candidate chips map clearly to numbered canvas overlays.
5. Low-confidence candidates are amber in both chip and overlay form.
6. Apply is visually primary and disabled/absent when there is nothing to apply.
7. Rejected candidates no longer appear as active canvas overlays.
