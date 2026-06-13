---
name: rollshot-run-spike
description: Use when Rollshot work has a costly technical unknown that cannot be resolved confidently by reading code, documentation, or a focused automated test, especially for platform, hardware, dependency, integration, or feasibility risks.
---

# Rollshot Run Spike

## Overview

Run the smallest experiment that can support a decision. A spike produces
evidence and a go/no-go recommendation, not production code.

## Decide Whether To Spike

Use a spike only when all are true:

- A specific technical unknown blocks or materially changes a product decision.
- Failure would make the planned implementation wasteful or invalid.
- Existing code, primary documentation, or a focused automated test cannot
  answer it confidently.
- The unknown has an observable success/failure condition.

Do not spike routine feature work, known bugs, ordinary performance tuning, or
questions already answered by product code. Recommend the normal Rollshot
development workflow instead.

## Define The Experiment

Before creating code, state:

- Decision the evidence will support.
- Assumptions and unknowns.
- Ranked risks, with the highest-risk unknown tested first.
- Observable pass/fail criteria and hard gates.
- Required environments, hardware, platforms, and manual observations.
- Timebox or stopping condition.

Stop at a failed hard gate. Record the result instead of building on an invalid
assumption.

## Isolate The Spike

- Create `spikes/<topic>/`.
- For Rust, use a standalone crate with an empty `[workspace]` table. Never add
  it to the root workspace.
- Keep production crates unchanged. If a temporary production edit is required
  to gather evidence, explicitly record it and revert it before committing.
- Prefer direct, disposable code. Do not add abstractions or production-grade
  hardening unless needed to make the evidence valid.
- Follow Rollshot diagnostics rules. Runtime instrumentation uses structured
  `tracing` with stable `rollshot::*` targets.

## Record Evidence

Create `spikes/<topic>/FINDINGS.md` from
`references/findings-template.md`. Treat it as the spike's primary output.

After each milestone, record:

- Exact environment and command.
- Evidence level: `compile`, `automated`, `runtime`, or `hardware`.
- Result: `PASS`, `FAIL`, `MITIGATED`, or `UNTESTED`.
- Observation, artifact paths, caveats, and next decision.

Never promote compile success into a runtime claim. Mark unavailable platform
or hardware checks as `UNTESTED`.

Commit coherent milestones with `spike(<scope>): ...` messages when commits are
requested or the active plan requires them.

## Close The Spike

Finish with:

- Go/no-go decision and the evidence supporting it.
- Rejected alternatives and fallback triggers.
- Remaining risks that product implementation must carry forward.
- Product handoff: what should be implemented, tested, or specified next.
- Lifecycle status.

Use `retained-reference` by default after the decision is consumed. Retained
spikes are historical evidence:

- Do not keep them synchronized with product code.
- Do not treat them as source of truth.
- Do not import or depend on them from production code.
- Do not delete them unless the user explicitly requests cleanup.

## Common Mistakes

| Mistake | Correction |
|---|---|
| Exploring without a decision question | State the blocked decision first. |
| Testing easy capabilities before fatal risks | Test the highest-risk gate first. |
| Treating the prototype as implementation | Hand findings to normal product development. |
| Claiming runtime support from compilation | Label the evidence level precisely. |
| Continuing after a failed hard gate | Stop and document the fallback. |
| Maintaining a completed retained spike | Preserve it as historical evidence only. |
