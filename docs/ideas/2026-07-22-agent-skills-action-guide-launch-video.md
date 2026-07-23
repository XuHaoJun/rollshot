# Rollshot Idea: Agent Skills and Action Guide Launch Videos

**Date:** 2026-07-22  
**Status:** Deferred idea; not an implementation spec  
**Area:** Agent, Action Guide, video export  
**Reference projects:** `learn-projects/brag`, `learn-projects/hyperframes`,
`learn-projects/pi`, `learn-projects/oh-my-pi`, `learn-projects/codex`

## Summary

Rollshot may eventually turn an Action Guide recording of a newly built feature
into a short, polished launch teaser. The strongest version combines two kinds
of evidence:

- Action Guide supplies the real interaction path, timestamps, reviewed steps,
  keyframes, captions, and annotations.
- A Rollshot agent may optionally inspect the user's project to understand the
  feature's purpose, official terminology, brand, and strongest product claim.

The agent would then choose the few moments worth showing and propose a short
story. The product promise is:

> Record the feature once; Rollshot finds the 15–25 seconds worth sharing.

This idea should remain deferred until Rollshot's agent system supports a safe,
provider-neutral skills model. It should not trigger an immediate attempt to
internalize either brag or Hyperframes.

## Why defer it

The desired experience looks small, but the reference implementations reveal
several independent systems:

- **brag** is a creative director workflow: inspect the product, choose an
  angle, write a hook and storyboard, select audio, hand off a composition,
  validate it, and write share copy.
- **Hyperframes** is a general video authoring and rendering platform: an HTML
  composition contract, seek-safe animation runtimes, browser frame capture,
  FFmpeg encoding and audio mixing, validation, preview, Studio editing,
  registries, media workflows, and local or cloud rendering.

Internalizing both would turn Rollshot into three products at once: capture and
Action Guide, an agentic creative director, and a general motion-graphics/video
platform. That is not a credible MVP boundary.

The prerequisite is a trustworthy agent foundation that can load specialized
workflows without baking every workflow into the core application.

## Agent skills direction

Rollshot should investigate a skills capability before designing the launch
video feature in detail. A skill is a versioned, inspectable workflow package
that teaches the agent how to perform a bounded job using registered Rollshot
tools. Skills should extend the agent without changing its provider-neutral
model facade.

The eventual skills system should preserve these properties:

1. **Provider neutrality.** Skills describe workflow and tool use; they do not
   depend on one model vendor's conversation or tool-call types.
2. **Progressive loading.** The agent discovers skill metadata first and loads
   full instructions or references only when selected.
3. **Typed capabilities.** Skills may call only registered, availability-aware
   tools with explicit input and output contracts.
4. **Bounded execution.** Existing run budgets, cancellation, terminal states,
   and privacy rules continue to apply to skill-driven work.
5. **Scoped repository access.** Reading a project is optional, explicitly
   authorized, constrained to the selected workspace, and visible to the user.
6. **Reviewable artifacts.** Storyboards, edit decisions, and proposed changes
   are inspectable before rendering or mutation.
7. **Provenance.** Outputs record the skill version, source assets, tool results,
   and accepted user decisions without leaking provider internals.
8. **No arbitrary authority.** Installing or invoking a skill does not silently
   grant filesystem, network, process, or publishing permissions.

Before choosing an architecture, study the local reference implementations:

- `learn-projects/pi` for a small agent loop and extension model.
- `learn-projects/oh-my-pi` for a broader ecosystem built around pi.
- `learn-projects/codex` for a production Rust agent architecture, sandboxing,
  approvals, tool execution, and lifecycle management.

The research should compare concepts and boundaries, not begin with a mandate
to port an entire framework. In particular, a Rust rewrite of pi and a fork of
Codex are separate, high-cost directions that require their own product and
engineering review.

## Eventual launch-video product shape

If the skills foundation proves sound, the launch-video workflow should be a
Rollshot skill whose primary evidence is the reviewed Action Guide. Repository
inspection is an optional enrichment, not a requirement.

The skill should propose a constrained, reviewable edit decision list rather
than emit and execute arbitrary HTML or JavaScript:

```text
LaunchTeaserPlan
├── hook
├── format and duration
├── shots[]
│   ├── source time range
│   ├── crop or focus region
│   ├── speed
│   ├── caption
│   └── transition intent
├── result shot
├── outro
└── audio direction
```

The user reviews this plan, edits copy or shot selection, and explicitly starts
rendering. Agent confidence must never substitute for review of captured
content that may contain private information.

## Recommended validation sequence

### 1. Complete the agent and skills foundation

Define discovery, loading, capability declarations, tool permissions, budgets,
cancellation, artifact review, and provenance. Validate these using a smaller
skill before making video generation the test case.

### 2. Run a concierge experiment

Use external brag and Hyperframes as research tools, not product dependencies.
Manually translate a few real Action Guides into launch videos and determine:

- whether Action Guide produces better shot selection than code inspection
  alone;
- whether users actually share the resulting videos;
- how much manual correction the hook, copy, crops, and timing require;
- which motion and audio treatments materially improve the result.

### 3. Consider a narrow Rollshot MVP

If the experiment validates demand, build only the smallest coherent teaser:

- landscape output;
- 15–25 seconds;
- three to five clips selected around reviewed Action Guide steps;
- trim, crop, focus/pan, text overlays, simple transitions, and MP4 export;
- one polished visual treatment;
- optional fixed music bed;
- editable copy and shot order before render.

The agent produces `LaunchTeaserPlan`; a deterministic Rollshot renderer
executes the supported operations. This preserves the product's distinctive
advantage—understanding the real recorded workflow—without building a general
video platform.

### 4. Re-evaluate Hyperframes integration

Only after the narrow workflow succeeds, decide whether users need:

- more native Rollshot templates;
- an optional Hyperframes export/adapter for advanced composition;
- or a broader authoring surface.

This decision should be based on observed editing needs, not on Hyperframes'
available feature set.

## Explicit non-goals for the first launch-video MVP

- No internal Hyperframes fork or Rust rewrite.
- No general HTML/CSS/JavaScript composition runtime.
- No arbitrary agent-generated code execution in the product path.
- No full nonlinear video editor or motion-graphics Studio.
- No template marketplace, cloud rendering, or multi-format campaign suite.
- No seven-tone creative system, voiceover generation, beat-reactive visuals,
  or broad media catalog before the basic teaser proves valuable.
- No requirement that every user grant repository access.

## Open questions for later discovery

1. What is the smallest useful skill package and manifest for Rollshot?
2. Should skills be data/instructions only, WebAssembly modules, signed native
   plugins, or a deliberately smaller combination?
3. Which existing Rollshot tools are safe to expose to skills without adding
   general shell access?
4. How should users inspect skill provenance, requested capabilities, and the
   exact project files read by an agent?
5. Does a fixed Rollshot renderer meet the quality bar, or is an external
   Hyperframes adapter necessary for the first convincing result?
6. Must the original recording be retained so the renderer can extract short
   dynamic clips around Action Guide timestamps?
7. What privacy review is required before code-derived claims and captured UI
   are combined into a shareable artifact?

## Restart condition

Resume this idea only when Rollshot has a reviewed direction for agent skills
and can demonstrate one smaller skill end to end with bounded tools,
cancellation, explicit permissions, and reviewable output. At that point,
repeat product discovery using current code and current upstream reference
projects; this note is a snapshot, not the source of truth.
