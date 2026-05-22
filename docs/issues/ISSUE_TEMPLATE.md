---
title: Short, descriptive issue title
status: open       # open | in-progress | resolved | closed
date: 2026-05-22   # ISO 8601 date the issue was filed
severity: medium   # low | medium | high | critical (optional)
reporter: noah     # GitHub handle or name (optional)
tags: []           # e.g. [capture, stitcher, macos] (optional)
---

# {{ title }}

## TL;DR

One-paragraph summary: what's broken / what's being proposed, and why it
matters. A reader should be able to stop after this section and still know
whether the issue is relevant to them.

## Symptom / Context

What is observed, or what is the situation that motivates this issue. For
bugs: reproduction steps, command output, screenshots. For proposals: the
problem being solved and any constraints.

```
# paste raw command output / logs here when relevant
```

## Analysis

Where the problem lives, what's been ruled out, and any hypotheses. Link
to relevant code with `path/to/file.rs:line`. Cite commits with their
short SHA when bisecting.

## Proposed Resolution

Concrete next steps or design sketch. If multiple options exist, list
them with tradeoffs rather than picking silently.

## Open Questions

- Things that block a decision.
- Things you'd like a reviewer's opinion on.
