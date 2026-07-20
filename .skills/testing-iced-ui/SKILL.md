---
name: testing-iced-ui
description: Use when building, modifying, debugging, or reviewing user-visible iced UI, including layout, interaction, responsive behavior, visual regressions, screenshots, golden baselines, Simulator, Emulator, or native window behavior.
---

# Testing iced UI

## Overview

Treat UI artifacts as evidence. Verify visible behavior through deterministic state, structural assertions, interactions, and inspected images.

**REQUIRED SUB-SKILL:** Use `iced-rs` for iced 0.14 APIs and invariants.
**REQUIRED BACKGROUND:** Use `superpowers:test-driven-development` for product changes and `superpowers:verification-before-completion` before handoff.

## Modes

- **Auto (default):** The agent chooses coverage; an independent reviewer may accept baselines.
- **Human:** Only when explicitly requested. Produce evidence; do not write baselines before approval.

## Workflow

1. Before editing, define deterministic scenarios for the user-visible behavior: affected open/expanded/error states, default and minimum windows, and relevant long-content/platform cases.
2. Write the smallest failing semantic, layout, or interaction assertion. A passing existing test is not RED.
3. Choose the lowest sufficient layer:

   | Boundary | Harness |
   |---|---|
   | Widget tree, bounds, clipping, messages | `iced_test::Simulator` |
   | `Program`, `Task`, `Subscription`, multi-step flow | `iced_test::Emulator` + `Preset` |
   | DPI, focus, clipboard, dialogs, compositor, capture | native platform smoke |

   Do not make experimental `.ice` files the primary suite. Check Linux and macOS paths for capture UI changes.
4. Select by user-facing text or stable widget ID; use coordinates only for Canvas/geometry. Assert targets are visible, enabled, and unobscured. Never replace state checks with sleeps.
5. Pin backend, fonts, theme, viewport, fixture data, and isolated temp storage. Keep structural assertions primary because pixels vary by environment.
6. For each visual scenario, produce baseline/expected, actual, and diff images. The main agent must inspect them with `view_image`; green tests alone are insufficient. Unexplained renderer, font, DPI, or platform drift fails.
7. Run focused tests, then proportional crate/workspace tests, formatting, clippy, and `git diff --check`.
8. Send baseline decisions to a clean-context subagent using the contract below. On rejection, fix the product or scenario and repeat.

## Baseline reviewer contract

The main agent must not write baselines. Spawn a subagent with `fork_turns="none"` and provide only:

- the user-visible requirement and auto/human mode;
- changed-file list and scenario manifest;
- baseline, actual, and diff paths;
- semantic/interaction test output;
- exact allowed baseline paths and update command.

Omit the main agent's verdict. The reviewer inspects every image, then rejects with concrete findings or accepts. In auto mode it may update only allowed baseline files. In human mode it returns a verdict without writes; after user approval, a new clean-context reviewer performs the allowed update. It never edits product code, assertions, or scenarios.

## Completion report

Report scenarios/viewports, interactions, inspected artifacts, reviewer verdict, baseline changes, native coverage, commands, and remaining risk.

## Common mistakes

- Optional screenshots or “manual visual check” in the default path
- Reaching a state through a long dependent flow instead of a preset fixture
- Exact pixel hashes as the only assertion
- Updating a baseline to silence clipping, overlap, or unrelated diffs
- Letting the product-changing agent approve its own baseline
