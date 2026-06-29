# Smart Redaction Agent Phase B1 Region Inspection

**Date:** 2026-06-27
**Status:** Draft for user review

## Goal

Give the Smart Redaction authoring agent a truthful, bounded way to inspect the
current screenshot before it writes JavaScript.

Phase A made validation, dry-run, and prepared canonical region features work.
Phase B1 exposes those prepared observations as product tools so the model can
reason from image dimensions and coarse visual features instead of guessing from
the prompt and waiting for dry-run failures.

## Success Criteria

- Product Smart Redaction runs expose a real `inspect_image_context` tool.
- Product Smart Redaction runs expose a real `inspect_region_features` tool for
  canonical prepared regions only.
- `inspect_region_features` never accepts arbitrary crop coordinates or raw
  pixels.
- Oversized or unprepared canonical regions return structured unavailable
  results, not fake observations and not runtime panics.
- OCR, layout, and template-match inspection remain unavailable as separate
  tools in product runs, but their availability is visible in
  `inspect_image_context`.
- Tool schemas are valid JSON object schemas and tool results stay within the
  existing result-byte budget.
- Existing Phase A dry-run and existing-preset region-feature behavior remains
  unchanged.

## Scope

### In Scope

- Add a named canonical region catalog for:
  - `full`
  - `top_strip`
  - `left_strip`
  - `right_strip`
  - `bottom_strip`
- Reuse the same catalog for workbench region-feature preparation and inspection
  tool answers.
- Add product-visible agent tools:
  - `inspect_image_context`
  - `inspect_region_features`
- Register these tools in the Smart Redaction workbench authoring registry.
- Add tests in `rollshot-agent` and `rollshot-app` for schemas, successful
  inspection, skipped regions, and registry contents.

### Out of Scope

- OCR model enablement or OCR text exposure.
- Layout inspection.
- Template handle persistence or template-match inspection.
- Arbitrary region crop inspection.
- Raw screenshot pixels, image thumbnails, or attachment bytes in tool results.
- Source patch/edit tools.
- Workbench UI redesign.

## Design

### 1. Named Canonical Region Catalog

Phase A currently prepares deterministic region-feature queries, but the helper
returns only queries. B1 should introduce a small named catalog that records both
prepared and skipped regions.

Each catalog entry contains:

- canonical name,
- bounds in image coordinates,
- matching `RegionFeaturesQuery` when the region can be prepared,
- skipped/unavailable reason when it cannot be prepared.

The preparation path should iterate the catalog and call
`RealAutomationHost::prepare_region_features` only for entries with a query.
This keeps dry-run behavior aligned with inspection behavior: if the tool says
`top_strip` is available, JavaScript using the same canonical bounds should hit
the prepared host cache.

The catalog remains workbench-owned for B1. It is product authoring context, not
a new automation language concept.

### 2. `inspect_image_context`

Add an agent inspection tool that returns current authoring and image context in
one bounded response.

Result shape:

- `image`: width, height, and payload mode.
- `source`: current generation, source byte count, evidence count.
- `regions`: one entry per canonical region with name, bounds, status, and
  unavailable reason when applicable.
- `capabilities`: availability summary for `region_features`, `ocr`, `layout`,
  and `template_match`.

`region_features` should report `available` when at least one canonical region
is prepared, `partial` when some canonical regions are skipped, and
`unavailable` when none are usable. OCR and layout report unavailable in default
product builds. Template match reports unavailable unless real
`AutomationInput.capability_handles` are present; B1 does not create those
handles.

This tool complements the existing `inspect_context_summary`. Product prompts
should steer the model toward `inspect_image_context`, but B1 does not need to
remove the Phase A summary tool.

### 3. `inspect_region_features`

Replace the product stub with a real bounded inspection tool backed by the
prepared `AutomationHost`.

Arguments:

- `region`: one canonical region name: `full`, `top_strip`, `left_strip`,
  `right_strip`, or `bottom_strip`.

Result shape:

- `region`: canonical name.
- `status`: `available` or `unavailable`.
- `bounds`: canonical bounds when known.
- `features`: at most one feature summary when available.
- `unavailable_reason`: structured code when unavailable.

Feature summaries include only:

- measured bounds,
- dominant RGBA,
- edge density.

The tool must not accept arbitrary rectangles. It should call
`AutomationHost::region_features` with the catalog query and convert capability
errors into structured unavailable responses. It should not synthesize data if
the host cache is missing.

### 4. Registry and Prompt Contract

The Smart Redaction product registry should expose:

- `replace_source`
- `validate_source`
- `submit_for_review`
- `request_user_input`
- `inspect_context_summary`
- `inspect_image_context`
- `inspect_region_features`
- `dry_run`

`inspect_ocr` and `inspect_layout` remain out of the product registry until
later phases return real data. The authoring guide should instruct the model to
call `inspect_image_context` before writing or replacing source, then use
`inspect_region_features` when coarse visual evidence is needed.

### 5. Error Handling

- Unknown canonical region names are recoverable argument errors.
- Skipped catalog entries return `status: "unavailable"` with a stable reason
  such as `area_limit_exceeded`.
- Host capability errors return `status: "unavailable"` with the host code, such
  as `vision_index_unavailable` or `limit_exceeded`.
- Tool result-byte budget failures remain hard `ToolRegistry` errors.
- Empty or invalid screenshots continue to fail during vision context
  preparation before the agent run starts.

## Testing

### Agent Tool Tests

- `inspect_image_context` schema is an object schema.
- `inspect_image_context` returns image dimensions, authoring generation, region
  availability, and capability availability.
- `inspect_region_features` schema is an object schema.
- `inspect_region_features` rejects unknown canonical names as a recoverable
  argument error.
- `inspect_region_features` returns bounded feature summaries from a prepared or
  fake host.
- `inspect_region_features` converts host errors into structured unavailable
  results.

### Workbench Tests

- The canonical region catalog matches the Phase A prompt top-strip dimensions.
- Oversized images keep skipped regions in the catalog with reasons.
- Vision preparation prepares only catalog entries with queries.
- Product registry includes `inspect_image_context` and
  `inspect_region_features`.
- Product registry still excludes `inspect_ocr` and `inspect_layout`.
- A synthetic image can prepare context, inspect `top_strip`, and dry-run a
  region-feature preset using the same canonical query.

## Verification Commands

Use narrow checks first:

```bash
rtk cargo test -p rollshot-agent inspect_region_features
rtk cargo test -p rollshot-agent inspect_image_context
rtk cargo test -p rollshot-app result_workspace::workbench
```

Before claiming Phase B1 complete:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo fmt --check
```

Run wider workspace tests if the implementation changes shared automation or
vision contracts.

## Risks

- Exposing arbitrary crop inspection would invite expensive or privacy-sensitive
  tool usage. B1 avoids this by allowing only canonical regions.
- If preparation and inspection compute regions separately, small formula drift
  could make tool evidence disagree with dry-run behavior. The named catalog
  must be shared by both paths.
- Region features are coarse. They help with geometry and visual density, but
  they cannot identify text. OCR-dependent authoring quality remains a B2
  problem.
- Adding too many inspection details can increase prompt/tool-result size.
  Keep each result bounded and avoid raw image data.

## Deferred Work

- OCR-backed `inspect_ocr` with explicit text privacy policy.
- Layout inspection.
- Template-match handle visibility and lifecycle.
- Source read/patch editing ergonomics.
- Fixture-level model evaluation for whether B1 improves preset authoring.
