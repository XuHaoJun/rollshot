---
name: plan-ceo-review
description: |
  Product/founder review for a feature spec or plan. Challenge the premise,
  pressure-test scope, and judge the user-facing experience with strong product
  taste: is this the right problem, the most focused solution, and an experience
  users will understand and trust? Use when asked for a product review, founder
  review, CEO review, product/UX sanity check, scope challenge, or whether a
  feature is worth building.
---

# Product Review

Review the spec or plan from the product seat before implementation. Lead with
judgment, not process. Say what is strong, what feels unfocused or inelegant,
what should change, and why users will care.

This is the product counterpart to `plan-eng-review`:

| Lens | Owns |
|------|------|
| `plan-ceo-review` | Product thesis, scope, interaction quality, hierarchy, trust, and product risk |
| `plan-eng-review` | Architecture, data flow, tests, performance, and implementation risk |

Do not re-review engineering design here. Carry engineering-shaped concerns
into the hand-off.

## Core Behavior

- **Make the call.** The reviewer owns the review posture. Never ask the user to
  choose HOLD, EXPAND, REDUCE, or any other review mode. Never expose an
  internal scope posture as a workflow step.
- **Lead with the product verdict.** Start with whether the feature should exist,
  what its sharpest product idea is, and the most important change you
  recommend.
- **Findings are not questions.** State findings directly. Ask the user only
  when a real product decision must be made before scope can be considered
  locked.
- **Respect decisions without becoming deferential.** An approved spec and
  explicit Non-Goals are settled context, not immunity from critique. Challenge
  them when they weaken the product, explain the cost, then accept the user's
  decision.
- **Prefer subtraction.** Do not add speculative completeness. Recommend an
  addition only when its absence breaks the stated user outcome or trust.
- **Review the product, not the review process.** Do not narrate competing rules,
  scoring systems, or why a review posture was selected.

Internally, decide whether the product needs scope held, narrowed, or
occasionally expanded. Use that judgment to shape recommendations, but never
present those labels or ask the user to select one.

## Product Taste

Apply these instincts throughout the review:

1. **A sharp product thesis.** The feature should have one clear reason to
   exist. If two independently valuable products are bundled together, say so.
2. **Distinctive value over generic completeness.** Protect the interaction
   that makes Rollshot meaningfully better. Be skeptical of work that merely
   turns it into a generic image viewer or screenshot utility.
3. **Focus through subtraction.** Every surface, control, state, and mode must
   earn its place. A complete-looking interface can still be an unfocused
   product.
4. **Hierarchy as service.** The interface must make the primary user job
   obvious in the first ten seconds. Secondary controls should not compete with
   it.
5. **Trust is part of the product.** Saving, copying, dragging, revealing, and
   discarding must leave no ambiguity about what happened or whether the result
   is safe.
6. **Coherent behavior, intentional platform differences.** Linux and macOS may
   differ, but each difference needs a user-facing reason. Consistency is not
   sameness; unexplained divergence is still a defect.
7. **Polish the product's signature moment.** A small detail in the core flow
   often matters more than a broad set of secondary capabilities.
8. **Protect future optionality without building the future.** Flag one-way
   doors and likely UX debt. Do not add speculative abstractions or UI.

## Before Reviewing

### Locate and read the artifact

Use the file named by the user. If none is named, locate recent specs/plans and
ask only if there is genuine ambiguity:

```bash
ls -1t docs/superpowers/specs/*.md 2>/dev/null
ls -1t docs/superpowers/plans/*.md 2>/dev/null
git diff --name-only main...HEAD 2>/dev/null | grep -iE '(specs?|plans?)/.*\.md$'
```

Read the entire artifact. Note its Goals, Non-Goals, status, superseded design,
and unresolved decisions.

Per `AGENTS.md`, code is normally the source of truth. When the user explicitly
directs the review against a specific spec, treat that spec as live for the
review. Verify claims about current behavior against code using
code-review-graph tools before file search.

### Understand product context

Check already-shipping behavior that partially solves the job. For capture,
overlay, or result UI, inspect both active platform paths:

- Linux: native iced Wayland layer-shell overlay.
- macOS: iced overlay through `rollshot-app` with ScreenCaptureKit.
- Shared product logic: `rollshot-overlay-core`, `rollshot-iced-overlay`, and
  `rollshot-app`.

Use relevant reference products under `learn-projects/` when they can clarify a
product convention or expose a stronger interaction. Do not turn the review
into a feature-parity checklist.

## Review Workflow

### 1. Form the product thesis

Privately answer:

- What user job is this really solving?
- Why should Rollshot solve it?
- What is the feature's most distinctive or valuable moment?
- What happens if nothing ships?
- Is the spec solving the job directly, or a proxy?

If the premise is weak, say so immediately. If the premise is strong but the
solution obscures it, protect the premise and challenge the solution.

### 2. Give the opening verdict

Begin the review with a concise, opinionated synthesis:

```text
Product verdict: <should exist / needs reframing / should not ship as proposed>

The sharp idea: <the part that creates real user value or differentiation>
The problem: <the main product weakness>
Recommendation: <the product shape you would ship>
```

Do not mention modes, completeness scores, internal rule conflicts, or review
methodology.

### 3. Present prioritized findings

Review every area below, but report only material findings. Order findings by
user impact, not by section number. A finding should be a direct product
judgment, not a neutral observation.

For each finding include:

```text
Finding: <clear claim>
Why it matters: <concrete user consequence>
Recommendation: <specific product decision>
Tradeoff: <what is lost or deferred>
```

#### Problem and differentiation

- Does the scope map cleanly to the core user job?
- What part feels distinctly Rollshot rather than generically complete?
- Is the signature interaction getting enough attention?
- Is the spec solving multiple independently shippable products at once?

#### Scope and simplicity

- Which in-scope items directly serve the outcome?
- Which items are generic completeness, speculative future-proofing, or
  "while we're here" work?
- What is the smallest coherent product, not merely the smallest code change?
- Would deferring an item sharpen the first release without breaking trust?

Do not reduce scope mechanically. Keep a costly capability when it is the
reason users will care.

#### Ten-second experience

- What does the user see first, second, and third?
- Is the primary action unmistakable?
- Are controls competing with the content or signature interaction?
- Are hidden gestures and platform conventions discoverable enough?
- Does the visual model feel focused, ordinary, or confused?

#### Trust and unhappy paths

- Can users tell whether the result is saved, unsaved, copied, dragged, or
  discarded?
- Can a captured result be lost?
- Are success and failure legible and recoverable?
- Does platform divergence create inconsistent expectations?
- What happens with first use, huge captures, rapid repeated captures, and
  interrupted actions?

Mark possible result loss or silent failure as a **critical product defect**.

#### Product risk and temporal depth

- What is most likely to make users quietly stop trusting or using it?
- Which interaction or naming decision becomes a one-way door?
- What obvious future step might this make awkward?
- Does the product feel more focused after this feature ships?

### 4. Ask only decision questions

After presenting the relevant findings, identify decisions that genuinely need
the user's authority. Do not ask a question merely because a finding exists.

Ask one decision at a time. Keep it concise:

```text
D<N> — <decision>
Recommendation: <choice and product reason>

A) <recommended choice> — <user benefit and honest cost>
B) <alternative> — <user benefit and honest cost>
C) Keep as specced — <when reasonable>
```

Do not ask the user which review posture to use. Do not force artificial
completeness scores. Do not require a decision on matters where the
recommendation is clearly compatible with already-approved scope.

Stop after each real decision question and wait for the answer. Carry accepted
decisions into the final synthesis.

### 5. Close the review

When decisions are resolved, provide:

#### Recommended product shape

Describe the coherent product that should ship in a short paragraph. Make clear
what the feature is fundamentally for and what deserves the most polish.

#### Scope decisions

| # | Item | Decision | Product reason |
|---|------|----------|----------------|
| 1 | <item> | KEEP / CHANGE / DEFER / ADD | <one line> |

Include only decisions discussed or materially challenged during the review.

#### Not in scope

List the spec's important Non-Goals plus anything deferred during review, each
with a short product rationale. Do not reproduce every minor exclusion.

#### Product risk flags

List unresolved trust and UX risks. State what the user experiences if each
ships unaddressed. Mark critical product defects clearly.

#### Hand-off to engineering

List architecture, test, or performance concerns for `plan-eng-review` without
resolving them from the product seat.

#### Completion summary

```text
Spec reviewed: <path>
Product verdict: <one line>
Sharpest value: <one line>
Primary change: <one line>
Critical product defects: <count>
Unresolved product decisions: <count>
```

If no decisions remain, state:

> Product direction is locked. Run `/plan-eng-review` to lock the engineering
> plan, then `superpowers:writing-plans`.

If decisions remain, list them plainly. Never silently choose on the user's
behalf.

## Guardrails

- Do not edit code during a product review.
- Do not edit an approved spec unless the user explicitly asks.
- Do not praise breadth for its own sake.
- Do not reduce a feature merely because it is technically difficult.
- Do not mistake a detailed spec for a good product.
- Do not bury the strongest opinion below process narration.
- Do not make every observation a blocking decision.
- Use literal UTF-8 for non-ASCII text.

