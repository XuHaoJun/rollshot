---
name: testing-iced-ui
description: Use when building, modifying, debugging, or reviewing user-visible iced UI, including layout, interaction, responsive behavior, visual regressions, screenshots, golden baselines, Simulator, Emulator, or native window behavior.
---

# Testing iced UI

## Overview

Verify UI through deterministic state, structural assertions, interactions, and inspected images.

**REQUIRED SUB-SKILL:** Use `iced-rs` for iced 0.14 APIs.
**REQUIRED BACKGROUND:** Use `superpowers:test-driven-development` and `superpowers:verification-before-completion`.

## Modes

- **Auto (default):** The agent chooses coverage; an independent reviewer may accept baselines.
- **Human:** Only when explicitly requested. Produce evidence; do not write baselines before approval.

## Visual capability preflight

Before planning scenarios, inspect a known local PNG and report exactly:

```text
Visual capability: semantic | pixel-only | none
Provider: native:<tool> | mcp:<server/tool> | cli:<command> | none
Probe: <path> — passed | failed
Pixel diff: <tool> | none
CI: semantic | artifact-only
```

Count `semantic` only after interpreting an image returned by the provider. Metadata, capture, hashes, and ImageMagick are `pixel-only`. Without it, block auto acceptance; use a capable reviewer or human mode. Mark CI `semantic` only when its job runs a verified semantic agent; otherwise use `artifact-only`.

## Workflow

1. Before editing, define deterministic affected states, default/minimum windows, and relevant long-content/platform cases.
2. Write the smallest failing semantic, layout, or interaction assertion. A passing existing test is not RED.
3. Choose the lowest sufficient layer:

   | Boundary | Harness |
   |---|---|
   | Widget tree, bounds, clipping, messages | `iced_test::Simulator` |
   | `Program`, `Task`, `Subscription`, multi-step flow | `iced_test::Emulator` + `Preset` |
   | DPI, focus, clipboard, dialogs, compositor, capture | native platform smoke |

   Avoid experimental `.ice` files as the primary suite. Check both capture UI platforms.
4. Select by text or stable ID; use coordinates only for Canvas. Assert visible, enabled, and unobscured targets. Never sleep for state.
5. Pin backend, fonts, theme, viewport, fixtures, and temp storage. Keep structural assertions primary.
6. Produce baseline/expected, actual, and diff per visual scenario. Inspect each semantically; green tests alone are insufficient. Unexplained environment drift fails.
7. Run focused and proportional tests, formatting, clippy, and `git diff --check`.
8. Send baseline decisions to a clean-context subagent using the contract below. On rejection, fix the product or scenario and repeat.

## Baseline reviewer contract

The main agent must not write baselines. Spawn with `fork_turns="none"` and provide only:

- requirement, mode, changed files, and scenario manifest;
- baseline, actual, diff, and semantic test output;
- allowed baseline paths and exact update command.

Omit the main verdict. The reviewer inspects every image, then rejects or accepts. Auto mode may update only allowed baselines. Human mode returns a verdict without writes; after approval, a new clean reviewer updates. It never edits code, assertions, or scenarios.

## Completion report

Report the capability block, scenarios/viewports, interactions, inspected artifacts, reviewer verdict, baseline changes, native coverage, commands, and risk.

## Common mistakes

- Treating screenshot capture or pixel diff as semantic inspection
- Using optional screenshots, dependent setup flows, or exact hashes alone
- Updating baselines to silence regressions or self-approving them
