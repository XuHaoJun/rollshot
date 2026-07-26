# Provider Boundary Reliability Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: Rollshot agent-foundation Gate G0
- Started: 2026-07-26
- Last updated: 2026-07-26

## Decision

Determine whether Rig 0.39 or 0.40 distinguishes normal provider completion from incomplete EOF strongly enough to proceed with the host-owned reliability fix.

## Environment

- Evidence scope: local compile, automated, and runtime evidence
- Live providers: UNTESTED and out of scope
- Hardware: UNTESTED and not required

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| Normal versus partial completion | H2 hard | runtime | UNTESTED | `fixtures/cases.json` |
| Host wakes ignored bounds | H1 hard | automated | UNTESTED | Production tests after H2 |
| Rig 0.40 compatibility | H3 upgrade | compile/automated | UNTESTED | Conditional after H1/H2 |

## Observations

No probe command has run yet.

## Final Recommendation

- Go / no-go: UNTESTED — blocked on H2 probes
- Supporting evidence: UNTESTED — no runtime observation recorded
- Rejected alternatives: provider trait redesign; Rig patch/fork; transport rewrite; live-provider acceptance
- Fallback triggers: H2 failure stops this plan; H3 failure retains a passing 0.39 path when available
- Remaining risks: external provider cost; live infrastructure latency; socket cleanup; interrupted-stream billing
- Product handoff: UNTESTED — no handoff before H2
