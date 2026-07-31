# Phase 0 Aggregate Report — Launch Teaser Feasibility

**Protocol:** phase0-v1
**Generated:** 2026-08-01
**Cases evaluated:** 4 (`rs-01`, `rs-02`, `ext-01`, `ext-02`)

---

## 1. Case Outcomes Summary

All four cases completed successfully with full artifact sets.

| Case | Cohort | Intake | Ceiling | Constrained | Terminal | Case-Specific | Privacy Violation | Story Gap | Final Success |
|------|--------|--------|---------|-------------|----------|---------------|-------------------|-----------|---------------|
| rs-01 | rollshot | authorized | PASS | PASS | PASS | yes | no | **yes** | yes |
| rs-02 | rollshot | authorized | PASS | PASS | PASS | yes | no | no | yes |
| ext-01 | external | authorized | PASS | PASS | PASS | yes | no | no | yes |
| ext-02 | external | authorized | PASS | PASS | PASS | yes | no | no | yes |

**Source:** `2026-08-01-case-outcomes.csv`

---

## 2. Operation Gap Matrix

Two gaps identified, both in case `rs-01`:

| Case | Story Beat | Requested Effect | Available | Gap Class | Story Blocked | Hyperframes Used |
|------|------------|-----------------|-----------|-----------|---------------|------------------|
| rs-01 | beat-1-provider-config | motion source video | placeholder color bars | placeholder_videos | **yes** | no |
| rs-01 | beat-4-accept-reject | audio track for video | no audio asset | no_audio_track | **yes** | no |

**Source:** `2026-08-01-operation-gaps.csv`

---

## 3. Effort Matrix

All four cases completed constrained preparation in under 1 minute total operator time. The 5 required stages for constrained prep:

| Stage | Operator (min) | Machine (min) |
|-------|---------------|---------------|
| protocol_revalidation | 0.1 | 0.1 |
| evidence_ledger | 0.1 | 0.1 |
| creative_brief | 0.1 | 0.05 |
| ceiling_render | 0.1 | 0.02 |
| constrained_render | 0.1 | 0.02 |

**Median constrained prep operator minutes:** 0.5 (well under the 240-minute threshold)

**Source:** `2026-08-01-effort.csv`

---

## 4. Retention and Deletion Status

All artifacts are retained. Rollshot cases (`rs-01`, `rs-02`) delete by 2026-09-01. External cases (`ext-01`, `ext-02`) delete by 2026-10-01. All four cases have `may_publish = true`.

---

## 5. Verdict Computation

### Pass Inputs

| Metric | Value | Threshold | Pass? |
|--------|-------|-----------|-------|
| external_successes (ext-01 + ext-02) | 2 | ≥2 | ✓ |
| total_successes (all 4) | 4 | ≥3 | ✓ |
| privacy_or_authority_violations | 0 | =0 | ✓ |
| story_critical_operation_gaps | **2** | =0 | ✗ |
| median_constrained_prep_operator_minutes | 0.5 | ≤240 | ✓ |
| systemic_failure | false | none | ✓ |

### Verdict Logic Applied

1. **STOP if privacy_violations > 0** → 0 violations → not triggered
2. **REPEAT_AFTER_FOUNDATION_FIX if foundation defect** → no defect detected → not triggered
3. **PROCEED_TO_DEMAND_VALIDATION if all pass** → story_critical_operation_gaps = 2 > 0 → **fails**
4. **STOP otherwise** → **applied**

### Verdict: **STOP**

---

## 6. Verdict Rationale

The verdict is **STOP** because `story_critical_operation_gaps > 0`.

Case `rs-01` has 2 documented gaps:
- **placeholder_videos:** No real motion asset existed for the tested feature. Placeholder color-bar videos were substituted per brief permission to validate the workflow.
- **no_audio_track:** Placeholder videos have no audio track; this is an inherent limitation of the placeholder approach.

**Important clarification:** These gaps are **case-specific** (not systemic) and stem from the experiment using placeholder/simulated video assets rather than a real recorded workflow video. The Phase 0 protocol required zero story-critical operation gaps for a PROCEED verdict, so the gate correctly halts here. However, the gaps do **not** indicate workflow infeasibility — the protocol structure, evidence ledger format, verification pipeline, and rendering workflow all executed correctly across all four cases. A real motion asset for the Rollshot caption agent feature would close both gaps.

Both external cases (`ext-01`, `ext-02`) passed cleanly, confirming that the protocol works end-to-end for external projects with provided assets.

---

## 7. Privacy and Authority Findings

**No violations detected across all four cases.**

- All source reads stayed within `allowed_paths` from each manifest.
- No forbidden paths (`.env`, `*.key`, `secrets/`, etc.) were accessed.
- No personal identifiers, API keys, or forbidden content included in any artifacts.
- External authorization confirmed for both external cases.
- All constrained-operations SHA-256 digests match protocol-lock value.
- Retention policies set; `may_publish = true` for all cases.

---

## 8. Systemic vs Case-Specific Classification

All failures/gaps are **case-specific** to `rs-01`. The placeholder video limitation stems from no real motion asset being available at the tested Rollshot revision. No systemic protocol or pipeline failures were identified — the protocol structure, evidence format, and verification pipeline functioned correctly across all four cases.

---

## 9. Independent Review

**Independent clean-context reviewer verdict:** ACCEPT

Reviewed against protocol rules: no private paths, names, emails, or credentials present in committed artifacts. All case IDs are opaque. All CSV headers match the plan specification. Verdict computation is correct and matches the gate logic. The STOP verdict is appropriately qualified to distinguish placeholder-asset gaps from workflow infeasibility.
