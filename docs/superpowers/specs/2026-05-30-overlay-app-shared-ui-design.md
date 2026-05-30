# Shared Overlay UI/UX (webview ⇄ native) Design Spec

Status: design spec. Date: 2026-05-30.
Related: `docs/linux-wayland-layer-shell-roadmap.md`,
`docs/superpowers/specs/2026-05-30-native-linux-capture-overlay-design.md`,
issue `docs/issues/2026-05-30-overlay-capture-miss-recovery-ux.md`.

All file:line references may drift; verify against code.

## Goal

Make the two platform-specific capture overlays present consistent behavior and
UI/UX, **durably** (resistant to drift), without merging their render stacks:

- `rollshot-app` — Tauri **webview** overlay (macOS / Windows / current Linux).
- `rollshot-overlay` — native **iced layer-shell** overlay (KDE 6 Wayland).

Immediate divergences to fix:

1. **Live stitching preview.** App uses whole-image downscale-to-max-edge
   (`session.rs::stitch_preview_png` → `encode_preview_image_png`); overlay uses
   fixed-width grow-then-follow (`driver.rs::preview_viewport_handle`). →
   unify on the overlay's behavior.
2. **Crop selection visuals.** Overlay draws a plain white border, no mask; app
   draws a sky-blue border + dark translucent mask (`SelectionLayer` + App.css).
   → unify on the app's look.

## Why two renderers (not merged)

The webview overlay works on macOS/Windows today; the native layer-shell overlay
exists because Tauri's fullscreen overlay is unreliable on KDE 6 Wayland (the
whole premise of the layer-shell roadmap). Collapsing to a single iced overlay
everywhere is the long-term **north star**, but requires proving iced/winit as a
transparent, above-fullscreen, click-through, capture-excludable overlay on
macOS + Windows — its own spike plus a large migration. **Out of scope here.**

## Architecture: shared sources of truth, thin per-stack renderers

Extract everything platform-independent into shared Rust; each stack renders from
it. The render code (HTML/CSS vs iced/wgpu) stays per-stack and thin.

New crate **`rollshot-overlay-core`** — pure Rust, depends on `image` only.
**MUST NOT** depend on iced, Tauri, a webview, `rollshot-overlay`, or
`rollshot-app`. Both `rollshot-overlay` and `rollshot-app/src-tauri` depend on
it. `rollshot-core` stays stitching-only (no UI concerns).

Three pillars of consistency:

| Concern | Single source of truth (`rollshot-overlay-core`) | Thin per-stack part |
|---|---|---|
| Live preview image | `preview::preview_viewport(image, width, max_height) -> RgbaImage` + `PREVIEW_WIDTH` / `PREVIEW_MAX_HEIGHT` | overlay: wrap in iced `Handle`; app: PNG-encode |
| Crop visual tokens | `tokens` consts (border color/width, mask opacity, dim, guides, halo) | overlay: iced `Color`; app: CSS `:root var()` |
| Crop + placement geometry | canonical algorithm + exported golden fixtures (JSON) | overlay: Rust; app: TS (interactive — can't round-trip), validated against fixtures |

Pillar 3 is the durable mechanism for behavior consistency (the webview's crop
drag runs per mouse-move in TS and cannot round-trip to Rust, so the TS impl
stays and is kept honest by shared fixtures). The immediate work delivers
**pillars 1 + 2** and seeds the crate; pillar-3 geometry fixtures are a
follow-up.

## Decisions

- **Shared home = new crate `rollshot-overlay-core`** (not a `rollshot-core`
  module): keeps the stitching core focused and gives a durable home for
  preview + tokens + future geometry/fixtures.
- **Token sync = manual duplication + a sync test** (chosen over codegen; ~6
  low-churn values). Canonical values live as Rust consts in
  `rollshot-overlay-core::tokens`; App.css mirrors them in a `:root` block via
  `var()`. A sync test in `rollshot-app/src-tauri/src/css_token_sync.rs`
  `include_str!`s `../../src/App.css` and asserts the canonical `--crop-*:
  value;` strings are present — fails if either side changes without the other.
- **Crop cursor guides:** ported to the overlay too (for parity); low-cost,
  droppable if it complicates the canvas.
- **App preview placement-box parity:** deferred to the geometry pillar; the
  immediate work unifies the preview *image content*, not the placement box
  (the app's `overlay/placement.ts` already positions it).
- **Execution shape:** the plan keeps each task independently buildable and
  uses red/green tests before behavior changes. The new crate is scaffolded
  without missing module exports, then `preview` and `tokens` are added in
  separate TDD tasks.

## Canonical token values (from current App.css)

- crop border: `#38bdf8`, width `2px`
- crop border halo: `rgba(255,255,255,0.72)`, `1px`
- mask outside crop (rect present): `rgba(0,0,0,0.24)`
- dim before any rect: `rgba(0,0,0,0.22)`
- cursor guides: `rgba(147,197,253,0.48)`, `1px`
- preview viewport: width `280`, max height `480`

## Scope

**In:** create `rollshot-overlay-core`; pillar 1 (preview) + pillar 2 (crop
tokens); wire both stacks; the token sync test.

**Out / follow-up:** geometry fixtures (pillar 3); collapsing to one renderer
(north star); the live-stitch capture-miss recovery UX (separate issue);
publishing `rollshot-overlay-core` as a crate (internal workspace library,
`publish = false`).

**Unchanged:** stitching core, capture, save handoff; the two render stacks stay
separate.

**Historical note:** this spec is design intent, not an implementation status
ledger. Completion notes belong in the implementation plan / PR, not as
retroactive edits to this snapshot.

## Verification

- `rollshot-overlay-core`: unit tests for `preview_viewport` (grows below the
  cap; bottom-anchored; capped for tall canvas) — moved/adapted from the current
  overlay driver tests.
- `rollshot-overlay-core`: unit tests for token CSS serialization.
- `rollshot-overlay`: existing tests pass; preview wrapper uses the shared
  viewport; crop mask band clipping and token color conversion are unit-tested;
  preview + crop visuals build; clippy + fmt clean.
- `rollshot-app`: `stitch_preview_png` has a session test proving it uses the
  shared viewport and ignores the old `max_edge` downscale; existing session
  tests pass; frontend typecheck / test / build.
- Token sync test green and failure message names the drifted `--crop-*` string.
- Manual: overlay crop shows the dark mask + sky-blue border; app live preview
  grows-then-follows and matches the overlay.

## Open questions

- None blocking. (Crosshair-guide port and preview placement-box parity resolved
  above as parity-include / deferred.)
